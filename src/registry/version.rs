//! Loose version parsing for Docker tags.
//!
//! Image tags are not SemVer: "4.0.9.2244-ls250", "v2.10.0", "16-alpine",
//! "2024.10.1", "version-1.32.4", "8.4.2-oracle". A tag is split into an
//! optional prefix, a dotted numeric core, an optional build number (the
//! LinuxServer "-lsNNN" style) and a flavor suffix ("-alpine", "-oracle").
//! Two tags are only compared when prefix, flavor and core length match, so
//! "16-alpine" is compared with "17-alpine" but never with "16.4" or "17".

use std::cmp::Ordering;

use serde::Serialize;

use crate::settings::TagScope;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Version {
    pub raw: String,
    pub prefix: String,
    pub core: Vec<u64>,
    /// Pre-release marker ("rc.1", "beta2"), if any.
    pub pre: Option<String>,
    /// Build number such as LinuxServer's "ls250".
    pub build: Option<u64>,
    /// Remaining suffix that identifies the variant ("alpine", "oracle").
    pub flavor: String,
}

const PRE_MARKERS: &[&str] =
    &["alpha", "beta", "rc", "pre", "preview", "dev", "nightly", "snapshot", "canary", "test", "unstable"];

impl Version {
    pub fn parse(tag: &str) -> Option<Self> {
        let raw = tag.trim();
        if raw.is_empty() || raw.len() > 128 {
            return None;
        }
        // Prefix: leading non-digit characters ("v", "version-", "release-").
        let digit_start = raw.find(|c: char| c.is_ascii_digit())?;
        let prefix = &raw[..digit_start];
        if prefix.len() > 16 || prefix.chars().any(|c| !(c.is_ascii_alphabetic() || c == '-' || c == '_')) {
            return None;
        }
        let rest = &raw[digit_start..];
        // Core: digits separated by dots.
        let mut core = Vec::new();
        let mut consumed = 0;
        for (i, seg) in rest.split('.').enumerate() {
            let digits: String = seg.chars().take_while(|c| c.is_ascii_digit()).collect();
            if digits.is_empty() || digits.len() > 12 {
                break;
            }
            core.push(digits.parse().ok()?);
            consumed += digits.len() + usize::from(i > 0);
            if digits.len() != seg.len() || core.len() == 5 {
                break;
            }
        }
        if core.is_empty() {
            return None;
        }
        // Dates without dots (20241001) are fine as a single component, but a
        // lone huge number like a commit timestamp is still comparable.
        let mut tail = rest[consumed..].trim_start_matches(['-', '_', '+', '.']);

        let mut pre = None;
        let mut build = None;
        let mut flavor_parts: Vec<String> = Vec::new();
        while !tail.is_empty() {
            let (part, next) = match tail.find(['-', '_', '+']) {
                Some(i) => (&tail[..i], tail[i + 1..].trim_start_matches(['-', '_', '+'])),
                None => (tail, ""),
            };
            let lower = part.to_ascii_lowercase();
            if let Some(n) = lower.strip_prefix("ls").and_then(|n| n.parse::<u64>().ok()) {
                build = Some(n);
            } else if PRE_MARKERS.iter().any(|m| lower.starts_with(m)) {
                pre = Some(part.to_string());
                // "rc.1": include the dotted number that follows.
            } else if pre.is_some() && lower.chars().all(|c| c.is_ascii_digit() || c == '.') {
                if let Some(p) = pre.as_mut() {
                    p.push('.');
                    p.push_str(part);
                }
            } else if !part.is_empty() {
                // Strip trailing version numbers from the flavor ("alpine3.20" -> "alpine")
                let name: String = part
                    .chars()
                    .take_while(|c| !c.is_ascii_digit())
                    .collect::<String>()
                    .trim_end_matches('.')
                    .to_ascii_lowercase();
                flavor_parts.push(if name.is_empty() { lower } else { name });
            }
            tail = next;
        }
        Some(Self {
            raw: raw.to_string(),
            prefix: prefix.to_ascii_lowercase(),
            core,
            pre,
            build,
            flavor: flavor_parts.join("-"),
        })
    }

    pub fn is_prerelease(&self) -> bool {
        self.pre.is_some()
    }

    /// Whether two versions belong to the same "release line" and can be compared.
    pub fn comparable(&self, other: &Version) -> bool {
        self.prefix == other.prefix
            && self.flavor == other.flavor
            && self.core.len() == other.core.len()
            && self.build.is_some() == other.build.is_some()
    }

    /// The scope of the change from `self` to a newer `other`.
    pub fn change_scope(&self, other: &Version) -> TagScope {
        let a = &self.core;
        let b = &other.core;
        if a.first() != b.first() {
            TagScope::Major
        } else if a.len() > 1 && a.get(1) != b.get(1) {
            TagScope::Minor
        } else if a != b || self.build != other.build || self.pre != other.pre {
            TagScope::Patch
        } else {
            TagScope::Digest
        }
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if !self.comparable(other) {
            return None;
        }
        Some(self.cmp_unchecked(other))
    }
}

impl Version {
    fn cmp_unchecked(&self, other: &Self) -> Ordering {
        for (a, b) in self.core.iter().zip(&other.core) {
            match a.cmp(b) {
                Ordering::Equal => {}
                o => return o,
            }
        }
        // A release sorts after its pre-releases.
        match (&self.pre, &other.pre) {
            (None, Some(_)) => return Ordering::Greater,
            (Some(_), None) => return Ordering::Less,
            (Some(a), Some(b)) => match natural_cmp(a, b) {
                Ordering::Equal => {}
                o => return o,
            },
            (None, None) => {}
        }
        self.build.cmp(&other.build)
    }
}

/// Compares strings treating digit runs as numbers ("rc10" > "rc9").
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek(), bi.peek()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().filter(|c| c.is_ascii_digit()) {
                    na.push(*c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().filter(|c| c.is_ascii_digit()) {
                    nb.push(*c);
                    bi.next();
                }
                let (x, y) = (na.parse::<u64>().unwrap_or(0), nb.parse::<u64>().unwrap_or(0));
                match x.cmp(&y) {
                    Ordering::Equal => {}
                    o => return o,
                }
            }
            (Some(x), Some(y)) => match x.cmp(y) {
                Ordering::Equal => {
                    ai.next();
                    bi.next();
                }
                o => return o,
            },
        }
    }
}

/// Tags that move over time rather than naming a fixed version.
pub fn is_channel_tag(tag: &str) -> bool {
    Version::parse(tag).is_none()
}

/// From `candidates`, the newest tag comparable to `current` and newer than
/// it, limited to `scope` (Digest means "none").
pub fn newest_within<'a>(
    current: &Version,
    candidates: impl IntoIterator<Item = &'a str>,
    scope: TagScope,
    allow_prerelease: bool,
) -> Option<Version> {
    if scope == TagScope::Digest {
        return None;
    }
    candidates
        .into_iter()
        .filter_map(Version::parse)
        .filter(|v| current.comparable(v))
        .filter(|v| allow_prerelease || !v.is_prerelease() || current.is_prerelease())
        .filter(|v| v.cmp_unchecked(current) == Ordering::Greater)
        .filter(|v| current.change_scope(v) <= scope)
        .max_by(|a, b| a.cmp_unchecked(b))
}

/// Sorts version tags newest first; channel tags keep their relative order
/// and go first.
pub fn sort_tags_desc(tags: &mut [String]) {
    tags.sort_by(|a, b| match (Version::parse(a), Version::parse(b)) {
        (Some(va), Some(vb)) => {
            let ord = va.core.cmp(&vb.core).then_with(|| va.build.cmp(&vb.build));
            match (&va.pre, &vb.pre) {
                (None, Some(_)) if ord == Ordering::Equal => Ordering::Less,
                (Some(_), None) if ord == Ordering::Equal => Ordering::Greater,
                _ => ord.reverse(),
            }
        }
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap_or_else(|| panic!("failed to parse {s}"))
    }

    #[test]
    fn parses_common_tags() {
        let a = v("4.0.9.2244-ls250");
        assert_eq!(a.core, vec![4, 0, 9, 2244]);
        assert_eq!(a.build, Some(250));
        assert_eq!(a.flavor, "");

        let b = v("v2.10.0");
        assert_eq!(b.prefix, "v");
        assert_eq!(b.core, vec![2, 10, 0]);

        let c = v("16.4-alpine3.20");
        assert_eq!(c.core, vec![16, 4]);
        assert_eq!(c.flavor, "alpine");

        let d = v("1.2.3-rc.1");
        assert_eq!(d.pre.as_deref(), Some("rc.1"));

        let e = v("version-1.32.4");
        assert_eq!(e.prefix, "version-");

        assert!(Version::parse("latest").is_none());
        assert!(Version::parse("develop").is_none());
        assert!(Version::parse("release").is_none());
        assert!(is_channel_tag("nightly"));
    }

    #[test]
    fn ordering_and_comparability() {
        assert!(v("1.27.2") > v("1.27.1"));
        assert!(v("1.28.0") > v("1.27.9"));
        assert!(v("4.0.10.2544-ls260") > v("4.0.9.2244-ls250"));
        assert!(v("4.0.9.2244-ls251") > v("4.0.9.2244-ls250"));
        assert!(v("1.2.3") > v("1.2.3-rc.2"));
        assert!(v("1.2.3-rc.10") > v("1.2.3-rc.9"));
        assert_eq!(v("16-alpine").partial_cmp(&v("16.4-alpine")), None);
        assert_eq!(v("16-alpine").partial_cmp(&v("17")), None);
        assert_eq!(v("v1.2.3").partial_cmp(&v("1.2.4")), None);
    }

    #[test]
    fn scopes() {
        assert_eq!(v("1.2.3").change_scope(&v("2.0.0")), TagScope::Major);
        assert_eq!(v("1.2.3").change_scope(&v("1.3.0")), TagScope::Minor);
        assert_eq!(v("1.2.3").change_scope(&v("1.2.4")), TagScope::Patch);
        assert_eq!(v("4.0.9.2244-ls250").change_scope(&v("4.0.9.2244-ls251")), TagScope::Patch);
    }

    #[test]
    fn newest_within_scope() {
        let tags = ["8.4.2", "8.4.3", "8.5.0", "9.0.1", "9.1.0-rc1", "8.4.3-oracle", "latest", "8.4"];
        let cur = v("8.4.2");
        assert_eq!(newest_within(&cur, tags, TagScope::Patch, false).unwrap().raw, "8.4.3");
        assert_eq!(newest_within(&cur, tags, TagScope::Minor, false).unwrap().raw, "8.5.0");
        assert_eq!(newest_within(&cur, tags, TagScope::Major, false).unwrap().raw, "9.0.1");
        assert_eq!(newest_within(&cur, tags, TagScope::Major, true).unwrap().raw, "9.1.0-rc1");
        assert!(newest_within(&cur, tags, TagScope::Digest, false).is_none());
        let floating = v("8.4");
        assert!(newest_within(&floating, tags, TagScope::Major, false).is_none());
    }

    #[test]
    fn sorting() {
        let mut tags: Vec<String> =
            ["1.2.0", "latest", "1.10.0", "1.2.0-rc1", "develop", "1.9.9"].iter().map(|s| s.to_string()).collect();
        sort_tags_desc(&mut tags);
        assert_eq!(tags, vec!["latest", "develop", "1.10.0", "1.9.9", "1.2.0", "1.2.0-rc1"]);
    }
}
