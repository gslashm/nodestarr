//! Per-container preferences (update policy, backup choices, uptime check,
//! integration credentials, display name, notes).
//!
//! Keyed by the container's stable identity so they survive updates and
//! recreation. The whole table is small, so it is cached in memory.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use parking_lot::RwLock;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::Result;
use crate::settings::{AutoInstall, BackupStrategy, Settings, TagScope, UpdatePolicy};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UpdatePrefs {
    /// None inherits the global default.
    pub policy: Option<UpdatePolicy>,
    /// Which updates install automatically; None inherits the global default.
    pub auto_install: Option<AutoInstall>,
    /// Before update types (see [`AutoInstall::from_scope`]). Read from older
    /// preferences by [`ContainerPrefs::upgrade`], never written.
    #[serde(skip_serializing)]
    pub auto_scope: Option<TagScope>,
    /// "Skip this version": auto-update ignores this remote digest.
    pub skipped_digest: String,
    /// Paused after a rollback until cleared or a newer version appears.
    pub held: bool,
    pub min_age_hours: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MountChoice {
    /// Container path of the mount.
    pub destination: String,
    pub include: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BackupPrefs {
    /// None follows "back up everything by default".
    pub enabled: Option<bool>,
    pub strategy: Option<BackupStrategy>,
    /// Explicit include/exclude decisions per mount (overrides auto-detection).
    pub mounts: Vec<MountChoice>,
    /// Extra exclude globs, relative to each backed-up folder.
    pub exclude: Vec<String>,
    /// Use the app's built-in backup when an integration supports it.
    pub use_native: Option<bool>,
    pub retention_count: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UptimePrefs {
    pub enabled: Option<bool>,
    /// Override URL (defaults to the detected web UI).
    pub url: String,
    pub expect_status: String,
    pub keyword: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct IntegrationPrefs {
    pub enabled: Option<bool>,
    /// Override the URL Nodestarr uses to reach the app's API.
    pub base_url: String,
    pub api_key: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ContainerPrefs {
    pub display_name: String,
    pub icon: String,
    pub notes: String,
    pub tags: Vec<String>,
    pub hidden: bool,
    pub web_url: String,
    pub alerts_muted: bool,
    /// Dashboard "needs attention" items put away, by kind (crash, health,
    /// web, app, held, update). The value is the fingerprint of the occurrence
    /// that was dismissed, so the item comes back when the situation changes;
    /// "*" ignores that kind for this container for good.
    pub dismissed: BTreeMap<String, String>,
    pub update: UpdatePrefs,
    pub backup: BackupPrefs,
    pub uptime: UptimePrefs,
    pub integration: IntegrationPrefs,
}

impl ContainerPrefs {
    pub fn update_policy(&self, settings: &Settings) -> UpdatePolicy {
        self.update.policy.unwrap_or(settings.updates.default_policy)
    }

    pub fn auto_install(&self, settings: &Settings) -> AutoInstall {
        self.update.auto_install.unwrap_or(settings.updates.auto_install)
    }

    /// Carries choices stored by older versions over to the fields that
    /// replaced them.
    pub fn upgrade(&mut self) {
        if let Some(scope) = self.update.auto_scope.take() {
            self.update.auto_install = Some(AutoInstall::from_scope(scope));
        }
    }

    pub fn min_age_hours(&self, settings: &Settings) -> u32 {
        self.update.min_age_hours.unwrap_or(settings.updates.min_age_hours)
    }

    pub fn backup_enabled(&self, settings: &Settings) -> bool {
        self.backup.enabled.unwrap_or(settings.backups.include_all_by_default)
    }

    pub fn backup_strategy(&self, settings: &Settings) -> BackupStrategy {
        self.backup.strategy.unwrap_or(settings.backups.default_strategy)
    }
}

#[derive(Clone)]
pub struct PrefsStore {
    db: Db,
    cache: Arc<RwLock<HashMap<String, Arc<ContainerPrefs>>>>,
}

impl PrefsStore {
    pub async fn load(db: Db) -> Result<Self> {
        let rows = db
            .call(|c| {
                let mut stmt = c.prepare("SELECT identity, value FROM container_prefs")?;
                let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
                Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
            })
            .await?;
        let mut map = HashMap::new();
        for (identity, value) in rows {
            match serde_json::from_str::<ContainerPrefs>(&value) {
                Ok(mut p) => {
                    p.upgrade();
                    map.insert(identity, Arc::new(p));
                }
                Err(err) => tracing::warn!(%identity, %err, "ignoring unreadable container preferences"),
            }
        }
        Ok(Self { db, cache: Arc::new(RwLock::new(map)) })
    }

    /// Preferences for an identity (defaults when none were saved).
    pub fn get(&self, identity: &str) -> Arc<ContainerPrefs> {
        self.cache.read().get(identity).cloned().unwrap_or_else(|| Arc::new(ContainerPrefs::default()))
    }

    pub fn all(&self) -> HashMap<String, Arc<ContainerPrefs>> {
        self.cache.read().clone()
    }

    pub async fn set(&self, identity: &str, mut prefs: ContainerPrefs) -> Result<Arc<ContainerPrefs>> {
        prefs.upgrade();
        let identity_owned = identity.to_string();
        if prefs == ContainerPrefs::default() {
            self.db
                .call(move |c| {
                    c.execute("DELETE FROM container_prefs WHERE identity = ?1", [&identity_owned])?;
                    Ok(())
                })
                .await?;
            self.cache.write().remove(identity);
            return Ok(Arc::new(prefs));
        }
        let json = serde_json::to_string(&prefs)?;
        self.db
            .call(move |c| {
                c.execute(
                    "INSERT INTO container_prefs (identity, value, updated_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT(identity) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                    params![identity_owned, json, crate::util::now()],
                )?;
                Ok(())
            })
            .await?;
        let arc = Arc::new(prefs);
        self.cache.write().insert(identity.to_string(), arc.clone());
        Ok(arc)
    }

    /// Read-modify-write helper.
    pub async fn update<F>(&self, identity: &str, f: F) -> Result<Arc<ContainerPrefs>>
    where
        F: FnOnce(&mut ContainerPrefs),
    {
        let mut p = (*self.get(identity)).clone();
        f(&mut p);
        self.set(identity, p).await
    }

    /// Moves preferences when a container's identity changes (e.g. a
    /// standalone container adopted into a compose stack).
    pub async fn rename(&self, from: &str, to: &str) -> Result<()> {
        let prefs = (*self.get(from)).clone();
        if prefs == ContainerPrefs::default() {
            return Ok(());
        }
        self.set(to, prefs).await?;
        self.set(from, ContainerPrefs::default()).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roundtrip_and_defaults() {
        let db = Db::open_in_memory().unwrap();
        let store = PrefsStore::load(db.clone()).await.unwrap();
        assert_eq!(*store.get("x"), ContainerPrefs::default());
        store
            .update("compose:media/sonarr", |p| {
                p.update.policy = Some(UpdatePolicy::Auto);
                p.notes = "TV + anime".into();
                p.dismissed.insert("app".into(), "*".into());
            })
            .await
            .unwrap();
        let reloaded = PrefsStore::load(db).await.unwrap();
        let p = reloaded.get("compose:media/sonarr");
        assert_eq!(p.update.policy, Some(UpdatePolicy::Auto));
        assert_eq!(p.update_policy(&Settings::default()), UpdatePolicy::Auto);
        assert_eq!(p.notes, "TV + anime");
        assert_eq!(p.dismissed.get("app").map(String::as_str), Some("*"));
        // Resetting to defaults deletes the row.
        reloaded.set("compose:media/sonarr", ContainerPrefs::default()).await.unwrap();
        assert!(reloaded.all().is_empty());
    }

    #[tokio::test]
    async fn older_auto_update_scope_carries_over() {
        let db = Db::open_in_memory().unwrap();
        let store = PrefsStore::load(db.clone()).await.unwrap();
        let old: ContainerPrefs = serde_json::from_str(r#"{"update": {"auto_scope": "minor"}}"#).unwrap();
        store.set("standalone:pg", old).await.unwrap();
        let p = PrefsStore::load(db).await.unwrap().get("standalone:pg");
        let rules = p.update.auto_install.expect("carried over");
        assert!(rules.minor && rules.switch_tags && !rules.major);
        assert_eq!(p.update.auto_scope, None);
        assert_eq!(p.auto_install(&Settings::default()), rules);
    }
}
