//! Persistence for disk health: disks, SMART samples, self-tests, events and
//! pool capacity samples.

use std::collections::HashMap;

use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use super::selftest::TestRun;
use super::smart::Report;
use super::{DiskView, Label};
use crate::db::Db;
use crate::error::Result;
use crate::settings::Severity;

/// A disk as stored: its last view, latest SMART report and label.
pub struct DiskRow {
    pub view: DiskView,
    pub report: Option<Report>,
}

pub async fn load_disks(db: &Db) -> Result<Vec<DiskRow>> {
    db.call(|c| {
        let mut stmt = c.prepare("SELECT id, first_seen, last_seen, info, report, label FROM disks")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, first_seen, last_seen, info, report, label) = row?;
            // A row an older or newer Nodestarr wrote differently still loads
            // (with defaults); its history isn't lost.
            let mut view: DiskView = serde_json::from_str(&info).unwrap_or_default();
            view.id = id;
            view.first_seen = first_seen;
            view.last_seen = last_seen;
            view.label = label.and_then(|l| serde_json::from_str::<Label>(&l).ok()).unwrap_or_default();
            let report = report.and_then(|r| serde_json::from_str::<Report>(&r).ok());
            out.push(DiskRow { view, report });
        }
        Ok(out)
    })
    .await
}

/// Writes disks (their view and, when given, their latest report).
pub async fn save_disks(db: &Db, disks: Vec<(DiskView, Option<String>)>) -> Result<()> {
    if disks.is_empty() {
        return Ok(());
    }
    let rows: Vec<(DiskView, String, Option<String>)> = disks
        .into_iter()
        .map(|(v, report)| {
            let info = serde_json::to_string(&v).unwrap_or_else(|_| "{}".into());
            (v, info, report)
        })
        .collect();
    db.call(move |c| {
        let tx = c.transaction()?;
        for (v, info, report) in &rows {
            tx.execute(
                "INSERT INTO disks (id, node, devpath, first_seen, last_seen, info, report) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET node = excluded.node, devpath = excluded.devpath,
                   last_seen = excluded.last_seen, info = excluded.info,
                   report = COALESCE(excluded.report, disks.report)",
                params![v.id, v.node, v.devpath, v.first_seen, v.last_seen, info, report],
            )?;
        }
        tx.commit()?;
        Ok(())
    })
    .await
}

pub async fn save_label(db: &Db, id: String, label: &Label) -> Result<bool> {
    let text = serde_json::to_string(label)?;
    db.call(move |c| Ok(c.execute("UPDATE disks SET label = ?1 WHERE id = ?2", params![text, id])? > 0)).await
}

/// Deletes a disk with its whole history.
pub async fn forget(db: &Db, id: String) -> Result<()> {
    db.call(move |c| {
        let tx = c.transaction()?;
        tx.execute("DELETE FROM disks WHERE id = ?1", [&id])?;
        tx.execute("DELETE FROM disk_samples WHERE disk = ?1", [&id])?;
        tx.execute("DELETE FROM disk_events WHERE subject = ?1", [&id])?;
        tx.execute("DELETE FROM disk_tests WHERE disk = ?1", [&id])?;
        tx.commit()?;
        Ok(())
    })
    .await
}

// ---------------------------------------------------------------------------
// Samples
// ---------------------------------------------------------------------------

/// One SMART reading. Counters are cumulative, so "max" is the right way to
/// summarize them over a time bucket.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Sample {
    pub ts: i64,
    /// active, standby (asleep, not read) or error.
    pub state: String,
    pub temp: Option<f64>,
    pub poh: Option<i64>,
    pub realloc: Option<i64>,
    pub pending: Option<i64>,
    pub uncorrect: Option<i64>,
    pub crc: Option<i64>,
    pub wear: Option<f64>,
    pub written: Option<i64>,
    pub cycles: Option<i64>,
}

fn opt_i64(v: Option<u64>) -> Option<i64> {
    v.and_then(|n| i64::try_from(n).ok())
}

impl Sample {
    pub fn from_report(ts: i64, r: &Report) -> Self {
        Self {
            ts,
            state: "active".into(),
            temp: r.temperature,
            poh: opt_i64(r.power_on_hours),
            realloc: opt_i64(r.reallocated.or(r.grown_defects)),
            pending: opt_i64(r.pending),
            uncorrect: opt_i64(r.offline_uncorrectable.or(r.media_errors).or(r.uncorrected_errors)),
            crc: opt_i64(r.crc_errors),
            wear: r.wear_used,
            written: opt_i64(r.bytes_written),
            cycles: opt_i64(r.load_cycles),
        }
    }

    pub fn state_only(ts: i64, state: &str) -> Self {
        Self { ts, state: state.into(), ..Default::default() }
    }
}

pub async fn insert_samples(db: &Db, rows: Vec<(String, Sample)>) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    db.call(move |c| {
        let tx = c.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO disk_samples (disk, ts, state, temp, poh, realloc, pending, uncorrect, crc, wear, written, cycles)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?;
            for (disk, s) in &rows {
                stmt.execute(params![
                    disk, s.ts, s.state, s.temp, s.poh, s.realloc, s.pending, s.uncorrect, s.crc, s.wear, s.written, s.cycles
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    })
    .await
}

fn row_to_sample(r: &rusqlite::Row<'_>) -> rusqlite::Result<Sample> {
    Ok(Sample {
        ts: r.get(0)?,
        state: r.get(1)?,
        temp: r.get(2)?,
        poh: r.get(3)?,
        realloc: r.get(4)?,
        pending: r.get(5)?,
        uncorrect: r.get(6)?,
        crc: r.get(7)?,
        wear: r.get(8)?,
        written: r.get(9)?,
        cycles: r.get(10)?,
    })
}

/// For each disk, its earliest successful reading at or after the given
/// time: the baseline that recent changes are measured against.
pub async fn baselines(db: &Db, disks: Vec<(String, i64)>) -> Result<HashMap<String, Sample>> {
    db.call(move |c| {
        let mut stmt = c.prepare(
            "SELECT ts, state, temp, poh, realloc, pending, uncorrect, crc, wear, written, cycles FROM disk_samples
             WHERE disk = ?1 AND ts >= ?2 AND state = 'active' ORDER BY ts LIMIT 1",
        )?;
        let mut out = HashMap::new();
        for (d, since) in disks {
            if let Some(s) = stmt.query_row(params![d, since], row_to_sample).optional()? {
                out.insert(d, s);
            }
        }
        Ok(out)
    })
    .await
}

/// Temperatures since `since` for every disk, oldest first.
pub async fn recent_temps(db: &Db, since: i64) -> Result<HashMap<String, Vec<(i64, Option<f64>)>>> {
    db.call(move |c| {
        let mut stmt = c.prepare("SELECT disk, ts, temp FROM disk_samples WHERE ts >= ?1 ORDER BY disk, ts")?;
        let rows = stmt
            .query_map([since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<f64>>(2)?)))?;
        let mut out: HashMap<String, Vec<(i64, Option<f64>)>> = HashMap::new();
        for row in rows {
            let (disk, ts, temp) = row?;
            out.entry(disk).or_default().push((ts, temp));
        }
        Ok(out)
    })
    .await
}

/// A time bucket of a disk's history.
#[derive(Debug, Clone, Default, Serialize)]
pub struct HistoryPoint {
    pub ts: i64,
    pub temp: Option<f64>,
    pub temp_max: Option<f64>,
    pub poh: Option<i64>,
    pub realloc: Option<i64>,
    pub pending: Option<i64>,
    pub uncorrect: Option<i64>,
    pub crc: Option<i64>,
    pub wear: Option<f64>,
    pub written: Option<i64>,
    pub cycles: Option<i64>,
    /// Share of readings in the bucket that found the disk asleep.
    pub asleep: f64,
}

pub async fn history(db: &Db, disk: String, since: i64, bucket: i64) -> Result<Vec<HistoryPoint>> {
    let bucket = bucket.max(60);
    db.call(move |c| {
        let mut stmt = c.prepare(
            "SELECT (ts / ?3) * ?3 AS t, AVG(temp), MAX(temp), MAX(poh), MAX(realloc), MAX(pending), MAX(uncorrect),
                    MAX(crc), MAX(wear), MAX(written), MAX(cycles),
                    SUM(CASE WHEN state = 'standby' THEN 1 ELSE 0 END), COUNT(*)
             FROM disk_samples WHERE disk = ?1 AND ts >= ?2 GROUP BY t ORDER BY t",
        )?;
        let rows = stmt.query_map(params![disk, since, bucket], |r| {
            let asleep: i64 = r.get(11)?;
            let count: i64 = r.get(12)?;
            Ok(HistoryPoint {
                ts: r.get(0)?,
                temp: r.get(1)?,
                temp_max: r.get(2)?,
                poh: r.get(3)?,
                realloc: r.get(4)?,
                pending: r.get(5)?,
                uncorrect: r.get(6)?,
                crc: r.get(7)?,
                wear: r.get(8)?,
                written: r.get(9)?,
                cycles: r.get(10)?,
                asleep: if count > 0 { asleep as f64 / count as f64 } else { 0.0 },
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    })
    .await
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

const TEST_COLUMNS: &str = "id, disk, node, devpath, kind, status, queued_at, started_at, finished_at, progress, \
                            expected_secs, start_hours, log_top, result, lba, message, actor, schedule";

fn row_to_test(r: &rusqlite::Row<'_>) -> rusqlite::Result<TestRun> {
    Ok(TestRun {
        id: r.get(0)?,
        disk: r.get(1)?,
        node: r.get(2)?,
        devpath: r.get(3)?,
        kind: r.get(4)?,
        status: r.get(5)?,
        queued_at: r.get(6)?,
        started_at: r.get(7)?,
        finished_at: r.get(8)?,
        progress: r.get(9)?,
        expected_secs: r.get(10)?,
        start_hours: r.get(11)?,
        log_top: r.get(12)?,
        result: r.get(13)?,
        lba: r.get(14)?,
        message: r.get(15)?,
        actor: r.get(16)?,
        schedule: r.get(17)?,
    })
}

/// Inserts or updates a test run.
pub async fn save_test(db: &Db, t: TestRun) -> Result<()> {
    db.call(move |c| {
        c.execute(
            &format!(
                "INSERT INTO disk_tests ({TEST_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
                 ON CONFLICT(id) DO UPDATE SET node = excluded.node, devpath = excluded.devpath, status = excluded.status,
                   started_at = excluded.started_at, finished_at = excluded.finished_at, progress = excluded.progress,
                   expected_secs = excluded.expected_secs, start_hours = excluded.start_hours, log_top = excluded.log_top,
                   result = excluded.result, lba = excluded.lba, message = excluded.message"
            ),
            params![
                t.id,
                t.disk,
                t.node,
                t.devpath,
                t.kind,
                t.status,
                t.queued_at,
                t.started_at,
                t.finished_at,
                t.progress,
                t.expected_secs,
                t.start_hours,
                t.log_top,
                t.result,
                t.lba,
                t.message,
                t.actor,
                t.schedule
            ],
        )?;
        Ok(())
    })
    .await
}

pub async fn get_test(db: &Db, id: String) -> Result<Option<TestRun>> {
    db.call(move |c| {
        Ok(c.query_row(&format!("SELECT {TEST_COLUMNS} FROM disk_tests WHERE id = ?1"), [id], row_to_test)
            .optional()?)
    })
    .await
}

/// Queued and running tests, oldest first.
pub async fn active_tests(db: &Db) -> Result<Vec<TestRun>> {
    db.call(|c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {TEST_COLUMNS} FROM disk_tests WHERE status IN ('queued', 'running') ORDER BY queued_at"
        ))?;
        let rows = stmt.query_map([], row_to_test)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    })
    .await
}

/// Finished (or skipped) tests, newest first, for one disk or all.
pub async fn recent_tests(db: &Db, disk: Option<String>, limit: u32) -> Result<Vec<TestRun>> {
    db.call(move |c| {
        let limit = limit.clamp(1, 500);
        let rows = match disk {
            Some(d) => {
                let mut stmt = c.prepare(&format!(
                    "SELECT {TEST_COLUMNS} FROM disk_tests WHERE disk = ?1 AND status NOT IN ('queued', 'running')
                     ORDER BY COALESCE(finished_at, queued_at) DESC LIMIT {limit}"
                ))?;
                stmt.query_map([d], row_to_test)?.collect::<std::result::Result<Vec<_>, _>>()?
            }
            None => {
                let mut stmt = c.prepare(&format!(
                    "SELECT {TEST_COLUMNS} FROM disk_tests WHERE status NOT IN ('queued', 'running')
                     ORDER BY COALESCE(finished_at, queued_at) DESC LIMIT {limit}"
                ))?;
                stmt.query_map([], row_to_test)?.collect::<std::result::Result<Vec<_>, _>>()?
            }
        };
        Ok(rows)
    })
    .await
}

/// Each disk's most recent finished test (passed, failed, aborted, interrupted).
pub async fn last_finished_tests(db: &Db) -> Result<HashMap<String, TestRun>> {
    db.call(|c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {TEST_COLUMNS} FROM disk_tests
             WHERE status IN ('passed', 'failed', 'aborted', 'interrupted') ORDER BY finished_at"
        ))?;
        let rows = stmt.query_map([], row_to_test)?;
        let mut out = HashMap::new();
        for row in rows {
            let t = row?;
            out.insert(t.disk.clone(), t);
        }
        Ok(out)
    })
    .await
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct EventRow {
    pub id: i64,
    pub subject: String,
    pub ts: i64,
    pub severity: Severity,
    pub kind: String,
    pub message: String,
}

/// A new event about a disk (its id) or pool ("pool:<id>").
#[derive(Debug, Clone)]
pub struct NewEvent {
    pub subject: String,
    pub severity: Severity,
    pub kind: &'static str,
    pub message: String,
}

impl NewEvent {
    pub fn new(subject: impl Into<String>, severity: Severity, kind: &'static str, message: impl Into<String>) -> Self {
        Self { subject: subject.into(), severity, kind, message: message.into() }
    }
}

pub async fn insert_events(db: &Db, ts: i64, events: Vec<NewEvent>) -> Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    db.call(move |c| {
        let tx = c.transaction()?;
        for e in &events {
            tx.execute(
                "INSERT INTO disk_events (subject, ts, severity, kind, message) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![e.subject, ts, e.severity.as_str(), e.kind, e.message],
            )?;
        }
        tx.commit()?;
        Ok(())
    })
    .await
}

/// Events about one subject, or all of them, newest first.
pub async fn events(db: &Db, subject: Option<String>, limit: u32) -> Result<Vec<EventRow>> {
    db.call(move |c| {
        let limit = limit.clamp(1, 1000);
        let map = |r: &rusqlite::Row<'_>| {
            let sev: String = r.get(3)?;
            Ok(EventRow {
                id: r.get(0)?,
                subject: r.get(1)?,
                ts: r.get(2)?,
                severity: serde_json::from_value(serde_json::Value::String(sev)).unwrap_or_default(),
                kind: r.get(4)?,
                message: r.get(5)?,
            })
        };
        let rows = match subject {
            Some(s) => {
                let mut stmt = c.prepare(&format!(
                    "SELECT id, subject, ts, severity, kind, message FROM disk_events WHERE subject = ?1 ORDER BY ts DESC, id DESC LIMIT {limit}"
                ))?;
                stmt.query_map([s], map)?.collect::<std::result::Result<Vec<_>, _>>()?
            }
            None => {
                let mut stmt = c.prepare(&format!(
                    "SELECT id, subject, ts, severity, kind, message FROM disk_events ORDER BY ts DESC, id DESC LIMIT {limit}"
                ))?;
                stmt.query_map([], map)?.collect::<std::result::Result<Vec<_>, _>>()?
            }
        };
        Ok(rows)
    })
    .await
}

// ---------------------------------------------------------------------------
// Pools
// ---------------------------------------------------------------------------

/// Records pool sizes (one row per pool and hour).
pub async fn insert_pool_samples(db: &Db, ts: i64, rows: Vec<(String, Option<u64>, Option<u64>)>) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let hour = ts - ts.rem_euclid(3600);
    db.call(move |c| {
        let tx = c.transaction()?;
        for (pool, size, used) in &rows {
            tx.execute(
                "INSERT OR REPLACE INTO pool_samples (pool, ts, size, used) VALUES (?1, ?2, ?3, ?4)",
                params![pool, hour, size.and_then(|n| i64::try_from(n).ok()), used.and_then(|n| i64::try_from(n).ok())],
            )?;
        }
        tx.commit()?;
        Ok(())
    })
    .await
}

/// (ts, used) of each pool since `since`, oldest first.
pub async fn pool_series(db: &Db, since: i64) -> Result<HashMap<String, Vec<(i64, i64)>>> {
    db.call(move |c| {
        let mut stmt =
            c.prepare("SELECT pool, ts, used FROM pool_samples WHERE ts >= ?1 AND used IS NOT NULL ORDER BY pool, ts")?;
        let rows =
            stmt.query_map([since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))?;
        let mut out: HashMap<String, Vec<(i64, i64)>> = HashMap::new();
        for row in rows {
            let (pool, ts, used) = row?;
            out.entry(pool).or_default().push((ts, used));
        }
        Ok(out)
    })
    .await
}

/// (ts, size, used) of one pool since `since`, for its chart.
pub async fn pool_history(db: &Db, pool: String, since: i64) -> Result<Vec<(i64, Option<i64>, Option<i64>)>> {
    db.call(move |c| {
        let mut stmt = c.prepare("SELECT ts, size, used FROM pool_samples WHERE pool = ?1 AND ts >= ?2 ORDER BY ts")?;
        let rows = stmt.query_map(params![pool, since], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    })
    .await
}

/// Drops history older than `cutoff` (keeps queued and running tests).
pub async fn prune(db: &Db, cutoff: i64) -> Result<()> {
    db.call(move |c| {
        let tx = c.transaction()?;
        tx.execute("DELETE FROM disk_samples WHERE ts < ?1", [cutoff])?;
        tx.execute("DELETE FROM disk_events WHERE ts < ?1", [cutoff])?;
        tx.execute("DELETE FROM disk_tests WHERE queued_at < ?1 AND status NOT IN ('queued', 'running')", [cutoff])?;
        tx.execute("DELETE FROM pool_samples WHERE ts < ?1", [cutoff])?;
        tx.commit()?;
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn samples_history_and_baselines() {
        let db = Db::open_in_memory().unwrap();
        let s = |ts: i64, realloc: i64, temp: f64| Sample {
            ts,
            state: "active".into(),
            temp: Some(temp),
            realloc: Some(realloc),
            ..Default::default()
        };
        insert_samples(
            &db,
            vec![
                ("a".into(), s(1000, 0, 30.0)),
                ("a".into(), Sample::state_only(1500, "standby")),
                ("a".into(), s(2000, 2, 40.0)),
                ("a".into(), s(4000, 5, 35.0)),
                ("b".into(), s(1000, 0, 20.0)),
            ],
        )
        .await
        .unwrap();
        let base = baselines(&db, vec![("a".into(), 1200), ("b".into(), 1200), ("c".into(), 0)]).await.unwrap();
        assert_eq!(base["a"].ts, 2000, "standby rows are skipped");
        assert!(!base.contains_key("b"));
        // Each disk is measured from its own time.
        let base = baselines(&db, vec![("a".into(), 0), ("b".into(), 0)]).await.unwrap();
        assert_eq!((base["a"].ts, base["b"].ts), (1000, 1000));
        let h = history(&db, "a".into(), 0, 3000).await.unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].temp, Some(35.0));
        assert_eq!(h[0].temp_max, Some(40.0));
        assert_eq!(h[0].realloc, Some(2));
        assert!((h[0].asleep - 1.0 / 3.0).abs() < 1e-9);
        let temps = recent_temps(&db, 0).await.unwrap();
        assert_eq!(temps["a"].len(), 4);
        prune(&db, 3000).await.unwrap();
        assert_eq!(history(&db, "a".into(), 0, 60).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn tests_and_events_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let mut t = TestRun {
            id: "t1".into(),
            disk: "d".into(),
            node: "pve".into(),
            devpath: "/dev/sda".into(),
            kind: "short".into(),
            status: "queued".into(),
            queued_at: 10,
            ..Default::default()
        };
        save_test(&db, t.clone()).await.unwrap();
        assert_eq!(active_tests(&db).await.unwrap().len(), 1);
        t.status = "passed".into();
        t.finished_at = Some(20);
        t.result = Some("Completed without error".into());
        save_test(&db, t.clone()).await.unwrap();
        assert!(active_tests(&db).await.unwrap().is_empty());
        assert_eq!(
            get_test(&db, "t1".into()).await.unwrap().unwrap().result.as_deref(),
            Some("Completed without error")
        );
        assert_eq!(last_finished_tests(&db).await.unwrap()["d"].id, "t1");
        assert_eq!(recent_tests(&db, Some("d".into()), 5).await.unwrap().len(), 1);

        insert_events(&db, 5, vec![NewEvent::new("d", Severity::Warning, "attribute", "Pending sectors: 0 → 2")])
            .await
            .unwrap();
        let ev = events(&db, Some("d".into()), 10).await.unwrap();
        assert_eq!(ev[0].severity, Severity::Warning);
        assert_eq!(ev[0].kind, "attribute");
        forget(&db, "d".into()).await.unwrap();
        assert!(events(&db, None, 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn disks_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let view = DiskView {
            id: "WDC_X".into(),
            node: "pve".into(),
            devpath: "/dev/sda".into(),
            first_seen: 10,
            last_seen: 20,
            ..Default::default()
        };
        let report = Report { temperature: Some(33.0), ..Default::default() };
        save_disks(&db, vec![(view.clone(), Some(serde_json::to_string(&report).unwrap()))]).await.unwrap();
        // A later save without a new report keeps the stored one.
        let moved = DiskView { node: "pve2".into(), devpath: "/dev/sdc".into(), last_seen: 30, ..view.clone() };
        save_disks(&db, vec![(moved, None)]).await.unwrap();
        let label = Label { bay: "Bay 3".into(), purchased: "2021-03-14".into(), ..Default::default() };
        assert!(save_label(&db, "WDC_X".into(), &label).await.unwrap());
        assert!(!save_label(&db, "nope".into(), &label).await.unwrap());
        let rows = load_disks(&db).await.unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!((row.view.node.as_str(), row.view.devpath.as_str()), ("pve2", "/dev/sdc"));
        assert_eq!((row.view.first_seen, row.view.last_seen), (10, 30));
        assert_eq!(row.view.label.bay, "Bay 3");
        assert_eq!(row.report.as_ref().and_then(|r| r.temperature), Some(33.0));
        forget(&db, "WDC_X".into()).await.unwrap();
        assert!(load_disks(&db).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn pool_samples_are_hourly() {
        let db = Db::open_in_memory().unwrap();
        insert_pool_samples(&db, 3600 + 10, vec![("p".into(), Some(100), Some(10))]).await.unwrap();
        insert_pool_samples(&db, 3600 + 20, vec![("p".into(), Some(100), Some(11))]).await.unwrap();
        insert_pool_samples(&db, 7200 + 5, vec![("p".into(), Some(100), Some(12))]).await.unwrap();
        let series = pool_series(&db, 0).await.unwrap();
        assert_eq!(series["p"], vec![(3600, 11), (7200, 12)]);
        assert_eq!(pool_history(&db, "p".into(), 0).await.unwrap().len(), 2);
    }
}
