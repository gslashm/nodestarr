//! Docker host statistics read from /proc (inside a Proxmox LXC these are
//! the container's own numbers, thanks to LXCFS).

use parking_lot::Mutex;
use serde::Serialize;

use crate::app::App;
use crate::util::fsx::DiskUsage;

#[derive(Debug, Clone, Serialize, Default)]
pub struct HostStats {
    /// Percent of all cores.
    pub cpu: f64,
    pub cpus: usize,
    pub mem_total: u64,
    pub mem_available: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    pub load: [f64; 3],
    pub uptime: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PathUsage {
    pub path: String,
    #[serde(flatten)]
    pub usage: DiskUsage,
}

/// Previous /proc/stat totals for CPU deltas.
static PREV_CPU: Mutex<Option<(u64, u64)>> = Mutex::new(None);

/// The most recent sample and when it was taken, so API reads don't
/// disturb the sampling loop's CPU deltas.
static LAST: Mutex<Option<(i64, HostStats)>> = Mutex::new(None);

fn read_cpu() -> Option<(u64, u64, usize)> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let mut lines = stat.lines();
    let first = lines.next()?;
    let vals: Vec<u64> = first.split_whitespace().skip(1).filter_map(|v| v.parse().ok()).collect();
    if vals.len() < 4 {
        return None;
    }
    let idle = vals[3] + vals.get(4).copied().unwrap_or(0);
    let total: u64 = vals.iter().take(8).sum();
    let cpus = stat.lines().filter(|l| l.starts_with("cpu") && l.as_bytes().get(3).is_some_and(|b| b.is_ascii_digit())).count();
    Some((total, idle, cpus.max(1)))
}

fn meminfo() -> Option<(u64, u64, u64, u64)> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let get = |key: &str| {
        text.lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .map(|kb| kb * 1024)
    };
    Some((get("MemTotal:")?, get("MemAvailable:").unwrap_or(0), get("SwapTotal:").unwrap_or(0), get("SwapFree:").unwrap_or(0)))
}

/// Total and available memory in bytes.
pub fn memory() -> Option<(u64, u64)> {
    meminfo().map(|(t, a, _, _)| (t, a))
}

pub fn sample(_app: &App) -> Option<HostStats> {
    let (total, idle, cpus) = read_cpu()?;
    let cpu = {
        let mut prev = PREV_CPU.lock();
        let pct = match *prev {
            Some((pt, pi)) if total > pt => {
                let dt = (total - pt) as f64;
                let di = idle.saturating_sub(pi) as f64;
                ((dt - di) / dt * 100.0).clamp(0.0, 100.0)
            }
            _ => 0.0,
        };
        *prev = Some((total, idle));
        (pct * 10.0).round() / 10.0
    };
    let (mem_total, mem_available, swap_total, swap_free) = meminfo().unwrap_or_default();
    let load = std::fs::read_to_string("/proc/loadavg")
        .ok()
        .map(|l| {
            let v: Vec<f64> = l.split_whitespace().take(3).filter_map(|x| x.parse().ok()).collect();
            [v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0)]
        })
        .unwrap_or_default();
    let uptime = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|u| u.split_whitespace().next().and_then(|x| x.parse::<f64>().ok()))
        .map(|f| f as u64)
        .unwrap_or(0);
    let stats = HostStats {
        cpu,
        cpus,
        mem_total,
        mem_available,
        swap_total,
        swap_free,
        load,
        uptime,
    };
    *LAST.lock() = Some((crate::util::now(), stats.clone()));
    Some(stats)
}

/// A recent sample: the sampling loop's when it is fresh, a new one otherwise.
pub fn current(app: &App) -> Option<HostStats> {
    if let Some((at, s)) = LAST.lock().as_ref() {
        if crate::util::now() - at <= 15 {
            return Some(s.clone());
        }
    }
    sample(app)
}

/// Usage of the configured disk paths (and the backup and data folders).
pub fn disks(app: &App) -> Vec<PathUsage> {
    let s = app.settings.get();
    let mut paths: Vec<String> = s.monitoring.disk_paths.clone();
    paths.push(s.backups.root.clone());
    paths.push(app.boot.data_dir.to_string_lossy().into_owned());
    paths.sort();
    paths.dedup();
    let mut seen_fs = std::collections::HashSet::new();
    paths
        .into_iter()
        .filter_map(|p| {
            let usage = crate::util::fsx::disk_usage(std::path::Path::new(&p)).ok()?;
            // Paths on the same filesystem report identical numbers; keep one.
            if !seen_fs.insert((usage.total, usage.used / (1024 * 1024))) {
                return None;
            }
            Some(PathUsage { path: p, usage })
        })
        .collect()
}
