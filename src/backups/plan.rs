//! Working out what to back up for a container, and how.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::app::App;
use crate::docker::Entry;
use crate::docker::apps::{self, DbEngine};
use crate::settings::BackupStrategy;

/// How a mount's files are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    /// The host path is visible to Nodestarr (same path mounted in).
    Direct,
    /// Streamed through Docker's archive API (named volumes, unmounted paths).
    DockerCopy,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlannedMount {
    pub destination: String,
    pub source: String,
    /// "bind" or "volume".
    pub kind: String,
    pub include: bool,
    /// Why it is included or excluded (shown in the UI).
    pub reason: String,
    pub access: Access,
}

#[derive(Debug, Clone, Serialize)]
pub struct DbPlan {
    pub engine: DbEngine,
    /// Shell command run inside the container; writes the dump to stdout.
    #[serde(skip)]
    pub command: Vec<String>,
    /// Restore command; reads the dump from stdin.
    #[serde(skip)]
    pub restore_command: Vec<String>,
    pub description: String,
}

/// The resolved method actually used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    Live,
    Pause,
    Stop,
    /// The app's own backup feature (via its API).
    Native,
    /// A database dump (plus any included files, copied live).
    Dump,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupPlan {
    pub enabled: bool,
    pub method: Method,
    pub mounts: Vec<PlannedMount>,
    pub excludes: Vec<String>,
    pub db: Option<DbPlan>,
    pub native: bool,
    pub compose_files: Vec<String>,
    pub warnings: Vec<String>,
}

impl BackupPlan {
    pub fn has_content(&self) -> bool {
        self.native || self.db.is_some() || self.mounts.iter().any(|m| m.include)
    }
}

/// Host or container paths that never hold app data worth backing up.
const SYSTEM_PREFIXES: &[&str] =
    &["/etc", "/dev", "/proc", "/sys", "/run", "/var/run", "/tmp", "/usr", "/lib", "/lib64", "/bin", "/sbin", "/boot"];

/// Matches whole path components, so "/runner" is not mistaken for "/run".
fn is_system_path(p: &str) -> bool {
    let path = Path::new(p);
    SYSTEM_PREFIXES.iter().any(|s| path.starts_with(s))
}

fn env_map(entry: &Entry) -> std::collections::HashMap<String, String> {
    entry
        .inspect
        .config
        .as_ref()
        .and_then(|c| c.env.as_ref())
        .map(|v| v.iter().filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect())
        .unwrap_or_default()
}

/// Builds the dump/restore commands for a database container. Credentials
/// stay inside the container: commands reference its own env vars.
pub fn db_plan(engine: DbEngine, entry: &Entry) -> Option<DbPlan> {
    let env = env_map(entry);
    match engine {
        DbEngine::Postgres => {
            let user = env.get("POSTGRES_USER").cloned().unwrap_or_else(|| "postgres".into());
            let sh = |cmd: String| vec!["sh".into(), "-c".into(), cmd];
            Some(DbPlan {
                engine,
                command: sh(format!(
                    "PGPASSWORD=\"${{POSTGRES_PASSWORD:-}}\" exec pg_dumpall --clean --if-exists -U {}",
                    shell_words::quote(&user)
                )),
                restore_command: sh(format!(
                    "PGPASSWORD=\"${{POSTGRES_PASSWORD:-}}\" exec psql -X -q -v ON_ERROR_STOP=0 -U {} -d postgres",
                    shell_words::quote(&user)
                )),
                description: format!("pg_dumpall as {user}"),
            })
        }
        DbEngine::Mysql | DbEngine::Mariadb => {
            let has_root = env.contains_key("MYSQL_ROOT_PASSWORD") || env.contains_key("MARIADB_ROOT_PASSWORD");
            let (auth, scope) = if has_root {
                (
                    "-uroot -p\"${MARIADB_ROOT_PASSWORD:-$MYSQL_ROOT_PASSWORD}\"".to_string(),
                    "--all-databases".to_string(),
                )
            } else if env.contains_key("MYSQL_USER") || env.contains_key("MARIADB_USER") {
                (
                    "-u\"${MARIADB_USER:-$MYSQL_USER}\" -p\"${MARIADB_PASSWORD:-$MYSQL_PASSWORD}\"".to_string(),
                    "--databases \"${MARIADB_DATABASE:-$MYSQL_DATABASE}\"".to_string(),
                )
            } else {
                return None;
            };
            Some(DbPlan {
                engine,
                command: vec![
                    "sh".into(),
                    "-c".into(),
                    format!(
                        "D=\"$(command -v mariadb-dump || command -v mysqldump)\"; exec \"$D\" {scope} --single-transaction --routines --events --triggers {auth}"
                    ),
                ],
                restore_command: vec![
                    "sh".into(),
                    "-c".into(),
                    format!("C=\"$(command -v mariadb || command -v mysql)\"; exec \"$C\" {auth}"),
                ],
                description: if has_root { "dump of all databases as root".into() } else { "dump of the app database".into() },
            })
        }
        DbEngine::Mongo => Some(DbPlan {
            engine,
            command: vec![
                "sh".into(),
                "-c".into(),
                "if [ -n \"$MONGO_INITDB_ROOT_USERNAME\" ]; then exec mongodump --archive --authenticationDatabase admin -u \"$MONGO_INITDB_ROOT_USERNAME\" -p \"$MONGO_INITDB_ROOT_PASSWORD\"; else exec mongodump --archive; fi".into(),
            ],
            restore_command: vec![
                "sh".into(),
                "-c".into(),
                "if [ -n \"$MONGO_INITDB_ROOT_USERNAME\" ]; then exec mongorestore --archive --drop --authenticationDatabase admin -u \"$MONGO_INITDB_ROOT_USERNAME\" -p \"$MONGO_INITDB_ROOT_PASSWORD\"; else exec mongorestore --archive --drop; fi".into(),
            ],
            description: "mongodump archive".into(),
        }),
        DbEngine::Redis => None,
    }
}

/// Builds the backup plan for a container from settings, preferences and
/// the app catalog.
pub fn plan(app: &App, entry: &Entry) -> BackupPlan {
    let settings = app.settings.get();
    let prefs = app.prefs.get(&entry.summary.identity);
    let s = &entry.summary;
    let info = s.app.as_deref().and_then(apps::by_slug);
    let db_engine = info.and_then(|a| a.db).filter(|e| *e != DbEngine::Redis);
    let stacks_dirs = &settings.docker.stacks_dirs;
    let mut warnings = Vec::new();

    let mut mounts = Vec::new();
    for m in &s.mounts {
        if m.kind != "bind" && m.kind != "volume" {
            continue;
        }
        let explicit = prefs.backup.mounts.iter().find(|c| c.destination == m.destination);
        let source_path = Path::new(&m.source);
        let (include, reason) = if let Some(choice) = explicit {
            (choice.include, if choice.include { "included by you".to_string() } else { "excluded by you".to_string() })
        } else if is_system_path(&m.destination) || (m.kind == "bind" && is_system_path(&m.source)) {
            (false, "system path".into())
        } else if db_engine.is_some() {
            (false, "database files are backed up as a dump instead".into())
        } else if info.is_some_and(|a| a.data_paths.iter().any(|d| Path::new(&m.destination).starts_with(d))) {
            (false, "media or bulk data".into())
        } else if info.is_some_and(|a| a.config_paths.iter().any(|c| Path::new(&m.destination).starts_with(c))) {
            (true, "app configuration".into())
        } else if m.kind == "volume" {
            (true, "named volume".into())
        } else if settings.backups.data_roots.iter().any(|r| source_path.starts_with(r)) {
            (false, "under a data folder (see Settings > Backups)".into())
        } else if stacks_dirs.iter().any(|d| source_path.starts_with(d))
            || s.compose_dir.as_deref().is_some_and(|d| source_path.starts_with(d))
        {
            (true, "inside the stack folder".into())
        } else {
            (true, "bind mount".into())
        };
        let access = if m.kind == "bind" && source_path.exists() { Access::Direct } else { Access::DockerCopy };
        mounts.push(PlannedMount {
            destination: m.destination.clone(),
            source: m.source.clone(),
            kind: m.kind.clone(),
            include,
            reason,
            access,
        });
    }

    let mut excludes: Vec<String> = settings.backups.exclude.clone();
    if let Some(a) = info {
        excludes.extend(a.backup_exclude.iter().map(|s| s.to_string()));
    }
    excludes.extend(prefs.backup.exclude.iter().cloned());
    excludes.sort();
    excludes.dedup();

    let db = db_engine.and_then(|e| {
        let plan = db_plan(e, entry);
        if plan.is_none() {
            warnings.push("no database credentials found in the container environment; falling back to files".into());
        }
        plan
    });
    let native_supported = info.is_some_and(|a| a.integration == Some("arr")) && settings.integrations.enabled;
    let strategy = prefs.backup_strategy(&settings);
    let native = native_supported && prefs.backup.use_native.unwrap_or(false);

    let method = if db.is_some() {
        Method::Dump
    } else if native {
        Method::Native
    } else {
        match strategy {
            BackupStrategy::Pause => Method::Pause,
            BackupStrategy::Stop => Method::Stop,
            BackupStrategy::Live => Method::Live,
            BackupStrategy::Auto => {
                if info.is_some_and(|a| a.sqlite) {
                    Method::Pause
                } else {
                    Method::Live
                }
            }
        }
    };
    // Falling back from a failed dump plan: copy files with a stop for safety.
    let method = if db_engine.is_some() && db.is_none() {
        for m in mounts.iter_mut() {
            if m.reason.starts_with("database files") {
                m.include = true;
                m.reason = "database files (container is stopped while copied)".into();
            }
        }
        Method::Stop
    } else {
        method
    };
    if mounts.iter().any(|m| m.include && m.access == Access::DockerCopy && m.kind == "bind") {
        warnings.push("some folders are not mounted into Nodestarr; they are copied through Docker (slower, no exclude filtering until after transfer)".into());
    }

    let compose_files = if settings.backups.include_compose && s.stack.is_some() {
        let mut files = s.compose_files.clone();
        if let Some(dir) = &s.compose_dir {
            let env = Path::new(dir).join(".env");
            if env.is_file() {
                files.push(env.to_string_lossy().into_owned());
            }
        }
        files.into_iter().filter(|f| Path::new(f).is_file()).collect()
    } else {
        Vec::new()
    };

    BackupPlan {
        enabled: prefs.backup_enabled(&settings),
        method,
        mounts,
        excludes,
        db,
        native,
        compose_files,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_paths() {
        assert!(is_system_path("/var/run/docker.sock"));
        assert!(is_system_path("/etc/localtime"));
        assert!(is_system_path("/dev/dri"));
        assert!(!is_system_path("/config"));
        assert!(!is_system_path("/opt/stacks/media/config"));
        assert!(!is_system_path("/runner/data"));
        assert!(is_system_path("/run/secrets"));
    }
}
