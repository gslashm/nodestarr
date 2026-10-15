//! Container lifecycle calls and small waiting helpers.

use std::time::Duration;

use bollard::Docker;
use bollard::query_parameters::{
    InspectContainerOptions, KillContainerOptions, RemoveContainerOptions, RenameContainerOptions,
    RestartContainerOptions, StopContainerOptions,
};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result, is_not_modified};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Kill,
    Recreate,
    Remove,
}

impl Action {
    pub fn verb(self) -> &'static str {
        match self {
            Action::Start => "start",
            Action::Stop => "stop",
            Action::Restart => "restart",
            Action::Pause => "pause",
            Action::Unpause => "resume",
            Action::Kill => "kill",
            Action::Recreate => "recreate",
            Action::Remove => "remove",
        }
    }

    pub fn past(self) -> &'static str {
        match self {
            Action::Start => "started",
            Action::Stop => "stopped",
            Action::Restart => "restarted",
            Action::Pause => "paused",
            Action::Unpause => "resumed",
            Action::Kill => "killed",
            Action::Recreate => "recreated",
            Action::Remove => "removed",
        }
    }

    /// Actions that interrupt a running app (subject to the Plex guard).
    pub fn is_disruptive(self) -> bool {
        matches!(
            self,
            Action::Stop | Action::Restart | Action::Kill | Action::Recreate | Action::Remove | Action::Pause
        )
    }
}

fn ok_if_not_modified(res: std::result::Result<(), bollard::errors::Error>) -> Result<()> {
    match res {
        Ok(()) => Ok(()),
        Err(e) if is_not_modified(&e) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub async fn start(api: &Docker, id: &str) -> Result<()> {
    ok_if_not_modified(api.start_container(id, None).await)
}

/// Stops with the given grace period; `None` uses the container's own
/// stop timeout (or Docker's default of 10s).
pub async fn stop(api: &Docker, id: &str, timeout: Option<u32>) -> Result<()> {
    ok_if_not_modified(
        api.stop_container(id, Some(StopContainerOptions { t: timeout.map(|t| t as i32), signal: None })).await,
    )
}

pub async fn restart(api: &Docker, id: &str, timeout: Option<u32>) -> Result<()> {
    api.restart_container(id, Some(RestartContainerOptions { t: timeout.map(|t| t as i32), signal: None }))
        .await
        .map_err(Into::into)
}

pub async fn pause(api: &Docker, id: &str) -> Result<()> {
    ok_if_not_modified(api.pause_container(id).await)
}

pub async fn unpause(api: &Docker, id: &str) -> Result<()> {
    ok_if_not_modified(api.unpause_container(id).await)
}

pub async fn kill(api: &Docker, id: &str, signal: &str) -> Result<()> {
    api.kill_container(id, Some(KillContainerOptions { signal: signal.to_string() })).await.map_err(Into::into)
}

pub async fn remove(api: &Docker, id: &str, force: bool, volumes: bool) -> Result<()> {
    api.remove_container(id, Some(RemoveContainerOptions { v: volumes, force, link: false })).await.map_err(Into::into)
}

pub async fn rename(api: &Docker, id: &str, name: &str) -> Result<()> {
    api.rename_container(id, RenameContainerOptions { name: name.to_string() }).await.map_err(Into::into)
}

/// Snapshot of what matters while waiting for a container to settle.
#[derive(Debug, Clone, PartialEq)]
pub struct RunState {
    pub running: bool,
    pub restarting: bool,
    pub health: Option<String>,
    pub restart_count: i64,
    pub exit_code: Option<i64>,
    pub started_at: Option<i64>,
}

pub async fn run_state(api: &Docker, id: &str) -> Result<RunState> {
    let inspect = api.inspect_container(id, None::<InspectContainerOptions>).await?;
    let state = inspect.state.unwrap_or_default();
    let health = state.health.and_then(|h| h.status).and_then(|s| {
        use bollard::models::HealthStatusEnum as H;
        match s {
            H::HEALTHY => Some("healthy".to_string()),
            H::UNHEALTHY => Some("unhealthy".to_string()),
            H::STARTING => Some("starting".to_string()),
            _ => None,
        }
    });
    Ok(RunState {
        running: state.running.unwrap_or(false),
        restarting: state.restarting.unwrap_or(false),
        health,
        restart_count: inspect.restart_count.unwrap_or(0),
        exit_code: state.exit_code,
        started_at: state.started_at.as_deref().and_then(crate::util::parse_rfc3339),
    })
}

/// The container's last few log lines, to explain a failed start.
async fn last_output(api: &Docker, id: &str) -> String {
    let q = crate::docker::logs::LogQuery {
        tail: Some("3".into()),
        since: None,
        until: None,
        timestamps: false,
        stdout: true,
        stderr: true,
    };
    match crate::docker::logs::fetch(api, id, &q).await {
        Ok(lines) if !lines.is_empty() => {
            let text: Vec<String> = lines.iter().map(|l| crate::util::truncate(l.t.trim(), 200)).collect();
            format!("; last output: {}", text.join(" | "))
        }
        _ => String::new(),
    }
}

/// Outcome of [`wait_until_stable`].
#[derive(Debug, Clone, PartialEq)]
pub enum Stability {
    /// Running (and healthy, if it has a health check) for the settle period.
    Stable,
    /// Exited, restarted, or reported unhealthy.
    Failed(String),
    /// Still starting when the timeout expired.
    TimedOut(String),
}

/// Waits until a container has been running for `settle` without restarting,
/// and, if it defines a health check, reports healthy. Used after updates to
/// decide whether to roll back.
pub async fn wait_until_stable(
    api: &Docker,
    id: &str,
    settle: Duration,
    timeout: Duration,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<Stability> {
    let started = tokio::time::Instant::now();
    let initial = run_state(api, id).await?;
    let base_restarts = initial.restart_count;
    let mut running_since: Option<tokio::time::Instant> = initial.running.then(tokio::time::Instant::now);
    loop {
        if cancel.is_cancelled() {
            return Err(AppError::conflict("cancelled"));
        }
        let s = run_state(api, id).await?;
        if s.restart_count > base_restarts || s.restarting {
            let code = s.exit_code.filter(|c| *c != 0).map(|c| format!(" (exit code {c})")).unwrap_or_default();
            return Ok(Stability::Failed(format!(
                "the container keeps restarting{code}{}",
                last_output(api, id).await
            )));
        }
        if !s.running {
            return Ok(Stability::Failed(format!(
                "the container exited with code {}{}",
                s.exit_code.unwrap_or_default(),
                last_output(api, id).await
            )));
        }
        if s.health.as_deref() == Some("unhealthy") {
            return Ok(Stability::Failed("health check reports unhealthy".into()));
        }
        let since = *running_since.get_or_insert_with(tokio::time::Instant::now);
        let healthy_enough = matches!(s.health.as_deref(), None | Some("healthy"));
        if healthy_enough && since.elapsed() >= settle {
            return Ok(Stability::Stable);
        }
        if started.elapsed() >= timeout {
            return Ok(Stability::TimedOut(match s.health.as_deref() {
                Some("starting") => "health check still starting".into(),
                _ => "did not settle in time".into(),
            }));
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
