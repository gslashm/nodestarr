//! Application state and startup.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;

use crate::activity::{ActivityLog, Actor, NewActivity};
use crate::auth::AuthService;
use crate::backups::BackupService;
use crate::compose::ComposeService;
use crate::config::BootConfig;
use crate::db::Db;
use crate::docker::DockerService;
use crate::events::EventBus;
use crate::integrations::IntegrationService;
use crate::monitor::MonitorService;
use crate::monitor::uptime::UptimeService;
use crate::mqtt::MqttService;
use crate::notify::Notifier;
use crate::prefs::PrefsStore;
use crate::proxmox::ProxmoxService;
use crate::proxmox::disks::DiskService;
use crate::registry::RegistryClient;
use crate::settings::{SettingsStore, Severity};
use crate::tasks::TaskManager;
use crate::updates::UpdateService;

/// Shared state handed to every request handler and background job.
pub struct AppState {
    pub boot: BootConfig,
    pub db: Db,
    pub settings: SettingsStore,
    pub bus: EventBus,
    pub activity: ActivityLog,
    pub tasks: TaskManager,
    pub prefs: PrefsStore,
    pub docker: DockerService,
    pub compose: ComposeService,
    pub registry: RegistryClient,
    pub updates: UpdateService,
    pub backups: BackupService,
    pub notifier: Notifier,
    pub integrations: IntegrationService,
    pub proxmox: ProxmoxService,
    /// Proxmox disk health, pools and self-tests.
    pub disks: DiskService,
    pub monitor: MonitorService,
    pub uptime: UptimeService,
    pub mqtt: MqttService,
    pub auth: AuthService,
    /// Shared HTTP client for outbound calls (registries, notifications, APIs).
    pub http: reqwest::Client,
    pub started_at: i64,
}

pub type App = Arc<AppState>;

impl AppState {
    /// Records an activity entry without failing the caller.
    pub async fn note(&self, entry: NewActivity) {
        self.activity.note(entry).await;
    }
}

/// Builds the shared outbound HTTP client.
pub fn http_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(crate::USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .pool_idle_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(2)
        .build()
        .context("building HTTP client")
}

/// Installs rustls' crypto provider (ring) once per process.
pub fn init_tls() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Opens storage and constructs all services (no background work yet).
pub async fn build(boot: BootConfig) -> anyhow::Result<App> {
    init_tls();
    std::fs::create_dir_all(&boot.data_dir)
        .with_context(|| format!("creating data directory {}", boot.data_dir.display()))?;
    let _ = std::fs::remove_dir_all(boot.data_dir.join("tmp"));

    let db = Db::open(&boot.db_path()).with_context(|| format!("opening {}", boot.db_path().display()))?;
    let settings = SettingsStore::load(db.clone()).await?;
    if let Some(base) = &boot.url_base {
        let base = base.trim_end_matches('/').to_string();
        settings
            .update(|s| {
                s.general.url_base = base;
                Ok(())
            })
            .await?;
    }
    let bus = EventBus::new();
    let http = http_client()?;
    let activity = ActivityLog::new(db.clone(), bus.clone());
    let tasks = TaskManager::new(db.clone(), bus.clone());
    let prefs = PrefsStore::load(db.clone()).await?;

    let api = crate::docker::connect(&boot.docker_host).await.or_else(|err| {
        // Start anyway: the inventory loop keeps retrying, and the UI shows
        // a clear "Docker unreachable" state instead of a crash loop.
        tracing::error!(%err, "Docker is not reachable yet");
        crate::docker::connect_lazy(&boot.docker_host)
    })?;
    let docker = DockerService::new(api, bus.clone(), settings.clone());
    let compose = ComposeService::new(settings.clone(), docker.clone(), boot.data_dir.clone());
    let registry = RegistryClient::new(http.clone(), settings.clone());
    let updates = UpdateService::load(db.clone(), bus.clone()).await?;
    let backups = BackupService::new(db.clone(), bus.clone());
    let notifier = Notifier::new(http.clone(), settings.clone(), db.clone());
    let integrations = IntegrationService::new(http.clone(), bus.clone());
    let proxmox = ProxmoxService::new(settings.clone(), docker.clone());
    let disks = DiskService::new(bus.clone(), &boot.data_dir);
    let monitor = MonitorService::new(db.clone(), bus.clone());
    let uptime = UptimeService::new(bus.clone());
    let mqtt = MqttService::new();
    let auth = AuthService::new(db.clone(), settings.clone());

    let app = Arc::new(AppState {
        boot,
        db,
        settings,
        bus,
        activity,
        tasks,
        prefs,
        docker,
        compose,
        registry,
        updates,
        backups,
        notifier,
        integrations,
        proxmox,
        disks,
        monitor,
        uptime,
        mqtt,
        auth,
        http,
        started_at: crate::util::now(),
    });
    Ok(app)
}

/// Starts background services: Docker inventory, schedulers, monitors.
pub async fn start_background(app: &App) {
    tokio::spawn(app.docker.clone().run());

    let compose = app.compose.clone();
    tokio::spawn(async move {
        match compose.detect().await {
            Some(v) => tracing::info!(version = %v, "docker compose available"),
            None => tracing::warn!("docker compose not found; stack actions are disabled"),
        }
    });

    // Wait briefly for the first inventory so schedulers see containers.
    for _ in 0..50 {
        if app.docker.is_synced() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    crate::scheduler::start(app.clone());
    crate::monitor::start(app.clone());
    crate::monitor::uptime::start(app.clone());
    crate::integrations::start(app.clone());
    crate::mqtt::start(app.clone());
    crate::proxmox::start(app.clone());
    crate::proxmox::disks::start(app.clone());
    crate::notify::start(app.clone());
}

/// `nodestarr serve`.
pub async fn serve(boot: BootConfig) -> anyhow::Result<()> {
    crate::logging::init(&boot.log);
    tracing::info!(version = crate::VERSION, data = %boot.data_dir.display(), "starting Nodestarr");

    let app = build(boot).await?;
    crate::auth::bootstrap_admin(&app).await?;
    start_background(&app).await;

    let listen = app.boot.listen;
    let router = crate::http::router(app.clone());
    let listener = tokio::net::TcpListener::bind(listen).await.with_context(|| format!("binding {listen}"))?;
    tracing::info!(%listen, "web UI ready");
    app.note(
        NewActivity::new("system.started", Severity::Info, format!("Nodestarr {} started", crate::VERSION))
            .actor(&Actor::system()),
    )
    .await;

    axum::serve(listener, router.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HTTP server failed")?;
    tracing::info!("shutting down");
    app.mqtt.shutdown().await;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }
}
