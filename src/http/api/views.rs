//! View models combining a container's summary with Nodestarr's own state
//! (updates, preferences, stats, uptime, app status, last backup).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde::Serialize;

use crate::app::App;
use crate::docker::model::ContainerSummary;
use crate::integrations::AppStatus;
use crate::monitor::stats::LiveStat;
use crate::settings::{AutoInstall, UpdatePolicy};
use crate::updates::UpdateInfo;

#[derive(Debug, Clone, Serialize)]
pub struct PrefsBrief {
    pub tags: Vec<String>,
    pub icon: String,
    pub web_url: String,
    pub hidden: bool,
    pub notes: bool,
    pub held: bool,
    pub skipped_digest: String,
    pub backup_enabled: bool,
    pub alerts_muted: bool,
    pub dismissed: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UptimeBrief {
    pub state: String,
    pub since: i64,
    pub latency_ms: Option<u64>,
    pub uptime_24h: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContainerView {
    #[serde(flatten)]
    pub summary: Arc<ContainerSummary>,
    pub display_name: String,
    pub policy: UpdatePolicy,
    /// Which updates install automatically (its own choice or the default).
    pub auto_install: AutoInstall,
    /// Hours an update must be out before it installs automatically.
    pub min_age_hours: u32,
    pub update: Option<Arc<UpdateInfo>>,
    pub prefs: PrefsBrief,
    pub stats: Option<LiveStat>,
    pub uptime: Option<UptimeBrief>,
    pub app_status: Option<Arc<AppStatus>>,
    pub last_backup: Option<i64>,
    pub busy: bool,
}

/// Shared lookups for building many views at once.
pub struct ViewContext {
    pub updates: HashMap<String, Arc<UpdateInfo>>,
    pub stats: HashMap<String, LiveStat>,
    pub uptime: HashMap<String, crate::monitor::uptime::UptimeStatus>,
    pub apps: HashMap<String, Arc<AppStatus>>,
    pub backups: HashMap<String, i64>,
}

impl ViewContext {
    pub async fn load(app: &App) -> Self {
        Self {
            updates: app.updates.all(),
            stats: crate::monitor::stats::latest(app),
            uptime: app.uptime.by_identity(),
            apps: app.integrations.all(),
            backups: app.backups.latest_by_identity().await.unwrap_or_default(),
        }
    }
}

pub fn container_view(app: &App, ctx: &ViewContext, summary: Arc<ContainerSummary>) -> ContainerView {
    let settings = app.settings.get();
    let prefs = app.prefs.get(&summary.identity);
    let display_name = if prefs.display_name.is_empty() { summary.default_name() } else { prefs.display_name.clone() };
    let identity = summary.identity.clone();
    ContainerView {
        display_name,
        policy: prefs.update_policy(&settings),
        auto_install: prefs.auto_install(&settings),
        min_age_hours: prefs.min_age_hours(&settings),
        update: ctx.updates.get(&identity).cloned(),
        prefs: PrefsBrief {
            tags: prefs.tags.clone(),
            icon: prefs.icon.clone(),
            web_url: prefs.web_url.clone(),
            hidden: prefs.hidden,
            notes: !prefs.notes.is_empty(),
            held: prefs.update.held,
            skipped_digest: prefs.update.skipped_digest.clone(),
            backup_enabled: prefs.backup_enabled(&settings),
            alerts_muted: prefs.alerts_muted,
            dismissed: prefs.dismissed.clone(),
        },
        stats: ctx.stats.get(&identity).cloned(),
        uptime: ctx.uptime.get(&identity).map(|u| UptimeBrief {
            state: u.state.clone(),
            since: u.since,
            latency_ms: u.latency_ms,
            uptime_24h: u.uptime_24h,
        }),
        app_status: ctx.apps.get(&identity).cloned(),
        last_backup: ctx.backups.get(&identity).copied(),
        busy: app.tasks.is_busy(&identity),
        summary,
    }
}

/// Glob match for hidden-container patterns ("*-old", "nodestarr-updater-*").
pub fn name_matches(patterns: &[String], name: &str) -> bool {
    patterns.iter().any(|p| globset::Glob::new(p).map(|g| g.compile_matcher().is_match(name)).unwrap_or(false))
}
