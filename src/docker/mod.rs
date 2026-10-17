//! Docker integration: connection, live inventory, actions, logs, console,
//! stats, and container (re)creation.

pub mod actions;
pub mod apps;
pub mod exec;
pub mod images;
pub mod inventory;
pub mod logs;
pub mod migration;
pub mod model;
pub mod raw;
pub mod reach;
pub mod recreate;
pub mod resources;
pub mod spec;
pub mod stats;

pub use inventory::{DockerService, Entry, Lifecycle};

use std::time::Duration;

use bollard::{API_DEFAULT_VERSION, Docker};

/// Generous client timeout: it bounds how long Docker may take to *start*
/// answering (stopping a container waits for its grace period first);
/// streaming responses are not limited by it.
const CLIENT_TIMEOUT_SECS: u64 = 600;

/// Creates a client for the endpoint without contacting it.
pub fn connect_lazy(host: &str) -> anyhow::Result<Docker> {
    let docker = if let Some(path) = host.strip_prefix("unix://") {
        Docker::connect_with_unix(path, CLIENT_TIMEOUT_SECS, API_DEFAULT_VERSION)?
    } else if host.starts_with("tcp://") || host.starts_with("http://") {
        let addr = host.replacen("tcp://", "http://", 1);
        Docker::connect_with_http(&addr, CLIENT_TIMEOUT_SECS, API_DEFAULT_VERSION)?
    } else if host.starts_with('/') {
        Docker::connect_with_unix(host, CLIENT_TIMEOUT_SECS, API_DEFAULT_VERSION)?
    } else {
        anyhow::bail!("unsupported DOCKER_HOST '{host}' (use unix:///path or tcp://host:port)");
    };
    Ok(docker)
}

/// Connects to the Docker endpoint and negotiates the API version.
pub async fn connect(host: &str) -> anyhow::Result<Docker> {
    let docker = connect_lazy(host)?;
    negotiate(&docker).await?;
    Ok(docker)
}

/// Downgrades the client's API version to what the daemon supports. The
/// version is shared by all clones of the client.
pub async fn negotiate(docker: &Docker) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(10), docker.clone().negotiate_version())
        .await
        .map_err(|_| anyhow::anyhow!("timed out talking to Docker"))??;
    Ok(())
}

/// Finds the id of the container Nodestarr runs in, if any, from mountinfo
/// (works with cgroup v1 and v2) or the hostname.
pub fn detect_self_container_id() -> Option<String> {
    if let Ok(mountinfo) = std::fs::read_to_string("/proc/self/mountinfo") {
        for line in mountinfo.lines() {
            if let Some(idx) = line.find("/containers/") {
                let rest = &line[idx + "/containers/".len()..];
                let id: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
                if id.len() == 64 {
                    return Some(id);
                }
            }
        }
    }
    if let Ok(cgroup) = std::fs::read_to_string("/proc/self/cgroup") {
        for line in cgroup.lines() {
            for part in line.split(['/', '-', '.']) {
                if part.len() == 64 && part.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(part.to_string());
                }
            }
        }
    }
    None
}
