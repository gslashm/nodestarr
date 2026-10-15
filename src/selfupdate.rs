//! Updating Nodestarr's own container.
//!
//! A process can't replace the container it runs in, so Nodestarr pulls the new
//! image and starts a short-lived helper container from it. The helper
//! recreates the Nodestarr container with the new image, checks that it comes
//! up, and removes itself (rolling back if the new version doesn't start).

use std::time::Duration;

use bollard::models::{ContainerCreateBody, HostConfig};
use bollard::query_parameters::{CreateContainerOptions, InspectContainerOptions};
use serde_json::json;

use crate::activity::{Actor, NewActivity};
use crate::app::App;
use crate::config::BootConfig;
use crate::docker::{Entry, actions, recreate};
use crate::error::{AppError, Result};
use crate::notify::Notification;
use crate::registry::ImageRef;
use crate::settings::Severity;
use crate::tasks::TaskCtx;

/// GitHub repository ("owner/name") that publishes Nodestarr releases, used for
/// "new Nodestarr version" checks and release notes. Set at build time with
/// NODESTARR_REPO; builds without it skip those checks.
pub const REPO: Option<&str> = match option_env!("NODESTARR_REPO") {
    Some(r) if !r.is_empty() => Some(r),
    _ => None,
};

pub async fn start(app: &App, ctx: &TaskCtx, entry: &Entry, tag: Option<&str>) -> Result<serde_json::Value> {
    let s = &entry.summary;
    let new_ref = match tag {
        Some(t) => match crate::compose::yaml_edit::plan_tag_edit(&s.image, t).map_err(AppError::BadRequest)? {
            crate::compose::yaml_edit::TagEdit::Literal(v) => v,
            _ => return Err(AppError::bad_request("unsupported image reference")),
        },
        None => s.image.clone(),
    };
    let target = ImageRef::parse(&new_ref).ok_or_else(|| AppError::bad_request("invalid image"))?;
    ctx.log(format!("Pulling {}", target.familiar()));
    crate::updates::apply::pull(app, ctx, &target).await?;
    let pulled = app
        .docker
        .inspect_image_fresh(&new_ref)
        .await
        .ok_or_else(|| AppError::conflict("image missing after pull"))?;
    if pulled.id == s.image_id && tag.is_none() {
        ctx.log("Nodestarr is already up to date");
        return Ok(json!({ "changed": false }));
    }

    // The helper needs the same Docker socket mount as Nodestarr.
    let socket_bind = s
        .mounts
        .iter()
        .find(|m| m.destination.ends_with("docker.sock"))
        .map(|m| format!("{}:{}", m.source, m.destination))
        .ok_or_else(|| AppError::conflict("can't find Nodestarr's Docker socket mount for the update helper"))?;
    let docker_host = std::env::var("DOCKER_HOST").unwrap_or_else(|_| "unix:///var/run/docker.sock".into());
    let name = format!("nodestarr-updater-{}", chrono::Utc::now().format("%Y%m%d%H%M%S"));
    let body = ContainerCreateBody {
        image: Some(new_ref.clone()),
        entrypoint: Some(vec!["/usr/local/bin/nodestarr".into()]),
        cmd: Some(vec![
            "self-update-helper".into(),
            "--container".into(),
            s.id.clone(),
            "--image".into(),
            new_ref.clone(),
        ]),
        env: Some(vec![format!("DOCKER_HOST={docker_host}")]),
        labels: Some(std::collections::HashMap::from([("nodestarr.helper".to_string(), "true".to_string())])),
        host_config: Some(HostConfig {
            binds: Some(vec![socket_bind]),
            auto_remove: Some(true),
            network_mode: Some("none".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let created = app
        .docker
        .api
        .create_container(Some(CreateContainerOptions { name: Some(name.clone()), platform: String::new() }), body)
        .await?;
    app.note(
        NewActivity::new("system.update", Severity::Info, format!("Updating Nodestarr to {}", target.familiar()))
            .actor(&Actor::system()),
    )
    .await;
    ctx.log("Starting the update helper; Nodestarr will restart in a few seconds");
    ctx.flush();
    actions::start(&app.docker.api, &created.id).await?;
    Ok(json!({ "changed": true, "helper": name }))
}

/// `nodestarr self-update-helper`: runs inside the helper container.
pub async fn run_helper(boot: &BootConfig, container: &str, image: &str) -> anyhow::Result<()> {
    crate::app::init_tls();
    println!("nodestarr update helper: waiting for Nodestarr to finish its request");
    tokio::time::sleep(Duration::from_secs(3)).await;
    let api = crate::docker::connect(&boot.docker_host).await?;
    let inspect = api.inspect_container(container, None::<InspectContainerOptions>).await?;
    let old_image = inspect.image.clone().unwrap_or_default();
    let old_cfg = api.inspect_image(&old_image).await.ok().and_then(|i| i.config);
    let body = recreate::body_from_inspect(&inspect, old_cfg.as_ref(), image);
    let log = |l: String| println!("{l}");
    let rec = recreate::replace(&api, container, body, Some(30), &log).await?;
    let cancel = tokio_util::sync::CancellationToken::new();
    match actions::wait_until_stable(&api, &rec.new_id, Duration::from_secs(10), Duration::from_secs(120), &cancel).await {
        Ok(actions::Stability::Stable) => {
            println!("new Nodestarr container is up; removing the old one");
            recreate::finalize(&api, &rec).await?;
            Ok(())
        }
        other => {
            println!("new Nodestarr container did not come up ({other:?}); restoring the previous version");
            recreate::revert(&api, &rec, &log).await?;
            anyhow::bail!("update failed, previous version restored")
        }
    }
}

/// Notifies once per release when a newer Nodestarr version is published.
pub async fn check_for_release(app: &App) {
    let Some(repo) = REPO else { return };
    let Ok(resp) = app
        .http
        .get(format!("https://api.github.com/repos/{repo}/releases/latest"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    else {
        return;
    };
    let Ok(v) = resp.json::<serde_json::Value>().await else { return };
    let Some(tag) = v["tag_name"].as_str() else { return };
    let (Some(latest), Some(current)) = (
        crate::registry::version::Version::parse(tag),
        crate::registry::version::Version::parse(crate::VERSION),
    ) else {
        return;
    };
    let newer = latest.core > current.core;
    let key = format!("notified:{tag}");
    let already: Option<String> = app.db.get_json("settings", "nodestarr_release".into()).await.ok().flatten();
    if newer && already.as_deref() != Some(key.as_str()) {
        app.notifier.send(
            Notification::new("system.update", Severity::Info, format!("Nodestarr {tag} is available"), format!("You run {}. Update from Settings > System.", crate::VERSION))
                .link(app, "/system"),
        );
        let _ = app.db.put_json("settings", "nodestarr_release".into(), &key).await;
    }
}
