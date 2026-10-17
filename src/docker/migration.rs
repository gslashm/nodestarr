//! Spotting data migrations in a starting container's output.
//!
//! A new version often migrates its database before it answers, which can
//! take far longer than a normal start, and its health check can't pass
//! until it's done. Updates watch for this so they wait instead of rolling
//! back halfway through: the previous version usually can't read data a
//! newer one has migrated.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bollard::Docker;
use futures_util::StreamExt;
use parking_lot::Mutex;

use crate::docker::logs::{self, LogQuery};

/// Follows a container's output in the background, noting whether it shows
/// a data migration. Stops following when dropped.
pub struct MigrationWatch {
    /// The line that showed a migration, while one is going on.
    seen: Arc<Mutex<Option<String>>>,
    /// How long to wait in all for a container that is migrating.
    limit: Duration,
    log: Box<dyn Fn(String) + Send + Sync>,
    told: AtomicBool,
    task: tokio::task::JoinHandle<()>,
}

impl MigrationWatch {
    /// Starts reading the container's output from its start. `limit` is how
    /// long a migrating container may take in all; `log` hears when that
    /// longer wait begins.
    pub fn start(api: &Docker, id: &str, limit: Duration, log: impl Fn(String) + Send + Sync + 'static) -> Self {
        let seen = Arc::new(Mutex::new(None));
        let q = LogQuery {
            tail: Some("all".into()),
            since: None,
            until: None,
            timestamps: false,
            stdout: true,
            stderr: true,
        };
        let stream = logs::follow(api, id, &q);
        let state = seen.clone();
        let task = tokio::spawn(async move {
            let mut stream = std::pin::pin!(stream);
            while let Some(Ok(lines)) = stream.next().await {
                for line in lines {
                    let text = logs::strip_ansi(&line.t);
                    match classify(&text) {
                        Some(Sign::Migrating) => {
                            state.lock().get_or_insert_with(|| crate::util::truncate(text.trim(), 160));
                        }
                        Some(Sign::NothingToMigrate) => *state.lock() = None,
                        None => {}
                    }
                }
            }
        });
        Self { seen, limit, log: Box::new(log), told: AtomicBool::new(false), task }
    }

    /// Whether the output shows a migration going on.
    pub fn is_migrating(&self) -> bool {
        self.seen.lock().is_some()
    }

    /// How long to wait in all for the container to be ready, given the
    /// `usual` limit: longer while it is migrating. Says so in the log once
    /// the usual limit has passed (`waited` is the time spent so far).
    pub fn limit(&self, usual: Duration, waited: Duration) -> Duration {
        let Some(line) = self.seen.lock().clone() else { return usual };
        let limit = self.limit.max(usual);
        if waited >= usual && limit > usual && !self.told.swap(true, Ordering::Relaxed) {
            (self.log)(format!(
                "Not ready after {}, but its output shows a data migration (\"{line}\"); waiting up to {} in all for it to finish",
                crate::util::human_duration(usual.as_secs() as i64),
                crate::util::human_duration(limit.as_secs() as i64),
            ));
        }
        limit
    }
}

impl Drop for MigrationWatch {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Debug, PartialEq)]
enum Sign {
    Migrating,
    NothingToMigrate,
}

/// Phrases (besides words starting with "migrat") that show a migration.
const MIGRATING: &[&str] = &[
    "running upgrade ", // Alembic: "Running upgrade 1a2b -> 3c4d"
    "upgrading database",
    "upgrading the database",
    "database upgrade",
    "upgrading schema",
    "schema upgrade",
    "updating database schema",
    "updating the database schema",
];

/// Phrases that, next to one of the above, say there is nothing to do.
const NOTHING: &[&str] = &[
    "no migration",
    "no pending migration",
    "no new migration",
    "nothing to migrate",
    "no need to migrate",
    "not needed",
    "not required",
    "up to date",
    "up-to-date",
    "already applied",
    "already migrated",
];

/// Lines some apps print on every start, migrating or not.
const ROUTINE: &[&str] = &[
    // Sonarr, Radarr and the other *arr apps check the SQLite version and
    // open each database through their migration runner every time.
    "databaseengineversioncheck",
    "*** migrating data source=",
];

/// What a line of output says about migrations, if anything: e.g.
/// "Running migrations:", "Applying migration 0042", "== CreateUsers:
/// migrating ==" or "Upgrading database schema" show one, "No migrations
/// to apply." says there is none.
fn classify(line: &str) -> Option<Sign> {
    let l = line.to_lowercase();
    let about = starts_word(&l, "migrat") || MIGRATING.iter().any(|p| l.contains(p));
    if !about || ROUTINE.iter().any(|p| l.contains(p)) {
        None
    } else if NOTHING.iter().any(|p| l.contains(p)) {
        Some(Sign::NothingToMigrate)
    } else {
        Some(Sign::Migrating)
    }
}

/// Whether `word` starts a word in `s`. Dotted names don't count, so
/// "[alembic.runtime.migration]", a logger name printed on every start, is
/// passed over.
fn starts_word(s: &str, word: &str) -> bool {
    s.match_indices(word)
        .any(|(i, _)| s[..i].chars().next_back().is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '.' | '_' | '/'))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spots_migrations() {
        for line in [
            "Running migrations:",
            "== 20240101000000 CreateUsers: migrating =====================",
            "[Info] FluentMigrator.Runner.MigrationRunner: 210: add_series_tags migrating",
            "INFO  [alembic.runtime.migration] Running upgrade 1a2b -> 3c4d, add column",
            "2024/01/01 00:00:00 ...migrations.go:123:Migrate() [I] Migration[245]: Add column",
            "Migrating database to version 12",
            "Updating database schema",
            "Upgrading the database, this may take a while",
            "[Nest] 7  - LOG [Migrations] Running migrations, please wait",
            "\u{1b}[1mApplying migration `20240101_init`\u{1b}[0m",
        ] {
            assert_eq!(classify(&logs::strip_ansi(line)), Some(Sign::Migrating), "{line}");
        }
    }

    #[test]
    fn hears_when_there_is_nothing_to_migrate() {
        for line in [
            "  No migrations to apply.",
            "No pending migrations to apply.",
            "Database upgrade not needed",
            "Migrations are up to date",
        ] {
            assert_eq!(classify(line), Some(Sign::NothingToMigrate), "{line}");
        }
    }

    #[test]
    fn ignores_routine_output() {
        for line in [
            "INFO  [alembic.runtime.migration] Context impl PostgresqlImpl.",
            "INFO  [alembic.runtime.migration] Will assume transactional DDL.",
            "[Info] FluentMigrator.Runner.MigrationRunner: DatabaseEngineVersionCheck migrating",
            "[Info] MigrationController: *** Migrating data source=/config/sonarr.db;cache size=-20000 ***",
            "Loading /app/migrations/0001_init.sql",
            "Database is up to date",
            "Server started on port 8080",
        ] {
            assert_eq!(classify(line), None, "{line}");
        }
    }
}
