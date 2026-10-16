//! What kind of update an update is.
//!
//! Every available update gets a type, so the Updates page can sort and
//! filter by it and automatic updates can install some types and not others:
//!
//! * how far the version moves ([`Bump`]): a major, minor or patch version,
//!   a rebuild of the same version, or unknown when the image doesn't say
//!   which version it is;
//! * whether the release notes in between mention security fixes or
//!   breaking changes (see [`super::notes`]);
//! * whether the new version is a pre-release.

use serde::{Deserialize, Serialize};

use crate::registry::version::Version;
use crate::settings::{AutoInstall, TagScope};
use crate::updates::notes::NotesScan;

/// How far an update moves the version.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bump {
    Major,
    Minor,
    Patch,
    /// The same version in a new image (a refreshed base image, a packaging
    /// revision such as LinuxServer's ls250 to ls251).
    Rebuild,
    /// Nodestarr can't tell: the image doesn't say which version it is.
    #[default]
    Unknown,
}

impl Bump {
    /// "major versions", "rebuilds": for sentences about this kind of update.
    pub fn plural(self) -> &'static str {
        match self {
            Bump::Major => "major versions",
            Bump::Minor => "minor versions",
            Bump::Patch => "patch versions",
            Bump::Rebuild => "rebuilds",
            Bump::Unknown => "updates of unknown size",
        }
    }

    /// "minor version", "rebuild".
    pub fn singular(self) -> &'static str {
        match self {
            Bump::Major => "major version",
            Bump::Minor => "minor version",
            Bump::Patch => "patch version",
            Bump::Rebuild => "rebuild",
            Bump::Unknown => "update of unknown size",
        }
    }
}

impl From<TagScope> for Bump {
    fn from(s: TagScope) -> Self {
        match s {
            TagScope::Major => Bump::Major,
            TagScope::Minor => Bump::Minor,
            TagScope::Patch => Bump::Patch,
            TagScope::Digest => Bump::Rebuild,
        }
    }
}

/// What one update changes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Change {
    pub bump: Bump,
    /// The bump comes from the tag alone (the image doesn't say its version),
    /// so it is the most the update can be: a new build of "16" is at most a
    /// minor version.
    pub estimated: bool,
    /// The version running now and the one the update brings, when known.
    pub from_version: Option<String>,
    pub to_version: Option<String>,
    pub prerelease: bool,
    /// The release notes mention security fixes.
    pub security: bool,
    /// The release notes mention breaking changes or required manual steps.
    pub breaking: bool,
    /// The release notes that were read, and what they said.
    pub notes: Option<NotesScan>,
}

impl Change {
    /// A few words for notifications and Home Assistant: "minor version,
    /// security fixes".
    pub fn summary(&self) -> String {
        let mut parts = vec![if self.estimated {
            format!("{} at most", self.bump.singular())
        } else {
            self.bump.singular().to_string()
        }];
        if self.security {
            parts.push("security fixes".into());
        }
        if self.breaking {
            parts.push("breaking changes".into());
        }
        if self.prerelease {
            parts.push("pre-release".into());
        }
        parts.join(", ")
    }
}

/// Why automatic updates won't install `change` under `rules`, or None when
/// they may. `switches_tag` is true when installing it means changing the
/// container's tag.
pub fn blocked_by(rules: &AutoInstall, change: &Change, switches_tag: bool) -> Option<String> {
    if switches_tag && !rules.switch_tags {
        return Some("it changes the tag, and automatic updates keep tags as they are".into());
    }
    if change.breaking && !rules.breaking {
        return Some("its release notes mention breaking changes".into());
    }
    let allowed = match change.bump {
        Bump::Major => rules.major,
        Bump::Minor => rules.minor,
        Bump::Patch => rules.patch,
        Bump::Rebuild => rules.rebuild,
        Bump::Unknown => rules.unknown,
    };
    // Security fixes may skip the version rule, but never for a major version.
    if allowed || (change.security && rules.security && change.bump != Bump::Major) {
        return None;
    }
    Some(format!("{} aren't installed automatically", change.bump.plural()))
}

/// The new build of the tag a container runs. `running` is the running
/// version when the image says it (labels or env, not the tag), `latest` the
/// new build's version, and `tag` the tag both were pulled as.
pub fn classify_build(running: Option<&str>, latest: Option<&str>, tag: &str) -> Change {
    let mut change = Change {
        from_version: running.map(str::to_string),
        to_version: latest.map(str::to_string),
        ..Default::default()
    };
    let parsed = (running.and_then(Version::parse), latest.and_then(Version::parse));
    match (running, latest, parsed) {
        (Some(a), Some(b), _) if a.trim() == b.trim() => change.bump = Bump::Rebuild,
        (_, _, (Some(a), Some(b))) if sizable(&a, &b) => change.bump = bump_between(&a, &b),
        _ => {
            // Without both versions, the tag still limits how far a new build
            // can go: "16" moves by minor versions at most, "16.4" by patches,
            // and "16.4.2" can only be rebuilt.
            if let Some(t) = Version::parse(tag) {
                change.estimated = true;
                change.bump = match t.core.len() {
                    1 => Bump::Minor,
                    2 => Bump::Patch,
                    _ => Bump::Rebuild,
                };
            }
        }
    }
    change.prerelease = latest.and_then(Version::parse).is_some_and(|v| v.is_prerelease());
    change
}

/// A newer version tag than the one a container runs.
pub fn classify_tag(current: &Version, newer: &Version) -> Change {
    Change {
        bump: bump_between(current, newer),
        from_version: Some(current.raw.clone()),
        to_version: Some(newer.raw.clone()),
        prerelease: newer.is_prerelease(),
        ..Default::default()
    }
}

/// How far the version moves from `from` to `to`. Only the numbers count, so
/// "v1.2" and "1.3-alpine" compare fine; "16.4" and "16.4.1" too (missing
/// numbers are zeros).
pub fn bump_between(from: &Version, to: &Version) -> Bump {
    // Year-based versions (2024.12 to 2025.1) start a new year without being
    // a major update: the year and the month are the feature releases.
    let calver = is_calver(from) && is_calver(to);
    let at = |v: &Version, i: usize| v.core.get(i).copied().unwrap_or(0);
    let len = from.core.len().max(to.core.len());
    match (0..len).find(|&i| at(from, i) != at(to, i)) {
        Some(0 | 1) if calver => Bump::Minor,
        Some(_) if calver => Bump::Patch,
        Some(0) => Bump::Major,
        Some(1) => Bump::Minor,
        Some(_) => Bump::Patch,
        // 1.2.3-rc1 to 1.2.3 is a patch-sized step; ls250 to ls251 a rebuild.
        None if from.pre != to.pre => Bump::Patch,
        None => Bump::Rebuild,
    }
}

/// Whether two versions read from labels look like the same numbering, so
/// that sizing the change means something (a commit hash that happens to
/// parse doesn't).
pub(crate) fn sizable(a: &Version, b: &Version) -> bool {
    let prefix = |v: &Version| v.prefix.trim_end_matches(['v', 'V']).to_string();
    prefix(a) == prefix(b) && a.core.len().abs_diff(b.core.len()) <= 1 && is_calver(a) == is_calver(b)
}

fn is_calver(v: &Version) -> bool {
    v.core.first().is_some_and(|y| (2000..=2100).contains(y) || (20000101..=21001231).contains(y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap_or_else(|| panic!("failed to parse {s}"))
    }

    #[test]
    fn bumps() {
        assert_eq!(bump_between(&v("1.2.3"), &v("2.0.0")), Bump::Major);
        assert_eq!(bump_between(&v("1.2.3"), &v("1.3.0")), Bump::Minor);
        assert_eq!(bump_between(&v("1.2.3"), &v("1.2.4")), Bump::Patch);
        assert_eq!(bump_between(&v("v1.2"), &v("1.3-alpine")), Bump::Minor);
        assert_eq!(bump_between(&v("16.4"), &v("16.4.1")), Bump::Patch);
        // Sonarr's four-part versions and LinuxServer builds.
        assert_eq!(bump_between(&v("4.0.9.2244-ls250"), &v("4.0.10.2544-ls260")), Bump::Patch);
        assert_eq!(bump_between(&v("4.0.9.2244-ls250"), &v("4.0.9.2244-ls251")), Bump::Rebuild);
        assert_eq!(bump_between(&v("1.2.3-rc1"), &v("1.2.3")), Bump::Patch);
        // Plex: the commit hash after the numbers doesn't matter.
        assert_eq!(bump_between(&v("1.41.0.8992-8463ad060"), &v("1.41.1.9057-af5eaea7a")), Bump::Patch);
    }

    #[test]
    fn year_based_versions_are_never_major() {
        assert_eq!(bump_between(&v("2024.12.4"), &v("2025.1.0")), Bump::Minor);
        assert_eq!(bump_between(&v("2024.10.1"), &v("2024.11.0")), Bump::Minor);
        assert_eq!(bump_between(&v("2024.10.1"), &v("2024.10.2")), Bump::Patch);
    }

    #[test]
    fn new_builds_of_a_tag() {
        let c = classify_build(Some("4.0.9.2244-ls250"), Some("4.0.10.2544-ls260"), "latest");
        assert_eq!((c.bump, c.estimated), (Bump::Patch, false));
        assert_eq!(c.from_version.as_deref(), Some("4.0.9.2244-ls250"));
        assert_eq!(classify_build(Some("5.0.0"), Some("5.0.0"), "latest").bump, Bump::Rebuild);
        assert!(classify_build(Some("1.27.2"), Some("2.0.0-beta1"), "mainline").prerelease);
        // No versions: unknown on a channel tag, bounded by a version tag.
        assert_eq!(classify_build(None, None, "latest").bump, Bump::Unknown);
        let floating = classify_build(None, None, "16");
        assert_eq!((floating.bump, floating.estimated), (Bump::Minor, true));
        assert_eq!(classify_build(None, Some("16.4.3"), "16.4").bump, Bump::Patch);
        assert_eq!(classify_build(None, None, "16.4.2").bump, Bump::Rebuild);
        // A commit hash isn't a version to size against.
        assert_eq!(classify_build(Some("1.2.3"), Some("abc1234"), "latest").bump, Bump::Unknown);
    }

    #[test]
    fn newer_tags() {
        let c = classify_tag(&v("8.4.2"), &v("8.5.0"));
        assert_eq!(c.bump, Bump::Minor);
        assert_eq!(c.to_version.as_deref(), Some("8.5.0"));
        assert_eq!(classify_tag(&v("16-alpine"), &v("17-alpine")).bump, Bump::Major);
    }

    #[test]
    fn rules() {
        let rules = AutoInstall::default();
        let change = |bump, security, breaking| Change { bump, security, breaking, ..Default::default() };
        assert_eq!(blocked_by(&rules, &change(Bump::Minor, false, false), false), None);
        assert!(blocked_by(&rules, &change(Bump::Major, false, false), false).unwrap().contains("major"));
        assert!(blocked_by(&rules, &change(Bump::Minor, false, true), false).unwrap().contains("breaking"));
        assert!(blocked_by(&rules, &change(Bump::Patch, false, false), true).unwrap().contains("tag"));
        // Security fixes skip the version rule, except for major versions.
        let strict = AutoInstall { minor: false, ..AutoInstall::default() };
        assert!(blocked_by(&strict, &change(Bump::Minor, false, false), false).is_some());
        assert_eq!(blocked_by(&strict, &change(Bump::Minor, true, false), false), None);
        assert!(blocked_by(&strict, &change(Bump::Major, true, false), false).is_some());
        // Breaking changes still hold a security fix back.
        assert!(blocked_by(&strict, &change(Bump::Patch, true, true), false).unwrap().contains("breaking"));
    }

    #[test]
    fn summaries() {
        let c = Change { bump: Bump::Minor, security: true, ..Default::default() };
        assert_eq!(c.summary(), "minor version, security fixes");
        let e = Change { bump: Bump::Patch, estimated: true, ..Default::default() };
        assert_eq!(e.summary(), "patch version at most");
    }
}
