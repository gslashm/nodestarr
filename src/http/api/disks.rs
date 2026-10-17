//! Proxmox disk health: disks, SMART history, pools, self-tests and their
//! schedules, and SSH host access to the nodes.

use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::App;
use crate::error::{AppError, Result};
use crate::events::Event;
use crate::http::Auth;
use crate::proxmox::disks::{self, Label, host, selftest};
use crate::settings::TestSchedule;

pub fn routes() -> Router<App> {
    Router::new()
        .route("/proxmox/disk-health", get(overview))
        .route("/proxmox/disk-health/refresh", post(refresh))
        .route("/proxmox/disk-health/events", get(events))
        .route("/proxmox/disk-health/schedules", put(put_schedules))
        .route("/proxmox/disk-health/scrub", post(scrub))
        .route("/proxmox/disk-health/pool-history", get(pool_history))
        .route("/proxmox/disk-health/tests/{id}", delete(cancel_test))
        .route("/proxmox/disk-health/disks/{id}", get(disk).delete(forget))
        .route("/proxmox/disk-health/disks/{id}/history", get(history))
        .route("/proxmox/disk-health/disks/{id}/raw", get(raw))
        .route("/proxmox/disk-health/disks/{id}/tests", post(start_test))
        .route("/proxmox/disk-health/disks/{id}/tests/abort", post(abort_test))
        .route("/proxmox/disk-health/disks/{id}/identify", post(identify))
        .route("/proxmox/disk-health/disks/{id}/label", put(label))
        .route("/proxmox/host-access", get(host_access))
        .route("/proxmox/host-access/key", post(create_key))
        .route("/proxmox/host-access/test", post(test_access))
        .route("/proxmox/host-access/host-keys/{node}", delete(forget_host_key))
        .route("/proxmox/host-access/helper.sh", get(helper_script))
}

fn schedules_view(app: &App) -> Vec<Value> {
    let settings = app.settings.get();
    let all = app.disks.disks();
    settings
        .proxmox
        .disks
        .schedules
        .iter()
        .map(|s| {
            let covers = all.iter().filter(|d| d.present && selftest::selects(s, d)).count();
            let mut v = serde_json::to_value(s).unwrap_or_default();
            v["next_run"] = json!(if s.enabled { selftest::next_run(&settings, &s.cron) } else { None });
            v["covers"] = json!(covers);
            v
        })
        .collect()
}

async fn overview(State(app): State<App>) -> Result<Json<Value>> {
    let s = app.settings.get();
    let active = selftest_active(&app).await?;
    let recent = disks::events(&app, None, 40).await?;
    Ok(Json(json!({
        "enabled": s.proxmox.enabled && s.proxmox.disks.enabled,
        "proxmox": s.proxmox.enabled,
        "sampling": app.disks.sampling(),
        "sample_minutes": s.proxmox.disks.sample_minutes,
        "host_access": {
            "enabled": s.proxmox.host_access.enabled,
            "has_key": app.disks.host().has_key(),
            "nodes": app.disks.host().status(),
        },
        "disks": app.disks.disks(),
        "pools": app.disks.pools(),
        "tests": { "active": active, "recent": recent_tests(&app, None, 30).await? },
        "schedules": schedules_view(&app),
        "events": recent,
    })))
}

async fn selftest_active(app: &App) -> Result<Vec<selftest::TestRun>> {
    disks::tests(app, true, None, 0).await
}

async fn recent_tests(app: &App, disk: Option<String>, limit: u32) -> Result<Vec<selftest::TestRun>> {
    disks::tests(app, false, disk, limit).await
}

async fn refresh(State(app): State<App>) -> Json<Value> {
    Json(json!({ "started": disks::refresh(&app) }))
}

#[derive(Deserialize)]
struct EventsQuery {
    subject: Option<String>,
    limit: Option<u32>,
}

async fn events(State(app): State<App>, Query(q): Query<EventsQuery>) -> Result<Json<Value>> {
    Ok(Json(json!({ "events": disks::events(&app, q.subject.as_deref(), q.limit.unwrap_or(100)).await? })))
}

async fn disk(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>> {
    let d = app.disks.disk(&id).ok_or_else(|| AppError::not_found(format!("disk {id}")))?;
    let active = selftest_active(&app).await?.into_iter().find(|t| t.disk == id);
    Ok(Json(json!({
        "disk": d,
        "report": app.disks.report(&id),
        "has_raw": app.disks.raw(&id).is_some(),
        "test": active,
        "tests": recent_tests(&app, Some(id.clone()), 50).await?,
        "events": disks::events(&app, Some(&id), 200).await?,
    })))
}

#[derive(Deserialize)]
struct DaysQuery {
    days: Option<u32>,
}

async fn history(State(app): State<App>, Path(id): Path<String>, Query(q): Query<DaysQuery>) -> Result<Json<Value>> {
    let days = q.days.unwrap_or(30);
    Ok(Json(json!({ "id": id, "days": days, "points": disks::history(&app, &id, days).await? })))
}

async fn raw(State(app): State<App>, Path(id): Path<String>) -> Result<impl IntoResponse> {
    let v = app.disks.raw(&id).ok_or_else(|| {
        AppError::not_found("no smartctl output yet (it's kept in memory after a read through host access)")
    })?;
    let name = format!("attachment; filename=\"smartctl-{id}.json\"");
    Ok(([(header::CONTENT_DISPOSITION, name)], Json(v.as_ref().clone())))
}

#[derive(Deserialize)]
struct TestBody {
    kind: String,
}

async fn start_test(
    State(app): State<App>,
    Auth(p): Auth,
    Path(id): Path<String>,
    Json(b): Json<TestBody>,
) -> Result<Json<Value>> {
    Ok(Json(json!({ "test": selftest::enqueue(&app, &id, &b.kind, &p.actor()).await? })))
}

async fn abort_test(State(app): State<App>, Auth(p): Auth, Path(id): Path<String>) -> Result<Json<Value>> {
    Ok(Json(json!({ "test": selftest::abort(&app, &id, &p.actor()).await? })))
}

async fn cancel_test(State(app): State<App>, Auth(p): Auth, Path(id): Path<String>) -> Result<Json<Value>> {
    Ok(Json(json!({ "test": selftest::cancel(&app, &id, &p.actor()).await? })))
}

#[derive(Deserialize)]
struct IdentifyBody {
    #[serde(default = "thirty")]
    seconds: u32,
}

fn thirty() -> u32 {
    30
}

async fn identify(
    State(app): State<App>,
    Auth(p): Auth,
    Path(id): Path<String>,
    body: Option<Json<IdentifyBody>>,
) -> Result<Json<Value>> {
    let seconds = body.map(|Json(b)| b.seconds).unwrap_or(30);
    disks::identify(&app, &id, seconds, &p.actor()).await?;
    Ok(Json(json!({ "ok": true, "seconds": seconds.clamp(5, 300) })))
}

async fn label(State(app): State<App>, Path(id): Path<String>, Json(l): Json<Label>) -> Result<Json<Value>> {
    Ok(Json(json!({ "disk": disks::set_label(&app, &id, l).await? })))
}

async fn forget(State(app): State<App>, Auth(p): Auth, Path(id): Path<String>) -> Result<Json<Value>> {
    disks::forget(&app, &id, &p.actor()).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn put_schedules(State(app): State<App>, Json(list): Json<Vec<TestSchedule>>) -> Result<Json<Value>> {
    app.settings
        .update(move |s| {
            s.proxmox.disks.schedules = list;
            Ok(())
        })
        .await?;
    app.bus.publish(Event::Settings);
    app.bus.publish(Event::Disks);
    Ok(Json(json!({ "schedules": schedules_view(&app) })))
}

#[derive(Deserialize)]
struct ScrubBody {
    pool: String,
    action: String,
}

async fn scrub(State(app): State<App>, Auth(p): Auth, Json(b): Json<ScrubBody>) -> Result<Json<Value>> {
    disks::scrub(&app, &b.pool, &b.action, &p.actor()).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct PoolHistoryQuery {
    pool: String,
    days: Option<u32>,
}

async fn pool_history(State(app): State<App>, Query(q): Query<PoolHistoryQuery>) -> Result<Json<Value>> {
    let points: Vec<Value> = disks::pool_history(&app, &q.pool, q.days.unwrap_or(90))
        .await?
        .into_iter()
        .map(|(ts, size, used)| json!({ "ts": ts, "size": size, "used": used }))
        .collect();
    Ok(Json(json!({ "pool": q.pool, "points": points })))
}

async fn host_access(State(app): State<App>) -> Result<Json<Value>> {
    let s = app.settings.get();
    let ha = &s.proxmox.host_access;
    let mut names: Vec<String> = match app.proxmox.cached_overview() {
        Some(o) if !o.nodes.is_empty() => o.nodes.iter().map(|n| n.node.clone()).collect(),
        Some(o) => vec![o.node.node.clone()],
        None => Vec::new(),
    };
    for n in ha.addresses.keys() {
        if !names.contains(n) {
            names.push(n.clone());
        }
    }
    let mut nodes = Vec::new();
    for n in &names {
        let configured = ha.addresses.get(n).map(|a| a.trim().to_string()).filter(|a| !a.is_empty());
        nodes.push(json!({
            "node": n,
            "address": disks::node_address(&app, n).await,
            "configured": configured.is_some(),
            "status": app.disks.host().node(n),
            "host_keys": app.disks.host().host_keys(n),
        }));
    }
    Ok(Json(json!({
        "enabled": ha.enabled,
        "has_key": app.disks.host().has_key(),
        "public_key": app.disks.host().public_key(),
        "install": app.disks.host().install_commands(&ha.user),
        "helper_version": host::HELPER_VERSION,
        "helper_path": host::HELPER_PATH,
        "nodes": nodes,
    })))
}

async fn create_key(State(app): State<App>) -> Result<Json<Value>> {
    let comment = format!(
        "nodestarr@{}",
        app.settings.get().general.instance_name.to_ascii_lowercase().replace(char::is_whitespace, "-")
    );
    let key = app.disks.host().create_key(&comment).await?;
    Ok(Json(json!({ "public_key": key })))
}

#[derive(Deserialize)]
struct TestAccessBody {
    node: String,
}

async fn test_access(State(app): State<App>, Json(b): Json<TestAccessBody>) -> Result<Json<Value>> {
    Ok(Json(json!({ "node": disks::test_access(&app, &b.node).await? })))
}

async fn forget_host_key(State(app): State<App>, Path(node): Path<String>) -> Result<Json<Value>> {
    app.disks.host().forget_host_key(&node)?;
    Ok(Json(json!({ "ok": true })))
}

async fn helper_script() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8"),
            (header::CONTENT_DISPOSITION, "attachment; filename=\"nodestarr-host\""),
        ],
        host::HELPER_SCRIPT,
    )
}
