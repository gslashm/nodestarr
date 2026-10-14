//! Monitoring: host overview, resource history, alerts and uptime checks.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::App;
use crate::error::{AppError, Result};
use crate::http::api::containers::range_secs;

pub fn routes() -> Router<App> {
    Router::new()
        .route("/monitoring/overview", get(overview))
        .route("/monitoring/history", get(history))
        .route("/monitoring/alerts", get(alerts))
        .route("/monitoring/alerts/{id}/dismiss", post(dismiss_alert))
        .route("/uptime", get(uptime_list))
        .route("/uptime/run", post(uptime_run))
        .route("/uptime/test", post(uptime_test))
        .route("/uptime/{id}/beats", get(uptime_beats))
}

/// Everything the Monitoring page and the dashboard header need at once.
async fn overview(State(app): State<App>) -> Result<Json<Value>> {
    let containers = app.docker.list();
    let running = containers.iter().filter(|c| c.is_running()).count();
    let unhealthy = containers.iter().filter(|c| c.health.as_deref() == Some("unhealthy")).count();
    let stats = crate::monitor::stats::latest(&app);
    let (cpu_total, mem_total) = stats.values().fold((0.0, 0u64), |(c, m), s| (c + s.cpu, m + s.mem));
    let alerts = crate::monitor::alerts::list(&app, true, 100).await?;
    let uptime = app.uptime.all();
    let down = uptime.iter().filter(|u| u.state == "down").count();
    let updates = app.updates.all();
    let updates_available = updates.values().filter(|u| u.has_update()).count();
    Ok(Json(json!({
        "host": crate::monitor::host::current(&app),
        "disks": crate::monitor::host::disks(&app),
        "docker": app.docker.info(),
        "containers": {
            "total": containers.len(),
            "running": running,
            "stopped": containers.len() - running,
            "unhealthy": unhealthy,
            "cpu": (cpu_total * 10.0).round() / 10.0,
            "memory": mem_total,
        },
        "alerts": alerts,
        "uptime": { "checks": uptime.len(), "down": down },
        "updates_available": updates_available,
        "stats": stats.into_values().collect::<Vec<_>>(),
        "history_enabled": app.settings.get().monitoring.history_enabled,
    })))
}

#[derive(Deserialize)]
struct HistoryQuery {
    /// "host", or a container id, name or identity.
    target: String,
    range: Option<String>,
    points: Option<usize>,
}

async fn history(State(app): State<App>, Query(q): Query<HistoryQuery>) -> Result<Json<Value>> {
    let identity = if q.target == "host" {
        "host".to_string()
    } else {
        match app.docker.get(&q.target) {
            Some(e) => e.summary.identity.clone(),
            // A removed container's history is still reachable by identity.
            None if q.target.contains(':') => q.target.clone(),
            None => return Err(AppError::not_found("container")),
        }
    };
    let to = crate::util::now();
    let from = to - range_secs(q.range.as_deref());
    let series = crate::monitor::stats::history(&app, &identity, from, to, q.points.unwrap_or(720)).await?;
    Ok(Json(json!({ "identity": identity, "from": from, "to": to, "series": series })))
}

#[derive(Deserialize)]
struct AlertsQuery {
    #[serde(deserialize_with = "crate::util::flag", default)]
    active: bool,
    limit: Option<u32>,
}

async fn alerts(State(app): State<App>, Query(q): Query<AlertsQuery>) -> Result<Json<Value>> {
    let list = crate::monitor::alerts::list(&app, q.active, q.limit.unwrap_or(200)).await?;
    let settings = app.settings.get();
    Ok(Json(json!({ "alerts": list, "rules": settings.monitoring.rules })))
}

async fn dismiss_alert(State(app): State<App>, Path(id): Path<i64>) -> Json<Value> {
    app.monitor.dismiss_alert(id);
    Json(json!({ "ok": true }))
}

async fn uptime_list(State(app): State<App>) -> Json<Value> {
    let settings = app.settings.get();
    Json(json!({
        "enabled": settings.uptime.enabled,
        "interval_secs": settings.uptime.interval_secs,
        "checks": app.uptime.all(),
    }))
}

async fn uptime_run(State(app): State<App>) -> Json<Value> {
    let app2 = app.clone();
    tokio::spawn(async move {
        crate::monitor::uptime::run_now(&app2).await;
    });
    Json(json!({ "started": true }))
}

#[derive(Deserialize)]
struct TestBody {
    url: String,
    #[serde(default)]
    expect_status: String,
    #[serde(default)]
    keyword: String,
}

async fn uptime_test(State(app): State<App>, Json(b): Json<TestBody>) -> Result<Json<Value>> {
    let expect = if b.expect_status.trim().is_empty() { app.settings.get().uptime.expect_status.clone() } else { b.expect_status };
    let result = crate::monitor::uptime::test(&app, b.url.trim(), &expect, &b.keyword).await?;
    Ok(Json(serde_json::to_value(result)?))
}

#[derive(Deserialize)]
struct BeatsQuery {
    range: Option<String>,
}

async fn uptime_beats(State(app): State<App>, Path(id): Path<String>, Query(q): Query<BeatsQuery>) -> Result<Json<Value>> {
    let since = crate::util::now() - range_secs(q.range.as_deref().or(Some("24h")));
    let beats = crate::monitor::uptime::beats(&app, &id, since).await?;
    Ok(Json(json!({ "id": id, "status": app.uptime.get(&id), "beats": beats })))
}
