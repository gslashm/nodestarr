//! Storage pools on a node: ZFS pools (from the Proxmox API), mdadm arrays
//! (`/proc/mdstat`), SnapRAID arrays (its config and `snapraid status`) and
//! LVM thin pools, in one model: groups of members with a redundancy level,
//! plus the pool's scrub, resilver or rebuild state.
//!
//! Members carry the names their pool uses (by-id paths, kernel names, mount
//! points); the disk service maps them to physical disks afterwards.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::smart::{Finding, Level};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolKind {
    #[default]
    Zfs,
    Md,
    Snapraid,
    Lvmthin,
}

impl PoolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PoolKind::Zfs => "zfs",
            PoolKind::Md => "md",
            PoolKind::Snapraid => "snapraid",
            PoolKind::Lvmthin => "lvmthin",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolHealth {
    Ok,
    /// Working, with less redundancy than designed.
    Degraded,
    /// Not working (data unavailable).
    Faulted,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Member {
    /// Short display name ("sda", "d1", "ata-WDC_…").
    pub name: String,
    /// The name as the pool reports it (path, kernel name, mount point).
    pub path: Option<String>,
    /// The physical disk, once mapped.
    pub disk: Option<String>,
    /// data, parity, spare, log, cache, special, dedup, content.
    pub role: String,
    pub state: Option<String>,
    /// Counts as working for redundancy.
    pub ok: bool,
    pub read: Option<f64>,
    pub write: Option<f64>,
    pub cksum: Option<f64>,
    /// "(resilvering)", "was /dev/sdc1", "faulty", "spare".
    pub note: Option<String>,
    /// Members sharing a slot replace each other (a disk being replaced and
    /// its replacement, a failed disk and the hot spare standing in).
    pub slot: usize,
    /// Filesystem size and use (SnapRAID disks).
    pub size: Option<u64>,
    pub used: Option<u64>,
    /// Parity level of a SnapRAID parity member (1 for "parity", 2 for "2-parity" …).
    pub level: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Group {
    pub name: String,
    /// data, log, cache, spare, special, dedup (ZFS); data, parity, content (SnapRAID).
    pub class: String,
    /// raidz, draid, mirror, stripe, disk, raid0 … raid10, linear, snapraid, thin.
    pub kind: String,
    /// Member failures the group survives when complete.
    pub parity: u32,
    pub state: Option<String>,
    pub members: Vec<Member>,
}

impl Group {
    /// Slots without a working member.
    pub fn failed(&self) -> u32 {
        let mut slots: Vec<(usize, bool)> = Vec::new();
        for m in &self.members {
            match slots.iter_mut().find(|(s, _)| *s == m.slot) {
                Some((_, ok)) => *ok |= m.ok,
                None => slots.push((m.slot, m.ok)),
            }
        }
        slots.iter().filter(|(_, ok)| !ok).count() as u32
    }

    /// Whether losing this group loses the pool's data.
    pub fn holds_data(&self) -> bool {
        matches!(self.class.as_str(), "data" | "special" | "dedup")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Scan {
    /// scrub, resilver, expand (ZFS); check, resync, recovery, reshape, repair (md).
    pub kind: String,
    /// running, paused, finished, canceled, pending, none.
    pub state: String,
    /// 0..1 while running.
    pub progress: Option<f64>,
    pub eta_secs: Option<u64>,
    pub speed: Option<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub duration_secs: Option<u64>,
    /// What a finished scrub repaired, or how much a resilver copied.
    pub repaired: Option<String>,
    pub errors: Option<u64>,
    pub text: String,
}

impl Scan {
    pub fn is_active(&self) -> bool {
        self.state == "running" || self.state == "pending"
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapraid {
    pub parity_levels: u32,
    pub content_files: Vec<String>,
    /// Newest content file change: when the last sync (or scrub) finished.
    pub last_sync: Option<i64>,
    /// When `snapraid status` last ran, and what it said.
    pub status_at: Option<i64>,
    pub oldest_scrub_days: Option<u32>,
    pub median_scrub_days: Option<u32>,
    pub newest_scrub_days: Option<u32>,
    pub not_scrubbed_pct: Option<f64>,
    pub sync_progress: Option<f64>,
    pub errors: Option<u64>,
    /// A snapraid command (sync, scrub, fix) is running right now.
    pub busy: bool,
    pub summary: Vec<String>,
}

/// How many more member failures the pool survives (in its weakest group).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tolerance {
    /// Failures the weakest group survives when all members work.
    pub designed: u32,
    /// Members down right now (in that group).
    pub failed: u32,
    /// Further failures it survives (negative: data is already lost).
    pub remaining: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pool {
    /// "<node>/<kind>/<name>".
    pub id: String,
    pub node: String,
    pub kind: PoolKind,
    pub name: String,
    pub state: String,
    pub health: PoolHealth,
    /// "RAID-Z2", "Mirror", "RAID 5", "SnapRAID", "Thin pool" …
    pub layout: String,
    pub size: Option<u64>,
    pub used: Option<u64>,
    pub free: Option<u64>,
    pub fragmentation: Option<u32>,
    pub dedup: Option<f64>,
    pub groups: Vec<Group>,
    pub scan: Option<Scan>,
    pub tolerance: Option<Tolerance>,
    pub status_text: Option<String>,
    pub action: Option<String>,
    pub errors_text: Option<String>,
    pub data_errors: Option<u64>,
    pub metadata_size: Option<u64>,
    pub metadata_used: Option<u64>,
    pub snapraid: Option<Snapraid>,
    // Filled in by the disk service:
    pub findings: Vec<Finding>,
    /// Bytes a day the used space grew over the last weeks, and the days
    /// until it's full at that pace.
    pub growth_per_day: Option<f64>,
    pub full_in_days: Option<f64>,
}

impl Pool {
    fn new(node: &str, kind: PoolKind, name: &str) -> Self {
        Self {
            id: format!("{node}/{}/{name}", kind.as_str()),
            node: node.to_string(),
            kind,
            name: name.to_string(),
            ..Default::default()
        }
    }

    /// Redundancy of the weakest group that holds data.
    pub fn compute_tolerance(&mut self) {
        self.tolerance = self
            .groups
            .iter()
            .filter(|g| g.holds_data())
            .map(|g| {
                let failed = g.failed();
                Tolerance { designed: g.parity, failed, remaining: g.parity as i32 - failed as i32 }
            })
            .min_by_key(|t| (t.remaining, t.designed));
    }

    pub fn members(&self) -> impl Iterator<Item = &Member> {
        self.groups.iter().flat_map(|g| g.members.iter())
    }
}

fn num_u64(v: &Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn num_f64(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(|s| s.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------------------
// ZFS
// ---------------------------------------------------------------------------

fn zfs_health(state: &str) -> PoolHealth {
    match state {
        "ONLINE" => PoolHealth::Ok,
        "DEGRADED" => PoolHealth::Degraded,
        "FAULTED" | "UNAVAIL" | "SUSPENDED" | "REMOVED" | "OFFLINE" => PoolHealth::Faulted,
        _ => PoolHealth::Unknown,
    }
}

/// Whether a ZFS member state counts as working. A DEGRADED leaf (too many
/// errors) still serves data.
fn zfs_member_ok(state: &str) -> bool {
    matches!(state, "ONLINE" | "DEGRADED" | "AVAIL" | "INUSE")
}

/// (kind, parity) of a ZFS vdev name: "raidz2-0" → ("raidz", 2),
/// "draid2:4d:12c:1s-0" → ("draid", 2), "mirror-1" → ("mirror", 0).
fn zfs_vdev_kind(name: &str) -> Option<(&'static str, u32)> {
    if let Some(rest) = name.strip_prefix("raidz") {
        let level = rest.split('-').next().unwrap_or("");
        return Some(("raidz", if level.is_empty() { 1 } else { level.parse().unwrap_or(1) }));
    }
    if let Some(rest) = name.strip_prefix("draid") {
        let level = rest.split([':', '-']).next().unwrap_or("");
        return Some(("draid", if level.is_empty() { 1 } else { level.parse().unwrap_or(1) }));
    }
    if name.starts_with("mirror-") || name == "mirror" {
        return Some(("mirror", 0));
    }
    None
}

/// The short name shown for a member path.
pub fn short_name(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    base.to_string()
}

fn zfs_member(v: &Value, role: &str, slot: usize) -> Member {
    let name = v["name"].as_str().unwrap_or("").to_string();
    let state = text(&v["state"]);
    let msg = text(&v["msg"]);
    Member {
        name: short_name(&name),
        path: Some(name.clone()),
        role: role.to_string(),
        ok: state.as_deref().is_some_and(zfs_member_ok),
        state,
        read: num_f64(&v["read"]),
        write: num_f64(&v["write"]),
        cksum: num_f64(&v["cksum"]),
        note: msg,
        slot,
        ..Default::default()
    }
}

/// Members of one vdev; nested "replacing-N" and "spare-N" vdevs share a slot.
fn zfs_members(children: &[Value], role: &str) -> Vec<Member> {
    let mut out = Vec::new();
    for (slot, c) in children.iter().enumerate() {
        let name = c["name"].as_str().unwrap_or("");
        match c["children"].as_array() {
            Some(nested) if name.starts_with("replacing-") || name.starts_with("spare-") => {
                let note = if name.starts_with("replacing-") { "replacing" } else { "hot spare in use" };
                for n in nested {
                    let mut m = zfs_member(n, role, slot);
                    m.note = Some(match &m.note {
                        Some(x) => format!("{note}, {x}"),
                        None => note.to_string(),
                    });
                    out.push(m);
                }
            }
            _ => out.push(zfs_member(c, role, slot)),
        }
    }
    out
}

fn zfs_group(v: &Value, class: &str) -> Group {
    let name = v["name"].as_str().unwrap_or("").to_string();
    match (zfs_vdev_kind(&name), v["children"].as_array()) {
        (Some((kind, parity)), Some(children)) => {
            let members = zfs_members(children, class);
            let parity =
                if kind == "mirror" { members.iter().map(|m| m.slot).max().unwrap_or(0) as u32 } else { parity };
            Group { name, class: class.to_string(), kind: kind.to_string(), parity, state: text(&v["state"]), members }
        }
        // A single disk (or file) vdev: no redundancy of its own.
        _ => Group {
            name: short_name(&name),
            class: class.to_string(),
            kind: "disk".into(),
            parity: 0,
            state: text(&v["state"]),
            members: vec![zfs_member(v, class, 0)],
        },
    }
}

/// A ZFS pool from the Proxmox API: an entry of `GET disks/zfs` and the
/// pool's `GET disks/zfs/{name}` (a parsed `zpool status -P -v`).
/// `tz_offset` is the node's UTC offset in seconds, for the dates in the
/// scan line.
pub fn zfs_pool(node: &str, list: &Value, detail: &Value, tz_offset: i64) -> Pool {
    let name = detail["name"].as_str().or_else(|| list["name"].as_str()).unwrap_or("").to_string();
    let mut p = Pool::new(node, PoolKind::Zfs, &name);
    p.state = text(&detail["state"]).or_else(|| text(&list["health"])).unwrap_or_default();
    p.health = zfs_health(&p.state);
    p.size = num_u64(&list["size"]);
    p.used = num_u64(&list["alloc"]);
    p.free = num_u64(&list["free"]);
    p.fragmentation = num_u64(&list["frag"]).map(|f| f as u32);
    p.dedup = num_f64(&list["dedup"]).filter(|d| *d > 0.0);
    p.status_text = text(&detail["status"]);
    p.action = text(&detail["action"]);
    p.errors_text = text(&detail["errors"]);
    p.data_errors = p.errors_text.as_deref().map(|e| {
        if e.starts_with("No known data errors") {
            0
        } else {
            e.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(1)
        }
    });
    if let Some(scan) = text(&detail["scan"]) {
        p.scan = Some(parse_scan(&scan, tz_offset));
    }
    // A raidz expansion (OpenZFS 2.3) shows up in its own line.
    if let Some(expand) = text(&detail["expand"]) {
        let mut s = parse_scan(&expand, tz_offset);
        s.kind = "expand".into();
        if p.scan.as_ref().is_none_or(|x| !x.is_active()) {
            p.scan = Some(s);
        }
    }

    for section in detail["children"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let sname = section["name"].as_str().unwrap_or("");
        let children = section["children"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        match sname {
            "logs" | "special" | "dedup" => {
                let class = if sname == "logs" { "log" } else { sname };
                p.groups.extend(children.iter().map(|c| zfs_group(c, class)));
            }
            "cache" | "spares" => {
                let class = if sname == "cache" { "cache" } else { "spare" };
                let members = zfs_members(children, class);
                if !members.is_empty() {
                    p.groups.push(Group {
                        name: sname.to_string(),
                        class: class.to_string(),
                        kind: "disk".into(),
                        parity: 0,
                        state: None,
                        members,
                    });
                }
            }
            // The pool's own row: its children are the data vdevs.
            _ => p.groups.extend(children.iter().map(|c| zfs_group(c, "data"))),
        }
    }
    let data: Vec<&Group> = p.groups.iter().filter(|g| g.class == "data").collect();
    p.layout = zfs_layout(&data);
    p.compute_tolerance();
    p
}

fn zfs_layout(data: &[&Group]) -> String {
    if data.is_empty() {
        return "ZFS".into();
    }
    let first = data[0];
    let same = data.iter().all(|g| g.kind == first.kind && g.parity == first.parity);
    let label = |g: &Group| match g.kind.as_str() {
        "raidz" => format!("RAID-Z{}", g.parity),
        "draid" => format!("dRAID{}", g.parity),
        "mirror" => "Mirror".to_string(),
        _ => "Stripe".to_string(),
    };
    if !same {
        return "Mixed vdevs".into();
    }
    match (first.kind.as_str(), data.len()) {
        ("disk", _) if data.len() == 1 => "Single disk".into(),
        ("disk", _) => "Stripe".into(),
        (_, 1) => label(first),
        (_, n) => format!("{} × {n}", label(first)),
    }
}

/// Parses a `zpool status` scan (or expand) line, joined onto one line.
pub fn parse_scan(s: &str, tz_offset: i64) -> Scan {
    let mut scan = Scan { text: s.to_string(), state: "none".into(), ..Default::default() };
    let lower = s.to_ascii_lowercase();
    scan.kind = if lower.starts_with("resilver") {
        "resilver"
    } else if lower.starts_with("expansion") || lower.contains("expansion of") {
        "expand"
    } else {
        "scrub"
    }
    .into();
    if lower.contains("none requested") {
        return scan;
    }
    if lower.contains("in progress since") {
        scan.state = "running".into();
        scan.started_at = date_after(s, "since ", tz_offset);
    } else if lower.contains("paused since") {
        scan.state = "paused".into();
        scan.started_at = date_after(s, "started on ", tz_offset);
    } else if lower.contains("canceled on") {
        scan.state = "canceled".into();
        scan.finished_at = date_after(s, "canceled on ", tz_offset);
    } else if lower.contains(" on ")
        && (lower.contains("repaired") || lower.contains("resilvered") || lower.contains("copied"))
    {
        scan.state = "finished".into();
        scan.finished_at = date_after(s, " on ", tz_offset);
    }
    let words: Vec<&str> = s.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        let next = words.get(i + 1).copied().unwrap_or("");
        if let Some(p) = w.strip_suffix('%')
            && next.starts_with("done")
        {
            scan.progress = p.parse::<f64>().ok().map(|p| (p / 100.0).clamp(0.0, 1.0));
        }
        if next.starts_with("to") && words.get(i + 2).is_some_and(|x| x.starts_with("go")) {
            scan.eta_secs = parse_duration(w);
        }
        if (*w == "repaired" || *w == "resilvered") && scan.state == "finished" && scan.repaired.is_none() {
            scan.repaired = Some(next.to_string());
        }
        if *w == "in" && scan.state == "finished" && scan.duration_secs.is_none() {
            // "in 03:24:41" or "in 1 days 03:24:41".
            scan.duration_secs = if words.get(i + 2).is_some_and(|x| x.starts_with("day")) {
                let days: u64 = next.parse().unwrap_or(0);
                words.get(i + 3).and_then(|t| parse_duration(t)).map(|t| t + days * 86400)
            } else {
                parse_duration(next)
            };
        }
        if *w == "with" && next.parse::<u64>().is_ok() && words.get(i + 2).is_some_and(|x| x.starts_with("error")) {
            scan.errors = next.parse().ok();
        }
        if *w == "issued" && next == "at" {
            scan.speed = words.get(i + 2).map(|x| x.trim_end_matches(',').to_string());
        }
        // Running scans: "0B repaired, 28.15% done" or "145G resilvered, …".
        if (next.starts_with("repaired") || next.starts_with("resilvered")) && scan.state == "running" {
            scan.repaired = Some(w.to_string());
        }
    }
    if scan.state == "running" && scan.speed.is_none() {
        // Without "issued at" (older ZFS): "scanned at 321M/s".
        if let Some(i) = words.iter().position(|w| *w == "at") {
            scan.speed = words.get(i + 1).map(|x| x.trim_end_matches(',').to_string());
        }
    }
    if scan.state == "finished"
        && let (Some(end), Some(d)) = (scan.finished_at, scan.duration_secs)
    {
        scan.started_at = Some(end - d as i64);
    }
    scan
}

/// "03:24:41" → seconds; also "1d 2h 3m"-free forms like "45:10".
fn parse_duration(s: &str) -> Option<u64> {
    let parts: Vec<u64> = s.trim_end_matches(',').split(':').map(|p| p.parse().ok()).collect::<Option<Vec<_>>>()?;
    match parts.as_slice() {
        [h, m, sec] => Some(h * 3600 + m * 60 + sec),
        [m, sec] => Some(m * 60 + sec),
        _ => None,
    }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Reads "Sun Oct 11 03:48:42 2026" following `marker` as node-local time.
fn date_after(s: &str, marker: &str, tz_offset: i64) -> Option<i64> {
    let idx = s.find(marker)? + marker.len();
    let words: Vec<&str> = s[idx..].split_whitespace().take(5).collect();
    let [_, mon, day, time, year] = words.as_slice() else { return None };
    let month = MONTHS.iter().position(|m| m == mon)? as u32 + 1;
    let day: u32 = day.parse().ok()?;
    let year: i32 = year.trim_end_matches([',', '.']).parse().ok()?;
    let mut hms = time.split(':').map(|x| x.parse::<u32>().ok());
    let (h, m, sec) = (hms.next()??, hms.next()??, hms.next()??);
    let dt = chrono::NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(h, m, sec)?;
    Some(dt.and_utc().timestamp() - tz_offset)
}

// ---------------------------------------------------------------------------
// mdadm (/proc/mdstat)
// ---------------------------------------------------------------------------

/// Arrays in `/proc/mdstat`.
pub fn parse_mdstat(node: &str, text: &str) -> Vec<Pool> {
    // Each array with its "[n/m]": raid disks wanted, and working.
    let mut out: Vec<(Pool, Option<(u32, u32)>)> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("Personalities") || trimmed.starts_with("unused devices") {
            continue;
        }
        if !line.starts_with(char::is_whitespace)
            && let Some((name, rest)) = line.split_once(" : ")
        {
            out.push((md_header(node, name.trim(), rest), None));
            continue;
        }
        let Some((p, disks)) = out.last_mut() else { continue };
        md_detail(p, trimmed, disks);
    }
    out.into_iter()
        .map(|(mut p, disks)| {
            md_finish(&mut p, disks);
            p
        })
        .collect()
}

fn md_header(node: &str, name: &str, rest: &str) -> Pool {
    let mut p = Pool::new(node, PoolKind::Md, name);
    let mut words = rest.split_whitespace().peekable();
    let active = words.next().unwrap_or("");
    let mut notes = Vec::new();
    while let Some(w) = words.peek().copied() {
        if w.starts_with('(') {
            notes.push(w.trim_matches(|c| c == '(' || c == ')').to_string());
            words.next();
        } else {
            break;
        }
    }
    let mut level = String::new();
    if words.peek().is_some_and(|w| !w.contains('[')) {
        level = words.next().unwrap_or("").to_string();
    }
    let mut members: Vec<Member> = words
        .filter_map(|w| {
            let (dev, rest) = w.split_once('[')?;
            let (idx, flags) = rest.split_once(']')?;
            let flags: Vec<&str> = flags.split(['(', ')']).filter(|f| !f.is_empty()).collect();
            let faulty = flags.contains(&"F");
            let spare = flags.contains(&"S");
            let note = flags
                .iter()
                .map(|f| match *f {
                    "F" => "faulty",
                    "S" => "spare",
                    "W" => "write-mostly",
                    "R" => "replacement",
                    "J" => "journal",
                    other => other,
                })
                .collect::<Vec<_>>()
                .join(", ");
            Some(Member {
                name: dev.to_string(),
                path: Some(format!("/dev/{dev}")),
                role: if spare { "spare".into() } else { "data".into() },
                state: Some(
                    if faulty {
                        "faulty"
                    } else if spare {
                        "spare"
                    } else {
                        "active"
                    }
                    .into(),
                ),
                ok: !faulty,
                note: (!note.is_empty()).then_some(note),
                slot: idx.parse().unwrap_or(0),
                ..Default::default()
            })
        })
        .collect();
    members.sort_by_key(|m| m.slot);
    p.state = std::iter::once(active.to_string()).chain(notes).collect::<Vec<_>>().join(", ");
    let (kind, parity) = match level.as_str() {
        "raid1" => ("raid1", members.iter().filter(|m| m.role == "data").count().saturating_sub(1) as u32),
        "raid4" | "raid5" => (if level == "raid4" { "raid4" } else { "raid5" }, 1),
        "raid6" => ("raid6", 2),
        "raid10" => ("raid10", 1),
        "raid0" => ("raid0", 0),
        "linear" => ("linear", 0),
        "" => ("inactive", 0),
        _ => ("other", 0),
    };
    p.layout = match kind {
        "raid1" => "RAID 1".into(),
        "raid4" => "RAID 4".into(),
        "raid5" => "RAID 5".into(),
        "raid6" => "RAID 6".into(),
        "raid10" => "RAID 10".into(),
        "raid0" => "RAID 0".into(),
        "linear" => "Linear".into(),
        "inactive" => "Inactive array".into(),
        _ => level.clone(),
    };
    // RAID 4 keeps its parity on one dedicated disk: the last active one.
    if kind == "raid4"
        && let Some(last) = members.iter_mut().rev().find(|m| m.role == "data")
    {
        last.role = "parity".into();
    }
    let (data, spares): (Vec<Member>, Vec<Member>) = members.into_iter().partition(|m| m.role != "spare");
    p.groups.push(Group {
        name: name.to_string(),
        class: "data".into(),
        kind: kind.into(),
        parity,
        state: None,
        members: data,
    });
    if !spares.is_empty() {
        p.groups.push(Group {
            name: "spares".into(),
            class: "spare".into(),
            kind: "disk".into(),
            parity: 0,
            state: None,
            members: spares,
        });
    }
    if active == "inactive" {
        p.health = PoolHealth::Faulted;
    }
    p
}

fn md_detail(p: &mut Pool, line: &str, disks: &mut Option<(u32, u32)>) {
    if let Some(idx) = line.find(" blocks")
        && p.size.is_none()
    {
        p.size = line[..idx].trim().parse::<u64>().ok().map(|k| k * 1024);
        if line.contains("near-copies") || line.contains("far-copies") || line.contains("offset-copies") {
            let copies: u32 = line
                .split_whitespace()
                .collect::<Vec<_>>()
                .windows(2)
                .find(|w| w[1].contains("-copies"))
                .and_then(|w| w[0].parse().ok())
                .unwrap_or(2);
            if let Some(g) = p.groups.first_mut() {
                g.parity = copies.saturating_sub(1);
            }
        }
        // "[3/2]": raid disks wanted and working. (The numbers after member
        // names are device numbers, not slots: a replacement being rebuilt
        // into slot 1 of a mirror can be number 2.)
        *disks = line.split_whitespace().find_map(|w| {
            let (n, m) = w.strip_prefix('[')?.strip_suffix(']')?.split_once('/')?;
            Some((n.parse().ok()?, m.parse().ok()?))
        });
        return;
    }
    for kind in ["recovery", "resync", "reshape", "check", "repair"] {
        let Some(pos) = line.find(kind) else { continue };
        let rest = &line[pos + kind.len()..];
        let rest = rest.trim_start().trim_start_matches('=').trim_start();
        let mut scan = Scan { kind: kind.into(), text: line.to_string(), ..Default::default() };
        if rest.starts_with("DELAYED") || rest.starts_with("PENDING") {
            scan.state = "pending".into();
        } else {
            scan.state = "running".into();
            scan.progress =
                rest.split('%').next().and_then(|p| p.trim().parse::<f64>().ok()).map(|p| (p / 100.0).clamp(0.0, 1.0));
            for w in rest.split_whitespace() {
                if let Some(m) = w.strip_prefix("finish=").and_then(|m| m.strip_suffix("min")) {
                    scan.eta_secs = m.parse::<f64>().ok().map(|m| (m * 60.0) as u64);
                }
                if let Some(sp) = w.strip_prefix("speed=") {
                    scan.speed = Some(sp.to_string());
                }
            }
        }
        p.scan = Some(scan);
        return;
    }
}

fn md_finish(p: &mut Pool, disks: Option<(u32, u32)>) {
    let rebuilding =
        p.scan.as_ref().is_some_and(|s| matches!(s.kind.as_str(), "recovery" | "reshape") && s.is_active());
    if let Some((wanted, working)) = disks
        && let Some(g) = p.groups.iter_mut().find(|g| g.class == "data")
    {
        if g.kind == "raid1" {
            g.parity = wanted.saturating_sub(1);
        }
        // Raid disks that aren't there at all: empty slots.
        for _ in g.members.len() as u32..wanted {
            g.members.push(Member {
                name: "missing".into(),
                role: "data".into(),
                state: Some("missing".into()),
                ok: false,
                ..Default::default()
            });
        }
        // While rebuilding, the newest members (the highest device numbers)
        // aren't in sync yet.
        let healthy = g.members.iter().filter(|m| m.ok).count() as u32;
        if rebuilding && healthy > working {
            let mut excess = healthy - working;
            for m in g.members.iter_mut().rev().filter(|m| m.ok && m.state.as_deref() == Some("active")) {
                if excess == 0 {
                    break;
                }
                m.ok = false;
                m.state = Some("rebuilding".into());
                m.note = Some(match m.note.take() {
                    Some(n) => format!("rebuilding, {n}"),
                    None => "rebuilding".into(),
                });
                excess -= 1;
            }
        }
    }
    for g in &mut p.groups {
        for (i, m) in g.members.iter_mut().enumerate() {
            m.slot = i;
        }
    }
    p.compute_tolerance();
    if p.health == PoolHealth::Unknown {
        p.health = match p.tolerance {
            Some(t) if t.remaining < 0 => PoolHealth::Faulted,
            Some(t) if t.failed > 0 => PoolHealth::Degraded,
            _ => PoolHealth::Ok,
        };
    }
    if p.state.starts_with("active") {
        let status = match p.health {
            PoolHealth::Degraded => "degraded",
            PoolHealth::Faulted => "failed",
            _ => "clean",
        };
        p.state = format!("{}, {status}", p.state);
    }
}

// ---------------------------------------------------------------------------
// SnapRAID
// ---------------------------------------------------------------------------

/// A SnapRAID array from its config file, plus `snapraid status` output when
/// available. Members reference mount points (data) and parity files.
pub fn snapraid_pool(node: &str, conf: &str, status: Option<&str>) -> Option<Pool> {
    let mut parity: Vec<Member> = Vec::new();
    let mut data: Vec<Member> = Vec::new();
    let mut content = Vec::new();
    for line in conf.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut words = line.splitn(2, char::is_whitespace);
        let key = words.next().unwrap_or("");
        let value = words.next().unwrap_or("").trim();
        let level = match key {
            "parity" | "1-parity" => Some(1),
            "q-parity" | "2-parity" => Some(2),
            "z-parity" | "3-parity" => Some(3),
            "4-parity" => Some(4),
            "5-parity" => Some(5),
            "6-parity" => Some(6),
            _ => None,
        };
        if let Some(level) = level {
            // Split parity: one level spread over several files (disks).
            for file in value.split(',').map(str::trim).filter(|f| !f.is_empty()) {
                parity.push(Member {
                    name: format!("parity {level}"),
                    path: Some(file.to_string()),
                    role: "parity".into(),
                    ok: true,
                    slot: parity.len(),
                    level: Some(level),
                    ..Default::default()
                });
            }
        } else if key == "data" || key == "disk" {
            let mut parts = value.splitn(2, char::is_whitespace);
            let name = parts.next().unwrap_or("").to_string();
            let dir = parts.next().unwrap_or("").trim().to_string();
            if !name.is_empty() && !dir.is_empty() {
                data.push(Member {
                    name,
                    path: Some(dir),
                    role: "data".into(),
                    ok: true,
                    slot: data.len(),
                    ..Default::default()
                });
            }
        } else if key == "content" {
            content.push(value.to_string());
        }
    }
    if parity.is_empty() && data.is_empty() {
        return None;
    }
    let mut p = Pool::new(node, PoolKind::Snapraid, "snapraid");
    let levels = parity.iter().filter_map(|m| m.level).max().unwrap_or(0);
    p.layout = format!("{} data, {} parity", data.len(), levels);
    // Any `levels` disks (data or parity) can fail and be rebuilt.
    p.groups.push(Group {
        name: "data".into(),
        class: "data".into(),
        kind: "snapraid".into(),
        parity: levels,
        state: None,
        members: data,
    });
    p.groups.push(Group {
        name: "parity".into(),
        class: "parity".into(),
        kind: "snapraid".into(),
        parity: 0,
        state: None,
        members: parity,
    });
    p.snapraid = Some(Snapraid { parity_levels: levels, content_files: content, ..Default::default() });
    p.state = "configured".into();
    p.health = PoolHealth::Ok;
    if let Some(status) = status {
        apply_snapraid_status(&mut p, status);
    }
    p.compute_tolerance();
    Some(p)
}

/// Reads the summary sentences of `snapraid status`.
pub fn apply_snapraid_status(p: &mut Pool, status: &str) {
    let Some(sr) = p.snapraid.as_mut() else { return };
    for line in status.lines().map(str::trim) {
        let words: Vec<&str> = line.split_whitespace().collect();
        if line.starts_with("The oldest block was scrubbed") {
            let nums: Vec<u32> = words.iter().filter_map(|w| w.trim_end_matches([',', '.']).parse().ok()).collect();
            sr.oldest_scrub_days = nums.first().copied();
            sr.median_scrub_days = nums.get(1).copied();
            sr.newest_scrub_days = nums.get(2).copied();
        } else if line.contains("of the array is not scrubbed") {
            sr.not_scrubbed_pct = words.iter().find_map(|w| w.strip_suffix('%')).and_then(|n| n.parse().ok());
        } else if line.contains("sync in progress at") {
            sr.sync_progress =
                words.iter().find_map(|w| w.trim_end_matches('.').strip_suffix('%')).and_then(|n| n.parse().ok());
        } else if line.starts_with("No error detected") {
            sr.errors = Some(0);
        } else if line.contains("errors!") || (line.contains("errors.") && line.starts_with("DANGER")) {
            sr.errors = words.iter().find_map(|w| w.parse::<u64>().ok()).or(Some(1));
        }
        let sentence = line.ends_with('.') || line.ends_with('!');
        let table = words.first().is_some_and(|w| w.chars().all(|c| c.is_ascii_digit()));
        if sentence
            && !table
            && !line.starts_with("Loading")
            && !line.starts_with("Using")
            && !line.starts_with("Self test")
        {
            sr.summary.push(line.to_string());
        }
        // Per disk rows: Files Fragmented Excess Wasted Used Free Use Name.
        if words.len() == 8 && table && words[6].ends_with('%') {
            let gb = |s: &str| s.parse::<f64>().ok().map(|g| (g * 1e9) as u64);
            if let Some(g) = p.groups.iter_mut().find(|g| g.class == "data")
                && let Some(m) = g.members.iter_mut().find(|m| m.name == words[7])
            {
                if m.used.is_none() {
                    m.used = gb(words[4]);
                }
                if m.size.is_none() {
                    m.size = gb(words[4]).zip(gb(words[5])).map(|(u, f)| u + f);
                }
            }
        }
    }
    if sr.errors.is_some_and(|e| e > 0) {
        p.health = PoolHealth::Degraded;
    }
}

// ---------------------------------------------------------------------------
// LVM thin pools
// ---------------------------------------------------------------------------

/// Thin pools from `GET disks/lvmthin`, with their volume group's physical
/// volumes from `GET disks/lvm`.
pub fn lvm_thin_pools(node: &str, thin: &Value, lvm: &Value) -> Vec<Pool> {
    let vgs = lvm["children"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    thin.as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(|t| {
            let lv = t["lv"].as_str()?;
            let vg = t["vg"].as_str().unwrap_or("");
            let mut p = Pool::new(node, PoolKind::Lvmthin, &format!("{vg}/{lv}"));
            p.layout = "LVM thin pool".into();
            p.size = num_u64(&t["lv_size"]);
            p.used = num_u64(&t["used"]);
            p.free = p.size.zip(p.used).map(|(s, u)| s.saturating_sub(u));
            p.metadata_size = num_u64(&t["metadata_size"]);
            p.metadata_used = num_u64(&t["metadata_used"]);
            p.state = text(&t["lv_state"]).unwrap_or_else(|| "active".into());
            p.health = PoolHealth::Ok;
            let pvs = vgs
                .iter()
                .find(|g| g["name"].as_str() == Some(vg))
                .and_then(|g| g["children"].as_array())
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let members = pvs
                .iter()
                .enumerate()
                .filter_map(|(i, pv)| {
                    let path = pv["name"].as_str()?;
                    Some(Member {
                        name: short_name(path),
                        path: Some(path.to_string()),
                        role: "data".into(),
                        ok: true,
                        slot: i,
                        size: num_u64(&pv["size"]),
                        ..Default::default()
                    })
                })
                .collect();
            p.groups.push(Group {
                name: vg.to_string(),
                class: "data".into(),
                kind: "thin".into(),
                parity: 0,
                state: None,
                members,
            });
            p.compute_tolerance();
            Some(p)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Assessment
// ---------------------------------------------------------------------------

fn finding(level: Level, code: &str, title: impl Into<String>, detail: impl Into<String>) -> Finding {
    Finding { level, code: code.into(), title: title.into(), detail: detail.into() }
}

fn pct(a: u64, b: u64) -> f64 {
    if b == 0 { 0.0 } else { a as f64 * 100.0 / b as f64 }
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Settings that shape the pool checks.
#[derive(Debug, Clone, Copy)]
pub struct PoolRules {
    /// Days without a scrub before it's worth a mention.
    pub scrub_days: u32,
}

/// Judges a pool. Findings come back most serious first.
pub fn assess(p: &Pool, now: i64, rules: PoolRules) -> Vec<Finding> {
    let mut out = Vec::new();
    match p.health {
        PoolHealth::Faulted => out.push(finding(
            Level::Failing,
            "faulted",
            format!("{} is {}", p.name, if p.state.is_empty() { "not working" } else { p.state.as_str() }),
            p.status_text.clone().unwrap_or_else(|| "The pool can't serve its data.".into()),
        )),
        PoolHealth::Degraded => {
            // A rebuild onto a replacement is the way out of this, not more trouble.
            let rebuilding = p
                .scan
                .as_ref()
                .is_some_and(|s| s.is_active() && matches!(s.kind.as_str(), "resilver" | "recovery" | "reshape"));
            let (title, level) = match p.tolerance {
                Some(t) if t.remaining <= 0 && t.designed > 0 && rebuilding => {
                    ("No redundancy until the rebuild finishes".to_string(), Level::Warning)
                }
                Some(t) if t.remaining <= 0 && t.designed > 0 => {
                    ("No redundancy left: one more failure loses data".to_string(), Level::Failing)
                }
                Some(t) if t.failed > 0 => (
                    format!(
                        "{} down, survives {} more",
                        plural(u64::from(t.failed), "disk", "disks"),
                        plural(t.remaining.max(0) as u64, "failure", "failures")
                    ),
                    Level::Warning,
                ),
                _ => ("Degraded".to_string(), Level::Warning),
            };
            let detail = if rebuilding {
                "The rebuild restores redundancy. Until it finishes, avoid heavy load and don't pull other disks."
                    .to_string()
            } else {
                let said: Vec<&str> = [p.status_text.as_deref(), p.action.as_deref()].into_iter().flatten().collect();
                if said.is_empty() {
                    "Replace or reconnect the missing disk so the pool can rebuild its redundancy.".into()
                } else {
                    said.join(" ")
                }
            };
            out.push(finding(level, "degraded", title, detail));
        }
        _ => {}
    }
    if let Some(n) = p.data_errors.filter(|n| *n > 0) {
        out.push(finding(
            Level::Failing,
            "data_errors",
            format!("Permanent errors in {}", plural(n, "file", "files")),
            p.errors_text.clone().unwrap_or_default(),
        ));
    }
    for m in p.members().filter(|m| m.ok) {
        let errs = [m.read, m.write, m.cksum].iter().flatten().filter(|e| **e > 0.0).count();
        if errs > 0 {
            out.push(finding(
                Level::Watch,
                "member_errors",
                format!("{} had I/O or checksum errors", m.name),
                format!(
                    "Read {}, write {}, checksum {}. ZFS repaired what it could from redundancy. Find the cause \
                     (cable, disk, controller), then `zpool clear {}` resets the counters.",
                    m.read.unwrap_or(0.0),
                    m.write.unwrap_or(0.0),
                    m.cksum.unwrap_or(0.0),
                    p.name
                ),
            ));
        }
    }
    if p.kind == PoolKind::Zfs
        && p.tolerance.is_some_and(|t| t.designed == 0)
        && p.groups.iter().filter(|g| g.class == "data").map(|g| g.members.len()).sum::<usize>() > 1
    {
        out.push(finding(
            Level::Info,
            "no_redundancy",
            "No redundancy",
            "The disks are striped: losing any one of them loses the whole pool. Keep good backups.",
        ));
    }

    if let Some(s) = &p.scan {
        match s.state.as_str() {
            "running" | "pending" => out.push(finding(
                if s.kind == "resilver" || s.kind == "recovery" { Level::Watch } else { Level::Info },
                "scan_running",
                format!(
                    "{} {}",
                    match s.kind.as_str() {
                        "resilver" => "Resilvering",
                        "expand" => "Expanding",
                        "scrub" => "Scrubbing",
                        "recovery" => "Rebuilding",
                        "resync" => "Resyncing",
                        "check" => "Checking",
                        "reshape" => "Reshaping",
                        other => other,
                    },
                    s.progress.map(|p| format!("{:.0}% done", p * 100.0)).unwrap_or_default()
                ),
                if s.kind == "resilver" || s.kind == "recovery" {
                    "Redundancy is reduced until it finishes; avoid heavy load and don't pull other disks."
                } else {
                    "Running in the background."
                },
            )),
            "finished" if s.errors.is_some_and(|e| e > 0) => out.push(finding(
                Level::Warning,
                "scrub_errors",
                format!("The last {} found {}", s.kind, plural(s.errors.unwrap_or(0), "error", "errors")),
                "Some data couldn't be repaired from redundancy. Check `zpool status -v` for affected files.",
            )),
            _ => {}
        }
    }
    let max_age = i64::from(rules.scrub_days.max(1)) * 86400;
    match p.kind {
        PoolKind::Zfs => {
            let last = p.scan.as_ref().filter(|s| s.kind == "scrub").and_then(|s| s.finished_at);
            let running = p.scan.as_ref().is_some_and(Scan::is_active);
            match last {
                Some(t) if now - t > max_age && !running => out.push(finding(
                    Level::Watch,
                    "scrub_age",
                    format!("Last scrubbed {} days ago", (now - t) / 86400),
                    "Scrubs read every block and repair silent corruption. Monthly is the usual rhythm.",
                )),
                None if !running && p.scan.as_ref().is_some_and(|s| s.state == "none") => out.push(finding(
                    Level::Watch,
                    "scrub_age",
                    "Never scrubbed",
                    "Scrubs read every block and repair silent corruption. Start one, then scrub monthly.",
                )),
                _ => {}
            }
        }
        PoolKind::Snapraid => {
            if let Some(sr) = &p.snapraid {
                if sr.errors.is_some_and(|e| e > 0) {
                    out.push(finding(
                        Level::Failing,
                        "snapraid_errors",
                        format!("SnapRAID reports {}", plural(sr.errors.unwrap_or(0), "error", "errors")),
                        "Run `snapraid -e fix` and then `snapraid scrub -p bad` to repair them.",
                    ));
                }
                if let Some(sync) = sr.last_sync
                    && now - sync > 7 * 86400
                {
                    out.push(finding(
                        Level::Watch,
                        "sync_age",
                        format!("Last synced {} days ago", (now - sync) / 86400),
                        "Files added or changed since the last sync aren't protected by parity yet.",
                    ));
                }
                if let Some(days) = sr.oldest_scrub_days.filter(|d| *d > rules.scrub_days) {
                    out.push(finding(
                        Level::Watch,
                        "scrub_age",
                        format!("Some data last scrubbed {days} days ago"),
                        format!(
                            "{}A regular `snapraid scrub` catches silent corruption early.",
                            sr.not_scrubbed_pct
                                .map(|p| format!("{p:.0}% of the array isn't scrubbed. "))
                                .unwrap_or_default()
                        ),
                    ));
                }
                if let Some(pc) = sr.sync_progress {
                    out.push(finding(
                        Level::Info,
                        "sync_running",
                        format!("Sync in progress at {pc:.0}%"),
                        "Parity is being updated.",
                    ));
                }
            }
            // Parity must be at least as large as the fullest data disk.
            let biggest =
                p.groups.iter().filter(|g| g.class == "data").flat_map(|g| &g.members).filter_map(|m| m.used).max();
            for m in p.groups.iter().filter(|g| g.class == "parity").flat_map(|g| &g.members) {
                if let (Some(size), Some(need)) = (m.size, biggest)
                    && need > size
                {
                    out.push(finding(
                        Level::Warning,
                        "parity_small",
                        format!("{} is smaller than the fullest data disk", m.name),
                        "The parity file grows with the fullest data disk; syncs will fail once it doesn't fit. \
                         Use a parity disk at least as large as your largest data disk.",
                    ));
                } else if let (Some(size), Some(used)) = (m.size, m.used)
                    && pct(used, size) > 95.0
                {
                    out.push(finding(
                        Level::Warning,
                        "parity_full",
                        format!("{} is {:.0}% full", m.name, pct(used, size)),
                        "The next sync may run out of room for parity.",
                    ));
                }
            }
        }
        PoolKind::Lvmthin => {
            if let (Some(u), Some(s)) = (p.metadata_used, p.metadata_size) {
                let used = pct(u, s);
                if used > 80.0 {
                    out.push(finding(
                        if used > 90.0 { Level::Failing } else { Level::Warning },
                        "metadata",
                        format!("Thin pool metadata {used:.0}% full"),
                        "When the metadata fills up the pool can be damaged. Grow it with \
                         `lvextend --poolmetadatasize +1G` before it runs out.",
                    ));
                }
            }
        }
        PoolKind::Md => {}
    }
    if let (Some(used), Some(size)) = (p.used, p.size)
        && size > 0
    {
        let full = pct(used, size);
        let limit = if p.kind == PoolKind::Zfs { 80.0 } else { 90.0 };
        if full >= 95.0 {
            out.push(finding(
                Level::Warning,
                "capacity",
                format!("{full:.0}% full"),
                "Free up space or add capacity soon.",
            ));
        } else if full >= limit {
            out.push(finding(
                Level::Watch,
                "capacity",
                format!("{full:.0}% full"),
                if p.kind == PoolKind::Zfs {
                    "ZFS slows down as pools fill up; above 80% it's time to plan more space."
                } else if p.kind == PoolKind::Lvmthin {
                    "Guests on a full thin pool stop with I/O errors. Free space or extend the pool."
                } else {
                    "Plan more space."
                },
            ));
        }
    }
    if p.fragmentation.is_some_and(|f| f >= 70) {
        out.push(finding(
            Level::Info,
            "fragmentation",
            format!("{}% free space fragmentation", p.fragmentation.unwrap_or(0)),
            "Writes get slower as free space fragments. It eases as the pool gets emptier.",
        ));
    }
    out.sort_by_key(|f| std::cmp::Reverse(f.level));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tank() -> (Value, Value) {
        let disk = |n: &str, state: &str, cksum: u64| json!({"name": format!("/dev/disk/by-id/ata-WDC_WD40EFRX-68N32N0_WD-WCC7K{n}-part1"), "state": state, "read": 0, "write": 0, "cksum": cksum, "msg": ""});
        let list = json!({"name": "tank", "size": 24_000_000_000_000u64, "alloc": 16_000_000_000_000u64, "free": 8_000_000_000_000u64, "frag": 12, "dedup": 1.0, "health": "DEGRADED"});
        let detail = json!({
            "name": "tank",
            "state": "DEGRADED",
            "status": "One or more devices could not be used because the label is missing or invalid.",
            "action": "Replace the device using 'zpool replace'.",
            "scan": "scrub repaired 0B in 03:24:41 with 0 errors on Sun Oct 11 03:48:42 2026",
            "errors": "No known data errors",
            "children": [
                {"name": "tank", "state": "DEGRADED", "read": 0, "write": 0, "cksum": 0, "msg": "", "children": [
                    {"name": "raidz2-0", "state": "DEGRADED", "read": 0, "write": 0, "cksum": 0, "msg": "", "children": [
                        disk("0000001", "ONLINE", 0), disk("0000002", "ONLINE", 3), disk("0000003", "ONLINE", 0),
                        {"name": "1234567890123456789", "state": "UNAVAIL", "read": 0, "write": 0, "cksum": 0, "msg": "was /dev/disk/by-id/ata-WDC_WD40EFRX-68N32N0_WD-WCC7K0000004-part1"},
                        disk("0000005", "ONLINE", 0), disk("0000006", "ONLINE", 0)
                    ]}
                ]},
                {"name": "logs", "children": [
                    {"name": "mirror-1", "state": "ONLINE", "read": 0, "write": 0, "cksum": 0, "msg": "", "children": [
                        {"name": "/dev/nvme0n1p1", "state": "ONLINE", "read": 0, "write": 0, "cksum": 0, "msg": ""},
                        {"name": "/dev/nvme1n1p1", "state": "ONLINE", "read": 0, "write": 0, "cksum": 0, "msg": ""}
                    ]}
                ]},
                {"name": "cache", "children": [{"name": "/dev/nvme0n1p2", "state": "ONLINE", "read": 0, "write": 0, "cksum": 0, "msg": ""}]},
                {"name": "spares", "children": [{"name": "/dev/disk/by-id/ata-WDC_WD40EFRX-68N32N0_WD-WCC7K0000007-part1", "state": "AVAIL", "msg": ""}]}
            ]
        });
        (list, detail)
    }

    #[test]
    fn zfs_raidz2_degraded() {
        let (list, detail) = tank();
        let p = zfs_pool("pve", &list, &detail, 0);
        assert_eq!(p.id, "pve/zfs/tank");
        assert_eq!(p.layout, "RAID-Z2");
        assert_eq!(p.health, PoolHealth::Degraded);
        assert_eq!(p.used, Some(16_000_000_000_000));
        assert_eq!(p.data_errors, Some(0));
        let classes: Vec<&str> = p.groups.iter().map(|g| g.class.as_str()).collect();
        assert_eq!(classes, ["data", "log", "cache", "spare"]);
        let data = &p.groups[0];
        assert_eq!((data.kind.as_str(), data.parity, data.members.len()), ("raidz", 2, 6));
        assert_eq!(data.failed(), 1);
        assert_eq!(
            data.members[3].note.as_deref(),
            Some("was /dev/disk/by-id/ata-WDC_WD40EFRX-68N32N0_WD-WCC7K0000004-part1")
        );
        assert_eq!(p.groups[1].kind, "mirror");
        assert_eq!(p.groups[1].parity, 1);
        assert_eq!(p.tolerance, Some(Tolerance { designed: 2, failed: 1, remaining: 1 }));
        let scan = p.scan.as_ref().unwrap();
        assert_eq!((scan.kind.as_str(), scan.state.as_str()), ("scrub", "finished"));
        assert_eq!(scan.duration_secs, Some(3 * 3600 + 24 * 60 + 41));
        assert_eq!(scan.errors, Some(0));
        assert_eq!(scan.repaired.as_deref(), Some("0B"));
        // Sun Oct 11 03:48:42 2026 UTC.
        assert_eq!(scan.finished_at, Some(1791690522));
        let f = assess(&p, 1791690522 + 86400, PoolRules { scrub_days: 35 });
        assert_eq!(f[0].code, "degraded");
        assert_eq!(f[0].level, Level::Warning);
        assert!(f.iter().any(|x| x.code == "member_errors"));
        assert!(!f.iter().any(|x| x.code == "scrub_age"));
    }

    #[test]
    fn zfs_replacing_and_scans() {
        let detail = json!({
            "name": "rpool", "state": "ONLINE",
            "scan": "resilver in progress since Fri Oct 16 10:00:00 2026 2.34T scanned at 1.2G/s, 1.10T issued at 567M/s, 3.62T total 145G resilvered, 30.12% done, 01:05:43 to go",
            "errors": "No known data errors",
            "children": [{"name": "rpool", "state": "ONLINE", "children": [
                {"name": "mirror-0", "state": "ONLINE", "children": [
                    {"name": "/dev/sda3", "state": "ONLINE", "read": 0, "write": 0, "cksum": 0, "msg": ""},
                    {"name": "replacing-1", "state": "ONLINE", "children": [
                        {"name": "/dev/sdb3", "state": "OFFLINE", "msg": ""},
                        {"name": "/dev/sdc3", "state": "ONLINE", "msg": "(resilvering)"}
                    ]}
                ]}
            ]}]
        });
        let p = zfs_pool("pve", &json!({"name": "rpool"}), &detail, 7200);
        assert_eq!(p.layout, "Mirror");
        let g = &p.groups[0];
        assert_eq!(g.parity, 1);
        assert_eq!(g.members.len(), 3);
        assert_eq!(g.members[1].slot, g.members[2].slot);
        assert_eq!(g.failed(), 0);
        assert_eq!(g.members[2].note.as_deref(), Some("replacing, (resilvering)"));
        let s = p.scan.unwrap();
        assert_eq!((s.kind.as_str(), s.state.as_str()), ("resilver", "running"));
        assert_eq!(s.progress, Some(0.3012));
        assert_eq!(s.eta_secs, Some(3943));
        assert_eq!(s.speed.as_deref(), Some("567M/s"));
        assert_eq!(s.repaired.as_deref(), Some("145G"));
        // The node runs two hours ahead of UTC.
        assert_eq!(s.started_at, Some(1792144800 - 7200));

        let none = parse_scan("none requested", 0);
        assert_eq!(none.state, "none");
        let paused = parse_scan(
            "scrub paused since Mon Oct 12 10:00:00 2026 scrub started on Sun Oct 11 00:24:01 2026 1.47T scanned, 1.02T issued, 3.62T total 28.15% done",
            0,
        );
        assert_eq!(paused.state, "paused");
        assert_eq!(paused.progress, Some(0.2815));
        let long = parse_scan("scrub repaired 12K in 1 days 02:03:04 with 2 errors on Sun Oct 11 03:48:42 2026", 0);
        assert_eq!(long.duration_secs, Some(86400 + 2 * 3600 + 3 * 60 + 4));
        assert_eq!(long.errors, Some(2));
        assert_eq!(long.repaired.as_deref(), Some("12K"));
    }

    #[test]
    fn zfs_layouts() {
        let leaf = |n: &str| json!({"name": n, "state": "ONLINE"});
        let detail = json!({"name": "stripe", "state": "ONLINE", "children": [{"name": "stripe", "state": "ONLINE", "children": [leaf("/dev/sda"), leaf("/dev/sdb")]}]});
        let p = zfs_pool("pve", &json!({}), &detail, 0);
        assert_eq!(p.layout, "Stripe");
        assert_eq!(p.tolerance.unwrap().designed, 0);
        assert!(assess(&p, 0, PoolRules { scrub_days: 35 }).iter().any(|f| f.code == "no_redundancy"));
        let mirrors = json!({"name": "m", "state": "ONLINE", "children": [{"name": "m", "state": "ONLINE", "children": [
            {"name": "mirror-0", "state": "ONLINE", "children": [leaf("/dev/sda"), leaf("/dev/sdb")]},
            {"name": "mirror-1", "state": "ONLINE", "children": [leaf("/dev/sdc"), leaf("/dev/sdd")]}
        ]}]});
        assert_eq!(zfs_pool("pve", &json!({}), &mirrors, 0).layout, "Mirror × 2");
        let draid = json!({"name": "d", "state": "ONLINE", "children": [{"name": "d", "state": "ONLINE", "children": [
            {"name": "draid2:4d:12c:1s-0", "state": "ONLINE", "children": [leaf("/dev/sda")]}
        ]}]});
        let p = zfs_pool("pve", &json!({}), &draid, 0);
        assert_eq!(p.layout, "dRAID2");
        assert_eq!(p.groups[0].parity, 2);
    }

    #[test]
    fn mdstat_arrays() {
        let text = "Personalities : [raid1] [raid6] [raid5] [raid4]\n\
md0 : active raid5 sdd1[3] sdc1[1] sdb1[0]\n      7813772288 blocks super 1.2 level 5, 512k chunk, algorithm 2 [3/3] [UUU]\n      bitmap: 0/30 pages [0KB], 65536KB chunk\n\n\
md1 : active raid1 sdf1[1](F) sde1[0]\n      976630464 blocks super 1.2 [2/1] [U_]\n      [=>...................]  recovery =  8.5% (83123456/976630464) finish=123.4min speed=120000K/sec\n\n\
md2 : active raid6 sdg1[0] sdh1[1] sdi1[2] sdj1[4](S)\n      100 blocks super 1.2 level 6, 512k chunk, algorithm 2 [4/3] [UUU_]\n\n\
md127 : inactive sdk[0](S)\n      976630464 blocks super 1.2\n\nunused devices: <none>\n";
        let pools = parse_mdstat("pve", text);
        assert_eq!(pools.len(), 4);
        let md0 = &pools[0];
        assert_eq!((md0.layout.as_str(), md0.health), ("RAID 5", PoolHealth::Ok));
        assert_eq!(md0.size, Some(7813772288 * 1024));
        assert_eq!(md0.groups[0].members.len(), 3);
        assert_eq!(md0.state, "active, clean");
        let md1 = &pools[1];
        assert_eq!(md1.health, PoolHealth::Degraded);
        assert_eq!(md1.tolerance, Some(Tolerance { designed: 1, failed: 1, remaining: 0 }));
        let scan = md1.scan.as_ref().unwrap();
        assert_eq!((scan.kind.as_str(), scan.progress), ("recovery", Some(0.085)));
        assert_eq!(scan.eta_secs, Some(7404));
        assert_eq!(scan.speed.as_deref(), Some("120000K/sec"));
        let md2 = &pools[2];
        assert_eq!(md2.groups[0].members.len(), 4, "the missing slot shows up");
        assert_eq!(md2.groups[1].class, "spare");
        assert_eq!(md2.tolerance, Some(Tolerance { designed: 2, failed: 1, remaining: 1 }));
        assert_eq!(pools[3].health, PoolHealth::Faulted);
        // md1 rebuilds: no redundancy until then, but on its way back.
        let f = assess(md1, 0, PoolRules { scrub_days: 35 });
        assert_eq!(f[0].level, Level::Warning, "{f:?}");
        // Without the rebuild, one more failure loses data.
        let mut stuck = md1.clone();
        stuck.scan = None;
        assert_eq!(assess(&stuck, 0, PoolRules { scrub_days: 35 })[0].level, Level::Failing);
    }

    #[test]
    fn mdstat_rebuilding_replacement() {
        // The replacement (sdk1) got device number 2 and is being rebuilt
        // into the mirror's second slot.
        let text = "md0 : active raid1 sdk1[2] sdj1[0]\n      244066432 blocks super 1.2 [2/1] [U_]\n      \
                    [====>................]  recovery = 21.3% (52000000/244066432) finish=31.2min speed=98765K/sec\n";
        let p = &parse_mdstat("pve", text)[0];
        let g = &p.groups[0];
        assert_eq!(g.members.len(), 2, "no phantom member: {:?}", g.members);
        assert_eq!(g.members[0].name, "sdj1");
        assert!(g.members[0].ok);
        assert_eq!((g.members[1].name.as_str(), g.members[1].ok), ("sdk1", false));
        assert_eq!(g.members[1].state.as_deref(), Some("rebuilding"));
        assert_eq!(p.tolerance, Some(Tolerance { designed: 1, failed: 1, remaining: 0 }));
        assert_eq!(p.health, PoolHealth::Degraded);
        let f = assess(p, 0, PoolRules { scrub_days: 35 });
        assert_eq!((f[0].title.as_str(), f[0].level), ("No redundancy until the rebuild finishes", Level::Warning));
        // Once in sync, both count.
        let done =
            parse_mdstat("pve", "md0 : active raid1 sdk1[2] sdj1[0]\n      244066432 blocks super 1.2 [2/2] [UU]\n");
        assert_eq!(done[0].tolerance, Some(Tolerance { designed: 1, failed: 0, remaining: 1 }));
    }

    #[test]
    fn snapraid_config_and_status() {
        let conf = "# SnapRAID\nparity /mnt/parity1/snapraid.parity\n2-parity /mnt/parity2/a.parity,/mnt/parity3/b.parity\n\
                    content /var/snapraid/snapraid.content\ncontent /mnt/disk1/.snapraid.content\n\
                    data d1 /mnt/disk1/\ndisk d2 /mnt/disk2/\nexclude /lost+found/\n";
        let status = "Self test...\nLoading state from /var/snapraid/snapraid.content...\nUsing 4610 MiB of memory for the file-system.\nSnapRAID status report:\n\n\
   Files Fragmented Excess  Wasted  Used    Free  Use Name\n            Files  Fragments  GB      GB      GB\n\
   29546     229     1149    10.6    3891     101  97% d1\n   24353     244     1220    15.6    3892     101  97% d2\n\
 --------------------------------------------------------------------------\n\n\
The oldest block was scrubbed 48 days ago, the median 7, the newest 0.\n\nNo sync is in progress.\n\
The 18% of the array is not scrubbed.\nNo error detected.\n";
        let mut p = snapraid_pool("pve", conf, Some(status)).unwrap();
        assert_eq!(p.layout, "2 data, 2 parity");
        assert_eq!(p.groups[1].members.len(), 3);
        assert_eq!(p.tolerance, Some(Tolerance { designed: 2, failed: 0, remaining: 2 }));
        let sr = p.snapraid.clone().unwrap();
        assert_eq!(sr.content_files.len(), 2);
        assert_eq!((sr.oldest_scrub_days, sr.median_scrub_days, sr.newest_scrub_days), (Some(48), Some(7), Some(0)));
        assert_eq!(sr.not_scrubbed_pct, Some(18.0));
        assert_eq!(sr.errors, Some(0));
        assert!(sr.summary.iter().any(|l| l.starts_with("No sync is in progress")));
        assert_eq!(p.groups[0].members[0].used, Some(3_891_000_000_000));
        let f = assess(&p, 0, PoolRules { scrub_days: 35 });
        assert!(f.iter().any(|x| x.code == "scrub_age"));
        // A parity disk smaller than the fullest data disk.
        p.groups[1].members[0].size = Some(3_000_000_000_000);
        let f = assess(&p, 0, PoolRules { scrub_days: 35 });
        assert!(f.iter().any(|x| x.code == "parity_small"));
        let bad = "DANGER! In the array there are 3 errors!\n";
        apply_snapraid_status(&mut p, bad);
        assert_eq!(p.snapraid.as_ref().unwrap().errors, Some(3));
        assert_eq!(p.health, PoolHealth::Degraded);
        assert!(snapraid_pool("pve", "# empty\n", None).is_none());
    }

    #[test]
    fn lvm_thin() {
        let thin = json!([{"lv": "data", "vg": "pve", "lv_size": 1000, "used": 500, "metadata_size": 100, "metadata_used": 95}]);
        let lvm = json!({"children": [{"name": "pve", "size": 2000, "free": 10, "children": [{"name": "/dev/sda3", "size": 2000, "free": 10}]}]});
        let pools = lvm_thin_pools("pve", &thin, &lvm);
        assert_eq!(pools.len(), 1);
        let p = &pools[0];
        assert_eq!(p.id, "pve/lvmthin/pve/data");
        assert_eq!(p.free, Some(500));
        assert_eq!(p.groups[0].members[0].name, "sda3");
        let f = assess(p, 0, PoolRules { scrub_days: 35 });
        assert_eq!(f[0].code, "metadata");
        assert_eq!(f[0].level, Level::Failing);
    }
}
