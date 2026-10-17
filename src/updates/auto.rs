//! Scheduled automatic updates.

use serde_json::json;

use crate::activity::{Actor, NewActivity};
use crate::app::App;
use crate::notify::Notification;
use crate::prefs::UpdatePrefs;
use crate::registry::ImageRef;
use crate::settings::{AutoInstall, Severity, UpdatePolicy};
use crate::tasks::TaskSpec;
use crate::updates::apply::{Trigger, UpdateRequest};
use crate::updates::check::{check_one, display_name};
use crate::updates::classify::{self, Change};
use crate::updates::{UpdateInfo, UpdateStatus};

/// Why a container was or wasn't auto-updated.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Update(UpdateRequest),
    Skip(String),
}

/// Decides whether one container should be auto-updated right now: the
/// newer tag when one is offered and allowed, otherwise the new build of
/// its tag when that is allowed. The UI mirrors this (`autoPlan` in
/// `web/src/lib/updates.ts`) to say what will happen to each update.
pub fn decide(
    policy: UpdatePolicy,
    rules: &AutoInstall,
    prefs: &UpdatePrefs,
    min_age_hours: u32,
    info: &UpdateInfo,
    now: i64,
) -> Decision {
    if policy != UpdatePolicy::Auto {
        return Decision::Skip("policy is not auto".into());
    }
    if prefs.held {
        return Decision::Skip("auto-updates paused for this container".into());
    }
    if !info.has_update() {
        return Decision::Skip("no update".into());
    }
    let mut blocked = None;
    if let Some(newer) = &info.newer_tag {
        // State saved before update types has no change; the tag scope says as much.
        let change = info.change.clone().unwrap_or_else(|| Change { bump: newer.scope.into(), ..Default::default() });
        match classify::blocked_by(rules, &change, true) {
            None => {
                if let Some(wait) = too_new(newer.first_seen, min_age_hours, now) {
                    return Decision::Skip(wait);
                }
                return Decision::Update(UpdateRequest { tag: Some(newer.tag.clone()), skip_backup: false });
            }
            Some(why) => blocked = Some(why),
        }
    }
    let digest_update = info.digest_changed() || info.status == UpdateStatus::PendingRecreate;
    if !digest_update {
        return Decision::Skip(blocked.unwrap_or_else(|| "no new build of the current tag".into()));
    }
    let build = if info.newer_tag.is_some() { info.build_change.clone() } else { info.change.clone() };
    if let Some(why) = classify::blocked_by(rules, &build.unwrap_or_default(), false) {
        return Decision::Skip(blocked.unwrap_or(why));
    }
    // A skipped build is no update (see `UpdateInfo::apply_skip`), except
    // when a newer tag is offered too.
    let latest = info.latest_digest.as_deref().unwrap_or_default();
    if !latest.is_empty() && latest == prefs.skipped_digest {
        return Decision::Skip(blocked.unwrap_or_else(|| "this build was skipped".into()));
    }
    if !latest.is_empty() && latest == prefs.rolled_back_digest {
        return Decision::Skip(blocked.unwrap_or_else(|| "this build was rolled back before".into()));
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
            &prefs.auto_install(&settings),
            &prefs.update,
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
    use crate::settings::TagScope;
    use crate::updates::NewerTag;
    use crate::updates::classify::Bump;

    fn info(status: UpdateStatus, current: &str, latest: &str, first_seen: i64) -> UpdateInfo {
        UpdateInfo {
            status,
            current_digest: Some(current.into()),
            latest_digest: Some(latest.into()),
            first_seen: Some(first_seen),
            ..Default::default()
        }
    }

    fn change(bump: Bump) -> Change {
        Change { bump, ..Default::default() }
    }

    const FREE: UpdatePrefs = UpdatePrefs {
        policy: None,
        auto_install: None,
        auto_scope: None,
        skipped_digest: String::new(),
        rolled_back_digest: String::new(),
        held: false,
        min_age_hours: None,
    };

    fn skip_reason(d: Decision) -> String {
        match d {
            Decision::Skip(why) => why,
            other => panic!("expected a skip, got {other:?}"),
        }
    }

    const TAGS: AutoInstall = AutoInstall {
        rebuild: true,
        patch: true,
        minor: true,
        major: false,
        unknown: true,
        security: true,
        breaking: false,
        switch_tags: true,
    };

    #[test]
    fn decisions() {
        let now = 1_000_000;
        let rules = AutoInstall::default();
        let avail = info(UpdateStatus::Available, "a", "b", now - 10 * 3600);
        assert_eq!(
            decide(UpdatePolicy::Notify, &rules, &FREE, 0, &avail, now),
            Decision::Skip("policy is not auto".into())
        );
        assert!(matches!(decide(UpdatePolicy::Auto, &rules, &FREE, 6, &avail, now), Decision::Update(_)));
        assert!(matches!(decide(UpdatePolicy::Auto, &rules, &FREE, 24, &avail, now), Decision::Skip(_)));
        let held = UpdatePrefs { held: true, ..FREE };
        assert!(matches!(decide(UpdatePolicy::Auto, &rules, &held, 0, &avail, now), Decision::Skip(_)));
        let up = info(UpdateStatus::UpToDate, "a", "a", now);
        assert!(matches!(decide(UpdatePolicy::Auto, &rules, &FREE, 0, &up, now), Decision::Skip(_)));
    }

    #[test]
    fn skipped_and_rolled_back_builds_are_left_alone() {
        let now = 1_000_000;
        let rules = AutoInstall::default();
        let avail = info(UpdateStatus::Available, "a", "b", now - 10 * 3600);
        let rolled_back = UpdatePrefs { rolled_back_digest: "b".into(), ..FREE };
        assert!(skip_reason(decide(UpdatePolicy::Auto, &rules, &rolled_back, 0, &avail, now)).contains("rolled back"));
        // A skipped build is no update at all...
        let skipped = UpdatePrefs { skipped_digest: "b".into(), ..FREE };
        let mut i = avail.clone();
        i.apply_skip(&skipped.skipped_digest);
        assert_eq!(skip_reason(decide(UpdatePolicy::Auto, &rules, &skipped, 0, &i, now)), "no update");
        // ...nor what's installed instead of a newer tag it may not switch
        // to; the reason is the newer tag's.
        let mut i = avail.clone();
        i.newer_tag = Some(NewerTag { tag: "9.0.0".into(), scope: TagScope::Major, first_seen: Some(now) });
        i.change = Some(change(Bump::Major));
        i.build_change = Some(change(Bump::Rebuild));
        i.apply_skip(&skipped.skipped_digest);
        assert_eq!(i.status, UpdateStatus::Available);
        assert!(skip_reason(decide(UpdatePolicy::Auto, &TAGS, &skipped, 0, &i, now)).contains("major"));
        // A newer build installs.
        let newer = info(UpdateStatus::Available, "a", "c", now - 10 * 3600);
        assert!(matches!(decide(UpdatePolicy::Auto, &rules, &rolled_back, 0, &newer, now), Decision::Update(_)));
        assert!(matches!(decide(UpdatePolicy::Auto, &rules, &skipped, 0, &newer, now), Decision::Update(_)));
    }

    #[test]
    fn new_builds_follow_their_type() {
        let now = 1_000_000;
        let rules = AutoInstall::default();
        let mut i = info(UpdateStatus::Available, "a", "b", now - 10 * 3600);
        i.change = Some(change(Bump::Major));
        assert!(skip_reason(decide(UpdatePolicy::Auto, &rules, &FREE, 0, &i, now)).contains("major"));
        i.change = Some(Change { bump: Bump::Minor, breaking: true, ..Default::default() });
        assert!(skip_reason(decide(UpdatePolicy::Auto, &rules, &FREE, 0, &i, now)).contains("breaking"));
        let only_patches = AutoInstall { minor: false, ..rules };
        i.change = Some(Change { bump: Bump::Minor, security: true, ..Default::default() });
        assert!(matches!(decide(UpdatePolicy::Auto, &only_patches, &FREE, 0, &i, now), Decision::Update(_)));
        let no_security = AutoInstall { security: false, ..only_patches };
        assert!(matches!(decide(UpdatePolicy::Auto, &no_security, &FREE, 0, &i, now), Decision::Skip(_)));
    }

    #[test]
    fn tag_switches_follow_their_type() {
        let now = 1_000_000;
        let mut i = info(UpdateStatus::Available, "a", "a", now);
        i.newer_tag = Some(NewerTag { tag: "8.5.0".into(), scope: TagScope::Minor, first_seen: Some(now - 10 * 3600) });
        i.change = Some(change(Bump::Minor));
        // Tags stay as they are unless switching is on.
        assert!(skip_reason(decide(UpdatePolicy::Auto, &AutoInstall::default(), &FREE, 0, &i, now)).contains("tag"));
        let patches = AutoInstall { minor: false, ..TAGS };
        assert!(skip_reason(decide(UpdatePolicy::Auto, &patches, &FREE, 0, &i, now)).contains("minor"));
        match decide(UpdatePolicy::Auto, &TAGS, &FREE, 6, &i, now) {
            Decision::Update(req) => assert_eq!(req.tag.as_deref(), Some("8.5.0")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn a_blocked_tag_switch_falls_back_to_a_new_build() {
        let now = 1_000_000;
        let mut i = info(UpdateStatus::Available, "a", "b", now - 10 * 3600);
        i.newer_tag = Some(NewerTag { tag: "9.0.0".into(), scope: TagScope::Major, first_seen: Some(now) });
        i.change = Some(change(Bump::Major));
        i.build_change = Some(change(Bump::Rebuild));
        assert_eq!(decide(UpdatePolicy::Auto, &TAGS, &FREE, 0, &i, now), Decision::Update(UpdateRequest::default()));
        // When the build isn't allowed either, the reason is the offered update's.
        let strict = AutoInstall { rebuild: false, ..TAGS };
        assert!(skip_reason(decide(UpdatePolicy::Auto, &strict, &FREE, 0, &i, now)).contains("major"));
    }

    #[test]
    fn tag_switches_wait_for_the_minimum_age() {
        let now = 1_000_000;
        let mut i = info(UpdateStatus::Available, "a", "a", now);
        i.newer_tag = Some(NewerTag { tag: "8.5.0".into(), scope: TagScope::Minor, first_seen: Some(now - 3600) });
        assert!(skip_reason(decide(UpdatePolicy::Auto, &TAGS, &FREE, 6, &i, now)).contains("6h old"));
        assert!(matches!(decide(UpdatePolicy::Auto, &TAGS, &FREE, 1, &i, now), Decision::Update(_)));
    }
}
