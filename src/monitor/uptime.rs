//! Uptime checks: is each app's web UI actually answering?
//!
//! A container can be "running" while its app is wedged. Every container
//! with a web UI gets an automatic HTTP check (customizable per container),
//! plus any custom checks from Settings. A check goes "down" after several
//! consecutive failures and notifies on the way down and back up.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{StreamExt, stream};
use parking_lot::RwLock;
use rusqlite::params;
use serde::Serialize;

use crate::app::App;
use crate::db::Db;
use crate::docker::{Entry, reach};
use crate::error::Result;
use crate::events::{Event, EventBus};
use crate::notify::Notification;
use crate::settings::{Severity, parse_status_ranges};

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub id: String,
    pub name: String,
    pub url: String,
    pub identity: Option<String>,
    #[serde(skip)]
    pub expect: Vec<(u16, u16)>,
    pub keyword: String,
    pub custom: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProbeResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub status: Option<u16>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Beat {
    pub ts: i64,
    pub ok: bool,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UptimeStatus {
    pub id: String,
    pub name: String,
    pub url: String,
    pub identity: Option<String>,
    /// "up", "down", "pending" or "paused" (container stopped).
    pub state: String,
    pub since: i64,
    pub last_check: Option<i64>,
    pub latency_ms: Option<u64>,
    pub status_code: Option<u16>,
    pub error: Option<String>,
    pub uptime_24h: Option<f64>,
    pub uptime_7d: Option<f64>,
    pub beats: Vec<Beat>,
    pub custom: bool,
}

#[derive(Clone)]
pub struct UptimeService {
    inner: Arc<Inner>,
}

struct Inner {
    bus: EventBus,
    states: RwLock<HashMap<String, (UptimeStatus, u32)>>,
    http: reqwest::Client,
    /// Set while a round of checks runs (the loop and "check now" share it).
    running: std::sync::atomic::AtomicBool,
}

impl UptimeService {
    pub fn new(bus: EventBus) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(crate::USER_AGENT)
            .tls_danger_accept_invalid_certs(true)
            .redirect(reqwest::redirect::Policy::limited(3))
            .pool_max_idle_per_host(1)
            .no_proxy()
            .build()
            .expect("HTTP client");
        Self {
            inner: Arc::new(Inner {
                bus,
                states: RwLock::new(HashMap::new()),
                http,
                running: std::sync::atomic::AtomicBool::new(false),
            }),
        }
    }

    pub fn all(&self) -> Vec<UptimeStatus> {
        let mut v: Vec<UptimeStatus> = self.inner.states.read().values().map(|(s, _)| s.clone()).collect();
        v.sort_by_key(|a| a.name.to_lowercase());
        v
    }

    pub fn get(&self, id: &str) -> Option<UptimeStatus> {
        self.inner.states.read().get(id).map(|(s, _)| s.clone())
    }

    pub fn by_identity(&self) -> HashMap<String, UptimeStatus> {
        self.inner
            .states
            .read()
            .values()
            .filter_map(|(s, _)| s.identity.clone().map(|i| (i, s.clone())))
            .collect()
    }
}

/// The automatic (or customized) check for a container, if it has a web UI.
pub fn check_for(app: &App, entry: &Entry) -> Option<Check> {
    let settings = app.settings.get();
    let prefs = app.prefs.get(&entry.summary.identity);
    if prefs.uptime.enabled == Some(false) {
        return None;
    }
    let expect_spec = if prefs.uptime.expect_status.is_empty() { settings.uptime.expect_status.clone() } else { prefs.uptime.expect_status.clone() };
    let expect = parse_status_ranges(&expect_spec).unwrap_or_default();
    let name = crate::updates::check::display_name(app, entry);
    let url = if !prefs.uptime.url.is_empty() {
        prefs.uptime.url.clone()
    } else {
        if !settings.uptime.auto_checks && prefs.uptime.enabled != Some(true) {
            return None;
        }
        let app_info = entry.summary.app.as_deref().and_then(crate::docker::apps::by_slug);
        // Only containers that serve a web UI.
        let port = reach::app_port(entry)?;
        let has_web = app_info.is_some_and(|a| a.web_port.is_some()) || !entry.summary.links.is_empty();
        if !has_web {
            return None;
        }
        let https = app_info.is_some_and(|a| a.web_https);
        let base = reach::base_url(app, entry, port, https)?;
        let path = app_info.map(|a| a.health_path.unwrap_or(a.web_path)).unwrap_or("/");
        format!("{base}{path}")
    };
    Some(Check {
        id: entry.summary.identity.clone(),
        name,
        url,
        identity: Some(entry.summary.identity.clone()),
        expect,
        keyword: prefs.uptime.keyword.clone(),
        custom: false,
    })
}

/// Runs one probe.
pub async fn probe(app: &App, check: &Check) -> ProbeResult {
    let timeout = Duration::from_secs(app.settings.get().uptime.timeout_secs.clamp(1, 120) as u64);
    let start = Instant::now();
    if let Some(addr) = check.url.strip_prefix("tcp://") {
        let res = tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr)).await;
        let latency_ms = start.elapsed().as_millis() as u64;
        return match res {
            Ok(Ok(_)) => ProbeResult { ok: true, latency_ms, status: None, error: None },
            Ok(Err(e)) => ProbeResult { ok: false, latency_ms, status: None, error: Some(e.to_string()) },
            Err(_) => ProbeResult { ok: false, latency_ms, status: None, error: Some("timed out".into()) },
        };
    }
    let res = app.uptime.inner.http.get(&check.url).timeout(timeout).send().await;
    let latency_ms = start.elapsed().as_millis() as u64;
    match res {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let status_ok = if check.expect.is_empty() {
                status < 500
            } else {
                check.expect.iter().any(|(a, b)| status >= *a && status <= *b)
            };
            if !status_ok {
                return ProbeResult { ok: false, latency_ms, status: Some(status), error: Some(format!("HTTP {status}")) };
            }
            if !check.keyword.is_empty() {
                let body = resp.text().await.unwrap_or_default();
                if !body.contains(&check.keyword) {
                    return ProbeResult { ok: false, latency_ms, status: Some(status), error: Some(format!("'{}' not found in the page", check.keyword)) };
                }
            }
            ProbeResult { ok: true, latency_ms, status: Some(status), error: None }
        }
        Err(e) => ProbeResult {
            ok: false,
            latency_ms,
            status: None,
            error: Some(if e.is_timeout() { "timed out".into() } else { crate::util::describe_reqwest_error(&e) }),
        },
    }
}

fn all_checks(app: &App) -> Vec<(Check, bool)> {
    let settings = app.settings.get();
    let mut out = Vec::new();
    for e in app.docker.entries() {
        if let Some(c) = check_for(app, &e) {
            out.push((c, e.summary.is_running()));
        }
    }
    for c in settings.uptime.custom.iter().filter(|c| c.enabled) {
        out.push((
            Check {
                id: format!("custom:{}", c.id),
                name: c.name.clone(),
                url: c.url.clone(),
                identity: None,
                expect: parse_status_ranges(&c.expect_status).unwrap_or_default(),
                keyword: c.keyword.clone(),
                custom: true,
            },
            true,
        ));
    }
    out
}

async fn record(db: &Db, check_id: &str, ts: i64, r: &ProbeResult) -> Result<()> {
    let (id, ok, lat, st, err) = (check_id.to_string(), r.ok, r.latency_ms as i64, r.status.map(|s| s as i64), r.error.clone());
    db.call(move |c| {
        let hour = ts - ts % 3600;
        c.execute(
            "INSERT OR REPLACE INTO uptime_results (check_id, ts, ok, latency_ms, status, error) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, ts, ok as i64, lat, st, err],
        )?;
        c.execute(
            "INSERT INTO uptime_hourly (check_id, hour, ok_count, fail_count, latency_sum) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(check_id, hour) DO UPDATE SET ok_count = ok_count + excluded.ok_count,
               fail_count = fail_count + excluded.fail_count, latency_sum = latency_sum + excluded.latency_sum",
            params![id, hour, ok as i64, (!ok) as i64, lat],
        )?;
        Ok(())
    })
    .await
}

async fn uptime_pct(db: &Db, check_id: &str, since: i64) -> Option<f64> {
    let id = check_id.to_string();
    db.call(move |c| {
        let (ok, fail): (i64, i64) = c.query_row(
            "SELECT COALESCE(SUM(ok_count), 0), COALESCE(SUM(fail_count), 0) FROM uptime_hourly WHERE check_id = ?1 AND hour >= ?2",
            params![id, since],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(if ok + fail == 0 { None } else { Some((ok as f64 / (ok + fail) as f64 * 1000.0).round() / 10.0) })
    })
    .await
    .ok()
    .flatten()
}

/// Recent beats for a check (for the bar chart), oldest first.
pub async fn beats(app: &App, check_id: &str, since: i64) -> Result<Vec<Beat>> {
    let id = check_id.to_string();
    app.db
        .call(move |c| {
            let mut stmt = c.prepare("SELECT ts, ok, latency_ms FROM uptime_results WHERE check_id = ?1 AND ts >= ?2 ORDER BY ts")?;
            let rows = stmt.query_map(params![id, since], |r| {
                Ok(Beat { ts: r.get(0)?, ok: r.get::<_, i64>(1)? != 0, latency_ms: r.get::<_, Option<i64>>(2)?.unwrap_or(0) as u64 })
            })?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
        .await
}

/// Runs every check once now ("Check now"); returns false when a round is
/// already in progress.
pub async fn run_now(app: &App) -> bool {
    use std::sync::atomic::Ordering;
    if app.uptime.inner.running.swap(true, Ordering::AcqRel) {
        return false;
    }
    run_once(app).await;
    app.uptime.inner.running.store(false, Ordering::Release);
    true
}

/// Probes a check definition without saving anything (the "Test" button).
pub async fn test(app: &App, url: &str, expect_status: &str, keyword: &str) -> Result<ProbeResult> {
    let valid = url.starts_with("http://") || url.starts_with("https://") || url.starts_with("tcp://");
    if !valid {
        return Err(crate::error::AppError::bad_request("the URL must start with http://, https:// or tcp://"));
    }
    let expect = parse_status_ranges(expect_status).map_err(crate::error::AppError::BadRequest)?;
    let check = Check {
        id: "test".into(),
        name: "test".into(),
        url: url.to_string(),
        identity: None,
        expect,
        keyword: keyword.to_string(),
        custom: true,
    };
    Ok(probe(app, &check).await)
}

async fn run_once(app: &App) {
    let settings = app.settings.get();
    let threshold = settings.uptime.failures_before_down.max(1);
    let checks = all_checks(app);
    let live: std::collections::HashSet<String> = checks.iter().map(|(c, _)| c.id.clone()).collect();
    app.uptime.inner.states.write().retain(|k, _| live.contains(k));
    let results: Vec<(Check, bool, Option<ProbeResult>)> = stream::iter(checks)
        .map(|(c, running)| {
            let app = app.clone();
            async move {
                if running {
                    let r = probe(&app, &c).await;
                    (c, running, Some(r))
                } else {
                    (c, running, None)
                }
            }
        })
        .buffer_unordered(8)
        .collect()
        .await;
    let now = crate::util::now();
    for (check, _running, result) in results {
        let prev = app.uptime.inner.states.read().get(&check.id).cloned();
        let (mut status, mut fails) = prev.unwrap_or_else(|| {
            (
                UptimeStatus {
                    id: check.id.clone(),
                    name: check.name.clone(),
                    url: crate::util::redact_url(&check.url),
                    identity: check.identity.clone(),
                    state: "pending".into(),
                    since: now,
                    last_check: None,
                    latency_ms: None,
                    status_code: None,
                    error: None,
                    uptime_24h: None,
                    uptime_7d: None,
                    beats: Vec::new(),
                    custom: check.custom,
                },
                0,
            )
        });
        status.name = check.name.clone();
        status.url = crate::util::redact_url(&check.url);
        let old_state = status.state.clone();
        match &result {
            None => {
                if status.state != "paused" {
                    status.state = "paused".into();
                    status.since = now;
                }
                fails = 0;
            }
            Some(r) => {
                status.last_check = Some(now);
                status.latency_ms = Some(r.latency_ms);
                status.status_code = r.status;
                status.error = r.error.clone();
                status.beats.push(Beat { ts: now, ok: r.ok, latency_ms: r.latency_ms });
                if status.beats.len() > 60 {
                    let drop = status.beats.len() - 60;
                    status.beats.drain(..drop);
                }
                if r.ok {
                    fails = 0;
                    if status.state != "up" {
                        status.state = "up".into();
                        status.since = now;
                    }
                } else {
                    fails += 1;
                    if fails >= threshold && status.state != "down" {
                        status.state = "down".into();
                        status.since = now;
                    }
                }
                if let Err(err) = record(&app.db, &check.id, now, r).await {
                    tracing::debug!(%err, "failed to record uptime result");
                }
            }
        }
        status.uptime_24h = uptime_pct(&app.db, &check.id, now - 86400).await;
        status.uptime_7d = uptime_pct(&app.db, &check.id, now - 7 * 86400).await;
        let changed_state = old_state != status.state;
        if changed_state {
            let identity = check.identity.clone().unwrap_or_default();
            match (old_state.as_str(), status.state.as_str()) {
                (_, "down") => app.notifier.send(
                    Notification::new("uptime.down", Severity::Error, format!("{} is not answering", check.name), status.error.clone().unwrap_or_default())
                        .target(&identity, &check.name)
                        .field("URL", crate::util::redact_url(&check.url)),
                ),
                ("down", "up") => app.notifier.send(
                    Notification::new("uptime.up", Severity::Success, format!("{} is answering again", check.name), format!("Responded in {} ms", status.latency_ms.unwrap_or(0)))
                        .target(&identity, &check.name),
                ),
                _ => {}
            }
            if status.state == "down" || old_state == "down" {
                let kind: &'static str = if status.state == "down" { "uptime.down" } else { "uptime.up" };
                let sev = if status.state == "down" { Severity::Error } else { Severity::Success };
                let mut entry = crate::activity::NewActivity::new(kind, sev, format!("{}: web UI {}", check.name, if status.state == "down" { "not answering" } else { "answering again" }));
                if let Some(id) = &check.identity {
                    entry = entry.target(id, &check.name);
                }
                app.note(entry).await;
            }
        }
        let arc = Arc::new(status.clone());
        app.uptime.inner.states.write().insert(check.id.clone(), (status, fails));
        app.uptime.inner.bus.publish(Event::Uptime(arc));
    }
}

/// Deletes old raw results.
pub async fn prune(app: &App) -> Result<()> {
    let days = app.settings.get().uptime.retention_days.max(1) as i64;
    let cutoff = crate::util::now() - days * 86400;
    let raw_cutoff = crate::util::now() - 7 * 86400;
    app.db
        .call(move |c| {
            c.execute("DELETE FROM uptime_results WHERE ts < ?1", [raw_cutoff.max(cutoff)])?;
            c.execute("DELETE FROM uptime_hourly WHERE hour < ?1", [cutoff])?;
            Ok(())
        })
        .await
}

pub fn start(app: App) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            let settings = app.settings.get();
            if settings.uptime.enabled {
                run_now(&app).await;
            }
            let secs = settings.uptime.interval_secs.clamp(10, 3600) as u64;
            tokio::time::sleep(Duration::from_secs(secs)).await;
        }
    });
}
