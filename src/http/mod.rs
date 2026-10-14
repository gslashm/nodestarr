//! HTTP server: REST API under /api/v1, live events (SSE), WebSockets for
//! console and logs, Prometheus metrics, and the embedded web UI.

mod api;
mod assets;
mod middleware;
mod sse;
mod ws;

use axum::Router;
use axum::routing::get;

use crate::app::App;

pub use middleware::{Auth, ClientIp};

pub fn router(app: App) -> Router {
    let api = api::routes()
        .route("/events", get(sse::events))
        .route("/containers/{id}/console", get(ws::console))
        .route("/containers/{id}/logs/stream", get(ws::logs))
        .route("/stacks/{name}/logs/stream", get(ws::stack_logs));

    Router::new()
        .nest("/api/v1", api)
        .route("/metrics", get(api::system::metrics))
        .fallback(assets::serve)
        .layer(axum::middleware::from_fn_with_state(app.clone(), middleware::authenticate))
        .layer(axum::middleware::from_fn(middleware::security_headers))
        .layer(axum::middleware::from_fn_with_state(app.clone(), middleware::strip_base))
        .with_state(app)
}
