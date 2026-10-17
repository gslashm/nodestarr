//! Container endpoints: list, detail, actions, logs, exec, stats,
//! preferences, create/edit, config files, per-container updates and backups.

use std::collections::HashMap;
use std::path::Path as FsPath;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use bollard::query_parameters::{CreateContainerOptions, InspectContainerOptions};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::activity::NewActivity;
use crate::app::App;
use crate::docker::actions::Action;
use crate::docker::spec::{ContainerSpec, parse_docker_run};
use crate::error::{AppError, Result};
use crate::http::Auth;
use crate::http::api::views::{ViewContext, container_view, name_matches};
use crate::prefs::ContainerPrefs;
use crate::registry::ImageRef;
use crate::settings::{Severity, mask_secrets, merge_secrets};
use crate::tasks::TaskSpec;
use crate::updates::apply::{Trigger, UpdateRequest};
use crate::updates::check::display_name;

pub fn routes() -> Router<App> {
    Router::new()
        .route("/containers", get(list).post(create))
        .route("/containers/parse-run", post(parse_run))
        .route("/containers/to-compose", post(to_compose))
        .route("/containers/{id}", get(detail))
        .route("/containers/{id}/inspect", get(inspect))
        .route("/containers/{id}/actions/{action}", post(action))
        .route("/containers/{id}/logs", get(logs))
        .route("/containers/{id}/exec", post(exec))
        .route("/containers/{id}/stats", get(stats))
        .route("/containers/{id}/prefs", get(get_prefs).put(put_prefs))
        .route("/containers/{id}/spec", get(get_spec).put(put_spec))
        .route("/containers/{id}/files/roots", get(file_roots))
        .route("/containers/{id}/files", get(list_files))
        .route("/containers/{id}/files/content", get(read_file).put(save_file))
        .route("/containers/{id}/files/versions", get(file_versions))
        .route("/containers/{id}/files/restore", post(restore_file_version))
        .route("/files/versions/{vid}", get(file_version_content))
        .route("/containers/{id}/check", post(check))
        .route("/containers/{id}/update", post(update))
        .route("/containers/{id}/rollback", post(rollback))
        .route("/containers/{id}/tags", get(tags))
        .route("/containers/{id}/tag-preview", get(tag_preview))
        .route("/containers/{id}/releases", get(releases))
        .route("/containers/{id}/update-history", get(update_history))
        .route("/containers/{id}/skip", post(skip_version))
        .route("/containers/{id}/hold", put(hold))
        .route("/containers/{id}/dismiss", put(dismiss))
        .route("/containers/{id}/backups", get(backups).post(backup_now))
        .route("/containers/{id}/backup-plan", get(backup_plan))
        .route("/containers/{id}/app/refresh", post(app_refresh))
}

fn entry(app: &App, id: &str) -> Result<std::sync::Arc<crate::docker::Entry>> {
    app.docker.get(id).ok_or_else(|| AppError::not_found(format!("container '{id}'")))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(deserialize_with = "crate::util::flag", default)]
    all: bool,
}

async fn list(State(app): State<App>, Query(q): Query<ListQuery>) -> Result<Json<Value>> {
    let ctx = ViewContext::load(&app).await;
    let hidden_patterns = app.settings.get().docker.hidden_containers.clone();
    let views: Vec<_> = app
        .docker
        .list()
        .into_iter()
        .filter(|s| q.all || !name_matches(&hidden_patterns, &s.name))
        .filter(|s| !s.name.starts_with("nodestarr-updater-") && !s.name.contains("-nodestarr-old-"))
        .map(|s| container_view(&app, &ctx, s))
        .filter(|v| q.all || !v.prefs.hidden)
        .collect();
    Ok(Json(json!({
        "containers": views,
        "docker_connected": app.docker.is_connected(),
        "checking_updates": app.updates.is_checking(),
    })))
}

/// Env vars whose names look secret are masked in the detail view.
fn mask_env(env: &[String]) -> Vec<Value> {
    const SECRET_HINTS: &[&str] = &["PASS", "SECRET", "TOKEN", "KEY", "CREDENTIAL", "PRIVATE"];
    env.iter()
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv.as_str(), ""));
            let secret = SECRET_HINTS.iter().any(|h| k.to_ascii_uppercase().contains(h));
            json!({ "key": k, "value": if secret && !v.is_empty() { "••••••••" } else { v }, "secret": secret })
        })
        .collect()
}

async fn detail(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    let ctx = ViewContext::load(&app).await;
    let view = container_view(&app, &ctx, e.summary.clone());
    let i = &e.inspect;
    let cfg = i.config.clone().unwrap_or_default();
    let host = i.host_config.clone().unwrap_or_default();
    let image = app.docker.image_meta(&e.summary.image_id).await;
    let networks: Vec<Value> = i
        .network_settings
        .as_ref()
        .and_then(|n| n.networks.as_ref())
        .map(|m| {
            m.iter()
                .map(|(name, ep)| json!({ "name": name, "ip": ep.ip_address, "gateway": ep.gateway, "mac": ep.mac_address, "aliases": ep.aliases }))
                .collect()
        })
        .unwrap_or_default();
    let health_log: Vec<Value> = i
        .state
        .as_ref()
        .and_then(|s| s.health.as_ref())
        .and_then(|h| h.log.as_ref())
        .map(|l| {
            l.iter()
                .rev()
                .take(5)
                .map(|r| json!({ "exit_code": r.exit_code, "output": r.output.as_deref().map(|o| crate::util::truncate(o.trim(), 500)), "end": r.end.as_ref().map(|d| d.to_string()) }))
                .collect()
        })
        .unwrap_or_default();
    let mut labels: Vec<(String, String)> = cfg.labels.clone().unwrap_or_default().into_iter().collect();
    labels.sort();
    let prefs = app.prefs.get(&e.summary.identity);
    let uptime = app.uptime.get(&e.summary.identity);
    Ok(Json(json!({
        "container": view,
        "command": cfg.cmd,
        "entrypoint": cfg.entrypoint,
        "env": mask_env(&cfg.env.unwrap_or_default()),
        "labels": labels.into_iter().map(|(k, v)| json!({ "key": k, "value": v })).collect::<Vec<_>>(),
        "networks": networks,
        "hostname": cfg.hostname,
        "user": cfg.user,
        "working_dir": cfg.working_dir,
        "privileged": host.privileged.unwrap_or(false),
        "memory_limit": host.memory.filter(|m| *m > 0),
        "cpu_limit": host.nano_cpus.filter(|n| *n > 0).map(|n| n as f64 / 1e9),
        "devices": host.devices.unwrap_or_default().iter().map(|d| json!({ "host": d.path_on_host, "container": d.path_in_container })).collect::<Vec<_>>(),
        "health_log": health_log,
        "image": image.as_ref().map(|m| json!({ "id": m.id, "created": m.created, "size": m.size, "tags": m.repo_tags, "digests": m.repo_digests, "source": m.source_url(), "arch": m.arch, "os": m.os })),
        "notes": prefs.notes,
        "uptime": uptime,
        "can_edit_spec": e.summary.stack.is_none() && !e.summary.is_self,
    })))
}

async fn inspect(State(app): State<App>, Auth(p): Auth, Path(id): Path<String>) -> Result<Json<Value>> {
    p.require_admin()?;
    let e = entry(&app, &id)?;
    let raw = app.docker.api.inspect_container(&e.summary.id, None::<InspectContainerOptions>).await?;
    Ok(Json(serde_json::to_value(raw)?))
}

async fn action(
    State(app): State<App>,
    Auth(p): Auth,
    Path((id, action)): Path<(String, String)>,
    body: Option<Json<crate::ops::ActionOptions>>,
) -> Result<Json<Value>> {
    let action: Action = serde_json::from_value(Value::String(action.clone()))
        .map_err(|_| AppError::bad_request(format!("unknown action '{action}'")))?;
    let opts = body.map(|Json(b)| b).unwrap_or_default();
    let task = crate::ops::container_action(&app, &id, action, opts, &p.actor()).await?;
    Ok(Json(json!({ "ok": true, "task_id": task })))
}

#[derive(Deserialize)]
struct LogsDownload {
    #[serde(flatten)]
    query: crate::docker::logs::LogQuery,
    #[serde(default)]
    format: Option<String>,
}

async fn logs(State(app): State<App>, Path(id): Path<String>, Query(q): Query<LogsDownload>) -> Result<Response> {
    let e = entry(&app, &id)?;
    let lines = crate::docker::logs::fetch(&app.docker.api, &e.summary.id, &q.query).await?;
    if q.format.as_deref() == Some("text") {
        let text = crate::docker::logs::to_text(&lines, q.query.timestamps);
        let mut resp = text.into_response();
        let fname = format!("{}-{}.log", e.summary.name, chrono::Utc::now().format("%Y%m%d-%H%M%S"));
        resp.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
        if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{fname}\"")) {
            resp.headers_mut().insert(header::CONTENT_DISPOSITION, v);
        }
        return Ok(resp);
    }
    Ok(Json(json!({ "lines": lines })).into_response())
}

#[derive(Deserialize)]
struct ExecBody {
    command: String,
    #[serde(default)]
    user: Option<String>,
}

async fn exec(
    State(app): State<App>,
    Auth(p): Auth,
    Path(id): Path<String>,
    Json(b): Json<ExecBody>,
) -> Result<Json<Value>> {
    p.require_admin()?;
    let e = entry(&app, &id)?;
    if b.command.trim().is_empty() {
        return Err(AppError::bad_request("enter a command"));
    }
    let name = display_name(&app, &e);
    app.note(
        NewActivity::new(
            "container.exec",
            Severity::Info,
            format!("Ran `{}` in {name}", crate::util::truncate(&b.command, 120)),
        )
        .target(&e.summary.identity, &name)
        .actor(&p.actor()),
    )
    .await;
    let cmd = vec!["sh".to_string(), "-c".to_string(), b.command.clone()];
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        crate::docker::exec::capture(&app.docker.api, &e.summary.id, cmd, b.user.as_deref().filter(|u| !u.is_empty())),
    )
    .await
    .map_err(|_| {
        AppError::conflict("the command took longer than 2 minutes; use the console for long-running commands")
    })??;
    Ok(Json(serde_json::to_value(out)?))
}

#[derive(Deserialize)]
struct RangeQuery {
    #[serde(default)]
    range: Option<String>,
}

pub fn range_secs(r: Option<&str>) -> i64 {
    match r.unwrap_or("1h") {
        "15m" => 900,
        "6h" => 6 * 3600,
        "24h" | "1d" => 86400,
        "7d" => 7 * 86400,
        "30d" => 30 * 86400,
        _ => 3600,
    }
}

async fn stats(State(app): State<App>, Path(id): Path<String>, Query(q): Query<RangeQuery>) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    let to = crate::util::now();
    let from = to - range_secs(q.range.as_deref());
    let series = crate::monitor::stats::history(&app, &e.summary.identity, from, to, 720).await?;
    Ok(Json(json!({ "series": series, "latest": crate::monitor::stats::latest(&app).get(&e.summary.identity) })))
}

async fn get_prefs(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    let mut v = serde_json::to_value(&*app.prefs.get(&e.summary.identity))?;
    mask_secrets(&mut v);
    Ok(Json(v))
}

async fn put_prefs(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(mut incoming): Json<Value>,
) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    let identity = e.summary.identity.clone();
    let current = serde_json::to_value(&*app.prefs.get(&identity))?;
    merge_secrets(&mut incoming, &current);
    let prefs: ContainerPrefs =
        serde_json::from_value(incoming).map_err(|e| AppError::bad_request(format!("invalid preferences: {e}")))?;
    for g in &prefs.backup.exclude {
        globset::Glob::new(g).map_err(|e| AppError::bad_request(format!("invalid exclude '{g}': {e}")))?;
    }
    crate::settings::parse_status_ranges(&prefs.uptime.expect_status).map_err(AppError::BadRequest)?;
    let saved = app.prefs.set(&identity, prefs).await?;
    app.integrations.forget_creds(&identity);
    app.bus.publish(crate::events::Event::Prefs { identity: identity.clone() });
    // Policy changes can change the displayed update state.
    let app2 = app.clone();
    let e2 = e.clone();
    tokio::spawn(async move {
        let info = crate::updates::check::check_one(&app2, &e2, false).await;
        app2.updates.set(&e2.summary.identity, info).await;
    });
    let mut v = serde_json::to_value(&*saved)?;
    mask_secrets(&mut v);
    Ok(Json(v))
}

/// The full spec includes unmasked environment values, so only admins can read it.
async fn get_spec(State(app): State<App>, Auth(p): Auth, Path(id): Path<String>) -> Result<Json<Value>> {
    p.require_admin()?;
    let e = entry(&app, &id)?;
    let cfg = app.docker.image_meta(&e.summary.image_id).await.and_then(|m| m.config.clone());
    let spec = ContainerSpec::from_inspect(&e.inspect, cfg.as_ref());
    Ok(Json(json!({ "spec": spec, "compose": spec.to_compose(), "standalone": e.summary.stack.is_none() })))
}

/// Applies an edited spec to a standalone container by recreating it.
async fn put_spec(
    State(app): State<App>,
    Auth(p): Auth,
    Path(id): Path<String>,
    Json(spec): Json<ContainerSpec>,
) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    if e.summary.stack.is_some() {
        return Err(AppError::bad_request("this container belongs to a compose stack; edit the compose file instead"));
    }
    if e.summary.is_self {
        return Err(AppError::forbidden("Nodestarr can't edit its own container"));
    }
    let (body, extra_networks) = spec.to_create_body()?;
    let name = display_name(&app, &e);
    let identity = e.summary.identity.clone();
    let task_spec = TaskSpec::new("recreate", format!("Apply changes to {name}"), &p.actor())
        .target(identity.clone(), name.clone());
    let (app2, actor, cid) = (app.clone(), p.actor(), e.summary.id.clone());
    let task = app.tasks.spawn(task_spec, move |ctx| async move {
        let target = ImageRef::parse(body.image.as_deref().unwrap_or_default())
            .ok_or_else(|| AppError::bad_request("invalid image"))?;
        if app2.docker.inspect_image_fresh(&target.familiar()).await.is_none() {
            crate::updates::apply::pull(&app2, &ctx, &target).await?;
        }
        let rec = crate::docker::recreate::replace(
            &app2.docker.api,
            &cid,
            body,
            crate::updates::apply::stop_timeout(&app2),
            &|l| ctx.log(l),
        )
        .await?;
        for net in &extra_networks {
            let _ = app2
                .docker
                .api
                .connect_network(
                    net,
                    bollard::models::NetworkConnectRequest { container: rec.new_id.clone(), endpoint_config: None },
                )
                .await;
        }
        match crate::docker::actions::wait_until_stable(
            &app2.docker.api,
            &rec.new_id,
            std::time::Duration::from_secs(5),
            std::time::Duration::from_secs(60),
            &ctx.cancel_token(),
            None,
        )
        .await?
        {
            crate::docker::actions::Stability::Stable => {
                crate::docker::recreate::finalize(&app2.docker.api, &rec).await?;
            }
            crate::docker::actions::Stability::Failed(msg) | crate::docker::actions::Stability::TimedOut(msg) => {
                if rec.was_running {
                    crate::docker::recreate::revert(&app2.docker.api, &rec, &|l| ctx.log(l)).await?;
                    return Err(AppError::conflict(format!(
                        "the changed container didn't stay up ({msg}); the previous one was restored"
                    )));
                }
                crate::docker::recreate::finalize(&app2.docker.api, &rec).await?;
            }
        }
        app2.note(
            NewActivity::new("container.edited", Severity::Info, format!("{name} recreated with new settings"))
                .target(&identity, &name)
                .actor(&actor),
        )
        .await;
        Ok(json!({ "id": rec.new_id }))
    });
    Ok(Json(json!({ "task_id": task })))
}

#[derive(Deserialize)]
struct CreateBody {
    spec: ContainerSpec,
    #[serde(default = "yes")]
    start: bool,
}

fn yes() -> bool {
    true
}

async fn create(State(app): State<App>, Auth(p): Auth, Json(b): Json<CreateBody>) -> Result<Json<Value>> {
    let (body, extra_networks) = b.spec.to_create_body()?;
    let name = b.spec.name.clone();
    if !name.is_empty() && app.docker.get(&name).is_some() {
        return Err(AppError::conflict(format!("a container named '{name}' already exists")));
    }
    let label = if name.is_empty() { b.spec.image.clone() } else { name.clone() };
    let task_spec = TaskSpec::new("create", format!("Create {label}"), &p.actor()).lock(format!("create:{label}"));
    let (app2, actor, start) = (app.clone(), p.actor(), b.start);
    let task = app.tasks.spawn(task_spec, move |ctx| async move {
        let image = body.image.clone().unwrap_or_default();
        let target = ImageRef::parse(&image).ok_or_else(|| AppError::bad_request("invalid image"))?;
        // Like `docker run`: pull only when the image isn't here yet, so
        // locally built images work and existing ones aren't changed.
        if app2.docker.inspect_image_fresh(&image).await.is_some() {
            ctx.log(format!("Using {} from this machine", target.familiar()));
        } else {
            ctx.log(format!("Pulling {}", target.familiar()));
            crate::updates::apply::pull(&app2, &ctx, &target).await?;
        }
        let resp = app2
            .docker
            .api
            .create_container(
                Some(CreateContainerOptions {
                    name: if name.is_empty() { None } else { Some(name.clone()) },
                    platform: String::new(),
                }),
                body,
            )
            .await?;
        for w in &resp.warnings {
            ctx.log(format!("Docker warning: {w}"));
        }
        for net in &extra_networks {
            app2.docker
                .api
                .connect_network(
                    net,
                    bollard::models::NetworkConnectRequest { container: resp.id.clone(), endpoint_config: None },
                )
                .await?;
        }
        if start {
            ctx.log("Starting");
            crate::docker::actions::start(&app2.docker.api, &resp.id).await?;
        }
        app2.docker.refresh(&resp.id).await;
        app2.note(
            NewActivity::new("container.created", Severity::Success, format!("Container {label} created"))
                .actor(&actor),
        )
        .await;
        Ok(json!({ "id": resp.id }))
    });
    Ok(Json(json!({ "task_id": task })))
}

#[derive(Deserialize)]
struct ParseRunBody {
    command: String,
}

async fn parse_run(Json(b): Json<ParseRunBody>) -> Result<Json<Value>> {
    let parsed = parse_docker_run(&b.command)?;
    Ok(Json(serde_json::to_value(parsed)?))
}

async fn to_compose(Json(spec): Json<ContainerSpec>) -> Result<Json<Value>> {
    spec.validate()?;
    Ok(Json(json!({ "yaml": spec.to_compose() })))
}

// --- Config files ---------------------------------------------------------

async fn file_roots(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    let roots = crate::files::roots(&e, &app.settings.get().backups.data_roots);
    Ok(Json(serde_json::to_value(roots)?))
}

#[derive(Deserialize)]
struct FileQuery {
    root: String,
    #[serde(default)]
    path: String,
}

async fn list_files(
    State(app): State<App>,
    Auth(p): Auth,
    Path(id): Path<String>,
    Query(q): Query<FileQuery>,
) -> Result<Json<Value>> {
    p.require_admin()?;
    let e = entry(&app, &id)?;
    Ok(Json(serde_json::to_value(crate::files::list_dir(&e, &q.root, &q.path).await?)?))
}

async fn read_file(
    State(app): State<App>,
    Auth(p): Auth,
    Path(id): Path<String>,
    Query(q): Query<FileQuery>,
) -> Result<Json<Value>> {
    p.require_admin()?;
    let e = entry(&app, &id)?;
    Ok(Json(serde_json::to_value(crate::files::read_file(&e, &q.root, &q.path).await?)?))
}

#[derive(Deserialize)]
struct SaveFileBody {
    #[serde(flatten)]
    file: crate::files::SaveFile,
    /// "none", "restart" or "stop_start" (stop, write, start: for apps that
    /// rewrite their config on shutdown).
    #[serde(default)]
    apply: Option<String>,
}

async fn save_file(
    State(app): State<App>,
    Auth(p): Auth,
    Path(id): Path<String>,
    Json(b): Json<SaveFileBody>,
) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    let name = display_name(&app, &e);
    let actor = p.actor();
    let api = &app.docker.api;
    let mode = b.apply.as_deref().unwrap_or("none");
    let running = e.summary.is_running();
    let timeout = crate::updates::apply::stop_timeout(&app);
    if mode == "stop_start" && running {
        crate::docker::actions::stop(api, &e.summary.id, timeout).await?;
    }
    let saved = crate::files::save_file(&app.db, &e, &b.file, &actor).await;
    match mode {
        "stop_start" if running => crate::docker::actions::start(api, &e.summary.id).await?,
        "restart" if running && saved.is_ok() => crate::docker::actions::restart(api, &e.summary.id, timeout).await?,
        _ => {}
    }
    let saved = saved?;
    app.note(
        NewActivity::new("container.file_edited", Severity::Info, format!("Edited {} in {name}", b.file.path))
            .target(&e.summary.identity, &name)
            .actor(&actor),
    )
    .await;
    Ok(Json(serde_json::to_value(saved)?))
}

async fn file_versions(
    State(app): State<App>,
    Path(id): Path<String>,
    Query(q): Query<FileQuery>,
) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    let root = e.summary.mounts.iter().find(|m| m.destination == q.root).ok_or_else(|| AppError::not_found("mount"))?;
    let base = FsPath::new(&root.source);
    let path = crate::util::fsx::confine(base, &q.path)?;
    Ok(Json(serde_json::to_value(crate::files::list_versions(&app.db, &path.to_string_lossy()).await?)?))
}

async fn file_version_content(State(app): State<App>, Auth(p): Auth, Path(vid): Path<i64>) -> Result<Json<Value>> {
    p.require_admin()?;
    let (path, content) = crate::files::version_content(&app.db, vid).await?;
    Ok(Json(json!({ "path": path, "content": content })))
}

#[derive(Deserialize)]
struct RestoreVersionBody {
    root: String,
    path: String,
    version_id: i64,
}

async fn restore_file_version(
    State(app): State<App>,
    Auth(p): Auth,
    Path(id): Path<String>,
    Json(b): Json<RestoreVersionBody>,
) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    let (vpath, content) = crate::files::version_content(&app.db, b.version_id).await?;
    let current = crate::files::read_file(&e, &b.root, &b.path).await?;
    if current.path != vpath {
        return Err(AppError::bad_request("that version belongs to a different file"));
    }
    let save = crate::files::SaveFile { root: b.root, path: b.path, content, etag: None };
    let saved = crate::files::save_file(&app.db, &e, &save, &p.actor()).await?;
    Ok(Json(serde_json::to_value(saved)?))
}

// --- Updates ------------------------------------------------------------------

async fn check(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    app.registry.forget_digests();
    let info = crate::updates::check::check_one(&app, &e, true).await;
    app.updates.set(&e.summary.identity, info.clone()).await;
    Ok(Json(serde_json::to_value(info)?))
}

async fn update(
    State(app): State<App>,
    Auth(p): Auth,
    Path(id): Path<String>,
    body: Option<Json<UpdateRequest>>,
) -> Result<Json<Value>> {
    let req = body.map(|Json(b)| b).unwrap_or_default();
    let trigger = if p.via == crate::auth::AuthVia::ApiKey { Trigger::Api } else { Trigger::Manual };
    let task = crate::updates::apply::start_update(&app, &id, req, &p.actor(), trigger)?;
    Ok(Json(json!({ "task_id": task })))
}

#[derive(Deserialize, Default)]
struct RollbackBody {
    history_id: Option<i64>,
}

async fn rollback(
    State(app): State<App>,
    Auth(p): Auth,
    Path(id): Path<String>,
    body: Option<Json<RollbackBody>>,
) -> Result<Json<Value>> {
    let b = body.map(|Json(b)| b).unwrap_or_default();
    let task = crate::updates::apply::start_rollback(&app, &id, b.history_id, &p.actor())?;
    Ok(Json(json!({ "task_id": task })))
}

#[derive(Deserialize)]
struct TagsQuery {
    #[serde(deserialize_with = "crate::util::flag", default)]
    refresh: bool,
}

async fn tags(State(app): State<App>, Path(id): Path<String>, Query(q): Query<TagsQuery>) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    let r = ImageRef::parse(&e.summary.image).ok_or_else(|| AppError::bad_request("image reference is not valid"))?;
    let tags = app.registry.tags(&r, q.refresh).await?;
    let current = r.tag_or_latest().to_string();
    let (channels, versions) = split_tags(&tags, &current);
    let info = app.updates.get(&e.summary.identity);
    Ok(Json(json!({
        "current": current,
        "channels": channels.into_iter().take(60).collect::<Vec<_>>(),
        "versions": versions.into_iter().take(300).collect::<Vec<_>>(),
        "total": tags.len(),
        "update": info,
        "is_compose": e.summary.stack.is_some(),
    })))
}

/// Splits tags into channels ("latest", "develop") and versions, with
/// versions of the same line as `current` first.
pub fn split_tags<'a>(
    tags: &'a [crate::registry::client::TagInfo],
    current: &str,
) -> (Vec<&'a crate::registry::client::TagInfo>, Vec<&'a crate::registry::client::TagInfo>) {
    let (mut channels, mut versions): (Vec<_>, Vec<_>) =
        tags.iter().partition(|t| crate::registry::version::is_channel_tag(&t.name));
    channels.retain(|t| !t.name.starts_with("sha256") && !t.name.starts_with("sha-") && t.name.len() < 40);
    if let Some(cur) = crate::registry::version::Version::parse(current) {
        versions.sort_by_key(|t| crate::registry::version::Version::parse(&t.name).is_none_or(|v| !v.comparable(&cur)));
    }
    (channels, versions)
}

#[derive(Deserialize)]
struct TagPreviewQuery {
    tag: String,
}

/// Shows exactly which file edit a tag switch would make.
async fn tag_preview(
    State(app): State<App>,
    Path(id): Path<String>,
    Query(q): Query<TagPreviewQuery>,
) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    let s = &e.summary;
    let new_ref = match crate::compose::yaml_edit::plan_tag_edit(&s.image, &q.tag).map_err(AppError::BadRequest)? {
        crate::compose::yaml_edit::TagEdit::Literal(v) => v,
        _ => s.image.clone(),
    };
    let changes = match (&s.stack, &s.service) {
        (Some(stack), Some(service)) => {
            let stack = app.compose.stack_ref(stack)?;
            app.compose.plan_tag_change(&stack, service, &q.tag).await?
        }
        _ => Vec::new(),
    };
    let diffs: Vec<Value> =
        changes.iter().map(|c| json!({ "path": c.path, "diff": unified_diff(&c.before, &c.after, &c.path) })).collect();
    Ok(Json(json!({ "from": s.image, "to": new_ref, "files": diffs })))
}

pub fn unified_diff(before: &str, after: &str, name: &str) -> String {
    let name = name.trim_start_matches('/');
    similar::TextDiff::from_lines(before, after)
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{name}"), &format!("b/{name}"))
        .to_string()
}

async fn releases(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    let source = app.updates.get(&e.summary.identity).and_then(|i| i.source.clone()).or(app
        .docker
        .image_meta(&e.summary.image_id)
        .await
        .and_then(|m| m.source_url().map(str::to_string)));
    let Some(repo) = crate::updates::releases::repo_for(e.summary.app.as_deref(), source.as_deref()) else {
        return Ok(Json(json!({ "repo": null, "releases": [] })));
    };
    let notes = crate::updates::releases::fetch(&app, &repo).await?;
    Ok(Json(serde_json::to_value(notes)?))
}

async fn update_history(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    Ok(Json(serde_json::to_value(app.updates.history(Some(e.summary.identity.clone()), 50).await?)?))
}

#[derive(Deserialize, Default)]
struct SkipBody {
    /// Digest to skip; defaults to the currently available one. Empty string clears.
    digest: Option<String>,
}

async fn skip_version(
    State(app): State<App>,
    Path(id): Path<String>,
    body: Option<Json<SkipBody>>,
) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    let b = body.map(|Json(b)| b).unwrap_or_default();
    let digest = match b.digest {
        Some(d) => d,
        None => app
            .updates
            .get(&e.summary.identity)
            .and_then(|i| i.latest_digest.clone())
            .ok_or_else(|| AppError::conflict("no update to skip"))?,
    };
    app.prefs.update(&e.summary.identity, |p| p.update.skipped_digest = digest.clone()).await?;
    app.bus.publish(crate::events::Event::Prefs { identity: e.summary.identity.clone() });
    Ok(Json(json!({ "skipped_digest": digest })))
}

#[derive(Deserialize)]
struct HoldBody {
    held: bool,
}

async fn hold(State(app): State<App>, Path(id): Path<String>, Json(b): Json<HoldBody>) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    app.prefs.update(&e.summary.identity, |p| p.update.held = b.held).await?;
    app.bus.publish(crate::events::Event::Prefs { identity: e.summary.identity.clone() });
    Ok(Json(json!({ "held": b.held })))
}

/// Kinds of dashboard "needs attention" items that can be put away.
const ATTENTION_KINDS: &[&str] = &["crash", "health", "web", "app", "held", "update"];

#[derive(Deserialize)]
struct DismissBody {
    kind: String,
    /// The occurrence to hide, "*" to hide every occurrence, or null to show
    /// the item again.
    fingerprint: Option<String>,
}

async fn dismiss(State(app): State<App>, Path(id): Path<String>, Json(b): Json<DismissBody>) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    if !ATTENTION_KINDS.contains(&b.kind.as_str()) {
        return Err(AppError::bad_request(format!("unknown kind '{}'", b.kind)));
    }
    let saved = app
        .prefs
        .update(&e.summary.identity, |p| match b.fingerprint {
            Some(f) => {
                p.dismissed.insert(b.kind, f);
            }
            None => {
                p.dismissed.remove(&b.kind);
            }
        })
        .await?;
    app.bus.publish(crate::events::Event::Prefs { identity: e.summary.identity.clone() });
    Ok(Json(json!({ "dismissed": saved.dismissed })))
}

// --- Backups -------------------------------------------------------------------

async fn backups(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    let root = std::path::PathBuf::from(&app.settings.get().backups.root);
    Ok(Json(serde_json::to_value(app.backups.list(&root, Some(e.summary.identity.clone()), 200).await?)?))
}

async fn backup_plan(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    let plan = crate::backups::plan::plan(&app, &e);
    // Estimate sizes of directly readable folders (bounded work).
    let excludes = crate::backups::archive::build_globs(&plan.excludes);
    let mut sizes = HashMap::new();
    for m in plan.mounts.iter().filter(|m| m.include && m.access == crate::backups::plan::Access::Direct) {
        let src = m.source.clone();
        let ex = excludes.clone();
        let size = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::task::spawn_blocking(move || {
                crate::util::fsx::dir_size(FsPath::new(&src), &|rel| {
                    let r = rel.to_string_lossy();
                    ex.is_match(r.as_ref()) || ex.is_match(format!("{r}/__nodestarr_probe__"))
                })
            }),
        )
        .await;
        if let Ok(Ok(sz)) = size {
            sizes.insert(m.destination.clone(), sz);
        }
    }
    Ok(Json(json!({ "plan": plan, "sizes": sizes, "root_available": app.backups.root_available(&app) })))
}

async fn backup_now(State(app): State<App>, Auth(p): Auth, Path(id): Path<String>) -> Result<Json<Value>> {
    let task = crate::backups::start_backup(&app, &id, &p.actor())?;
    Ok(Json(json!({ "task_id": task })))
}

async fn app_refresh(State(app): State<App>, Path(id): Path<String>) -> Result<Json<Value>> {
    let e = entry(&app, &id)?;
    app.integrations.forget_creds(&e.summary.identity);
    let status = crate::integrations::poll(&app, &e).await;
    Ok(Json(serde_json::to_value(status)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_masking() {
        let v = mask_env(&["PUID=1000".into(), "API_KEY=abc".into(), "DB_PASSWORD=".into()]);
        assert_eq!(v[0]["value"], "1000");
        assert_eq!(v[1]["value"], "••••••••");
        assert_eq!(v[2]["value"], "");
    }

    #[test]
    fn diff_output() {
        let d = unified_diff("a\nb\n", "a\nc\n", "compose.yaml");
        assert!(d.contains("-b") && d.contains("+c"));
    }
}
