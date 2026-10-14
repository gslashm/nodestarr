//! Backup listing, runs, downloads and restores.

use std::path::PathBuf;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::App;
use crate::error::{AppError, Result};
use crate::http::Auth;

pub fn routes() -> Router<App> {
    Router::new()
        .route("/backups", get(list))
        .route("/backups/run", post(run))
        .route("/backups/scheduled/run", post(run_scheduled))
        .route("/backups/reindex", post(reindex))
        .route("/backups/{id}", delete(remove))
        .route("/backups/{id}/pin", put(pin))
        .route("/backups/{id}/download", get(download))
        .route("/backups/{id}/contents", get(contents))
        .route("/backups/{id}/restore", post(restore))
}

fn root(app: &App) -> PathBuf {
    PathBuf::from(&app.settings.get().backups.root)
}

#[derive(Deserialize)]
struct ListQuery {
    identity: Option<String>,
    limit: Option<u32>,
}

async fn list(State(app): State<App>, Query(q): Query<ListQuery>) -> Result<Json<Value>> {
    let root = root(&app);
    let records = app.backups.list(&root, q.identity, q.limit.unwrap_or(500)).await?;
    let usage = crate::util::fsx::disk_usage(&root).ok();
    let total: i64 = records.iter().filter(|r| r.exists).map(|r| r.size).sum();
    let settings = app.settings.get();
    let next = crate::scheduler::next_runs(&settings)
        .into_iter()
        .find(|(j, _)| *j == crate::scheduler::Job::Backup)
        .and_then(|(_, n)| n.map(|n| n.timestamp()));
    Ok(Json(json!({
        "backups": records,
        "root": settings.backups.root,
        "root_available": app.backups.root_available(&app),
        "disk": usage,
        "total_size": total,
        "schedule_enabled": settings.backups.schedule_enabled,
        "next_run": next,
    })))
}

#[derive(Deserialize)]
struct RunBody {
    #[serde(default)]
    targets: Vec<String>,
    #[serde(default)]
    stack: Option<String>,
}

async fn run(State(app): State<App>, Auth(p): Auth, Json(b): Json<RunBody>) -> Result<Json<Value>> {
    let actor = p.actor();
    let mut tasks = Vec::new();
    if let Some(stack) = &b.stack {
        tasks.push(crate::backups::start_stack_backup(&app, stack, &actor)?);
    }
    for t in &b.targets {
        tasks.push(crate::backups::start_backup(&app, t, &actor)?);
    }
    Ok(Json(json!({ "task_ids": tasks })))
}

async fn run_scheduled(State(app): State<App>, Auth(_p): Auth) -> Result<Json<Value>> {
    let app2 = app.clone();
    tokio::spawn(async move {
        if !crate::scheduler::run(&app2, crate::scheduler::Job::Backup).await {
            app2.bus.toast(crate::settings::Severity::Info, "A scheduled backup is already running");
        }
    });
    Ok(Json(json!({ "started": true })))
}

async fn reindex(State(app): State<App>) -> Result<Json<Value>> {
    let n = app.backups.reindex(&root(&app)).await?;
    Ok(Json(json!({ "indexed": n })))
}

async fn remove(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>> {
    app.backups.delete(&root(&app), &id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct PinBody {
    pinned: bool,
}

async fn pin(State(app): State<App>, Path(id): Path<String>, Json(b): Json<PinBody>) -> Result<Json<Value>> {
    app.backups.set_pinned(&id, b.pinned).await?;
    Ok(Json(json!({ "pinned": b.pinned })))
}

async fn download(State(app): State<App>, Auth(p): Auth, Path(id): Path<String>) -> Result<Response> {
    p.require_admin()?;
    let root = root(&app);
    let rec = app.backups.get(&root, &id).await?;
    let path = root.join(&rec.path);
    let file = tokio::fs::File::open(&path).await.map_err(|_| AppError::not_found("archive file"))?;
    let len = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    let stream = tokio_util::io::ReaderStream::with_capacity(file, 256 * 1024);
    let mut resp = Body::from_stream(stream).into_response();
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "backup.tar.zst".into());
    let prefix = crate::util::slug(rec.target_name.as_deref().unwrap_or("backup"));
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{prefix}-{name}\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(resp)
}

async fn contents(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>> {
    let root = root(&app);
    let rec = app.backups.get(&root, &id).await?;
    let path = root.join(&rec.path);
    let entries = tokio::task::spawn_blocking(move || crate::backups::archive::list_entries(&path, 2000)).await??;
    Ok(Json(json!({
        "record": rec,
        "entries": entries.into_iter().map(|(n, s)| json!({ "name": n, "size": s })).collect::<Vec<_>>(),
    })))
}

async fn restore(State(app): State<App>, Auth(p): Auth, Path(id): Path<String>, body: Option<Json<crate::backups::restore::RestoreOptions>>) -> Result<Json<Value>> {
    p.require_admin()?;
    let opts = body.map(|Json(b)| b).unwrap_or_default();
    let task = crate::backups::restore::start_restore(&app, &id, opts, &p.actor()).await?;
    Ok(Json(json!({ "task_id": task })))
}
