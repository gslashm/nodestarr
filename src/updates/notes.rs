//! Reading release notes for security fixes and breaking changes.
//!
//! An update's release notes are the GitHub releases after the running
//! version, up to and including the new one. Nodestarr looks for a few clear
//! signals and keeps the line that gave each one away, so the UI can show
//! why an update was marked:
//!
//! * security: CVE and GitHub advisory ids, a "Security" section, phrases
//!   like "security fix", fixed vulnerabilities, and the names of common
//!   attack classes (XSS, SQL injection, path traversal, ...);
//! * breaking: "breaking change", a "Breaking changes" section, "backwards
//!   incompatible", "action required" or "manual intervention", and
//!   conventional-commit markers such as `feat!:`.
//!
//! Negations ("no breaking changes", "non-breaking", "Breaking changes:
//! none") don't count. It is a heuristic, so it leans towards calling an
//! update breaking (which only holds automatic updates back) and is strict
//! about security (which can let them install).

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::registry::version::{Version, natural_cmp};
use crate::updates::classify::sizable;
use crate::updates::releases::Release;

/// What the release notes of one update said.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotesScan {
    /// GitHub repository ("owner/repo").
    pub repo: String,
    /// Tags of the releases that were read, newest first.
    pub releases: Vec<String>,
    /// GitHub only gives the newest releases, and they don't reach back to
    /// the running version: older ones in between weren't read.
    pub partial: bool,
    pub findings: Vec<Finding>,
    /// The new version's release page, when it has one.
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub kind: Signal,
    /// Tag of the release it was found in.
    pub release: String,
    /// The line that says so, without Markdown.
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    Security,
    Breaking,
}

/// Findings kept per signal.
const MAX_FINDINGS: usize = 3;
/// Longest excerpt, in characters.
const EXCERPT: usize = 140;

/// Reads the releases after `from` up to and including `to`. Without `from`
/// only the release of `to` is read. `releases` are newest first, as GitHub
/// lists them; `complete` says whether that is all of them.
pub fn scan(repo: &str, releases: &[Release], from: Option<&str>, to: &str, complete: bool) -> NotesScan {
    let to_v = Version::parse(to);
    let from_v = from.and_then(Version::parse).filter(|f| to_v.as_ref().is_some_and(|t| sizable(f, t)));
    let mut out = NotesScan { repo: repo.to_string(), ..Default::default() };
    let mut reached_from = false;
    for r in releases {
        let place = match (&to_v, Version::parse(&r.tag)) {
            (Some(t), Some(rv)) if sizable(&rv, t) => {
                if from_v.as_ref().is_some_and(|f| order(&rv, f) != Ordering::Greater) {
                    reached_from = true;
                    continue;
                }
                match order(&rv, t) {
                    Ordering::Greater => continue,
                    Ordering::Equal => Place::Target,
                    Ordering::Less if from_v.is_some() => Place::Between,
                    Ordering::Less => continue,
                }
            }
            (Some(_), _) => continue,
            // Versions that don't parse can still match by name.
            (None, _) if same_name(&r.tag, to) => Place::Target,
            (None, _) => continue,
        };
        if place == Place::Target && out.url.is_none() {
            out.url = Some(r.url.clone());
        }
        out.releases.push(r.tag.clone());
        for (kind, text) in signals(&format!("{}\n{}", r.name, r.body)) {
            let kept = out.findings.iter().filter(|f| f.kind == kind).count();
            if kept < MAX_FINDINGS && !out.findings.iter().any(|f| f.kind == kind && f.text == text) {
                out.findings.push(Finding { kind, release: r.tag.clone(), text });
            }
        }
    }
    out.partial = from_v.is_some() && !reached_from && !complete;
    out
}

#[derive(PartialEq)]
enum Place {
    Target,
    Between,
}

/// Orders release versions by their numbers (missing ones are zeros), then
/// pre-release, then build number when both have one; prefixes and
/// suffixes don't matter, so "v4.0.10.2544" equals "4.0.10.2544-ls260".
fn order(a: &Version, b: &Version) -> Ordering {
    let len = a.core.len().max(b.core.len());
    for i in 0..len {
        match a.core.get(i).unwrap_or(&0).cmp(b.core.get(i).unwrap_or(&0)) {
            Ordering::Equal => {}
            o => return o,
        }
    }
    let pre = match (&a.pre, &b.pre) {
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => natural_cmp(&x.to_ascii_lowercase(), &y.to_ascii_lowercase()),
        (None, None) => Ordering::Equal,
    };
    match (a.build, b.build) {
        (Some(x), Some(y)) => pre.then(x.cmp(&y)),
        _ => pre,
    }
}

fn same_name(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().to_ascii_lowercase().trim_start_matches('v').to_string();
    norm(a) == norm(b)
}

/// The signals in one release's text, with the line each came from.
pub fn signals(text: &str) -> Vec<(Signal, String)> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out = Vec::new();
    let mut push = |kind: Signal, line: &str| {
        let text = clean(line);
        if !text.is_empty() {
            out.push((kind, text));
        }
    };
    for (i, &line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        if let Some(kind) = heading_text(line).and_then(heading_signal) {
            // A section with an entry under it, unless the entry is "None".
            let next = lines[i + 1..].iter().find(|l| !l.is_empty()).copied();
            match next {
                Some(n) if is_none(n) => {}
                Some(n) if heading_text(n).is_none() => push(kind, n),
                _ => push(kind, line),
            }
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if is_security(&lower) {
            push(Signal::Security, line);
        }
        if is_breaking(&lower) {
            push(Signal::Breaking, line);
        }
    }
    out
}

/// The text of a Markdown heading, a line that is all bold, or a short line
/// ending in a colon ("Breaking changes:").
fn heading_text(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix('#') {
        return Some(rest.trim_start_matches('#').trim());
    }
    for mark in ["**", "__"] {
        if let Some(inner) = line.strip_prefix(mark).and_then(|l| l.strip_suffix(mark).or(l.strip_suffix(':')))
            && !inner.contains(mark)
        {
            return Some(inner.trim_end_matches([':', '*', '_']).trim());
        }
    }
    let bullet = line.starts_with(['-', '*', '+', '>']);
    (!bullet && line.ends_with(':') && line.len() <= 40).then(|| line.trim_end_matches(':').trim())
}

fn heading_signal(heading: &str) -> Option<Signal> {
    let words: Vec<String> = heading
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    let has = |w: &str| words.iter().any(|x| x == w);
    if has("breaking") && !has("no") && !has("non") {
        return Some(Signal::Breaking);
    }
    if has("incompatible") && (has("backward") || has("backwards") || has("changes")) {
        return Some(Signal::Breaking);
    }
    let about = has("security") || has("vulnerability") || has("vulnerabilities") || has("cve") || has("cves");
    if about && (words.len() == 1 || words.iter().any(|w| FIX_NOUNS.contains(&w.as_str()))) {
        return Some(Signal::Security);
    }
    None
}

/// Words that make "security ..." mean a fix rather than a feature.
const FIX_NOUNS: &[&str] = &[
    "fix",
    "fixes",
    "issue",
    "issues",
    "vulnerability",
    "vulnerabilities",
    "advisory",
    "advisories",
    "patch",
    "patches",
    "release",
    "update",
    "updates",
    "bug",
    "bugs",
    "flaw",
    "flaws",
    "hole",
    "holes",
    "problem",
    "problems",
    "notice",
];

const ATTACKS: &[&str] = &[
    "xss",
    "cross-site scripting",
    "cross site scripting",
    "csrf",
    "xsrf",
    "cross-site request forgery",
    "ssrf",
    "server-side request forgery",
    "sql injection",
    "remote code execution",
    "command injection",
    "code injection",
    "path traversal",
    "directory traversal",
    "privilege escalation",
    "authentication bypass",
    "auth bypass",
    "arbitrary code execution",
    "arbitrary file read",
    "arbitrary file write",
    "arbitrary file upload",
    "open redirect",
    "denial of service",
    "denial-of-service",
];

const FIX_VERBS: &[&str] = &[
    "fix",
    "fixes",
    "fixed",
    "fixing",
    "patch",
    "patches",
    "patched",
    "address",
    "addresses",
    "addressed",
    "resolve",
    "resolves",
    "resolved",
    "mitigate",
    "mitigates",
    "mitigated",
];

/// Features of security tools, not fixes ("vulnerability scanner").
const VULN_FEATURES: &[&str] =
    &["vulnerability scan", "vulnerability report", "vulnerability data", "vulnerability db", "vulnerabilities page"];

fn is_security(lower: &str) -> bool {
    if has_cve(lower) || has_ghsa(lower) {
        return true;
    }
    if occurrences(lower, "security").any(|i| {
        let next = lower[i + "security".len()..].split(|c: char| !c.is_ascii_alphanumeric()).find(|w| !w.is_empty());
        next.is_some_and(|w| FIX_NOUNS.contains(&w)) && !negated(lower, i)
    }) {
        return true;
    }
    if lower.contains("vulnerab") && !VULN_FEATURES.iter().any(|f| lower.contains(f)) {
        let fixed = FIX_VERBS.iter().any(|v| occurrences(lower, v).next().is_some());
        if (fixed || occurrences(lower, "vulnerable to").next().is_some())
            && !lower.match_indices("vulnerab").any(|(i, _)| negated(lower, i))
        {
            return true;
        }
    }
    ATTACKS.iter().any(|a| occurrences(lower, a).any(|i| !negated(lower, i)))
}

fn is_breaking(lower: &str) -> bool {
    for i in occurrences(lower, "breaking") {
        if negated(lower, i) {
            continue;
        }
        let after = lower[i + "breaking".len()..].trim_start_matches(['*', '_', '`', ']', ')', ' ', '-']);
        let before = lower[..i].trim_end_matches(['*', '_', '`', ' ']);
        if let Some(rest) = after.strip_prefix("change") {
            // "Breaking changes: none"
            let rest = rest.trim_start_matches(['s', ':', '*', '_', ' ', '-']);
            if !is_none(rest) {
                return true;
            }
        } else if after.starts_with(':') || before.ends_with(['[', '(']) {
            return true;
        }
    }
    const PHRASES: &[&str] = &[
        "backward incompatible",
        "backwards incompatible",
        "backward-incompatible",
        "backwards-incompatible",
        "not backward compatible",
        "not backwards compatible",
        "no longer backward compatible",
        "no longer backwards compatible",
        "action required",
        "manual intervention",
        "manual migration",
        "migration required",
        "requires manual",
    ];
    if PHRASES.iter().any(|p| occurrences(lower, p).any(|i| !negated(lower, i))) {
        return true;
    }
    conventional_breaking(lower)
}

/// A conventional-commit subject marked breaking: "feat!: ..." or "fix(api)!: ...".
fn conventional_breaking(lower: &str) -> bool {
    let s = lower.trim_start_matches(['-', '*', '+', ' ']);
    let kind_end = s.find(|c: char| !c.is_ascii_lowercase()).unwrap_or(s.len());
    if kind_end == 0 {
        return false;
    }
    let mut rest = &s[kind_end..];
    if rest.starts_with('(') {
        match rest.find(')') {
            Some(close) => rest = &rest[close + 1..],
            None => return false,
        }
    }
    rest.starts_with("!:")
}

/// Positions where `needle` appears as whole words.
fn occurrences<'a>(s: &'a str, needle: &'a str) -> impl Iterator<Item = usize> + 'a {
    s.match_indices(needle).map(|(i, _)| i).filter(move |&i| {
        let before = s[..i].chars().next_back();
        let after = s[i + needle.len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric()) && !after.is_some_and(|c| c.is_alphanumeric())
    })
}

/// Whether the words just before position `i` negate what follows ("no
/// breaking changes", "not a security issue", "non-breaking").
fn negated(lower: &str, i: usize) -> bool {
    let before = &lower[..i];
    if before.ends_with("non-") || before.ends_with("non ") {
        return true;
    }
    let words: Vec<&str> = before
        .split_whitespace()
        .rev()
        .take(3)
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\''))
        .collect();
    let negation = |w: &str| {
        matches!(w, "no" | "not" | "without" | "zero" | "never" | "non") || w.ends_with("n't") || w.ends_with("n’t")
    };
    // "no breaking", "not a breaking", "isn't really a breaking"
    words.first().copied().is_some_and(negation)
        || (words.len() > 1 && matches!(words[0], "a" | "an" | "any" | "real" | "really") && negation(words[1]))
        || (words.len() > 2
            && matches!(words[0], "a" | "an")
            && matches!(words[1], "real" | "really")
            && negation(words[2]))
}

/// "None", "N/A", "Nothing", ... as the only content of a line.
fn is_none(line: &str) -> bool {
    let s = clean(line).to_ascii_lowercase();
    let s = s.trim_end_matches(['.', '!']).trim();
    matches!(s, "none" | "n/a" | "na" | "nothing" | "no" | "-" | "none yet" | "not applicable")
        || s.starts_with("there are no ")
        || s.starts_with("no breaking")
        || s.starts_with("nothing ")
}

fn has_cve(lower: &str) -> bool {
    occurrences(lower, "cve").any(|i| {
        let rest = &lower.as_bytes()[i + 3..];
        rest.first() == Some(&b'-')
            && rest.get(1..5).is_some_and(|y| y.iter().all(u8::is_ascii_digit))
            && rest.get(5) == Some(&b'-')
            && rest.get(6..10).is_some_and(|n| n.iter().all(u8::is_ascii_digit))
    })
}

fn has_ghsa(lower: &str) -> bool {
    lower.match_indices("ghsa-").any(|(i, _)| {
        let id = &lower.as_bytes()[i + 5..];
        let group = |r: std::ops::Range<usize>| id.get(r).is_some_and(|g| g.iter().all(u8::is_ascii_alphanumeric));
        group(0..4) && id.get(4) == Some(&b'-') && group(5..9) && id.get(9) == Some(&b'-') && group(10..14)
    })
}

/// A line without Markdown, shortened for display.
fn clean(line: &str) -> String {
    let mut s = line.trim().trim_start_matches(['#', '>', '-', '*', '+', ' ']).to_string();
    // [text](url) becomes text.
    while let Some(open) = s.find('[') {
        let Some(mid) = s[open..].find("](").map(|m| open + m) else { break };
        let Some(close) = s[mid..].find(')').map(|c| mid + c) else { break };
        let text = s[open + 1..mid].to_string();
        s.replace_range(open..=close, &text);
    }
    for mark in ["**", "__", "`"] {
        s = s.replace(mark, "");
    }
    // GitHub's generated notes end entries with "by @user in https://...".
    for tail in [" by @", " in https://"] {
        if let Some(i) = s.find(tail) {
            s.truncate(i);
        }
    }
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= EXCERPT {
        return s;
    }
    let cut: String = s.chars().take(EXCERPT - 1).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<Signal> {
        signals(text).into_iter().map(|(k, _)| k).collect()
    }

    fn release(tag: &str, body: &str) -> Release {
        Release {
            tag: tag.into(),
            name: tag.into(),
            published_at: None,
            prerelease: false,
            url: format!("https://github.com/o/r/releases/tag/{tag}"),
            body: body.into(),
        }
    }

    #[test]
    fn security_signals() {
        assert_eq!(kinds("- Fixed CVE-2024-12345 in the login handler"), vec![Signal::Security]);
        assert_eq!(kinds("See GHSA-abcd-1234-wxyz for details"), vec![Signal::Security]);
        assert_eq!(kinds("This release contains an important security fix."), vec![Signal::Security]);
        assert_eq!(kinds("* Fix stored XSS in the comments page"), vec![Signal::Security]);
        assert_eq!(kinds("- Patched a vulnerability in file uploads"), vec![Signal::Security]);
        assert_eq!(kinds("### Security\n\n- Escape user names"), vec![Signal::Security]);
        // Not fixes.
        assert!(kinds("- New security settings page").is_empty());
        assert!(kinds("- Improved the vulnerability scanner speed, fixed sorting").is_empty());
        assert!(kinds("No security issues were found in the audit").is_empty());
        assert!(kinds("## Security improvements\n- Stricter cookie defaults").is_empty());
        assert!(kinds("Bump cve-check from 1.2 to 1.3").is_empty());
    }

    #[test]
    fn breaking_signals() {
        assert_eq!(kinds("## ⚠️ Breaking Changes\n\n- The API moved to /api/v2"), vec![Signal::Breaking]);
        assert_eq!(kinds("BREAKING CHANGE: config files are now YAML"), vec![Signal::Breaking]);
        assert_eq!(kinds("- **Breaking:** removed the legacy importer"), vec![Signal::Breaking]);
        assert_eq!(
            kinds("* feat(server)!: drop the v1 endpoints by @dev in https://github.com/o/r/pull/1"),
            vec![Signal::Breaking]
        );
        assert_eq!(kinds("Backwards-incompatible changes: the port is now 8080"), vec![Signal::Breaking]);
        assert_eq!(kinds("Manual intervention is needed for Postgres users"), vec![Signal::Breaking]);
        // Negations and unrelated uses.
        assert!(kinds("There are no breaking changes in this release.").is_empty());
        assert!(kinds("- Non-breaking cleanup of the settings page").is_empty());
        assert!(kinds("This is not a breaking change").is_empty());
        assert!(kinds("Breaking changes: none").is_empty());
        assert!(kinds("## Breaking changes\n\nNone").is_empty());
        assert!(kinds("- Fixed a bug breaking the login page").is_empty());
        assert!(kinds("- fix: no action required for most users").is_empty());
    }

    #[test]
    fn excerpts_are_clean() {
        let found =
            signals("* **api**: fix [CVE-2024-1234](https://nvd.nist.gov/x) by @me in https://github.com/o/r/pull/9");
        assert_eq!(found, vec![(Signal::Security, "api: fix CVE-2024-1234".to_string())]);
        let long = format!("- Security fix: {}", "a".repeat(300));
        assert!(signals(&long)[0].1.chars().count() <= EXCERPT);
    }

    #[test]
    fn scans_the_releases_in_between() {
        let releases = vec![
            release("v1.4.0", "New things"),
            release("v1.3.0", "## Breaking changes\n- Moved the config folder"),
            release("v1.2.1", "- Fix CVE-2024-0001"),
            release("v1.2.0", "## Breaking changes\n- Old news"),
        ];
        let s = scan("o/r", &releases, Some("1.2.0"), "1.3.0", false);
        assert_eq!(s.releases, vec!["v1.3.0", "v1.2.1"]);
        assert_eq!(s.url.as_deref(), Some("https://github.com/o/r/releases/tag/v1.3.0"));
        assert!(!s.partial);
        let kinds: Vec<_> = s.findings.iter().map(|f| (f.kind, f.release.as_str())).collect();
        assert_eq!(kinds, vec![(Signal::Breaking, "v1.3.0"), (Signal::Security, "v1.2.1")]);
        // LinuxServer versions match upstream tags by their numbers.
        let s = scan("o/r", &releases, Some("1.2.1-ls10"), "1.4.0-ls12", true);
        assert_eq!(s.releases, vec!["v1.4.0", "v1.3.0"]);
        // Older releases than the ones fetched may be missing.
        let s = scan("o/r", &releases, Some("1.0.0"), "1.4.0", false);
        assert!(s.partial);
        assert_eq!(s.releases.len(), 4);
        // Without the running version, only the new one is read.
        let s = scan("o/r", &releases, None, "1.2.1", false);
        assert_eq!(s.releases, vec!["v1.2.1"]);
    }
}
