//! The settings document. Every struct uses `#[serde(default)]` so missing
//! fields take their defaults.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::util::net::{CidrList, default_local_networks};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub general: General,
    pub security: Security,
    pub docker: DockerSettings,
    pub updates: Updates,
    pub backups: Backups,
    pub notifications: Notifications,
    pub integrations: Integrations,
    pub proxmox: Proxmox,
    pub mqtt: Mqtt,
    pub monitoring: Monitoring,
    pub uptime: Uptime,
}

// ---------------------------------------------------------------------------
// General
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    /// Shown in the page title and notifications.
    pub instance_name: String,
    /// Path prefix when served behind a reverse proxy, e.g. "/nodestarr".
    pub url_base: String,
    /// Host used for quick links to app web UIs (e.g. "192.168.1.50").
    /// Empty uses whatever address the browser opened Nodestarr with.
    pub public_host: String,
    /// Address of Nodestarr itself as reachable by people, used in notification
    /// links (e.g. "http://192.168.1.50:7979"). Optional.
    pub external_url: String,
    /// IANA time zone for schedules; empty uses the TZ environment variable.
    pub timezone: String,
    pub ui: UiSettings,
    /// Check GitHub for new Nodestarr releases.
    pub check_nodestarr_updates: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            instance_name: "Nodestarr".into(),
            url_base: String::new(),
            public_host: String::new(),
            external_url: String::new(),
            timezone: String::new(),
            ui: UiSettings::default(),
            check_nodestarr_updates: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    /// "dark", "light" or "system".
    pub theme: String,
    /// Accent color as hex.
    pub accent: String,
    /// "comfortable" or "compact" tables.
    pub density: String,
    /// Optional icon URL template with `{name}` replaced by the app slug,
    /// e.g. a self-hosted icon set. Empty shows monogram avatars.
    pub icon_url_template: String,
    /// Group the container list by compose stack.
    pub group_by_stack: bool,
    /// "24h" or "12h".
    pub time_format: String,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            accent: "#e46cff".into(),
            density: "comfortable".into(),
            icon_url_template: String::new(),
            group_by_stack: true,
            time_format: "24h".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Security
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    /// Nodestarr's own login page (default).
    #[default]
    Forms,
    /// Trust a username header set by an authenticating reverse proxy
    /// (Authelia, Authentik, ...). Only honored from trusted proxies.
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Security {
    pub auth_mode: AuthMode,
    /// Skip the login for requests from `local_networks`.
    pub bypass_local: bool,
    pub local_networks: Vec<String>,
    /// Reverse proxies whose X-Forwarded-For / auth headers are trusted.
    pub trusted_proxies: Vec<String>,
    /// Header carrying the username in external auth mode.
    pub external_header: String,
    /// Session length without "remember me".
    pub session_hours: u32,
    /// Session length with "remember me".
    pub remember_days: u32,
    /// Accept `?apikey=` in URLs (handy for scripts; headers are preferred).
    pub allow_query_api_key: bool,
    /// Serve /metrics without authentication.
    pub metrics_public: bool,
}

impl Default for Security {
    fn default() -> Self {
        Self {
            auth_mode: AuthMode::Forms,
            bypass_local: false,
            local_networks: default_local_networks(),
            trusted_proxies: Vec::new(),
            external_header: "Remote-User".into(),
            session_hours: 24,
            remember_days: 30,
            allow_query_api_key: true,
            metrics_public: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Docker
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DockerSettings {
    /// Folders scanned for compose stacks (one sub-folder per stack), so
    /// stacks show up even when they are not running.
    pub stacks_dirs: Vec<String>,
    /// Compose command; empty auto-detects `docker compose` / `docker-compose`.
    pub compose_command: String,
    /// Seconds to wait for a graceful stop before killing (0 = Docker default).
    pub stop_timeout_secs: u32,
    /// Glob patterns of container names to hide from the UI.
    pub hidden_containers: Vec<String>,
    /// Shells tried, in order, when opening a console.
    pub console_shells: Vec<String>,
    /// Full resync with Docker as a safety net next to the event stream.
    pub resync_secs: u32,
}

impl Default for DockerSettings {
    fn default() -> Self {
        Self {
            stacks_dirs: vec!["/opt/stacks".into()],
            compose_command: String::new(),
            stop_timeout_secs: 0,
            hidden_containers: Vec::new(),
            console_shells: vec!["/bin/bash".into(), "/bin/ash".into(), "/bin/sh".into()],
            resync_secs: 300,
        }
    }
}

// ---------------------------------------------------------------------------
// Updates
// ---------------------------------------------------------------------------

/// What Nodestarr does when an update is found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePolicy {
    /// Don't check this container.
    Ignore,
    /// Check and notify, never apply automatically.
    #[default]
    Notify,
    /// Apply automatically during the auto-update window.
    Auto,
}

/// How far a version-pinned tag may move: for reporting newer tags, and
/// before update types, for automatic updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TagScope {
    /// Only new builds of the current tag (e.g. a rebuilt `latest`).
    #[default]
    Digest,
    Patch,
    Minor,
    Major,
}

/// Which updates automatic updates install, by type. An update installs
/// when its version change is ticked (or it fixes a security issue and
/// `security` is on, unless it is a major version), its release notes don't
/// mention breaking changes (unless `breaking` is on), and it doesn't change
/// the tag (unless `switch_tags` is on).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoInstall {
    /// The same version in a new image.
    pub rebuild: bool,
    pub patch: bool,
    pub minor: bool,
    pub major: bool,
    /// Updates Nodestarr can't size (the image doesn't say its version).
    pub unknown: bool,
    /// Security fixes, even when their version change isn't ticked.
    pub security: bool,
    /// Updates whose release notes mention breaking changes.
    pub breaking: bool,
    /// Switch a version tag to a newer one (16.4 to 16.5). Off installs only
    /// new builds of the tag a container runs.
    pub switch_tags: bool,
}

impl Default for AutoInstall {
    fn default() -> Self {
        Self {
            rebuild: true,
            patch: true,
            minor: true,
            major: false,
            unknown: true,
            security: true,
            breaking: false,
            switch_tags: false,
        }
    }
}

impl AutoInstall {
    /// The choice from before update types: how far automatic updates could
    /// move a pinned tag. "New builds only" kept tags as they were; the others
    /// moved them up to that scope, and now also limit new builds of tags
    /// like `latest` to it.
    pub fn from_scope(scope: TagScope) -> Self {
        let base = Self::default();
        match scope {
            TagScope::Digest => base,
            TagScope::Patch => Self { minor: false, switch_tags: true, ..base },
            TagScope::Minor => Self { switch_tags: true, ..base },
            TagScope::Major => Self { major: true, switch_tags: true, ..base },
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RegistryAuth {
    /// Registry host, e.g. "docker.io", "ghcr.io".
    pub registry: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Updates {
    /// When to check registries for updates.
    pub check_cron: String,
    pub default_policy: UpdatePolicy,
    /// Which updates automatic updates install (containers can choose their own).
    pub auto_install: AutoInstall,
    /// Before update types: how far automatic updates could move a pinned
    /// tag. Read from older settings by [`Settings::upgrade`], never written.
    #[serde(skip_serializing)]
    pub default_auto_scope: Option<TagScope>,
    /// When automatic updates are applied.
    pub auto_cron: String,
    /// Only auto-apply updates that have been available this long. Manual
    /// updates ignore this.
    pub min_age_hours: u32,
    /// Report newer version tags up to this scope for pinned tags.
    pub notify_scope: TagScope,
    /// Skip alpha/beta/rc/nightly tags when looking for newer versions.
    pub ignore_prereleases: bool,
    /// Seconds a container must keep running after an update.
    pub settle_secs: u32,
    /// Max seconds to wait for healthy/responding after an update.
    pub verify_timeout_secs: u32,
    pub rollback_on_failure: bool,
    /// Take a backup before updating containers that have backups enabled.
    pub backup_before_update: bool,
    /// Previous images kept per container for rollback.
    pub keep_rollback_images: u32,
    /// Remove dangling images after a successful update.
    pub prune_after_update: bool,
    /// Defer automatic updates/restarts of Plex while someone is streaming.
    pub plex_guard: bool,
    /// Send a digest notification when new updates are found.
    pub notify_available: bool,
    pub registries: Vec<RegistryAuth>,
    /// Optional GitHub token for release notes (raises the rate limit).
    pub github_token: String,
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            check_cron: "17 */6 * * *".into(),
            default_policy: UpdatePolicy::Notify,
            auto_install: AutoInstall::default(),
            default_auto_scope: None,
            auto_cron: "0 4 * * *".into(),
            min_age_hours: 6,
            notify_scope: TagScope::Major,
            ignore_prereleases: true,
            settle_secs: 15,
            verify_timeout_secs: 180,
            rollback_on_failure: true,
            backup_before_update: true,
            keep_rollback_images: 2,
            prune_after_update: true,
            plex_guard: true,
            notify_available: true,
            registries: Vec::new(),
            github_token: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Backups
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Compression {
    #[default]
    Zstd,
    Gzip,
}

/// How to get a consistent copy of a running container's files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BackupStrategy {
    /// Pick per app: database dumps for database containers, a brief pause
    /// for apps with SQLite files, a live copy otherwise.
    #[default]
    Auto,
    /// Freeze the container while its files are copied (seconds).
    Pause,
    /// Stop the container during the backup.
    Stop,
    /// Copy while running.
    Live,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HookWhen {
    #[default]
    Success,
    Always,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BackupHook {
    /// Shell command run in Nodestarr's environment after a backup run.
    pub command: String,
    /// Or: run `exec_command` inside this container (e.g. an rclone container).
    pub exec_container: String,
    pub exec_command: String,
    pub when: HookWhen,
    pub timeout_secs: u32,
}

impl Default for BackupHook {
    fn default() -> Self {
        Self {
            command: String::new(),
            exec_container: String::new(),
            exec_command: String::new(),
            when: HookWhen::Success,
            timeout_secs: 3600,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Backups {
    /// Where archives are written (mount this from your pool).
    pub root: String,
    /// Run scheduled backups for containers with backups enabled.
    pub schedule_enabled: bool,
    pub cron: String,
    /// Back up every container unless it opts out (otherwise opt-in).
    pub include_all_by_default: bool,
    /// Archives kept per container (0 = unlimited).
    pub retention_count: u32,
    /// Delete archives older than this (0 = never).
    pub retention_days: u32,
    /// Pre-update backups are pruned after this many days.
    pub pre_update_retention_days: u32,
    pub compression: Compression,
    pub zstd_level: i32,
    pub default_strategy: BackupStrategy,
    /// Include the stack's compose file and .env in each backup.
    pub include_compose: bool,
    /// Bind mounts whose host path is under one of these are treated as
    /// media/data and excluded unless explicitly included.
    pub data_roots: Vec<String>,
    /// Global exclude globs applied inside every backed up folder.
    pub exclude: Vec<String>,
    pub hook: BackupHook,
}

impl Default for Backups {
    fn default() -> Self {
        Self {
            root: "/backups".into(),
            schedule_enabled: false,
            cron: "30 3 * * *".into(),
            include_all_by_default: true,
            retention_count: 7,
            retention_days: 0,
            pre_update_retention_days: 14,
            compression: Compression::Zstd,
            zstd_level: 3,
            default_strategy: BackupStrategy::Auto,
            include_compose: true,
            data_roots: vec!["/data".into(), "/mnt".into(), "/media".into(), "/srv".into(), "/downloads".into()],
            exclude: vec!["**/*.log".into(), "**/*.log.[0-9]*".into(), "**/logs/*.txt".into(), "**/.cache/**".into()],
            hook: BackupHook::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Notifications
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Success => "success",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderConfig {
    Discord {
        webhook_url: String,
        #[serde(default)]
        username: String,
        #[serde(default)]
        mention: String,
    },
    Email {
        host: String,
        #[serde(default = "default_smtp_port")]
        port: u16,
        /// "starttls", "tls" or "none".
        #[serde(default = "default_smtp_security")]
        security: String,
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: String,
        from: String,
        to: Vec<String>,
    },
    Telegram {
        bot_token: String,
        chat_id: String,
        #[serde(default)]
        topic_id: String,
    },
    Ntfy {
        #[serde(default = "default_ntfy_server")]
        server: String,
        topic: String,
        #[serde(default)]
        token: String,
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: String,
    },
    Gotify {
        server: String,
        app_token: String,
    },
    Pushover {
        user_key: String,
        app_token: String,
        #[serde(default)]
        device: String,
    },
    Slack {
        webhook_url: String,
    },
    Webhook {
        url: String,
        #[serde(default = "default_post")]
        method: String,
        #[serde(default)]
        headers: std::collections::BTreeMap<String, String>,
        /// Optional MiniJinja template for the body; empty sends Nodestarr's JSON.
        #[serde(default)]
        body_template: String,
    },
}

fn default_smtp_port() -> u16 {
    587
}
fn default_smtp_security() -> String {
    "starttls".into()
}
fn default_ntfy_server() -> String {
    "https://ntfy.sh".into()
}
fn default_post() -> String {
    "POST".into()
}

impl ProviderConfig {
    pub fn kind(&self) -> &'static str {
        match self {
            ProviderConfig::Discord { .. } => "discord",
            ProviderConfig::Email { .. } => "email",
            ProviderConfig::Telegram { .. } => "telegram",
            ProviderConfig::Ntfy { .. } => "ntfy",
            ProviderConfig::Gotify { .. } => "gotify",
            ProviderConfig::Pushover { .. } => "pushover",
            ProviderConfig::Slack { .. } => "slack",
            ProviderConfig::Webhook { .. } => "webhook",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationProvider {
    pub id: String,
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Event kinds or categories ("update", "update.failed", "*"). Empty = all.
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub min_severity: Severity,
    pub config: ProviderConfig,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct QuietHours {
    pub enabled: bool,
    /// "HH:MM" local time.
    pub start: String,
    pub end: String,
    /// Errors still go out during quiet hours.
    pub allow_errors: bool,
}

impl Default for QuietHours {
    fn default() -> Self {
        Self { enabled: false, start: "23:00".into(), end: "07:30".into(), allow_errors: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Notifications {
    pub providers: Vec<NotificationProvider>,
    pub quiet_hours: QuietHours,
    /// Identical notifications within this window are sent once.
    pub dedup_minutes: u32,
}

impl Default for Notifications {
    fn default() -> Self {
        Self { providers: Vec::new(), quiet_hours: QuietHours::default(), dedup_minutes: 30 }
    }
}

// ---------------------------------------------------------------------------
// Integrations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Integrations {
    pub enabled: bool,
    /// How often app APIs are polled for health and version.
    pub poll_secs: u32,
    /// Read API keys from app config files (config.xml, sabnzbd.ini, ...).
    pub auto_discover: bool,
    /// qBittorrent Web UI credentials (it has no API key).
    pub qbittorrent_username: String,
    pub qbittorrent_password: String,
}

impl Default for Integrations {
    fn default() -> Self {
        Self {
            enabled: true,
            poll_secs: 300,
            auto_discover: true,
            qbittorrent_username: String::new(),
            qbittorrent_password: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Proxmox
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TlsMode {
    /// Pin the certificate's SHA-256 fingerprint (works with self-signed certs).
    #[default]
    Fingerprint,
    /// Normal certificate validation.
    Verify,
    /// Accept any certificate (not recommended).
    Insecure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Proxmox {
    pub enabled: bool,
    /// e.g. https://192.168.1.10:8006
    pub url: String,
    /// e.g. "nodestarr@pve!nodestarr"
    pub token_id: String,
    pub token_secret: String,
    pub tls_mode: TlsMode,
    /// SHA-256 fingerprint, "AB:CD:..." (filled automatically by "Test").
    pub fingerprint: String,
    /// Node name; empty picks the first node.
    pub node: String,
    /// VMID of the guest Nodestarr runs in (protects it from accidental stops).
    pub self_vmid: Option<u32>,
    /// Default storage for "Back up now".
    pub backup_storage: String,
}

impl Default for Proxmox {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            token_id: String::new(),
            token_secret: String::new(),
            tls_mode: TlsMode::Fingerprint,
            fingerprint: String::new(),
            node: String::new(),
            self_vmid: None,
            backup_storage: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// MQTT / Home Assistant
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Mqtt {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub username: String,
    pub password: String,
    pub client_id: String,
    pub discovery_prefix: String,
    pub base_topic: String,
    /// Publish CPU/memory sensors per container.
    pub expose_stats: bool,
    /// Expose start/stop/restart/update controls.
    pub expose_controls: bool,
}

impl Default for Mqtt {
    fn default() -> Self {
        Self {
            enabled: false,
            host: String::new(),
            port: 1883,
            tls: false,
            username: String::new(),
            password: String::new(),
            client_id: "nodestarr".into(),
            discovery_prefix: "homeassistant".into(),
            base_topic: "nodestarr".into(),
            expose_stats: true,
            expose_controls: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Monitoring
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    /// Docker host available memory below threshold percent.
    HostMemory,
    /// Proxmox node memory used above threshold percent.
    ProxmoxMemory,
    /// Proxmox storage used above threshold percent.
    ProxmoxStorage,
    /// Container memory above threshold percent of its limit.
    ContainerMemory,
    /// Container CPU above threshold percent.
    ContainerCpu,
    /// Restarted at least `threshold` times within `duration_secs`.
    RestartLoop,
    /// Health check failing for `duration_secs`.
    Unhealthy,
    /// Exited with a non-zero code without being stopped from Nodestarr/Docker.
    UnexpectedExit,
    /// Killed by the kernel OOM killer.
    Oom,
    /// A watched path's filesystem above threshold percent full.
    Disk,
    /// No successful backup for `threshold` hours (containers with backups on).
    BackupAge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: String,
    pub kind: AlertKind,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub threshold: f64,
    #[serde(default)]
    pub duration_secs: u32,
    #[serde(default)]
    pub severity: Severity,
}

pub fn default_alert_rules() -> Vec<AlertRule> {
    let rule = |id: &str, kind, threshold, duration_secs, severity, enabled| AlertRule {
        id: id.into(),
        kind,
        enabled,
        threshold,
        duration_secs,
        severity,
    };
    vec![
        rule("host-memory", AlertKind::HostMemory, 10.0, 300, Severity::Warning, true),
        rule("proxmox-memory", AlertKind::ProxmoxMemory, 90.0, 300, Severity::Warning, true),
        rule("proxmox-storage", AlertKind::ProxmoxStorage, 90.0, 0, Severity::Warning, true),
        rule("restart-loop", AlertKind::RestartLoop, 3.0, 600, Severity::Error, true),
        rule("unhealthy", AlertKind::Unhealthy, 0.0, 120, Severity::Warning, true),
        rule("unexpected-exit", AlertKind::UnexpectedExit, 0.0, 0, Severity::Error, true),
        rule("oom", AlertKind::Oom, 0.0, 0, Severity::Error, true),
        rule("disk", AlertKind::Disk, 90.0, 0, Severity::Warning, true),
        rule("container-memory", AlertKind::ContainerMemory, 90.0, 300, Severity::Warning, true),
        rule("container-cpu", AlertKind::ContainerCpu, 95.0, 900, Severity::Info, false),
        rule("backup-age", AlertKind::BackupAge, 48.0, 0, Severity::Warning, true),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Monitoring {
    /// Record per-container CPU/memory/network history.
    pub history_enabled: bool,
    pub sample_secs: u32,
    /// Raw samples kept this long; older data is kept as 15-minute averages.
    pub raw_retention_hours: u32,
    pub rollup_retention_days: u32,
    /// Refresh interval for live stats while the UI is open.
    pub live_secs: u32,
    /// Filesystems to watch for the disk alert (paths as Nodestarr sees them).
    pub disk_paths: Vec<String>,
    pub rules: Vec<AlertRule>,
    pub activity_retention_days: u32,
    pub task_retention_days: u32,
}

impl Default for Monitoring {
    fn default() -> Self {
        Self {
            history_enabled: true,
            sample_secs: 60,
            raw_retention_hours: 48,
            rollup_retention_days: 30,
            live_secs: 3,
            disk_paths: vec!["/config".into(), "/backups".into()],
            rules: default_alert_rules(),
            activity_retention_days: 90,
            task_retention_days: 30,
        }
    }
}

// ---------------------------------------------------------------------------
// Uptime checks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomCheck {
    pub id: String,
    pub name: String,
    /// http(s)://... or tcp://host:port
    pub url: String,
    #[serde(default)]
    pub expect_status: String,
    #[serde(default)]
    pub keyword: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Uptime {
    pub enabled: bool,
    pub interval_secs: u32,
    pub timeout_secs: u32,
    /// Consecutive failures before a check counts as down.
    pub failures_before_down: u32,
    /// Create checks automatically for containers with a web UI.
    pub auto_checks: bool,
    /// Accepted HTTP statuses, e.g. "200-399,401,403". Empty = anything below 500.
    pub expect_status: String,
    pub custom: Vec<CustomCheck>,
    pub retention_days: u32,
}

impl Default for Uptime {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: 60,
            timeout_secs: 10,
            failures_before_down: 3,
            auto_checks: true,
            expect_status: String::new(),
            custom: Vec::new(),
            retention_days: 30,
        }
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Parses a cron expression, accepting 5- or 6-field forms and @aliases.
pub fn parse_cron(expr: &str) -> Result<croner::Cron, String> {
    croner::Cron::from_str(expr.trim()).map_err(|e| format!("invalid schedule '{expr}': {e}"))
}

/// Parses "200-399,401" style status lists.
pub fn parse_status_ranges(spec: &str) -> Result<Vec<(u16, u16)>, String> {
    let mut out = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (part, part),
        };
        let a: u16 = a.parse().map_err(|_| format!("invalid status '{part}'"))?;
        let b: u16 = b.parse().map_err(|_| format!("invalid status '{part}'"))?;
        if a > b || !(100..=599).contains(&a) || !(100..=599).contains(&b) {
            return Err(format!("invalid status range '{part}'"));
        }
        out.push((a, b));
    }
    Ok(out)
}

fn validate_hhmm(s: &str) -> Result<(), String> {
    let ok = s
        .split_once(':')
        .and_then(|(h, m)| Some((h.parse::<u32>().ok()?, m.parse::<u32>().ok()?)))
        .is_some_and(|(h, m)| h < 24 && m < 60);
    if ok { Ok(()) } else { Err(format!("invalid time '{s}', expected HH:MM")) }
}

impl Settings {
    /// Carries choices stored by older versions over to the fields that
    /// replaced them. Runs after every load and before every save.
    pub fn upgrade(&mut self) {
        if let Some(scope) = self.updates.default_auto_scope.take() {
            self.updates.auto_install = AutoInstall::from_scope(scope);
        }
    }

    /// Checks the whole document; returns a user-facing message on the first problem.
    pub fn validate(&self) -> Result<(), String> {
        let g = &self.general;
        if !g.url_base.is_empty() && (!g.url_base.starts_with('/') || g.url_base.ends_with('/')) {
            return Err("URL base must start with '/' and not end with '/', e.g. /nodestarr".into());
        }
        if !g.timezone.is_empty() && g.timezone.parse::<chrono_tz::Tz>().is_err() {
            return Err(format!("unknown time zone '{}'", g.timezone));
        }
        if !g.external_url.is_empty() && url::Url::parse(&g.external_url).is_err() {
            return Err("external URL must be a full URL like http://192.168.1.50:7979".into());
        }

        let s = &self.security;
        CidrList::parse(&s.local_networks)?;
        CidrList::parse(&s.trusted_proxies)?;
        if s.auth_mode == AuthMode::External && s.trusted_proxies.is_empty() {
            return Err("external authentication needs at least one trusted proxy".into());
        }
        if s.session_hours == 0 || s.remember_days == 0 {
            return Err("session lengths must be at least 1".into());
        }

        for dir in &self.docker.stacks_dirs {
            if !dir.starts_with('/') {
                return Err(format!("stacks folder '{dir}' must be an absolute path"));
            }
        }

        let u = &self.updates;
        parse_cron(&u.check_cron)?;
        parse_cron(&u.auto_cron)?;
        if u.verify_timeout_secs < 10 {
            return Err("update verification timeout must be at least 10 seconds".into());
        }

        let b = &self.backups;
        if !b.root.starts_with('/') {
            return Err("backup folder must be an absolute path".into());
        }
        parse_cron(&b.cron)?;
        if !(1..=19).contains(&b.zstd_level) {
            return Err("zstd level must be between 1 and 19".into());
        }
        for pattern in &b.exclude {
            globset::Glob::new(pattern).map_err(|e| format!("invalid exclude pattern '{pattern}': {e}"))?;
        }

        let mut ids = std::collections::HashSet::new();
        for p in &self.notifications.providers {
            if !ids.insert(&p.id) {
                return Err(format!("duplicate notification provider id '{}'", p.id));
            }
            validate_provider(p)?;
        }
        let q = &self.notifications.quiet_hours;
        validate_hhmm(&q.start)?;
        validate_hhmm(&q.end)?;

        let px = &self.proxmox;
        if px.enabled {
            let parsed = url::Url::parse(&px.url).map_err(|_| "Proxmox URL is not valid")?;
            if parsed.scheme() != "https" {
                return Err("Proxmox URL must use https".into());
            }
            if !px.token_id.contains('!') {
                return Err("Proxmox token ID looks like user@realm!tokenname".into());
            }
            if px.token_secret.is_empty() {
                return Err("Proxmox token secret is required".into());
            }
        }

        let m = &self.mqtt;
        if m.enabled && m.host.trim().is_empty() {
            return Err("MQTT broker host is required".into());
        }

        let mon = &self.monitoring;
        if mon.sample_secs < 10 {
            return Err("sampling interval must be at least 10 seconds".into());
        }
        if mon.live_secs < 1 {
            return Err("live refresh must be at least 1 second".into());
        }

        let up = &self.uptime;
        if up.interval_secs < 10 {
            return Err("uptime check interval must be at least 10 seconds".into());
        }
        parse_status_ranges(&up.expect_status)?;
        for c in &up.custom {
            if !(c.url.starts_with("http://") || c.url.starts_with("https://") || c.url.starts_with("tcp://")) {
                return Err(format!("check '{}' needs an http(s):// or tcp:// URL", c.name));
            }
            parse_status_ranges(&c.expect_status)?;
        }
        Ok(())
    }

    /// The configured time zone, falling back to $TZ and then UTC.
    pub fn tz(&self) -> chrono_tz::Tz {
        let from_setting = self.general.timezone.parse::<chrono_tz::Tz>().ok();
        from_setting.or_else(|| std::env::var("TZ").ok().and_then(|t| t.parse().ok())).unwrap_or(chrono_tz::UTC)
    }
}

fn validate_provider(p: &NotificationProvider) -> Result<(), String> {
    let need = |v: &str, what: &str| {
        if v.trim().is_empty() { Err(format!("{}: {what} is required", p.name)) } else { Ok(()) }
    };
    let need_url = |v: &str, what: &str| -> Result<(), String> {
        need(v, what)?;
        let u = url::Url::parse(v).map_err(|_| format!("{}: {what} is not a valid URL", p.name))?;
        if u.scheme() != "http" && u.scheme() != "https" {
            return Err(format!("{}: {what} must be http(s)", p.name));
        }
        Ok(())
    };
    match &p.config {
        ProviderConfig::Discord { webhook_url, .. } => need_url(webhook_url, "webhook URL"),
        ProviderConfig::Slack { webhook_url } => need_url(webhook_url, "webhook URL"),
        ProviderConfig::Email { host, from, to, security, .. } => {
            need(host, "SMTP host")?;
            need(from, "from address")?;
            if to.is_empty() {
                return Err(format!("{}: at least one recipient is required", p.name));
            }
            if !matches!(security.as_str(), "starttls" | "tls" | "none") {
                return Err(format!("{}: security must be starttls, tls or none", p.name));
            }
            Ok(())
        }
        ProviderConfig::Telegram { bot_token, chat_id, .. } => {
            need(bot_token, "bot token")?;
            need(chat_id, "chat ID")
        }
        ProviderConfig::Ntfy { server, topic, .. } => {
            need_url(server, "server")?;
            need(topic, "topic")
        }
        ProviderConfig::Gotify { server, app_token } => {
            need_url(server, "server")?;
            need(app_token, "app token")
        }
        ProviderConfig::Pushover { user_key, app_token, .. } => {
            need(user_key, "user key")?;
            need(app_token, "app token")
        }
        ProviderConfig::Webhook { url, method, body_template, .. } => {
            need_url(url, "URL")?;
            if !matches!(method.to_ascii_uppercase().as_str(), "POST" | "PUT" | "PATCH" | "GET") {
                return Err(format!("{}: unsupported method {method}", p.name));
            }
            if !body_template.is_empty() {
                minijinja::Environment::new()
                    .template_from_str(body_template)
                    .map_err(|e| format!("{}: template error: {e}", p.name))?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        Settings::default().validate().unwrap();
    }

    #[test]
    fn empty_document_deserializes_to_defaults() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.general.instance_name, "Nodestarr");
        assert_eq!(s.docker.stacks_dirs, vec!["/opt/stacks".to_string()]);
        let partial: Settings = serde_json::from_str(r#"{"updates": {"min_age_hours": 0}}"#).unwrap();
        assert_eq!(partial.updates.min_age_hours, 0);
        assert_eq!(partial.updates.keep_rollback_images, 2);
    }

    #[test]
    fn auto_update_scope_carries_over() {
        let mut s: Settings = serde_json::from_str(r#"{"updates": {"default_auto_scope": "patch"}}"#).unwrap();
        s.upgrade();
        let a = s.updates.auto_install;
        assert!(a.patch && a.rebuild && a.switch_tags && !a.minor && !a.major);
        assert!(s.updates.default_auto_scope.is_none());
        // Never written back, so it can't override later choices.
        let json = serde_json::to_value(&s).unwrap();
        assert!(json["updates"].get("default_auto_scope").is_none());
        assert_eq!(json["updates"]["auto_install"]["switch_tags"].as_bool(), Some(true));
        // "New builds only" keeps tags where they are.
        assert!(!AutoInstall::from_scope(TagScope::Digest).switch_tags);
        let mut fresh = Settings::default();
        fresh.upgrade();
        assert_eq!(fresh.updates.auto_install, AutoInstall::default());
    }

    #[test]
    fn status_ranges() {
        assert_eq!(parse_status_ranges("200-399, 401").unwrap(), vec![(200, 399), (401, 401)]);
        assert!(parse_status_ranges("abc").is_err());
        assert!(parse_status_ranges("400-300").is_err());
        assert!(parse_status_ranges("").unwrap().is_empty());
    }

    #[test]
    fn rejects_bad_values() {
        let mut s = Settings::default();
        s.updates.check_cron = "not a cron".into();
        assert!(s.validate().is_err());

        let mut s = Settings::default();
        s.general.url_base = "nodestarr/".into();
        assert!(s.validate().is_err());

        let mut s = Settings::default();
        s.security.auth_mode = AuthMode::External;
        assert!(s.validate().is_err());
    }

    #[test]
    fn provider_validation() {
        let mut s = Settings::default();
        s.notifications.providers.push(NotificationProvider {
            id: "d".into(),
            name: "Discord".into(),
            enabled: true,
            events: vec![],
            min_severity: Severity::Info,
            config: ProviderConfig::Discord {
                webhook_url: "not a url".into(),
                username: String::new(),
                mention: String::new(),
            },
        });
        assert!(s.validate().is_err());
    }
}
