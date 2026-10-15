//! Applying updates, switching tags, and rolling back.
//!
//! Sequence for one container:
//! 1. optional pre-update backup,
//! 2. pin the running image under a local `nodestarr-rollback/...` tag,
//! 3. pull the new image (and edit the compose file for tag switches),
//! 4. recreate the container (compose `up -d`, or an in-place replacement
//!    for standalone containers),
//! 5. recreate containers sharing its network namespace,
//! 6. verify it stays up (and healthy / answering HTTP),
//! 7. on failure, restore the previous image and compose file.

use std::collections::HashMap;
use std::time::Duration;

use bollard::query_parameters::{
    CreateImageOptions, ListContainersOptions, ListImagesOptions, PruneImagesOptions, RemoveImageOptions,
    TagImageOptions,
};
use futures_util::StreamExt;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::activity::{Actor, NewActivity};
use crate::app::App;
use crate::compose::{FileChange, StackRef};
use crate::docker::model::{LABEL_PROJECT, LABEL_SERVICE};
use crate::docker::{Entry, actions, recreate};
use crate::error::{AppError, Result};
use crate::notify::Notification;
use crate::registry::ImageRef;
use crate::settings::Severity;
use crate::tasks::{TaskCtx, TaskSpec};
use crate::updates::check::display_name;

/// Local repository for images pinned so an update can be undone.
pub const ROLLBACK_REPO_PREFIX: &str = "nodestarr-rollback/";

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct UpdateRequest {
    /// Switch to this tag (branch or version), e.g. "develop" or "4.0.10".
    #[serde(default)]
    pub tag: Option<String>,
    /// Skip the pre-update backup.
    #[serde(default)]
    pub skip_backup: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Manual,
    Auto,
    Api,
    Mqtt,
}

impl Trigger {
    fn as_str(self) -> &'static str {
        match self {
            Trigger::Manual => "manual",
            Trigger::Auto => "auto",
            Trigger::Api => "api",
            Trigger::Mqtt => "mqtt",
        }
    }
}

/// Starts an update task; returns the task id.
pub fn start_update(app: &App, key: &str, req: UpdateRequest, actor: &Actor, trigger: Trigger) -> Result<String> {
    let entry = app.docker.get(key).ok_or_else(|| AppError::not_found("container"))?;
    validate_tag(req.tag.as_deref())?;
    let identity = entry.summary.identity.clone();
    let name = display_name(app, &entry);
    let title = match &req.tag {
        Some(t) => format!("Switch {name} to {t}"),
        None => format!("Update {name}"),
    };
    let spec = TaskSpec::new("update", title, actor).target(identity.clone(), name);
    let app2 = app.clone();
    let actor = actor.clone();
    Ok(app.tasks.spawn(spec, move |ctx| async move { run_update(&app2, &ctx, &identity, req, &actor, trigger).await }))
}

fn validate_tag(tag: Option<&str>) -> Result<()> {
    if let Some(t) = tag {
        let ok = !t.is_empty()
            && t.len() <= 128
            && t.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            && t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
        if !ok {
            return Err(AppError::bad_request(format!("'{t}' is not a valid image tag")));
        }
    }
    Ok(())
}

/// What was changed, so it can be undone.
enum Applied {
    Compose { stack: StackRef, service: String, files: Option<Vec<FileChange>> },
    Standalone(recreate::Recreation),
}

/// Runs the update inside a task. Also used directly by the auto-updater.
pub async fn run_update(
    app: &App,
    ctx: &TaskCtx,
    identity: &str,
    req: UpdateRequest,
    actor: &Actor,
    trigger: Trigger,
) -> Result<serde_json::Value> {
    let entry =
        app.docker.by_identity(identity).ok_or_else(|| AppError::not_found("container (it may have been removed)"))?;
    if entry.summary.is_self {
        return crate::selfupdate::start(app, ctx, &entry, req.tag.as_deref()).await;
    }
    let s = entry.summary.clone();
    let settings = app.settings.get();
    let prefs = app.prefs.get(identity);
    let name = display_name(app, &entry);

    let old_ref = s.image.clone();
    let old_parsed =
        ImageRef::parse(&old_ref).ok_or_else(|| AppError::bad_request("container image reference is not valid"))?;
    let new_ref = match &req.tag {
        Some(tag) => match crate::compose::yaml_edit::plan_tag_edit(&old_ref, tag).map_err(AppError::BadRequest)? {
            crate::compose::yaml_edit::TagEdit::Literal(v) => v,
            _ => unreachable!("resolved image references contain no variables"),
        },
        None => old_ref.clone(),
    };
    let target = ImageRef::parse(&new_ref).ok_or_else(|| AppError::bad_request("invalid target image"))?;
    let from_version = s.version.clone();
    ctx.log(format!("{name}: {} → {}", old_parsed.familiar(), target.familiar()));

    // 1. Pre-update backup.
    if !req.skip_backup && settings.updates.backup_before_update && prefs.backup_enabled(&settings) {
        if app.backups.root_available(app) {
            ctx.log("Taking a pre-update backup");
            ctx.progress(0.05);
            crate::backups::run_container_backup(app, ctx, &entry, crate::backups::BackupKind::PreUpdate)
                .await
                .map_err(|e| {
                    AppError::conflict(format!(
                        "pre-update backup failed, update aborted: {e} (retry with 'skip backup' to update anyway)"
                    ))
                })?;
        } else {
            ctx.log("Backup folder not available, skipping the pre-update backup");
        }
    }
    ctx.check_cancelled()?;

    // 2. Pin the current image so it can't be garbage collected.
    let rollback_tag = match pin_image(app, identity, &s.image_id).await {
        Ok(t) => Some(t),
        Err(err) => {
            ctx.log(format!("Could not tag the current image for rollback: {err}"));
            None
        }
    };

    // 3. Pull.
    ctx.progress(0.2);
    if let Err(err) = pull(app, ctx, &target).await {
        unpin(app, rollback_tag.as_deref()).await;
        return Err(err);
    }
    ctx.check_cancelled()?;
    let pulled = app
        .docker
        .inspect_image_fresh(&new_ref)
        .await
        .ok_or_else(|| AppError::conflict(format!("{new_ref} is not available after pulling")))?;
    // The version being installed, for the history (also when it fails).
    let attempted_version = crate::docker::model::detect_version(&new_ref, Some(&pulled), None).0;
    if pulled.id == s.image_id && req.tag.is_none() {
        ctx.log("Already running the newest image, nothing to do");
        unpin(app, rollback_tag.as_deref()).await;
        let info = crate::updates::check::check_one(app, &entry, false).await;
        app.updates.set(identity, info).await;
        return Ok(json!({ "changed": false }));
    }

    // 4. Recreate.
    ctx.progress(0.6);
    let applied = match apply_change(app, ctx, &entry, &new_ref, req.tag.as_deref(), actor).await {
        Ok(a) => a,
        Err(err) => {
            ctx.log(format!("Applying the update failed: {err}"));
            if let Some(tag) = &rollback_tag {
                let _ = restore_compose_after_failure(app, ctx, &entry, tag, &old_ref, actor).await;
            }
            record_history(
                app,
                identity,
                &name,
                &old_ref,
                &new_ref,
                from_version.as_deref(),
                attempted_version.as_deref(),
                rollback_tag.as_deref(),
                None,
                "failed",
                trigger,
                ctx.id(),
                &err.to_string(),
            )
            .await;
            notify_failure(app, &name, identity, &format!("Update failed: {err}"), false);
            return Err(err);
        }
    };

    // 5. Containers that share its network namespace.
    recreate_network_dependents(app, ctx, &entry).await;

    // 6. Verify.
    ctx.progress(0.8);
    let new_id = match &applied {
        Applied::Standalone(r) => r.new_id.clone(),
        Applied::Compose { stack, service, .. } => {
            find_service_container(app, &stack.name, service).await.unwrap_or_default()
        }
    };
    let verdict = if new_id.is_empty() {
        Err("the new container could not be found".to_string())
    } else {
        verify(app, ctx, &entry, &new_id).await
    };

    match verdict {
        Ok(()) => {
            if let Applied::Standalone(r) = &applied {
                if let Err(err) = recreate::finalize(&app.docker.api, r).await {
                    ctx.log(format!("Could not remove the old container: {err}"));
                }
            }
            app.docker.refresh(&new_id).await;
            let to_version =
                app.docker.get(&new_id).and_then(|e| e.summary.version.clone()).or_else(|| attempted_version.clone());
            let files_json = match &applied {
                Applied::Compose { files: Some(f), .. } => Some(serde_json::to_string(f).unwrap_or_default()),
                _ => None,
            };
            record_history(
                app,
                identity,
                &name,
                &old_ref,
                &new_ref,
                from_version.as_deref(),
                to_version.as_deref(),
                rollback_tag.as_deref(),
                files_json,
                "applied",
                trigger,
                ctx.id(),
                "",
            )
            .await;
            let _ = app
                .prefs
                .update(identity, |p| {
                    p.update.skipped_digest.clear();
                })
                .await;
            cleanup_images(app, ctx, identity).await;
            if let Some(e) = app.docker.get(&new_id) {
                let info = crate::updates::check::check_one(app, &e, false).await;
                app.updates.set(identity, info).await;
            }
            let versions = match (&from_version, &to_version) {
                (Some(a), Some(b)) if a != b => format!("{a} → {b}"),
                (_, Some(b)) => b.clone(),
                _ => target.familiar(),
            };
            ctx.log(format!("{name} updated ({versions})"));
            app.note(
                NewActivity::new("update.applied", Severity::Success, format!("{name} updated ({versions})"))
                    .target(identity, &name)
                    .actor(actor)
                    .details(json!({ "from": old_ref, "to": new_ref, "trigger": trigger.as_str() })),
            )
            .await;
            app.notifier.send(
                Notification::new("update.applied", Severity::Success, format!("{name} updated"), versions.clone())
                    .target(identity, &name)
                    .field("Image", target.familiar())
                    .link(app, &format!("/containers/{}", s.name)),
            );
            Ok(json!({ "changed": true, "from_version": from_version, "to_version": to_version }))
        }
        Err(reason) => {
            ctx.log(format!("Verification failed: {reason}"));
            let latest_digest = app.updates.get(identity).and_then(|i| i.latest_digest.clone());
            if settings.updates.rollback_on_failure {
                let rb = rollback_applied(app, ctx, &entry, applied, rollback_tag.as_deref(), &old_ref, actor).await;
                let _ = app
                    .prefs
                    .update(identity, |p| {
                        if let Some(d) = &latest_digest {
                            p.update.skipped_digest = d.clone();
                        }
                    })
                    .await;
                let outcome = match &rb {
                    Ok(()) => "rolled back to the previous version".to_string(),
                    Err(e) => format!("rollback also failed: {e}"),
                };
                record_history(
                    app,
                    identity,
                    &name,
                    &old_ref,
                    &new_ref,
                    from_version.as_deref(),
                    attempted_version.as_deref(),
                    rollback_tag.as_deref(),
                    None,
                    "rolled_back",
                    trigger,
                    ctx.id(),
                    &format!("{reason}; {outcome}"),
                )
                .await;
                app.note(
                    NewActivity::new(
                        "update.rolled_back",
                        Severity::Error,
                        format!("{name}: update failed ({reason}), {outcome}"),
                    )
                    .target(identity, &name)
                    .actor(actor),
                )
                .await;
                notify_failure(
                    app,
                    &name,
                    identity,
                    &format!("{reason}. Nodestarr {outcome}; this version will be skipped by auto-update."),
                    true,
                );
                Err(AppError::conflict(format!("update failed ({reason}); {outcome}")))
            } else {
                record_history(
                    app,
                    identity,
                    &name,
                    &old_ref,
                    &new_ref,
                    from_version.as_deref(),
                    attempted_version.as_deref(),
                    rollback_tag.as_deref(),
                    None,
                    "failed",
                    trigger,
                    ctx.id(),
                    &reason,
                )
                .await;
                notify_failure(app, &name, identity, &reason, false);
                Err(AppError::conflict(format!("update applied but verification failed: {reason}")))
            }
        }
    }
}

async fn apply_change(
    app: &App,
    ctx: &TaskCtx,
    entry: &Entry,
    new_ref: &str,
    tag: Option<&str>,
    actor: &Actor,
) -> Result<Applied> {
    let s = &entry.summary;
    if let (Some(stack_name), Some(service)) = (&s.stack, &s.service) {
        let stack = app.compose.stack_ref(stack_name)?;
        let mut files = None;
        if let Some(tag) = tag {
            let changes = app.compose.plan_tag_change(&stack, service, tag).await?;
            for c in &changes {
                ctx.log(format!("Editing {}", c.path));
                crate::files::write_with_history(
                    &app.db,
                    std::path::Path::new(&c.path),
                    &c.after,
                    actor,
                    Some(format!("switch {service} to {tag}")),
                )
                .await?;
            }
            app.compose.invalidate(stack_name);
            files = Some(changes);
        }
        let result = app.compose.exec(&stack, &["up", "-d", "--no-deps", "--pull", "never", service], Some(ctx)).await;
        if let Err(err) = result {
            if let Some(changes) = &files {
                for c in changes {
                    let _ = c.revert(&app.db, actor, "revert failed update").await;
                }
            }
            return Err(err);
        }
        Ok(Applied::Compose { stack, service: service.clone(), files })
    } else {
        let old_cfg = app.docker.image_meta(&s.image_id).await.and_then(|m| m.config.clone());
        let body = recreate::body_from_inspect(&entry.inspect, old_cfg.as_ref(), new_ref);
        let timeout = stop_timeout(app);
        let log = |l: String| ctx.log(l);
        let rec = recreate::replace(&app.docker.api, &s.id, body, timeout, &log).await?;
        Ok(Applied::Standalone(rec))
    }
}

pub fn stop_timeout(app: &App) -> Option<u32> {
    let t = app.settings.get().docker.stop_timeout_secs;
    (t > 0).then_some(t)
}

/// Waits for the new container to settle and, when it has a web UI check,
/// to answer HTTP.
async fn verify(app: &App, ctx: &TaskCtx, entry: &Entry, new_id: &str) -> std::result::Result<(), String> {
    let settings = app.settings.get();
    let settle = Duration::from_secs(settings.updates.settle_secs as u64);
    let timeout = Duration::from_secs(settings.updates.verify_timeout_secs as u64);
    ctx.log(format!("Waiting for the container to settle ({}s)", settle.as_secs()));
    let started = tokio::time::Instant::now();
    match actions::wait_until_stable(&app.docker.api, new_id, settle, timeout, &ctx.cancel_token()).await {
        Ok(actions::Stability::Stable) => {}
        Ok(actions::Stability::Failed(msg)) | Ok(actions::Stability::TimedOut(msg)) => return Err(msg),
        Err(err) => return Err(err.to_string()),
    }
    if let Some(check) = crate::monitor::uptime::check_for(app, entry) {
        ctx.log(format!("Checking that {} answers", crate::util::redact_url(&check.url)));
        loop {
            let res = crate::monitor::uptime::probe(app, &check).await;
            if res.ok {
                ctx.log(format!("Web UI answered in {} ms", res.latency_ms));
                break;
            }
            if started.elapsed() >= timeout {
                return Err(format!("web UI did not answer: {}", res.error.unwrap_or_default()));
            }
            if ctx.is_cancelled() {
                return Err("cancelled".into());
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }
    Ok(())
}

/// Undoes an applied change after a failed verification.
async fn rollback_applied(
    app: &App,
    ctx: &TaskCtx,
    entry: &Entry,
    applied: Applied,
    rollback_tag: Option<&str>,
    old_ref: &str,
    actor: &Actor,
) -> Result<()> {
    ctx.log("Rolling back");
    match applied {
        Applied::Standalone(rec) => recreate::revert(&app.docker.api, &rec, &|l| ctx.log(l)).await,
        Applied::Compose { stack, service, files } => {
            if let Some(changes) = files {
                for c in changes {
                    c.revert(&app.db, actor, "automatic rollback").await?;
                }
                app.compose.invalidate(&stack.name);
            }
            if let Some(tag) = rollback_tag {
                retag(app, tag, old_ref).await?;
            }
            app.compose.exec(&stack, &["up", "-d", "--no-deps", "--pull", "never", &service], Some(ctx)).await?;
            let _ = entry;
            Ok(())
        }
    }
}

/// Best effort recovery when `compose up` itself failed.
async fn restore_compose_after_failure(
    app: &App,
    ctx: &TaskCtx,
    entry: &Entry,
    rollback_tag: &str,
    old_ref: &str,
    _actor: &Actor,
) -> Result<()> {
    let s = &entry.summary;
    if let (Some(stack_name), Some(service)) = (&s.stack, &s.service) {
        retag(app, rollback_tag, old_ref).await?;
        let stack = app.compose.stack_ref(stack_name)?;
        app.compose.exec(&stack, &["up", "-d", "--no-deps", "--pull", "never", service], Some(ctx)).await?;
    }
    Ok(())
}

/// Recreates containers using `network_mode: service:<this>` / `container:<this>`.
async fn recreate_network_dependents(app: &App, ctx: &TaskCtx, old: &Entry) {
    let entries = app.docker.entries();
    let deps = recreate::dependents_by_network(&entries, &old.summary.id, &old.summary.name);
    for id in deps {
        let Some(dep) = app.docker.get(&id) else { continue };
        let ds = &dep.summary;
        ctx.log(format!("Recreating {} (shares the network of {})", ds.name, old.summary.name));
        let res = if let (Some(stack), Some(service)) = (&ds.stack, &ds.service) {
            match app.compose.stack_ref(stack) {
                Ok(stack) => app
                    .compose
                    .exec(&stack, &["up", "-d", "--no-deps", "--force-recreate", "--pull", "never", service], Some(ctx))
                    .await
                    .map(|_| ()),
                Err(e) => Err(e),
            }
        } else {
            let cfg = app.docker.image_meta(&ds.image_id).await.and_then(|m| m.config.clone());
            let mut body = recreate::body_from_inspect(&dep.inspect, cfg.as_ref(), &ds.image);
            if let Some(new_target) = app.docker.by_identity(&old.summary.identity) {
                if let Some(h) = body.host_config.as_mut() {
                    h.network_mode = Some(format!("container:{}", new_target.summary.id));
                }
            }
            match recreate::replace(&app.docker.api, &ds.id, body, stop_timeout(app), &|l| ctx.log(l)).await {
                Ok(r) => recreate::finalize(&app.docker.api, &r).await,
                Err(e) => Err(e),
            }
        };
        if let Err(err) = res {
            ctx.log(format!("Could not recreate {}: {err}", ds.name));
        }
    }
}

/// Finds the current container of a compose service straight from Docker.
pub async fn find_service_container(app: &App, project: &str, service: &str) -> Option<String> {
    let mut filters = HashMap::new();
    filters
        .insert("label".to_string(), vec![format!("{LABEL_PROJECT}={project}"), format!("{LABEL_SERVICE}={service}")]);
    let list = app
        .docker
        .api
        .list_containers(Some(ListContainersOptions { all: true, filters: Some(filters), ..Default::default() }))
        .await
        .ok()?;
    let mut list: Vec<_> = list.into_iter().collect();
    // Prefer running, then newest.
    list.sort_by_key(|c| (c.state.map(|s| s.to_string() != "running").unwrap_or(true), -c.created.unwrap_or(0)));
    let id = list.first()?.id.clone()?;
    app.docker.refresh(&id).await;
    Some(id)
}

/// Pulls an image through the Docker API, logging layer progress.
pub async fn pull(app: &App, ctx: &TaskCtx, r: &ImageRef) -> Result<()> {
    let from_image = if r.is_docker_hub() {
        r.repository.trim_start_matches("library/").to_string()
    } else {
        format!("{}/{}", r.registry, r.repository)
    };
    let opts = CreateImageOptions {
        from_image: Some(from_image),
        tag: Some(r.tag_or_latest().to_string()),
        ..Default::default()
    };
    let creds = app.registry.docker_credentials(r);
    let mut stream = app.docker.api.create_image(Some(opts), None, creds);
    let mut layers: HashMap<String, (i64, i64)> = HashMap::new();
    let mut last_status: HashMap<String, String> = HashMap::new();
    while let Some(item) = stream.next().await {
        if ctx.is_cancelled() {
            return Err(AppError::conflict("cancelled"));
        }
        let info = item?;
        if let Some(err) = info.error_detail.and_then(|d| d.message) {
            return Err(AppError::conflict(format!("pull failed: {err}")));
        }
        let status = info.status.unwrap_or_default();
        let id = info.id.unwrap_or_default();
        if let Some(p) = info.progress_detail {
            if let (Some(cur), Some(total)) = (p.current, p.total) {
                if total > 0 && status.starts_with("Downloading") {
                    layers.insert(id.clone(), (cur, total));
                    let (c, t) = layers.values().fold((0i64, 0i64), |acc, (c, t)| (acc.0 + c, acc.1 + t));
                    if t > 0 {
                        ctx.progress(0.2 + 0.35 * (c as f32 / t as f32));
                    }
                    continue;
                }
            }
        }
        // Log status changes only (not every progress tick).
        if status.starts_with("Downloading") || status.starts_with("Extracting") || status.starts_with("Waiting") {
            continue;
        }
        if last_status.get(&id) != Some(&status) {
            last_status.insert(id.clone(), status.clone());
            if id.is_empty() {
                ctx.log(status);
            } else if status.contains("complete")
                || status.contains("Already exists")
                || status.starts_with("Pulling fs")
            {
                ctx.log(format!("{id}: {status}"));
            }
        }
    }
    Ok(())
}

fn rollback_repo(identity: &str) -> String {
    format!("{ROLLBACK_REPO_PREFIX}{}", crate::util::slug(identity))
}

/// Tags the image a container runs so it survives pulls and pruning.
async fn pin_image(app: &App, identity: &str, image_id: &str) -> Result<String> {
    let repo = rollback_repo(identity);
    let tag = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
    app.docker
        .api
        .tag_image(image_id, Some(TagImageOptions { repo: Some(repo.clone()), tag: Some(tag.clone()) }))
        .await?;
    Ok(format!("{repo}:{tag}"))
}

async fn unpin(app: &App, tag: Option<&str>) {
    if let Some(t) = tag {
        let _ = app
            .docker
            .api
            .remove_image(t, Some(RemoveImageOptions { force: false, noprune: false, platforms: None }), None)
            .await;
    }
}

/// Points `reference` (e.g. "nginx:1.27") back at a pinned rollback image.
async fn retag(app: &App, rollback_tag: &str, reference: &str) -> Result<()> {
    let r = ImageRef::parse(reference).ok_or_else(|| AppError::bad_request("invalid image reference"))?;
    let repo = if r.is_docker_hub() {
        r.repository.trim_start_matches("library/").to_string()
    } else {
        format!("{}/{}", r.registry, r.repository)
    };
    app.docker
        .api
        .tag_image(rollback_tag, Some(TagImageOptions { repo: Some(repo), tag: Some(r.tag_or_latest().to_string()) }))
        .await?;
    Ok(())
}

/// Keeps only the newest N rollback images per container and prunes
/// dangling images when enabled.
async fn cleanup_images(app: &App, ctx: &TaskCtx, identity: &str) {
    let settings = app.settings.get();
    let keep = settings.updates.keep_rollback_images as usize;
    let repo = rollback_repo(identity);
    if let Ok(images) = app.docker.api.list_images(Some(ListImagesOptions { all: false, ..Default::default() })).await {
        let mut tags: Vec<String> = images
            .iter()
            .flat_map(|i| i.repo_tags.iter())
            .filter(|t| t.starts_with(&format!("{repo}:")))
            .cloned()
            .collect();
        tags.sort();
        tags.reverse();
        for old in tags.into_iter().skip(keep) {
            if app
                .docker
                .api
                .remove_image(&old, Some(RemoveImageOptions { force: false, noprune: false, platforms: None }), None)
                .await
                .is_ok()
            {
                ctx.log(format!("Removed old rollback image {old}"));
            }
        }
    }
    if settings.updates.prune_after_update {
        let mut filters = HashMap::new();
        filters.insert("dangling".to_string(), vec!["true".to_string()]);
        match app.docker.api.prune_images(Some(PruneImagesOptions { filters: Some(filters) })).await {
            Ok(r) => {
                let freed = r.space_reclaimed.unwrap_or(0);
                if freed > 0 {
                    ctx.log(format!("Pruned unused images, freed {}", crate::util::human_bytes(freed as u64)));
                }
            }
            Err(err) => ctx.log(format!("Image prune failed: {err}")),
        }
    }
    app.docker.invalidate_images();
}

#[allow(clippy::too_many_arguments)]
async fn record_history(
    app: &App,
    identity: &str,
    name: &str,
    from_ref: &str,
    to_ref: &str,
    from_version: Option<&str>,
    to_version: Option<&str>,
    rollback_tag: Option<&str>,
    compose_before: Option<String>,
    status: &str,
    trigger: Trigger,
    task_id: &str,
    message: &str,
) {
    let values = (
        identity.to_string(),
        name.to_string(),
        from_ref.to_string(),
        to_ref.to_string(),
        from_version.map(str::to_string),
        to_version.map(str::to_string),
        rollback_tag.map(str::to_string),
        compose_before,
        status.to_string(),
        trigger.as_str(),
        task_id.to_string(),
        message.to_string(),
    );
    let res = app
        .updates
        .db()
        .call(move |c| {
            let v = values;
            c.execute(
                "INSERT INTO update_history (identity, target_name, ts, from_ref, to_ref, from_version, to_version, rollback_tag, compose_before, status, trigger, task_id, message)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![v.0, v.1, crate::util::now(), v.2, v.3, v.4, v.5, v.6, v.7, v.8, v.9, v.10, if v.11.is_empty() { None } else { Some(v.11) }],
            )?;
            Ok(())
        })
        .await;
    if let Err(err) = res {
        tracing::warn!(%err, "failed to record update history");
    }
}

fn notify_failure(app: &App, name: &str, identity: &str, detail: &str, rolled_back: bool) {
    let (kind, title) = if rolled_back {
        ("update.rolled_back", format!("{name} update rolled back"))
    } else {
        ("update.failed", format!("{name} update failed"))
    };
    app.notifier.send(Notification::new(kind, Severity::Error, title, detail.to_string()).target(identity, name));
}

// ---------------------------------------------------------------------------
// Manual rollback
// ---------------------------------------------------------------------------

/// Starts a rollback to the image recorded in an update history entry
/// (the most recent applied update when `history_id` is None).
pub fn start_rollback(app: &App, key: &str, history_id: Option<i64>, actor: &Actor) -> Result<String> {
    let entry = app.docker.get(key).ok_or_else(|| AppError::not_found("container"))?;
    if entry.summary.is_self {
        return Err(AppError::conflict("Nodestarr can't roll itself back; use Settings > System instead"));
    }
    let identity = entry.summary.identity.clone();
    let name = display_name(app, &entry);
    let spec = TaskSpec::new("rollback", format!("Roll back {name}"), actor).target(identity.clone(), name);
    let app2 = app.clone();
    let actor = actor.clone();
    Ok(app.tasks.spawn(spec, move |ctx| async move { run_rollback(&app2, &ctx, &identity, history_id, &actor).await }))
}

async fn run_rollback(
    app: &App,
    ctx: &TaskCtx,
    identity: &str,
    history_id: Option<i64>,
    actor: &Actor,
) -> Result<serde_json::Value> {
    struct Row {
        id: i64,
        from_ref: String,
        to_ref: String,
        rollback_tag: Option<String>,
        compose_before: Option<String>,
        from_version: Option<String>,
    }
    let id_owned = identity.to_string();
    let row: Option<Row> = app
        .updates
        .db()
        .call(move |c| {
            use rusqlite::OptionalExtension;
            let sql = "SELECT id, from_ref, to_ref, rollback_tag, compose_before, from_version FROM update_history
                       WHERE identity = ?1 AND status = 'applied' AND rollback_tag IS NOT NULL AND (?2 IS NULL OR id = ?2)
                       ORDER BY ts DESC, id DESC LIMIT 1";
            Ok(c.query_row(sql, params![id_owned, history_id], |r| {
                Ok(Row {
                    id: r.get(0)?,
                    from_ref: r.get(1)?,
                    to_ref: r.get(2)?,
                    rollback_tag: r.get(3)?,
                    compose_before: r.get(4)?,
                    from_version: r.get(5)?,
                })
            })
            .optional()?)
        })
        .await?;
    let Row { id: hid, from_ref, to_ref, rollback_tag, compose_before, from_version } =
        row.ok_or_else(|| AppError::not_found("an update to roll back"))?;
    let rollback_tag = rollback_tag.ok_or_else(|| AppError::conflict("no rollback image was kept for that update"))?;
    if app.docker.api.inspect_image(&rollback_tag).await.is_err() {
        return Err(AppError::conflict(format!("the rollback image {rollback_tag} no longer exists")));
    }
    let entry = app.docker.by_identity(identity).ok_or_else(|| AppError::not_found("container"))?;
    let name = display_name(app, &entry);
    ctx.log(format!("Rolling {name} back to {from_ref}"));
    retag(app, &rollback_tag, &from_ref).await?;
    let s = &entry.summary;
    let new_id = if let (Some(stack_name), Some(service)) = (&s.stack, &s.service) {
        if let Some(json_files) = compose_before {
            if let Ok(changes) = serde_json::from_str::<Vec<FileChange>>(&json_files) {
                for c in changes {
                    ctx.log(format!("{} {}", if c.created { "Removing" } else { "Restoring" }, c.path));
                    c.revert(&app.db, actor, "rollback").await?;
                }
            }
            app.compose.invalidate(stack_name);
        }
        let stack = app.compose.stack_ref(stack_name)?;
        app.compose.exec(&stack, &["up", "-d", "--no-deps", "--pull", "never", service], Some(ctx)).await?;
        find_service_container(app, stack_name, service).await.unwrap_or_default()
    } else {
        let cfg = app.docker.image_meta(&s.image_id).await.and_then(|m| m.config.clone());
        let body = recreate::body_from_inspect(&entry.inspect, cfg.as_ref(), &from_ref);
        let rec = recreate::replace(&app.docker.api, &s.id, body, stop_timeout(app), &|l| ctx.log(l)).await?;
        recreate::finalize(&app.docker.api, &rec).await?;
        rec.new_id
    };
    if !new_id.is_empty() {
        if let Err(reason) = verify(app, ctx, &entry, &new_id).await {
            ctx.log(format!("Warning: the rolled back container is not healthy: {reason}"));
        }
        app.docker.refresh(&new_id).await;
    }
    let latest = app.updates.get(identity).and_then(|i| i.latest_digest.clone());
    let _ = app
        .prefs
        .update(identity, |p| {
            if let Some(d) = latest {
                p.update.skipped_digest = d;
            }
        })
        .await;
    let task_id = ctx.id().to_string();
    let (id2, name2, from2, to2, fv) =
        (identity.to_string(), name.clone(), from_ref.clone(), to_ref.clone(), from_version.clone());
    app.updates
        .db()
        .call(move |c| {
            c.execute("UPDATE update_history SET status = 'reverted' WHERE id = ?1", [hid])?;
            c.execute(
                "INSERT INTO update_history (identity, target_name, ts, from_ref, to_ref, to_version, status, trigger, task_id, message)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'rolled_back', 'manual', ?7, 'manual rollback')",
                params![id2, name2, crate::util::now(), to2, from2, fv, task_id],
            )?;
            Ok(())
        })
        .await?;
    if let Some(e) = app.docker.by_identity(identity) {
        let info = crate::updates::check::check_one(app, &e, false).await;
        app.updates.set(identity, info).await;
    }
    app.note(
        NewActivity::new("update.rolled_back", Severity::Warning, format!("{name} rolled back to {from_ref}"))
            .target(identity, &name)
            .actor(actor),
    )
    .await;
    app.notifier.send(
        Notification::new(
            "update.rolled_back",
            Severity::Warning,
            format!("{name} rolled back"),
            format!("Now running {from_ref}"),
        )
        .target(identity, &name),
    );
    Ok(json!({ "rolled_back_to": from_ref }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_validation() {
        assert!(validate_tag(Some("develop")).is_ok());
        assert!(validate_tag(Some("4.0.9.2244-ls250")).is_ok());
        assert!(validate_tag(Some("v1_2")).is_ok());
        assert!(validate_tag(Some("-bad")).is_err());
        assert!(validate_tag(Some("a b")).is_err());
        assert!(validate_tag(Some("")).is_err());
        assert!(validate_tag(None).is_ok());
    }

    #[test]
    fn rollback_repo_names_are_valid_references() {
        let r = rollback_repo("compose:media/sonarr");
        assert_eq!(r, "nodestarr-rollback/compose-media-sonarr");
        assert!(ImageRef::parse(&format!("{r}:20241014120000")).is_some());
    }
}
