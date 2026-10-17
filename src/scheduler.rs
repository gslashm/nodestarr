//! Cron-style scheduler for update checks, automatic updates, backups and
//! housekeeping. It wakes every 30 seconds, which costs nothing measurable,
//! and runs any job whose schedule fired since the last wake-up.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::Serialize;

use crate::activity::Actor;
use crate::app::App;
use crate::db::RetentionCutoffs;
use crate::settings::{Settings, parse_cron};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Job {
    UpdateCheck,
    AutoUpdate,
    Backup,
    Maintenance,
    NodestarrUpdate,
}

impl Job {
    pub const ALL: [Job; 5] = [Job::UpdateCheck, Job::AutoUpdate, Job::Backup, Job::Maintenance, Job::NodestarrUpdate];

    /// Human-readable name.
    pub fn name(self) -> &'static str {
        match self {
            Job::UpdateCheck => "update check",
            Job::AutoUpdate => "automatic updates",
            Job::Backup => "scheduled backup",
            Job::Maintenance => "maintenance",
            Job::NodestarrUpdate => "Nodestarr update check",
        }
    }

    /// The identifier used in URLs and JSON.
    pub fn key(self) -> &'static str {
        match self {
            Job::UpdateCheck => "update-check",
            Job::AutoUpdate => "auto-update",
            Job::Backup => "backup",
            Job::Maintenance => "maintenance",
            Job::NodestarrUpdate => "nodestarr-update",
        }
    }
}

impl std::str::FromStr for Job {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        Job::ALL.into_iter().find(|j| j.key() == s).ok_or_else(|| format!("unknown job '{s}'"))
    }
}

/// When a job last started and finished.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct JobRun {
    pub started_at: i64,
    /// None while the job is running.
    pub finished_at: Option<i64>,
}

static RUNS: LazyLock<Mutex<HashMap<Job, JobRun>>> = LazyLock::new(Default::default);

/// Last run of each job since Nodestarr started.
pub fn last_runs() -> HashMap<Job, JobRun> {
    RUNS.lock().clone()
}

/// The cron expression for each enabled job.
fn schedule(s: &Settings) -> Vec<(Job, String)> {
    let mut jobs = vec![
        (Job::UpdateCheck, s.updates.check_cron.clone()),
        (Job::AutoUpdate, s.updates.auto_cron.clone()),
        (Job::Maintenance, "5 3 * * *".to_string()),
    ];
    if s.backups.schedule_enabled {
        jobs.push((Job::Backup, s.backups.cron.clone()));
    }
    if s.general.check_nodestarr_updates {
        jobs.push((Job::NodestarrUpdate, "41 9 * * *".to_string()));
    }
    jobs
}

/// Next run time of each job, for the UI.
pub fn next_runs(s: &Settings) -> Vec<(Job, Option<DateTime<chrono_tz::Tz>>)> {
    let tz = s.tz();
    let now = Utc::now().with_timezone(&tz);
    schedule(s)
        .into_iter()
        .map(|(job, expr)| {
            let next = parse_cron(&expr).ok().and_then(|c| c.find_next_occurrence(&now, false).ok());
            (job, next)
        })
        .collect()
}

/// True when `expr` had an occurrence in (from, to].
pub(crate) fn fired_between(expr: &str, from: &DateTime<chrono_tz::Tz>, to: &DateTime<chrono_tz::Tz>) -> bool {
    match parse_cron(expr) {
        Ok(cron) => cron.find_next_occurrence(from, false).map(|next| next <= *to).unwrap_or(false),
        Err(_) => false,
    }
}

pub fn start(app: App) {
    tokio::spawn(async move {
        // Catch up on a stale update check shortly after startup.
        {
            let app = app.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(90)).await;
                let newest = app.updates.all().values().filter_map(|i| i.checked_at).max().unwrap_or(0);
                if crate::util::now() - newest > 6 * 3600 {
                    run(&app, Job::UpdateCheck).await;
                }
            });
        }
        let mut last = Utc::now().with_timezone(&app.settings.get().tz());
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let settings = app.settings.get();
            let now = Utc::now().with_timezone(&settings.tz());
            let from = last.with_timezone(&settings.tz());
            for (job, expr) in schedule(&settings) {
                if !fired_between(&expr, &from, &now) {
                    continue;
                }
                let app = app.clone();
                tokio::spawn(async move {
                    tracing::info!(job = job.name(), "scheduled job starting");
                    if !run(&app, job).await {
                        tracing::info!(job = job.name(), "previous run still in progress, skipping");
                    }
                });
            }
            last = now;
        }
    });
}

/// Runs one job now unless it is already running (also used by "Run now"
/// buttons). Returns false when skipped.
pub async fn run(app: &App, job: Job) -> bool {
    {
        let mut runs = RUNS.lock();
        if runs.get(&job).is_some_and(|r| r.finished_at.is_none()) {
            return false;
        }
        runs.insert(job, JobRun { started_at: crate::util::now(), finished_at: None });
    }
    execute(app, job).await;
    if let Some(r) = RUNS.lock().get_mut(&job) {
        r.finished_at = Some(crate::util::now());
    }
    true
}

async fn execute(app: &App, job: Job) {
    match job {
        Job::UpdateCheck => {
            crate::updates::check::check_all(app, false, &Actor::schedule()).await;
        }
        Job::AutoUpdate => crate::updates::auto::run(app).await,
        Job::Backup => crate::backups::run_scheduled(app).await,
        Job::Maintenance => maintenance(app).await,
        Job::NodestarrUpdate => crate::selfupdate::check_for_release(app).await,
    }
}

async fn maintenance(app: &App) {
    let s = app.settings.get();
    let now = crate::util::now();
    let day = 86400;
    let cutoffs = RetentionCutoffs {
        activity: now - s.monitoring.activity_retention_days.max(1) as i64 * day,
        tasks: now - s.monitoring.task_retention_days.max(1) as i64 * day,
        alerts: now - 90 * day,
        update_history: now - 365 * day,
    };
    if let Err(err) = app.db.maintenance(cutoffs).await {
        tracing::warn!(%err, "database maintenance failed");
    }
    if let Err(err) = crate::monitor::uptime::prune(app).await {
        tracing::warn!(%err, "uptime prune failed");
    }
    // Apply backup retention even to containers that were not backed up today.
    let identities: HashSet<String> = app.docker.entries().iter().map(|e| e.summary.identity.clone()).collect();
    for identity in identities {
        let _ = crate::backups::apply_retention(app, &identity).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn fires_once_per_window() {
        let tz: chrono_tz::Tz = "Europe/Berlin".parse().unwrap();
        let from = tz.with_ymd_and_hms(2026, 10, 14, 3, 59, 40).unwrap();
        let to = tz.with_ymd_and_hms(2026, 10, 14, 4, 0, 10).unwrap();
        assert!(fired_between("0 4 * * *", &from, &to));
        let later = tz.with_ymd_and_hms(2026, 10, 14, 4, 0, 40).unwrap();
        assert!(!fired_between("0 4 * * *", &to, &later));
        assert!(!fired_between("not cron", &from, &to));
    }

    #[test]
    fn schedule_respects_toggles() {
        let mut s = Settings::default();
        assert!(!schedule(&s).iter().any(|(j, _)| *j == Job::Backup));
        s.backups.schedule_enabled = true;
        assert!(schedule(&s).iter().any(|(j, _)| *j == Job::Backup));
    }
}
