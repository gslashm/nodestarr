//! Finding app URLs and API keys from their config files.

use std::path::Path;

use crate::app::App;
use crate::docker::{Entry, reach};
use crate::integrations::{CredState, kind_of};

#[derive(Debug, Clone)]
pub struct Creds {
    /// scheme://host:port plus any URL base, without trailing slash.
    pub base_url: String,
    pub api_key: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub state: CredState,
}

/// Reads a file from the container's config folder: directly when mounted
/// into Nodestarr, otherwise with `docker exec cat`.
pub async fn read_config(app: &App, entry: &Entry, container_path: &str) -> Option<String> {
    for m in &entry.summary.mounts {
        if m.kind != "bind" {
            continue;
        }
        if let Ok(rel) = Path::new(container_path).strip_prefix(&m.destination) {
            let host = Path::new(&m.source).join(rel);
            if let Ok(text) = tokio::fs::read_to_string(&host).await {
                return Some(text);
            }
        }
    }
    let out = crate::docker::exec::capture(&app.docker.api, &entry.summary.id, vec!["cat".into(), container_path.into()], None)
        .await
        .ok()?;
    (out.exit_code == Some(0) && !out.stdout.is_empty()).then_some(out.stdout)
}

/// Value of a simple XML element: `<ApiKey>abc</ApiKey>`.
pub fn xml_element(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].trim().to_string()).filter(|s| !s.is_empty())
}

/// Value of an XML attribute anywhere in the document: `PlexOnlineToken="..."`.
pub fn xml_attribute(xml: &str, attr: &str) -> Option<String> {
    let needle = format!(" {attr}=\"");
    let start = xml.find(&needle)? + needle.len();
    let end = xml[start..].find('"')? + start;
    Some(xml[start..end].to_string()).filter(|s| !s.is_empty())
}

/// `key = value` from an INI section (or anywhere when `section` is None).
pub fn ini_value(ini: &str, section: Option<&str>, key: &str) -> Option<String> {
    let mut current = String::new();
    for line in ini.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            current = t[1..t.len() - 1].to_string();
            continue;
        }
        if section.is_some_and(|s| !s.eq_ignore_ascii_case(&current)) {
            continue;
        }
        if let Some((k, v)) = t.split_once('=') {
            if k.trim().eq_ignore_ascii_case(key) {
                let v = v.trim().trim_matches('"').to_string();
                return (!v.is_empty()).then_some(v);
            }
        }
    }
    None
}

/// `key: value` under a top-level YAML section (Bazarr's config.yaml).
fn yaml_value(yaml: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    for line in yaml.lines() {
        if !line.starts_with(' ') && !line.starts_with('\t') {
            in_section = line.trim_end().trim_end_matches(':') == section;
            continue;
        }
        if in_section {
            if let Some((k, v)) = line.trim().split_once(':') {
                if k.trim() == key {
                    let v = v.trim().trim_matches(|c| c == '\'' || c == '"').to_string();
                    return (!v.is_empty()).then_some(v);
                }
            }
        }
    }
    None
}

fn normalize_base(url_base: &str) -> String {
    let t = url_base.trim().trim_matches('/');
    if t.is_empty() { String::new() } else { format!("/{t}") }
}

pub async fn discover(app: &App, entry: &Entry) -> Creds {
    let kind = kind_of(entry).unwrap_or("");
    let port = reach::app_port(entry).unwrap_or(80);
    let mut https = false;
    let mut url_base = String::new();
    let mut api_key = None;
    let mut port_override = None;
    let mut state = CredState::Missing;
    let settings = app.settings.get();

    match kind {
        "arr" => {
            if let Some(xml) = read_config(app, entry, "/config/config.xml").await {
                api_key = xml_element(&xml, "ApiKey");
                url_base = normalize_base(&xml_element(&xml, "UrlBase").unwrap_or_default());
                if xml_element(&xml, "EnableSsl").is_some_and(|v| v.eq_ignore_ascii_case("true")) && entry.summary.network_mode == "host" {
                    https = true;
                    port_override = xml_element(&xml, "SslPort").and_then(|p| p.parse().ok());
                }
            }
        }
        "bazarr" => {
            if let Some(yaml) = read_config(app, entry, "/config/config/config.yaml").await {
                api_key = yaml_value(&yaml, "auth", "apikey");
                url_base = normalize_base(&yaml_value(&yaml, "general", "base_url").unwrap_or_default());
            } else if let Some(ini) = read_config(app, entry, "/config/config/config.ini").await {
                api_key = ini_value(&ini, Some("auth"), "apikey");
                url_base = normalize_base(&ini_value(&ini, Some("general"), "base_url").unwrap_or_default());
            }
        }
        "sabnzbd" => {
            if let Some(ini) = read_config(app, entry, "/config/sabnzbd.ini").await {
                api_key = ini_value(&ini, Some("misc"), "api_key");
                url_base = normalize_base(&ini_value(&ini, Some("misc"), "url_base").unwrap_or_default());
            }
        }
        "tautulli" => {
            if let Some(ini) = read_config(app, entry, "/config/config.ini").await {
                api_key = ini_value(&ini, Some("General"), "api_key");
                url_base = normalize_base(&ini_value(&ini, Some("General"), "http_root").unwrap_or_default());
            }
        }
        "seerr" => {
            for path in ["/app/config/settings.json", "/config/settings.json"] {
                if let Some(text) = read_config(app, entry, path).await {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                        api_key = v["main"]["apiKey"].as_str().map(str::to_string);
                    }
                    break;
                }
            }
        }
        "plex" => {
            for path in [
                "/config/Library/Application Support/Plex Media Server/Preferences.xml",
                "/config/Preferences.xml",
            ] {
                if let Some(xml) = read_config(app, entry, path).await {
                    api_key = xml_attribute(&xml, "PlexOnlineToken");
                    break;
                }
            }
        }
        "qbittorrent" => {
            if !settings.integrations.qbittorrent_username.is_empty() {
                return finish(
                    app,
                    entry,
                    port,
                    false,
                    String::new(),
                    Creds {
                        base_url: String::new(),
                        api_key: None,
                        username: Some(settings.integrations.qbittorrent_username.clone()),
                        password: Some(settings.integrations.qbittorrent_password.clone()),
                        state: CredState::Configured,
                    },
                );
            }
            state = CredState::NotNeeded;
        }
        "tdarr" | "immich" => state = CredState::NotNeeded,
        "homeassistant" => state = CredState::Missing,
        _ => {}
    }
    if api_key.is_some() {
        state = CredState::Discovered;
    }
    let creds = Creds {
        base_url: String::new(),
        api_key,
        username: None,
        password: None,
        state,
    };
    finish(app, entry, port_override.unwrap_or(port), https, url_base, creds)
}

fn finish(app: &App, entry: &Entry, port: u16, https: bool, url_base: String, mut creds: Creds) -> Creds {
    if let Some(base) = reach::base_url(app, entry, port, https) {
        creds.base_url = format!("{base}{url_base}");
    }
    creds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arr_config() {
        let xml = "<Config>\n  <Port>8989</Port>\n  <UrlBase>/sonarr/</UrlBase>\n  <ApiKey>abc123</ApiKey>\n</Config>";
        assert_eq!(xml_element(xml, "ApiKey").as_deref(), Some("abc123"));
        assert_eq!(normalize_base(&xml_element(xml, "UrlBase").unwrap()), "/sonarr");
        assert_eq!(xml_element("<Config><UrlBase></UrlBase></Config>", "UrlBase"), None);
    }

    #[test]
    fn parses_plex_token() {
        let xml = r#"<?xml version="1.0"?><Preferences MachineIdentifier="x" PlexOnlineToken="tok-1" FriendlyName="srv"/>"#;
        assert_eq!(xml_attribute(xml, "PlexOnlineToken").as_deref(), Some("tok-1"));
    }

    #[test]
    fn parses_ini_and_yaml() {
        let ini = "[misc]\napi_key = 0123\nurl_base = /sabnzbd\n[servers]\napi_key = nope\n";
        assert_eq!(ini_value(ini, Some("misc"), "api_key").as_deref(), Some("0123"));
        assert_eq!(ini_value(ini, Some("misc"), "url_base").as_deref(), Some("/sabnzbd"));
        let yaml = "general:\n  base_url: ''\n  port: 6767\nauth:\n  apikey: bz-key\n  type: null\n";
        assert_eq!(yaml_value(yaml, "auth", "apikey").as_deref(), Some("bz-key"));
        assert_eq!(yaml_value(yaml, "general", "base_url"), None);
    }
}
