//! Update overview, bulk actions, history and schedule.

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::App;
use crate::error::Result;
use crate::http::Auth;
use crate::http::api::views::{ViewContext, container_view};
use crate::updates::apply::{Trigger, UpdateRequest};

pub fn routes() -> Router<App> {
    Router::new()
        .route("/updates", get(overview))
        .route("/updates/check", post(check_all))
        .route("/updates/apply", post(apply))
        .route("/updates/auto/run", post(run_auto))
        .route("/updates/history", get(history))
}

async fn overview(State(app): State<App>) -> Result<Json<Value>> {
    let ctx = ViewContext::load(&app).await;
    let items: Vec<_> = app
        .docker
        .list()
        .into_iter()
        .map(|s| container_view(&app, &ctx, s))
        .filter(|v| !v.prefs.hidden)
        .collect();
    let settings = app.settings.get();
    let schedule: Vec<Value> = crate::scheduler::next_runs(&settings)
        .into_iter()
        .map(|(job, next)| json!({ "job": job, "next": next.map(|n| n.timestamp()) }))
        .collect();
    Ok(Json(json!({
        "containers": items,
        "checking": app.updates.is_checking(),
        "last_check": app.updates.last_check(),
        "schedule": schedule,
    })))
}

async fn check_all(State(app): State<App>, Auth(p): Auth) -> Result<Json<Value>> {
    if app.updates.is_checking() {
        return Ok(Json(json!({ "started": false, "message": "a check is already running" })));
    }
    let actor = p.actor();
    let app2 = app.clone();
    tokio::spawn(async move {
        crate::updates::check::check_all(&app2, true, &actor).await;
    });
    Ok(Json(json!({ "started": true })))
}

#[derive(Deserialize)]
struct ApplyBody {
    /// Container ids, names or identities.
    targets: Vec<String>,
    #[serde(default)]
    skip_backup: bool,
}

async fn apply(State(app): State<App>, Auth(p): Auth, Json(b): Json<ApplyBody>) -> Result<Json<Value>> {
    let actor = p.actor();
    let mut tasks = Vec::new();
    let mut errors = Vec::new();
    for t in &b.targets {
        let req = UpdateRequest { tag: None, skip_backup: b.skip_backup };
        match crate::updates::apply::start_update(&app, t, req, &actor, Trigger::Manual) {
            Ok(id) => tasks.push(id),
            Err(e) => errors.push(json!({ "target": t, "error": e.to_string() })),
        }
    }
    Ok(Json(json!({ "task_ids": tasks, "errors": errors })))
}

async fn run_auto(State(app): State<App>, Auth(_p): Auth) -> Result<Json<Value>> {
    let app2 = app.clone();
    tokio::spawn(async move {
        if !crate::scheduler::run(&app2, crate::scheduler::Job::AutoUpdate).await {
            app2.bus.toast(crate::settings::Severity::Info, "Automatic updates are already running");
        }
    });
    Ok(Json(json!({ "started": true })))
}

#[derive(Deserialize)]
struct HistoryQuery {
    identity: Option<String>,
    limit: Option<u32>,
}

async fn history(State(app): State<App>, Query(q): Query<HistoryQuery>) -> Result<Json<Value>> {
    Ok(Json(serde_json::to_value(app.updates.history(q.identity, q.limit.unwrap_or(100)).await?)?))
}
