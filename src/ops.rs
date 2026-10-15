//! High-level container and stack operations shared by the REST API, Home
//! Assistant and schedulers: they log activity, protect Nodestarr's own
//! container, and route compose-managed containers through `docker compose`.

use serde::Deserialize;
use serde_json::json;

use crate::activity::{Actor, NewActivity};
use crate::app::App;
use crate::docker::actions::{self, Action};
use crate::docker::recreate;
use crate::error::{AppError, Result};
use crate::settings::Severity;
use crate::tasks::TaskSpec;
use crate::updates::check::display_name;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ActionOptions {
    /// Signal for `kill` (default SIGKILL).
    #[serde(default)]
    pub signal: Option<String>,
    /// Remove anonymous volumes with `remove`.
    #[serde(default)]
    pub volumes: bool,
}

/// Performs a container action. Quick actions run inline; recreate runs as
/// a task and returns its id.
pub async fn container_action(app: &App, key: &str, action: Action, opts: ActionOptions, actor: &Actor) -> Result<Option<String>> {
    let entry = app.docker.get(key).ok_or_else(|| AppError::not_found("container"))?;
    let s = entry.summary.clone();
    let name = display_name(app, &entry);
    if s.is_self && matches!(action, Action::Stop | Action::Kill | Action::Pause | Action::Remove) {
        return Err(AppError::forbidden(format!(
            "Nodestarr can't {} its own container from the UI (it would lock you out); use the host instead",
            action.verb()
        )));
    }
    let timeout = crate::updates::apply::stop_timeout(app);
    let api = &app.docker.api;
    match action {
        Action::Start => actions::start(api, &s.id).await?,
        Action::Stop => actions::stop(api, &s.id, timeout).await?,
        Action::Restart => {
            if s.is_self {
                // Respond before the restart takes this process down.
                let api = api.clone();
                let id = s.id.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let _ = actions::restart(&api, &id, timeout).await;
                });
            } else {
                actions::restart(api, &s.id, timeout).await?
            }
        }
        Action::Pause => actions::pause(api, &s.id).await?,
        Action::Unpause => actions::unpause(api, &s.id).await?,
        Action::Kill => actions::kill(api, &s.id, opts.signal.as_deref().unwrap_or("SIGKILL")).await?,
        Action::Remove => {
            actions::remove(api, &s.id, true, opts.volumes).await?;
        }
        Action::Recreate => {
            let task = start_recreate(app, key, actor)?;
            return Ok(Some(task));
        }
    }
    app.note(
        NewActivity::new(
            match action {
                Action::Start => "container.started",
                Action::Stop => "container.stopped",
                Action::Restart => "container.restarted",
                Action::Pause => "container.paused",
                Action::Unpause => "container.resumed",
                Action::Kill => "container.killed",
                Action::Remove => "container.removed",
                Action::Recreate => "container.recreated",
            },
            if matches!(action, Action::Kill | Action::Remove) { Severity::Warning } else { Severity::Info },
            format!("{name} {}", action.past()),
        )
        .target(&s.identity, &name)
        .actor(actor),
    )
    .await;
    app.docker.refresh(&s.id).await;
    Ok(None)
}

/// Recreates a container with its current image and settings (compose
/// `up --force-recreate`, or an in-place replacement for standalone ones).
pub fn start_recreate(app: &App, key: &str, actor: &Actor) -> Result<String> {
    let entry = app.docker.get(key).ok_or_else(|| AppError::not_found("container"))?;
    if entry.summary.is_self {
        return Err(AppError::forbidden("use Settings > System > Update to recreate Nodestarr itself"));
    }
    let name = display_name(app, &entry);
    let identity = entry.summary.identity.clone();
    let spec = TaskSpec::new("recreate", format!("Recreate {name}"), actor).target(identity.clone(), name.clone());
    let (app2, actor2) = (app.clone(), actor.clone());
    Ok(app.tasks.spawn(spec, move |ctx| async move {
        let entry = app2.docker.by_identity(&identity).ok_or_else(|| AppError::not_found("container"))?;
        let s = entry.summary.clone();
        if let (Some(stack), Some(service)) = (&s.stack, &s.service) {
            let stack = app2.compose.stack_ref(stack)?;
            app2.compose
                .exec(&stack, &["up", "-d", "--no-deps", "--force-recreate", "--pull", "never", service], Some(&ctx))
                .await?;
        } else {
            let cfg = app2.docker.image_meta(&s.image_id).await.and_then(|m| m.config.clone());
            let body = recreate::body_from_inspect(&entry.inspect, cfg.as_ref(), &s.image);
            let rec = recreate::replace(&app2.docker.api, &s.id, body, crate::updates::apply::stop_timeout(&app2), &|l| ctx.log(l)).await?;
            recreate::finalize(&app2.docker.api, &rec).await?;
        }
        app2.note(NewActivity::new("container.recreated", Severity::Info, format!("{name} recreated")).target(&identity, &name).actor(&actor2)).await;
        Ok(serde_json::Value::Null)
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StackAction {
    Up,
    Down,
    Start,
    Stop,
    Restart,
    Pull,
    /// Pull then `up -d` (recreates what changed).
    Update,
}

impl StackAction {
    fn verb(self) -> &'static str {
        match self {
            StackAction::Up => "Deploy",
            StackAction::Down => "Take down",
            StackAction::Start => "Start",
            StackAction::Stop => "Stop",
            StackAction::Restart => "Restart",
            StackAction::Pull => "Pull",
            StackAction::Update => "Update",
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StackActionOptions {
    /// Limit to these services.
    #[serde(default)]
    pub services: Vec<String>,
    /// `down -v`: also remove named volumes.
    #[serde(default)]
    pub remove_volumes: bool,
    /// `up --remove-orphans`.
    #[serde(default)]
    pub remove_orphans: bool,
}

pub fn stack_action(app: &App, name: &str, action: StackAction, opts: StackActionOptions, actor: &Actor) -> Result<String> {
    let stack = app.compose.stack_ref(name)?;
    let self_in_stack = app.docker.in_stack(name).iter().any(|e| e.summary.is_self);
    if self_in_stack && matches!(action, StackAction::Down | StackAction::Stop) && opts.services.is_empty() {
        return Err(AppError::forbidden("this stack contains Nodestarr itself; stop individual services instead"));
    }
    for svc in &opts.services {
        if svc.starts_with('-') || svc.contains(char::is_whitespace) {
            return Err(AppError::bad_request("invalid service name"));
        }
    }
    let title = if opts.services.is_empty() {
        format!("{} stack {name}", action.verb())
    } else {
        format!("{} {} in {name}", action.verb(), opts.services.join(", "))
    };
    let self_service = app
        .docker
        .in_stack(name)
        .iter()
        .find(|e| e.summary.is_self)
        .and_then(|e| e.summary.service.clone());
    let spec = TaskSpec::new("compose", title.clone(), actor).lock(format!("stack:{name}"));
    let (app2, actor2, name2) = (app.clone(), actor.clone(), name.to_string());
    Ok(app.tasks.spawn(spec, move |ctx| async move {
        let mut opts = opts;
        // Never let compose recreate or restart Nodestarr's own container from
        // inside Nodestarr: the compose process would die halfway through.
        if let Some(own) = &self_service {
            if opts.services.is_empty() && !matches!(action, StackAction::Pull) {
                let project = app2.compose.resolved(&stack).await?;
                opts.services = project.services.keys().filter(|s| *s != own).cloned().collect();
                ctx.log(format!("Skipping '{own}' (Nodestarr itself); update Nodestarr from Settings > System"));
            } else if opts.services.iter().any(|s| s == own) && !matches!(action, StackAction::Pull) {
                return Err(AppError::forbidden("Nodestarr can't restart or recreate its own service through compose; use Settings > System"));
            }
        }
        let svcs: Vec<&str> = opts.services.iter().map(String::as_str).collect();
        fn with_services<'a>(mut args: Vec<&'a str>, services: &[&'a str]) -> Vec<&'a str> {
            args.extend_from_slice(services);
            args
        }
        let run = |args: Vec<&'static str>| with_services(args, &svcs);
        match action {
            StackAction::Up => {
                let mut args = vec!["up", "-d"];
                if opts.remove_orphans {
                    args.push("--remove-orphans");
                }
                app2.compose.exec(&stack, &run(args), Some(&ctx)).await?;
            }
            StackAction::Down => {
                let mut args = vec!["down"];
                if opts.remove_volumes {
                    args.push("-v");
                }
                if opts.remove_orphans {
                    args.push("--remove-orphans");
                }
                app2.compose.exec(&stack, &run(args), Some(&ctx)).await?;
            }
            StackAction::Start => {
                app2.compose.exec(&stack, &run(vec!["start"]), Some(&ctx)).await?;
            }
            StackAction::Stop => {
                app2.compose.exec(&stack, &run(vec!["stop"]), Some(&ctx)).await?;
            }
            StackAction::Restart => {
                app2.compose.exec(&stack, &run(vec!["restart"]), Some(&ctx)).await?;
            }
            StackAction::Pull => {
                app2.compose.exec(&stack, &run(vec!["pull"]), Some(&ctx)).await?;
            }
            StackAction::Update => {
                app2.compose.exec(&stack, &run(vec!["pull"]), Some(&ctx)).await?;
                ctx.progress(0.6);
                app2.compose.exec(&stack, &run(vec!["up", "-d"]), Some(&ctx)).await?;
            }
        }
        app2.bus.publish(crate::events::Event::StacksChanged);
        app2.note(NewActivity::new("stack.action", Severity::Info, title).target(format!("stack:{name2}"), &name2).actor(&actor2)).await;
        Ok(json!({}))
    }))
}
