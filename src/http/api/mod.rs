//! REST API routes, grouped by domain.

pub mod auth;
pub mod backups;
pub mod containers;
pub mod integrations;
pub mod monitoring;
pub mod proxmox;
pub mod stacks;
pub mod system;
pub mod updates;
mod views;

use axum::Router;

use crate::app::App;

pub fn routes() -> Router<App> {
    Router::new()
        .merge(auth::routes())
        .merge(containers::routes())
        .merge(stacks::routes())
        .merge(updates::routes())
        .merge(backups::routes())
        .merge(system::routes())
        .merge(monitoring::routes())
        .merge(integrations::routes())
        .merge(proxmox::routes())
}
