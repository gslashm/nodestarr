//! Resource usage sampling.
//!
//! Docker's stats endpoint is asked for a single "one-shot" sample, which
//! returns immediately instead of waiting a second for a second reading.
//! Rates (CPU %, network and disk throughput) are computed against the
//! previous sample Nodestarr took of the same container.

use std::collections::HashMap;

use bollard::Docker;
use bollard::models::ContainerStatsResponse;
use bollard::query_parameters::StatsOptions;
use futures_util::StreamExt;

/// Cumulative counters from one Docker stats sample.
#[derive(Debug, Clone, Copy, Default)]
pub struct RawSample {
    /// Unix milliseconds when taken.
    pub at_ms: i64,
    pub cpu_total: u64,
    pub system_total: u64,
    pub online_cpus: u32,
    pub mem_usage: u64,
    pub mem_limit: u64,
    pub net_rx: u64,
    pub net_tx: u64,
    pub blk_read: u64,
    pub blk_write: u64,
    pub pids: u64,
}

/// Rates derived from two samples.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct Rates {
    /// Percent where 100 = one full core.
    pub cpu: f64,
    pub mem: u64,
    pub mem_limit: u64,
    pub net_rx: f64,
    pub net_tx: f64,
    pub blk_read: f64,
    pub blk_write: f64,
    pub pids: u64,
}

pub async fn sample(api: &Docker, id: &str) -> Option<RawSample> {
    let mut stream = api.stats(
        id,
        Some(StatsOptions {
            stream: false,
            one_shot: true,
        }),
    );
    let stats = tokio::time::timeout(std::time::Duration::from_secs(10), stream.next())
        .await
        .ok()??
        .ok()?;
    Some(from_response(&stats))
}

pub fn from_response(s: &ContainerStatsResponse) -> RawSample {
    let cpu = s.cpu_stats.as_ref();
    let usage = cpu.and_then(|c| c.cpu_usage.as_ref());
    let online = cpu
        .and_then(|c| c.online_cpus)
        .filter(|n| *n > 0)
        .or_else(|| usage.and_then(|u| u.percpu_usage.as_ref()).map(|v| v.len() as u32))
        .unwrap_or(1);
    let mem = s.memory_stats.as_ref();
    let mem_usage = mem.map(memory_used).unwrap_or(0);
    let mem_limit = mem.and_then(|m| m.limit).unwrap_or(0);
    let (mut rx, mut tx) = (0u64, 0u64);
    if let Some(nets) = &s.networks {
        for n in nets.values() {
            rx += n.rx_bytes.unwrap_or(0);
            tx += n.tx_bytes.unwrap_or(0);
        }
    }
    let (mut br, mut bw) = (0u64, 0u64);
    if let Some(entries) = s.blkio_stats.as_ref().and_then(|b| b.io_service_bytes_recursive.as_ref()) {
        for e in entries {
            match e.op.as_deref().map(str::to_ascii_lowercase).as_deref() {
                Some("read") => br += e.value.unwrap_or(0),
                Some("write") => bw += e.value.unwrap_or(0),
                _ => {}
            }
        }
    }
    RawSample {
        at_ms: crate::util::now_ms(),
        cpu_total: usage.and_then(|u| u.total_usage).unwrap_or(0),
        system_total: cpu.and_then(|c| c.system_cpu_usage).unwrap_or(0),
        online_cpus: online,
        mem_usage,
        mem_limit,
        net_rx: rx,
        net_tx: tx,
        blk_read: br,
        blk_write: bw,
        pids: s.pids_stats.as_ref().and_then(|p| p.current).unwrap_or(0),
    }
}

/// Memory as `docker stats` reports it: usage minus reclaimable page cache.
fn memory_used(m: &bollard::models::ContainerMemoryStats) -> u64 {
    let usage = m.usage.unwrap_or(0);
    let stats: HashMap<String, u64> = m.stats.clone().unwrap_or_default();
    // cgroup v2 exposes inactive_file; v1 exposes total_inactive_file.
    let cache = stats
        .get("inactive_file")
        .or_else(|| stats.get("total_inactive_file"))
        .copied()
        .unwrap_or(0);
    usage.saturating_sub(cache)
}

/// Rates between two samples of the same container.
pub fn rates(prev: &RawSample, cur: &RawSample) -> Rates {
    let secs = ((cur.at_ms - prev.at_ms).max(1) as f64) / 1000.0;
    let cpu_delta = cur.cpu_total.saturating_sub(prev.cpu_total) as f64;
    let sys_delta = cur.system_total.saturating_sub(prev.system_total) as f64;
    let cpu = if sys_delta > 0.0 && cpu_delta > 0.0 {
        (cpu_delta / sys_delta) * cur.online_cpus as f64 * 100.0
    } else {
        0.0
    };
    let per_sec = |a: u64, b: u64| b.saturating_sub(a) as f64 / secs;
    Rates {
        cpu: (cpu * 10.0).round() / 10.0,
        mem: cur.mem_usage,
        mem_limit: cur.mem_limit,
        net_rx: per_sec(prev.net_rx, cur.net_rx),
        net_tx: per_sec(prev.net_tx, cur.net_tx),
        blk_read: per_sec(prev.blk_read, cur.blk_read),
        blk_write: per_sec(prev.blk_write, cur.blk_write),
        pids: cur.pids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_cpu_and_rates() {
        let prev = RawSample {
            at_ms: 0,
            cpu_total: 1_000_000_000,
            system_total: 100_000_000_000,
            online_cpus: 4,
            net_rx: 1000,
            ..Default::default()
        };
        let cur = RawSample {
            at_ms: 10_000,
            cpu_total: 1_500_000_000,
            system_total: 110_000_000_000,
            online_cpus: 4,
            net_rx: 11_000,
            mem_usage: 123,
            ..Default::default()
        };
        let r = rates(&prev, &cur);
        // 0.5s of CPU over 10s of system time across 4 cpus = 20%.
        assert!((r.cpu - 20.0).abs() < 0.01, "cpu {}", r.cpu);
        assert!((r.net_rx - 1000.0).abs() < 0.01);
        assert_eq!(r.mem, 123);
    }
}
