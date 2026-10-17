//! Reading the nodes: their disks, SMART data and pools, turned into the
//! views, history, findings and events the disk service serves.
//!
//! A read never fails as a whole because one node, pool or disk did: what
//! couldn't be read keeps its last known state (marked with the error).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use super::blockdev::{self, DiskRef, Topology};
use super::pools::{self, Pool, PoolHealth, PoolKind, PoolRules};
use super::smart::{self, Finding, Level, Media, Report, SelfTestEntry, SpinDown, TestOutcome};
use super::store::{self, NewEvent, Sample};
use super::{DiskView, LastTest, Role, TestSupport, disk_name, node_address};
use crate::activity::NewActivity;
use crate::app::App;
use crate::error::Result;
use crate::proxmox::client::PveClient;
use crate::settings::{AlertKind, Settings, Severity};

/// What's remembered about a node between reads.
#[derive(Debug, Clone, Default)]
pub(super) struct NodeMemo {
    pub topology: Option<Arc<Topology>>,
    /// The node's offset from UTC, for dates in `zpool status`.
    pub tz_offset: i64,
    /// The last `snapraid status` output and when it ran.
    pub snapraid_status: Option<(i64, String)>,
}

/// A disk from `GET /nodes/{node}/disks/list`.
#[derive(Debug, Clone, Default)]
struct PveDisk {
    devpath: String,
    model: Option<String>,
    serial: Option<String>,
    vendor: Option<String>,
    wwn: Option<String>,
    size: Option<u64>,
    /// hdd, ssd, nvme, usb, unknown.
    kind: String,
    used: Option<String>,
    rpm: Option<u32>,
    by_id: Option<String>,
    osd: Option<i64>,
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty() && *s != "unknown" && *s != "N/A").map(str::to_string)
}

fn num(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn pve_disk(v: &Value) -> Option<PveDisk> {
    let devpath = v["devpath"].as_str()?.to_string();
    // Partitions (include-partitions=1) are only needed for mapping.
    if v["parent"].is_string() || v["type"].as_str() == Some("partition") {
        return None;
    }
    Some(PveDisk {
        devpath,
        model: text(&v["model"]),
        serial: text(&v["serial"]),
        vendor: text(&v["vendor"]),
        wwn: text(&v["wwn"]),
        size: num(&v["size"]),
        kind: v["type"].as_str().unwrap_or("unknown").to_string(),
        used: text(&v["used"]),
        rpm: num(&v["rpm"]).and_then(|r| u32::try_from(r).ok()),
        by_id: text(&v["by_id_link"]),
        osd: v["osdid"].as_i64().filter(|o| *o >= 0),
    })
}

/// Serial numbers some USB bridges and cheap disks report for every unit.
fn usable_serial(s: &str) -> bool {
    let s = s.trim();
    let mut chars = s.chars();
    let first = chars.next();
    s.len() >= 4
        && !chars.all(|c| Some(c) == first)
        && !["0123456789ABCDEF", "123456789012", "To be filled by O.E.M.", "Default string"].contains(&s)
}

fn clean_id(s: &str) -> String {
    s.trim().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect()
}

/// A disk's stable id: model and serial when the serial is usable, else
/// its WWN, else node and device (which can change).
pub(super) fn disk_id(
    model: Option<&str>,
    serial: Option<&str>,
    wwn: Option<&str>,
    node: &str,
    devpath: &str,
) -> String {
    match serial.filter(|s| usable_serial(s)) {
        Some(sn) => format!("{}_{}", clean_id(model.unwrap_or("disk")), clean_id(sn)),
        None => match wwn.map(str::trim).filter(|w| w.len() > 4) {
            Some(w) => format!("wwn-{}", clean_id(w)),
            None => format!("{}_{}", clean_id(node), clean_id(devpath.trim_start_matches("/dev/"))),
        },
    }
}

enum Read {
    Ok(Box<Report>, Option<Value>),
    Asleep,
    Failed(String),
}

#[derive(Default)]
struct NodeScan {
    node: String,
    /// The disk list couldn't be read: leave this node's disks as they were.
    error: Option<String>,
    disks: Vec<PveDisk>,
    /// SMART reads by device path (only on SMART rounds).
    reads: HashMap<String, Read>,
    pools: Vec<Pool>,
    topology: Option<Arc<Topology>>,
    helper: bool,
    tz_offset: i64,
    snapraid_status: Option<(i64, String)>,
}

/// Reads all online nodes: pools always, SMART data when `smart` is set.
pub async fn sample(app: &App, smart: bool) -> Result<()> {
    let _guard = app.disks.inner.sample_lock.lock().await;
    let settings = app.settings.get();
    if !settings.proxmox.enabled || !settings.proxmox.disks.enabled {
        return Ok(());
    }
    app.disks.inner.state.write().sampling.running = true;
    app.disks.publish();
    let result = run(app, &settings, smart).await;
    {
        let mut st = app.disks.inner.state.write();
        st.sampling.running = false;
        st.sampling.error = result.as_ref().err().map(|e| e.to_string());
    }
    app.disks.publish();
    result
}

async fn run(app: &App, settings: &Settings, smart: bool) -> Result<()> {
    let now = crate::util::now();
    let overview = app.proxmox.overview(60).await?;
    let mut nodes: Vec<String> = overview.nodes.iter().filter(|n| n.online).map(|n| n.node.clone()).collect();
    if nodes.is_empty() {
        nodes.push(overview.node.node.clone());
    }
    let c = app.proxmox.client()?;
    let mut scans = Vec::with_capacity(nodes.len());
    for node in &nodes {
        scans.push(scan_node(app, &c, settings, node, smart, now).await);
    }
    apply(app, settings, scans, smart, now).await
}

fn enc(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

/// Makes sure the node's helper answered recently; true when it's usable.
async fn ensure_helper(app: &App, settings: &Settings, node: &str) -> bool {
    let host = app.disks.host();
    let now = crate::util::now();
    let stale = match host.node(node) {
        None => true,
        Some(n) if !n.ok => n.checked_at.is_none_or(|t| now - t > 600),
        Some(n) => n.helper.is_none() || n.checked_at.is_none_or(|t| now - t > 3600),
    };
    if stale
        && let Some(address) = node_address(app, node).await
        && let Err(err) = host.info(&settings.proxmox.host_access, node, &address, false).await
    {
        tracing::debug!(%err, node, "host helper unavailable");
    }
    host.usable(node)
}

async fn scan_node(app: &App, c: &PveClient, settings: &Settings, node: &str, smart: bool, now: i64) -> NodeScan {
    let memo = app.disks.inner.state.read().nodes.get(node).cloned().unwrap_or_default();
    let mut scan = NodeScan {
        node: node.to_string(),
        tz_offset: memo.tz_offset,
        snapraid_status: memo.snapraid_status.clone(),
        ..Default::default()
    };
    match c.get::<Vec<Value>>(&format!("/nodes/{node}/disks/list?include-partitions=1&skipsmart=1")).await {
        Ok(list) => scan.disks = list.iter().filter_map(pve_disk).collect(),
        Err(err) => {
            scan.error = Some(err.to_string());
            return scan;
        }
    }
    if let Ok(t) = c.get::<Value>(&format!("/nodes/{node}/time")).await
        && let (Some(local), Some(utc)) = (t["localtime"].as_i64(), t["time"].as_i64())
    {
        scan.tz_offset = local - utc;
    }
    match c.get::<Vec<Value>>(&format!("/nodes/{node}/disks/zfs")).await {
        Ok(list) => {
            for entry in &list {
                let Some(name) = entry["name"].as_str() else { continue };
                match c.get::<Value>(&format!("/nodes/{node}/disks/zfs/{}", enc(name))).await {
                    Ok(detail) => scan.pools.push(pools::zfs_pool(node, entry, &detail, scan.tz_offset)),
                    Err(err) => tracing::debug!(%err, node, pool = name, "zpool status failed"),
                }
            }
        }
        Err(err) => tracing::debug!(%err, node, "no ZFS pools"),
    }
    if let Ok(thin) = c.get::<Value>(&format!("/nodes/{node}/disks/lvmthin")).await {
        let lvm = c.get::<Value>(&format!("/nodes/{node}/disks/lvm")).await.unwrap_or(Value::Null);
        scan.pools.extend(pools::lvm_thin_pools(node, &thin, &lvm));
    }

    let ha = &settings.proxmox.host_access;
    let set_up = ha.enabled && app.disks.host().has_key();
    scan.helper = set_up && ensure_helper(app, settings, node).await;
    if scan.helper {
        helper_reads(app, settings, &mut scan, &memo, now).await;
    }
    if scan.topology.is_none() {
        scan.topology = memo.topology.clone();
    }
    if smart {
        scan.reads = if scan.helper {
            helper_smart(app, settings, node, &scan.disks).await
        } else {
            let resting =
                if settings.proxmox.disks.wake_sleeping { Vec::new() } else { resting(app, node, now, set_up) };
            let due: Vec<PveDisk> = scan
                .disks
                .iter()
                .filter(|d| !resting.iter().any(|(dev, serial)| *dev == d.devpath && *serial == d.serial))
                .cloned()
                .collect();
            api_smart(c, node, &due).await
        };
    }
    scan
}

/// How often a disk that sleeps between readings is read through the API.
const RESTING_SECS: i64 = 23 * 3600;

/// Disks of a node to leave alone this time, as (device path, serial). Every
/// read through the Proxmox API wakes a sleeping disk, so those that spin down
/// between readings are read about once a day. With host access set up but
/// not answering right now, disks that were asleep at its last look stay
/// alone until it answers again (without it, they're read daily too).
fn resting(app: &App, node: &str, now: i64, helper_set_up: bool) -> Vec<(String, Option<String>)> {
    app.disks
        .inner
        .state
        .read()
        .disks
        .values()
        .filter(|v| v.present && v.node == node)
        .filter(|v| {
            let recent = v.sampled_at.is_some_and(|t| now - t < RESTING_SECS);
            (v.spin.sleeps && recent) || (v.asleep && (helper_set_up || recent))
        })
        .map(|v| (v.devpath.clone(), v.serial.clone()))
        .collect()
}

/// Block devices, md arrays and SnapRAID through the host helper.
async fn helper_reads(app: &App, settings: &Settings, scan: &mut NodeScan, memo: &NodeMemo, now: i64) {
    let ha = &settings.proxmox.host_access;
    let host = app.disks.host();
    let node = scan.node.clone();
    let Some(address) = node_address(app, &node).await else { return };
    let run = |args: &[&str], secs: u64| {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let address = address.clone();
        let node = node.clone();
        async move { host.run(ha, &node, &address, &args, Duration::from_secs(secs), false).await }
    };
    match run(&["lsblk"], 30).await {
        Ok(out) if out.code == 0 => match out.json() {
            Ok(v) => scan.topology = Some(Arc::new(Topology::from_helper(&v["lsblk"], &v["links"], &v["mounts"]))),
            Err(err) => tracing::debug!(%err, node, "lsblk output unreadable"),
        },
        Ok(out) => tracing::debug!(node, "lsblk failed: {}", out.message()),
        Err(err) => tracing::debug!(%err, node, "lsblk failed"),
    }
    let info = host.node(&node).unwrap_or_default();
    if info.md_arrays
        && let Ok(out) = run(&["mdstat"], 20).await
        && out.code == 0
    {
        scan.pools.extend(pools::parse_mdstat(&node, &out.stdout));
    }
    if info.snapraid_conf {
        // `snapraid status` reads the whole state into memory: every 6 hours is plenty.
        let want_status = memo.snapraid_status.as_ref().is_none_or(|(at, _)| now - at > 6 * 3600);
        let args: &[&str] = if want_status { &["snapraid", "status"] } else { &["snapraid"] };
        match run(args, if want_status { 960 } else { 30 }).await {
            Ok(out) if out.code == 0 => {
                let sections = sections(&out.stdout);
                let busy = sections.get("busy").is_some_and(|b| b.trim() == "yes");
                if let Some(status) = sections.get("status").filter(|s| s.trim() != "busy" && !s.trim().is_empty()) {
                    scan.snapraid_status = Some((now, status.clone()));
                }
                let status = scan.snapraid_status.as_ref().map(|(_, s)| s.as_str());
                if let Some(conf) = sections.get("conf")
                    && let Some(mut p) = pools::snapraid_pool(&node, conf, status)
                {
                    if let Some(sr) = p.snapraid.as_mut() {
                        sr.busy = busy;
                        sr.status_at = scan.snapraid_status.as_ref().map(|(at, _)| *at);
                        sr.last_sync = sections
                            .get("content")
                            .into_iter()
                            .flat_map(|c| c.lines())
                            .filter_map(|l| l.split_whitespace().next()?.parse::<i64>().ok())
                            .max();
                    }
                    scan.pools.push(p);
                }
            }
            Ok(out) => tracing::debug!(node, "snapraid failed: {}", out.message()),
            Err(err) => tracing::debug!(%err, node, "snapraid failed"),
        }
    }
}

/// Splits the helper's "##name" sections.
fn sections(text: &str) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("##") {
            current = Some(name.trim().to_string());
            out.entry(name.trim().to_string()).or_default();
        } else if let Some(name) = &current {
            let s = out.entry(name.clone()).or_default();
            s.push_str(line);
            s.push('\n');
        }
    }
    out
}

/// SMART data through the helper, a few disks per call. Sleeping disks stay
/// asleep unless the settings say otherwise.
async fn helper_smart(app: &App, settings: &Settings, node: &str, disks: &[PveDisk]) -> HashMap<String, Read> {
    let mut out = HashMap::new();
    let Some(address) = node_address(app, node).await else { return out };
    let now = crate::util::now();
    for chunk in disks.chunks(8) {
        let mut args = vec!["smart".to_string()];
        if settings.proxmox.disks.wake_sleeping {
            args.push("--wake".into());
        }
        args.extend(chunk.iter().map(|d| d.devpath.clone()));
        let timeout = Duration::from_secs(30 + 20 * chunk.len() as u64);
        let res = app.disks.host().run(&settings.proxmox.host_access, node, &address, &args, timeout, false).await;
        let entries = match res
            .and_then(|o| if o.code == 0 { o.json() } else { Err(crate::error::AppError::upstream(o.message())) })
        {
            Ok(Value::Array(entries)) => entries,
            Ok(_) => Vec::new(),
            Err(err) => {
                for d in chunk {
                    out.insert(d.devpath.clone(), Read::Failed(err.to_string()));
                }
                continue;
            }
        };
        for e in entries {
            let Some(dev) = e["device"].as_str() else { continue };
            let code = e["exit"].as_i64().unwrap_or(-1);
            let read = match &e["data"] {
                _ if code == 15 => Read::Asleep,
                Value::Null => Read::Failed(if code == 124 {
                    "smartctl didn't answer in time".into()
                } else {
                    format!("smartctl failed (exit status {code})")
                }),
                data => {
                    let report = smart::from_smartctl(data, now);
                    // Bits 0 and 1: smartctl couldn't run or open the device at all.
                    if code & 0b11 != 0 && report.model.is_none() && report.passed.is_none() {
                        let why = report
                            .messages
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "smartctl couldn't open the device".into());
                        Read::Failed(why)
                    } else {
                        Read::Ok(Box::new(report), Some(data.clone()))
                    }
                }
            };
            out.insert(dev.to_string(), read);
        }
    }
    out
}

/// SMART data through the Proxmox API (wakes sleeping disks).
async fn api_smart(c: &PveClient, node: &str, disks: &[PveDisk]) -> HashMap<String, Read> {
    let now = crate::util::now();
    let mut out = HashMap::new();
    for d in disks {
        let read = match c.get::<Value>(&format!("/nodes/{node}/disks/smart?disk={}", enc(&d.devpath))).await {
            Ok(v) => Read::Ok(Box::new(smart::from_pve(&v, now)), None),
            Err(err) => Read::Failed(err.to_string()),
        };
        out.insert(d.devpath.clone(), read);
    }
    out
}

fn media_of(d: &PveDisk, report: Option<&Report>) -> Media {
    if report.is_some_and(|r| r.protocol == smart::Protocol::Nvme) {
        return Media::Nvme;
    }
    match d.kind.as_str() {
        "nvme" => Media::Nvme,
        "hdd" => Media::Hdd,
        "ssd" => Media::Ssd,
        _ => match report.and_then(|r| r.rpm).or(d.rpm) {
            Some(0) => Media::Ssd,
            Some(_) => Media::Hdd,
            None => Media::Unknown,
        },
    }
}

fn transport_of(d: &PveDisk, report: Option<&Report>) -> Option<String> {
    if d.kind == "usb" {
        return Some("usb".into());
    }
    match report.map(|r| r.protocol) {
        Some(smart::Protocol::Nvme) => Some("nvme".into()),
        Some(smart::Protocol::Scsi) => Some("sas".into()),
        Some(smart::Protocol::Ata) => Some("sata".into()),
        _ if d.kind == "nvme" => Some("nvme".into()),
        _ => None,
    }
}

/// The newest finished test: the drive's own log (which also has tests
/// Nodestarr didn't start), or Nodestarr's record of its last test.
fn last_test(report: Option<&Report>, ours: Option<&super::selftest::TestRun>) -> Option<LastTest> {
    let drive = report.and_then(|r| {
        let e =
            r.self_tests.log.iter().find(|e| e.outcome != TestOutcome::Running && e.outcome != TestOutcome::Unknown)?;
        let at = match (r.power_on_hours, e.hours) {
            (Some(now), Some(h)) if now >= h => Some(r.collected_at - (now - h) as i64 * 3600),
            _ => None,
        };
        Some(LastTest {
            kind: e.kind.clone(),
            outcome: e.outcome,
            status: e.status.clone(),
            at,
            estimated: true,
            lba: e.lba,
        })
    });
    let ours = ours.and_then(|t| {
        let outcome = match t.status.as_str() {
            "passed" => TestOutcome::Passed,
            "failed" => TestOutcome::Failed,
            "aborted" => TestOutcome::Aborted,
            "interrupted" => TestOutcome::Interrupted,
            _ => return None,
        };
        Some(LastTest {
            kind: t.kind.clone(),
            outcome,
            status: t.result.clone().unwrap_or_default(),
            at: t.finished_at,
            estimated: false,
            lba: t.lba.and_then(|l| u64::try_from(l).ok()),
        })
    });
    match (drive, ours) {
        // The drive's log entry is the same test when it ended within the
        // hour (power-on hours are whole hours).
        (Some(d), Some(o)) => {
            if d.at.zip(o.at).is_some_and(|(da, oa)| da > oa + 3600) {
                Some(d)
            } else {
                Some(o)
            }
        }
        (d, o) => o.or(d),
    }
}

fn test_support(report: Option<&Report>, helper: bool, settings: &Settings, present: bool) -> TestSupport {
    let st = report.map(|r| r.self_tests.clone()).unwrap_or_default();
    let mut t = TestSupport {
        short: st.short,
        long: st.long,
        conveyance: st.conveyance,
        short_minutes: st.short_minutes,
        long_minutes: st.long_minutes,
        conveyance_minutes: st.conveyance_minutes,
        available: false,
        reason: None,
    };
    t.reason = if !present {
        Some("The disk isn't connected.".into())
    } else if !settings.proxmox.host_access.enabled {
        Some("Proxmox can't start self-tests; set up host access in Settings > Proxmox.".into())
    } else if !helper {
        Some("Host access to this node isn't working right now.".into())
    } else if report.is_some_and(|r| r.source == smart::Source::Api) {
        Some("Waiting for the first read through host access.".into())
    } else if !(t.short || t.long) {
        Some("This disk doesn't support SMART self-tests.".into())
    } else {
        None
    };
    t.available = t.reason.is_none();
    t
}

/// Hourly temperatures of the last day, oldest first.
fn spark(rows: Option<&Vec<(i64, Option<f64>)>>, now: i64) -> Vec<Option<f64>> {
    let mut slots = vec![None; 24];
    for (ts, temp) in rows.into_iter().flatten() {
        let age = now - ts;
        if (0..86400).contains(&age)
            && let Some(t) = temp
        {
            slots[23 - (age / 3600) as usize] = Some(*t);
        }
    }
    slots
}

/// Events for changes between two reports of the same disk.
fn counter_events(id: &str, old: &Report, new: &Report) -> Vec<NewEvent> {
    let mut out = Vec::new();
    let mut push = |sev: Severity, msg: String| out.push(NewEvent::new(id, sev, "attribute", msg));
    match (old.passed, new.passed) {
        (Some(true), Some(false)) => push(Severity::Error, "SMART health check now reports the disk is failing".into()),
        (Some(false), Some(true)) => push(Severity::Success, "SMART health check passes again".into()),
        _ => {}
    }
    let pairs: [(&str, Option<u64>, Option<u64>, bool); 5] = [
        ("Reallocated sectors", old.reallocated.or(old.grown_defects), new.reallocated.or(new.grown_defects), true),
        ("Pending sectors", old.pending, new.pending, true),
        (
            "Uncorrectable errors",
            old.offline_uncorrectable.or(old.media_errors).or(old.uncorrected_errors),
            new.offline_uncorrectable.or(new.media_errors).or(new.uncorrected_errors),
            true,
        ),
        ("Cable (CRC) errors", old.crc_errors, new.crc_errors, false),
        ("Reported uncorrectable reads", old.reported_uncorrect, new.reported_uncorrect, false),
    ];
    for (label, a, b, serious) in pairs {
        if let (Some(a), Some(b)) = (a, b)
            && a != b
        {
            let sev = if b > a { if serious { Severity::Warning } else { Severity::Info } } else { Severity::Info };
            push(sev, format!("{label}: {a} → {b}"));
        }
    }
    if let (Some(a), Some(b)) = (old.wear_used, new.wear_used) {
        for mark in [50.0, 75.0, 90.0, 100.0] {
            if a < mark && b >= mark {
                push(
                    if mark >= 90.0 { Severity::Warning } else { Severity::Info },
                    format!("{mark:.0}% of its rated write endurance used"),
                );
            }
        }
    }
    out
}

fn level_severity(l: Level) -> Severity {
    match l {
        Level::Failing => Severity::Error,
        Level::Warning => Severity::Warning,
        Level::Ok => Severity::Success,
        _ => Severity::Info,
    }
}

/// Least-squares growth of a pool's used space, in bytes a day.
fn growth_per_day(series: &[(i64, i64)]) -> Option<f64> {
    if series.len() < 6 {
        return None;
    }
    let span = series.last()?.0 - series.first()?.0;
    if span < 2 * 86400 {
        return None;
    }
    let n = series.len() as f64;
    let t0 = series[0].0 as f64;
    let (sx, sy) = series.iter().fold((0.0, 0.0), |(sx, sy), (t, u)| (sx + (*t as f64 - t0), sy + *u as f64));
    let (mx, my) = (sx / n, sy / n);
    let (num, den) = series.iter().fold((0.0, 0.0), |(num, den), (t, u)| {
        let dx = *t as f64 - t0 - mx;
        (num + dx * (*u as f64 - my), den + dx * dx)
    });
    (den > 0.0).then(|| num / den * 86400.0)
}

fn scan_label(kind: &str) -> &str {
    match kind {
        "resilver" => "Resilver",
        "expand" => "Expansion",
        "recovery" => "Rebuild",
        "resync" => "Resync",
        "check" => "Check",
        "reshape" => "Reshape",
        _ => "Scrub",
    }
}

/// Events (and activity) for changes of a pool.
fn pool_events(old: Option<&Pool>, new: &Pool, initial: bool) -> (Vec<NewEvent>, Vec<NewActivity>) {
    let subject = format!("pool:{}", new.id);
    let mut ev = Vec::new();
    let mut act = Vec::new();
    let Some(old) = old else {
        if !initial {
            ev.push(NewEvent::new(
                &subject,
                Severity::Info,
                "pool",
                format!("{} ({}) appeared on {}", new.name, new.layout, new.node),
            ));
        }
        return (ev, act);
    };
    if old.state != new.state && !old.state.is_empty() {
        let sev = match new.health {
            PoolHealth::Ok => Severity::Success,
            PoolHealth::Degraded => Severity::Warning,
            PoolHealth::Faulted => Severity::Error,
            PoolHealth::Unknown => Severity::Info,
        };
        let msg = format!("{} on {}: {} → {}", new.name, new.node, old.state, new.state);
        ev.push(NewEvent::new(&subject, sev, "state", msg.clone()));
        if old.health != new.health {
            act.push(NewActivity::new("pool.state", sev, msg));
        }
    }
    let was_running = old.scan.as_ref().is_some_and(|s| s.state == "running");
    if let Some(s) = &new.scan {
        let label = scan_label(&s.kind);
        if s.state == "running" && !was_running {
            ev.push(NewEvent::new(&subject, Severity::Info, "scrub", format!("{label} started")));
            if s.kind == "resilver" || s.kind == "recovery" {
                act.push(NewActivity::new(
                    "pool.scrub",
                    Severity::Warning,
                    format!("{label} of {} on {} started", new.name, new.node),
                ));
            }
        } else if was_running && s.state == "finished" {
            let errors = s.errors.unwrap_or(0);
            let mut msg = format!("{label} finished");
            if let Some(d) = s.duration_secs {
                msg.push_str(&format!(" in {}", crate::util::human_duration(d as i64)));
            }
            if let Some(r) = &s.repaired {
                msg.push_str(&format!(", {} {r}", if s.kind == "resilver" { "resilvered" } else { "repaired" }));
            }
            msg.push_str(&format!(", {errors} {}", if errors == 1 { "error" } else { "errors" }));
            let sev = if errors > 0 { Severity::Warning } else { Severity::Success };
            ev.push(NewEvent::new(&subject, sev, "scrub", msg.clone()));
            act.push(NewActivity::new("pool.scrub", sev, format!("{} on {}: {msg}", new.name, new.node)));
        } else if was_running && s.state == "canceled" {
            ev.push(NewEvent::new(&subject, Severity::Info, "scrub", format!("{label} canceled")));
        }
    }
    (ev, act)
}

/// Maps a pool's members to disks. A member that's gone (a ZFS GUID with
/// "was /dev/disk/by-id/…") is looked up among the disks Nodestarr knew
/// before (`gone`), so it links to that disk's history. Mapped members are
/// named after their disk ("sdc"), except in SnapRAID ("d1", "parity 1").
fn map_members(pool: &mut Pool, topology: Option<&Topology>, refs: &[DiskRef], gone: &[DiskRef]) {
    let snapraid = pool.kind == PoolKind::Snapraid;
    for g in &mut pool.groups {
        for m in &mut g.members {
            let Some(path) = m.path.clone() else { continue };
            let found = if snapraid {
                // SnapRAID names directories and parity files: find their filesystem.
                topology.and_then(|t| {
                    let mount = t.mount_for(&path)?;
                    if m.size.is_none() {
                        m.size = mount.size;
                    }
                    if m.used.is_none() || m.role == "parity" {
                        m.used = mount.used;
                    }
                    blockdev::find_disk(&mount.source, Some(t), refs).map(|i| (&refs[i], true))
                })
            } else {
                blockdev::find_disk(&path, topology, refs).map(|i| (&refs[i], true)).or_else(|| {
                    let was = m.note.as_deref().and_then(blockdev::was_path)?;
                    blockdev::find_disk(was, topology, refs)
                        .map(|i| (&refs[i], true))
                        .or_else(|| blockdev::find_disk(was, None, gone).map(|i| (&gone[i], false)))
                })
            };
            if let Some((r, present)) = found {
                m.disk = Some(r.id.clone());
                if !snapraid && present {
                    m.name = r.devpath.trim_start_matches("/dev/").to_string();
                }
            }
        }
    }
}

async fn apply(app: &App, settings: &Settings, scans: Vec<NodeScan>, smart: bool, now: i64) -> Result<()> {
    let ds = &app.disks;
    let (prev_disks, prev_reports, prev_pools) = {
        let st = ds.inner.state.read();
        (st.disks.clone(), st.reports.clone(), st.pools.clone())
    };
    let initial = prev_disks.is_empty();
    let rule = |kind: AlertKind, default: f64| {
        settings
            .monitoring
            .rules
            .iter()
            .find(|r| r.kind == kind)
            .map(|r| r.threshold)
            .filter(|t| *t > 0.0)
            .unwrap_or(default)
    };
    let hdd_limit = rule(AlertKind::DiskTemperature, 50.0);
    let scrub_days = rule(AlertKind::PoolScrubAge, 35.0) as u32;

    // Stable ids, unique within this read.
    let mut ids: Vec<Vec<String>> = Vec::new();
    let mut count: HashMap<String, usize> = HashMap::new();
    for s in &scans {
        let row: Vec<String> = s
            .disks
            .iter()
            .map(|d| {
                let report = match s.reads.get(&d.devpath) {
                    Some(Read::Ok(r, _)) => Some(r.as_ref()),
                    _ => None,
                };
                let model = report.and_then(|r| r.model.clone()).or_else(|| d.model.clone());
                let serial = report.and_then(|r| r.serial.clone()).or_else(|| d.serial.clone());
                disk_id(model.as_deref(), serial.as_deref(), d.wwn.as_deref(), &s.node, &d.devpath)
            })
            .collect();
        for id in &row {
            *count.entry(id.clone()).or_default() += 1;
        }
        ids.push(row);
    }
    for (si, row) in ids.iter_mut().enumerate() {
        for (di, id) in row.iter_mut().enumerate() {
            if count[id.as_str()] > 1 {
                *id = format!(
                    "{}_{}_{}",
                    id,
                    clean_id(&scans[si].node),
                    clean_id(scans[si].disks[di].devpath.trim_start_matches("/dev/"))
                );
            }
        }
    }

    // Reports: this read's, else the last known one.
    let mut reports: HashMap<String, Arc<Report>> = HashMap::new();
    let mut new_reports: HashMap<String, Report> = HashMap::new();
    let mut raws: Vec<(String, Arc<Value>)> = Vec::new();
    for (si, s) in scans.iter().enumerate() {
        for (di, d) in s.disks.iter().enumerate() {
            let id = &ids[si][di];
            if let Some(Read::Ok(r, raw)) = s.reads.get(&d.devpath) {
                new_reports.insert(id.clone(), r.as_ref().clone());
                reports.insert(id.clone(), Arc::new(r.as_ref().clone()));
                if let Some(raw) = raw {
                    raws.push((id.clone(), Arc::new(raw.clone())));
                }
            } else if let Some(r) = prev_reports.get(id) {
                reports.insert(id.clone(), r.clone());
            }
        }
    }

    // Pools of the nodes that were read, with members mapped to disks.
    let seen_ids: HashSet<&String> = ids.iter().flatten().collect();
    let gone_refs: Vec<DiskRef> = prev_disks
        .values()
        .filter(|d| !seen_ids.contains(&d.id))
        .map(|d| DiskRef {
            id: d.id.clone(),
            devpath: d.devpath.clone(),
            serial: d.serial.clone(),
            wwn: d.wwn.clone(),
            by_id: d.by_id.clone(),
        })
        .collect();
    let mut pools_out: Vec<Pool> = Vec::new();
    let mut roles: HashMap<String, Vec<Role>> = HashMap::new();
    let scanned: HashSet<String> = scans.iter().filter(|s| s.error.is_none()).map(|s| s.node.clone()).collect();
    for (si, s) in scans.iter().enumerate() {
        if s.error.is_some() {
            continue;
        }
        let refs: Vec<DiskRef> = s
            .disks
            .iter()
            .enumerate()
            .map(|(di, d)| DiskRef {
                id: ids[si][di].clone(),
                devpath: d.devpath.clone(),
                serial: reports.get(&ids[si][di]).and_then(|r| r.serial.clone()).or_else(|| d.serial.clone()),
                wwn: d.wwn.clone(),
                by_id: d.by_id.clone(),
            })
            .collect();
        for p in &s.pools {
            let mut p = p.clone();
            map_members(&mut p, s.topology.as_deref(), &refs, &gone_refs);
            for g in &p.groups {
                for m in &g.members {
                    if let Some(disk) = &m.disk {
                        roles.entry(disk.clone()).or_default().push(Role {
                            pool: p.id.clone(),
                            pool_name: p.name.clone(),
                            kind: p.kind,
                            role: m.role.clone(),
                            group: g.name.clone(),
                            member: m.name.clone(),
                            state: m.state.clone(),
                            ok: m.ok,
                        });
                    }
                }
            }
            pools_out.push(p);
        }
    }

    // History to judge against: each report is measured against the 30 days
    // before it was taken, so a disk that wasn't read this time (a pools-only
    // read, asleep, resting) keeps the findings it had.
    let taken = |r: &Report| if r.collected_at > 0 { r.collected_at } else { now };
    let judged: Vec<(String, i64)> =
        reports.iter().map(|(id, r)| (id.clone(), taken(r.as_ref()) - 30 * 86400)).collect();
    let baselines = store::baselines(&app.db, judged).await.unwrap_or_default();
    let last_tests = store::last_finished_tests(&app.db).await.unwrap_or_default();
    let temps = store::recent_temps(&app.db, now - 86400).await.unwrap_or_default();

    let mut views: BTreeMap<String, DiskView> = BTreeMap::new();
    let mut events: Vec<NewEvent> = Vec::new();
    let mut activity: Vec<NewActivity> = Vec::new();
    let mut samples: Vec<(String, Sample)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (si, s) in scans.iter().enumerate() {
        if s.error.is_some() {
            continue;
        }
        let mounts_of = |devpath: &str| -> Vec<String> {
            let Some(t) = s.topology.as_deref() else { return Vec::new() };
            let disk = devpath.trim_start_matches("/dev/");
            t.mounts
                .iter()
                .filter(|m| t.resolve(&m.source).as_deref() == Some(disk))
                .map(|m| m.target.clone())
                .collect()
        };
        for (di, d) in s.disks.iter().enumerate() {
            let id = ids[si][di].clone();
            seen.insert(id.clone());
            let prev = prev_disks.get(&id);
            let report = reports.get(&id).map(Arc::as_ref);
            let read = s.reads.get(&d.devpath);
            let mut v = prev.cloned().unwrap_or_default();
            v.id = id.clone();
            v.node = s.node.clone();
            v.devpath = d.devpath.clone();
            v.by_id = d.by_id.clone();
            v.model = report.and_then(|r| r.model.clone()).or_else(|| d.model.clone());
            v.family = report.and_then(|r| r.family.clone());
            v.serial = report.and_then(|r| r.serial.clone()).or_else(|| d.serial.clone());
            v.wwn = d.wwn.clone();
            v.vendor = d.vendor.clone();
            v.firmware = report.and_then(|r| r.firmware.clone());
            v.media = media_of(d, report);
            v.transport = transport_of(d, report);
            v.size = d.size.or_else(|| report.and_then(|r| r.capacity));
            v.rpm = report.and_then(|r| r.rpm).or(d.rpm).filter(|r| *r > 0);
            v.used_by = d.used.clone();
            v.osd = d.osd;
            v.roles = roles.remove(&id).unwrap_or_default();
            v.mounts = mounts_of(&d.devpath);
            v.present = true;
            v.first_seen = prev.map(|p| p.first_seen).filter(|t| *t > 0).unwrap_or(now);
            v.last_seen = now;
            match read {
                Some(Read::Ok(..)) => {
                    v.sampled_at = Some(now);
                    v.checked_at = Some(now);
                    v.asleep = false;
                    v.error = None;
                }
                Some(Read::Asleep) => {
                    v.checked_at = Some(now);
                    v.asleep = true;
                    v.error = None;
                }
                Some(Read::Failed(e)) => {
                    v.checked_at = Some(now);
                    v.asleep = false;
                    v.error = Some(e.clone());
                }
                None => {}
            }
            // Whether reading through the API keeps waking it. The helper
            // leaves sleeping disks alone, so it doesn't need to know.
            if s.helper || v.media != Media::Hdd {
                v.spin = SpinDown::default();
            } else if let Some(Read::Ok(r, _)) = read {
                v.spin = v.spin.next(prev_reports.get(&id).and_then(|p| p.start_stops), r.start_stops);
            }
            v.resting = !s.helper && v.spin.sleeps && !settings.proxmox.disks.wake_sleeping;
            v.source = report.map(|r| r.source);
            v.tests = test_support(report, s.helper, settings, true);
            v.last_test = last_test(report, last_tests.get(&id));
            v.smr = v.media == Media::Hdd && v.model.as_deref().is_some_and(smart::known_smr);
            if let Some(r) = report {
                v.passed = r.passed;
                v.temperature = r.temperature;
                v.power_on_hours = r.power_on_hours;
                v.power_cycles = r.power_cycles;
                v.reallocated = r.reallocated.or(r.grown_defects);
                v.pending = r.pending;
                v.uncorrectable = r.offline_uncorrectable.or(r.media_errors).or(r.uncorrected_errors);
                v.crc_errors = r.crc_errors;
                v.wear_used = r.wear_used;
                v.bytes_written = r.bytes_written;
                v.load_cycles = r.load_cycles;
                let b = baselines.get(&id);
                let delta =
                    |cur: Option<u64>, then: Option<i64>| -> Option<i64> { Some(i64::try_from(cur?).ok()? - then?) };
                let trends = smart::Trends {
                    days: b.map(|b| (taken(r) - b.ts).max(0) as f64 / 86400.0).unwrap_or(0.0),
                    reallocated: b.and_then(|b| delta(r.reallocated.or(r.grown_defects), b.realloc)),
                    pending: b.and_then(|b| delta(r.pending, b.pending)),
                    uncorrectable: b.and_then(|b| delta(r.offline_uncorrectable.or(r.uncorrected_errors), b.uncorrect)),
                    crc: b.and_then(|b| delta(r.crc_errors, b.crc)),
                    media_errors: b.and_then(|b| delta(r.media_errors, b.uncorrect)),
                    load_cycles: b.and_then(|b| delta(r.load_cycles, b.cycles)),
                };
                let ctx = smart::Context {
                    media: v.media,
                    hdd_temp_limit: hdd_limit,
                    trends,
                    in_zfs: v.roles.iter().any(|r| r.kind == PoolKind::Zfs),
                    in_md: v.roles.iter().any(|r| r.kind == PoolKind::Md),
                    last_test: v.last_test.as_ref().map(|t| SelfTestEntry {
                        kind: t.kind.clone(),
                        status: t.status.clone(),
                        outcome: t.outcome,
                        hours: None,
                        lba: t.lba,
                        remaining: None,
                    }),
                    can_test: v.tests.available,
                };
                v.temp_limit = Some(smart::temp_limit(r, &ctx));
                v.findings = smart::assess(r, &ctx);
            } else if let Some(e) = &v.error {
                v.findings = vec![Finding {
                    level: Level::Info,
                    code: "unreadable".into(),
                    title: "SMART data couldn't be read".into(),
                    detail: e.clone(),
                }];
            }
            if v.asleep && report.is_some() {
                v.findings.push(Finding {
                    level: Level::Info,
                    code: "asleep".into(),
                    title: "Asleep at the last check".into(),
                    detail: "The disk was spun down, so Nodestarr left it alone and shows its last reading.".into(),
                });
            }
            if v.resting && report.is_some() {
                v.findings.push(Finding {
                    level: Level::Info,
                    code: "resting".into(),
                    title: "Spins down between readings".into(),
                    detail: "Every reading through the Proxmox API wakes it, so Nodestarr reads it about once a day. \
                             With host access, a sleeping disk is skipped instead and read while it's awake."
                        .into(),
                });
            }
            let old_verdict = prev.map(|p| p.verdict);
            v.verdict = smart::verdict(&v.findings);
            v.temps = spark(temps.get(&id), now);
            if smart {
                match read {
                    Some(Read::Ok(r, _)) => {
                        samples.push((id.clone(), Sample::from_report(now, r)));
                        if let Some(t) = v.temperature
                            && let Some(last) = v.temps.last_mut()
                        {
                            *last = Some(t);
                        }
                    }
                    Some(Read::Asleep) => samples.push((id.clone(), Sample::state_only(now, "standby"))),
                    Some(Read::Failed(_)) => samples.push((id.clone(), Sample::state_only(now, "error"))),
                    None => {}
                }
            }

            // What changed.
            let name = disk_name(&v);
            match prev {
                None => {
                    events.push(NewEvent::new(
                        &id,
                        Severity::Info,
                        "inventory",
                        format!(
                            "First seen on {} as {}{}",
                            v.node,
                            v.devpath,
                            v.size.map(|s| format!(" ({})", crate::util::human_bytes(s))).unwrap_or_default()
                        ),
                    ));
                    if !initial {
                        activity.push(NewActivity::new(
                            "disk.inventory",
                            Severity::Info,
                            format!("New disk on {}: {name} as {}", v.node, v.devpath),
                        ));
                    }
                }
                Some(p) => {
                    if !p.present {
                        events.push(NewEvent::new(
                            &id,
                            Severity::Info,
                            "inventory",
                            format!("Connected again on {} as {}", v.node, v.devpath),
                        ));
                        activity.push(NewActivity::new(
                            "disk.inventory",
                            Severity::Info,
                            format!("{name} is back on {}", v.node),
                        ));
                    } else if p.node != v.node {
                        events.push(NewEvent::new(
                            &id,
                            Severity::Info,
                            "inventory",
                            format!("Moved from {} to {}", p.node, v.node),
                        ));
                    } else if p.devpath != v.devpath {
                        events.push(NewEvent::new(
                            &id,
                            Severity::Info,
                            "inventory",
                            format!("Now {} (was {})", v.devpath, p.devpath),
                        ));
                    }
                }
            }
            if let (Some(old), Some(Read::Ok(new, _))) = (prev_reports.get(&id), read) {
                events.extend(counter_events(&id, old, new));
            }
            if let Some(old) = old_verdict
                && old != v.verdict
                && report.is_some()
            {
                let sev = level_severity(v.verdict);
                let top = v
                    .findings
                    .iter()
                    .find(|f| f.level == v.verdict)
                    .map(|f| format!(": {}", f.title))
                    .unwrap_or_default();
                events.push(NewEvent::new(
                    &id,
                    sev,
                    "health",
                    format!("Health {} → {}{top}", old.as_str(), v.verdict.as_str()),
                ));
                if v.verdict > old && v.verdict >= Level::Warning {
                    activity.push(NewActivity::new("disk.health", sev, format!("{name} on {}{top}", v.node)));
                }
            }
            views.insert(id, v);
        }
    }

    // Disks that were present on a node that was read, and aren't anymore.
    for (id, p) in &prev_disks {
        if seen.contains(id) || !p.present || !scanned.contains(&p.node) {
            continue;
        }
        let mut v = p.clone();
        v.present = false;
        v.tests = test_support(None, false, settings, false);
        events.push(NewEvent::new(
            id,
            Severity::Warning,
            "inventory",
            format!("No longer seen on {} (was {})", p.node, p.devpath),
        ));
        activity.push(NewActivity::new(
            "disk.inventory",
            Severity::Warning,
            format!("{} is gone from {} (was {})", disk_name(p), p.node, p.devpath),
        ));
        views.insert(id.clone(), v);
    }

    // Pool findings, growth and events.
    let series = store::pool_series(&app.db, now - 30 * 86400).await.unwrap_or_default();
    let initial_pools = prev_pools.is_empty() && initial;
    for p in &mut pools_out {
        p.findings = pools::assess(p, now, PoolRules { scrub_days });
        // A member disk's own trouble is the pool's trouble too.
        let member_findings: Vec<Finding> = p
            .members()
            .filter_map(|m| {
                let d = m.disk.as_ref().and_then(|id| views.get(id)).filter(|d| d.verdict >= Level::Warning)?;
                let f = d.findings.iter().find(|f| f.level == d.verdict);
                Some(Finding {
                    level: Level::Watch,
                    code: "member_health".into(),
                    title: format!("{} needs attention", m.name),
                    detail: f.map(|f| f.title.clone()).unwrap_or_default(),
                })
            })
            .collect();
        p.findings.extend(member_findings);
        p.findings.sort_by_key(|f| std::cmp::Reverse(f.level));
        p.growth_per_day = series.get(&p.id).and_then(|s| growth_per_day(s));
        p.full_in_days = match (p.growth_per_day, p.free) {
            (Some(g), Some(free)) if g > 0.0 => Some(free as f64 / g),
            _ => None,
        };
        let (ev, act) = pool_events(prev_pools.iter().find(|o| o.id == p.id), p, initial_pools);
        events.extend(ev);
        activity.extend(act);
    }
    for old in prev_pools.iter().filter(|o| scanned.contains(&o.node) && !pools_out.iter().any(|p| p.id == o.id)) {
        events.push(NewEvent::new(
            format!("pool:{}", old.id),
            Severity::Warning,
            "pool",
            format!("{} is no longer reported", old.name),
        ));
    }
    let pool_samples: Vec<(String, Option<u64>, Option<u64>)> =
        pools_out.iter().filter(|p| p.size.is_some()).map(|p| (p.id.clone(), p.size, p.used)).collect();
    // Keep pools of nodes that couldn't be read.
    pools_out.extend(prev_pools.into_iter().filter(|o| !scanned.contains(&o.node)));
    pools_out.sort_by(|a, b| a.node.cmp(&b.node).then(a.kind.as_str().cmp(b.kind.as_str())).then(a.name.cmp(&b.name)));

    let node_errors: BTreeMap<String, String> =
        scans.iter().filter_map(|s| s.error.as_ref().map(|e| (s.node.clone(), e.clone()))).collect();
    let to_save: Vec<(DiskView, Option<String>)> = views
        .values()
        .map(|v| (v.clone(), new_reports.get(&v.id).and_then(|r| serde_json::to_string(r).ok())))
        .collect();
    {
        let mut st = ds.inner.state.write();
        for (id, r) in reports {
            st.reports.insert(id, r);
        }
        for (id, raw) in raws {
            st.raw.insert(id, raw);
        }
        for (id, v) in views {
            st.disks.insert(id, v);
        }
        st.pools = pools_out;
        for s in &scans {
            if s.error.is_none() {
                st.nodes.insert(
                    s.node.clone(),
                    NodeMemo {
                        topology: s.topology.clone(),
                        tz_offset: s.tz_offset,
                        snapraid_status: s.snapraid_status.clone(),
                    },
                );
            }
        }
        st.sampling.last_run = Some(now);
        if smart {
            st.sampling.last_smart = Some(now);
        }
        st.sampling.node_errors = node_errors;
    }
    store::save_disks(&app.db, to_save).await?;
    store::insert_samples(&app.db, samples).await?;
    store::insert_pool_samples(&app.db, now, pool_samples).await?;
    if initial && !seen.is_empty() {
        let count = seen.len();
        activity.push(NewActivity::new(
            "disk.inventory",
            Severity::Info,
            format!(
                "Found {} on the Proxmox {}",
                if count == 1 { "1 disk".into() } else { format!("{count} disks") },
                if scanned.len() == 1 { "node" } else { "nodes" }
            ),
        ));
    }
    store::insert_events(&app.db, now, events).await?;
    for a in activity {
        app.note(a).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ids_are_stable_and_safe() {
        assert_eq!(
            disk_id(Some("WDC WD40EFRX-68N32N0"), Some("WD-WCC7K1234567"), None, "pve", "/dev/sda"),
            "WDC_WD40EFRX-68N32N0_WD-WCC7K1234567"
        );
        // The same disk as Proxmox spells the model.
        assert_eq!(
            disk_id(Some("WDC_WD40EFRX-68N32N0"), Some("WD-WCC7K1234567"), None, "pve2", "/dev/sdc"),
            "WDC_WD40EFRX-68N32N0_WD-WCC7K1234567"
        );
        assert_eq!(
            disk_id(Some("USB"), Some("000000000000"), Some("0x5000c500a1b2c3d4"), "pve", "/dev/sdx"),
            "wwn-0x5000c500a1b2c3d4"
        );
        assert_eq!(disk_id(Some("USB"), Some("0123456789ABCDEF"), None, "pve", "/dev/sdx"), "pve_sdx");
        assert_eq!(disk_id(None, Some("a b/c"), None, "pve", "/dev/sdx"), "disk_a_b_c");
    }

    #[test]
    fn proxmox_disk_list_entries() {
        let d = pve_disk(&json!({"devpath": "/dev/sda", "model": "WDC", "serial": "X1", "wwn": "unknown", "size": 100, "type": "hdd", "rpm": 5400, "osdid": -1, "by_id_link": "/dev/disk/by-id/ata-WDC_X1"})).unwrap();
        assert_eq!(d.wwn, None);
        assert_eq!(d.osd, None);
        assert_eq!(d.rpm, Some(5400));
        assert!(pve_disk(&json!({"devpath": "/dev/sda1", "parent": "/dev/sda", "type": "partition"})).is_none());
    }

    #[test]
    fn helper_sections() {
        let s = sections("##conf\nparity /p\ndata d1 /d\n##content\n1700000000 /c\n##busy\nno\n");
        assert_eq!(s["conf"], "parity /p\ndata d1 /d\n");
        assert_eq!(s["busy"].trim(), "no");
        assert!(!s.contains_key("status"));
    }

    #[test]
    fn growth_needs_enough_history() {
        let day = 86400;
        let s: Vec<(i64, i64)> = (0..10).map(|i| (i * day, 1000 + i * 50)).collect();
        assert!((growth_per_day(&s).unwrap() - 50.0).abs() < 1e-6);
        assert!(growth_per_day(&s[..3]).is_none());
        let short: Vec<(i64, i64)> = (0..10).map(|i| (i * 3600, i)).collect();
        assert!(growth_per_day(&short).is_none());
    }

    #[test]
    fn sparkline_slots() {
        let now = 100_000;
        let rows = vec![(now - 30, Some(40.0)), (now - 3600 * 5 - 10, Some(35.0)), (now - 90_000, Some(1.0))];
        let s = spark(Some(&rows), now);
        assert_eq!(s.len(), 24);
        assert_eq!(s[23], Some(40.0));
        assert_eq!(s[18], Some(35.0));
        assert_eq!(s.iter().flatten().count(), 2);
    }

    #[test]
    fn report_changes_become_events() {
        let old = Report {
            passed: Some(true),
            reallocated: Some(0),
            pending: Some(2),
            crc_errors: Some(3),
            wear_used: Some(74.0),
            ..Default::default()
        };
        let new = Report {
            passed: Some(false),
            reallocated: Some(8),
            pending: Some(0),
            crc_errors: Some(5),
            wear_used: Some(76.0),
            ..Default::default()
        };
        let ev = counter_events("d", &old, &new);
        let msgs: Vec<&str> = ev.iter().map(|e| e.message.as_str()).collect();
        assert!(msgs.contains(&"SMART health check now reports the disk is failing"));
        assert!(msgs.contains(&"Reallocated sectors: 0 → 8"));
        assert!(msgs.contains(&"Pending sectors: 2 → 0"));
        assert!(msgs.contains(&"Cable (CRC) errors: 3 → 5"));
        assert!(msgs.contains(&"75% of its rated write endurance used"));
        assert_eq!(ev.iter().find(|e| e.message.starts_with("Reallocated")).unwrap().severity, Severity::Warning);
        assert_eq!(ev.iter().find(|e| e.message.starts_with("Pending")).unwrap().severity, Severity::Info);
    }

    #[test]
    fn last_test_prefers_the_newest() {
        let report = Report {
            collected_at: 1_000_000,
            power_on_hours: Some(1000),
            self_tests: smart::SelfTests {
                log: vec![SelfTestEntry {
                    kind: "long".into(),
                    status: "Completed: read failure".into(),
                    outcome: TestOutcome::Failed,
                    hours: Some(990),
                    lba: Some(42),
                    remaining: None,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let t = last_test(Some(&report), None).unwrap();
        assert_eq!((t.outcome, t.estimated, t.at), (TestOutcome::Failed, true, Some(1_000_000 - 10 * 3600)));
        let ours = super::super::selftest::TestRun {
            kind: "long".into(),
            status: "failed".into(),
            finished_at: Some(1_000_000 - 10 * 3600 + 1200),
            result: Some("Completed: read failure".into()),
            ..Default::default()
        };
        // The same test in both: Nodestarr's record (exact time) wins.
        assert!(!last_test(Some(&report), Some(&ours)).unwrap().estimated);
        // A later test outside Nodestarr (smartd) wins over an older record.
        let older = super::super::selftest::TestRun {
            finished_at: Some(1_000_000 - 20 * 3600),
            status: "passed".into(),
            ..ours
        };
        assert!(last_test(Some(&report), Some(&older)).unwrap().estimated);
    }
}
