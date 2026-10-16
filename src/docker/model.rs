//! API-facing container types, derived from Docker's inspect output.

use std::collections::BTreeMap;

use bollard::models::{ContainerInspectResponse, ContainerStateStatusEnum, HealthStatusEnum};
use serde::Serialize;

use crate::docker::apps::{self, AppInfo};
use crate::docker::images::ImageMeta;

pub const LABEL_PROJECT: &str = "com.docker.compose.project";
pub const LABEL_SERVICE: &str = "com.docker.compose.service";
pub const LABEL_WORKING_DIR: &str = "com.docker.compose.project.working_dir";
pub const LABEL_CONFIG_FILES: &str = "com.docker.compose.project.config_files";
pub const LABEL_CONTAINER_NUMBER: &str = "com.docker.compose.container-number";
pub const LABEL_ONEOFF: &str = "com.docker.compose.oneoff";
pub const LABEL_ENV_FILE: &str = "com.docker.compose.project.environment_file";
/// Optional label to set a container's web UI link explicitly.
pub const LABEL_NODESTARR_URL: &str = "nodestarr.url";
/// Optional label to give a container a display name.
pub const LABEL_NODESTARR_NAME: &str = "nodestarr.name";
/// Optional label marking a container as Nodestarr itself.
pub const LABEL_NODESTARR_SELF: &str = "nodestarr.self";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PortInfo {
    pub private_port: u16,
    pub public_port: Option<u16>,
    pub protocol: String,
    pub host_ip: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MountInfo {
    /// "bind", "volume", "tmpfs", ...
    pub kind: String,
    /// Host path for binds, volume name for volumes.
    pub source: String,
    pub destination: String,
    pub rw: bool,
    /// Volume name for named volumes.
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WebLink {
    pub label: String,
    pub url: String,
}

/// Everything the container list needs, kept small so the list endpoint and
/// the live event stream stay cheap.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ContainerSummary {
    pub id: String,
    pub name: String,
    /// Stable key across recreations: "compose:<project>/<service>" or "container:<name>".
    pub identity: String,
    pub stack: Option<String>,
    pub service: Option<String>,
    /// Image reference as configured (e.g. lscr.io/linuxserver/sonarr:develop).
    pub image: String,
    pub image_id: String,
    pub state: String,
    pub health: Option<String>,
    pub exit_code: Option<i64>,
    pub oom_killed: bool,
    pub created: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub restart_count: i64,
    pub restart_policy: String,
    pub ports: Vec<PortInfo>,
    pub networks: Vec<String>,
    pub network_mode: String,
    pub mounts: Vec<MountInfo>,
    /// Detected application version (image labels, env or tag).
    pub version: Option<String>,
    pub version_source: Option<String>,
    /// Known application slug (e.g. "sonarr"), when recognized.
    pub app: Option<String>,
    pub app_name: Option<String>,
    pub links: Vec<WebLink>,
    pub compose_dir: Option<String>,
    pub compose_files: Vec<String>,
    /// True for the container running Nodestarr.
    pub is_self: bool,
    /// Name from the `nodestarr.name` label, if any.
    pub label_name: Option<String>,
}

impl ContainerSummary {
    pub fn is_running(&self) -> bool {
        self.state == "running"
    }

    pub fn is_compose(&self) -> bool {
        self.stack.is_some()
    }

    pub fn short_id(&self) -> &str {
        &self.id[..self.id.len().min(12)]
    }

    /// Name to show when the user hasn't set one: the `nodestarr.name` label,
    /// else the compose service (or container) name, using the app's proper
    /// name when the service is simply named after it ("sonarr" → "Sonarr").
    pub fn default_name(&self) -> String {
        if let Some(n) = &self.label_name {
            return n.clone();
        }
        let base = self.service.as_deref().unwrap_or(&self.name);
        if let (Some(slug), Some(app_name)) = (&self.app, &self.app_name)
            && (base.eq_ignore_ascii_case(slug) || base.eq_ignore_ascii_case(&app_name.replace(' ', "-")))
        {
            return app_name.clone();
        }
        base.to_string()
    }
}

/// Computes the stable identity for a container.
pub fn identity_for(labels: &std::collections::HashMap<String, String>, name: &str) -> String {
    match (labels.get(LABEL_PROJECT), labels.get(LABEL_SERVICE)) {
        (Some(project), Some(service)) => match labels.get(LABEL_CONTAINER_NUMBER).map(String::as_str) {
            Some(n) if n != "1" && !n.is_empty() => format!("compose:{project}/{service}#{n}"),
            _ => format!("compose:{project}/{service}"),
        },
        _ => format!("container:{name}"),
    }
}

pub struct SummaryInput<'a> {
    pub inspect: &'a ContainerInspectResponse,
    pub image: Option<&'a ImageMeta>,
    pub public_host: &'a str,
    pub self_id: Option<&'a str>,
}

/// Builds the summary from an inspect response.
pub fn summarize(input: SummaryInput<'_>) -> Option<ContainerSummary> {
    let inspect = input.inspect;
    let id = inspect.id.clone()?;
    let name = inspect.name.as_deref().unwrap_or_default().trim_start_matches('/').to_string();
    let config = inspect.config.as_ref();
    let labels = config.and_then(|c| c.labels.clone()).unwrap_or_default();
    let identity = identity_for(&labels, &name);
    let stack = labels.get(LABEL_PROJECT).cloned();
    let service = labels.get(LABEL_SERVICE).cloned();
    let image = config.and_then(|c| c.image.clone()).unwrap_or_default();
    let image_id = inspect.image.clone().unwrap_or_default();

    let state = inspect.state.as_ref();
    let state_str = state.and_then(|s| s.status).map(state_name).unwrap_or("unknown").to_string();
    let health = state
        .and_then(|s| s.health.as_ref())
        .and_then(|h| h.status)
        .and_then(|h| match h {
            HealthStatusEnum::HEALTHY => Some("healthy"),
            HealthStatusEnum::UNHEALTHY => Some("unhealthy"),
            HealthStatusEnum::STARTING => Some("starting"),
            _ => None,
        })
        .map(str::to_string);
    let exit_code = state.and_then(|s| s.exit_code);
    let started_at = state.and_then(|s| s.started_at.as_deref()).and_then(crate::util::parse_rfc3339);
    let finished_at = state.and_then(|s| s.finished_at.as_deref()).and_then(crate::util::parse_rfc3339);
    let created = inspect.created.as_ref().and_then(|d| crate::util::parse_rfc3339(&d.to_string())).unwrap_or(0);

    let host_config = inspect.host_config.as_ref();
    let network_mode = host_config.and_then(|h| h.network_mode.clone()).unwrap_or_default();
    let restart_policy = host_config
        .and_then(|h| h.restart_policy.as_ref())
        .and_then(|p| p.name)
        .map(|n| {
            use bollard::models::RestartPolicyNameEnum as R;
            match n {
                R::ALWAYS => "always",
                R::UNLESS_STOPPED => "unless-stopped",
                R::ON_FAILURE => "on-failure",
                _ => "no",
            }
        })
        .unwrap_or("no")
        .to_string();

    let ports = collect_ports(inspect);
    let networks = inspect
        .network_settings
        .as_ref()
        .and_then(|n| n.networks.as_ref())
        .map(|m| {
            let mut v: Vec<String> = m.keys().cloned().collect();
            v.sort();
            v
        })
        .unwrap_or_default();
    let mounts = collect_mounts(inspect);

    let app = apps::detect(&image, &labels);
    let (version, version_source) = detect_version(&image, input.image, app);

    let compose_dir = labels.get(LABEL_WORKING_DIR).cloned();
    let compose_files = labels
        .get(LABEL_CONFIG_FILES)
        .map(|s| s.split(',').map(str::trim).filter(|f| !f.is_empty()).map(str::to_string).collect())
        .unwrap_or_default();

    let links = web_links(&labels, &ports, &network_mode, app, input.public_host);
    let is_self = input.self_id.is_some_and(|sid| id.starts_with(sid) || sid.starts_with(&id))
        || labels.get(LABEL_NODESTARR_SELF).is_some_and(|v| v == "true");

    Some(ContainerSummary {
        id,
        name,
        identity,
        stack,
        service,
        image,
        image_id,
        state: state_str,
        health,
        exit_code,
        oom_killed: state.and_then(|s| s.oom_killed).unwrap_or(false),
        created,
        started_at,
        finished_at,
        restart_count: inspect.restart_count.unwrap_or(0),
        restart_policy,
        ports,
        networks,
        network_mode,
        mounts,
        version,
        version_source,
        app: app.map(|a| a.slug.to_string()),
        app_name: app.map(|a| a.name.to_string()),
        links,
        compose_dir,
        compose_files,
        is_self,
        label_name: labels.get(LABEL_NODESTARR_NAME).cloned(),
    })
}

pub fn state_name(s: ContainerStateStatusEnum) -> &'static str {
    match s {
        ContainerStateStatusEnum::CREATED => "created",
        ContainerStateStatusEnum::RUNNING => "running",
        ContainerStateStatusEnum::PAUSED => "paused",
        ContainerStateStatusEnum::RESTARTING => "restarting",
        ContainerStateStatusEnum::REMOVING => "removing",
        ContainerStateStatusEnum::EXITED => "exited",
        ContainerStateStatusEnum::DEAD => "dead",
        ContainerStateStatusEnum::STOPPING => "stopping",
        _ => "unknown",
    }
}

fn collect_ports(inspect: &ContainerInspectResponse) -> Vec<PortInfo> {
    let mut out = Vec::new();
    let bindings = inspect.network_settings.as_ref().and_then(|n| n.ports.as_ref());
    if let Some(map) = bindings {
        for (key, binds) in map {
            let (port, proto) = key.split_once('/').unwrap_or((key.as_str(), "tcp"));
            let Ok(private_port) = port.parse::<u16>() else {
                continue;
            };
            match binds {
                Some(list) if !list.is_empty() => {
                    for b in list {
                        let public_port = b.host_port.as_deref().and_then(|p| p.parse().ok());
                        let info = PortInfo {
                            private_port,
                            public_port,
                            protocol: proto.to_string(),
                            host_ip: b.host_ip.clone().filter(|ip| !ip.is_empty()),
                        };
                        // Docker lists IPv4 and IPv6 bindings separately; keep one.
                        if !out.iter().any(|o: &PortInfo| {
                            o.private_port == info.private_port
                                && o.public_port == info.public_port
                                && o.protocol == info.protocol
                        }) {
                            out.push(info);
                        }
                    }
                }
                _ => out.push(PortInfo { private_port, public_port: None, protocol: proto.to_string(), host_ip: None }),
            }
        }
    }
    out.sort_by_key(|p| (p.public_port.is_none(), p.private_port));
    out
}

fn collect_mounts(inspect: &ContainerInspectResponse) -> Vec<MountInfo> {
    inspect
        .mounts
        .as_ref()
        .map(|mounts| {
            mounts
                .iter()
                .filter_map(|m| {
                    let kind = m.typ.clone().unwrap_or_default();
                    let destination = m.destination.clone()?;
                    let source = match kind.as_str() {
                        "volume" => m.name.clone().or_else(|| m.source.clone()).unwrap_or_default(),
                        _ => m.source.clone().unwrap_or_default(),
                    };
                    Some(MountInfo { kind, source, destination, rw: m.rw.unwrap_or(true), name: m.name.clone() })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Version detection, most specific first:
/// 1. OCI `org.opencontainers.image.version` label,
/// 2. LinuxServer's `build_version` label,
/// 3. a `<APP>_VERSION` env var baked into the image (official images),
/// 4. the tag when it looks like a version.
pub fn detect_version(
    image_ref: &str,
    image: Option<&ImageMeta>,
    app: Option<&AppInfo>,
) -> (Option<String>, Option<String>) {
    if let Some(meta) = image {
        if let Some(v) = meta.labels.get("org.opencontainers.image.version").filter(|v| plausible_version(v)) {
            return (Some(v.clone()), Some("label".into()));
        }
        if let Some(bv) = meta.labels.get("build_version") {
            // "Linuxserver.io version:- 4.0.9.2244-ls250 Build-date:- 2024-..."
            if let Some(rest) = bv.split("version:-").nth(1)
                && let Some(v) = rest.split_whitespace().next()
            {
                return (Some(v.to_string()), Some("label".into()));
            }
        }
        for key in ["version", "org.label-schema.version"] {
            if let Some(v) = meta.labels.get(key).filter(|v| plausible_version(v)) {
                return (Some(v.clone()), Some("label".into()));
            }
        }
        if let Some(v) = version_from_env(image_ref, &meta.env, app) {
            return (Some(v), Some("env".into()));
        }
    }
    if let Some(r) = crate::registry::reference::ImageRef::parse(image_ref)
        && let Some(tag) = r.tag.as_deref()
        && crate::registry::version::Version::parse(tag).is_some()
    {
        return (Some(tag.to_string()), Some("tag".into()));
    }
    (None, None)
}

/// A `<APP>_VERSION` variable baked into an image (official images set one,
/// like `NGINX_VERSION`), or the app's own version variable. Also read from
/// a registry's image config, so a new build's version compares with this.
pub fn version_from_env(
    image_ref: &str,
    env: &std::collections::HashMap<String, String>,
    app: Option<&AppInfo>,
) -> Option<String> {
    let repo = crate::registry::reference::ImageRef::parse(image_ref)
        .map(|r| r.repo_basename().to_ascii_uppercase().replace('-', "_"))
        .unwrap_or_default();
    let own = (!repo.is_empty()).then(|| format!("{repo}_VERSION"));
    let app_keys = app.map(|a| a.version_env).unwrap_or_default().iter().map(|k| k.to_string());
    own.into_iter().chain(app_keys).find_map(|key| env.get(&key).filter(|v| plausible_version(v)).cloned())
}

fn plausible_version(v: &str) -> bool {
    let v = v.trim();
    !v.is_empty() && v.len() < 64 && v.chars().any(|c| c.is_ascii_digit()) && !v.contains(' ')
}

fn web_links(
    labels: &std::collections::HashMap<String, String>,
    ports: &[PortInfo],
    network_mode: &str,
    app: Option<&AppInfo>,
    public_host: &str,
) -> Vec<WebLink> {
    let mut links = Vec::new();
    if let Some(url) = labels.get(LABEL_NODESTARR_URL).filter(|u| !u.is_empty()) {
        links.push(WebLink { label: "Open".into(), url: url.clone() });
        return links;
    }
    // Traefik routers: Host(`app.example.com`)
    let mut traefik_hosts = BTreeMap::new();
    for (k, v) in labels {
        if k.starts_with("traefik.http.routers.")
            && k.ends_with(".rule")
            && let Some(start) = v.find("Host(`")
        {
            let rest = &v[start + 6..];
            if let Some(end) = rest.find('`') {
                traefik_hosts.insert(rest[..end].to_string(), ());
            }
        }
    }
    for host in traefik_hosts.keys() {
        links.push(WebLink { label: host.clone(), url: format!("https://{host}") });
    }

    let path = app.map(|a| a.web_path).unwrap_or("/");
    let scheme_for = |port: u16| {
        if app.is_some_and(|a| a.web_https) || matches!(port, 443 | 8443 | 9443) { "https" } else { "http" }
    };
    if network_mode == "host" {
        if let Some(app) = app
            && let Some(port) = app.web_port
        {
            links.push(WebLink {
                label: format!("{public_host}:{port}"),
                url: format!("{}://{public_host}:{port}{path}", scheme_for(port)),
            });
        }
        return links;
    }
    let tcp: Vec<&PortInfo> = ports.iter().filter(|p| p.protocol == "tcp" && p.public_port.is_some()).collect();
    // Prefer the app's known web port, then common web ports, then the lowest port.
    let preferred = app
        .and_then(|a| a.web_port)
        .and_then(|wp| tcp.iter().find(|p| p.private_port == wp))
        .or_else(|| {
            tcp.iter()
                .find(|p| matches!(p.private_port, 80 | 443 | 3000 | 5000 | 8000 | 8080 | 8081 | 8443 | 9000 | 9443))
        })
        .or_else(|| tcp.first())
        .copied();
    if let Some(p) = preferred {
        let port = p.public_port.unwrap_or(p.private_port);
        let host = match p.host_ip.as_deref() {
            Some(ip) if ip != "0.0.0.0" && ip != "::" => ip,
            _ => public_host,
        };
        let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_string() };
        links.push(WebLink {
            label: format!("{host}:{port}"),
            url: format!("{}://{host}:{port}{path}", scheme_for(p.private_port)),
        });
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn identity_prefers_compose_labels() {
        let mut labels = HashMap::new();
        assert_eq!(identity_for(&labels, "plex"), "container:plex");
        labels.insert(LABEL_PROJECT.to_string(), "media".to_string());
        labels.insert(LABEL_SERVICE.to_string(), "sonarr".to_string());
        assert_eq!(identity_for(&labels, "media-sonarr-1"), "compose:media/sonarr");
        labels.insert(LABEL_CONTAINER_NUMBER.to_string(), "2".to_string());
        assert_eq!(identity_for(&labels, "media-sonarr-2"), "compose:media/sonarr#2");
    }

    #[test]
    fn version_from_lsio_label() {
        let mut meta = ImageMeta::default();
        meta.labels.insert(
            "build_version".into(),
            "Linuxserver.io version:- 4.0.9.2244-ls250 Build-date:- 2024-09-01T00:00:00+00:00".into(),
        );
        let (v, src) = detect_version("lscr.io/linuxserver/sonarr:latest", Some(&meta), None);
        assert_eq!(v.as_deref(), Some("4.0.9.2244-ls250"));
        assert_eq!(src.as_deref(), Some("label"));
    }

    #[test]
    fn version_from_env_and_tag() {
        let mut meta = ImageMeta::default();
        meta.env.insert("NGINX_VERSION".into(), "1.27.1".into());
        let (v, src) = detect_version("nginx:latest", Some(&meta), None);
        assert_eq!(v.as_deref(), Some("1.27.1"));
        assert_eq!(src.as_deref(), Some("env"));
        let (v, src) = detect_version("mysql:8.4.2", None, None);
        assert_eq!(v.as_deref(), Some("8.4.2"));
        assert_eq!(src.as_deref(), Some("tag"));
        let (v, _) = detect_version("cloudflare/cloudflared:latest", None, None);
        assert_eq!(v, None);
    }

    #[test]
    fn links_from_ports() {
        let ports = vec![PortInfo {
            private_port: 8989,
            public_port: Some(8989),
            protocol: "tcp".into(),
            host_ip: Some("0.0.0.0".into()),
        }];
        let labels = HashMap::new();
        let app = apps::by_slug("sonarr");
        let links = web_links(&labels, &ports, "bridge", app, "192.168.1.50");
        assert_eq!(links[0].url, "http://192.168.1.50:8989/");

        let plex = apps::by_slug("plex");
        let links = web_links(&labels, &[], "host", plex, "192.168.1.50");
        assert_eq!(links[0].url, "http://192.168.1.50:32400/web");
    }
}
