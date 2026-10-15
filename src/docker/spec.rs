//! Container specs for the "Add container" and "Edit container" forms,
//! conversion to and from Docker's API, `docker run` import, and export to
//! a compose service.

use std::collections::{BTreeMap, HashMap};

use bollard::models::{
    ContainerCreateBody, ContainerInspectResponse, DeviceMapping, DeviceRequest, EndpointSettings, HealthConfig,
    HostConfig, HostConfigLogConfig, ImageConfig, Mount, MountType, NetworkingConfig, PortBinding, RestartPolicy,
    RestartPolicyNameEnum,
};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PortSpec {
    pub host_ip: String,
    /// Empty publishes on a random host port.
    pub host_port: String,
    pub container_port: String,
    pub protocol: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VolumeSpec {
    /// "bind", "volume" or "tmpfs".
    pub kind: String,
    /// Host path (bind) or volume name (volume); empty for anonymous volumes.
    pub source: String,
    pub target: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DeviceSpec {
    pub host: String,
    pub container: String,
    pub permissions: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct HealthSpec {
    /// Shell command (run with CMD-SHELL).
    pub command: String,
    pub interval_secs: u32,
    pub timeout_secs: u32,
    pub retries: u32,
    pub start_period_secs: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ContainerSpec {
    pub name: String,
    pub image: String,
    /// "no", "always", "unless-stopped", "on-failure" or "on-failure:N".
    pub restart: String,
    /// "bridge", "host", "none", a network name, or "container:<name>".
    pub network_mode: String,
    /// Extra networks to connect.
    pub networks: Vec<String>,
    pub ports: Vec<PortSpec>,
    pub volumes: Vec<VolumeSpec>,
    pub env: Vec<KeyValue>,
    pub labels: Vec<KeyValue>,
    pub devices: Vec<DeviceSpec>,
    pub group_add: Vec<String>,
    pub cap_add: Vec<String>,
    pub cap_drop: Vec<String>,
    pub privileged: bool,
    pub user: String,
    pub hostname: String,
    pub working_dir: String,
    /// Empty uses the image default.
    pub command: Vec<String>,
    pub entrypoint: Vec<String>,
    /// Memory limit in bytes.
    pub memory: Option<i64>,
    pub cpus: Option<f64>,
    pub shm_size: Option<i64>,
    pub extra_hosts: Vec<String>,
    pub dns: Vec<String>,
    /// NVIDIA GPUs: "all" or a count.
    pub gpus: String,
    pub tty: bool,
    pub stdin_open: bool,
    pub init: bool,
    pub healthcheck: Option<HealthSpec>,
    pub log_driver: String,
    pub log_opts: Vec<KeyValue>,
    pub security_opt: Vec<String>,
    pub sysctls: Vec<KeyValue>,
    pub tmpfs: Vec<String>,
}

/// Result of importing a `docker run` command.
#[derive(Debug, Clone, Serialize)]
pub struct ParsedRun {
    pub spec: ContainerSpec,
    /// Flags Nodestarr could not translate (shown to the user).
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Spec -> Docker API
// ---------------------------------------------------------------------------

impl ContainerSpec {
    pub fn validate(&self) -> Result<()> {
        if self.image.trim().is_empty() {
            return Err(AppError::bad_request("image is required"));
        }
        if crate::registry::ImageRef::parse(&self.image).is_none() {
            return Err(AppError::bad_request(format!("'{}' is not a valid image reference", self.image)));
        }
        if !self.name.is_empty()
            && !self
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        {
            return Err(AppError::bad_request(
                "container names may only contain letters, digits, '_', '.' and '-'",
            ));
        }
        for p in &self.ports {
            if p.container_port.trim().is_empty() {
                return Err(AppError::bad_request("every port needs a container port"));
            }
        }
        for v in &self.volumes {
            if !v.target.starts_with('/') {
                return Err(AppError::bad_request(format!("volume target '{}' must be an absolute path", v.target)));
            }
            if v.kind == "bind" && !v.source.starts_with('/') {
                return Err(AppError::bad_request(format!("bind source '{}' must be an absolute path", v.source)));
            }
        }
        parse_restart(&self.restart)?;
        Ok(())
    }

    /// Builds the create body; extra networks are returned separately since
    /// they are connected after creation.
    pub fn to_create_body(&self) -> Result<(ContainerCreateBody, Vec<String>)> {
        self.validate()?;
        let mut exposed = Vec::new();
        let mut bindings: HashMap<String, Option<Vec<PortBinding>>> = HashMap::new();
        for p in &self.ports {
            let proto = if p.protocol.is_empty() { "tcp" } else { p.protocol.as_str() };
            let cports = expand_range(&p.container_port)?;
            let hports = if p.host_port.trim().is_empty() {
                vec![String::new(); cports.len()]
            } else {
                let h = expand_range(&p.host_port)?;
                if h.len() != cports.len() {
                    return Err(AppError::bad_request(format!(
                        "port range {}:{} has mismatched lengths",
                        p.host_port, p.container_port
                    )));
                }
                h
            };
            for (cp, hp) in cports.iter().zip(hports) {
                let key = format!("{cp}/{proto}");
                exposed.push(key.clone());
                bindings.entry(key).or_insert_with(|| Some(Vec::new())).get_or_insert_with(Vec::new).push(PortBinding {
                    host_ip: if p.host_ip.is_empty() { None } else { Some(p.host_ip.clone()) },
                    host_port: Some(hp),
                });
            }
        }

        let mut binds = Vec::new();
        let mut mounts = Vec::new();
        let mut tmpfs: HashMap<String, String> = self
            .tmpfs
            .iter()
            .map(|t| match t.split_once(':') {
                Some((p, o)) => (p.to_string(), o.to_string()),
                None => (t.clone(), String::new()),
            })
            .collect();
        for v in &self.volumes {
            match v.kind.as_str() {
                "bind" => binds.push(format!("{}:{}{}", v.source, v.target, if v.read_only { ":ro" } else { "" })),
                "tmpfs" => {
                    tmpfs.insert(v.target.clone(), String::new());
                }
                _ => {
                    if v.source.is_empty() {
                        mounts.push(Mount {
                            target: Some(v.target.clone()),
                            typ: Some(MountType::VOLUME),
                            read_only: Some(v.read_only),
                            ..Default::default()
                        });
                    } else {
                        binds.push(format!("{}:{}{}", v.source, v.target, if v.read_only { ":ro" } else { "" }));
                    }
                }
            }
        }

        let devices: Vec<DeviceMapping> = self
            .devices
            .iter()
            .map(|d| DeviceMapping {
                path_on_host: Some(d.host.clone()),
                path_in_container: Some(if d.container.is_empty() { d.host.clone() } else { d.container.clone() }),
                cgroup_permissions: Some(if d.permissions.is_empty() { "rwm".into() } else { d.permissions.clone() }),
            })
            .collect();

        let device_requests = match self.gpus.trim() {
            "" => None,
            g => Some(vec![DeviceRequest {
                driver: Some("nvidia".into()),
                count: Some(if g == "all" { -1 } else { g.parse().map_err(|_| AppError::bad_request("gpus must be 'all' or a number"))? }),
                capabilities: Some(vec![vec!["gpu".into()]]),
                ..Default::default()
            }]),
        };

        let network_mode = if self.network_mode.trim().is_empty() {
            "bridge".to_string()
        } else {
            self.network_mode.trim().to_string()
        };
        let host = HostConfig {
            binds: (!binds.is_empty()).then_some(binds),
            mounts: (!mounts.is_empty()).then_some(mounts),
            port_bindings: (!bindings.is_empty()).then_some(bindings),
            restart_policy: Some(parse_restart(&self.restart)?),
            network_mode: Some(network_mode.clone()),
            devices: (!devices.is_empty()).then_some(devices),
            device_requests,
            group_add: non_empty(&self.group_add),
            cap_add: non_empty(&self.cap_add),
            cap_drop: non_empty(&self.cap_drop),
            privileged: Some(self.privileged),
            memory: self.memory,
            nano_cpus: self.cpus.map(|c| (c * 1e9) as i64),
            shm_size: self.shm_size,
            extra_hosts: non_empty(&self.extra_hosts),
            dns: non_empty(&self.dns),
            init: self.init.then_some(true),
            security_opt: non_empty(&self.security_opt),
            sysctls: kv_map(&self.sysctls),
            tmpfs: (!tmpfs.is_empty()).then_some(tmpfs),
            log_config: if self.log_driver.is_empty() {
                None
            } else {
                Some(HostConfigLogConfig {
                    typ: Some(self.log_driver.clone()),
                    config: kv_map(&self.log_opts),
                })
            },
            ..Default::default()
        };

        let networking_config = if matches!(network_mode.as_str(), "host" | "none" | "bridge" | "default")
            || network_mode.starts_with("container:")
        {
            None
        } else {
            Some(NetworkingConfig {
                endpoints_config: Some(HashMap::from([(network_mode.clone(), EndpointSettings::default())])),
            })
        };

        let healthcheck = self.healthcheck.as_ref().filter(|h| !h.command.trim().is_empty()).map(|h| HealthConfig {
            test: Some(vec!["CMD-SHELL".into(), h.command.clone()]),
            interval: secs_to_ns(h.interval_secs),
            timeout: secs_to_ns(h.timeout_secs),
            retries: (h.retries > 0).then_some(h.retries as i64),
            start_period: secs_to_ns(h.start_period_secs),
            start_interval: None,
        });

        let env: Vec<String> = self
            .env
            .iter()
            .filter(|kv| !kv.key.trim().is_empty())
            .map(|kv| format!("{}={}", kv.key.trim(), kv.value))
            .collect();
        exposed.sort();
        exposed.dedup();

        let body = ContainerCreateBody {
            image: Some(self.image.trim().to_string()),
            hostname: (!self.hostname.is_empty()).then(|| self.hostname.clone()),
            user: (!self.user.is_empty()).then(|| self.user.clone()),
            working_dir: (!self.working_dir.is_empty()).then(|| self.working_dir.clone()),
            env: (!env.is_empty()).then_some(env),
            cmd: non_empty(&self.command),
            entrypoint: non_empty(&self.entrypoint),
            labels: kv_map(&self.labels),
            exposed_ports: (!exposed.is_empty()).then_some(exposed),
            tty: Some(self.tty),
            open_stdin: Some(self.stdin_open),
            healthcheck,
            host_config: Some(host),
            networking_config,
            ..Default::default()
        };
        let extra: Vec<String> = self
            .networks
            .iter()
            .filter(|n| !n.is_empty() && **n != network_mode)
            .cloned()
            .collect();
        Ok((body, extra))
    }

    /// Reconstructs a spec from a running container, leaving out values that
    /// came from the image so the form only shows what the user set.
    pub fn from_inspect(inspect: &ContainerInspectResponse, image: Option<&ImageConfig>) -> Self {
        let body = crate::docker::recreate::body_from_inspect(
            inspect,
            image,
            inspect.config.as_ref().and_then(|c| c.image.as_deref()).unwrap_or_default(),
        );
        let host = body.host_config.clone().unwrap_or_default();
        let mut spec = ContainerSpec {
            name: inspect.name.clone().unwrap_or_default().trim_start_matches('/').to_string(),
            image: body.image.clone().unwrap_or_default(),
            hostname: body.hostname.clone().unwrap_or_default(),
            user: body.user.clone().unwrap_or_default(),
            working_dir: body.working_dir.clone().unwrap_or_default(),
            command: body.cmd.clone().unwrap_or_default(),
            entrypoint: body.entrypoint.clone().unwrap_or_default(),
            tty: body.tty.unwrap_or(false),
            stdin_open: body.open_stdin.unwrap_or(false),
            privileged: host.privileged.unwrap_or(false),
            memory: host.memory.filter(|m| *m > 0),
            cpus: host.nano_cpus.filter(|n| *n > 0).map(|n| n as f64 / 1e9),
            shm_size: host.shm_size.filter(|s| *s > 0 && *s != 67_108_864),
            network_mode: host.network_mode.clone().unwrap_or_default(),
            group_add: host.group_add.clone().unwrap_or_default(),
            cap_add: host.cap_add.clone().unwrap_or_default(),
            cap_drop: host.cap_drop.clone().unwrap_or_default(),
            extra_hosts: host.extra_hosts.clone().unwrap_or_default(),
            dns: host.dns.clone().unwrap_or_default(),
            init: host.init.unwrap_or(false),
            security_opt: host.security_opt.clone().unwrap_or_default(),
            ..Default::default()
        };
        spec.restart = match host.restart_policy.as_ref().and_then(|r| r.name) {
            Some(RestartPolicyNameEnum::ALWAYS) => "always".into(),
            Some(RestartPolicyNameEnum::UNLESS_STOPPED) => "unless-stopped".into(),
            Some(RestartPolicyNameEnum::ON_FAILURE) => {
                match host.restart_policy.as_ref().and_then(|r| r.maximum_retry_count).filter(|n| *n > 0) {
                    Some(n) => format!("on-failure:{n}"),
                    None => "on-failure".into(),
                }
            }
            _ => "no".into(),
        };
        for (key, binds) in host.port_bindings.iter().flatten() {
            let (cport, proto) = key.split_once('/').unwrap_or((key.as_str(), "tcp"));
            for b in binds.iter().flatten() {
                spec.ports.push(PortSpec {
                    host_ip: b.host_ip.clone().unwrap_or_default(),
                    host_port: b.host_port.clone().unwrap_or_default(),
                    container_port: cport.to_string(),
                    protocol: proto.to_string(),
                });
            }
        }
        spec.ports.sort_by(|a, b| a.container_port.cmp(&b.container_port));
        for b in host.binds.iter().flatten() {
            let parts: Vec<&str> = b.split(':').collect();
            if parts.len() >= 2 {
                spec.volumes.push(VolumeSpec {
                    kind: if parts[0].starts_with('/') { "bind".into() } else { "volume".into() },
                    source: parts[0].to_string(),
                    target: parts[1].to_string(),
                    read_only: parts.get(2).is_some_and(|o| o.split(',').any(|x| x == "ro")),
                });
            }
        }
        for m in host.mounts.iter().flatten() {
            spec.volumes.push(VolumeSpec {
                kind: match m.typ {
                    Some(MountType::BIND) => "bind".into(),
                    Some(MountType::TMPFS) => "tmpfs".into(),
                    _ => "volume".into(),
                },
                source: m.source.clone().unwrap_or_default(),
                target: m.target.clone().unwrap_or_default(),
                read_only: m.read_only.unwrap_or(false),
            });
        }
        for (path, _) in host.tmpfs.iter().flatten() {
            spec.tmpfs.push(path.clone());
        }
        for d in host.devices.iter().flatten() {
            spec.devices.push(DeviceSpec {
                host: d.path_on_host.clone().unwrap_or_default(),
                container: d.path_in_container.clone().unwrap_or_default(),
                permissions: d.cgroup_permissions.clone().unwrap_or_default(),
            });
        }
        if let Some(req) = host.device_requests.iter().flatten().find(|r| r.driver.as_deref() == Some("nvidia")) {
            spec.gpus = match req.count {
                Some(-1) | None => "all".into(),
                Some(n) => n.to_string(),
            };
        }
        spec.env = body
            .env
            .iter()
            .flatten()
            .map(|e| match e.split_once('=') {
                Some((k, v)) => KeyValue { key: k.into(), value: v.into() },
                None => KeyValue { key: e.clone(), value: String::new() },
            })
            .collect();
        spec.labels = body
            .labels
            .iter()
            .flatten()
            .filter(|(k, _)| !k.starts_with("com.docker.compose."))
            .map(|(k, v)| KeyValue { key: k.clone(), value: v.clone() })
            .collect();
        spec.labels.sort_by(|a, b| a.key.cmp(&b.key));
        if let Some(lc) = &host.log_config {
            let driver = lc.typ.clone().unwrap_or_default();
            if driver != "json-file" || lc.config.as_ref().is_some_and(|c| !c.is_empty()) {
                spec.log_driver = driver;
                spec.log_opts = lc
                    .config
                    .iter()
                    .flatten()
                    .map(|(k, v)| KeyValue { key: k.clone(), value: v.clone() })
                    .collect();
            }
        }
        if let Some(sys) = &host.sysctls {
            spec.sysctls = sys.iter().map(|(k, v)| KeyValue { key: k.clone(), value: v.clone() }).collect();
        }
        if let Some(hc) = &body.healthcheck {
            if let Some(test) = &hc.test {
                if test.first().map(String::as_str) == Some("CMD-SHELL") || test.first().map(String::as_str) == Some("CMD") {
                    spec.healthcheck = Some(HealthSpec {
                        command: test[1..].join(" "),
                        interval_secs: ns_to_secs(hc.interval),
                        timeout_secs: ns_to_secs(hc.timeout),
                        retries: hc.retries.unwrap_or(0) as u32,
                        start_period_secs: ns_to_secs(hc.start_period),
                    });
                }
            }
        }
        if let Some(networks) = inspect.network_settings.as_ref().and_then(|n| n.networks.as_ref()) {
            let mut extra: Vec<String> = networks.keys().filter(|n| **n != spec.network_mode).cloned().collect();
            extra.sort();
            spec.networks = extra;
        }
        spec
    }

    /// Renders the spec as a compose service definition.
    pub fn to_compose(&self) -> String {
        let mut out = String::new();
        let service = if self.name.is_empty() {
            crate::registry::ImageRef::parse(&self.image)
                .map(|r| r.repo_basename().to_string())
                .unwrap_or_else(|| "app".into())
        } else {
            self.name.clone()
        };
        out.push_str("services:\n");
        out.push_str(&format!("  {}:\n", yaml_key(&service)));
        out.push_str(&format!("    image: {}\n", yaml_str(&self.image)));
        if !self.name.is_empty() {
            out.push_str(&format!("    container_name: {}\n", yaml_str(&self.name)));
        }
        let restart = if self.restart.is_empty() { "unless-stopped" } else { self.restart.as_str() };
        out.push_str(&format!("    restart: {}\n", yaml_str(restart)));
        let list = |out: &mut String, key: &str, items: &[String]| {
            if !items.is_empty() {
                out.push_str(&format!("    {key}:\n"));
                for i in items {
                    out.push_str(&format!("      - {}\n", yaml_str(i)));
                }
            }
        };
        if !self.network_mode.is_empty() && self.network_mode != "bridge" && self.network_mode != "default" {
            if matches!(self.network_mode.as_str(), "host" | "none") || self.network_mode.starts_with("container:") {
                out.push_str(&format!("    network_mode: {}\n", yaml_str(&self.network_mode)));
            } else {
                let mut nets = vec![self.network_mode.clone()];
                nets.extend(self.networks.iter().cloned());
                list(&mut out, "networks", &nets);
            }
        } else if !self.networks.is_empty() {
            list(&mut out, "networks", &self.networks);
        }
        if !self.hostname.is_empty() {
            out.push_str(&format!("    hostname: {}\n", yaml_str(&self.hostname)));
        }
        if !self.user.is_empty() {
            out.push_str(&format!("    user: {}\n", yaml_str(&self.user)));
        }
        if !self.working_dir.is_empty() {
            out.push_str(&format!("    working_dir: {}\n", yaml_str(&self.working_dir)));
        }
        if !self.entrypoint.is_empty() {
            list(&mut out, "entrypoint", &self.entrypoint);
        }
        if !self.command.is_empty() {
            list(&mut out, "command", &self.command);
        }
        let ports: Vec<String> = self
            .ports
            .iter()
            .map(|p| {
                let mut s = String::new();
                if !p.host_ip.is_empty() {
                    s.push_str(&p.host_ip);
                    s.push(':');
                }
                if !p.host_port.is_empty() {
                    s.push_str(&p.host_port);
                    s.push(':');
                }
                s.push_str(&p.container_port);
                if !p.protocol.is_empty() && p.protocol != "tcp" {
                    s.push('/');
                    s.push_str(&p.protocol);
                }
                s
            })
            .collect();
        list(&mut out, "ports", &ports);
        let vols: Vec<String> = self
            .volumes
            .iter()
            .filter(|v| v.kind != "tmpfs")
            .map(|v| {
                let mut s = if v.source.is_empty() { v.target.clone() } else { format!("{}:{}", v.source, v.target) };
                if v.read_only {
                    s.push_str(":ro");
                }
                s
            })
            .collect();
        list(&mut out, "volumes", &vols);
        let tmpfs: Vec<String> = self
            .tmpfs
            .iter()
            .cloned()
            .chain(self.volumes.iter().filter(|v| v.kind == "tmpfs").map(|v| v.target.clone()))
            .collect();
        list(&mut out, "tmpfs", &tmpfs);
        if !self.env.is_empty() {
            out.push_str("    environment:\n");
            for kv in &self.env {
                out.push_str(&format!("      {}: {}\n", yaml_key(&kv.key), yaml_str(&kv.value)));
            }
        }
        let devices: Vec<String> = self
            .devices
            .iter()
            .map(|d| {
                let c = if d.container.is_empty() { &d.host } else { &d.container };
                if d.permissions.is_empty() || d.permissions == "rwm" {
                    format!("{}:{}", d.host, c)
                } else {
                    format!("{}:{}:{}", d.host, c, d.permissions)
                }
            })
            .collect();
        list(&mut out, "devices", &devices);
        list(&mut out, "group_add", &self.group_add);
        list(&mut out, "cap_add", &self.cap_add);
        list(&mut out, "cap_drop", &self.cap_drop);
        list(&mut out, "extra_hosts", &self.extra_hosts);
        list(&mut out, "dns", &self.dns);
        list(&mut out, "security_opt", &self.security_opt);
        if self.privileged {
            out.push_str("    privileged: true\n");
        }
        if self.init {
            out.push_str("    init: true\n");
        }
        if self.tty {
            out.push_str("    tty: true\n");
        }
        if self.stdin_open {
            out.push_str("    stdin_open: true\n");
        }
        if let Some(mem) = self.memory {
            out.push_str(&format!("    mem_limit: {mem}\n"));
        }
        if let Some(cpus) = self.cpus {
            out.push_str(&format!("    cpus: {cpus}\n"));
        }
        if let Some(shm) = self.shm_size {
            out.push_str(&format!("    shm_size: {shm}\n"));
        }
        if !self.sysctls.is_empty() {
            out.push_str("    sysctls:\n");
            for kv in &self.sysctls {
                out.push_str(&format!("      {}: {}\n", yaml_key(&kv.key), yaml_str(&kv.value)));
            }
        }
        if !self.labels.is_empty() {
            out.push_str("    labels:\n");
            for kv in &self.labels {
                out.push_str(&format!("      {}: {}\n", yaml_key(&kv.key), yaml_str(&kv.value)));
            }
        }
        if !self.gpus.is_empty() {
            out.push_str("    deploy:\n      resources:\n        reservations:\n          devices:\n");
            out.push_str("            - driver: nvidia\n");
            if self.gpus == "all" {
                out.push_str("              count: all\n");
            } else {
                out.push_str(&format!("              count: {}\n", self.gpus));
            }
            out.push_str("              capabilities: [gpu]\n");
        }
        if let Some(h) = &self.healthcheck {
            if !h.command.is_empty() {
                out.push_str("    healthcheck:\n");
                out.push_str(&format!("      test: [\"CMD-SHELL\", {}]\n", json_str(&h.command)));
                if h.interval_secs > 0 {
                    out.push_str(&format!("      interval: {}s\n", h.interval_secs));
                }
                if h.timeout_secs > 0 {
                    out.push_str(&format!("      timeout: {}s\n", h.timeout_secs));
                }
                if h.retries > 0 {
                    out.push_str(&format!("      retries: {}\n", h.retries));
                }
                if h.start_period_secs > 0 {
                    out.push_str(&format!("      start_period: {}s\n", h.start_period_secs));
                }
            }
        }
        if !self.log_driver.is_empty() {
            out.push_str(&format!("    logging:\n      driver: {}\n", yaml_str(&self.log_driver)));
            if !self.log_opts.is_empty() {
                out.push_str("      options:\n");
                for kv in &self.log_opts {
                    out.push_str(&format!("        {}: {}\n", yaml_key(&kv.key), yaml_str(&kv.value)));
                }
            }
        }
        // Declare external networks so `docker compose up` doesn't create new ones.
        let mut nets: Vec<&String> = Vec::new();
        if !self.network_mode.is_empty()
            && !matches!(self.network_mode.as_str(), "host" | "none" | "bridge" | "default")
            && !self.network_mode.starts_with("container:")
        {
            nets.push(&self.network_mode);
        }
        nets.extend(self.networks.iter());
        if !nets.is_empty() {
            out.push_str("\nnetworks:\n");
            for n in nets {
                out.push_str(&format!("  {}:\n    external: true\n", yaml_key(n)));
            }
        }
        out
    }
}

fn non_empty(v: &[String]) -> Option<Vec<String>> {
    let f: Vec<String> = v.iter().filter(|s| !s.trim().is_empty()).cloned().collect();
    (!f.is_empty()).then_some(f)
}

fn kv_map(v: &[KeyValue]) -> Option<HashMap<String, String>> {
    let m: HashMap<String, String> = v
        .iter()
        .filter(|kv| !kv.key.trim().is_empty())
        .map(|kv| (kv.key.trim().to_string(), kv.value.clone()))
        .collect();
    (!m.is_empty()).then_some(m)
}

fn secs_to_ns(s: u32) -> Option<i64> {
    (s > 0).then_some(s as i64 * 1_000_000_000)
}

fn ns_to_secs(ns: Option<i64>) -> u32 {
    ns.map(|n| (n / 1_000_000_000) as u32).unwrap_or(0)
}

fn parse_restart(s: &str) -> Result<RestartPolicy> {
    let (name, count) = match s.trim() {
        "" | "no" => (RestartPolicyNameEnum::NO, None),
        "always" => (RestartPolicyNameEnum::ALWAYS, None),
        "unless-stopped" => (RestartPolicyNameEnum::UNLESS_STOPPED, None),
        "on-failure" => (RestartPolicyNameEnum::ON_FAILURE, None),
        other => match other.strip_prefix("on-failure:") {
            Some(n) => (
                RestartPolicyNameEnum::ON_FAILURE,
                Some(n.parse::<i64>().map_err(|_| AppError::bad_request(format!("invalid restart policy '{other}'")))?),
            ),
            None => return Err(AppError::bad_request(format!("invalid restart policy '{other}'"))),
        },
    };
    Ok(RestartPolicy {
        name: Some(name),
        maximum_retry_count: count,
    })
}

fn expand_range(s: &str) -> Result<Vec<String>> {
    let s = s.trim();
    match s.split_once('-') {
        Some((a, b)) => {
            let (a, b): (u16, u16) = (
                a.parse().map_err(|_| AppError::bad_request(format!("invalid port '{s}'")))?,
                b.parse().map_err(|_| AppError::bad_request(format!("invalid port '{s}'")))?,
            );
            if b < a || b - a > 1000 {
                return Err(AppError::bad_request(format!("invalid port range '{s}'")));
            }
            Ok((a..=b).map(|p| p.to_string()).collect())
        }
        None => {
            s.parse::<u16>().map_err(|_| AppError::bad_request(format!("invalid port '{s}'")))?;
            Ok(vec![s.to_string()])
        }
    }
}

/// Quotes a scalar for YAML only when needed.
fn yaml_str(s: &str) -> String {
    let needs_quotes = s.is_empty()
        || s.starts_with(|c: char| " -?:,[]{}#&*!|>'\"%@`".contains(c))
        || s.ends_with(' ')
        || s.contains(": ")
        || s.contains(" #")
        || matches!(s.to_ascii_lowercase().as_str(), "yes" | "no" | "true" | "false" | "on" | "off" | "null" | "~")
        || s.parse::<f64>().is_ok();
    if needs_quotes { json_str(s) } else { s.to_string() }
}

fn yaml_key(s: &str) -> String {
    yaml_str(s)
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""))
}

/// Parses "512m", "2g", "1073741824" into bytes.
pub fn parse_bytes(s: &str) -> Option<i64> {
    let s = s.trim().to_ascii_lowercase();
    let (num, mult) = match s.chars().last()? {
        'b' => (s.trim_end_matches('b'), 1i64),
        'k' => (&s[..s.len() - 1], 1024),
        'm' => (&s[..s.len() - 1], 1024 * 1024),
        'g' => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        _ => (s.as_str(), 1),
    };
    let n: f64 = num.trim().parse().ok()?;
    Some((n * mult as f64) as i64)
}

/// Parses "30s", "1m30s", "500ms", "2h" into seconds.
fn parse_duration_secs(s: &str) -> Option<u32> {
    let mut total = 0f64;
    let mut num = String::new();
    let mut chars = s.trim().chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
            continue;
        }
        let n: f64 = num.parse().ok()?;
        num.clear();
        let unit = if c == 'm' && chars.peek() == Some(&'s') {
            chars.next();
            0.001
        } else {
            match c {
                'h' => 3600.0,
                'm' => 60.0,
                's' => 1.0,
                _ => return None,
            }
        };
        total += n * unit;
    }
    if !num.is_empty() {
        total += num.parse::<f64>().ok()?;
    }
    Some(total.round() as u32)
}

// ---------------------------------------------------------------------------
// `docker run` import
// ---------------------------------------------------------------------------

/// Flags that take a value.
const VALUE_FLAGS: &[&str] = &[
    "--name", "-p", "--publish", "-v", "--volume", "--mount", "-e", "--env", "--env-file", "-l", "--label",
    "--restart", "--network", "--net", "--network-alias", "--net-alias", "--device", "--group-add", "--cap-add",
    "--cap-drop", "-u", "--user", "-h", "--hostname", "-w", "--workdir", "--entrypoint", "-m", "--memory", "--cpus",
    "--shm-size", "--add-host", "--dns", "--dns-search", "--gpus", "--health-cmd", "--health-interval",
    "--health-timeout", "--health-retries", "--health-start-period", "--log-driver", "--log-opt", "--security-opt",
    "--sysctl", "--tmpfs", "--pull", "--platform", "--ip", "--mac-address", "--memory-swap", "--ulimit",
    "--pid", "--ipc", "--runtime", "--stop-signal", "--stop-timeout", "--expose", "--label-file", "--cidfile",
    "--cpuset-cpus", "--cpu-shares", "--storage-opt", "--volumes-from", "--link", "--domainname", "--userns",
    "--uts", "--cgroupns", "--workdir",
];

/// Parses a `docker run ...` command into a spec.
pub fn parse_docker_run(command: &str) -> Result<ParsedRun> {
    // Join shell line continuations first.
    let joined = command.replace("\\\r\n", " ").replace("\\\n", " ");
    let words = shell_words::split(&joined).map_err(|e| AppError::bad_request(format!("could not parse command: {e}")))?;
    let mut iter = words.into_iter().peekable();
    // Skip a leading `sudo docker run` / `docker container run` / `podman run`.
    while let Some(w) = iter.peek() {
        if matches!(w.as_str(), "sudo" | "docker" | "podman" | "container" | "run" | "create") {
            iter.next();
        } else {
            break;
        }
    }

    let mut spec = ContainerSpec {
        restart: "no".into(),
        ..Default::default()
    };
    let mut warnings = Vec::new();
    let mut health = HealthSpec::default();
    let mut has_health = false;

    while let Some(word) = iter.next() {
        if !word.starts_with('-') || word == "-" {
            spec.image = word;
            spec.command = iter.by_ref().collect();
            break;
        }
        // Split "--flag=value".
        let (flag, inline) = match word.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (word.clone(), None),
        };
        // Expand combined short flags: -dit -> -d -i -t; "-p8080:80" -> -p 8080:80.
        if !flag.starts_with("--") && flag.len() > 2 {
            let short = &flag[..2];
            if VALUE_FLAGS.contains(&short) {
                apply_flag(short, flag[2..].to_string(), &mut spec, &mut health, &mut has_health, &mut warnings);
                continue;
            }
            for c in flag[1..].chars() {
                apply_bool_flag(&format!("-{c}"), &mut spec, &mut warnings);
            }
            continue;
        }
        if VALUE_FLAGS.contains(&flag.as_str()) {
            let value = match inline {
                Some(v) => v,
                None => match iter.next() {
                    Some(v) => v,
                    None => return Err(AppError::bad_request(format!("{flag} needs a value"))),
                },
            };
            apply_flag(&flag, value, &mut spec, &mut health, &mut has_health, &mut warnings);
        } else {
            apply_bool_flag(&flag, &mut spec, &mut warnings);
        }
    }
    if spec.image.is_empty() {
        return Err(AppError::bad_request("no image found in the command"));
    }
    if has_health {
        spec.healthcheck = Some(health);
    }
    Ok(ParsedRun { spec, warnings })
}

fn apply_bool_flag(flag: &str, spec: &mut ContainerSpec, warnings: &mut Vec<String>) {
    match flag {
        "-d" | "--detach" => {}
        "-i" | "--interactive" => spec.stdin_open = true,
        "-t" | "--tty" => spec.tty = true,
        "--privileged" => spec.privileged = true,
        "--init" => spec.init = true,
        "--rm" => warnings.push("--rm ignored: managed containers should persist".into()),
        "-P" | "--publish-all" => warnings.push("--publish-all is not supported; list ports explicitly".into()),
        "--read-only" => warnings.push("--read-only is not supported by the form".into()),
        other => warnings.push(format!("ignored unknown flag {other}")),
    }
}

fn apply_flag(
    flag: &str,
    value: String,
    spec: &mut ContainerSpec,
    health: &mut HealthSpec,
    has_health: &mut bool,
    warnings: &mut Vec<String>,
) {
    match flag {
        "--name" => spec.name = value,
        "-p" | "--publish" => match parse_port(&value) {
            Some(p) => spec.ports.push(p),
            None => warnings.push(format!("could not parse port '{value}'")),
        },
        "-v" | "--volume" => match parse_volume(&value) {
            Some(v) => spec.volumes.push(v),
            None => warnings.push(format!("could not parse volume '{value}'")),
        },
        "--mount" => {
            let kv: BTreeMap<String, String> = value
                .split(',')
                .map(|p| match p.split_once('=') {
                    Some((k, v)) => (k.trim().to_string(), v.trim().to_string()),
                    None => (p.trim().to_string(), "true".to_string()),
                })
                .collect();
            let target = kv.get("target").or(kv.get("dst")).or(kv.get("destination")).cloned().unwrap_or_default();
            spec.volumes.push(VolumeSpec {
                kind: kv.get("type").cloned().unwrap_or_else(|| "volume".into()),
                source: kv.get("source").or(kv.get("src")).cloned().unwrap_or_default(),
                target,
                read_only: kv.get("readonly").or(kv.get("ro")).is_some_and(|v| v == "true" || v == "1"),
            });
        }
        "-e" | "--env" => {
            let (k, v) = value.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())).unwrap_or((value, String::new()));
            spec.env.push(KeyValue { key: k, value: v });
        }
        "-l" | "--label" => {
            let (k, v) = value.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())).unwrap_or((value, String::new()));
            spec.labels.push(KeyValue { key: k, value: v });
        }
        "--restart" => spec.restart = value,
        "--network" | "--net" => {
            if spec.network_mode.is_empty() {
                spec.network_mode = value;
            } else {
                spec.networks.push(value);
            }
        }
        "--device" => {
            let parts: Vec<&str> = value.split(':').collect();
            spec.devices.push(DeviceSpec {
                host: parts[0].to_string(),
                container: parts.get(1).map(|s| s.to_string()).unwrap_or_default(),
                permissions: parts.get(2).map(|s| s.to_string()).unwrap_or_default(),
            });
        }
        "--group-add" => spec.group_add.push(value),
        "--cap-add" => spec.cap_add.push(value),
        "--cap-drop" => spec.cap_drop.push(value),
        "-u" | "--user" => spec.user = value,
        "-h" | "--hostname" => spec.hostname = value,
        "-w" | "--workdir" => spec.working_dir = value,
        "--entrypoint" => spec.entrypoint = shell_words::split(&value).unwrap_or_else(|_| vec![value]),
        "-m" | "--memory" => spec.memory = parse_bytes(&value),
        "--cpus" => spec.cpus = value.parse().ok(),
        "--shm-size" => spec.shm_size = parse_bytes(&value),
        "--add-host" => spec.extra_hosts.push(value),
        "--dns" => spec.dns.push(value),
        "--gpus" => spec.gpus = value.trim_start_matches("count=").to_string(),
        "--health-cmd" => {
            health.command = value;
            *has_health = true;
        }
        "--health-interval" => health.interval_secs = parse_duration_secs(&value).unwrap_or(0),
        "--health-timeout" => health.timeout_secs = parse_duration_secs(&value).unwrap_or(0),
        "--health-retries" => health.retries = value.parse().unwrap_or(0),
        "--health-start-period" => health.start_period_secs = parse_duration_secs(&value).unwrap_or(0),
        "--log-driver" => spec.log_driver = value,
        "--log-opt" => {
            let (k, v) = value.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())).unwrap_or((value, String::new()));
            spec.log_opts.push(KeyValue { key: k, value: v });
        }
        "--security-opt" => spec.security_opt.push(value),
        "--sysctl" => {
            let (k, v) = value.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())).unwrap_or((value, String::new()));
            spec.sysctls.push(KeyValue { key: k, value: v });
        }
        "--tmpfs" => spec.tmpfs.push(value),
        "--pull" | "--platform" | "--cidfile" => {}
        "--env-file" => warnings.push(format!("--env-file {value} ignored: add those variables to the form")),
        other => warnings.push(format!("ignored {other} {value}")),
    }
}

fn parse_port(s: &str) -> Option<PortSpec> {
    let (main, proto) = match s.rsplit_once('/') {
        Some((m, p)) => (m, p.to_string()),
        None => (s, "tcp".to_string()),
    };
    // IPv6 host IPs come bracketed: [::1]:8080:80
    let (host_ip, rest) = if let Some(stripped) = main.strip_prefix('[') {
        let (ip, rest) = stripped.split_once("]:")?;
        (ip.to_string(), rest)
    } else {
        let parts: Vec<&str> = main.split(':').collect();
        if parts.len() == 3 {
            (parts[0].to_string(), &main[parts[0].len() + 1..])
        } else {
            (String::new(), main)
        }
    };
    let (host_port, container_port) = match rest.split_once(':') {
        Some((h, c)) => (h.to_string(), c.to_string()),
        None => (String::new(), rest.to_string()),
    };
    if container_port.is_empty() {
        return None;
    }
    Some(PortSpec {
        host_ip,
        host_port,
        container_port,
        protocol: proto,
    })
}

fn parse_volume(s: &str) -> Option<VolumeSpec> {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        1 => Some(VolumeSpec {
            kind: "volume".into(),
            source: String::new(),
            target: parts[0].to_string(),
            read_only: false,
        }),
        2 | 3 => Some(VolumeSpec {
            kind: if parts[0].starts_with('/') || parts[0].starts_with('.') || parts[0].starts_with('~') {
                "bind".into()
            } else {
                "volume".into()
            },
            source: parts[0].to_string(),
            target: parts[1].to_string(),
            read_only: parts.get(2).is_some_and(|o| o.split(',').any(|x| x == "ro")),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_linuxserver_style_run() {
        let cmd = r#"docker run -d \
  --name=sonarr \
  -e PUID=1000 \
  -e PGID=1000 \
  -e TZ=Etc/UTC \
  -p 8989:8989 \
  -v /path/to/sonarr/data:/config \
  -v /path/to/tvseries:/tv:ro \
  --restart unless-stopped \
  lscr.io/linuxserver/sonarr:latest"#;
        let parsed = parse_docker_run(cmd).unwrap();
        let s = parsed.spec;
        assert_eq!(s.name, "sonarr");
        assert_eq!(s.image, "lscr.io/linuxserver/sonarr:latest");
        assert_eq!(s.env.len(), 3);
        assert_eq!(s.ports, vec![PortSpec { host_ip: "".into(), host_port: "8989".into(), container_port: "8989".into(), protocol: "tcp".into() }]);
        assert_eq!(s.volumes.len(), 2);
        assert!(s.volumes[1].read_only);
        assert_eq!(s.restart, "unless-stopped");
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    }

    #[test]
    fn parses_combined_flags_and_command() {
        let p = parse_docker_run("docker run -it --rm -p127.0.0.1:8080:80/udp --device /dev/dri:/dev/dri --gpus all alpine sh -c 'echo hi'").unwrap();
        assert!(p.spec.tty && p.spec.stdin_open);
        assert_eq!(p.spec.ports[0].host_ip, "127.0.0.1");
        assert_eq!(p.spec.ports[0].protocol, "udp");
        assert_eq!(p.spec.devices[0].host, "/dev/dri");
        assert_eq!(p.spec.gpus, "all");
        assert_eq!(p.spec.command, vec!["sh", "-c", "echo hi"]);
        assert_eq!(p.warnings.len(), 1); // --rm
    }

    #[test]
    fn spec_to_body_and_back() {
        let spec = ContainerSpec {
            name: "web".into(),
            image: "nginx:1.27".into(),
            restart: "unless-stopped".into(),
            ports: vec![PortSpec { host_port: "8080".into(), container_port: "80".into(), ..Default::default() }],
            volumes: vec![VolumeSpec { kind: "bind".into(), source: "/srv/www".into(), target: "/usr/share/nginx/html".into(), read_only: true }],
            env: vec![KeyValue { key: "A".into(), value: "b".into() }],
            memory: Some(512 * 1024 * 1024),
            ..Default::default()
        };
        let (body, extra) = spec.to_create_body().unwrap();
        assert!(extra.is_empty());
        let host = body.host_config.clone().unwrap();
        assert_eq!(host.binds.unwrap(), vec!["/srv/www:/usr/share/nginx/html:ro".to_string()]);
        let pb = host.port_bindings.unwrap();
        assert_eq!(pb["80/tcp"].as_ref().unwrap()[0].host_port.as_deref(), Some("8080"));
        assert_eq!(body.exposed_ports.unwrap(), vec!["80/tcp".to_string()]);
    }

    #[test]
    fn compose_export() {
        let spec = ContainerSpec {
            name: "plex".into(),
            image: "lscr.io/linuxserver/plex:latest".into(),
            restart: "unless-stopped".into(),
            network_mode: "host".into(),
            env: vec![KeyValue { key: "VERSION".into(), value: "docker".into() }, KeyValue { key: "PUID".into(), value: "1000".into() }],
            devices: vec![DeviceSpec { host: "/dev/dri".into(), container: "/dev/dri".into(), permissions: String::new() }],
            group_add: vec!["993".into()],
            ..Default::default()
        };
        let y = spec.to_compose();
        assert!(y.contains("network_mode: host"));
        assert!(y.contains("PUID: \"1000\""), "{y}");
        assert!(y.contains("- /dev/dri:/dev/dri"));
        assert!(y.contains("- \"993\""));
    }

    #[test]
    fn helpers() {
        assert_eq!(parse_bytes("512m"), Some(536_870_912));
        assert_eq!(parse_bytes("2g"), Some(2_147_483_648));
        assert_eq!(parse_duration_secs("1m30s"), Some(90));
        assert_eq!(parse_duration_secs("500ms"), Some(1));
        assert_eq!(expand_range("8000-8002").unwrap(), vec!["8000", "8001", "8002"]);
        assert!(parse_restart("sometimes").is_err());
    }
}
