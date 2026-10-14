//! Activity log: the "History" of everything that happened (updates,
//! backups, container crashes, logins, settings changes).

use std::sync::Arc;

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::Result;
use crate::events::{Event, EventBus};
use crate::settings::Severity;

/// Who triggered something. Rendered as "user:jake", "api:homeassistant",
/// "auto-update", "schedule", "system".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Actor(pub String);

impl Actor {
    pub fn user(name: &str) -> Self {
        Self(format!("user:{name}"))
    }
    pub fn api(key_name: &str) -> Self {
        Self(format!("api:{key_name}"))
    }
    pub fn system() -> Self {
        Self("system".into())
    }
    pub fn auto_update() -> Self {
        Self("auto-update".into())
    }
    pub fn schedule() -> Self {
        Self("schedule".into())
    }
    pub fn mqtt() -> Self {
        Self("homeassistant".into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn is_user(&self) -> bool {
        self.0.starts_with("user:") || self.0.starts_with("api:")
    }
}

impl std::fmt::Display for Actor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ActivityEntry {
    pub id: i64,
    /// Unix seconds.
    pub ts: i64,
    pub kind: String,
    pub severity: Severity,
    pub target: Option<String>,
    pub target_name: Option<String>,
    pub message: String,
    pub actor: Option<String>,
    pub details: Option<serde_json::Value>,
}

/// Input for [`ActivityLog::record`].
#[derive(Debug, Clone)]
pub struct NewActivity {
    pub kind: &'static str,
    pub severity: Severity,
    pub target: Option<String>,
    pub target_name: Option<String>,
    pub message: String,
    pub actor: Option<Actor>,
    pub details: Option<serde_json::Value>,
}

impl NewActivity {
    pub fn new(kind: &'static str, severity: Severity, message: impl Into<String>) -> Self {
        Self {
            kind,
            severity,
            target: None,
            target_name: None,
            message: message.into(),
            actor: None,
            details: None,
        }
    }

    pub fn target(mut self, identity: impl Into<String>, name: impl Into<String>) -> Self {
        self.target = Some(identity.into());
        self.target_name = Some(name.into());
        self
    }

    pub fn actor(mut self, actor: &Actor) -> Self {
        self.actor = Some(actor.clone());
        self
    }

    pub fn details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct ActivityQuery {
    pub kind: Option<String>,
    pub target: Option<String>,
    pub severity: Option<Severity>,
    /// Return entries with id lower than this (pagination).
    pub before: Option<i64>,
    pub limit: Option<u32>,
    pub search: Option<String>,
}

#[derive(Clone)]
pub struct ActivityLog {
    db: Db,
    bus: EventBus,
}

impl ActivityLog {
    pub fn new(db: Db, bus: EventBus) -> Self {
        Self { db, bus }
    }

    pub async fn record(&self, entry: NewActivity) -> Result<Arc<ActivityEntry>> {
        let ts = crate::util::now();
        let details = entry.details.as_ref().map(|d| d.to_string());
        let (kind, severity) = (entry.kind, entry.severity);
        let (target, target_name, message) =
            (entry.target.clone(), entry.target_name.clone(), entry.message.clone());
        let actor = entry.actor.as_ref().map(|a| a.0.clone());
        let actor_db = actor.clone();
        let id = self
            .db
            .call(move |c| {
                c.execute(
                    "INSERT INTO activity (ts, kind, severity, target, target_name, message, actor, details)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![ts, kind, severity.as_str(), target, target_name, message, actor_db, details],
                )?;
                Ok(c.last_insert_rowid())
            })
            .await?;
        let saved = Arc::new(ActivityEntry {
            id,
            ts,
            kind: entry.kind.to_string(),
            severity: entry.severity,
            target: entry.target,
            target_name: entry.target_name,
            message: entry.message,
            actor,
            details: entry.details,
        });
        self.bus.publish(Event::Activity(saved.clone()));
        Ok(saved)
    }

    /// Records without failing the caller; logging is best effort.
    pub async fn note(&self, entry: NewActivity) {
        if let Err(err) = self.record(entry).await {
            tracing::warn!(%err, "failed to record activity");
        }
    }

    pub async fn list(&self, q: ActivityQuery) -> Result<Vec<ActivityEntry>> {
        self.db
            .call(move |c| {
                let mut sql = String::from(
                    "SELECT id, ts, kind, severity, target, target_name, message, actor, details FROM activity WHERE 1=1",
                );
                let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
                if let Some(kind) = q.kind.filter(|k| !k.is_empty()) {
                    // "update" matches update.*; "update.failed" matches exactly.
                    if kind.contains('.') {
                        sql.push_str(" AND kind = ?");
                        args.push(Box::new(kind));
                    } else {
                        sql.push_str(" AND kind LIKE ?");
                        args.push(Box::new(format!("{kind}.%")));
                    }
                }
                if let Some(target) = q.target.filter(|t| !t.is_empty()) {
                    sql.push_str(" AND target = ?");
                    args.push(Box::new(target));
                }
                if let Some(sev) = q.severity {
                    sql.push_str(" AND severity = ?");
                    args.push(Box::new(sev.as_str()));
                }
                if let Some(before) = q.before {
                    sql.push_str(" AND id < ?");
                    args.push(Box::new(before));
                }
                if let Some(search) = q.search.filter(|s| !s.trim().is_empty()) {
                    sql.push_str(" AND (message LIKE ? OR target_name LIKE ?)");
                    let pat = format!("%{}%", search.trim());
                    args.push(Box::new(pat.clone()));
                    args.push(Box::new(pat));
                }
                sql.push_str(" ORDER BY id DESC LIMIT ?");
                args.push(Box::new(q.limit.unwrap_or(100).clamp(1, 1000)));
                let mut stmt = c.prepare(&sql)?;
                let rows = stmt.query_map(rusqlite::params_from_iter(args.iter().map(|a| a.as_ref())), |r| {
                    let severity: String = r.get(3)?;
                    let details: Option<String> = r.get(8)?;
                    Ok(ActivityEntry {
                        id: r.get(0)?,
                        ts: r.get(1)?,
                        kind: r.get(2)?,
                        severity: serde_json::from_value(serde_json::Value::String(severity))
                            .unwrap_or_default(),
                        target: r.get(4)?,
                        target_name: r.get(5)?,
                        message: r.get(6)?,
                        actor: r.get(7)?,
                        details: details.and_then(|d| serde_json::from_str(&d).ok()),
                    })
                })?;
                Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn record_and_filter() {
        let db = Db::open_in_memory().unwrap();
        let log = ActivityLog::new(db, EventBus::new());
        log.record(
            NewActivity::new("update.applied", Severity::Success, "Sonarr updated")
                .target("compose:media/sonarr", "sonarr")
                .actor(&Actor::user("jake")),
        )
        .await
        .unwrap();
        log.record(NewActivity::new("backup.failed", Severity::Error, "Backup failed"))
            .await
            .unwrap();

        let all = log.list(ActivityQuery::default()).await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].kind, "backup.failed");

        let updates = log
            .list(ActivityQuery {
                kind: Some("update".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].actor.as_deref(), Some("user:jake"));

        let errors = log
            .list(ActivityQuery {
                severity: Some(Severity::Error),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(errors.len(), 1);

        let searched = log
            .list(ActivityQuery {
                search: Some("sonarr".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(searched.len(), 1);
    }
}
