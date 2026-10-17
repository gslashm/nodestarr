//! SMART self-tests, started by hand or on schedules, through the host
//! helper.
//!
//! Tests are queued in the database and a small runner takes them from
//! there: it waits while the disk's pool resilvers or scrubs, keeps to the
//! per-node limit, starts the test, follows its progress and reads the
//! result from the drive's own self-test log. Everything survives a restart
//! of Nodestarr; a test keeps running in the drive meanwhile.

use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::smart::{self, TestOutcome};
use super::store::{self, NewEvent};
use super::{DiskView, disk_name, node_address};
use crate::activity::{Actor, NewActivity};
use crate::app::App;
use crate::error::{AppError, Result};
use crate::notify::Notification;
use crate::settings::{Settings, Severity, TEST_KINDS, TestSchedule, parse_cron};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TestRun {
    pub id: String,
    pub disk: String,
    pub node: String,
    pub devpath: String,
    /// short, long or conveyance.
    pub kind: String,
    /// queued, running, passed, failed, aborted, interrupted, skipped or error.
    pub status: String,
    pub queued_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub progress: Option<f64>,
    pub expected_secs: Option<i64>,
    pub start_hours: Option<i64>,
    /// The drive's newest log entry when the test started; the result is a newer one.
    pub log_top: Option<String>,
    pub result: Option<String>,
    pub lba: Option<i64>,
    pub message: Option<String>,
    pub actor: Option<String>,
    pub schedule: Option<String>,
}

impl TestRun {
    pub fn is_active(&self) -> bool {
        self.status == "queued" || self.status == "running"
    }
}

fn label(kind: &str) -> &str {
    match kind {
        "long" => "Long",
        "conveyance" => "Conveyance",
        _ => "Short",
    }
}

/// Queues a test on one disk.
pub async fn enqueue(app: &App, disk_id: &str, kind: &str, actor: &Actor) -> Result<TestRun> {
    if !TEST_KINDS.contains(&kind) {
        return Err(AppError::bad_request("the test type is short, long or conveyance"));
    }
    let d = app.disks.disk(disk_id).ok_or_else(|| AppError::not_found(format!("disk {disk_id}")))?;
    if !d.tests.available {
        return Err(AppError::conflict(d.tests.reason.clone().unwrap_or_else(|| "self-tests aren't available".into())));
    }
    if !supports(&d, kind) {
        return Err(AppError::bad_request(format!("this disk doesn't support {kind} self-tests")));
    }
    queue(app, &d, kind, actor, None).await
}

fn supports(d: &DiskView, kind: &str) -> bool {
    match kind {
        "short" => d.tests.short,
        "long" => d.tests.long,
        "conveyance" => d.tests.conveyance,
        _ => false,
    }
}

async fn queue(app: &App, d: &DiskView, kind: &str, actor: &Actor, schedule: Option<&str>) -> Result<TestRun> {
    if store::active_tests(&app.db).await?.iter().any(|t| t.disk == d.id) {
        return Err(AppError::conflict("a self-test is already running or waiting on this disk"));
    }
    let minutes = match kind {
        "long" => d.tests.long_minutes,
        "conveyance" => d.tests.conveyance_minutes,
        _ => d.tests.short_minutes,
    };
    let t = TestRun {
        id: crate::util::short_id(),
        disk: d.id.clone(),
        node: d.node.clone(),
        devpath: d.devpath.clone(),
        kind: kind.to_string(),
        status: "queued".into(),
        queued_at: crate::util::now(),
        expected_secs: minutes.map(|m| i64::from(m) * 60),
        actor: Some(actor.to_string()),
        schedule: schedule.map(str::to_string),
        ..Default::default()
    };
    store::save_test(&app.db, t.clone()).await?;
    app.disks.wake_tests();
    app.disks.publish();
    Ok(t)
}

/// Aborts the running test of a disk, or cancels its queued one.
pub async fn abort(app: &App, disk_id: &str, actor: &Actor) -> Result<TestRun> {
    let t = store::active_tests(&app.db)
        .await?
        .into_iter()
        .find(|t| t.disk == disk_id)
        .ok_or_else(|| AppError::not_found("no self-test is running or waiting on this disk"))?;
    if t.status == "queued" {
        return Ok(finish(app, t, "skipped", None, None, Some(format!("Canceled by {actor}"))).await);
    }
    let s = app.settings.get();
    let address = node_address(app, &t.node)
        .await
        .ok_or_else(|| AppError::bad_request(format!("no SSH address for {}", t.node)))?;
    let out = app
        .disks
        .host()
        .run(
            &s.proxmox.host_access,
            &t.node,
            &address,
            &["selftest".into(), t.devpath.clone(), "abort".into()],
            Duration::from_secs(60),
            false,
        )
        .await?;
    if out.code != 0 {
        return Err(AppError::upstream(format!(
            "couldn't abort the test: {}",
            smartctl_problem(&out.stdout, &out.message())
        )));
    }
    Ok(finish(app, t, "aborted", Some("Aborted by host".into()), None, Some(format!("Aborted by {actor}"))).await)
}

/// Cancels a queued test by its id.
pub async fn cancel(app: &App, id: &str, actor: &Actor) -> Result<TestRun> {
    let t = store::get_test(&app.db, id.to_string()).await?.ok_or_else(|| AppError::not_found(format!("test {id}")))?;
    match t.status.as_str() {
        "queued" => Ok(finish(app, t, "skipped", None, None, Some(format!("Canceled by {actor}"))).await),
        "running" => abort(app, &t.disk, actor).await,
        _ => Err(AppError::conflict("this test already finished")),
    }
}

/// When a schedule runs next (unix seconds).
pub fn next_run(settings: &Settings, cron: &str) -> Option<i64> {
    let tz = settings.tz();
    let now = Utc::now().with_timezone(&tz);
    parse_cron(cron).ok()?.find_next_occurrence(&now, false).ok().map(|d| d.timestamp())
}

/// Whether a schedule covers a disk.
pub fn selects(s: &TestSchedule, d: &DiskView) -> bool {
    if !s.nodes.is_empty() && !s.nodes.contains(&d.node) {
        return false;
    }
    match s.disks.as_str() {
        "hdd" => d.media == smart::Media::Hdd,
        "ssd" => d.media.is_flash(),
        "nvme" => d.media == smart::Media::Nvme,
        "list" => s.ids.contains(&d.id),
        _ => true,
    }
}

/// Starts the test runner and the schedules.
pub fn start(app: App) {
    {
        let app = app.clone();
        tokio::spawn(async move {
            let mut last = Utc::now();
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let s = app.settings.get();
                let tz = s.tz();
                let now = Utc::now();
                if s.proxmox.enabled && s.proxmox.disks.enabled {
                    let (from, to) = (last.with_timezone(&tz), now.with_timezone(&tz));
                    for sch in s.proxmox.disks.schedules.iter().filter(|x| x.enabled) {
                        if crate::scheduler::fired_between(&sch.cron, &from, &to) {
                            run_schedule(&app, sch).await;
                        }
                    }
                }
                last = now;
            }
        });
    }
    tokio::spawn(async move {
        let mut memo = Memo::default();
        loop {
            if app.settings.get().proxmox.enabled {
                tick(&app, &mut memo).await;
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(15)) => {}
                _ = app.disks.inner.tests_wake.notified() => {}
            }
        }
    });
}

/// Queues a schedule's test on every disk it covers.
async fn run_schedule(app: &App, sch: &TestSchedule) {
    let mut queued = 0usize;
    let mut skipped: Vec<String> = Vec::new();
    for d in app.disks.disks().iter().filter(|d| d.present && selects(sch, d)) {
        if !d.tests.available {
            skipped.push(format!(
                "{} ({})",
                d.devpath,
                d.tests.reason.clone().unwrap_or_default().trim_end_matches('.')
            ));
            continue;
        }
        if !supports(d, &sch.kind) {
            skipped.push(format!("{} (no {} test)", d.devpath, sch.kind));
            continue;
        }
        match queue(app, d, &sch.kind, &Actor::schedule(), Some(&sch.id)).await {
            Ok(_) => queued += 1,
            Err(err) => skipped.push(format!("{} ({err})", d.devpath)),
        }
    }
    if queued == 0 && skipped.is_empty() {
        return;
    }
    let mut msg = format!(
        "{}: {} self-test queued for {}",
        sch.name,
        label(&sch.kind).to_lowercase(),
        if queued == 1 { "1 disk".to_string() } else { format!("{queued} disks") }
    );
    if !skipped.is_empty() {
        msg.push_str(&format!("; skipped {}", skipped.join(", ")));
    }
    app.note(NewActivity::new("disk.test", Severity::Info, crate::util::truncate(&msg, 900)).actor(&Actor::schedule()))
        .await;
}

#[derive(Default)]
struct Memo {
    /// When each running test was last polled.
    polled: HashMap<String, i64>,
    /// Since when a running test's node couldn't be reached.
    unreachable: HashMap<String, i64>,
    /// Polls that found neither a running test nor a result.
    no_result: HashMap<String, u32>,
}

async fn tick(app: &App, memo: &mut Memo) {
    let Ok(active) = store::active_tests(&app.db).await else { return };
    let now = crate::util::now();
    let mut due: HashMap<String, Vec<TestRun>> = HashMap::new();
    for t in active.iter().filter(|t| t.status == "running") {
        let fresh = t.started_at.is_some_and(|s| now - s < 120);
        let every = if t.kind == "long" && !fresh { 300 } else { 30 };
        if now - memo.polled.get(&t.id).copied().unwrap_or(0) >= every {
            due.entry(t.node.clone()).or_default().push(t.clone());
        }
    }
    let settings = app.settings.get();
    for (node, tests) in due {
        poll(app, &settings, &node, tests, memo).await;
    }
    let Ok(active) = store::active_tests(&app.db).await else { return };
    let mut running: HashMap<String, usize> = HashMap::new();
    for t in active.iter().filter(|t| t.status == "running") {
        *running.entry(t.node.clone()).or_default() += 1;
    }
    for t in active.into_iter().filter(|t| t.status == "queued") {
        try_start(app, &settings, t, &mut running).await;
    }
    let live: std::collections::HashSet<String> =
        store::active_tests(&app.db).await.unwrap_or_default().into_iter().map(|t| t.id).collect();
    memo.polled.retain(|k, _| live.contains(k));
    memo.unreachable.retain(|k, _| live.contains(k));
    memo.no_result.retain(|k, _| live.contains(k));
}

/// Why the disk should wait: its pool is resilvering, rebuilding or scrubbing.
fn busy_reason(app: &App, d: &DiskView) -> Option<String> {
    let pools = app.disks.pools();
    d.roles.iter().find_map(|r| {
        let p = pools.iter().find(|p| p.id == r.pool)?;
        let s = p.scan.as_ref().filter(|s| s.is_active())?;
        Some(format!("{} of {} in progress", s.kind, p.name))
    })
}

async fn wait(app: &App, mut t: TestRun, message: String) {
    if t.message.as_deref() != Some(message.as_str()) {
        t.message = Some(message);
        if store::save_test(&app.db, t).await.is_ok() {
            app.disks.publish();
        }
    }
}

/// smartctl's own complaint in its JSON output, or the helper's message.
fn smartctl_problem(stdout: &str, fallback: &str) -> String {
    serde_json::from_str::<Value>(stdout)
        .ok()
        .and_then(|v| {
            let msgs: Vec<String> = v["smartctl"]["messages"]
                .as_array()?
                .iter()
                .filter_map(|m| m["string"].as_str().map(str::to_string))
                .collect();
            (!msgs.is_empty()).then(|| msgs.join(" "))
        })
        .unwrap_or_else(|| if fallback.is_empty() { "smartctl failed".into() } else { fallback.to_string() })
}

fn signature(e: Option<&smart::SelfTestEntry>) -> String {
    e.map(|e| {
        format!("{}|{}|{}|{}", e.kind, e.hours.map(|h| h.to_string()).unwrap_or_default(), e.status, e.lba.unwrap_or(0))
    })
    .unwrap_or_default()
}

/// The drive's log as the test starts: its length and newest entry.
fn log_state(log: &[smart::SelfTestEntry]) -> String {
    format!("{}#{}", log.len(), signature(log.first()))
}

/// Whether the log gained an entry since `start`. Two identical results in
/// a row (same test, same hour) are told apart by the length, or, once the
/// log is full and rotates, by the old newest entry moving down one place.
fn has_new_entry(start: Option<&str>, log: &[smart::SelfTestEntry]) -> bool {
    if log.is_empty() {
        return false;
    }
    let Some((len, top)) = start.and_then(|s| s.split_once('#')) else { return true };
    let len: usize = len.parse().unwrap_or(0);
    signature(log.first()) != top || log.len() > len || (!top.is_empty() && signature(log.get(1)) == top)
}

async fn try_start(app: &App, settings: &Settings, t: TestRun, running: &mut HashMap<String, usize>) {
    let now = crate::util::now();
    let Some(d) = app.disks.disk(&t.disk).filter(|d| d.present) else {
        finish(app, t, "skipped", None, None, Some("The disk isn't connected anymore.".into())).await;
        return;
    };
    let host = app.disks.host();
    if !settings.proxmox.host_access.enabled || !host.usable(&d.node) {
        if now - t.queued_at > 3600 {
            let why = host.node(&d.node).and_then(|n| n.error).unwrap_or_else(|| "host access isn't working".into());
            finish(app, t, "error", None, None, Some(format!("Couldn't start within an hour: {why}"))).await;
        } else {
            wait(app, t, format!("Waiting for host access to {}", d.node)).await;
        }
        return;
    }
    if let Some(reason) = busy_reason(app, &d) {
        if now - t.queued_at > 86400 {
            finish(app, t, "skipped", None, None, Some(format!("Waited a day: {reason}"))).await;
        } else {
            wait(app, t, format!("Waiting: {reason}")).await;
        }
        return;
    }
    let max = settings.proxmox.disks.max_parallel_tests as usize;
    if max > 0 && running.get(&d.node).copied().unwrap_or(0) >= max {
        wait(app, t, format!("Waiting for other self-tests on {} to finish", d.node)).await;
        return;
    }
    let Some(address) = node_address(app, &d.node).await else { return };
    let ha = &settings.proxmox.host_access;
    // A fresh read first: the current power-on hours, the newest log entry,
    // and whether a test runs already (smartd's own schedule, say).
    let fresh = host
        .run(
            ha,
            &d.node,
            &address,
            &["smart".into(), "--wake".into(), d.devpath.clone()],
            Duration::from_secs(90),
            false,
        )
        .await
        .and_then(|o| o.json())
        .ok()
        .and_then(|v| v.get(0).cloned())
        .and_then(|e| (!e["data"].is_null()).then(|| smart::from_smartctl(&e["data"], now)));
    let Some(report) = fresh else {
        wait(app, t, "Couldn't read the disk; trying again shortly".into()).await;
        return;
    };
    if report.self_tests.running.is_some() {
        finish(app, t, "skipped", None, None, Some("Another self-test was already running on the disk.".into())).await;
        return;
    }
    if !report.self_tests.supports(&t.kind) {
        let msg = format!("The disk doesn't support {} self-tests.", t.kind);
        finish(app, t, "skipped", None, None, Some(msg)).await;
        return;
    }
    let out = match host
        .run(
            ha,
            &d.node,
            &address,
            &["selftest".into(), d.devpath.clone(), t.kind.clone()],
            Duration::from_secs(60),
            false,
        )
        .await
    {
        Ok(o) => o,
        Err(err) => {
            wait(app, t, format!("Couldn't start yet: {err}")).await;
            return;
        }
    };
    if out.code != 0 {
        let problem = smartctl_problem(&out.stdout, &out.message());
        let status =
            if problem.contains("Can't start self-test without aborting current test") { "skipped" } else { "error" };
        finish(app, t, status, None, None, Some(problem)).await;
        return;
    }
    let mut t = t;
    t.status = "running".into();
    t.node = d.node.clone();
    t.devpath = d.devpath.clone();
    t.started_at = Some(now);
    t.progress = Some(0.0);
    t.start_hours = report.power_on_hours.and_then(|h| i64::try_from(h).ok());
    t.log_top = Some(log_state(&report.self_tests.log));
    if let Some(m) = report.self_tests.minutes(&t.kind) {
        t.expected_secs = Some(i64::from(m) * 60);
    }
    t.message = None;
    if store::save_test(&app.db, t.clone()).await.is_err() {
        return;
    }
    *running.entry(d.node.clone()).or_default() += 1;
    let _ = store::insert_events(
        &app.db,
        now,
        vec![NewEvent::new(&d.id, Severity::Info, "test", format!("{} self-test started", label(&t.kind)))],
    )
    .await;
    if t.schedule.is_none() {
        let actor = Actor(t.actor.clone().unwrap_or_else(|| "system".into()));
        app.note(
            NewActivity::new(
                "disk.test",
                Severity::Info,
                format!("{} self-test started on {} ({} on {})", label(&t.kind), disk_name(&d), d.devpath, d.node),
            )
            .actor(&actor),
        )
        .await;
    }
    app.disks.publish();
}

async fn poll(app: &App, settings: &Settings, node: &str, tests: Vec<TestRun>, memo: &mut Memo) {
    let now = crate::util::now();
    for t in &tests {
        memo.polled.insert(t.id.clone(), now);
    }
    let address = node_address(app, node).await.unwrap_or_default();
    let mut args = vec!["selftest-status".to_string()];
    args.extend(tests.iter().map(|t| t.devpath.clone()));
    let timeout = Duration::from_secs(30 + 15 * tests.len() as u64);
    let res = app
        .disks
        .host()
        .run(&settings.proxmox.host_access, node, &address, &args, timeout, false)
        .await
        .and_then(|o| if o.code == 0 { o.json() } else { Err(AppError::upstream(o.message())) });
    let entries = match res {
        Ok(Value::Array(e)) => e,
        Ok(_) => Vec::new(),
        Err(err) => {
            for t in tests {
                let since = *memo.unreachable.entry(t.id.clone()).or_insert(now);
                if now - since > 12 * 3600 {
                    finish(
                        app,
                        t,
                        "interrupted",
                        None,
                        None,
                        Some(format!("Lost contact with {node} for 12 hours: {err}")),
                    )
                    .await;
                }
            }
            return;
        }
    };
    for mut t in tests {
        memo.unreachable.remove(&t.id);
        let Some(e) = entries.iter().find(|e| e["device"].as_str() == Some(t.devpath.as_str())) else { continue };
        if e["data"].is_null() {
            continue;
        }
        let r = smart::from_smartctl(&e["data"], now);
        if let Some(run) = &r.self_tests.running {
            // Drives report progress in steps of 10%.
            let progress = run.remaining.map(|rem| 1.0 - f64::from(rem) / 100.0);
            if progress.is_some() && progress != t.progress {
                t.progress = progress;
                if store::save_test(&app.db, t).await.is_ok() {
                    app.disks.publish();
                }
            }
            continue;
        }
        let newest = r.self_tests.log.first().filter(|_| has_new_entry(t.log_top.as_deref(), &r.self_tests.log));
        match newest {
            Some(entry) => {
                let status = match entry.outcome {
                    TestOutcome::Passed => "passed",
                    TestOutcome::Failed => "failed",
                    TestOutcome::Aborted => "aborted",
                    _ => "interrupted",
                };
                finish(app, t, status, Some(entry.status.clone()), entry.lba, None).await;
            }
            None => {
                let n = memo.no_result.entry(t.id.clone()).or_insert(0);
                *n += 1;
                let started = t.started_at.unwrap_or(now);
                if *n >= 3 && now - started > 180 {
                    finish(
                        app,
                        t,
                        "interrupted",
                        None,
                        None,
                        Some(
                            "The drive reports neither a running test nor a result. It may have been reset, \
                             power-cycled or put to sleep."
                                .into(),
                        ),
                    )
                    .await;
                }
            }
        }
    }
}

/// Records the end of a test: database, events, activity, notification.
async fn finish(
    app: &App,
    mut t: TestRun,
    status: &str,
    result: Option<String>,
    lba: Option<u64>,
    message: Option<String>,
) -> TestRun {
    let now = crate::util::now();
    let ran = t.started_at.is_some();
    t.status = status.to_string();
    t.finished_at = Some(now);
    if status == "passed" {
        t.progress = Some(1.0);
    }
    t.result = result.or(t.result);
    t.lba = lba.and_then(|l| i64::try_from(l).ok());
    t.message = message;
    if let Err(err) = store::save_test(&app.db, t.clone()).await {
        tracing::warn!(%err, "couldn't record a self-test result");
    }
    let disk = app.disks.disk(&t.disk);
    let name = disk.as_ref().map(disk_name).unwrap_or_else(|| t.disk.clone());
    let took = t.started_at.map(|s| format!(" after {}", crate::util::human_duration(now - s))).unwrap_or_default();
    let (sev, text) = match status {
        "passed" => (Severity::Success, format!("{} self-test passed{took}", label(&t.kind))),
        "failed" => (
            Severity::Error,
            format!(
                "{} self-test failed{took}: {}{}",
                label(&t.kind),
                t.result.clone().unwrap_or_else(|| "failed".into()),
                t.lba.map(|l| format!(" (first bad block {l})")).unwrap_or_default()
            ),
        ),
        "aborted" => (Severity::Warning, format!("{} self-test aborted{took}", label(&t.kind))),
        "interrupted" => (
            Severity::Warning,
            format!(
                "{} self-test didn't finish{took}: {}",
                label(&t.kind),
                t.result.clone().or(t.message.clone()).unwrap_or_default()
            ),
        ),
        "skipped" => {
            (Severity::Info, format!("{} self-test skipped: {}", label(&t.kind), t.message.clone().unwrap_or_default()))
        }
        _ => (
            Severity::Warning,
            format!("{} self-test couldn't start: {}", label(&t.kind), t.message.clone().unwrap_or_default()),
        ),
    };
    let _ = store::insert_events(&app.db, now, vec![NewEvent::new(&t.disk, sev, "test", text.clone())]).await;
    // Scheduled tests that were merely skipped show up in the schedule's summary instead.
    if ran || t.schedule.is_none() {
        let actor = Actor(t.actor.clone().unwrap_or_else(|| "system".into()));
        app.note(NewActivity::new("disk.test", sev, format!("{name}: {text}")).actor(&actor)).await;
    }
    let notify = match status {
        "failed" | "interrupted" | "error" => true,
        "passed" => app.settings.get().proxmox.disks.notify_passed,
        _ => false,
    };
    if notify && (ran || status == "error") {
        let mut n = Notification::new(
            "disk.test",
            sev,
            format!("{name}: {}", label(&t.kind).to_lowercase() + " self-test"),
            text,
        );
        if let Some(d) = &disk {
            n = n.field("Disk", format!("{} on {}", d.devpath, d.node));
            if !d.label.bay.is_empty() {
                n = n.field("Bay", d.label.bay.clone());
            }
        }
        app.notifier.send(n.link(app, &format!("/proxmox/disks/{}", t.disk)));
    }
    app.disks.publish();
    if ran {
        // The drive's log and attributes changed: read them again.
        super::refresh(app);
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxmox::disks::smart::Media;

    #[test]
    fn schedules_select_disks() {
        let d = |id: &str, node: &str, media: Media| DiskView {
            id: id.into(),
            node: node.into(),
            media,
            ..Default::default()
        };
        let mut s = TestSchedule {
            id: "s".into(),
            name: "S".into(),
            enabled: true,
            kind: "short".into(),
            cron: "0 3 * * *".into(),
            disks: "all".into(),
            ids: vec![],
            nodes: vec![],
        };
        assert!(selects(&s, &d("a", "pve", Media::Hdd)));
        s.disks = "ssd".into();
        assert!(selects(&s, &d("a", "pve", Media::Nvme)));
        assert!(!selects(&s, &d("a", "pve", Media::Hdd)));
        s.disks = "list".into();
        s.ids = vec!["b".into()];
        assert!(!selects(&s, &d("a", "pve", Media::Hdd)));
        assert!(selects(&s, &d("b", "pve", Media::Hdd)));
        s.nodes = vec!["pve2".into()];
        assert!(!selects(&s, &d("b", "pve", Media::Hdd)));
    }

    #[test]
    fn smartctl_messages() {
        let out = r#"{"smartctl": {"messages": [{"string": "Can't start self-test without aborting current test (90% remaining),", "severity": "error"}]}}"#;
        assert!(smartctl_problem(out, "x").starts_with("Can't start self-test"));
        assert_eq!(smartctl_problem("garbage", "helper said no"), "helper said no");
        assert_eq!(smartctl_problem("{}", ""), "smartctl failed");
    }

    #[test]
    fn new_log_entries_are_recognized() {
        let a = smart::SelfTestEntry {
            kind: "short".into(),
            status: "Completed without error".into(),
            hours: Some(10),
            ..Default::default()
        };
        let b = smart::SelfTestEntry { hours: Some(5), ..a.clone() };
        let before = vec![a.clone(), b.clone()];
        let start = log_state(&before);
        // Nothing new.
        assert!(!has_new_entry(Some(&start), &before));
        // The same result again within the hour: the log grew.
        assert!(has_new_entry(Some(&start), &[a.clone(), a.clone(), b.clone()]));
        // A different result.
        let failed = smart::SelfTestEntry { status: "Completed: read failure".into(), ..a.clone() };
        assert!(has_new_entry(Some(&start), &[failed, a.clone()]));
        // A full log that rotated: same length, the old newest entry moved down.
        let full: Vec<_> = std::iter::repeat_n(b.clone(), 20).chain([a.clone()]).collect();
        let mut rotated = full.clone();
        rotated.pop();
        rotated.insert(0, full[0].clone());
        assert!(has_new_entry(Some(&log_state(&full)), &rotated));
        // An empty log at the start.
        assert!(has_new_entry(Some(&log_state(&[])), std::slice::from_ref(&a)));
        assert!(!has_new_entry(Some(&log_state(&[])), &[]));
        // Unknown start state: take the newest entry.
        assert!(has_new_entry(None, &[a]));
    }
}
