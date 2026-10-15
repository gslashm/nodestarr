//! Proxmox VE: node and guest overview, power actions, snapshots, vzdump
//! backups, tasks, package updates and disk health.

use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::App;
use crate::error::{AppError, Result};
use crate::http::Auth;
use crate::proxmox::{BackupRequest, GuestAction};

pub fn routes() -> Router<App> {
    Router::new()
        .route("/proxmox/status", get(status))
        .route("/proxmox/rrd", get(rrd))
        .route("/proxmox/guests/{vmid}/actions/{action}", post(guest_action))
        .route("/proxmox/guests/{vmid}/snapshots", get(snapshots).post(create_snapshot))
        .route("/proxmox/guests/{vmid}/snapshots/{name}", delete(delete_snapshot))
        .route("/proxmox/guests/{vmid}/snapshots/{name}/rollback", post(rollback_snapshot))
        .route("/proxmox/guests/{vmid}/backup", post(backup))
        .route("/proxmox/storage/{storage}/backups", get(storage_backups))
        .route("/proxmox/tasks", get(tasks))
        .route("/proxmox/tasks/{upid}/log", get(task_log))
        .route("/proxmox/apt", get(apt))
        .route("/proxmox/apt/refresh", post(apt_refresh))
        .route("/proxmox/disks", get(disks))
}

#[derive(Deserialize)]
struct StatusQuery {
    /// Bypass the cache.
    #[serde(deserialize_with = "crate::util::flag", default)]
    refresh: bool,
}

async fn status(State(app): State<App>, Query(q): Query<StatusQuery>) -> Json<Value> {
    let settings = app.settings.get();
    if !settings.proxmox.enabled {
        return Json(json!({ "enabled": false }));
    }
    let max_age = if q.refresh { 0 } else { 20 };
    match app.proxmox.overview(max_age).await {
        Ok(o) => Json(json!({ "enabled": true, "overview": o, "self_vmid": app.proxmox.self_vmid(), "error": null })),
        Err(err) => Json(json!({
            "enabled": true,
            // Show the last good data with the error, if there is any.
            "overview": app.proxmox.cached_overview(),
            "self_vmid": app.proxmox.self_vmid(),
            "error": err.to_string(),
        })),
    }
}

#[derive(Deserialize)]
struct RrdQuery {
    vmid: Option<u32>,
    /// hour, day, week, month or year.
    timeframe: Option<String>,
}

async fn rrd(State(app): State<App>, Query(q): Query<RrdQuery>) -> Result<Json<Value>> {
    let tf = q.timeframe.as_deref().unwrap_or("hour");
    let points = app.proxmox.rrd(q.vmid, tf).await?;
    Ok(Json(json!({ "vmid": q.vmid, "timeframe": tf, "points": points })))
}

#[derive(Deserialize, Default)]
struct ConfirmBody {
    /// Confirms a disruptive action on the guest that runs Nodestarr.
    #[serde(default)]
    confirm_self: bool,
}

async fn guest_action(
    State(app): State<App>,
    Auth(p): Auth,
    Path((vmid, action)): Path<(u32, String)>,
    body: Option<Json<ConfirmBody>>,
) -> Result<Json<Value>> {
    let action: GuestAction = serde_json::from_value(Value::String(action.clone()))
        .map_err(|_| AppError::bad_request(format!("unknown action '{action}' (start, shutdown, stop, reboot, suspend, resume)")))?;
    let confirm = body.map(|Json(b)| b.confirm_self).unwrap_or(false);
    let task = crate::proxmox::guest_action(&app, vmid, action, confirm, &p.actor()).await?;
    Ok(Json(json!({ "task_id": task })))
}

async fn snapshots(State(app): State<App>, Path(vmid): Path<u32>) -> Result<Json<Value>> {
    Ok(Json(json!({ "vmid": vmid, "snapshots": app.proxmox.snapshots(vmid).await? })))
}

#[derive(Deserialize)]
struct SnapshotBody {
    name: String,
    #[serde(default)]
    description: String,
    /// Include RAM (VMs only).
    #[serde(default)]
    include_ram: bool,
}

async fn create_snapshot(State(app): State<App>, Auth(p): Auth, Path(vmid): Path<u32>, Json(b): Json<SnapshotBody>) -> Result<Json<Value>> {
    let task = crate::proxmox::create_snapshot(&app, vmid, b.name.trim(), &b.description, b.include_ram, &p.actor()).await?;
    Ok(Json(json!({ "task_id": task })))
}

async fn delete_snapshot(State(app): State<App>, Auth(p): Auth, Path((vmid, name)): Path<(u32, String)>) -> Result<Json<Value>> {
    let task = crate::proxmox::delete_snapshot(&app, vmid, &name, &p.actor()).await?;
    Ok(Json(json!({ "task_id": task })))
}

async fn rollback_snapshot(
    State(app): State<App>,
    Auth(p): Auth,
    Path((vmid, name)): Path<(u32, String)>,
    body: Option<Json<ConfirmBody>>,
) -> Result<Json<Value>> {
    let confirm = body.map(|Json(b)| b.confirm_self).unwrap_or(false);
    let task = crate::proxmox::rollback_snapshot(&app, vmid, &name, confirm, &p.actor()).await?;
    Ok(Json(json!({ "task_id": task })))
}

async fn backup(State(app): State<App>, Auth(p): Auth, Path(vmid): Path<u32>, Json(req): Json<BackupRequest>) -> Result<Json<Value>> {
    let task = crate::proxmox::backup_now(&app, vmid, req, &p.actor()).await?;
    Ok(Json(json!({ "task_id": task })))
}

async fn storage_backups(State(app): State<App>, Path(storage): Path<String>) -> Result<Json<Value>> {
    Ok(Json(json!({ "storage": storage, "backups": app.proxmox.backups(&storage).await? })))
}

#[derive(Deserialize)]
struct TasksQuery {
    limit: Option<u32>,
}

async fn tasks(State(app): State<App>, Query(q): Query<TasksQuery>) -> Result<Json<Value>> {
    Ok(Json(json!({ "tasks": app.proxmox.tasks(q.limit.unwrap_or(50)).await? })))
}

async fn task_log(State(app): State<App>, Path(upid): Path<String>) -> Result<Json<Value>> {
    Ok(Json(json!({ "upid": upid, "lines": app.proxmox.task_log(&upid).await? })))
}

async fn apt(State(app): State<App>) -> Result<Json<Value>> {
    Ok(Json(json!({ "updates": app.proxmox.apt_updates().await? })))
}

async fn apt_refresh(State(app): State<App>, Auth(p): Auth) -> Result<Json<Value>> {
    let task = crate::proxmox::refresh_apt(&app, &p.actor()).await?;
    Ok(Json(json!({ "task_id": task })))
}

async fn disks(State(app): State<App>) -> Result<Json<Value>> {
    Ok(Json(json!({ "disks": app.proxmox.disks().await? })))
}
