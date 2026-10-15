//! Nodestarr: a lightweight, self-hosted manager for Docker containers and
//! Compose stacks, with Proxmox awareness.
//!
//! The crate is organized by domain. [`app`] wires everything together;
//! [`http`] exposes the REST API and serves the web UI.

pub mod activity;
pub mod app;
pub mod auth;
pub mod backups;
pub mod cli;
pub mod compose;
pub mod config;
pub mod db;
pub mod docker;
pub mod error;
pub mod events;
pub mod files;
pub mod http;
pub mod integrations;
pub mod logging;
pub mod metrics;
pub mod monitor;
pub mod mqtt;
pub mod notify;
pub mod ops;
pub mod prefs;
pub mod proxmox;
pub mod registry;
pub mod scheduler;
pub mod selfupdate;
pub mod settings;
pub mod tasks;
pub mod updates;
pub mod util;

/// Nodestarr's version, from Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// User agent for outbound requests.
pub const USER_AGENT: &str = concat!("Nodestarr/", env!("CARGO_PKG_VERSION"));
