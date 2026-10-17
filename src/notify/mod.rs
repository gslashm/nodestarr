//! Notifications: routing events to Discord, email, Telegram, ntfy, Gotify,
//! Pushover, Slack and generic webhooks.
//!
//! Callers fire and forget ([`Notifier::send`]); a background worker applies
//! each provider's event filter, de-duplicates bursts, holds non-urgent
//! messages during quiet hours (sent as one digest afterwards) and retries
//! failed deliveries.

mod providers;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::app::App;
use crate::db::Db;
use crate::error::Result;
use crate::settings::{NotificationProvider, QuietHours, Settings, SettingsStore, Severity};

/// Every event kind a provider can subscribe to, grouped by category.
pub const EVENT_KINDS: &[(&str, &str)] = &[
    ("update.available", "New updates found"),
    ("update.applied", "Update applied"),
    ("update.failed", "Update failed"),
    ("update.rolled_back", "Update rolled back"),
    ("update.auto_summary", "Automatic update summary"),
    ("backup.completed", "Backup completed"),
    ("backup.failed", "Backup failed"),
    ("restore.completed", "Restore completed"),
    ("restore.failed", "Restore failed"),
    ("container.crashed", "Container exited unexpectedly"),
    ("container.oom", "Container ran out of memory"),
    ("container.unhealthy", "Container unhealthy"),
    ("container.restart_loop", "Container restarting repeatedly"),
    ("uptime.down", "Web UI stopped answering"),
    ("uptime.up", "Web UI answering again"),
    ("alert.fired", "Resource alert"),
    ("alert.resolved", "Resource alert resolved"),
    ("app.health", "App reports a health problem"),
    ("proxmox.task", "Proxmox task finished (backups, snapshots)"),
    ("disk.test", "Disk self-test failed or didn't finish"),
    ("security.login_failed", "Repeated failed logins"),
    ("system.update", "New Nodestarr version"),
    ("test", "Test message"),
];

#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub kind: String,
    pub severity: Severity,
    pub title: String,
    pub message: String,
    pub fields: Vec<(String, String)>,
    pub target: Option<String>,
    pub target_name: Option<String>,
    pub url: Option<String>,
    pub ts: i64,
}

impl Notification {
    pub fn new(kind: &str, severity: Severity, title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.to_string(),
            severity,
            title: title.into(),
            message: message.into(),
            fields: Vec::new(),
            target: None,
            target_name: None,
            url: None,
            ts: crate::util::now(),
        }
    }

    pub fn target(mut self, identity: &str, name: &str) -> Self {
        self.target = Some(identity.to_string());
        self.target_name = Some(name.to_string());
        self
    }

    pub fn field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.push((key.into(), value.into()));
        self
    }

    /// Links to a Nodestarr page when an external URL is configured.
    pub fn link(mut self, app: &App, path: &str) -> Self {
        let settings = app.settings.get();
        let base = settings.general.external_url.trim_end_matches('/');
        if !base.is_empty() {
            self.url = Some(format!("{base}{}{path}", settings.general.url_base));
        }
        self
    }

    fn dedup_key(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.kind,
            self.target.as_deref().unwrap_or(""),
            self.title,
            crate::util::sha256_hex(&self.message)
        )
    }
}

/// Severity rank for filtering: success counts as informational.
fn rank(s: Severity) -> u8 {
    match s {
        Severity::Info | Severity::Success => 0,
        Severity::Warning => 1,
        Severity::Error => 2,
    }
}

/// Does `provider` want this notification?
pub fn wants(provider: &NotificationProvider, n: &Notification) -> bool {
    if !provider.enabled {
        return false;
    }
    if n.kind == "test" {
        return true;
    }
    if rank(n.severity) < rank(provider.min_severity) {
        return false;
    }
    if provider.events.is_empty() || n.kind == "digest" {
        return true;
    }
    provider.events.iter().any(|e| e == "*" || e == &n.kind || n.kind.split('.').next().is_some_and(|cat| cat == e))
}

/// Minutes since midnight for "HH:MM".
fn hhmm(s: &str) -> Option<u32> {
    let (h, m) = s.split_once(':')?;
    Some(h.parse::<u32>().ok()? * 60 + m.parse::<u32>().ok()?)
}

/// True while quiet hours are active.
pub fn in_quiet_hours(q: &QuietHours, minutes_now: u32) -> bool {
    if !q.enabled {
        return false;
    }
    let (Some(start), Some(end)) = (hhmm(&q.start), hhmm(&q.end)) else { return false };
    if start == end {
        return false;
    }
    if start < end { minutes_now >= start && minutes_now < end } else { minutes_now >= start || minutes_now < end }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DeliveryStatus {
    pub last_ok: Option<i64>,
    pub last_error: Option<String>,
    pub last_error_at: Option<i64>,
}

#[derive(Clone)]
pub struct Notifier {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    settings: SettingsStore,
    #[allow(dead_code)]
    db: Db,
    tx: mpsc::UnboundedSender<Notification>,
    rx: Mutex<Option<mpsc::UnboundedReceiver<Notification>>>,
    dedup: Mutex<HashMap<String, i64>>,
    status: Mutex<HashMap<String, DeliveryStatus>>,
}

impl Notifier {
    pub fn new(http: reqwest::Client, settings: SettingsStore, db: Db) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            inner: Arc::new(Inner {
                http,
                settings,
                db,
                tx,
                rx: Mutex::new(Some(rx)),
                dedup: Mutex::new(HashMap::new()),
                status: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Queues a notification for delivery.
    pub fn send(&self, n: Notification) {
        let _ = self.inner.tx.send(n);
    }

    pub fn status(&self) -> HashMap<String, DeliveryStatus> {
        self.inner.status.lock().clone()
    }

    /// Sends a test message to one provider and reports the result.
    pub async fn test(&self, app: &App, provider: &NotificationProvider) -> Result<()> {
        let n = Notification::new(
            "test",
            Severity::Info,
            "Nodestarr test notification",
            "If you can read this, notifications from Nodestarr work.",
        )
        .field("Instance", app.settings.get().general.instance_name.clone())
        .link(app, "/settings/notifications");
        let settings = self.inner.settings.get();
        providers::deliver(&self.inner.http, &settings, provider, &n).await
    }

    fn record(&self, provider_id: &str, result: &Result<()>) {
        let mut map = self.inner.status.lock();
        let st = map.entry(provider_id.to_string()).or_default();
        match result {
            Ok(()) => st.last_ok = Some(crate::util::now()),
            Err(e) => {
                st.last_error = Some(e.to_string());
                st.last_error_at = Some(crate::util::now());
            }
        }
    }

    fn is_duplicate(&self, n: &Notification, window_minutes: u32) -> bool {
        if window_minutes == 0 || n.kind == "test" {
            return false;
        }
        let now = crate::util::now();
        let key = n.dedup_key();
        let mut map = self.inner.dedup.lock();
        map.retain(|_, t| now - *t < window_minutes as i64 * 60);
        if map.contains_key(&key) {
            return true;
        }
        map.insert(key, now);
        false
    }

    async fn deliver_all(&self, settings: &Settings, n: &Notification) {
        let targets: Vec<NotificationProvider> =
            settings.notifications.providers.iter().filter(|p| wants(p, n)).cloned().collect();
        let futures = targets.into_iter().map(|p| {
            let this = self.clone();
            let n = n.clone();
            let settings = settings.clone();
            async move {
                let mut result = Ok(());
                for attempt in 0..3 {
                    result = providers::deliver(&this.inner.http, &settings, &p, &n).await;
                    if result.is_ok() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs(2u64.pow(attempt) * 5)).await;
                }
                if let Err(err) = &result {
                    tracing::warn!(provider = %p.name, %err, "notification delivery failed");
                }
                this.record(&p.id, &result);
            }
        });
        futures_util::future::join_all(futures).await;
    }
}

/// Starts the delivery worker.
pub fn start(app: App) {
    let Some(mut rx) = app.notifier.inner.rx.lock().take() else { return };
    let notifier = app.notifier.clone();
    tokio::spawn(async move {
        let mut held: Vec<Notification> = Vec::new();
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                maybe = rx.recv() => {
                    let Some(n) = maybe else { break };
                    let settings = app.settings.get();
                    if notifier.is_duplicate(&n, settings.notifications.dedup_minutes) {
                        continue;
                    }
                    let q = &settings.notifications.quiet_hours;
                    let urgent = n.severity == Severity::Error && q.allow_errors;
                    if !urgent && n.kind != "test" && in_quiet_hours(q, local_minutes(&settings)) {
                        held.push(n);
                        continue;
                    }
                    notifier.deliver_all(&settings, &n).await;
                }
                _ = tick.tick() => {
                    if held.is_empty() {
                        continue;
                    }
                    let settings = app.settings.get();
                    if in_quiet_hours(&settings.notifications.quiet_hours, local_minutes(&settings)) {
                        continue;
                    }
                    let batch = std::mem::take(&mut held);
                    // One digest per provider containing what that provider wants.
                    for p in settings.notifications.providers.iter().filter(|p| p.enabled) {
                        let mine: Vec<&Notification> = batch.iter().filter(|n| wants(p, n)).collect();
                        if mine.is_empty() {
                            continue;
                        }
                        let worst = mine.iter().map(|n| n.severity).max().unwrap_or(Severity::Info);
                        let body = mine.iter().map(|n| format!("• {}: {}", n.title, n.message.lines().next().unwrap_or(""))).collect::<Vec<_>>().join("\n");
                        let digest = Notification::new("digest", worst, format!("{} notifications during quiet hours", mine.len()), body);
                        let result = providers::deliver(&notifier.inner.http, &settings, p, &digest).await;
                        notifier.record(&p.id, &result);
                    }
                }
            }
        }
    });
}

fn local_minutes(settings: &Settings) -> u32 {
    use chrono::Timelike;
    let now = chrono::Utc::now().with_timezone(&settings.tz());
    now.hour() * 60 + now.minute()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ProviderConfig;

    fn provider(events: &[&str], min: Severity) -> NotificationProvider {
        NotificationProvider {
            id: "p".into(),
            name: "P".into(),
            enabled: true,
            events: events.iter().map(|s| s.to_string()).collect(),
            min_severity: min,
            config: ProviderConfig::Slack { webhook_url: "https://hooks.slack.com/x".into() },
        }
    }

    #[test]
    fn routing() {
        let n = Notification::new("update.failed", Severity::Error, "t", "m");
        assert!(wants(&provider(&[], Severity::Info), &n));
        assert!(wants(&provider(&["update"], Severity::Info), &n));
        assert!(wants(&provider(&["update.failed"], Severity::Info), &n));
        assert!(!wants(&provider(&["backup"], Severity::Info), &n));
        let info = Notification::new("update.available", Severity::Info, "t", "m");
        assert!(!wants(&provider(&[], Severity::Warning), &info));
        let ok = Notification::new("backup.completed", Severity::Success, "t", "m");
        assert!(wants(&provider(&[], Severity::Info), &ok));
        assert!(!wants(&provider(&[], Severity::Warning), &ok));
    }

    #[test]
    fn quiet_hours_wrap_midnight() {
        let q = QuietHours { enabled: true, start: "23:00".into(), end: "07:30".into(), allow_errors: true };
        assert!(in_quiet_hours(&q, 23 * 60 + 5));
        assert!(in_quiet_hours(&q, 3 * 60));
        assert!(!in_quiet_hours(&q, 7 * 60 + 30));
        assert!(!in_quiet_hours(&q, 12 * 60));
        let day = QuietHours { enabled: true, start: "12:00".into(), end: "13:00".into(), allow_errors: true };
        assert!(in_quiet_hours(&day, 12 * 60 + 30));
        assert!(!in_quiet_hours(&day, 13 * 60));
    }
}
