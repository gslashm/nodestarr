//! Container resource sampling: live stats while the UI is open, and a
//! low-frequency history (one sample per minute by default) kept in SQLite
//! with 15-minute rollups for older data.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use futures_util::{StreamExt, stream};
use rusqlite::params;
use serde::Serialize;

use crate::app::App;
use crate::docker::stats::{self, RawSample, Rates};
use crate::error::Result;
use crate::events::Event;

const ROLLUP_RES: i64 = 900;

#[derive(Debug, Clone, Serialize)]
pub struct LiveStat {
    pub id: String,
    pub identity: String,
    pub cpu: f64,
    pub mem: u64,
    pub mem_limit: u64,
    pub net_rx: f64,
    pub net_tx: f64,
    pub blk_read: f64,
    pub blk_write: f64,
    pub pids: u64,
    pub ts: i64,
}

impl LiveStat {
    fn from_rates(id: &str, identity: &str, r: &Rates) -> Self {
        Self {
            id: id.to_string(),
            identity: identity.to_string(),
            cpu: r.cpu,
            mem: r.mem,
            mem_limit: r.mem_limit,
            net_rx: r.net_rx,
            net_tx: r.net_tx,
            blk_read: r.blk_read,
            blk_write: r.blk_write,
            pids: r.pids,
            ts: crate::util::now(),
        }
    }
}

/// Samples every running container once and updates the latest values.
pub async fn sample_all(app: &App) -> Vec<LiveStat> {
    let running: Vec<(String, String)> = app
        .docker
        .list()
        .iter()
        .filter(|c| c.is_running())
        .map(|c| (c.id.clone(), c.identity.clone()))
        .collect();
    let api = app.docker.api.clone();
    let samples: Vec<(String, String, Option<RawSample>)> = stream::iter(running)
        .map(|(id, identity)| {
            let api = api.clone();
            async move {
                let s = stats::sample(&api, &id).await;
                (id, identity, s)
            }
        })
        .buffer_unordered(6)
        .collect()
        .await;
    let mut out = Vec::with_capacity(samples.len());
    let monitor = &app.monitor;
    let mut prev = monitor.inner.prev.lock();
    let live_ids: std::collections::HashSet<&String> = samples.iter().map(|(id, _, _)| id).collect();
    prev.retain(|k, _| live_ids.contains(k));
    for (id, identity, sample) in &samples {
        let Some(cur) = sample else { continue };
        if let Some(p) = prev.get(id) {
            let r = stats::rates(p, cur);
            out.push(LiveStat::from_rates(id, identity, &r));
        }
        prev.insert(id.clone(), *cur);
    }
    drop(prev);
    let mut latest = monitor.inner.latest.write();
    latest.clear();
    for s in &out {
        latest.insert(s.identity.clone(), s.clone());
    }
    out
}

/// Persists one history row per container (bucketed to the interval).
pub async fn record(app: &App, samples: &[LiveStat], host: Option<&crate::monitor::host::HostStats>, interval: i64) -> Result<()> {
    struct Row {
        identity: String,
        cpu: f64,
        mem: i64,
        mem_limit: i64,
        net_rx: i64,
        net_tx: i64,
        blk_r: i64,
        blk_w: i64,
    }
    let ts = (crate::util::now() / interval) * interval;
    let rows: Vec<Row> = samples
        .iter()
        .map(|s| Row {
            identity: s.identity.clone(),
            cpu: s.cpu,
            mem: s.mem as i64,
            mem_limit: s.mem_limit as i64,
            net_rx: s.net_rx as i64,
            net_tx: s.net_tx as i64,
            blk_r: s.blk_read as i64,
            blk_w: s.blk_write as i64,
        })
        .chain(host.map(|h| Row {
            identity: "host".to_string(),
            cpu: h.cpu,
            mem: (h.mem_total.saturating_sub(h.mem_available)) as i64,
            mem_limit: h.mem_total as i64,
            net_rx: 0,
            net_tx: 0,
            blk_r: 0,
            blk_w: 0,
        }))
        .collect();
    app.db
        .call(move |c| {
            let tx = c.transaction()?;
            {
                let mut stmt = tx.prepare_cached(
                    "INSERT OR REPLACE INTO metrics (identity, res, ts, cpu, mem, mem_limit, net_rx, net_tx, blk_r, blk_w)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                )?;
                for r in rows {
                    stmt.execute(params![r.identity, interval, ts, r.cpu, r.mem, r.mem_limit, r.net_rx, r.net_tx, r.blk_r, r.blk_w])?;
                }
            }
            tx.commit()?;
            Ok(())
        })
        .await
}

/// Folds raw samples older than the raw retention into 15-minute averages
/// and drops expired data.
pub async fn rollup(app: &App) -> Result<()> {
    let s = app.settings.get();
    let now = crate::util::now();
    let raw_cutoff = now - s.monitoring.raw_retention_hours as i64 * 3600;
    let rollup_cutoff = now - s.monitoring.rollup_retention_days as i64 * 86400;
    app.db
        .call(move |c| {
            let tx = c.transaction()?;
            tx.execute(
                "INSERT OR REPLACE INTO metrics (identity, res, ts, cpu, mem, mem_limit, net_rx, net_tx, blk_r, blk_w)
                 SELECT identity, ?1, (ts / ?1) * ?1, AVG(cpu), CAST(AVG(mem) AS INTEGER), MAX(mem_limit),
                        CAST(AVG(net_rx) AS INTEGER), CAST(AVG(net_tx) AS INTEGER), CAST(AVG(blk_r) AS INTEGER), CAST(AVG(blk_w) AS INTEGER)
                 FROM metrics WHERE res != ?1 AND ts < ?2
                 GROUP BY identity, ts / ?1",
                params![ROLLUP_RES, raw_cutoff],
            )?;
            tx.execute("DELETE FROM metrics WHERE res != ?1 AND ts < ?2", params![ROLLUP_RES, raw_cutoff])?;
            tx.execute("DELETE FROM metrics WHERE res = ?1 AND ts < ?2", params![ROLLUP_RES, rollup_cutoff])?;
            tx.commit()?;
            Ok(())
        })
        .await
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Series {
    pub ts: Vec<i64>,
    pub cpu: Vec<f64>,
    pub mem: Vec<i64>,
    pub mem_limit: Vec<i64>,
    pub net_rx: Vec<i64>,
    pub net_tx: Vec<i64>,
    pub blk_r: Vec<i64>,
    pub blk_w: Vec<i64>,
}

/// History for one identity ("host" for the Docker host) between two times,
/// downsampled to at most `max_points` buckets.
pub async fn history(app: &App, identity: &str, from: i64, to: i64, max_points: usize) -> Result<Series> {
    let identity = identity.to_string();
    let raw = app
        .db
        .call(move |c| {
            // Raw rows exist only for recent data and rollups only for older
            // data, so the two never overlap in time.
            let mut stmt = c.prepare_cached(
                "SELECT ts, cpu, mem, mem_limit, net_rx, net_tx, blk_r, blk_w FROM metrics
                 WHERE identity = ?1 AND ts BETWEEN ?2 AND ?3 ORDER BY ts",
            )?;
            let rows = stmt.query_map(params![identity, from, to], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<f64>>(1)?.unwrap_or(0.0),
                    r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(5)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(6)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(7)?.unwrap_or(0),
                ))
            })?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
        .await?;
    let max_points = max_points.clamp(10, 5000);
    let bucket = ((to - from).max(1) / max_points as i64).max(1);
    let mut s = Series::default();
    let mut i = 0;
    while i < raw.len() {
        let b = raw[i].0 / bucket;
        let mut n = 0i64;
        let mut acc = (0.0f64, 0i64, 0i64, 0i64, 0i64, 0i64, 0i64);
        let mut ts = raw[i].0;
        while i < raw.len() && raw[i].0 / bucket == b {
            let r = raw[i];
            ts = r.0;
            acc.0 += r.1;
            acc.1 += r.2;
            acc.2 = acc.2.max(r.3);
            acc.3 += r.4;
            acc.4 += r.5;
            acc.5 += r.6;
            acc.6 += r.7;
            n += 1;
            i += 1;
        }
        s.ts.push(ts);
        s.cpu.push((acc.0 / n as f64 * 10.0).round() / 10.0);
        s.mem.push(acc.1 / n);
        s.mem_limit.push(acc.2);
        s.net_rx.push(acc.3 / n);
        s.net_tx.push(acc.4 / n);
        s.blk_r.push(acc.5 / n);
        s.blk_w.push(acc.6 / n);
    }
    Ok(s)
}

/// The sampling loop: fast while someone watches, slow otherwise.
pub async fn run(app: App) {
    let mut last_record = 0i64;
    let mut last_rollup = crate::util::now();
    loop {
        let settings = app.settings.get();
        let viewers = app.monitor.inner.live_viewers.load(Ordering::Relaxed);
        let history = settings.monitoring.history_enabled;
        let sample_secs = settings.monitoring.sample_secs.max(10) as i64;
        let need_mqtt = settings.mqtt.enabled && settings.mqtt.expose_stats;
        if viewers == 0 && !history && !need_mqtt {
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }
        let samples = sample_all(&app).await;
        let host = crate::monitor::host::sample(&app);
        if viewers > 0 {
            app.bus.publish(Event::Stats(Arc::new(samples.clone())));
            if let Some(h) = &host {
                app.bus.publish(Event::Host(Arc::new(h.clone())));
            }
        }
        let now = crate::util::now();
        if history && now - last_record >= sample_secs - 1 {
            last_record = now;
            if let Err(err) = record(&app, &samples, host.as_ref(), sample_secs).await {
                tracing::warn!(%err, "failed to store metrics");
            }
        }
        if now - last_rollup >= ROLLUP_RES {
            last_rollup = now;
            if let Err(err) = rollup(&app).await {
                tracing::warn!(%err, "metrics rollup failed");
            }
        }
        let wait = if viewers > 0 {
            settings.monitoring.live_secs.max(1) as u64
        } else {
            sample_secs as u64
        };
        // Wake early when a viewer connects.
        let notify = app.monitor.inner.wake.clone();
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(wait)) => {},
            _ = notify.notified() => {},
        }
    }
}

/// Latest stats per identity (for list views and MQTT).
pub fn latest(app: &App) -> HashMap<String, LiveStat> {
    app.monitor.inner.latest.read().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rollup_and_history() {
        use crate::db::Db;
        let db = Db::open_in_memory().unwrap();
        let now = crate::util::now();
        // Old raw samples get folded into a rollup bucket.
        db.call(move |c| {
            for i in 0..4 {
                c.execute(
                    "INSERT INTO metrics (identity, res, ts, cpu, mem, mem_limit, net_rx, net_tx, blk_r, blk_w) VALUES ('a', 60, ?1, ?2, 100, 1000, 0, 0, 0, 0)",
                    params![now - 72 * 3600 + i * 60, i as f64 * 10.0],
                )?;
            }
            Ok(())
        })
        .await
        .unwrap();
        let raw_cutoff = now - 48 * 3600;
        db.call(move |c| {
            c.execute(
                "INSERT OR REPLACE INTO metrics (identity, res, ts, cpu, mem, mem_limit, net_rx, net_tx, blk_r, blk_w)
                 SELECT identity, 900, (ts / 900) * 900, AVG(cpu), CAST(AVG(mem) AS INTEGER), MAX(mem_limit), 0, 0, 0, 0
                 FROM metrics WHERE res != 900 AND ts < ?1 GROUP BY identity, ts / 900",
                [raw_cutoff],
            )?;
            c.execute("DELETE FROM metrics WHERE res != 900 AND ts < ?1", [raw_cutoff])?;
            let n: i64 = c.query_row("SELECT COUNT(*) FROM metrics WHERE res = 900", [], |r| r.get(0))?;
            assert!((1..=2).contains(&n));
            let raw: i64 = c.query_row("SELECT COUNT(*) FROM metrics WHERE res = 60", [], |r| r.get(0))?;
            assert_eq!(raw, 0);
            Ok(())
        })
        .await
        .unwrap();
    }
}
