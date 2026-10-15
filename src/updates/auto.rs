//! Scheduled automatic updates.

use serde_json::json;

use crate::activity::{Actor, NewActivity};
use crate::app::App;
use crate::notify::Notification;
use crate::registry::ImageRef;
use crate::settings::{Severity, TagScope, UpdatePolicy};
use crate::tasks::TaskSpec;
use crate::updates::apply::{Trigger, UpdateRequest};
use crate::updates::check::{check_one, display_name};
use crate::updates::{UpdateInfo, UpdateStatus};

/// Why a container was or wasn't auto-updated.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Update(UpdateRequest),
    Skip(String),
}

/// Decides whether one container should be auto-updated right now.
pub fn decide(
    policy: UpdatePolicy,
    scope: TagScope,
    held: bool,
    skipped_digest: &str,
    min_age_hours: u32,
    info: &UpdateInfo,
    now: i64,
) -> Decision {
    if policy != UpdatePolicy::Auto {
        return Decision::Skip("policy is not auto".into());
    }
    if held {
        return Decision::Skip("auto-updates paused for this container".into());
    }
    if !info.has_update() {
        return Decision::Skip("no update".into());
    }
    // A newer version tag within the allowed scope takes precedence.
    if let Some(newer) = &info.newer_tag
        && scope != TagScope::Digest
        && newer.scope <= scope
    {
        if let Some(wait) = too_new(newer.first_seen, min_age_hours, now) {
            return Decision::Skip(wait);
        }
        return Decision::Update(UpdateRequest { tag: Some(newer.tag.clone()), skip_backup: false });
    }
    let digest_update = info.digest_changed() || info.status == UpdateStatus::PendingRecreate;
    if !digest_update {
        return Decision::Skip("only a newer tag outside the allowed scope".into());
    }
    if !skipped_digest.is_empty() && info.latest_digest.as_deref() == Some(skipped_digest) {
        return Decision::Skip("this version was skipped (it failed before or was rolled back)".into());
    }
    if let Some(wait) = too_new(info.first_seen, min_age_hours, now) {
        return Decision::Skip(wait);
    }
    Decision::Update(UpdateRequest::default())
}

/// The reason to wait when an update first seen at `first_seen` is younger
/// than the minimum age, or None when it may be installed.
fn too_new(first_seen: Option<i64>, min_age_hours: u32, now: i64) -> Option<String> {
    if min_age_hours == 0 {
        return None;
    }
    let age = now - first_seen.unwrap_or(now);
    (age < min_age_hours as i64 * 3600)
        .then(|| format!("waiting until the update is {min_age_hours}h old ({}h so far)", age / 3600))
}

/// Runs one auto-update pass (scheduled).
pub async fn run(app: &App) {
    let settings = app.settings.get();
    let actor = Actor::auto_update();
    let mut entries = app.docker.entries();
    // Nodestarr updates itself last, since that restarts this process.
    entries.sort_by_key(|e| (e.summary.is_self, e.summary.stack.clone(), e.summary.name.clone()));

    let mut updated = Vec::new();
    let mut failed = Vec::new();
    let mut deferred = Vec::new();
    for entry in entries {
        let s = &entry.summary;
        let prefs = app.prefs.get(&s.identity);
        if prefs.update_policy(&settings) != UpdatePolicy::Auto {
            continue;
        }
        // Re-check right before deciding so stale info never triggers an update.
        let info = check_one(app, &entry, true).await;
        app.updates.set(&s.identity, info.clone()).await;
        let decision = decide(
            UpdatePolicy::Auto,
            prefs.auto_scope(&settings),
            prefs.update.held,
            &prefs.update.skipped_digest,
            prefs.min_age_hours(&settings),
            &info,
            crate::util::now(),
        );
        let name = display_name(app, &entry);
        let req = match decision {
            Decision::Update(req) => req,
            Decision::Skip(reason) => {
                if info.has_update() {
                    tracing::info!(container = %name, %reason, "auto-update skipped");
                }
                continue;
            }
        };
        if settings.updates.plex_guard
            && s.app.as_deref().and_then(crate::docker::apps::by_slug).is_some_and(|a| a.streaming_server)
            && let Some(n) = crate::integrations::active_streams(app).await.filter(|n| *n > 0)
        {
            deferred.push(format!("{name} ({n} active stream{})", if n == 1 { "" } else { "s" }));
            continue;
        }
        let identity = s.identity.clone();
        let title = match &req.tag {
            Some(t) => format!("Auto-update {name} to {t}"),
            None => format!("Auto-update {name}"),
        };
        let spec = TaskSpec::new("update", title, &actor).target(identity.clone(), name.clone());
        let app2 = app.clone();
        let actor2 = actor.clone();
        let result = app
            .tasks
            .run(spec, move |ctx| async move {
                crate::updates::apply::run_update(&app2, &ctx, &identity, req, &actor2, Trigger::Auto).await
            })
            .await;
        match result {
            Ok(v) if v.get("changed").and_then(|c| c.as_bool()) == Some(false) => {}
            Ok(_) => updated.push(name),
            Err(err) => failed.push(format!("{name}: {err}")),
        }
    }

    if updated.is_empty() && failed.is_empty() && deferred.is_empty() {
        return;
    }
    let mut lines = Vec::new();
    if !updated.is_empty() {
        lines.push(format!("Updated: {}", updated.join(", ")));
    }
    if !failed.is_empty() {
        lines.push(format!("Failed: {}", failed.join("; ")));
    }
    if !deferred.is_empty() {
        lines.push(format!("Deferred while streaming: {}", deferred.join(", ")));
    }
    let severity = if failed.is_empty() { Severity::Success } else { Severity::Warning };
    let message = lines.join("\n");
    app.note(
        NewActivity::new("update.auto_summary", severity, format!("Auto-update run: {}", lines.join(" · ")))
            .actor(&actor)
            .details(json!({ "updated": updated, "failed": failed, "deferred": deferred })),
    )
    .await;
    app.notifier
        .send(Notification::new("update.auto_summary", severity, "Automatic updates", message).link(app, "/updates"));
}

/// Image references that are safe for automatic tag bumps (never digests).
pub fn bumpable(image: &str) -> bool {
    ImageRef::parse(image).is_some_and(|r| r.digest.is_none() && r.tag.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updates::NewerTag;

    fn info(status: UpdateStatus, current: &str, latest: &str, first_seen: i64) -> UpdateInfo {
        UpdateInfo {
            status,
            current_digest: Some(current.into()),
            latest_digest: Some(latest.into()),
            first_seen: Some(first_seen),
            ..Default::default()
        }
    }

    #[test]
    fn decisions() {
        let now = 1_000_000;
        let avail = info(UpdateStatus::Available, "a", "b", now - 10 * 3600);
        assert_eq!(
            decide(UpdatePolicy::Notify, TagScope::Digest, false, "", 0, &avail, now),
            Decision::Skip("policy is not auto".into())
        );
        assert!(matches!(decide(UpdatePolicy::Auto, TagScope::Digest, false, "", 6, &avail, now), Decision::Update(_)));
        assert!(matches!(decide(UpdatePolicy::Auto, TagScope::Digest, false, "", 24, &avail, now), Decision::Skip(_)));
        assert!(matches!(decide(UpdatePolicy::Auto, TagScope::Digest, false, "b", 0, &avail, now), Decision::Skip(_)));
        assert!(matches!(decide(UpdatePolicy::Auto, TagScope::Digest, true, "", 0, &avail, now), Decision::Skip(_)));
        let up = info(UpdateStatus::UpToDate, "a", "a", now);
        assert!(matches!(decide(UpdatePolicy::Auto, TagScope::Digest, false, "", 0, &up, now), Decision::Skip(_)));
    }

    #[test]
    fn tag_bumps_respect_scope() {
        let now = 1_000_000;
        let mut i = info(UpdateStatus::Available, "a", "a", now);
        i.newer_tag = Some(NewerTag { tag: "8.5.0".into(), scope: TagScope::Minor, first_seen: Some(now - 10 * 3600) });
        assert!(matches!(decide(UpdatePolicy::Auto, TagScope::Patch, false, "", 0, &i, now), Decision::Skip(_)));
        match decide(UpdatePolicy::Auto, TagScope::Minor, false, "", 6, &i, now) {
            Decision::Update(req) => assert_eq!(req.tag.as_deref(), Some("8.5.0")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn tag_bumps_wait_for_the_minimum_age() {
        let now = 1_000_000;
        let mut i = info(UpdateStatus::Available, "a", "a", now);
        i.newer_tag = Some(NewerTag { tag: "8.5.0".into(), scope: TagScope::Minor, first_seen: Some(now - 3600) });
        match decide(UpdatePolicy::Auto, TagScope::Minor, false, "", 6, &i, now) {
            Decision::Skip(why) => assert!(why.contains("6h old"), "{why}"),
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(decide(UpdatePolicy::Auto, TagScope::Minor, false, "", 1, &i, now), Decision::Update(_)));
    }
}
