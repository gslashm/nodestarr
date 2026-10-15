//! App integrations: talking to the apps themselves (Sonarr, Plex, SABnzbd,
//! ...) for health warnings, real versions, active streams and native backups.
//!
//! API keys are discovered automatically from the apps' config files
//! (config.xml, sabnzbd.ini, settings.json, Preferences.xml), read either
//! directly (when the config folder is mounted into Nodestarr) or through
//! `docker exec cat` as a fallback. They can be overridden per container.

mod clients;
pub mod discover;

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{StreamExt, stream};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;

use crate::app::App;
use crate::docker::Entry;
use crate::error::{AppError, Result};
use crate::events::{Event, EventBus};
use crate::notify::Notification;
use crate::settings::Severity;

pub use discover::Creds;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HealthIssue {
    /// "notice", "warning" or "error".
    pub level: String,
    pub message: String,
    pub source: Option<String>,
    pub wiki: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CredState {
    /// Read from the app's config file.
    Discovered,
    /// Set by you in the container's settings.
    Configured,
    /// Needed but not found.
    Missing,
    /// The app's API doesn't need credentials.
    NotNeeded,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AppStatus {
    pub kind: String,
    pub reachable: bool,
    pub version: Option<String>,
    pub branch: Option<String>,
    pub health: Vec<HealthIssue>,
    /// Small facts shown in the UI ("Queue": "3", "Streams": "1").
    pub details: BTreeMap<String, String>,
    pub update_available: Option<bool>,
    pub credentials: CredState,
    pub base_url: Option<String>,
    pub checked_at: i64,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct IntegrationService {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    bus: EventBus,
    statuses: RwLock<HashMap<String, Arc<AppStatus>>>,
    creds: Mutex<HashMap<String, (Creds, i64)>>,
    streams: Mutex<Option<(i64, Option<u32>)>>,
}

impl IntegrationService {
    pub fn new(http: reqwest::Client, bus: EventBus) -> Self {
        // App APIs on the LAN often use self-signed certificates.
        let http = reqwest::Client::builder()
            .user_agent(crate::USER_AGENT)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .tls_danger_accept_invalid_certs(true)
            .pool_max_idle_per_host(1)
            // Apps live on the LAN; never send their API keys through a proxy.
            .no_proxy()
            .build()
            .unwrap_or(http);
        Self {
            inner: Arc::new(Inner {
                http,
                bus,
                statuses: RwLock::new(HashMap::new()),
                creds: Mutex::new(HashMap::new()),
                streams: Mutex::new(None),
            }),
        }
    }

    pub fn status(&self, identity: &str) -> Option<Arc<AppStatus>> {
        self.inner.statuses.read().get(identity).cloned()
    }

    pub fn all(&self) -> HashMap<String, Arc<AppStatus>> {
        self.inner.statuses.read().clone()
    }

    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.inner.http
    }

    fn store(&self, identity: &str, status: AppStatus) -> Option<Arc<AppStatus>> {
        let previous = self.inner.statuses.read().get(identity).cloned();
        let arc = Arc::new(status);
        let changed = previous.as_ref().is_none_or(|p| {
            let mut a = (**p).clone();
            let mut b = (*arc).clone();
            a.checked_at = 0;
            b.checked_at = 0;
            a != b
        });
        self.inner.statuses.write().insert(identity.to_string(), arc.clone());
        if changed {
            self.inner.bus.publish(Event::App { identity: identity.to_string(), status: arc });
        }
        previous
    }

    pub fn forget_creds(&self, identity: &str) {
        self.inner.creds.lock().remove(identity);
    }
}

/// The integration kind for a container, if any.
pub fn kind_of(entry: &Entry) -> Option<&'static str> {
    entry.summary.app.as_deref().and_then(crate::docker::apps::by_slug).and_then(|a| a.integration)
}

/// Credentials for a container: preferences override discovery; discovery
/// results are cached for 10 minutes.
pub async fn creds_for(app: &App, entry: &Entry) -> Result<Creds> {
    let identity = &entry.summary.identity;
    let prefs = app.prefs.get(identity);
    if let Some((c, at)) = app.integrations.inner.creds.lock().get(identity)
        && crate::util::now() - at < 600
        && prefs.integration.base_url.is_empty()
        && prefs.integration.api_key.is_empty()
    {
        return Ok(c.clone());
    }
    let mut creds = discover::discover(app, entry).await;
    let ip = &prefs.integration;
    if !ip.base_url.is_empty() {
        creds.base_url = ip.base_url.trim_end_matches('/').to_string();
    }
    if !ip.api_key.is_empty() {
        creds.api_key = Some(ip.api_key.clone());
        creds.state = CredState::Configured;
    }
    if !ip.username.is_empty() {
        creds.username = Some(ip.username.clone());
        creds.password = Some(ip.password.clone());
        creds.state = CredState::Configured;
    }
    if creds.base_url.is_empty() {
        return Err(AppError::conflict(
            "Nodestarr can't work out how to reach this app; set its URL in the container's settings",
        ));
    }
    app.integrations.inner.creds.lock().insert(identity.clone(), (creds.clone(), crate::util::now()));
    Ok(creds)
}

/// Polls one container's app API.
pub async fn poll(app: &App, entry: &Entry) -> Option<Arc<AppStatus>> {
    let kind = kind_of(entry)?;
    let identity = entry.summary.identity.clone();
    let settings = app.settings.get();
    let prefs = app.prefs.get(&identity);
    if !settings.integrations.enabled || prefs.integration.enabled == Some(false) || !entry.summary.is_running() {
        return None;
    }
    let mut status = AppStatus {
        kind: kind.to_string(),
        reachable: false,
        version: None,
        branch: None,
        health: Vec::new(),
        details: BTreeMap::new(),
        update_available: None,
        credentials: CredState::Missing,
        base_url: None,
        checked_at: crate::util::now(),
        error: None,
    };
    match creds_for(app, entry).await {
        Ok(creds) => {
            status.credentials = creds.state;
            status.base_url = Some(creds.base_url.clone());
            let http = app.integrations.http();
            if let Err(err) = clients::status(http, kind, entry, &creds, &mut status).await {
                status.error = Some(err.to_string());
                // A wrong cached key: rediscover next time.
                app.integrations.forget_creds(&identity);
            }
        }
        Err(err) => status.error = Some(err.to_string()),
    }
    let name = crate::updates::check::display_name(app, entry);
    let new_issues: Vec<HealthIssue> = {
        let previous = app.integrations.status(&identity);
        status
            .health
            .iter()
            .filter(|h| h.level != "notice")
            .filter(|h| previous.as_ref().is_none_or(|p| !p.health.contains(h)))
            .cloned()
            .collect()
    };
    app.integrations.store(&identity, status);
    if !new_issues.is_empty() {
        let worst = if new_issues.iter().any(|h| h.level == "error") { Severity::Error } else { Severity::Warning };
        let body = new_issues.iter().map(|h| format!("• {}", h.message)).collect::<Vec<_>>().join("\n");
        app.notifier.send(
            Notification::new("app.health", worst, format!("{name} reports a problem"), body).target(&identity, &name),
        );
    }
    app.integrations.status(&identity)
}

/// Active media streams across Plex (or Tautulli). Cached for 30 seconds.
pub async fn active_streams(app: &App) -> Option<u32> {
    if let Some((at, n)) = *app.integrations.inner.streams.lock()
        && crate::util::now() - at < 30
    {
        return n;
    }
    let mut result = None;
    for entry in app.docker.entries() {
        let Some(kind) = kind_of(&entry) else { continue };
        if !entry.summary.is_running() || !matches!(kind, "plex" | "tautulli") {
            continue;
        }
        if let Ok(creds) = creds_for(app, &entry).await
            && let Ok(n) = clients::streams(app.integrations.http(), kind, &creds).await
        {
            result = Some(result.unwrap_or(0).max(n));
            if kind == "plex" {
                break;
            }
        }
    }
    *app.integrations.inner.streams.lock() = Some((crate::util::now(), result));
    result
}

/// Triggers the app's own backup and downloads it into `dir`.
pub async fn native_backup(app: &App, entry: &Entry, dir: &Path) -> Result<std::path::PathBuf> {
    let kind = kind_of(entry).ok_or_else(|| AppError::conflict("this app has no API integration"))?;
    if kind != "arr" {
        return Err(AppError::conflict("this app has no built-in backup Nodestarr can trigger"));
    }
    let creds = creds_for(app, entry).await?;
    clients::arr_backup(app.integrations.http(), entry, &creds, dir).await
}

/// Background polling loop.
pub fn start(app: App) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(15)).await;
        loop {
            let settings = app.settings.get();
            if settings.integrations.enabled {
                let entries: Vec<Arc<Entry>> = app
                    .docker
                    .entries()
                    .into_iter()
                    .filter(|e| kind_of(e).is_some() && e.summary.is_running())
                    .collect();
                stream::iter(entries)
                    .map(|e| {
                        let app = app.clone();
                        async move {
                            poll(&app, &e).await;
                        }
                    })
                    .buffer_unordered(3)
                    .collect::<Vec<_>>()
                    .await;
            }
            let secs = settings.integrations.poll_secs.clamp(30, 86_400) as u64;
            tokio::time::sleep(Duration::from_secs(secs)).await;
        }
    });
}
