//! SQLite storage.
//!
//! One connection guarded by a mutex is plenty for Nodestarr's write volume (a
//! handful of writes per minute). Calls run on tokio's blocking pool so the
//! async workers never wait on disk.

use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension};

use crate::error::{AppError, Result};

/// Migrations in order. Never edit an applied migration; add a new one.
const MIGRATIONS: &[&str] = &[include_str!("migrations/001_init.sql")];

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// Opens (creating if needed) the database at `path` and applies migrations.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        Self::configure(&conn)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate_blocking()?;
        // Keep the database private: it holds password hashes and API tokens.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(db)
    }

    /// An in-memory database, for tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure(&conn)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate_blocking()?;
        Ok(db)
    }

    fn configure(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;
             PRAGMA cache_size = -2000;
             PRAGMA temp_store = MEMORY;
             PRAGMA auto_vacuum = INCREMENTAL;",
        )?;
        Ok(())
    }

    fn migrate_blocking(&self) -> Result<()> {
        let mut conn = self.conn.lock();
        let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let current = current.max(0) as usize;
        if current > MIGRATIONS.len() {
            return Err(AppError::Other(anyhow::anyhow!(
                "database schema version {current} is newer than this Nodestarr build supports ({}); \
                 refusing to start to avoid damaging it",
                MIGRATIONS.len()
            )));
        }
        for (idx, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            let version = idx + 1;
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", version as i64)?;
            tx.commit()?;
            tracing::info!(version, "applied database migration");
        }
        Ok(())
    }

    /// Runs `f` with exclusive access to the connection on the blocking pool.
    pub async fn call<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = conn.lock();
            f(&mut guard)
        })
        .await?
    }

    /// Synchronous access, for startup code and CLI tools.
    pub fn call_blocking<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Connection) -> Result<T>,
    {
        let mut guard = self.conn.lock();
        f(&mut guard)
    }

    /// Reads a JSON document from a key/value table.
    pub async fn get_json<T>(&self, table: &'static str, key: String) -> Result<Option<T>>
    where
        T: serde::de::DeserializeOwned + Send + 'static,
    {
        self.call(move |c| {
            let sql = format!("SELECT value FROM {table} WHERE {} = ?1", kv_key_column(table));
            let raw: Option<String> = c.query_row(&sql, [&key], |r| r.get(0)).optional()?;
            match raw {
                Some(text) => Ok(Some(serde_json::from_str(&text).map_err(|e| {
                    AppError::Other(anyhow::anyhow!("corrupt {table} entry {key}: {e}"))
                })?)),
                None => Ok(None),
            }
        })
        .await
    }

    /// Writes a JSON document to a key/value table.
    pub async fn put_json<T>(&self, table: &'static str, key: String, value: &T) -> Result<()>
    where
        T: serde::Serialize,
    {
        let text = serde_json::to_string(value)?;
        self.call(move |c| {
            let sql = format!(
                "INSERT INTO {table} ({k}, value, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT({k}) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                k = kv_key_column(table)
            );
            c.execute(&sql, rusqlite::params![key, text, crate::util::now()])?;
            Ok(())
        })
        .await
    }

    /// Deletes rows older than the given cutoffs and reclaims free pages.
    pub async fn maintenance(&self, cutoffs: RetentionCutoffs) -> Result<()> {
        self.call(move |c| {
            let tx = c.transaction()?;
            tx.execute("DELETE FROM activity WHERE ts < ?1", [cutoffs.activity])?;
            tx.execute("DELETE FROM tasks WHERE created_at < ?1", [cutoffs.tasks])?;
            tx.execute("DELETE FROM sessions WHERE expires_at < ?1", [crate::util::now()])?;
            tx.execute(
                "DELETE FROM alerts WHERE resolved_at IS NOT NULL AND resolved_at < ?1",
                [cutoffs.alerts],
            )?;
            tx.execute(
                "DELETE FROM update_history WHERE ts < ?1",
                [cutoffs.update_history],
            )?;
            // Keep the 25 most recent versions of each edited file.
            tx.execute(
                "DELETE FROM file_versions WHERE id IN (
                   SELECT id FROM (
                     SELECT id, ROW_NUMBER() OVER (PARTITION BY path ORDER BY ts DESC) AS rn
                     FROM file_versions) WHERE rn > 25)",
                [],
            )?;
            tx.commit()?;
            c.execute_batch("PRAGMA incremental_vacuum(2000); PRAGMA optimize;")?;
            Ok(())
        })
        .await
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RetentionCutoffs {
    pub activity: i64,
    pub tasks: i64,
    pub alerts: i64,
    pub update_history: i64,
}

fn kv_key_column(table: &str) -> &'static str {
    match table {
        "settings" => "key",
        _ => "identity",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migrations_apply_and_kv_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        db.put_json("settings", "main".into(), &serde_json::json!({"a": 1}))
            .await
            .unwrap();
        let v: Option<serde_json::Value> = db.get_json("settings", "main".into()).await.unwrap();
        assert_eq!(v.unwrap()["a"], 1);
        db.put_json("container_prefs", "compose:x/y".into(), &serde_json::json!({"b": 2}))
            .await
            .unwrap();
        let v: Option<serde_json::Value> =
            db.get_json("container_prefs", "compose:x/y".into()).await.unwrap();
        assert_eq!(v.unwrap()["b"], 2);
        let version: i64 = db
            .call(|c| Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?))
            .await
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
    }
}
