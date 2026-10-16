//! Runtime settings.
//!
//! Settings are one typed document stored as JSON in the database. Every field
//! has a default, so adding fields never needs a migration and older documents
//! keep loading. Readers grab a cheap `Arc<Settings>` snapshot; writers go
//! through [`SettingsStore::update`], which validates, persists and notifies
//! subscribers (schedulers re-plan, the MQTT client reconnects, and so on).

mod model;
mod secrets;

pub use model::*;
pub use secrets::{SECRET_PLACEHOLDER, mask_secrets, merge_secrets};

use std::sync::Arc;

use tokio::sync::watch;

use crate::db::Db;
use crate::error::{AppError, Result};

const SETTINGS_KEY: &str = "main";

#[derive(Clone)]
pub struct SettingsStore {
    db: Db,
    tx: Arc<watch::Sender<Arc<Settings>>>,
    /// Serializes read-modify-write cycles so concurrent saves don't clobber each other.
    write_lock: Arc<tokio::sync::Mutex<()>>,
}

impl SettingsStore {
    /// Loads settings from the database (or defaults on first run).
    pub async fn load(db: Db) -> Result<Self> {
        let stored: Option<serde_json::Value> = db.get_json("settings", SETTINGS_KEY.to_string()).await?;
        let settings = match stored {
            Some(value) => match serde_json::from_value::<Settings>(value) {
                Ok(mut s) => {
                    s.upgrade();
                    s
                }
                Err(err) => {
                    // Never brick the app over a settings document; fall back to
                    // defaults but keep the stored copy untouched for inspection.
                    tracing::error!(%err, "stored settings are invalid, using defaults until saved");
                    Settings::default()
                }
            },
            None => {
                let s = Settings::default();
                db.put_json("settings", SETTINGS_KEY.to_string(), &s).await?;
                s
            }
        };
        let (tx, _) = watch::channel(Arc::new(settings));
        Ok(Self { db, tx: Arc::new(tx), write_lock: Arc::new(tokio::sync::Mutex::new(())) })
    }

    /// Current settings snapshot.
    pub fn get(&self) -> Arc<Settings> {
        self.tx.borrow().clone()
    }

    /// Subscribe to changes.
    pub fn subscribe(&self) -> watch::Receiver<Arc<Settings>> {
        self.tx.subscribe()
    }

    /// Applies `f` to a copy of the settings, validates, persists and publishes.
    pub async fn update<F>(&self, f: F) -> Result<Arc<Settings>>
    where
        F: FnOnce(&mut Settings) -> Result<()>,
    {
        let _guard = self.write_lock.lock().await;
        let mut next = (*self.get()).clone();
        f(&mut next)?;
        // Documents from an older Nodestarr (an import, a stale browser tab).
        next.upgrade();
        next.validate().map_err(AppError::BadRequest)?;
        self.db.put_json("settings", SETTINGS_KEY.to_string(), &next).await?;
        let next = Arc::new(next);
        self.tx.send_replace(next.clone());
        Ok(next)
    }

    /// Replaces one top-level section from client JSON, keeping stored secrets
    /// wherever the client sent back the placeholder.
    pub async fn replace_section(&self, section: &str, incoming: serde_json::Value) -> Result<Arc<Settings>> {
        let section = section.to_string();
        self.update(move |s| {
            let mut doc = serde_json::to_value(&*s)?;
            let current = doc
                .get(&section)
                .cloned()
                .ok_or_else(|| AppError::not_found(format!("settings section '{section}'")))?;
            let mut incoming = incoming;
            merge_secrets(&mut incoming, &current);
            doc[&section] = incoming;
            *s = serde_json::from_value(doc)
                .map_err(|e| AppError::bad_request(format!("invalid {section} settings: {e}")))?;
            Ok(())
        })
        .await
    }

    /// Settings as JSON with secrets replaced by a placeholder, for the UI.
    pub fn masked(&self) -> serde_json::Value {
        let mut v = serde_json::to_value(&*self.get()).unwrap_or_default();
        mask_secrets(&mut v);
        v
    }
}
