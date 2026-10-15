//! Update detection and application.
//!
//! * [`check`] compares each container's image with its registry: a changed
//!   digest means a new build of the same tag (`latest`, `develop`), and for
//!   version-pinned tags newer version tags are looked up too.
//! * [`apply`] performs one-click updates and tag/branch switches with a
//!   pre-update backup, health verification and automatic rollback.
//! * [`auto`] applies updates on a schedule according to each container's policy.

pub mod apply;
pub mod auto;
pub mod check;
pub mod releases;

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::Result;
use crate::events::{Event, EventBus};
use crate::settings::TagScope;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    /// Not checked yet.
    #[default]
    Unknown,
    UpToDate,
    /// A new build of the tag or a newer version tag exists.
    Available,
    /// A newer image was already pulled; recreating the container applies it.
    PendingRecreate,
    /// The last check failed (see `error`).
    Error,
    /// Update checks are disabled for this container.
    Ignored,
    /// Built locally or not from a registry, nothing to compare.
    Local,
    /// Pinned by digest; never changes.
    Pinned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NewerTag {
    pub tag: String,
    pub scope: TagScope,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UpdateInfo {
    pub status: UpdateStatus,
    /// Image reference that was checked.
    pub image: String,
    pub checked_at: Option<i64>,
    pub current_digest: Option<String>,
    pub latest_digest: Option<String>,
    /// Version of the newest build of the current tag.
    pub latest_version: Option<String>,
    pub latest_created: Option<i64>,
    /// When `latest_digest` was first seen (for the minimum-age rule).
    pub first_seen: Option<i64>,
    /// A newer version tag, for version-pinned images.
    pub newer_tag: Option<NewerTag>,
    pub error: Option<String>,
    /// Source repository (for release notes).
    pub source: Option<String>,
}

impl UpdateInfo {
    /// True when a new build of the same tag is available.
    pub fn digest_changed(&self) -> bool {
        matches!((&self.current_digest, &self.latest_digest), (Some(a), Some(b)) if a != b)
    }

    pub fn has_update(&self) -> bool {
        matches!(self.status, UpdateStatus::Available | UpdateStatus::PendingRecreate)
    }
}

/// One row of update history.
#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub identity: String,
    pub target_name: Option<String>,
    pub ts: i64,
    pub from_ref: Option<String>,
    pub to_ref: Option<String>,
    pub from_version: Option<String>,
    pub to_version: Option<String>,
    pub rollback_tag: Option<String>,
    pub status: String,
    pub trigger: Option<String>,
    pub task_id: Option<String>,
    pub message: Option<String>,
    /// Whether this entry can still be rolled back to.
    pub can_rollback: bool,
}

#[derive(Clone)]
pub struct UpdateService {
    inner: Arc<Inner>,
}

struct Inner {
    db: Db,
    bus: EventBus,
    state: RwLock<HashMap<String, Arc<UpdateInfo>>>,
    checking: std::sync::atomic::AtomicBool,
    last_check: std::sync::atomic::AtomicI64,
}

impl UpdateService {
    pub async fn load(db: Db, bus: EventBus) -> Result<Self> {
        let rows = db
            .call(|c| {
                let mut stmt = c.prepare("SELECT identity, value FROM update_state")?;
                let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
                Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
            })
            .await?;
        let state = rows
            .into_iter()
            .filter_map(|(k, v)| serde_json::from_str::<UpdateInfo>(&v).ok().map(|i| (k, Arc::new(i))))
            .collect();
        Ok(Self {
            inner: Arc::new(Inner {
                db,
                bus,
                state: RwLock::new(state),
                checking: std::sync::atomic::AtomicBool::new(false),
                last_check: std::sync::atomic::AtomicI64::new(0),
            }),
        })
    }

    pub fn get(&self, identity: &str) -> Option<Arc<UpdateInfo>> {
        self.inner.state.read().get(identity).cloned()
    }

    pub fn all(&self) -> HashMap<String, Arc<UpdateInfo>> {
        self.inner.state.read().clone()
    }

    pub fn is_checking(&self) -> bool {
        self.inner.checking.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn last_check(&self) -> Option<i64> {
        let v = self.inner.last_check.load(std::sync::atomic::Ordering::Relaxed);
        (v > 0).then_some(v)
    }

    /// Stores and publishes new state for a container.
    pub async fn set(&self, identity: &str, info: UpdateInfo) {
        let changed = self.get(identity).is_none_or(|old| *old != info);
        let arc = Arc::new(info);
        self.inner.state.write().insert(identity.to_string(), arc.clone());
        if !changed {
            return;
        }
        self.inner.bus.publish(Event::Update { identity: identity.to_string(), info: arc.clone() });
        let json = match serde_json::to_string(&*arc) {
            Ok(j) => j,
            Err(_) => return,
        };
        let id = identity.to_string();
        let res = self
            .inner
            .db
            .call(move |c| {
                c.execute(
                    "INSERT INTO update_state (identity, value, updated_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT(identity) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                    params![id, json, crate::util::now()],
                )?;
                Ok(())
            })
            .await;
        if let Err(err) = res {
            tracing::warn!(%err, "failed to persist update state");
        }
    }

    /// Drops state for identities that no longer exist.
    pub async fn forget_missing(&self, live: &std::collections::HashSet<String>) {
        let gone: Vec<String> = self.inner.state.read().keys().filter(|k| !live.contains(*k)).cloned().collect();
        if gone.is_empty() {
            return;
        }
        self.inner.state.write().retain(|k, _| live.contains(k));
        let res = self
            .inner
            .db
            .call(move |c| {
                for k in gone {
                    c.execute("DELETE FROM update_state WHERE identity = ?1", [k])?;
                }
                Ok(())
            })
            .await;
        if let Err(err) = res {
            tracing::warn!(%err, "failed to prune update state");
        }
    }

    pub async fn history(&self, identity: Option<String>, limit: u32) -> Result<Vec<HistoryEntry>> {
        self.inner
            .db
            .call(move |c| {
                let mut sql = String::from(
                    "SELECT id, identity, target_name, ts, from_ref, to_ref, from_version, to_version, rollback_tag, status, trigger, task_id, message
                     FROM update_history",
                );
                if identity.is_some() {
                    sql.push_str(" WHERE identity = ?1");
                }
                sql.push_str(" ORDER BY ts DESC, id DESC LIMIT ");
                sql.push_str(&limit.clamp(1, 500).to_string());
                let mut stmt = c.prepare(&sql)?;
                let map = |r: &rusqlite::Row<'_>| {
                    let rollback_tag: Option<String> = r.get(8)?;
                    let status: String = r.get(9)?;
                    Ok(HistoryEntry {
                        id: r.get(0)?,
                        identity: r.get(1)?,
                        target_name: r.get(2)?,
                        ts: r.get(3)?,
                        from_ref: r.get(4)?,
                        to_ref: r.get(5)?,
                        from_version: r.get(6)?,
                        to_version: r.get(7)?,
                        can_rollback: rollback_tag.is_some() && status == "applied",
                        rollback_tag,
                        status,
                        trigger: r.get(10)?,
                        task_id: r.get(11)?,
                        message: r.get(12)?,
                    })
                };
                let rows = match &identity {
                    Some(id) => stmt.query_map([id], map)?.collect::<std::result::Result<Vec<_>, _>>()?,
                    None => stmt.query_map([], map)?.collect::<std::result::Result<Vec<_>, _>>()?,
                };
                Ok(rows)
            })
            .await
    }

    pub(crate) fn db(&self) -> &Db {
        &self.inner.db
    }

    pub(crate) fn set_checking(&self, v: bool) {
        self.inner.checking.store(v, std::sync::atomic::Ordering::Relaxed);
        if !v {
            self.inner.last_check.store(crate::util::now(), std::sync::atomic::Ordering::Relaxed);
        }
        self.inner.bus.publish(crate::events::Event::UpdateCheck { running: v, last_check: self.last_check() });
    }
}
