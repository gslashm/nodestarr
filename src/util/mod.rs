//! Small helpers shared across modules.

pub mod fsx;
pub mod net;
pub mod ring;

use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

/// Current unix time in seconds.
pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Current unix time in milliseconds.
pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Fills a buffer with cryptographically secure random bytes.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).expect("operating system random source unavailable");
    buf
}

/// A random URL-safe token with `bytes` bytes of entropy.
pub fn random_token(bytes: usize) -> String {
    use base64::Engine;
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("operating system random source unavailable");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

/// A short random hex id (16 hex chars), used for task and backup ids.
pub fn short_id() -> String {
    hex::encode(random_bytes::<8>())
}

pub fn sha256_hex(data: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(data.as_ref()))
}

/// Constant-time equality for secrets.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Deserializes a yes/no flag from a boolean or from URL query text such as
/// `1`, `true`, `yes`, `on` (and `0`, `false`, `no`, `off`, empty).
pub fn flag<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    struct V;
    impl serde::de::Visitor<'_> for V {
        type Value = bool;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a boolean such as true, false, 1 or 0")
        }

        fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<bool, E> {
            Ok(v)
        }

        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<bool, E> {
            Ok(v != 0)
        }

        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<bool, E> {
            Ok(v != 0)
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<bool, E> {
            match v.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => Ok(true),
                "" | "0" | "false" | "no" | "off" => Ok(false),
                other => Err(E::custom(format!("'{other}' is not true or false"))),
            }
        }
    }
    d.deserialize_any(V)
}

/// Truncates a string to at most `max` bytes on a char boundary, adding an ellipsis.
pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max.saturating_sub(1);
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// Human readable description of a reqwest error, including the cause chain
/// (reqwest's own Display hides the useful part, e.g. "connection refused").
pub fn describe_reqwest_error(err: &reqwest::Error) -> String {
    use std::error::Error as _;
    let mut msg = if err.is_timeout() {
        "request timed out".to_string()
    } else if err.is_connect() {
        "could not connect".to_string()
    } else if let Some(status) = err.status() {
        format!("HTTP {status}")
    } else {
        err.to_string()
    };
    if let Some(url) = err.url() {
        msg.push_str(&format!(" ({})", redact_url(url.as_str())));
    }
    let mut source = err.source();
    let mut seen = 0;
    while let Some(s) = source {
        let text = s.to_string();
        if !msg.contains(&text) {
            msg.push_str(": ");
            msg.push_str(&text);
        }
        source = s.source();
        seen += 1;
        if seen > 4 {
            break;
        }
    }
    msg
}

/// Removes credentials and query strings from a URL before it is logged.
pub fn redact_url(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(mut u) => {
            let _ = u.set_username("");
            let _ = u.set_password(None);
            u.set_query(None);
            // Discord/Slack style webhook URLs carry the secret in the path.
            let path = u.path().to_string();
            if path.contains("/webhooks/") || path.contains("/services/") {
                u.set_path("/");
                // Appended after formatting so the ellipsis isn't percent-encoded.
                return format!("{u}…");
            }
            u.to_string()
        }
        Err(_) => "<invalid url>".to_string(),
    }
}

/// Formats a byte count for log and notification messages.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

/// Formats a duration in seconds as e.g. "3d 4h" or "12m 5s".
pub fn human_duration(secs: i64) -> String {
    let secs = secs.max(0);
    let (d, h, m, s) = (secs / 86400, (secs % 86400) / 3600, (secs % 3600) / 60, secs % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

/// Parses an RFC 3339 timestamp (as Docker returns them) to unix seconds.
/// Docker uses "0001-01-01T00:00:00Z" for "never", which maps to `None`.
pub fn parse_rfc3339(ts: &str) -> Option<i64> {
    let parsed = chrono::DateTime::parse_from_rfc3339(ts).ok()?;
    let secs = parsed.timestamp();
    if secs <= 0 { None } else { Some(secs) }
}

/// Lowercase, hyphenated version of a name, safe for file names and MQTT ids.
pub fn slug(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() { "x".to_string() } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_respects_char_boundaries() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("héllo wörld", 4), "hé…");
    }

    #[test]
    fn slug_works() {
        assert_eq!(slug("compose:media/sonarr"), "compose-media-sonarr");
        assert_eq!(slug("--Hello  World--"), "hello-world");
        assert_eq!(slug("***"), "x");
    }

    #[test]
    fn redact_url_strips_secrets() {
        assert_eq!(redact_url("https://user:pw@example.com/api?token=abc"), "https://example.com/api");
        assert_eq!(redact_url("https://discord.com/api/webhooks/123/secret"), "https://discord.com/…");
    }

    #[test]
    fn rfc3339_zero_is_none() {
        assert_eq!(parse_rfc3339("0001-01-01T00:00:00Z"), None);
        assert_eq!(parse_rfc3339("2024-01-02T03:04:05.123456789Z"), Some(1704164645));
    }

    #[test]
    fn human_formats() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1536), "1.5 KiB");
        assert_eq!(human_duration(90061), "1d 1h");
        assert_eq!(human_duration(65), "1m 5s");
    }

    #[test]
    fn constant_time_eq() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }
}
