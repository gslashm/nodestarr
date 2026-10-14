//! Reading and editing `.env` files without disturbing comments or order.

/// Parses `KEY=value` lines (with optional `export`, quotes and comments).
pub fn parse(content: &str) -> Vec<(String, String)> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (k, v) = line.split_once('=')?;
            let key = k.trim();
            if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return None;
            }
            Some((key.to_string(), unquote(v.trim())))
        })
        .collect()
}

fn unquote(v: &str) -> String {
    if v.len() >= 2 && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\''))) {
        let inner = &v[1..v.len() - 1];
        if v.starts_with('"') {
            inner.replace("\\n", "\n").replace("\\\"", "\"").replace("\\\\", "\\")
        } else {
            inner.to_string()
        }
    } else {
        // Strip trailing inline comments: VALUE # comment
        match v.find(" #") {
            Some(i) => v[..i].trim_end().to_string(),
            None => v.to_string(),
        }
    }
}

pub fn get(content: &str, key: &str) -> Option<String> {
    parse(content).into_iter().rev().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// Sets `key=value`, replacing the last existing assignment in place or
/// appending a new line.
pub fn set(content: &str, key: &str, value: &str) -> String {
    let needs_quotes = value.contains(char::is_whitespace) || value.contains('#') || value.contains('"');
    let rendered = if needs_quotes {
        format!("{key}=\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        format!("{key}={value}")
    };
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    let idx = lines.iter().rposition(|l| {
        let t = l.trim_start();
        let t = t.strip_prefix("export ").unwrap_or(t);
        t.split_once('=').is_some_and(|(k, _)| k.trim() == key)
    });
    match idx {
        Some(i) => {
            let export = lines[i].trim_start().starts_with("export ");
            lines[i] = if export { format!("export {rendered}") } else { rendered };
        }
        None => lines.push(rendered),
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_get() {
        let env = "# comment\nPUID=1000\nexport TZ=\"Europe/Berlin\"\nTAG=latest # pinned later\nEMPTY=\n";
        assert_eq!(get(env, "PUID").as_deref(), Some("1000"));
        assert_eq!(get(env, "TZ").as_deref(), Some("Europe/Berlin"));
        assert_eq!(get(env, "TAG").as_deref(), Some("latest"));
        assert_eq!(get(env, "EMPTY").as_deref(), Some(""));
        assert_eq!(get(env, "MISSING"), None);
    }

    #[test]
    fn set_replaces_or_appends() {
        let env = "# media\nSONARR_TAG=latest\nOTHER=1";
        let out = set(env, "SONARR_TAG", "develop");
        assert_eq!(out, "# media\nSONARR_TAG=develop\nOTHER=1\n");
        let out = set(&out, "NEW", "two words");
        assert!(out.ends_with("NEW=\"two words\"\n"));
        assert_eq!(set("export A=1", "A", "2"), "export A=2\n");
    }
}
