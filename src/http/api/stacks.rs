//! Compose stack endpoints: list, detail, actions, editor, history,
//! create and delete.

use std::path::Path as FsPath;

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::activity::NewActivity;
use crate::app::App;
use crate::compose::{ComposeService, FileEdit};
use crate::error::{AppError, Result};
use crate::events::Event;
use crate::http::Auth;
use crate::http::api::containers::unified_diff;
use crate::ops::{StackAction, StackActionOptions};
use crate::settings::Severity;
use crate::tasks::TaskSpec;

pub fn routes() -> Router<App> {
    Router::new()
        .route("/stacks", get(list).post(create))
        .route("/stacks/{name}", get(detail).delete(remove))
        .route("/stacks/{name}/actions/{action}", post(action))
        .route("/stacks/{name}/files", get(files).put(save_files))
        .route("/stacks/{name}/file", get(file))
        .route("/stacks/{name}/tree", get(tree))
        .route("/stacks/{name}/validate", post(validate))
        .route("/stacks/{name}/diff", post(diff))
        .route("/stacks/{name}/history", get(history))
        .route("/stacks/{name}/backup", post(backup))
}

async fn list(State(app): State<App>) -> Result<Json<Value>> {
    let updates = app.updates.all();
    let stacks: Vec<Value> = app
        .compose
        .list()
        .into_iter()
        .map(|s| {
            let pending =
                s.containers.iter().filter(|c| updates.get(&c.identity).is_some_and(|u| u.has_update())).count();
            let mut v = serde_json::to_value(&s).unwrap_or_default();
            v["updates"] = json!(pending);
            v
        })
        .collect();
    Ok(Json(
        json!({ "stacks": stacks, "compose_available": app.compose.available(), "compose_version": app.compose.version() }),
    ))
}

async fn detail(State(app): State<App>, Path(name): Path<String>) -> Result<Json<Value>> {
    let summary = app.compose.summary(&name)?;
    let (resolved, error) = match app.compose.stack_ref(&name) {
        Ok(stack) => match app.compose.resolved(&stack).await {
            Ok(p) => (Some(p), None),
            Err(e) => (None, Some(e.to_string())),
        },
        Err(e) => (None, Some(e.to_string())),
    };
    // Services defined in the files, with their container (if any).
    let mut services = Vec::new();
    if let Some(p) = &resolved {
        for (svc, def) in &p.services {
            let container = summary.containers.iter().find(|c| c.service.as_deref() == Some(svc.as_str()));
            services.push(json!({
                "name": svc,
                "image": def.image,
                "profiles": def.profiles,
                "network_mode": def.network_mode,
                "built": def.build.is_some(),
                "container": container,
            }));
        }
    } else {
        for c in &summary.containers {
            services.push(json!({ "name": c.service, "container": c }));
        }
    }
    Ok(Json(json!({ "stack": summary, "services": services, "config_error": error })))
}

async fn action(
    State(app): State<App>,
    Auth(p): Auth,
    Path((name, action)): Path<(String, String)>,
    body: Option<Json<StackActionOptions>>,
) -> Result<Json<Value>> {
    let action: StackAction = serde_json::from_value(Value::String(action.clone()))
        .map_err(|_| AppError::bad_request(format!("unknown action '{action}'")))?;
    let opts = body.map(|Json(b)| b).unwrap_or_default();
    let task = crate::ops::stack_action(&app, &name, action, opts, &p.actor())?;
    Ok(Json(json!({ "task_id": task })))
}

/// Compose files and .env hold secrets, so only admins can read them.
async fn files(State(app): State<App>, Auth(p): Auth, Path(name): Path<String>) -> Result<Json<Value>> {
    p.require_admin()?;
    let stack = app.compose.stack_ref(&name)?;
    let files = app.compose.read_files(&stack).await?;
    Ok(Json(json!({ "files": files, "dir": stack.dir })))
}

#[derive(Deserialize)]
struct FileQuery {
    path: String,
}

/// Any text file of the stack (env files, Dockerfiles...), for the editor.
async fn file(
    State(app): State<App>,
    Auth(p): Auth,
    Path(name): Path<String>,
    Query(q): Query<FileQuery>,
) -> Result<Json<Value>> {
    p.require_admin()?;
    let stack = app.compose.stack_ref(&name)?;
    Ok(Json(serde_json::to_value(app.compose.read_file(&stack, &q.path).await?)?))
}

/// The files in the stack folder, for opening ones compose doesn't mention.
async fn tree(State(app): State<App>, Auth(p): Auth, Path(name): Path<String>) -> Result<Json<Value>> {
    p.require_admin()?;
    let stack = app.compose.stack_ref(&name)?;
    let dir = stack.dir.clone();
    let (entries, truncated) = tokio::task::spawn_blocking(move || ComposeService::tree(&stack))
        .await
        .map_err(|e| AppError::Other(e.into()))?;
    Ok(Json(json!({ "dir": dir, "entries": entries, "truncated": truncated })))
}

#[derive(Deserialize)]
struct EditBody {
    files: Vec<FileEdit>,
    #[serde(default)]
    deploy: bool,
    /// Deploy only these services.
    #[serde(default)]
    services: Vec<String>,
    /// Rebuild images while deploying (a Dockerfile changed).
    #[serde(default)]
    build: bool,
}

/// Points each edit at the file it may change (see `resolve_file`).
fn resolve_edits(app: &App, stack: &crate::compose::StackRef, files: Vec<FileEdit>) -> Result<Vec<FileEdit>> {
    files
        .into_iter()
        .map(|f| {
            let path = app.compose.resolve_file(stack, &f.path)?;
            Ok(FileEdit { path: path.to_string_lossy().into_owned(), content: f.content })
        })
        .collect()
}

async fn validate(State(app): State<App>, Path(name): Path<String>, Json(b): Json<EditBody>) -> Result<Json<Value>> {
    let stack = app.compose.stack_ref(&name)?;
    let files = resolve_edits(&app, &stack, b.files)?;
    match app.compose.validate(&stack, &files).await? {
        Ok(()) => Ok(Json(json!({ "ok": true }))),
        Err(msg) => Ok(Json(json!({ "ok": false, "message": msg }))),
    }
}

async fn diff(State(app): State<App>, Path(name): Path<String>, Json(b): Json<EditBody>) -> Result<Json<Value>> {
    let stack = app.compose.stack_ref(&name)?;
    let mut out = Vec::new();
    for f in resolve_edits(&app, &stack, b.files)? {
        let before = tokio::fs::read_to_string(&f.path).await.unwrap_or_default();
        if before != f.content {
            let label = FsPath::new(&f.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            out.push(json!({ "path": f.path, "diff": unified_diff(&before, &f.content, &label) }));
        }
    }
    Ok(Json(json!({ "files": out })))
}

async fn save_files(
    State(app): State<App>,
    Auth(p): Auth,
    Path(name): Path<String>,
    Json(b): Json<EditBody>,
) -> Result<Json<Value>> {
    let stack = app.compose.stack_ref(&name)?;
    let files = resolve_edits(&app, &stack, b.files)?;
    if let Err(msg) = app.compose.validate(&stack, &files).await? {
        return Err(AppError::BadRequest(format!("the compose files are not valid: {msg}")));
    }
    let actor = p.actor();
    let mut saved = Vec::new();
    for f in &files {
        // New files may go in new folders (a service's build context, say).
        crate::compose::create_parents(FsPath::new(&f.path))?;
        if crate::files::write_with_history(
            &app.db,
            FsPath::new(&f.path),
            &f.content,
            &actor,
            Some(format!("stack {name}")),
        )
        .await?
        {
            saved.push(f.path.clone());
        }
    }
    app.compose.invalidate(&name);
    app.bus.publish(Event::StacksChanged);
    if !saved.is_empty() {
        app.note(
            NewActivity::new(
                "stack.edited",
                Severity::Info,
                format!(
                    "Edited {} in stack {name}",
                    saved
                        .iter()
                        .map(|s| FsPath::new(s)
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
            .target(format!("stack:{name}"), &name)
            .actor(&actor),
        )
        .await;
    }
    let task = if b.deploy {
        let opts = StackActionOptions { services: b.services, build: b.build, ..Default::default() };
        Some(crate::ops::stack_action(&app, &name, StackAction::Up, opts, &actor)?)
    } else {
        None
    };
    Ok(Json(json!({ "saved": saved, "task_id": task })))
}

#[derive(Deserialize)]
struct HistoryQuery {
    path: String,
}

async fn history(
    State(app): State<App>,
    Path(name): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Value>> {
    let stack = app.compose.stack_ref(&name)?;
    let path = app.compose.resolve_file(&stack, &q.path)?;
    Ok(Json(serde_json::to_value(crate::files::list_versions(&app.db, &path.to_string_lossy()).await?)?))
}

#[derive(Deserialize)]
struct CreateBody {
    name: String,
    compose: String,
    #[serde(default)]
    env: Option<String>,
    /// More files for the new folder, with paths relative to it.
    #[serde(default)]
    files: Vec<FileEdit>,
    #[serde(default)]
    deploy: bool,
}

async fn create(State(app): State<App>, Auth(p): Auth, Json(b): Json<CreateBody>) -> Result<Json<Value>> {
    let stack = app.compose.create_stack(&b.name, &b.compose, b.env.as_deref(), &b.files).await?;
    let actor = p.actor();
    app.note(
        NewActivity::new("stack.created", Severity::Success, format!("Stack {} created in {}", stack.name, stack.dir))
            .target(format!("stack:{}", stack.name), &stack.name)
            .actor(&actor),
    )
    .await;
    app.bus.publish(Event::StacksChanged);
    let task = if b.deploy {
        // The new stack isn't in the inventory yet, so run compose directly.
        let spec = TaskSpec::new("compose", format!("Deploy stack {}", stack.name), &actor)
            .lock(format!("stack:{}", stack.name));
        let app2 = app.clone();
        let st = stack.clone();
        Some(app.tasks.spawn(spec, move |ctx| async move {
            app2.compose.exec(&st, &["up", "-d"], Some(&ctx)).await?;
            app2.bus.publish(Event::StacksChanged);
            Ok(json!({}))
        }))
    } else {
        None
    };
    Ok(Json(json!({ "stack": stack, "task_id": task })))
}

#[derive(Deserialize)]
struct RemoveQuery {
    #[serde(deserialize_with = "crate::util::flag", default)]
    remove_files: bool,
    #[serde(deserialize_with = "crate::util::flag", default)]
    remove_volumes: bool,
}

async fn remove(
    State(app): State<App>,
    Auth(p): Auth,
    Path(name): Path<String>,
    Query(q): Query<RemoveQuery>,
) -> Result<Json<Value>> {
    let stack = app.compose.stack_ref(&name)?;
    if app.docker.in_stack(&name).iter().any(|e| e.summary.is_self) {
        return Err(AppError::forbidden("this stack contains Nodestarr itself"));
    }
    let actor = p.actor();
    let spec = TaskSpec::new("compose", format!("Remove stack {name}"), &actor).lock(format!("stack:{name}"));
    let app2 = app.clone();
    let task = app.tasks.spawn(spec, move |ctx| async move {
        let mut args = vec!["down", "--remove-orphans"];
        if q.remove_volumes {
            args.push("-v");
        }
        app2.compose.exec(&stack, &args, Some(&ctx)).await?;
        let mut moved = None;
        if q.remove_files {
            let target = app2.compose.trash_stack_folder(&stack).await?;
            ctx.log(format!("Moved the stack folder to {target}"));
            moved = Some(target);
        }
        app2.compose.invalidate(&stack.name);
        app2.bus.publish(Event::StacksChanged);
        app2.note(
            NewActivity::new("stack.removed", Severity::Warning, format!("Stack {} removed", stack.name))
                .target(format!("stack:{}", stack.name), &stack.name)
                .actor(&actor),
        )
        .await;
        Ok(json!({ "trash": moved }))
    });
    Ok(Json(json!({ "task_id": task })))
}

async fn backup(State(app): State<App>, Auth(p): Auth, Path(name): Path<String>) -> Result<Json<Value>> {
    let task = crate::backups::start_stack_backup(&app, &name, &p.actor())?;
    Ok(Json(json!({ "task_id": task })))
}
