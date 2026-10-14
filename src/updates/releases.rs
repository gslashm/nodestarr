//! Release notes from GitHub, shown before updating.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::app::App;
use crate::error::{AppError, Result};

/// Upstream repositories for apps whose images point at a packaging repo.
const UPSTREAM: &[(&str, &str)] = &[
    ("sonarr", "Sonarr/Sonarr"),
    ("radarr", "Radarr/Radarr"),
    ("lidarr", "Lidarr/Lidarr"),
    ("readarr", "Readarr/Readarr"),
    ("prowlarr", "Prowlarr/Prowlarr"),
    ("whisparr", "Whisparr/Whisparr"),
    ("bazarr", "morpheus65535/bazarr"),
    ("overseerr", "sct/overseerr"),
    ("jellyseerr", "Fallenbagel/jellyseerr"),
    ("seerr", "seerr-team/seerr"),
    ("tautulli", "Tautulli/Tautulli"),
    ("sabnzbd", "sabnzbd/sabnzbd"),
    ("qbittorrent", "qbittorrent/qBittorrent"),
    ("immich", "immich-app/immich"),
    ("homeassistant", "home-assistant/core"),
    ("frigate", "blakeblackshear/frigate"),
    ("jellyfin", "jellyfin/jellyfin"),
    ("tdarr", "HaveAGitGat/Tdarr"),
    ("cloudflared", "cloudflare/cloudflared"),
    ("uptime-kuma", "louislam/uptime-kuma"),
    ("vaultwarden", "dani-garcia/vaultwarden"),
    ("zigbee2mqtt", "Koenkk/zigbee2mqtt"),
    ("esphome", "esphome/esphome"),
    ("audiobookshelf", "advplyr/audiobookshelf"),
    ("navidrome", "navidrome/navidrome"),
    ("gluetun", "qdm12/gluetun"),
    ("flaresolverr", "FlareSolverr/FlareSolverr"),
    ("autobrr", "autobrr/autobrr"),
    ("recyclarr", "recyclarr/recyclarr"),
    ("unpackerr", "Unpackerr/unpackerr"),
    ("kometa", "Kometa-Team/Kometa"),
    ("maintainerr", "jorenn92/Maintainerr"),
    ("caddy", "caddyserver/caddy"),
    ("traefik", "traefik/traefik"),
    ("portainer", "portainer/portainer"),
    ("dockge", "louislam/dockge"),
    ("homepage", "gethomepage/homepage"),
    ("adguardhome", "AdguardTeam/AdGuardHome"),
    ("node-red", "node-red/node-red"),
    ("paperless", "paperless-ngx/paperless-ngx"),
    ("nginx-proxy-manager", "NginxProxyManager/nginx-proxy-manager"),
    ("gitea", "go-gitea/gitea"),
    ("syncthing", "syncthing/syncthing"),
    ("berth", "your-user/berth"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub tag: String,
    pub name: String,
    pub published_at: Option<String>,
    pub prerelease: bool,
    pub url: String,
    /// Markdown body, truncated.
    pub body: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseNotes {
    pub repo: String,
    pub releases: Vec<Release>,
}

fn cache() -> &'static Mutex<HashMap<String, (Instant, ReleaseNotes)>> {
    static C: OnceLock<Mutex<HashMap<String, (Instant, ReleaseNotes)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Extracts "owner/repo" from a GitHub URL.
pub fn github_repo(url: &str) -> Option<String> {
    let rest = url
        .trim()
        .trim_end_matches(".git")
        .split("github.com/")
        .nth(1)?;
    let mut parts = rest.split('/').filter(|p| !p.is_empty());
    let owner = parts.next()?;
    let repo = parts.next()?;
    Some(format!("{owner}/{repo}"))
}

/// Picks the repository to read release notes from.
pub fn repo_for(app_slug: Option<&str>, source: Option<&str>) -> Option<String> {
    if let Some(slug) = app_slug {
        if let Some((_, repo)) = UPSTREAM.iter().find(|(s, _)| *s == slug) {
            return Some(repo.to_string());
        }
    }
    source.and_then(github_repo)
}

pub async fn fetch(app: &App, repo: &str) -> Result<ReleaseNotes> {
    if let Some((at, notes)) = cache().lock().get(repo) {
        if at.elapsed() < Duration::from_secs(3600) {
            return Ok(notes.clone());
        }
    }
    #[derive(Deserialize)]
    struct Gh {
        tag_name: String,
        name: Option<String>,
        published_at: Option<String>,
        prerelease: bool,
        draft: bool,
        html_url: String,
        body: Option<String>,
    }
    let token = app.settings.get().updates.github_token.clone();
    let mut req = app
        .http
        .get(format!("https://api.github.com/repos/{repo}/releases?per_page=15"))
        .header("Accept", "application/vnd.github+json");
    if !token.is_empty() {
        req = req.bearer_auth(token);
    }
    let resp = req.send().await?;
    match resp.status().as_u16() {
        200 => {}
        404 => return Err(AppError::not_found(format!("releases for {repo}"))),
        403 | 429 => {
            return Err(AppError::upstream(
                "GitHub rate limit reached; add a GitHub token in Settings > Updates",
            ));
        }
        s => return Err(AppError::upstream(format!("GitHub returned HTTP {s}"))),
    }
    let items: Vec<Gh> = resp.json().await?;
    let notes = ReleaseNotes {
        repo: repo.to_string(),
        releases: items
            .into_iter()
            .filter(|r| !r.draft)
            .map(|r| Release {
                name: r.name.filter(|n| !n.is_empty()).unwrap_or_else(|| r.tag_name.clone()),
                tag: r.tag_name,
                published_at: r.published_at,
                prerelease: r.prerelease,
                url: r.html_url,
                body: crate::util::truncate(&r.body.unwrap_or_default(), 20_000),
            })
            .collect(),
    };
    cache().lock().insert(repo.to_string(), (Instant::now(), notes.clone()));
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_parsing() {
        assert_eq!(github_repo("https://github.com/linuxserver/docker-sonarr").as_deref(), Some("linuxserver/docker-sonarr"));
        assert_eq!(github_repo("https://github.com/immich-app/immich.git").as_deref(), Some("immich-app/immich"));
        assert_eq!(github_repo("https://gitlab.com/x/y"), None);
        assert_eq!(repo_for(Some("sonarr"), Some("https://github.com/linuxserver/docker-sonarr")).as_deref(), Some("Sonarr/Sonarr"));
        assert_eq!(repo_for(None, Some("https://github.com/a/b")).as_deref(), Some("a/b"));
    }
}
