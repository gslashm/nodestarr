//! Nodestarr itself: health, status, settings (with tests, export and import),
//! tasks, activity, Docker resources (images, volumes, networks, cleanup),
//! scheduled jobs, self-update and Prometheus metrics.

use std::collections::HashMap;
use std::str::FromStr;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::activity::{ActivityQuery, NewActivity};
use crate::app::App;
use crate::docker::resources::{self, NewNetwork, NewVolume, PruneRequest};
use crate::error::{AppError, Result};
use crate::events::Event;
use crate::http::Auth;
use crate::prefs::ContainerPrefs;
use crate::scheduler::Job;
use crate::settings::{NotificationProvider, Settings, Severity, mask_secrets, merge_secrets};
use crate::tasks::TaskSpec;

/// Settings sections the API accepts, with display names for messages.
const SECTIONS: &[(&str, &str)] = &[
    ("general", "General"),
    ("security", "Security"),
    ("docker", "Docker"),
    ("updates", "Update"),
    ("backups", "Backup"),
    ("notifications", "Notification"),
    ("integrations", "Integration"),
    ("proxmox", "Proxmox"),
    ("mqtt", "Home Assistant"),
    ("monitoring", "Monitoring"),
    ("uptime", "Uptime check"),
];

/// Format marker for exported settings files.
const EXPORT_FORMAT: u32 = 1;

pub fn routes() -> Router<App> {
    Router::new()
        .route("/health", get(health))
        .route("/system/status", get(status))
        .route("/system/logs", get(logs))
        .route("/system/timezones", get(timezones))
        .route("/system/df", get(disk_usage))
        .route("/system/prune", post(prune))
        .route("/system/update", post(self_update))
        .route("/system/jobs", get(jobs))
        .route("/system/jobs/{job}/run", post(run_job))
        .route("/system/mqtt/reconnect", post(mqtt_reconnect))
        .route("/settings", get(get_settings))
        .route("/settings/export", get(export))
        .route("/settings/import", post(import))
        .route("/settings/test/notification", post(test_notification))
        .route("/settings/test/proxmox", post(test_proxmox))
        .route("/settings/test/mqtt", post(test_mqtt))
        .route("/settings/{section}", put(put_section))
        .route("/notifications/kinds", get(notification_kinds))
        .route("/tasks", get(tasks))
        .route("/tasks/{id}", get(task))
        .route("/tasks/{id}/cancel", post(cancel_task))
        .route("/activity", get(activity))
        .route("/apps", get(apps))
        .route("/images", get(images))
        .route("/images/{id}", delete(remove_image))
        .route("/volumes", get(volumes).post(create_volume))
        .route("/volumes/{name}", delete(remove_volume))
        .route("/networks", get(networks).post(create_network))
        .route("/networks/{id}", delete(remove_network))
}

// ------------------------------------------------------------ health, status

/// Public liveness probe (used by the container HEALTHCHECK).
async fn health(State(app): State<App>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "version": crate::VERSION,
        "docker": app.docker.is_connected(),
    }))
}

/// Resident memory of the Nodestarr process, from /proc.
fn process_memory() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|kb| kb.parse::<u64>().ok())
        .map(|kb| kb * 1024)
}

fn db_size(app: &App) -> u64 {
    let path = app.boot.db_path();
    let mut total = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    for suffix in ["-wal", "-shm"] {
        let mut p = path.clone().into_os_string();
        p.push(suffix);
        total += std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
    }
    total
}

async fn status(State(app): State<App>) -> Json<Value> {
    let settings = app.settings.get();
    let containers = app.docker.list();
    let running = containers.iter().filter(|c| c.is_running()).count();
    let self_container = containers.iter().find(|c| c.is_self).map(|c| {
        json!({
            "id": c.id,
            "name": c.name,
            "identity": c.identity,
            "image": c.image,
            "update": app.updates.get(&c.identity),
        })
    });
    Json(json!({
        "version": crate::VERSION,
        "instance_name": settings.general.instance_name,
        "started_at": app.started_at,
        "now": crate::util::now(),
        "timezone": settings.tz().name(),
        "process": {
            "memory": process_memory(),
            "pid": std::process::id(),
            "workers": app.boot.workers,
        },
        "data_dir": app.boot.data_dir,
        "database_size": db_size(&app),
        "docker": {
            "host": crate::util::redact_url(&app.boot.docker_host),
            "connected": app.docker.is_connected(),
            "info": app.docker.info(),
            "compose": app.compose.version(),
        },
        "containers": { "total": containers.len(), "running": running },
        "tasks_active": app.tasks.active_count(),
        "live_viewers": app.monitor.viewers(),
        "self": self_container,
        "mqtt": {
            "enabled": settings.mqtt.enabled,
            "connected": app.mqtt.is_connected(),
            "error": app.mqtt.last_error(),
        },
        "proxmox": { "enabled": settings.proxmox.enabled },
        "backups_root_available": app.backups.root_available(&app),
        "repo": crate::selfupdate::REPO,
    }))
}

#[derive(Deserialize)]
struct LogsQuery {
    limit: Option<usize>,
    /// Minimum level: trace, debug, info, warn, error.
    level: Option<String>,
}

async fn logs(Query(q): Query<LogsQuery>) -> Result<Json<Value>> {
    let level = match q.level.as_deref().filter(|l| !l.is_empty()) {
        Some(l) => Some(tracing::Level::from_str(l).map_err(|_| AppError::bad_request(format!("unknown log level '{l}'")))?),
        None => None,
    };
    let entries = crate::logging::recent(q.limit.unwrap_or(500).min(2000), level);
    Ok(Json(json!({ "entries": entries })))
}

async fn timezones() -> Json<Value> {
    let zones: Vec<&str> = chrono_tz::TZ_VARIANTS.iter().map(|tz| tz.name()).collect();
    Json(json!({ "timezones": zones }))
}

// ------------------------------------------------------------ settings

fn section_label(section: &str) -> Result<&'static str> {
    SECTIONS
        .iter()
        .find(|(k, _)| *k == section)
        .map(|(_, label)| *label)
        .ok_or_else(|| AppError::not_found(format!("settings section '{section}'")))
}

async fn get_settings(State(app): State<App>) -> Json<Value> {
    Json(json!({
        "settings": app.settings.masked(),
        "defaults": {
            "alert_rules": crate::settings::default_alert_rules(),
        },
        "mqtt_topics": crate::mqtt::topics(&app.settings.get().mqtt),
        "secret_placeholder": crate::settings::SECRET_PLACEHOLDER,
    }))
}

/// Side effects of saved settings that services don't pick up on their own.
async fn after_settings_change(app: &App, before: &Settings, after: &Settings) {
    if before.general.public_host != after.general.public_host {
        let app = app.clone();
        tokio::spawn(async move { app.docker.rebuild_summaries().await });
    }
    if before.docker.compose_command != after.docker.compose_command {
        let app = app.clone();
        tokio::spawn(async move {
            app.compose.detect().await;
            app.bus.publish(Event::StacksChanged);
        });
    } else if before.docker.stacks_dirs != after.docker.stacks_dirs {
        app.bus.publish(Event::StacksChanged);
    }
    app.bus.publish(Event::Settings);
}

async fn put_section(State(app): State<App>, Auth(p): Auth, Path(section): Path<String>, Json(body): Json<Value>) -> Result<Json<Value>> {
    let label = section_label(&section)?;
    if !body.is_object() {
        return Err(AppError::bad_request("expected a JSON object"));
    }
    let before = app.settings.get();
    let after = app.settings.replace_section(&section, body).await?;
    after_settings_change(&app, &before, &after).await;
    app.note(
        NewActivity::new("settings.changed", Severity::Info, format!("{label} settings saved"))
            .actor(&p.actor())
            .details(json!({ "section": section })),
    )
    .await;
    Ok(Json(json!({ "settings": app.settings.masked() })))
}

#[derive(Deserialize)]
struct ExportQuery {
    /// Include passwords and tokens (the file is then sensitive).
    #[serde(deserialize_with = "crate::util::flag", default)]
    secrets: bool,
}

async fn export(State(app): State<App>, Auth(p): Auth, Query(q): Query<ExportQuery>) -> Result<Response> {
    p.require_admin()?;
    let mut settings = serde_json::to_value(&*app.settings.get())?;
    let mut prefs = serde_json::to_value(app.prefs.all().iter().map(|(k, v)| (k.clone(), (**v).clone())).collect::<HashMap<String, ContainerPrefs>>())?;
    if !q.secrets {
        mask_secrets(&mut settings);
        mask_secrets(&mut prefs);
    }
    let doc = json!({
        "nodestarr_export": EXPORT_FORMAT,
        "version": crate::VERSION,
        "exported_at": crate::util::now(),
        "includes_secrets": q.secrets,
        "settings": settings,
        "container_prefs": prefs,
    });
    app.note(
        NewActivity::new("settings.exported", Severity::Info, if q.secrets { "Settings exported (with secrets)" } else { "Settings exported" })
            .actor(&p.actor()),
    )
    .await;
    let mut resp = Json(doc).into_response();
    let name = format!("nodestarr-settings-{}.json", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
        resp.headers_mut().insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(resp)
}

#[derive(Deserialize)]
struct ImportQuery {
    #[serde(deserialize_with = "crate::util::flag", default = "yes")]
    settings: bool,
    #[serde(deserialize_with = "crate::util::flag", default = "yes")]
    prefs: bool,
}

fn yes() -> bool {
    true
}

async fn import(State(app): State<App>, Auth(p): Auth, Query(q): Query<ImportQuery>, Json(doc): Json<Value>) -> Result<Json<Value>> {
    p.require_admin()?;
    if doc["nodestarr_export"].as_u64() != Some(EXPORT_FORMAT as u64) {
        return Err(AppError::bad_request("this is not a Nodestarr settings export"));
    }
    let mut sections = Vec::new();
    if q.settings {
        if let Some(incoming) = doc["settings"].as_object().cloned() {
            let before = app.settings.get();
            let after = app
                .settings
                .update(|s| {
                    let mut current = serde_json::to_value(&*s)?;
                    for (section, value) in incoming {
                        // Unknown sections (from a newer Nodestarr) are ignored.
                        let Some(cur) = current.get(&section).cloned() else { continue };
                        let mut value = value;
                        // Masked secrets in the file keep the stored values.
                        merge_secrets(&mut value, &cur);
                        current[&section] = value;
                        sections.push(section);
                    }
                    *s = serde_json::from_value(current)
                        .map_err(|e| AppError::bad_request(format!("the file has invalid settings: {e}")))?;
                    Ok(())
                })
                .await?;
            after_settings_change(&app, &before, &after).await;
        }
    }
    let mut prefs_count = 0;
    if q.prefs {
        if let Some(all) = doc["container_prefs"].as_object() {
            for (identity, value) in all {
                let current = serde_json::to_value(&*app.prefs.get(identity))?;
                let mut value = value.clone();
                merge_secrets(&mut value, &current);
                let prefs: ContainerPrefs = serde_json::from_value(value)
                    .map_err(|e| AppError::bad_request(format!("invalid preferences for {identity}: {e}")))?;
                app.prefs.set(identity, prefs).await?;
                app.bus.publish(Event::Prefs { identity: identity.clone() });
                prefs_count += 1;
            }
        }
    }
    app.note(
        NewActivity::new(
            "settings.imported",
            Severity::Warning,
            format!("Settings imported ({} sections, {prefs_count} container preferences)", sections.len()),
        )
        .actor(&p.actor()),
    )
    .await;
    Ok(Json(json!({ "sections": sections, "container_prefs": prefs_count })))
}

/// Fills secrets the browser sent back as placeholders from the stored copy.
fn with_stored_secrets(mut incoming: Value, stored: Option<Value>) -> Value {
    match stored {
        Some(cur) => merge_secrets(&mut incoming, &cur),
        None => merge_secrets(&mut incoming, &Value::Null),
    }
    incoming
}

async fn test_notification(State(app): State<App>, Auth(p): Auth, Json(body): Json<Value>) -> Result<Json<Value>> {
    p.require_admin()?;
    let stored = body["id"].as_str().and_then(|id| {
        app.settings
            .get()
            .notifications
            .providers
            .iter()
            .find(|pr| pr.id == id)
            .and_then(|pr| serde_json::to_value(pr).ok())
    });
    let provider: NotificationProvider = serde_json::from_value(with_stored_secrets(body, stored))
        .map_err(|e| AppError::bad_request(format!("invalid provider: {e}")))?;
    app.notifier.test(&app, &provider).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn test_proxmox(State(app): State<App>, Auth(p): Auth, Json(body): Json<Value>) -> Result<Json<Value>> {
    p.require_admin()?;
    let stored = serde_json::to_value(&app.settings.get().proxmox).ok();
    let settings: crate::settings::Proxmox = serde_json::from_value(with_stored_secrets(body, stored))
        .map_err(|e| AppError::bad_request(format!("invalid Proxmox settings: {e}")))?;
    let result = crate::proxmox::ProxmoxService::test(&settings).await?;
    Ok(Json(result))
}

async fn test_mqtt(State(app): State<App>, Auth(p): Auth, Json(body): Json<Value>) -> Result<Json<Value>> {
    p.require_admin()?;
    let stored = serde_json::to_value(&app.settings.get().mqtt).ok();
    let settings: crate::settings::Mqtt = serde_json::from_value(with_stored_secrets(body, stored))
        .map_err(|e| AppError::bad_request(format!("invalid MQTT settings: {e}")))?;
    crate::mqtt::test(&settings).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn mqtt_reconnect(State(app): State<App>) -> Json<Value> {
    app.mqtt.reload();
    Json(json!({ "ok": true }))
}

async fn notification_kinds(State(app): State<App>) -> Json<Value> {
    let kinds: Vec<Value> = crate::notify::EVENT_KINDS
        .iter()
        .map(|(kind, label)| json!({ "kind": kind, "label": label }))
        .collect();
    Json(json!({ "kinds": kinds, "status": app.notifier.status() }))
}

// ------------------------------------------------------------ tasks, activity

#[derive(Deserialize)]
struct TasksQuery {
    limit: Option<usize>,
    /// Only tasks for this container identity.
    target: Option<String>,
    /// Only unfinished tasks.
    #[serde(deserialize_with = "crate::util::flag", default)]
    active: bool,
}

async fn tasks(State(app): State<App>, Query(q): Query<TasksQuery>) -> Result<Json<Value>> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let mut list = app.tasks.list(if q.target.is_some() { 500 } else { limit }).await?;
    if let Some(t) = &q.target {
        list.retain(|s| s.target.as_deref() == Some(t.as_str()));
    }
    if q.active {
        list.retain(|s| !s.status.is_finished());
    }
    list.truncate(limit);
    Ok(Json(json!({ "tasks": list })))
}

async fn task(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>> {
    let (snapshot, log) = app.tasks.get(&id).await?;
    Ok(Json(json!({ "task": snapshot, "log": log })))
}

async fn cancel_task(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>> {
    if app.tasks.cancel(&id) {
        Ok(Json(json!({ "cancelling": true })))
    } else {
        Err(AppError::conflict("the task is not running"))
    }
}

async fn activity(State(app): State<App>, Query(q): Query<ActivityQuery>) -> Result<Json<Value>> {
    let entries = app.activity.list(q).await?;
    Ok(Json(json!({ "entries": entries })))
}

async fn apps() -> Json<Value> {
    let list: Vec<Value> = crate::docker::apps::CATALOG
        .iter()
        .map(|a| {
            json!({
                "slug": a.slug,
                "name": a.name,
                "web_port": a.web_port,
                "integration": a.integration,
                "database": a.db,
                "sqlite": a.sqlite,
            })
        })
        .collect();
    Json(json!({ "apps": list }))
}

// ------------------------------------------------------------ jobs, self-update

async fn jobs(State(app): State<App>) -> Json<Value> {
    let settings = app.settings.get();
    let next: HashMap<Job, Option<i64>> = crate::scheduler::next_runs(&settings)
        .into_iter()
        .map(|(j, n)| (j, n.map(|t| t.timestamp())))
        .collect();
    let last = crate::scheduler::last_runs();
    let list: Vec<Value> = Job::ALL
        .iter()
        .map(|j| {
            json!({
                "job": j.key(),
                "name": j.name(),
                "enabled": next.contains_key(j),
                "next_run": next.get(j).copied().flatten(),
                "last_run": last.get(j),
            })
        })
        .collect();
    Json(json!({ "jobs": list }))
}

async fn run_job(State(app): State<App>, Auth(p): Auth, Path(job): Path<String>) -> Result<Json<Value>> {
    let job = Job::from_str(&job).map_err(AppError::NotFound)?;
    if crate::scheduler::last_runs().get(&job).is_some_and(|r| r.finished_at.is_none()) {
        return Err(AppError::conflict(format!("the {} is already running", job.name())));
    }
    app.note(NewActivity::new("system.job", Severity::Info, format!("Started the {} by hand", job.name())).actor(&p.actor()))
        .await;
    let app2 = app.clone();
    tokio::spawn(async move {
        crate::scheduler::run(&app2, job).await;
    });
    Ok(Json(json!({ "started": true })))
}

#[derive(Deserialize, Default)]
struct SelfUpdateBody {
    /// Switch Nodestarr to another tag (for example a pinned version).
    #[serde(default)]
    tag: Option<String>,
}

async fn self_update(State(app): State<App>, Auth(p): Auth, body: Option<Json<SelfUpdateBody>>) -> Result<Json<Value>> {
    p.require_admin()?;
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let me = app
        .docker
        .list()
        .into_iter()
        .find(|c| c.is_self)
        .ok_or_else(|| AppError::conflict("Nodestarr isn't running in a Docker container Nodestarr can see, so it can't update itself; update it the way you installed it"))?;
    let req = crate::updates::apply::UpdateRequest { tag: body.tag, skip_backup: true };
    let task = crate::updates::apply::start_update(&app, &me.id, req, &p.actor(), crate::updates::apply::Trigger::Manual)?;
    Ok(Json(json!({ "task_id": task })))
}

// ------------------------------------------------------------ docker resources

async fn disk_usage(State(app): State<App>) -> Result<Json<Value>> {
    let usage = resources::disk_usage(&app).await?;
    Ok(Json(serde_json::to_value(usage)?))
}

async fn prune(State(app): State<App>, Auth(p): Auth, Json(req): Json<PruneRequest>) -> Result<Json<Value>> {
    p.require_admin()?;
    let nothing = !(req.dangling_images || req.unused_images || req.containers || req.volumes || req.networks || req.build_cache);
    if nothing {
        return Err(AppError::bad_request("choose what to clean up"));
    }
    let spec = TaskSpec::new("prune", "Clean up Docker", &p.actor()).lock("docker:prune");
    let app2 = app.clone();
    let actor = p.actor();
    let task = app.tasks.spawn(spec, move |ctx| async move {
        let log = |l: String| ctx.log(l);
        let report = resources::prune(&app2, &req, &log).await?;
        ctx.log(format!("Freed {}", crate::util::human_bytes(report.reclaimed.max(0) as u64)));
        for e in &report.errors {
            ctx.log(format!("Problem: {e}"));
        }
        app2.note(
            NewActivity::new(
                "docker.prune",
                if report.errors.is_empty() { Severity::Success } else { Severity::Warning },
                format!(
                    "Docker cleanup freed {} ({} images, {} containers, {} volumes, {} networks)",
                    crate::util::human_bytes(report.reclaimed.max(0) as u64),
                    report.images_removed,
                    report.containers_removed,
                    report.volumes_removed,
                    report.networks_removed
                ),
            )
            .actor(&actor),
        )
        .await;
        Ok(serde_json::to_value(report)?)
    });
    Ok(Json(json!({ "task_id": task })))
}

async fn images(State(app): State<App>) -> Result<Json<Value>> {
    let rows = resources::images(&app).await?;
    let total: i64 = rows.iter().map(|r| r.size).sum();
    Ok(Json(json!({ "images": rows, "total_size": total })))
}

#[derive(Deserialize)]
struct ForceQuery {
    #[serde(deserialize_with = "crate::util::flag", default)]
    force: bool,
}

async fn remove_image(State(app): State<App>, Auth(p): Auth, Path(id): Path<String>, Query(q): Query<ForceQuery>) -> Result<Json<Value>> {
    let removed = resources::remove_image(&app, &id, q.force).await?;
    app.note(NewActivity::new("docker.image_removed", Severity::Info, format!("Image {} removed", short_ref(&id))).actor(&p.actor()))
        .await;
    Ok(Json(json!({ "removed": removed })))
}

fn short_ref(id: &str) -> String {
    let trimmed = id.trim_start_matches("sha256:");
    if trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        trimmed[..12].to_string()
    } else {
        id.to_string()
    }
}

#[derive(Deserialize)]
struct VolumesQuery {
    /// Ask Docker for sizes (slow on large volumes).
    #[serde(deserialize_with = "crate::util::flag", default)]
    sizes: bool,
}

async fn volumes(State(app): State<App>, Query(q): Query<VolumesQuery>) -> Result<Json<Value>> {
    let rows = resources::volumes(&app, q.sizes).await?;
    Ok(Json(json!({ "volumes": rows })))
}

async fn create_volume(State(app): State<App>, Auth(p): Auth, Json(v): Json<NewVolume>) -> Result<Json<Value>> {
    let name = resources::create_volume(&app, v).await?;
    app.note(NewActivity::new("docker.volume_created", Severity::Info, format!("Volume {name} created")).actor(&p.actor()))
        .await;
    Ok(Json(json!({ "name": name })))
}

async fn remove_volume(State(app): State<App>, Auth(p): Auth, Path(name): Path<String>) -> Result<Json<Value>> {
    resources::remove_volume(&app, &name).await?;
    app.note(NewActivity::new("docker.volume_removed", Severity::Warning, format!("Volume {} removed", short_ref(&name))).actor(&p.actor()))
        .await;
    Ok(Json(json!({ "ok": true })))
}

async fn networks(State(app): State<App>) -> Result<Json<Value>> {
    Ok(Json(json!({ "networks": resources::networks(&app).await? })))
}

async fn create_network(State(app): State<App>, Auth(p): Auth, Json(n): Json<NewNetwork>) -> Result<Json<Value>> {
    let name = n.name.clone();
    let id = resources::create_network(&app, n).await?;
    app.note(NewActivity::new("docker.network_created", Severity::Info, format!("Network {name} created")).actor(&p.actor()))
        .await;
    Ok(Json(json!({ "id": id })))
}

async fn remove_network(State(app): State<App>, Auth(p): Auth, Path(id): Path<String>) -> Result<Json<Value>> {
    resources::remove_network(&app, &id).await?;
    app.note(NewActivity::new("docker.network_removed", Severity::Info, format!("Network {} removed", short_ref(&id))).actor(&p.actor()))
        .await;
    Ok(Json(json!({ "ok": true })))
}

// ------------------------------------------------------------ metrics

/// Prometheus metrics (`/metrics`, outside the API prefix).
pub async fn metrics(State(app): State<App>) -> Response {
    let body = crate::metrics::render(&app).await;
    let mut resp = body.into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_match_the_settings_document() {
        let doc = serde_json::to_value(Settings::default()).unwrap();
        let keys: Vec<&str> = doc.as_object().unwrap().keys().map(String::as_str).collect();
        for (section, _) in SECTIONS {
            assert!(keys.contains(section), "unknown section {section}");
        }
        assert_eq!(keys.len(), SECTIONS.len(), "every settings section should be editable");
    }

    #[test]
    fn short_refs() {
        let id = format!("sha256:{}", "a".repeat(64));
        assert_eq!(short_ref(&id), "aaaaaaaaaaaa");
        assert_eq!(short_ref("media_config"), "media_config");
    }
}
