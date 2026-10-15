//! Bootstrap configuration.
//!
//! Only the handful of values needed before the database is open live here
//! (where to listen, where data lives, how to reach Docker). Everything else is
//! a runtime setting stored in the database and editable from the UI, see
//! [`crate::settings`].

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// Default port. Sits next to the *arr family (7878, 8989, 9696, ...).
pub const DEFAULT_PORT: u16 = 7979;

#[derive(Debug, Parser)]
#[command(name = "nodestarr", version = crate::VERSION, about = "Lightweight Docker and Compose manager")]
pub struct Cli {
    #[command(flatten)]
    pub boot: BootConfig,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Args)]
pub struct BootConfig {
    /// Directory for the database and other persistent state.
    #[arg(long, env = "NODESTARR_DATA_DIR", default_value = "/config", global = true)]
    pub data_dir: PathBuf,

    /// Address and port to listen on.
    #[arg(long, env = "NODESTARR_LISTEN", default_value_t = SocketAddr::from(([0, 0, 0, 0], DEFAULT_PORT)), global = true)]
    pub listen: SocketAddr,

    /// Docker endpoint, e.g. unix:///var/run/docker.sock or tcp://host:2375.
    #[arg(long, env = "DOCKER_HOST", default_value = "unix:///var/run/docker.sock", global = true)]
    pub docker_host: String,

    /// Log filter, e.g. "info" or "nodestarr=debug,info".
    #[arg(long, env = "NODESTARR_LOG", default_value = "info", global = true)]
    pub log: String,

    /// Number of async worker threads.
    #[arg(long, env = "NODESTARR_WORKERS", default_value_t = 2, global = true)]
    pub workers: usize,

    /// Create this admin user on first start (skips the setup screen).
    #[arg(long, env = "NODESTARR_ADMIN_USER", global = true, hide = true)]
    pub admin_user: Option<String>,

    /// Password for NODESTARR_ADMIN_USER.
    #[arg(long, env = "NODESTARR_ADMIN_PASSWORD", global = true, hide = true, hide_env_values = true)]
    pub admin_password: Option<String>,

    /// Overrides the URL base setting (useful if a bad value locked you out).
    #[arg(long, env = "NODESTARR_URL_BASE", global = true, hide = true)]
    pub url_base: Option<String>,

    /// Directory to serve the web UI from instead of the embedded copy
    /// (development only).
    #[arg(long, env = "NODESTARR_WEB_DIR", global = true, hide = true)]
    pub web_dir: Option<PathBuf>,
}

impl BootConfig {
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("nodestarr.db")
    }
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Run the web server (default).
    Serve,
    /// Reset a user's password. Prints a generated password if none is given.
    ResetPassword {
        #[arg(long)]
        username: String,
        #[arg(long)]
        password: Option<String>,
    },
    /// Turn off two-factor authentication for a user.
    #[command(name = "disable-2fa")]
    Disable2fa {
        #[arg(long)]
        username: String,
    },
    /// Exit 0 if the local server answers its health endpoint (for Docker HEALTHCHECK).
    Healthcheck,
    /// Internal: recreate the Nodestarr container with a new image.
    #[command(hide = true)]
    SelfUpdateHelper {
        #[arg(long)]
        container: String,
        #[arg(long)]
        image: String,
    },
    /// Print the version.
    Version,
}
