//! App integrations overview: what each recognized app reports about itself.

use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{StreamExt, stream};
use serde_json::{Value, json};

use crate::app::App;
use crate::docker::Entry;
use crate::error::Result;
use crate::integrations::kind_of;

pub fn routes() -> Router<App> {
    Router::new()
        .route("/integrations", get(list))
        .route("/integrations/refresh", post(refresh_all))
        .route("/integrations/streams", get(streams))
}

fn integrated(app: &App) -> Vec<Arc<Entry>> {
    app.docker.entries().into_iter().filter(|e| kind_of(e).is_some()).collect()
}

async fn list(State(app): State<App>) -> Json<Value> {
    let settings = app.settings.get();
    let statuses = app.integrations.all();
    let mut items: Vec<Value> = integrated(&app)
        .iter()
        .map(|e| {
            let identity = &e.summary.identity;
            let prefs = app.prefs.get(identity);
            json!({
                "identity": identity,
                "id": e.summary.id,
                "name": crate::updates::check::display_name(&app, e),
                "app": e.summary.app,
                "kind": kind_of(e),
                "running": e.summary.is_running(),
                "enabled": prefs.integration.enabled != Some(false),
                "overridden": !prefs.integration.base_url.is_empty() || !prefs.integration.api_key.is_empty(),
                "status": statuses.get(identity),
            })
        })
        .collect();
    items.sort_by(|a, b| {
        a["name"].as_str().unwrap_or("").to_lowercase().cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
    });
    Json(json!({
        "enabled": settings.integrations.enabled,
        "poll_secs": settings.integrations.poll_secs,
        "apps": items,
    }))
}

/// Polls every integrated app now and returns the fresh list.
async fn refresh_all(State(app): State<App>) -> Result<Json<Value>> {
    stream::iter(integrated(&app))
        .map(|e| {
            let app = app.clone();
            async move {
                app.integrations.forget_creds(&e.summary.identity);
                crate::integrations::poll(&app, &e).await;
            }
        })
        .buffer_unordered(4)
        .collect::<Vec<_>>()
        .await;
    Ok(list(State(app)).await)
}

async fn streams(State(app): State<App>) -> Json<Value> {
    Json(json!({ "active_streams": crate::integrations::active_streams(&app).await }))
}
