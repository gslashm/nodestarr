//! Alert engine.
//!
//! Threshold rules are evaluated every 30 seconds and must hold for the
//! rule's duration before firing (and resolve once the condition clears).
//! Crashes and OOM kills are taken from Docker's event stream as they happen.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use rusqlite::params;
use serde::Serialize;

use crate::activity::NewActivity;
use crate::app::App;
use crate::docker::Lifecycle;
use crate::error::Result;
use crate::events::Event;
use crate::notify::Notification;
use crate::settings::{AlertKind, AlertRule, Severity};

type Key = (String, String);

#[derive(Default)]
pub struct AlertState {
    pending: HashMap<Key, i64>,
    firing: HashMap<Key, (i64, i64)>,
    /// Alerts dismissed by hand: not raised again until the condition clears.
    dismissed: HashSet<Key>,
    restarts: HashMap<String, Vec<(i64, i64)>>,
    kills: HashMap<String, i64>,
    last_backup_check: i64,
    backup_obs: Vec<Obs>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlertView {
    pub id: i64,
    pub rule_id: String,
    pub target: String,
    pub target_name: Option<String>,
    pub severity: Severity,
    pub message: String,
    pub started_at: i64,
    pub resolved_at: Option<i64>,
}

#[derive(Debug, Clone)]
struct Obs {
    target: String,
    name: String,
    active: bool,
    message: String,
}

enum Action {
    Fire { rule: AlertRule, obs: Obs },
    Resolve { rule: AlertRule, key: Key, id: i64, name: String },
}

fn pct(a: f64, b: f64) -> f64 {
    if b <= 0.0 { 0.0 } else { a / b * 100.0 }
}

fn notification_kind(kind: AlertKind) -> &'static str {
    match kind {
        AlertKind::Unhealthy => "container.unhealthy",
        AlertKind::RestartLoop => "container.restart_loop",
        AlertKind::UnexpectedExit => "container.crashed",
        AlertKind::Oom => "container.oom",
        _ => "alert.fired",
    }
}

/// Current observations for one threshold rule.
async fn observe(app: &App, rule: &AlertRule, state: &mut AlertState, now: i64) -> Vec<Obs> {
    let t = rule.threshold;
    let muted = |identity: &str| app.prefs.get(identity).alerts_muted;
    let name_of = |e: &crate::docker::Entry| crate::updates::check::display_name(app, e);
    match rule.kind {
        AlertKind::HostMemory => crate::monitor::host::memory()
            .map(|(total, avail)| {
                let free = pct(avail as f64, total as f64);
                vec![Obs {
                    target: "host".into(),
                    name: "Docker host".into(),
                    active: free < t,
                    message: format!(
                        "Only {:.0}% memory available ({} of {})",
                        free,
                        crate::util::human_bytes(avail),
                        crate::util::human_bytes(total)
                    ),
                }]
            })
            .unwrap_or_default(),
        AlertKind::ProxmoxMemory => app
            .proxmox
            .cached_overview()
            .filter(|_| app.proxmox.enabled())
            .map(|o| {
                let used = pct(o.node.mem_used as f64, o.node.mem_total as f64);
                vec![Obs {
                    target: format!("pve:{}", o.node.node),
                    name: format!("Proxmox {}", o.node.node),
                    active: used > t,
                    message: format!(
                        "Host memory at {:.0}% ({} of {})",
                        used,
                        crate::util::human_bytes(o.node.mem_used),
                        crate::util::human_bytes(o.node.mem_total)
                    ),
                }]
            })
            .unwrap_or_default(),
        AlertKind::ProxmoxStorage => app
            .proxmox
            .cached_overview()
            .filter(|_| app.proxmox.enabled())
            .map(|o| {
                o.storages
                    .iter()
                    .filter(|s| s.active && s.total > 0)
                    .map(|s| {
                        let used = pct(s.used as f64, s.total as f64);
                        Obs {
                            target: format!("pve-storage:{}", s.storage),
                            name: format!("Storage {}", s.storage),
                            active: used > t,
                            message: format!("{} is {:.0}% full", s.storage, used),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default(),
        AlertKind::Disk => crate::monitor::host::disks(app)
            .into_iter()
            .map(|d| {
                let used = pct(d.usage.used as f64, d.usage.total as f64);
                Obs {
                    target: format!("disk:{}", d.path),
                    name: d.path.clone(),
                    active: used > t,
                    message: format!(
                        "{} is {:.0}% full ({} free)",
                        d.path,
                        used,
                        crate::util::human_bytes(d.usage.available)
                    ),
                }
            })
            .collect(),
        AlertKind::ContainerMemory => {
            let host_total = crate::monitor::host::memory().map(|m| m.0).unwrap_or(u64::MAX);
            let latest = crate::monitor::stats::latest(app);
            app.docker
                .entries()
                .iter()
                .filter(|e| !muted(&e.summary.identity))
                .filter_map(|e| {
                    let s = latest.get(&e.summary.identity)?;
                    // Only containers with an explicit memory limit.
                    if s.mem_limit == 0 || s.mem_limit as f64 >= host_total as f64 * 0.95 {
                        return None;
                    }
                    let used = pct(s.mem as f64, s.mem_limit as f64);
                    Some(Obs {
                        target: e.summary.identity.clone(),
                        name: name_of(e),
                        active: used > t,
                        message: format!(
                            "Memory at {:.0}% of its {} limit",
                            used,
                            crate::util::human_bytes(s.mem_limit)
                        ),
                    })
                })
                .collect()
        }
        AlertKind::ContainerCpu => {
            let latest = crate::monitor::stats::latest(app);
            app.docker
                .entries()
                .iter()
                .filter(|e| !muted(&e.summary.identity))
                .filter_map(|e| {
                    let s = latest.get(&e.summary.identity)?;
                    Some(Obs {
                        target: e.summary.identity.clone(),
                        name: name_of(e),
                        active: s.cpu > t,
                        message: format!("CPU at {:.0}%", s.cpu),
                    })
                })
                .collect()
        }
        AlertKind::Unhealthy => app
            .docker
            .entries()
            .iter()
            .filter(|e| !muted(&e.summary.identity))
            .map(|e| Obs {
                target: e.summary.identity.clone(),
                name: name_of(e),
                active: e.summary.health.as_deref() == Some("unhealthy") && e.summary.is_running(),
                message: "Health check is failing".into(),
            })
            .collect(),
        AlertKind::RestartLoop => {
            let window = rule.duration_secs.max(60) as i64;
            let mut out = Vec::new();
            for e in app.docker.entries() {
                let id = e.summary.identity.clone();
                let hist = state.restarts.entry(id.clone()).or_default();
                hist.push((now, e.summary.restart_count));
                hist.retain(|(ts, _)| now - ts <= window);
                let first = hist.first().map(|h| h.1).unwrap_or(e.summary.restart_count);
                let delta = e.summary.restart_count - first;
                if muted(&id) {
                    continue;
                }
                out.push(Obs {
                    target: id,
                    name: name_of(&e),
                    active: delta as f64 >= t.max(1.0),
                    message: format!("Restarted {delta} times in {} minutes", window / 60),
                });
            }
            let live: HashSet<String> = app.docker.entries().iter().map(|e| e.summary.identity.clone()).collect();
            state.restarts.retain(|k, _| live.contains(k));
            out
        }
        AlertKind::BackupAge => {
            if now - state.last_backup_check < 600 {
                return state.backup_obs.clone();
            }
            state.last_backup_check = now;
            let settings = app.settings.get();
            if !settings.backups.schedule_enabled {
                state.backup_obs = Vec::new();
                return Vec::new();
            }
            let latest = app.backups.latest_by_identity().await.unwrap_or_default();
            let max_age = (t.max(1.0) * 3600.0) as i64;
            let obs: Vec<Obs> = app
                .docker
                .entries()
                .iter()
                .filter(|e| !e.summary.is_self && !muted(&e.summary.identity))
                .filter(|e| app.prefs.get(&e.summary.identity).backup_enabled(&settings))
                .filter(|e| e.summary.created < now - max_age)
                .filter(|e| crate::backups::plan::plan(app, e).has_content())
                .map(|e| {
                    let last = latest.get(&e.summary.identity).copied();
                    Obs {
                        target: e.summary.identity.clone(),
                        name: name_of(e),
                        active: last.is_none_or(|l| now - l > max_age),
                        message: match last {
                            Some(l) => format!("Last backup was {} ago", crate::util::human_duration(now - l)),
                            None => "Never backed up".into(),
                        },
                    }
                })
                .collect();
            state.backup_obs = obs.clone();
            obs
        }
        AlertKind::DiskHealth | AlertKind::DiskTemperature | AlertKind::PoolHealth | AlertKind::PoolScrubAge => {
            observe_storage(app, rule, now)
        }
        AlertKind::UnexpectedExit | AlertKind::Oom => {
            // Event driven; resolve once the container runs again.
            app.docker
                .entries()
                .iter()
                .map(|e| Obs {
                    target: e.summary.identity.clone(),
                    name: name_of(e),
                    active: !e.summary.is_running(),
                    message: String::new(),
                })
                .collect()
        }
    }
}

/// Disk and pool rules, from the disk service's latest reading. Disks and
/// pools that weren't read yet (or whose node is unreachable) keep whatever
/// state their alert is in, rather than resolving.
fn observe_storage(app: &App, rule: &AlertRule, now: i64) -> Vec<Obs> {
    let settings = app.settings.get();
    if !settings.proxmox.enabled || !settings.proxmox.disks.enabled {
        return Vec::new();
    }
    let t = rule.threshold;
    let disk_obs = |d: &crate::proxmox::disks::DiskView, active: bool, message: String| Obs {
        target: format!("pvedisk:{}", d.id),
        name: format!("{} on {}", crate::proxmox::disks::disk_name(d), d.node),
        active,
        message,
    };
    match rule.kind {
        AlertKind::DiskHealth => app
            .disks
            .disks()
            .iter()
            .filter(|d| d.present && d.sampled_at.is_some())
            .map(|d| match crate::proxmox::disks::health_problem(d) {
                Some(f) => disk_obs(d, true, format!("{} ({})", f.title, d.devpath)),
                None => disk_obs(d, false, String::new()),
            })
            .collect(),
        AlertKind::DiskTemperature => {
            // Only current readings count: not the last one of a disk that's
            // asleep, read only daily (resting), or couldn't be read lately.
            let fresh_for = i64::from(settings.proxmox.disks.sample_minutes.max(5)) * 120 + 600;
            app.disks
                .disks()
                .iter()
                .filter(|d| d.present)
                .filter_map(|d| {
                    let temp = d.temperature?;
                    let current = !d.asleep && !d.resting && d.sampled_at.is_some_and(|at| now - at <= fresh_for);
                    // Flash drives tolerate more: their own limit or the threshold + 20 °C.
                    let limit = if d.media.is_flash() { d.temp_limit.unwrap_or(t + 20.0) } else { t };
                    Some(disk_obs(
                        d,
                        current && temp > limit,
                        format!("{temp:.0} °C, above {limit:.0} °C ({})", d.devpath),
                    ))
                })
                .collect()
        }
        AlertKind::PoolHealth => app
            .disks
            .pools()
            .iter()
            .map(|p| {
                let problem = p.findings.iter().find(|f| {
                    f.level >= crate::proxmox::disks::smart::Level::Warning
                        && matches!(
                            f.code.as_str(),
                            "faulted"
                                | "degraded"
                                | "data_errors"
                                | "scrub_errors"
                                | "snapraid_errors"
                                | "metadata"
                                | "parity_small"
                        )
                });
                Obs {
                    target: format!("pool:{}", p.id),
                    name: format!("{} on {}", p.name, p.node),
                    active: problem.is_some(),
                    message: problem.map(|f| f.title.clone()).unwrap_or_default(),
                }
            })
            .collect(),
        AlertKind::PoolScrubAge => {
            let max_days = t.max(1.0);
            app.disks
                .pools()
                .iter()
                .filter_map(|p| {
                    use crate::proxmox::disks::pools::PoolKind;
                    let running = p.scan.as_ref().is_some_and(|s| s.is_active());
                    let (age_days, never) = match p.kind {
                        PoolKind::Zfs => {
                            let last = p.scan.as_ref().filter(|s| s.kind == "scrub").and_then(|s| s.finished_at);
                            match last {
                                Some(at) => (Some((now - at) as f64 / 86400.0), false),
                                None if p.scan.as_ref().is_some_and(|s| s.state == "none") => (None, true),
                                None => return None,
                            }
                        }
                        PoolKind::Snapraid => (p.snapraid.as_ref()?.oldest_scrub_days.map(f64::from), false),
                        _ => return None,
                    };
                    let stale = never || age_days.is_some_and(|a| a > max_days);
                    Some(Obs {
                        target: format!("pool:{}", p.id),
                        name: format!("{} on {}", p.name, p.node),
                        active: stale && !running,
                        message: if never {
                            "Never scrubbed".into()
                        } else {
                            format!("Last scrubbed {:.0} days ago", age_days.unwrap_or(0.0))
                        },
                    })
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

async fn evaluate(app: &App) {
    let settings = app.settings.get();
    let now = crate::util::now();
    let mut actions = Vec::new();
    // Take the state out while awaiting observations, then put it back.
    let mut state = std::mem::take(&mut *app.monitor.inner.alerts.lock());
    for rule in settings.monitoring.rules.iter() {
        let event_driven = matches!(rule.kind, AlertKind::UnexpectedExit | AlertKind::Oom);
        if !rule.enabled {
            // Resolve anything left over from a rule that was switched off.
            for (key, (id, _)) in state.firing.iter().filter(|(k, _)| k.0 == rule.id) {
                actions.push(Action::Resolve { rule: rule.clone(), key: key.clone(), id: *id, name: key.1.clone() });
            }
            continue;
        }
        let obs = observe(app, rule, &mut state, now).await;
        let seen: HashSet<String> = obs.iter().map(|o| o.target.clone()).collect();
        for o in obs {
            let key = (rule.id.clone(), o.target.clone());
            if event_driven {
                if !o.active {
                    state.dismissed.remove(&key);
                    if let Some((id, _)) = state.firing.get(&key) {
                        actions.push(Action::Resolve {
                            rule: rule.clone(),
                            key: key.clone(),
                            id: *id,
                            name: o.name.clone(),
                        });
                    }
                }
                continue;
            }
            if o.active {
                if state.dismissed.contains(&key) {
                    continue;
                }
                let since = *state.pending.entry(key.clone()).or_insert(now);
                if now - since >= rule.duration_secs as i64 && !state.firing.contains_key(&key) {
                    actions.push(Action::Fire { rule: rule.clone(), obs: o });
                }
            } else {
                state.pending.remove(&key);
                state.dismissed.remove(&key);
                if let Some((id, _)) = state.firing.get(&key) {
                    actions.push(Action::Resolve {
                        rule: rule.clone(),
                        key: key.clone(),
                        id: *id,
                        name: o.name.clone(),
                    });
                }
            }
        }
        // Targets that disappeared (container removed) resolve too.
        for (key, (id, _)) in state.firing.iter().filter(|(k, _)| k.0 == rule.id && !seen.contains(&k.1)) {
            actions.push(Action::Resolve { rule: rule.clone(), key: key.clone(), id: *id, name: key.1.clone() });
        }
    }
    *app.monitor.inner.alerts.lock() = state;
    for a in actions {
        apply(app, a).await;
    }
}

async fn apply(app: &App, action: Action) {
    match action {
        Action::Fire { rule, obs } => {
            if let Err(err) = fire(app, &rule, &obs.target, &obs.name, &obs.message).await {
                tracing::warn!(%err, "failed to record alert");
            }
        }
        Action::Resolve { rule, key, id, name } => {
            let now = crate::util::now();
            let res = app
                .db
                .call(move |c| {
                    c.execute("UPDATE alerts SET resolved_at = ?1 WHERE id = ?2", params![now, id])?;
                    Ok(())
                })
                .await;
            if let Err(err) = res {
                tracing::warn!(%err, "failed to resolve alert");
                return;
            }
            let started = app.monitor.inner.alerts.lock().firing.remove(&key).map(|f| f.1).unwrap_or(now);
            let view = AlertView {
                id,
                rule_id: rule.id.clone(),
                target: key.1.clone(),
                target_name: Some(name.clone()),
                severity: rule.severity,
                message: "resolved".into(),
                started_at: started,
                resolved_at: Some(now),
            };
            app.bus.publish(Event::Alert(std::sync::Arc::new(view)));
            if !matches!(rule.kind, AlertKind::UnexpectedExit | AlertKind::Oom) {
                let display =
                    if name.contains(':') { name.split(':').next_back().unwrap_or(&name).to_string() } else { name };
                app.notifier.send(Notification::new(
                    "alert.resolved",
                    Severity::Success,
                    format!("Resolved: {display}"),
                    format!(
                        "{} is back to normal after {}",
                        rule_label(rule.kind),
                        crate::util::human_duration(now - started)
                    ),
                ));
            }
        }
    }
}

fn rule_label(kind: AlertKind) -> &'static str {
    match kind {
        AlertKind::HostMemory => "Host memory",
        AlertKind::ProxmoxMemory => "Proxmox memory",
        AlertKind::ProxmoxStorage => "Proxmox storage",
        AlertKind::ContainerMemory => "Container memory",
        AlertKind::ContainerCpu => "Container CPU",
        AlertKind::RestartLoop => "Restart loop",
        AlertKind::Unhealthy => "Health check",
        AlertKind::UnexpectedExit => "Crash",
        AlertKind::Oom => "Out of memory",
        AlertKind::Disk => "Disk usage",
        AlertKind::BackupAge => "Backup age",
        AlertKind::DiskHealth => "Disk health",
        AlertKind::DiskTemperature => "Disk temperature",
        AlertKind::PoolHealth => "Pool health",
        AlertKind::PoolScrubAge => "Pool scrub age",
    }
}

async fn fire(app: &App, rule: &AlertRule, target: &str, name: &str, message: &str) -> Result<()> {
    let now = crate::util::now();
    let (rid, tgt, nm, msg, sev) =
        (rule.id.clone(), target.to_string(), name.to_string(), message.to_string(), rule.severity);
    let id = app
        .db
        .call(move |c| {
            c.execute(
                "INSERT INTO alerts (rule_id, target, target_name, severity, message, started_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![rid, tgt, nm, sev.as_str(), msg, now],
            )?;
            Ok(c.last_insert_rowid())
        })
        .await?;
    app.monitor.inner.alerts.lock().firing.insert((rule.id.clone(), target.to_string()), (id, now));
    let view = AlertView {
        id,
        rule_id: rule.id.clone(),
        target: target.to_string(),
        target_name: Some(name.to_string()),
        severity: rule.severity,
        message: message.to_string(),
        started_at: now,
        resolved_at: None,
    };
    app.bus.publish(Event::Alert(std::sync::Arc::new(view)));
    let kind = notification_kind(rule.kind);
    let is_container = !target.contains(':') || target.starts_with("compose:") || target.starts_with("container:");
    let mut n =
        Notification::new(kind, rule.severity, format!("{name}: {}", rule_label(rule.kind)), message.to_string());
    if is_container {
        n = n.target(target, name);
    }
    let page =
        if target.starts_with("pvedisk:") || target.starts_with("pool:") { "/proxmox/disks" } else { "/monitoring" };
    app.notifier.send(n.link(app, page));
    let activity_kind: &'static str = match rule.kind {
        AlertKind::UnexpectedExit => "container.crashed",
        AlertKind::Oom => "container.oom",
        AlertKind::Unhealthy => "container.unhealthy",
        AlertKind::RestartLoop => "container.restart_loop",
        _ => "alert.fired",
    };
    let mut entry = NewActivity::new(activity_kind, rule.severity, format!("{name}: {message}"));
    if is_container {
        entry = entry.target(target, name);
    }
    app.note(entry).await;
    Ok(())
}

fn handle_lifecycle(app: &App, ev: &Lifecycle) -> Option<(AlertKind, String)> {
    let mut state = app.monitor.inner.alerts.lock();
    match ev.action.as_str() {
        "kill" | "stop" => {
            state.kills.insert(ev.id.clone(), ev.ts);
            None
        }
        "oom" => Some((AlertKind::Oom, "Killed by the out-of-memory killer".into())),
        "die" => {
            let killed_recently = state.kills.get(&ev.id).is_some_and(|t| ev.ts - t <= 30);
            state.kills.retain(|_, t| ev.ts - *t <= 120);
            match ev.exit_code {
                Some(code) if code != 0 && !killed_recently => {
                    Some((AlertKind::UnexpectedExit, format!("Exited unexpectedly with code {code}")))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Runs the alert engine forever.
pub async fn run(app: App) {
    // Re-adopt alerts that were open when Nodestarr stopped.
    if let Ok(open) = list(&app, true, 500).await {
        let mut state = app.monitor.inner.alerts.lock();
        for a in open {
            state.firing.insert((a.rule_id.clone(), a.target.clone()), (a.id, a.started_at));
        }
    }
    let mut lifecycle = app.docker.subscribe_lifecycle();
    let mut dismissals = app.monitor.inner.dismiss_rx.lock().take();
    let mut tick = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = tick.tick() => evaluate(&app).await,
            Some(id) = async { match dismissals.as_mut() { Some(rx) => rx.recv().await, None => std::future::pending().await } } => {
                if let Err(err) = dismiss(&app, id).await {
                    tracing::warn!(%err, "failed to dismiss alert");
                }
            }
            ev = lifecycle.recv() => match ev {
                Ok(ev) => {
                    if let Some((kind, message)) = handle_lifecycle(&app, &ev) {
                        let settings = app.settings.get();
                        let Some(rule) = settings.monitoring.rules.iter().find(|r| r.kind == kind && r.enabled).cloned() else { continue };
                        if app.prefs.get(&ev.identity).alerts_muted {
                            continue;
                        }
                        let key = (rule.id.clone(), ev.identity.clone());
                        if app.monitor.inner.alerts.lock().firing.contains_key(&key) {
                            continue;
                        }
                        let name = app.docker.get(&ev.id).map(|e| crate::updates::check::display_name(&app, &e)).unwrap_or_else(|| ev.name.clone());
                        if let Err(err) = fire(&app, &rule, &ev.identity, &name, &message).await {
                            tracing::warn!(%err, "failed to record alert");
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => {
                    lifecycle = app.docker.subscribe_lifecycle();
                }
            },
        }
    }
}

/// Closes an alert by hand. A threshold alert stays quiet until its
/// condition clears; a crash alert simply closes.
async fn dismiss(app: &App, id: i64) -> Result<()> {
    {
        let mut state = app.monitor.inner.alerts.lock();
        let key = state.firing.iter().find(|(_, (aid, _))| *aid == id).map(|(k, _)| k.clone());
        if let Some(key) = key {
            state.firing.remove(&key);
            state.pending.remove(&key);
            state.dismissed.insert(key);
        }
    }
    let now = crate::util::now();
    let view = app
        .db
        .call(move |c| {
            use rusqlite::OptionalExtension;
            c.execute("UPDATE alerts SET resolved_at = ?1 WHERE id = ?2 AND resolved_at IS NULL", params![now, id])?;
            Ok(c.query_row(
                "SELECT id, rule_id, target, target_name, severity, message, started_at, resolved_at FROM alerts WHERE id = ?1",
                [id],
                row_to_view,
            )
            .optional()?)
        })
        .await?;
    if let Some(v) = view {
        app.bus.publish(Event::Alert(std::sync::Arc::new(v)));
    }
    Ok(())
}

fn row_to_view(r: &rusqlite::Row<'_>) -> rusqlite::Result<AlertView> {
    let sev: String = r.get(4)?;
    Ok(AlertView {
        id: r.get(0)?,
        rule_id: r.get(1)?,
        target: r.get(2)?,
        target_name: r.get(3)?,
        severity: serde_json::from_value(serde_json::Value::String(sev)).unwrap_or_default(),
        message: r.get(5)?,
        started_at: r.get(6)?,
        resolved_at: r.get(7)?,
    })
}

/// Alerts from the database, newest first.
pub async fn list(app: &App, active_only: bool, limit: u32) -> Result<Vec<AlertView>> {
    app.db
        .call(move |c| {
            let sql = format!(
                "SELECT id, rule_id, target, target_name, severity, message, started_at, resolved_at FROM alerts {} ORDER BY started_at DESC LIMIT {}",
                if active_only { "WHERE resolved_at IS NULL" } else { "" },
                limit.clamp(1, 1000)
            );
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map([], row_to_view)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages() {
        assert_eq!(pct(1.0, 0.0), 0.0);
        assert_eq!(pct(50.0, 200.0), 25.0);
        assert_eq!(notification_kind(AlertKind::Oom), "container.oom");
        assert_eq!(notification_kind(AlertKind::Disk), "alert.fired");
    }
}
