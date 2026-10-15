//! Catalog of well-known self-hosted apps.
//!
//! Recognizing an app by its image lets Nodestarr link to its web UI, pick a
//! sensible backup profile (skip Plex's cache, Sonarr's MediaCover, ...),
//! choose a consistent backup method for its database, and wire up API
//! integrations. Unknown images still work, just with generic defaults.

use std::collections::HashMap;

/// Database engines Nodestarr knows how to dump consistently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DbEngine {
    Postgres,
    Mysql,
    Mariadb,
    Redis,
    Mongo,
}

#[derive(Debug)]
pub struct AppInfo {
    pub slug: &'static str,
    pub name: &'static str,
    /// Image repository basenames that identify the app.
    pub images: &'static [&'static str],
    pub web_port: Option<u16>,
    pub web_path: &'static str,
    pub web_https: bool,
    /// Path used for uptime checks (defaults to web_path).
    pub health_path: Option<&'static str>,
    /// API integration kind, if Nodestarr can talk to the app.
    pub integration: Option<&'static str>,
    /// The app keeps SQLite databases in its config folder.
    pub sqlite: bool,
    pub db: Option<DbEngine>,
    /// Globs (relative to each backed-up folder) left out of backups.
    pub backup_exclude: &'static [&'static str],
    /// Container paths treated as configuration for backups.
    pub config_paths: &'static [&'static str],
    /// Container paths that hold bulk data (never backed up by default).
    pub data_paths: &'static [&'static str],
    /// Env vars that carry the app version in its image.
    pub version_env: &'static [&'static str],
    /// Plex: updates/restarts can be held while someone is streaming.
    pub streaming_server: bool,
}

const CONFIG: &[&str] = &["/config"];
const NONE: &[&str] = &[];

macro_rules! app {
    ($slug:expr, $name:expr, [$($img:expr),*], $port:expr $(, $field:ident = $val:expr)* $(,)?) => {
        AppInfo {
            slug: $slug,
            name: $name,
            images: &[$($img),*],
            web_port: $port,
            $($field: $val,)*
            ..AppInfo::DEFAULT
        }
    };
}

impl AppInfo {
    const DEFAULT: AppInfo = AppInfo {
        slug: "",
        name: "",
        images: &[],
        web_port: None,
        web_path: "/",
        web_https: false,
        health_path: None,
        integration: None,
        sqlite: false,
        db: None,
        backup_exclude: NONE,
        config_paths: CONFIG,
        data_paths: NONE,
        version_env: NONE,
        streaming_server: false,
    };
}

const ARR_EXCLUDE: &[&str] = &["MediaCover/**", "logs/**", "logs.db*", "Backups/**", "asp/**", "*.pid", "Sentry/**"];

pub static CATALOG: &[AppInfo] = &[
    // --- *arr family ---------------------------------------------------------
    app!(
        "sonarr",
        "Sonarr",
        ["sonarr"],
        Some(8989),
        integration = Some("arr"),
        sqlite = true,
        health_path = Some("/ping"),
        backup_exclude = ARR_EXCLUDE,
        data_paths = &["/tv", "/anime", "/downloads", "/data"]
    ),
    app!(
        "radarr",
        "Radarr",
        ["radarr"],
        Some(7878),
        integration = Some("arr"),
        sqlite = true,
        health_path = Some("/ping"),
        backup_exclude = ARR_EXCLUDE,
        data_paths = &["/movies", "/downloads", "/data"]
    ),
    app!(
        "lidarr",
        "Lidarr",
        ["lidarr"],
        Some(8686),
        integration = Some("arr"),
        sqlite = true,
        health_path = Some("/ping"),
        backup_exclude = ARR_EXCLUDE,
        data_paths = &["/music", "/downloads", "/data"]
    ),
    app!(
        "readarr",
        "Readarr",
        ["readarr"],
        Some(8787),
        integration = Some("arr"),
        sqlite = true,
        health_path = Some("/ping"),
        backup_exclude = ARR_EXCLUDE,
        data_paths = &["/books", "/downloads", "/data"]
    ),
    app!(
        "whisparr",
        "Whisparr",
        ["whisparr"],
        Some(6969),
        integration = Some("arr"),
        sqlite = true,
        health_path = Some("/ping"),
        backup_exclude = ARR_EXCLUDE
    ),
    app!(
        "prowlarr",
        "Prowlarr",
        ["prowlarr"],
        Some(9696),
        integration = Some("arr"),
        sqlite = true,
        health_path = Some("/ping"),
        backup_exclude = ARR_EXCLUDE
    ),
    app!(
        "bazarr",
        "Bazarr",
        ["bazarr"],
        Some(6767),
        integration = Some("bazarr"),
        sqlite = true,
        backup_exclude = &["cache/**", "log/**", "backup/**", "restore/**"]
    ),
    app!(
        "overseerr",
        "Overseerr",
        ["overseerr"],
        Some(5055),
        integration = Some("seerr"),
        sqlite = true,
        health_path = Some("/api/v1/status"),
        backup_exclude = &["logs/**", "cache/**"],
        config_paths = &["/config", "/app/config"]
    ),
    app!(
        "jellyseerr",
        "Jellyseerr",
        ["jellyseerr"],
        Some(5055),
        integration = Some("seerr"),
        sqlite = true,
        health_path = Some("/api/v1/status"),
        backup_exclude = &["logs/**", "cache/**"],
        config_paths = &["/app/config", "/config"]
    ),
    app!(
        "seerr",
        "Seerr",
        ["seerr"],
        Some(5055),
        integration = Some("seerr"),
        sqlite = true,
        health_path = Some("/api/v1/status"),
        backup_exclude = &["logs/**", "cache/**"],
        config_paths = &["/app/config", "/config"]
    ),
    app!("ombi", "Ombi", ["ombi"], Some(3579), sqlite = true),
    app!(
        "tautulli",
        "Tautulli",
        ["tautulli"],
        Some(8181),
        integration = Some("tautulli"),
        sqlite = true,
        backup_exclude = &["cache/**", "logs/**", "backups/**", "newsletters/**"]
    ),
    app!("recyclarr", "Recyclarr", ["recyclarr"], None, backup_exclude = &["logs/**", "repositories/**"]),
    app!("unpackerr", "Unpackerr", ["unpackerr"], None),
    app!("notifiarr", "Notifiarr", ["notifiarr"], Some(5454)),
    app!("autobrr", "autobrr", ["autobrr"], Some(7474), sqlite = true, backup_exclude = &["logs/**"]),
    app!(
        "maintainerr",
        "Maintainerr",
        ["maintainerr"],
        Some(6246),
        sqlite = true,
        config_paths = &["/opt/data", "/config"]
    ),
    app!("wizarr", "Wizarr", ["wizarr"], Some(5690), sqlite = true, config_paths = &["/data/database", "/config"]),
    app!("kometa", "Kometa", ["kometa", "plex-meta-manager"], None, backup_exclude = &["logs/**", "assets/**"]),
    app!("flaresolverr", "FlareSolverr", ["flaresolverr"], Some(8191), config_paths = NONE),
    app!("byparr", "Byparr", ["byparr"], Some(8191), config_paths = NONE),
    app!("spottarr", "Spottarr", ["spottarr"], Some(8383), sqlite = true, config_paths = &["/data", "/config"]),
    // --- Downloaders -----------------------------------------------------------
    app!(
        "sabnzbd",
        "SABnzbd",
        ["sabnzbd"],
        Some(8080),
        integration = Some("sabnzbd"),
        sqlite = true,
        backup_exclude = &["logs/**", "admin/future/**", "*.log", "Downloads/**"],
        data_paths = &["/downloads", "/incomplete-downloads", "/data"]
    ),
    app!("nzbget", "NZBGet", ["nzbget"], Some(6789), backup_exclude = &["*.log"]),
    app!(
        "qbittorrent",
        "qBittorrent",
        ["qbittorrent", "qbittorrent-nox"],
        Some(8080),
        integration = Some("qbittorrent"),
        backup_exclude = &["qBittorrent/logs/**", "qBittorrent/cache/**"],
        data_paths = &["/downloads", "/data"]
    ),
    app!("transmission", "Transmission", ["transmission"], Some(9091), data_paths = &["/downloads", "/watch"]),
    app!("deluge", "Deluge", ["deluge"], Some(8112), data_paths = &["/downloads"]),
    app!("gluetun", "Gluetun", ["gluetun"], Some(8000), config_paths = &["/gluetun"]),
    // --- Media servers ---------------------------------------------------------
    app!(
        "plex",
        "Plex",
        ["plex", "pms-docker"],
        Some(32400),
        web_path = "/web",
        health_path = Some("/identity"),
        integration = Some("plex"),
        sqlite = true,
        streaming_server = true,
        backup_exclude = &[
            "**/Cache/**",
            "**/Metadata/**",
            "**/Media/**",
            "**/Codecs/**",
            "**/Crash Reports/**",
            "**/Logs/**",
            "**/Updates/**",
            "**/Drivers/**",
            "**/Diagnostics/**",
            "**/Scanners/**",
            "**/Plug-in Support/Caches/**",
            "**/Plug-in Support/Databases/*.db-20*",
            "transcode/**",
        ],
        data_paths = &["/data", "/tv", "/movies", "/media", "/transcode"],
        version_env = &["PLEX_VERSION", "VERSION"]
    ),
    app!(
        "jellyfin",
        "Jellyfin",
        ["jellyfin"],
        Some(8096),
        health_path = Some("/health"),
        sqlite = true,
        backup_exclude =
            &["cache/**", "log/**", "metadata/**", "transcodes/**", "data/metadata/**", "data/transcodes/**"],
        data_paths = &["/data", "/media", "/cache"]
    ),
    app!(
        "emby",
        "Emby",
        ["emby", "embyserver"],
        Some(8096),
        sqlite = true,
        backup_exclude = &["cache/**", "logs/**", "metadata/**", "transcoding-temp/**"]
    ),
    app!(
        "tdarr",
        "Tdarr",
        ["tdarr"],
        Some(8265),
        integration = Some("tdarr"),
        backup_exclude = &["**/logs/**", "**/temp/**", "**/Backups/**"],
        config_paths = &["/app/server", "/app/configs", "/config"],
        data_paths = &["/media", "/temp", "/data"]
    ),
    app!(
        "tdarr_node",
        "Tdarr Node",
        ["tdarr_node"],
        None,
        config_paths = &["/app/configs"],
        data_paths = &["/media", "/temp", "/data"]
    ),
    app!(
        "audiobookshelf",
        "Audiobookshelf",
        ["audiobookshelf"],
        Some(80),
        sqlite = true,
        config_paths = &["/config", "/metadata"],
        backup_exclude = &["cache/**", "logs/**"]
    ),
    app!(
        "navidrome",
        "Navidrome",
        ["navidrome"],
        Some(4533),
        sqlite = true,
        config_paths = &["/data"],
        backup_exclude = &["cache/**"]
    ),
    app!(
        "kavita",
        "Kavita",
        ["kavita"],
        Some(5000),
        sqlite = true,
        config_paths = &["/kavita/config", "/config"],
        backup_exclude = &["logs/**", "cache/**"]
    ),
    app!("komga", "Komga", ["komga"], Some(25600), sqlite = true),
    app!("calibre-web", "Calibre-Web", ["calibre-web"], Some(8083), sqlite = true),
    app!("calibre", "Calibre", ["calibre"], Some(8080)),
    // --- Photos & files ----------------------------------------------------------
    app!(
        "immich",
        "Immich",
        ["immich-server"],
        Some(2283),
        integration = Some("immich"),
        health_path = Some("/api/server/ping"),
        config_paths = NONE,
        data_paths = &["/usr/src/app/upload", "/data"]
    ),
    app!("immich-ml", "Immich ML", ["immich-machine-learning"], None, config_paths = NONE, data_paths = &["/cache"]),
    app!(
        "photoprism",
        "PhotoPrism",
        ["photoprism"],
        Some(2342),
        config_paths = &["/photoprism/storage"],
        data_paths = &["/photoprism/originals"]
    ),
    app!(
        "nextcloud",
        "Nextcloud",
        ["nextcloud"],
        Some(80),
        config_paths = &["/config", "/var/www/html/config"],
        data_paths = &["/data", "/var/www/html/data"]
    ),
    app!("syncthing", "Syncthing", ["syncthing"], Some(8384), config_paths = &["/config", "/var/syncthing/config"]),
    app!(
        "filebrowser",
        "File Browser",
        ["filebrowser"],
        Some(80),
        sqlite = true,
        config_paths = &["/database", "/config"]
    ),
    app!(
        "paperless",
        "Paperless-ngx",
        ["paperless-ngx"],
        Some(8000),
        config_paths = &["/usr/src/paperless/data"],
        data_paths = &["/usr/src/paperless/media", "/usr/src/paperless/consume"]
    ),
    // --- Home automation -----------------------------------------------------------
    app!(
        "homeassistant",
        "Home Assistant",
        ["home-assistant", "homeassistant"],
        Some(8123),
        integration = Some("homeassistant"),
        sqlite = true,
        backup_exclude = &["backups/**", "tts/**", "*.log", "*.log.*", ".cache/**"],
        health_path = Some("/manifest.json")
    ),
    app!(
        "frigate",
        "Frigate",
        ["frigate"],
        Some(5000),
        sqlite = true,
        config_paths = &["/config"],
        data_paths = &["/media/frigate", "/media", "/tmp/cache"],
        backup_exclude = &["model_cache/**"]
    ),
    app!(
        "zigbee2mqtt",
        "Zigbee2MQTT",
        ["zigbee2mqtt"],
        Some(8080),
        config_paths = &["/app/data", "/config"],
        backup_exclude = &["log/**"]
    ),
    app!(
        "mosquitto",
        "Mosquitto",
        ["eclipse-mosquitto", "mosquitto"],
        None,
        config_paths = &["/mosquitto/config", "/mosquitto/data"],
        backup_exclude = &["log/**"]
    ),
    app!("node-red", "Node-RED", ["node-red"], Some(1880), config_paths = &["/data"]),
    app!(
        "esphome",
        "ESPHome",
        ["esphome"],
        Some(6052),
        config_paths = &["/config"],
        backup_exclude = &[".esphome/build/**", ".esphome/platformio/**"]
    ),
    app!("wyoming-whisper", "Wyoming Whisper", ["wyoming-whisper"], None, config_paths = NONE, data_paths = &["/data"]),
    app!("wyoming-piper", "Wyoming Piper", ["wyoming-piper"], None, config_paths = NONE, data_paths = &["/data"]),
    // --- Infrastructure --------------------------------------------------------------
    app!(
        "cloudflared",
        "cloudflared",
        ["cloudflared"],
        None,
        config_paths = &["/etc/cloudflared", "/home/nonroot/.cloudflared"]
    ),
    app!("caddy", "Caddy", ["caddy"], Some(80), config_paths = &["/etc/caddy", "/data", "/config"]),
    app!("traefik", "Traefik", ["traefik"], Some(8080), config_paths = &["/etc/traefik", "/letsencrypt"]),
    app!(
        "nginx-proxy-manager",
        "Nginx Proxy Manager",
        ["nginx-proxy-manager"],
        Some(81),
        sqlite = true,
        config_paths = &["/data", "/etc/letsencrypt"]
    ),
    app!(
        "nginx",
        "nginx",
        ["nginx"],
        Some(80),
        config_paths = &["/etc/nginx", "/config"],
        version_env = &["NGINX_VERSION"]
    ),
    app!(
        "portainer",
        "Portainer",
        ["portainer-ce", "portainer"],
        Some(9443),
        web_https = true,
        config_paths = &["/data"]
    ),
    app!("dockge", "Dockge", ["dockge"], Some(5001), config_paths = &["/app/data"]),
    app!("uptime-kuma", "Uptime Kuma", ["uptime-kuma"], Some(3001), sqlite = true, config_paths = &["/app/data"]),
    app!("homepage", "Homepage", ["homepage"], Some(3000), config_paths = &["/app/config"]),
    app!(
        "vaultwarden",
        "Vaultwarden",
        ["vaultwarden", "server"],
        Some(80),
        sqlite = true,
        config_paths = &["/data"],
        backup_exclude = &["icon_cache/**", "tmp/**"]
    ),
    app!(
        "adguardhome",
        "AdGuard Home",
        ["adguardhome"],
        Some(3000),
        config_paths = &["/opt/adguardhome/conf", "/opt/adguardhome/work"],
        backup_exclude = &["data/querylog.json*", "data/stats.db"]
    ),
    app!(
        "pihole",
        "Pi-hole",
        ["pihole"],
        Some(80),
        web_path = "/admin",
        config_paths = &["/etc/pihole", "/etc/dnsmasq.d"]
    ),
    app!("wireguard", "WireGuard", ["wireguard", "wg-easy"], None, config_paths = &["/config", "/etc/wireguard"]),
    app!("rclone", "rclone", ["rclone"], None, config_paths = &["/config", "/config/rclone"]),
    app!("watchtower", "Watchtower", ["watchtower"], None, config_paths = NONE),
    app!("gitea", "Gitea", ["gitea"], Some(3000), sqlite = true, config_paths = &["/data"]),
    app!("code-server", "code-server", ["code-server"], Some(8443), config_paths = &["/config"]),
    app!("nodestarr", "Nodestarr", ["nodestarr"], Some(7979), config_paths = &["/config"]),
    // --- Databases (dumped instead of copied) --------------------------------------------
    app!(
        "postgres",
        "PostgreSQL",
        ["postgres", "postgis", "pgvecto-rs", "pgvector", "postgresql"],
        None,
        db = Some(DbEngine::Postgres),
        config_paths = NONE,
        version_env = &["PG_VERSION", "PG_MAJOR"]
    ),
    app!(
        "immich-postgres",
        "PostgreSQL (Immich)",
        ["immich-postgres", "postgres-immich"],
        None,
        db = Some(DbEngine::Postgres),
        config_paths = NONE,
        version_env = &["PG_VERSION", "PG_MAJOR"]
    ),
    app!(
        "mysql",
        "MySQL",
        ["mysql"],
        None,
        db = Some(DbEngine::Mysql),
        config_paths = NONE,
        version_env = &["MYSQL_VERSION"]
    ),
    app!(
        "mariadb",
        "MariaDB",
        ["mariadb"],
        None,
        db = Some(DbEngine::Mariadb),
        config_paths = NONE,
        version_env = &["MARIADB_VERSION"]
    ),
    app!(
        "redis",
        "Redis",
        ["redis", "valkey"],
        None,
        db = Some(DbEngine::Redis),
        config_paths = NONE,
        version_env = &["REDIS_VERSION", "VALKEY_VERSION"]
    ),
    app!(
        "mongo",
        "MongoDB",
        ["mongo", "mongodb"],
        None,
        db = Some(DbEngine::Mongo),
        config_paths = NONE,
        version_env = &["MONGO_VERSION"]
    ),
];

fn index() -> &'static HashMap<&'static str, &'static AppInfo> {
    static INDEX: std::sync::OnceLock<HashMap<&'static str, &'static AppInfo>> = std::sync::OnceLock::new();
    INDEX.get_or_init(|| {
        let mut m = HashMap::new();
        for app in CATALOG {
            for img in app.images {
                m.entry(*img).or_insert(app);
            }
        }
        m
    })
}

pub fn by_slug(slug: &str) -> Option<&'static AppInfo> {
    CATALOG.iter().find(|a| a.slug == slug)
}

/// Recognizes an app from its image reference, honoring a `nodestarr.app` label.
pub fn detect(image: &str, labels: &HashMap<String, String>) -> Option<&'static AppInfo> {
    if let Some(slug) = labels.get("nodestarr.app") {
        return by_slug(slug);
    }
    let reference = crate::registry::reference::ImageRef::parse(image)?;
    let repo = reference.repository.to_ascii_lowercase();
    let base = reference.repo_basename().to_ascii_lowercase();
    // Generic names like "server" only count with a known namespace.
    if base == "server" {
        return if repo.contains("vaultwarden") { by_slug("vaultwarden") } else { None };
    }
    if repo.contains("immich-app/postgres") || repo.contains("tensorchord/pgvecto-rs") {
        return by_slug("immich-postgres");
    }
    if let Some(app) = index().get(base.as_str()) {
        return Some(app);
    }
    // "docker-sonarr", "sonarr-anime", "tdarr_node" style names.
    let trimmed = base.trim_start_matches("docker-").trim_end_matches("-server").trim_end_matches("-docker");
    if let Some(app) = index().get(trimmed) {
        return Some(app);
    }
    let first = trimmed.split(['-', '_']).next().unwrap_or(trimmed);
    index().get(first).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(image: &str) -> Option<&'static str> {
        detect(image, &HashMap::new()).map(|a| a.slug)
    }

    #[test]
    fn detects_common_images() {
        assert_eq!(d("lscr.io/linuxserver/sonarr:develop"), Some("sonarr"));
        assert_eq!(d("ghcr.io/hotio/radarr:release"), Some("radarr"));
        assert_eq!(d("plexinc/pms-docker:latest"), Some("plex"));
        assert_eq!(d("lscr.io/linuxserver/plex"), Some("plex"));
        assert_eq!(d("ghcr.io/immich-app/immich-server:release"), Some("immich"));
        assert_eq!(d("ghcr.io/immich-app/postgres:14-vectorchord0.4.3"), Some("immich-postgres"));
        assert_eq!(d("ghcr.io/home-assistant/home-assistant:stable"), Some("homeassistant"));
        assert_eq!(d("ghcr.io/seerr-team/seerr:latest"), Some("seerr"));
        assert_eq!(d("ghcr.io/haveagitgat/tdarr:latest"), Some("tdarr"));
        assert_eq!(d("ghcr.io/haveagitgat/tdarr_node:latest"), Some("tdarr_node"));
        assert_eq!(d("mysql:8.4"), Some("mysql"));
        assert_eq!(d("vaultwarden/server:latest"), Some("vaultwarden"));
        assert_eq!(d("someone/server:1"), None);
        assert_eq!(d("cloudflare/cloudflared:latest"), Some("cloudflared"));
        assert_eq!(d("sonarr-anime:local"), Some("sonarr"));
        assert_eq!(d("busybox"), None);
    }

    #[test]
    fn label_override() {
        let mut labels = HashMap::new();
        labels.insert("nodestarr.app".to_string(), "radarr".to_string());
        assert_eq!(detect("myimage:1", &labels).map(|a| a.slug), Some("radarr"));
    }

    #[test]
    fn slugs_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for app in CATALOG {
            assert!(seen.insert(app.slug), "duplicate slug {}", app.slug);
        }
    }
}
