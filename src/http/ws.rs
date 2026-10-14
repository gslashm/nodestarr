//! WebSockets: interactive console (docker exec with a TTY) and live logs.

use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, header};
use axum::response::Response;
use bollard::container::LogOutput;
use bollard::exec::{CreateExecOptions, StartExecResults};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio::io::AsyncWriteExt;

use crate::activity::NewActivity;
use crate::app::App;
use crate::docker::logs::{LogLine, LogQuery};
use crate::error::{AppError, Result};
use crate::http::Auth;
use crate::settings::Severity;

/// Rejects cross-site WebSocket connections.
fn check_origin(headers: &HeaderMap) -> Result<()> {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return Ok(()); // non-browser clients (scripts with an API key)
    };
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let origin_host = origin.split("://").nth(1).unwrap_or(origin);
    if origin_host.eq_ignore_ascii_case(host) {
        Ok(())
    } else {
        Err(AppError::forbidden("cross-origin WebSocket refused"))
    }
}

#[derive(Debug, Deserialize)]
pub struct ConsoleQuery {
    pub shell: Option<String>,
    pub user: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMsg {
    Input { data: String },
    Resize { cols: u16, rows: u16 },
}

pub async fn console(
    State(app): State<App>,
    Auth(p): Auth,
    Path(id): Path<String>,
    Query(q): Query<ConsoleQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response> {
    p.require_admin()?;
    check_origin(&headers)?;
    let entry = app.docker.get(&id).ok_or_else(|| AppError::not_found("container"))?;
    if !entry.summary.is_running() {
        return Err(AppError::conflict("the container is not running"));
    }
    let name = crate::updates::check::display_name(&app, &entry);
    app.note(
        NewActivity::new("container.console", Severity::Info, format!("Console opened on {name}"))
            .target(&entry.summary.identity, &name)
            .actor(&p.actor()),
    )
    .await;
    let container = entry.summary.id.clone();
    Ok(ws.on_upgrade(move |socket| async move {
        if let Err(err) = console_session(app, container, q, socket).await {
            tracing::debug!(%err, "console session ended with an error");
        }
    }))
}

async fn console_session(app: App, container: String, q: ConsoleQuery, socket: WebSocket) -> Result<()> {
    let (mut tx, mut rx) = socket.split();
    let shells = app.settings.get().docker.console_shells.clone();
    let shell = match q.shell.filter(|s| !s.is_empty()) {
        Some(s) => Some(s),
        None => crate::docker::exec::find_shell(&app.docker.api, &container, &shells).await,
    };
    let Some(shell) = shell else {
        let _ = tx
            .send(Message::Text(json!({"type": "error", "message": "No shell found in this container (it may be a minimal image without one)."}).to_string().into()))
            .await;
        return Ok(());
    };
    let created = app
        .docker
        .api
        .create_exec(
            &container,
            CreateExecOptions {
                cmd: Some(vec![shell.clone()]),
                attach_stdin: Some(true),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                tty: Some(true),
                user: q.user.clone().filter(|u| !u.is_empty()),
                env: Some(vec!["TERM=xterm-256color".to_string(), "COLORTERM=truecolor".to_string()]),
                ..Default::default()
            },
        )
        .await?;
    let exec_id = created.id;
    let StartExecResults::Attached { mut output, mut input } = app.docker.api.start_exec(&exec_id, None).await? else {
        return Err(AppError::conflict("exec did not attach"));
    };
    let _ = tx
        .send(Message::Text(json!({"type": "ready", "shell": shell}).to_string().into()))
        .await;
    if let (Some(cols), Some(rows)) = (q.cols, q.rows) {
        let _ = app
            .docker
            .api
            .resize_exec(&exec_id, bollard::exec::ResizeExecOptions { height: rows, width: cols })
            .await;
    }
    loop {
        tokio::select! {
            out = output.next() => match out {
                Some(Ok(LogOutput::Console { message } | LogOutput::StdOut { message } | LogOutput::StdErr { message })) => {
                    if tx.send(Message::Binary(message)).await.is_err() {
                        break;
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(_)) | None => break,
            },
            msg = rx.next() => match msg {
                Some(Ok(Message::Binary(data))) => {
                    if input.write_all(&data).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Text(text))) => match serde_json::from_str::<ClientMsg>(&text) {
                    Ok(ClientMsg::Input { data }) => {
                        if input.write_all(data.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                    Ok(ClientMsg::Resize { cols, rows }) => {
                        let _ = app
                            .docker
                            .api
                            .resize_exec(&exec_id, bollard::exec::ResizeExecOptions { height: rows.max(1), width: cols.max(1) })
                            .await;
                    }
                    Err(_) => {}
                },
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            },
        }
    }
    let code = app.docker.api.inspect_exec(&exec_id).await.ok().and_then(|i| i.exit_code);
    let _ = tx.send(Message::Text(json!({"type": "exit", "code": code}).to_string().into())).await;
    let _ = tx.close().await;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct LogsWsQuery {
    pub tail: Option<String>,
    pub since: Option<i64>,
    #[serde(deserialize_with = "crate::util::flag", default = "yes")]
    pub timestamps: bool,
}

fn yes() -> bool {
    true
}

pub async fn logs(
    State(app): State<App>,
    Auth(_p): Auth,
    Path(id): Path<String>,
    Query(q): Query<LogsWsQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response> {
    check_origin(&headers)?;
    let entry = app.docker.get(&id).ok_or_else(|| AppError::not_found("container"))?;
    let query = LogQuery {
        tail: q.tail.or(Some("500".into())),
        since: q.since,
        until: None,
        timestamps: q.timestamps,
        stdout: true,
        stderr: true,
    };
    let stream = crate::docker::logs::follow(&app.docker.api, &entry.summary.id, &query);
    Ok(ws.on_upgrade(move |socket| pump_logs(socket, Box::pin(stream.map(|r| r.map(|lines| (None, lines)))))))
}

type TaggedStream = std::pin::Pin<Box<dyn futures_util::Stream<Item = std::result::Result<(Option<String>, Vec<LogLine>), String>> + Send>>;

/// Sends log batches to the socket, coalescing bursts into one frame per 100 ms.
async fn pump_logs(socket: WebSocket, mut stream: TaggedStream) {
    let (mut tx, mut rx) = socket.split();
    let mut pending: Vec<serde_json::Value> = Vec::new();
    let mut flush = tokio::time::interval(Duration::from_millis(100));
    loop {
        tokio::select! {
            item = stream.next() => match item {
                Some(Ok((source, lines))) => {
                    for l in lines {
                        let mut v = serde_json::to_value(&l).unwrap_or_default();
                        if let Some(src) = &source {
                            v["c"] = json!(src);
                        }
                        pending.push(v);
                    }
                    if pending.len() >= 1000 && send_batch(&mut tx, &mut pending).await.is_err() {
                        return;
                    }
                }
                Some(Err(err)) => {
                    let _ = send_batch(&mut tx, &mut pending).await;
                    let _ = tx.send(Message::Text(json!({"error": err}).to_string().into())).await;
                    return;
                }
                None => {
                    let _ = send_batch(&mut tx, &mut pending).await;
                    let _ = tx.send(Message::Text(json!({"end": true}).to_string().into())).await;
                    return;
                }
            },
            _ = flush.tick() => {
                if !pending.is_empty() && send_batch(&mut tx, &mut pending).await.is_err() {
                    return;
                }
            }
            msg = rx.next() => match msg {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                _ => {}
            },
        }
    }
}

async fn send_batch(
    tx: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    pending: &mut Vec<serde_json::Value>,
) -> std::result::Result<(), axum::Error> {
    if pending.is_empty() {
        return Ok(());
    }
    let batch = std::mem::take(pending);
    tx.send(Message::Text(json!({ "lines": batch }).to_string().into())).await
}

pub async fn stack_logs(
    State(app): State<App>,
    Auth(_p): Auth,
    Path(name): Path<String>,
    Query(q): Query<LogsWsQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response> {
    check_origin(&headers)?;
    let entries = app.docker.in_stack(&name);
    if entries.is_empty() {
        return Err(AppError::not_found(format!("containers of stack '{name}'")));
    }
    let query = LogQuery {
        tail: q.tail.or(Some("200".into())),
        since: q.since,
        until: None,
        timestamps: true,
        stdout: true,
        stderr: true,
    };
    let streams: Vec<TaggedStream> = entries
        .iter()
        .map(|e| {
            let label = e.summary.service.clone().unwrap_or_else(|| e.summary.name.clone());
            let s = crate::docker::logs::follow(&app.docker.api, &e.summary.id, &query).map(move |r| r.map(|lines| (Some(label.clone()), lines)));
            Box::pin(s) as TaggedStream
        })
        .collect();
    let merged: TaggedStream = Box::pin(futures_util::stream::select_all(streams));
    Ok(ws.on_upgrade(move |socket| pump_logs(socket, merged)))
}
