//! Registry checks.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures_util::{StreamExt, stream};

use crate::activity::{Actor, NewActivity};
use crate::app::App;
use crate::docker::Entry;
use crate::notify::Notification;
use crate::registry::ImageRef;
use crate::registry::client::Platform;
use crate::registry::version::{Version, newest_within};
use crate::settings::{Severity, UpdatePolicy};
use crate::updates::{NewerTag, UpdateInfo, UpdateStatus};

/// Checks one container. `fresh` bypasses the short registry cache.
pub async fn check_one(app: &App, entry: &Entry, fresh: bool) -> UpdateInfo {
    let s = &entry.summary;
    let settings = app.settings.get();
    let prefs = app.prefs.get(&s.identity);
    let previous = app.updates.get(&s.identity);
    let mut info = UpdateInfo {
        image: s.image.clone(),
        checked_at: Some(crate::util::now()),
        ..Default::default()
    };

    if prefs.update_policy(&settings) == UpdatePolicy::Ignore {
        info.status = UpdateStatus::Ignored;
        return info;
    }
    let Some(reference) = ImageRef::parse(&s.image).filter(|_| !s.image.starts_with("sha256:")) else {
        info.status = UpdateStatus::Local;
        return info;
    };
    if reference.digest.is_some() {
        info.status = UpdateStatus::Pinned;
        return info;
    }

    let meta = app.docker.image_meta(&s.image_id).await;
    info.current_digest = meta
        .as_ref()
        .and_then(|m| m.repo_digests.iter().find_map(|rd| reference.digest_from_repo_digest(rd)));
    if meta.as_ref().is_some_and(|m| m.repo_digests.is_empty()) {
        // Built locally (no registry digest at all).
        info.status = UpdateStatus::Local;
        return info;
    }
    let platform = meta
        .as_ref()
        .map(|m| Platform {
            os: m.os.clone(),
            arch: m.arch.clone(),
            variant: m.variant.clone(),
        })
        .unwrap_or_default();

    // A newer image already pulled for this tag but not applied yet?
    let local_tag = app.docker.inspect_image_fresh(&s.image).await;
    let pending = local_tag.as_ref().is_some_and(|t| !t.id.is_empty() && t.id != s.image_id);

    match app.registry.digest(&reference, fresh).await {
        Ok(remote) => {
            info.latest_digest = Some(remote.clone());
            let same_as_running = info.current_digest.as_deref() == Some(remote.as_str());
            if !same_as_running {
                // Remember when this digest first appeared (minimum-age rule).
                info.first_seen = previous
                    .as_ref()
                    .filter(|p| p.latest_digest.as_deref() == Some(remote.as_str()))
                    .and_then(|p| p.first_seen)
                    .or(Some(crate::util::now()));
                match app.registry.remote_image(&reference, &platform).await {
                    Ok(r) => {
                        info.latest_version = r.version;
                        info.latest_created = r.created;
                        info.source = r.source;
                    }
                    Err(err) => tracing::debug!(%err, image = %s.image, "could not read remote image details"),
                }
            }
            info.status = if pending {
                UpdateStatus::PendingRecreate
            } else if same_as_running {
                UpdateStatus::UpToDate
            } else if info.current_digest.is_none() {
                // Can't tell which build is running (e.g. pulled under another
                // name); report available only if the local tag differs.
                match &local_tag {
                    Some(t) if t.repo_digests.iter().any(|d| d.ends_with(&remote)) => UpdateStatus::UpToDate,
                    _ => UpdateStatus::Available,
                }
            } else {
                UpdateStatus::Available
            };
        }
        Err(crate::registry::client::RegistryError::NotFound) => {
            info.status = if pending { UpdateStatus::PendingRecreate } else { UpdateStatus::Local };
        }
        Err(err) => {
            info.status = UpdateStatus::Error;
            info.error = Some(err.to_string());
            if let Some(p) = &previous {
                info.latest_digest = p.latest_digest.clone();
                info.latest_version = p.latest_version.clone();
                info.first_seen = p.first_seen;
            }
            return info;
        }
    }

    // Version-pinned tags: look for newer versions of the same line.
    if let Some(current) = reference.tag.as_deref().and_then(Version::parse) {
        match app.registry.tags(&reference, fresh).await {
            Ok(tags) => {
                let names = tags.iter().map(|t| t.name.as_str());
                if let Some(newer) = newest_within(&current, names, settings.updates.notify_scope, !settings.updates.ignore_prereleases) {
                    info.newer_tag = Some(NewerTag {
                        scope: current.change_scope(&newer),
                        tag: newer.raw,
                    });
                    if info.status == UpdateStatus::UpToDate {
                        info.status = UpdateStatus::Available;
                    }
                }
            }
            Err(err) => tracing::debug!(%err, image = %s.image, "could not list tags"),
        }
    }
    if info.source.is_none() {
        info.source = meta.as_ref().and_then(|m| m.source_url().map(str::to_string));
    }
    info
}

/// Summary of a full check run.
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct CheckSummary {
    pub checked: usize,
    pub available: usize,
    pub errors: usize,
    /// Containers whose update appeared during this run.
    pub new: Vec<String>,
}

/// Checks every container (each distinct image is only asked once).
pub async fn check_all(app: &App, fresh: bool, actor: &Actor) -> CheckSummary {
    if app.updates.is_checking() {
        return CheckSummary::default();
    }
    app.updates.set_checking(true);
    if fresh {
        app.registry.forget_digests();
    }
    let entries: Vec<Arc<Entry>> = app.docker.entries();
    let live: HashSet<String> = entries.iter().map(|e| e.summary.identity.clone()).collect();

    let results: Vec<(Arc<Entry>, UpdateInfo)> = stream::iter(entries)
        .map(|e| {
            let app = app.clone();
            async move {
                let info = check_one(&app, &e, false).await;
                (e, info)
            }
        })
        .buffer_unordered(4)
        .collect()
        .await;

    let mut summary = CheckSummary::default();
    let mut new_lines = Vec::new();
    for (entry, info) in results {
        let s = &entry.summary;
        let before = app.updates.get(&s.identity);
        summary.checked += 1;
        if info.status == UpdateStatus::Error {
            summary.errors += 1;
        }
        if info.has_update() {
            summary.available += 1;
            let was_known = before.as_ref().is_some_and(|b| {
                b.has_update() && b.latest_digest == info.latest_digest && b.newer_tag == info.newer_tag
            });
            if !was_known {
                let name = display_name(app, &entry);
                summary.new.push(name.clone());
                new_lines.push(describe_update(&name, &info));
            }
        }
        app.updates.set(&s.identity, info).await;
    }
    app.updates.forget_missing(&live).await;
    app.updates.set_checking(false);

    if !new_lines.is_empty() {
        let settings = app.settings.get();
        let title = if new_lines.len() == 1 {
            "Update available".to_string()
        } else {
            format!("{} updates available", new_lines.len())
        };
        if settings.updates.notify_available {
            app.notifier.send(
                Notification::new("update.available", Severity::Info, title.clone(), new_lines.join("\n"))
                    .link(app, "/updates"),
            );
        }
        app.note(NewActivity::new("update.available", Severity::Info, format!("{title}: {}", summary.new.join(", "))).actor(actor))
            .await;
    }
    tracing::info!(checked = summary.checked, available = summary.available, errors = summary.errors, "update check finished");
    summary
}

pub fn display_name(app: &App, entry: &Entry) -> String {
    let p = app.prefs.get(&entry.summary.identity);
    if p.display_name.is_empty() { entry.summary.default_name() } else { p.display_name.clone() }
}

fn describe_update(name: &str, info: &UpdateInfo) -> String {
    let mut parts = Vec::new();
    if info.digest_changed() || info.status == UpdateStatus::PendingRecreate {
        match &info.latest_version {
            Some(v) => parts.push(format!("new build {v}")),
            None => parts.push("new build".to_string()),
        }
    }
    if let Some(t) = &info.newer_tag {
        parts.push(format!("newer tag {}", t.tag));
    }
    if parts.is_empty() {
        parts.push("update".into());
    }
    format!("{name}: {}", parts.join(", "))
}

/// Maps identities to info for quick lookups in views.
pub fn index(app: &App) -> HashMap<String, Arc<UpdateInfo>> {
    app.updates.all()
}
