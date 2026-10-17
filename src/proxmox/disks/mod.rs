//! Disk health and storage pools of the Proxmox nodes.
//!
//! In the background Nodestarr reads:
//! - every physical disk's SMART data, kept as history (temperature,
//!   reallocated and pending sectors, cable errors, wear …) and judged into a
//!   verdict with plain-language findings ([`smart`]);
//! - ZFS pools and LVM thin pools through the Proxmox API, and mdadm and
//!   SnapRAID arrays through the host helper, with scrub, resilver and
//!   rebuild progress and the redundancy left ([`pools`]);
//! - SMART self-tests started by hand or on schedules ([`selftest`]).
//!
//! The Proxmox API can read SMART data but always wakes sleeping disks, and
//! can't run self-tests. With host access ([`host`]) Nodestarr runs a small
//! helper over SSH for that, leaving spun-down disks asleep.
//!
//! Disks are identified by model and serial number, so their history follows
//! them across `/dev/sdX` renames, controller changes and nodes.

pub mod blockdev;
pub mod host;
pub mod pools;
mod scan;
pub mod selftest;
pub mod smart;
mod store;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Notify;

use crate::activity::{Actor, NewActivity};
use crate::app::App;
use crate::error::{AppError, Result};
use crate::events::{Event, EventBus};
use crate::settings::Severity;

pub use scan::sample;
pub use store::{EventRow, HistoryPoint};

use self::host::{HostRunner, NodeAccess};
use self::pools::{Pool, PoolKind};
use self::smart::{Finding, Level, Media, Report, Source, SpinDown, TestOutcome};

/// Where a disk sits and what to remember about it; entered by hand.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Label {
    /// Its slot in the case ("Bay 3", "top left").
    pub bay: String,
    pub notes: String,
    /// Dates as "YYYY-MM-DD".
    pub purchased: String,
    pub warranty_until: String,
}

/// A disk's place in a pool.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Role {
    pub pool: String,
    pub pool_name: String,
    pub kind: PoolKind,
    /// data, parity, spare, log, cache, special, dedup.
    pub role: String,
    /// Its vdev or group ("raidz2-0", "md0").
    pub group: String,
    /// Its name in the pool ("d1" and "parity 2" in SnapRAID).
    pub member: String,
    pub state: Option<String>,
    pub ok: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LastTest {
    pub kind: String,
    pub outcome: TestOutcome,
    pub status: String,
    /// When it ended: from Nodestarr's records, or estimated from the
    /// drive's power-on hours (`estimated`).
    pub at: Option<i64>,
    pub estimated: bool,
    pub lba: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TestSupport {
    pub short: bool,
    pub long: bool,
    pub conveyance: bool,
    /// The drive's own estimates, in minutes.
    pub short_minutes: Option<u32>,
    pub long_minutes: Option<u32>,
    pub conveyance_minutes: Option<u32>,
    /// Self-tests can be started now (host access works on the node).
    pub available: bool,
    /// Why they can't, when they can't.
    pub reason: Option<String>,
}

/// A physical disk as the UI sees it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiskView {
    pub id: String,
    pub node: String,
    pub devpath: String,
    pub by_id: Option<String>,
    pub model: Option<String>,
    pub family: Option<String>,
    pub serial: Option<String>,
    pub wwn: Option<String>,
    pub vendor: Option<String>,
    pub firmware: Option<String>,
    pub media: Media,
    /// sata, sas, nvme or usb.
    pub transport: Option<String>,
    pub size: Option<u64>,
    pub rpm: Option<u32>,
    /// What Proxmox says uses the disk ("ZFS", "LVM", "partitions" …).
    pub used_by: Option<String>,
    /// Ceph OSD on this disk.
    pub osd: Option<i64>,
    pub roles: Vec<Role>,
    pub mounts: Vec<String>,
    /// Seen at the last successful scan of its node.
    pub present: bool,
    pub first_seen: i64,
    pub last_seen: i64,
    /// The last successful SMART read, and the last attempt (asleep or not).
    pub sampled_at: Option<i64>,
    pub checked_at: Option<i64>,
    pub source: Option<Source>,
    pub asleep: bool,
    /// Whether it spins down between readings (learned from its start count).
    pub spin: SpinDown,
    /// Read through the Proxmox API only about once a day, because every
    /// reading wakes it.
    pub resting: bool,
    pub error: Option<String>,
    pub passed: Option<bool>,
    pub verdict: Level,
    pub findings: Vec<Finding>,
    pub temperature: Option<f64>,
    pub temp_limit: Option<f64>,
    pub power_on_hours: Option<u64>,
    pub power_cycles: Option<u64>,
    /// Reallocated sectors (ATA) or grown defects (SAS).
    pub reallocated: Option<u64>,
    pub pending: Option<u64>,
    /// Offline uncorrectable sectors (ATA), media errors (NVMe), uncorrected errors (SAS).
    pub uncorrectable: Option<u64>,
    pub crc_errors: Option<u64>,
    pub wear_used: Option<f64>,
    pub bytes_written: Option<u64>,
    pub load_cycles: Option<u64>,
    pub tests: TestSupport,
    pub last_test: Option<LastTest>,
    pub label: Label,
    /// A known drive-managed SMR model.
    pub smr: bool,
    /// Temperatures of the last day (oldest first), for sparklines.
    pub temps: Vec<Option<f64>>,
}

/// The state of background reading.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Sampling {
    pub running: bool,
    /// Last pool refresh and last SMART read.
    pub last_run: Option<i64>,
    pub last_smart: Option<i64>,
    pub next_smart: Option<i64>,
    pub error: Option<String>,
    /// Nodes that couldn't be read at the last attempt.
    pub node_errors: BTreeMap<String, String>,
}

#[derive(Default)]
struct State {
    disks: BTreeMap<String, DiskView>,
    reports: HashMap<String, Arc<Report>>,
    /// The latest raw smartctl JSON per disk (memory only), for download.
    raw: HashMap<String, Arc<Value>>,
    pools: Vec<Pool>,
    nodes: HashMap<String, scan::NodeMemo>,
    sampling: Sampling,
    loaded: bool,
}

#[derive(Clone)]
pub struct DiskService {
    inner: Arc<Inner>,
}

struct Inner {
    bus: EventBus,
    host: HostRunner,
    state: RwLock<State>,
    /// One read of the nodes at a time.
    sample_lock: tokio::sync::Mutex<()>,
    /// Wakes the self-test runner (a test was queued or aborted).
    tests_wake: Notify,
    /// Node name → cluster address, and when it was fetched.
    cluster: Mutex<Option<(i64, HashMap<String, String>)>>,
}

impl DiskService {
    pub fn new(bus: EventBus, data_dir: &std::path::Path) -> Self {
        Self {
            inner: Arc::new(Inner {
                bus,
                host: HostRunner::new(data_dir),
                state: RwLock::new(State::default()),
                sample_lock: tokio::sync::Mutex::new(()),
                tests_wake: Notify::new(),
                cluster: Mutex::new(None),
            }),
        }
    }

    pub fn host(&self) -> &HostRunner {
        &self.inner.host
    }

    /// Every known disk: present ones first, by node and device.
    pub fn disks(&self) -> Vec<DiskView> {
        let mut v: Vec<DiskView> = self.inner.state.read().disks.values().cloned().collect();
        v.sort_by(|a, b| {
            b.present.cmp(&a.present).then_with(|| a.node.cmp(&b.node)).then_with(|| natural(&a.devpath, &b.devpath))
        });
        v
    }

    pub fn disk(&self, id: &str) -> Option<DiskView> {
        self.inner.state.read().disks.get(id).cloned()
    }

    pub fn report(&self, id: &str) -> Option<Arc<Report>> {
        self.inner.state.read().reports.get(id).cloned()
    }

    pub fn raw(&self, id: &str) -> Option<Arc<Value>> {
        self.inner.state.read().raw.get(id).cloned()
    }

    pub fn pools(&self) -> Vec<Pool> {
        self.inner.state.read().pools.clone()
    }

    pub fn pool(&self, id: &str) -> Option<Pool> {
        self.inner.state.read().pools.iter().find(|p| p.id == id).cloned()
    }

    pub fn sampling(&self) -> Sampling {
        self.inner.state.read().sampling.clone()
    }

    /// Wakes the self-test runner.
    pub fn wake_tests(&self) {
        self.inner.tests_wake.notify_one();
    }

    fn publish(&self) {
        self.inner.bus.publish(Event::Disks);
    }
}

/// Kernel order: "sda" < "sdb" < "sdaa", then "nvme2n1" < "nvme10n1".
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    fn key(path: &str) -> (u8, String, usize, String, u64, u64) {
        let name = path.trim_start_matches("/dev/");
        for prefix in ["xvd", "sd", "hd", "vd"] {
            if let Some(rest) = name.strip_prefix(prefix)
                && !rest.is_empty()
                && rest.chars().all(|c| c.is_ascii_lowercase())
            {
                return (0, prefix.to_string(), rest.len(), rest.to_string(), 0, 0);
            }
        }
        if let Some(rest) = name.strip_prefix("nvme") {
            let mut parts = rest.split('n').map(|p| p.parse::<u64>().unwrap_or(0));
            return (1, String::new(), 0, String::new(), parts.next().unwrap_or(0), parts.next().unwrap_or(0));
        }
        (2, name.to_string(), 0, String::new(), 0, 0)
    }
    key(a).cmp(&key(b))
}

/// Loads what's known about the disks from the database.
async fn load(app: &App) {
    match store::load_disks(&app.db).await {
        Ok(rows) => {
            let mut state = app.disks.inner.state.write();
            for row in rows {
                if let Some(r) = row.report {
                    state.reports.insert(row.view.id.clone(), Arc::new(r));
                }
                state.disks.insert(row.view.id.clone(), row.view);
            }
            state.loaded = true;
        }
        Err(err) => tracing::warn!(%err, "couldn't load disk health history"),
    }
}

/// Starts background reading, the self-test runner and test schedules.
pub fn start(app: App) {
    tokio::spawn(async move {
        load(&app).await;
        selftest::start(app.clone());
        // Give the API connection a moment after startup.
        tokio::time::sleep(Duration::from_secs(40)).await;
        let mut next_smart = 0i64;
        let mut next_pools = 0i64;
        let mut next_prune = 0i64;
        loop {
            let s = app.settings.get();
            let enabled = s.proxmox.enabled && s.proxmox.disks.enabled;
            let now = crate::util::now();
            if enabled {
                let smart = now >= next_smart;
                if smart || now >= next_pools {
                    if let Err(err) = sample(&app, smart).await {
                        tracing::debug!(%err, "disk health read failed");
                    }
                    let now = crate::util::now();
                    if smart {
                        next_smart = now + i64::from(s.proxmox.disks.sample_minutes.max(5)) * 60;
                        app.disks.inner.state.write().sampling.next_smart = Some(next_smart);
                    }
                    next_pools = now + 300;
                }
                if now >= next_prune {
                    let cutoff = now - i64::from(s.proxmox.disks.retention_days.max(7)) * 86400;
                    if let Err(err) = store::prune(&app.db, cutoff).await {
                        tracing::warn!(%err, "disk history cleanup failed");
                    }
                    next_prune = now + 86400;
                }
            }
            tokio::time::sleep(Duration::from_secs(20)).await;
        }
    });
}

/// Runs a full read now, in the background. False when one is already running.
pub fn refresh(app: &App) -> bool {
    if app.disks.inner.sample_lock.try_lock().is_err() {
        return false;
    }
    let app = app.clone();
    tokio::spawn(async move {
        if let Err(err) = sample(&app, true).await {
            tracing::info!(%err, "disk health read failed");
        }
    });
    true
}

/// The address to reach a node over SSH: the configured one, else its
/// cluster address, else (for the node the API talks to) the API's host.
pub async fn node_address(app: &App, node: &str) -> Option<String> {
    let s = app.settings.get();
    if let Some(a) = s.proxmox.host_access.addresses.get(node).map(|a| a.trim()).filter(|a| !a.is_empty()) {
        return Some(a.to_string());
    }
    if let Some(ip) = cluster_addresses(app).await.get(node) {
        return Some(ip.clone());
    }
    let api_node = app.proxmox.cached_overview().map(|o| o.node.node.clone());
    if api_node.as_deref() == Some(node) || api_node.is_none() {
        return url::Url::parse(&s.proxmox.url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.trim_matches(['[', ']']).to_string()));
    }
    None
}

/// Node addresses from `GET /cluster/status` (cached for 10 minutes).
async fn cluster_addresses(app: &App) -> HashMap<String, String> {
    let now = crate::util::now();
    if let Some((at, map)) = app.disks.inner.cluster.lock().as_ref()
        && now - at < 600
    {
        return map.clone();
    }
    let mut map = HashMap::new();
    if let Ok(c) = app.proxmox.client()
        && let Ok(list) = c.get::<Vec<Value>>("/cluster/status").await
    {
        for n in list.iter().filter(|n| n["type"].as_str() == Some("node")) {
            if let (Some(name), Some(ip)) = (n["name"].as_str(), n["ip"].as_str()) {
                map.insert(name.to_string(), ip.to_string());
            }
        }
    }
    *app.disks.inner.cluster.lock() = Some((now, map.clone()));
    map
}

/// Connects to a node with host access, pinning its SSH host key if none is
/// pinned yet, and reads what the helper can do there.
pub async fn test_access(app: &App, node: &str) -> Result<NodeAccess> {
    let s = app.settings.get();
    let address = node_address(app, node)
        .await
        .ok_or_else(|| AppError::bad_request(format!("set an SSH address for {node} first")))?;
    let info = app.disks.host().info(&s.proxmox.host_access, node, &address, true).await?;
    refresh(app);
    Ok(info)
}

/// Starts, pauses or stops a ZFS scrub (host access).
pub async fn scrub(app: &App, pool_id: &str, action: &str, actor: &Actor) -> Result<()> {
    if !matches!(action, "start" | "pause" | "stop") {
        return Err(AppError::bad_request("the scrub action is start, pause or stop"));
    }
    let pool = app.disks.pool(pool_id).ok_or_else(|| AppError::not_found(format!("pool {pool_id}")))?;
    if pool.kind != PoolKind::Zfs {
        return Err(AppError::bad_request("only ZFS pools can be scrubbed from here"));
    }
    let out = helper_call(app, &pool.node, vec!["zpool-scrub".into(), pool.name.clone(), action.into()], 30).await?;
    if out.code != 0 {
        return Err(AppError::upstream(format!("zpool scrub failed: {}", out.message())));
    }
    let verb = match action {
        "start" => "started",
        "pause" => "paused",
        _ => "stopped",
    };
    let message = format!("Scrub of {} on {} {verb}", pool.name, pool.node);
    store::insert_events(
        &app.db,
        crate::util::now(),
        vec![store::NewEvent::new(format!("pool:{}", pool.id), Severity::Info, "scrub", format!("Scrub {verb}"))],
    )
    .await?;
    app.note(NewActivity::new("pool.scrub", Severity::Info, message).actor(actor)).await;
    let app2 = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let _ = sample(&app2, false).await;
    });
    Ok(())
}

/// Reads from a disk for a while so its activity light blinks (host access).
pub async fn identify(app: &App, id: &str, seconds: u32, actor: &Actor) -> Result<()> {
    let d = app.disks.disk(id).ok_or_else(|| AppError::not_found(format!("disk {id}")))?;
    if !d.present {
        return Err(AppError::conflict("this disk isn't connected right now"));
    }
    let seconds = seconds.clamp(5, 300);
    let out = helper_call(app, &d.node, vec!["identify".into(), d.devpath.clone(), seconds.to_string()], 20).await?;
    if out.code != 0 {
        return Err(AppError::upstream(out.message()));
    }
    app.note(
        NewActivity::new(
            "disk.identify",
            Severity::Info,
            format!("Blinking {} ({}) on {} for {seconds}s", d.devpath, disk_name(&d), d.node),
        )
        .actor(actor),
    )
    .await;
    Ok(())
}

/// Runs a helper command on a node, checking host access first.
async fn helper_call(app: &App, node: &str, args: Vec<String>, timeout_secs: u64) -> Result<host::Output> {
    let s = app.settings.get();
    if !s.proxmox.host_access.enabled {
        return Err(AppError::bad_request("turn on host access in Settings > Proxmox first"));
    }
    let address =
        node_address(app, node).await.ok_or_else(|| AppError::bad_request(format!("no SSH address for {node}")))?;
    app.disks.host().run(&s.proxmox.host_access, node, &address, &args, Duration::from_secs(timeout_secs), false).await
}

pub async fn set_label(app: &App, id: &str, label: Label) -> Result<DiskView> {
    let clean = |s: &str, max: usize| crate::util::truncate(s.trim(), max);
    let date_ok = |s: &str| s.is_empty() || chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok();
    let label = Label {
        bay: clean(&label.bay, 60),
        notes: clean(&label.notes, 2000),
        purchased: label.purchased.trim().to_string(),
        warranty_until: label.warranty_until.trim().to_string(),
    };
    if !date_ok(&label.purchased) || !date_ok(&label.warranty_until) {
        return Err(AppError::bad_request("dates look like 2024-03-14"));
    }
    if !store::save_label(&app.db, id.to_string(), &label).await? {
        return Err(AppError::not_found(format!("disk {id}")));
    }
    let view = {
        let mut state = app.disks.inner.state.write();
        let d = state.disks.get_mut(id).ok_or_else(|| AppError::not_found(format!("disk {id}")))?;
        d.label = label;
        d.clone()
    };
    app.disks.publish();
    Ok(view)
}

/// Deletes a disk that's gone, with its history.
pub async fn forget(app: &App, id: &str, actor: &Actor) -> Result<()> {
    let d = app.disks.disk(id).ok_or_else(|| AppError::not_found(format!("disk {id}")))?;
    if d.present {
        return Err(AppError::conflict("this disk is still connected; only disks that are gone can be forgotten"));
    }
    store::forget(&app.db, id.to_string()).await?;
    {
        let mut state = app.disks.inner.state.write();
        state.disks.remove(id);
        state.reports.remove(id);
        state.raw.remove(id);
    }
    app.note(
        NewActivity::new("disk.inventory", Severity::Info, format!("Forgot {} and its history", disk_name(&d)))
            .actor(actor),
    )
    .await;
    app.disks.publish();
    Ok(())
}

pub async fn history(app: &App, id: &str, days: u32) -> Result<Vec<HistoryPoint>> {
    let days = days.clamp(1, 3650);
    let since = crate::util::now() - i64::from(days) * 86400;
    // About 400 points whatever the range: hourly for two weeks, coarser beyond.
    let bucket = (i64::from(days) * 86400 / 400).max(3600);
    store::history(&app.db, id.to_string(), since, bucket).await
}

pub async fn pool_history(app: &App, id: &str, days: u32) -> Result<Vec<(i64, Option<i64>, Option<i64>)>> {
    let since = crate::util::now() - i64::from(days.clamp(1, 3650)) * 86400;
    store::pool_history(&app.db, id.to_string(), since).await
}

/// Self-tests: the queued and running ones, or finished ones (newest first).
pub async fn tests(app: &App, active: bool, disk: Option<String>, limit: u32) -> Result<Vec<selftest::TestRun>> {
    if active { store::active_tests(&app.db).await } else { store::recent_tests(&app.db, disk, limit).await }
}

pub async fn events(app: &App, subject: Option<&str>, limit: u32) -> Result<Vec<EventRow>> {
    store::events(&app.db, subject.map(str::to_string), limit).await
}

/// "WDC WD40EFRX (WD-WCC7K…)".
pub fn disk_name(d: &DiskView) -> String {
    let model = d.model.clone().unwrap_or_else(|| d.devpath.clone());
    match d.serial.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => format!("{model} ({s})"),
        None => model,
    }
}

/// Alert-worthy findings: everything at warning or worse except
/// temperature (which has its own alert).
pub fn health_problem(d: &DiskView) -> Option<&Finding> {
    d.findings.iter().find(|f| f.level >= Level::Warning && !f.code.starts_with("temperature"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_device_order() {
        let mut v = vec!["/dev/sdaa", "/dev/sdb", "/dev/sda", "/dev/nvme10n1", "/dev/nvme2n1"];
        v.sort_by(|a, b| natural(a, b));
        assert_eq!(v, ["/dev/sda", "/dev/sdb", "/dev/sdaa", "/dev/nvme2n1", "/dev/nvme10n1"]);
    }

    #[test]
    fn names_and_problems() {
        let mut d = DiskView { devpath: "/dev/sda".into(), model: Some("WDC WD40EFRX".into()), ..Default::default() };
        assert_eq!(disk_name(&d), "WDC WD40EFRX");
        d.serial = Some("WD-1".into());
        assert_eq!(disk_name(&d), "WDC WD40EFRX (WD-1)");
        d.findings = vec![
            Finding { level: Level::Warning, code: "temperature".into(), title: "hot".into(), detail: String::new() },
            Finding { level: Level::Watch, code: "reallocated".into(), title: "8".into(), detail: String::new() },
        ];
        assert!(health_problem(&d).is_none());
        d.findings.push(Finding {
            level: Level::Warning,
            code: "pending".into(),
            title: "2".into(),
            detail: String::new(),
        });
        assert_eq!(health_problem(&d).unwrap().code, "pending");
    }
}
