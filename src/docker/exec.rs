//! Running commands inside containers (non-interactive).

use std::path::Path;

use bollard::Docker;
use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecResults};
use futures_util::StreamExt;
use serde::Serialize;
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Serialize)]
pub struct ExecOutput {
    pub exit_code: Option<i64>,
    pub stdout: String,
    pub stderr: String,
}

async fn create(api: &Docker, container: &str, cmd: Vec<String>, user: Option<&str>, stdin: bool) -> Result<String> {
    let created = api
        .create_exec(
            container,
            CreateExecOptions {
                cmd: Some(cmd),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                attach_stdin: Some(stdin),
                tty: Some(false),
                user: user.map(str::to_string),
                ..Default::default()
            },
        )
        .await?;
    Ok(created.id)
}

async fn exit_code(api: &Docker, exec_id: &str) -> Option<i64> {
    api.inspect_exec(exec_id).await.ok().and_then(|i| i.exit_code)
}

const MAX_CAPTURE: usize = 1024 * 1024;

/// Runs a command and captures its output (each stream capped at 1 MiB).
pub async fn capture(api: &Docker, container: &str, cmd: Vec<String>, user: Option<&str>) -> Result<ExecOutput> {
    let id = create(api, container, cmd, user, false).await?;
    let mut out = ExecOutput {
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
    };
    if let StartExecResults::Attached { mut output, .. } = api.start_exec(&id, None).await? {
        while let Some(chunk) = output.next().await {
            match chunk? {
                LogOutput::StdOut { message } | LogOutput::Console { message } if out.stdout.len() < MAX_CAPTURE => {
                    out.stdout.push_str(&String::from_utf8_lossy(&message));
                }
                LogOutput::StdErr { message } if out.stderr.len() < MAX_CAPTURE => {
                    out.stderr.push_str(&String::from_utf8_lossy(&message));
                }
                _ => {}
            }
        }
    }
    out.exit_code = exit_code(api, &id).await;
    Ok(out)
}

/// Runs a command, writing its stdout to `dest`. Returns bytes written.
pub async fn to_file(api: &Docker, container: &str, cmd: Vec<String>, dest: &Path) -> Result<u64> {
    let id = create(api, container, cmd, None, false).await?;
    let mut file = tokio::io::BufWriter::new(tokio::fs::File::create(dest).await?);
    let mut written = 0u64;
    let mut stderr = String::new();
    if let StartExecResults::Attached { mut output, .. } = api.start_exec(&id, None).await? {
        while let Some(chunk) = output.next().await {
            match chunk? {
                LogOutput::StdOut { message } | LogOutput::Console { message } => {
                    file.write_all(&message).await?;
                    written += message.len() as u64;
                }
                LogOutput::StdErr { message } if stderr.len() < 16 * 1024 => {
                    stderr.push_str(&String::from_utf8_lossy(&message));
                }
                _ => {}
            }
        }
    }
    file.flush().await?;
    match exit_code(api, &id).await {
        Some(0) => Ok(written),
        code => Err(AppError::conflict(format!(
            "command exited with {}: {}",
            code.map(|c| c.to_string()).unwrap_or_else(|| "unknown status".into()),
            last_lines(&stderr, 8)
        ))),
    }
}

/// Runs a command with `src` piped to its stdin.
pub async fn from_file(api: &Docker, container: &str, cmd: Vec<String>, src: &Path) -> Result<String> {
    let id = create(api, container, cmd, None, true).await?;
    let mut stderr = String::new();
    if let StartExecResults::Attached { mut output, mut input } = api.start_exec(&id, None).await? {
        let src = src.to_path_buf();
        let writer = async move {
            let mut f = tokio::fs::File::open(&src).await?;
            tokio::io::copy(&mut f, &mut input).await?;
            input.shutdown().await?;
            Ok::<(), std::io::Error>(())
        };
        let reader = async {
            while let Some(chunk) = output.next().await {
                if let Ok(LogOutput::StdErr { message } | LogOutput::StdOut { message }) = chunk {
                    if stderr.len() < 64 * 1024 {
                        stderr.push_str(&String::from_utf8_lossy(&message));
                    }
                }
            }
        };
        let (w, ()) = tokio::join!(writer, reader);
        w?;
    }
    match exit_code(api, &id).await {
        Some(0) | None => Ok(stderr),
        Some(code) => Err(AppError::conflict(format!("command exited with {code}: {}", last_lines(&stderr, 8)))),
    }
}

fn last_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Tries shells in order and returns the first that exists in the container.
pub async fn find_shell(api: &Docker, container: &str, candidates: &[String]) -> Option<String> {
    for shell in candidates {
        if let Ok(out) = capture(api, container, vec![shell.clone(), "-c".into(), "exit 0".into()], None).await {
            if out.exit_code == Some(0) {
                return Some(shell.clone());
            }
        }
    }
    None
}
