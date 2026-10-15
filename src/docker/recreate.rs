//! Recreating standalone (non-compose) containers.
//!
//! Docker can't change a container's image in place, so updating a
//! container started with `docker run` means creating a new one with the
//! same settings. Two details matter for correctness:
//!
//! * Settings inherited from the *old image* (ENV, CMD, labels, ...) must not
//!   be copied, or they would override the new image's defaults. Only values
//!   that differ from the old image's are treated as user settings.
//! * Anonymous volumes are re-attached explicitly; otherwise the new
//!   container would start with empty ones.
//!
//! The old container is renamed and kept (stopped) until the new one is
//! verified, so a failed update can be reverted in seconds.

use std::collections::{HashMap, HashSet};

use bollard::Docker;
use bollard::models::{
    ContainerCreateBody, ContainerInspectResponse, EndpointSettings, HostConfig, ImageConfig, Mount,
    MountType, NetworkConnectRequest, NetworkingConfig,
};
use bollard::query_parameters::{CreateContainerOptions, InspectContainerOptions};

use crate::docker::actions;
use crate::error::{AppError, Result};

/// A recreation in progress; call [`finalize`] or [`revert`].
#[derive(Debug, Clone)]
pub struct Recreation {
    pub name: String,
    pub old_id: String,
    pub old_temp_name: String,
    pub new_id: String,
    pub was_running: bool,
}

fn is_anonymous_volume_name(name: &str) -> bool {
    name.len() == 64 && name.chars().all(|c| c.is_ascii_hexdigit())
}

/// Builds a create body that reproduces `inspect` with `image`, dropping
/// values inherited from `old_image`.
pub fn body_from_inspect(
    inspect: &ContainerInspectResponse,
    old_image: Option<&ImageConfig>,
    image: &str,
) -> ContainerCreateBody {
    let cfg = inspect.config.clone().unwrap_or_default();
    let img = old_image.cloned().unwrap_or_default();
    let id = inspect.id.clone().unwrap_or_default();

    let image_env: HashSet<&String> = img.env.iter().flatten().collect();
    let env: Vec<String> = cfg
        .env
        .iter()
        .flatten()
        .filter(|e| !image_env.contains(e))
        .cloned()
        .collect();

    let image_labels = img.labels.clone().unwrap_or_default();
    let labels: HashMap<String, String> = cfg
        .labels
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter(|(k, v)| image_labels.get(k) != Some(v))
        .collect();

    let image_ports: HashSet<&String> = img.exposed_ports.iter().flatten().collect();
    let exposed: Vec<String> = cfg
        .exposed_ports
        .iter()
        .flatten()
        .filter(|p| !image_ports.contains(p))
        .cloned()
        .collect();

    let same = |a: &Option<Vec<String>>, b: &Option<Vec<String>>| {
        a.as_deref().unwrap_or_default() == b.as_deref().unwrap_or_default()
    };
    let cmd = if same(&cfg.cmd, &img.cmd) { None } else { cfg.cmd.clone() };
    let entrypoint = if same(&cfg.entrypoint, &img.entrypoint) {
        None
    } else {
        cfg.entrypoint.clone()
    };
    let user = cfg.user.clone().filter(|u| !u.is_empty() && Some(u) != img.user.as_ref());
    let working_dir = cfg
        .working_dir
        .clone()
        .filter(|w| !w.is_empty() && Some(w) != img.working_dir.as_ref());
    let healthcheck = if cfg.healthcheck == img.healthcheck { None } else { cfg.healthcheck.clone() };
    let stop_signal = cfg.stop_signal.clone().filter(|s| Some(s) != img.stop_signal.as_ref());
    // Docker defaults the hostname to the short container id; don't pin it.
    let hostname = cfg.hostname.clone().filter(|h| !h.is_empty() && !id.starts_with(h.as_str()));

    let mut host = inspect.host_config.clone().unwrap_or_default();
    reattach_anonymous_volumes(inspect, &mut host);

    let networking_config = endpoints_for_create(inspect, &host).map(|(name, ep)| NetworkingConfig {
        endpoints_config: Some(HashMap::from([(name, ep)])),
    });

    ContainerCreateBody {
        hostname,
        domainname: cfg.domainname.clone().filter(|d| !d.is_empty()),
        user,
        attach_stdin: cfg.attach_stdin,
        attach_stdout: cfg.attach_stdout,
        attach_stderr: cfg.attach_stderr,
        exposed_ports: if exposed.is_empty() { None } else { Some(exposed) },
        tty: cfg.tty,
        open_stdin: cfg.open_stdin,
        stdin_once: cfg.stdin_once,
        env: if env.is_empty() { None } else { Some(env) },
        cmd,
        healthcheck,
        args_escaped: None,
        image: Some(image.to_string()),
        volumes: None,
        working_dir,
        entrypoint,
        network_disabled: cfg.network_disabled,
        on_build: None,
        labels: if labels.is_empty() { None } else { Some(labels) },
        stop_signal,
        stop_timeout: cfg.stop_timeout,
        shell: None,
        host_config: Some(host),
        networking_config,
    }
}

/// Adds explicit mounts for anonymous volumes so their data carries over.
fn reattach_anonymous_volumes(inspect: &ContainerInspectResponse, host: &mut HostConfig) {
    let explicit: HashSet<String> = host
        .mounts
        .iter()
        .flatten()
        .filter_map(|m| m.target.clone())
        .chain(host.binds.iter().flatten().filter_map(|b| b.split(':').nth(1).map(str::to_string)))
        .collect();
    let mut extra = Vec::new();
    for m in inspect.mounts.iter().flatten() {
        let (Some(name), Some(dest)) = (&m.name, &m.destination) else { continue };
        let is_volume = m.typ.as_deref() == Some("volume");
        if is_volume && is_anonymous_volume_name(name) && !explicit.contains(dest) {
            extra.push(Mount {
                target: Some(dest.clone()),
                source: Some(name.clone()),
                typ: Some(MountType::VOLUME),
                read_only: m.rw.map(|rw| !rw),
                ..Default::default()
            });
        }
    }
    if !extra.is_empty() {
        host.mounts.get_or_insert_with(Vec::new).extend(extra);
    }
}

/// The endpoint config for the container's primary network, preserving
/// user-set aliases, static IPs and MAC address.
fn endpoints_for_create(inspect: &ContainerInspectResponse, host: &HostConfig) -> Option<(String, EndpointSettings)> {
    let mode = host.network_mode.clone().unwrap_or_default();
    if mode == "host" || mode == "none" || mode.starts_with("container:") {
        return None;
    }
    let networks = inspect.network_settings.as_ref()?.networks.as_ref()?;
    let primary = if networks.contains_key(&mode) {
        mode.clone()
    } else if mode == "default" || mode.is_empty() {
        networks.keys().find(|k| *k == "bridge").or_else(|| networks.keys().next())?.clone()
    } else {
        networks.keys().next()?.clone()
    };
    let ep = networks.get(&primary)?;
    Some((primary, endpoint_config(inspect, ep)))
}

fn endpoint_config(inspect: &ContainerInspectResponse, ep: &EndpointSettings) -> EndpointSettings {
    let id = inspect.id.clone().unwrap_or_default();
    let name = inspect.name.clone().unwrap_or_default().trim_start_matches('/').to_string();
    let aliases: Vec<String> = ep
        .aliases
        .iter()
        .flatten()
        .filter(|a| !id.starts_with(a.as_str()) && **a != name)
        .cloned()
        .collect();
    EndpointSettings {
        ipam_config: ep.ipam_config.clone(),
        links: ep.links.clone(),
        mac_address: ep.mac_address.clone().filter(|m| !m.is_empty()),
        aliases: if aliases.is_empty() { None } else { Some(aliases) },
        driver_opts: ep.driver_opts.clone(),
        gw_priority: ep.gw_priority,
        ..Default::default()
    }
}

/// Networks other than the primary one, to connect after creation.
fn secondary_networks(inspect: &ContainerInspectResponse, primary: Option<&str>) -> Vec<(String, EndpointSettings)> {
    let Some(networks) = inspect.network_settings.as_ref().and_then(|n| n.networks.as_ref()) else {
        return Vec::new();
    };
    networks
        .iter()
        .filter(|(name, _)| Some(name.as_str()) != primary)
        .map(|(name, ep)| (name.clone(), endpoint_config(inspect, ep)))
        .collect()
}

/// Replaces a container with a new one built from `body`.
///
/// On success the new container is running (if the old one was) and the
/// old one is stopped and renamed; on any failure the original is restored.
pub async fn replace(
    api: &Docker,
    container: &str,
    body: ContainerCreateBody,
    stop_timeout: Option<u32>,
    log: &(dyn Fn(String) + Send + Sync),
) -> Result<Recreation> {
    let inspect = api.inspect_container(container, None::<InspectContainerOptions>).await?;
    if inspect
        .host_config
        .as_ref()
        .and_then(|h| h.auto_remove)
        .unwrap_or(false)
    {
        return Err(AppError::conflict(
            "this container was started with --rm and cannot be recreated",
        ));
    }
    let old_id = inspect.id.clone().unwrap_or_default();
    let name = inspect.name.clone().unwrap_or_default().trim_start_matches('/').to_string();
    let was_running = inspect.state.as_ref().and_then(|s| s.running).unwrap_or(false);
    let primary = body
        .networking_config
        .as_ref()
        .and_then(|n| n.endpoints_config.as_ref())
        .and_then(|m| m.keys().next().cloned());
    let secondary = secondary_networks(&inspect, primary.as_deref());
    let temp_name = format!("{name}-nodestarr-old-{}", &old_id[..8.min(old_id.len())]);

    if was_running {
        log(format!("Stopping {name}"));
        actions::stop(api, &old_id, stop_timeout).await?;
    }
    actions::rename(api, &old_id, &temp_name).await?;

    let created = async {
        log(format!("Creating new container {name}"));
        let resp = api
            .create_container(
                Some(CreateContainerOptions {
                    name: Some(name.clone()),
                    platform: String::new(),
                }),
                body,
            )
            .await?;
        for w in &resp.warnings {
            log(format!("Docker warning: {w}"));
        }
        let new_id = resp.id;
        for (net, ep) in &secondary {
            log(format!("Connecting to network {net}"));
            api.connect_network(
                net,
                NetworkConnectRequest {
                    container: new_id.clone(),
                    endpoint_config: Some(ep.clone()),
                },
            )
            .await?;
        }
        if was_running {
            log(format!("Starting {name}"));
            actions::start(api, &new_id).await?;
        }
        Ok::<String, AppError>(new_id)
    }
    .await;

    match created {
        Ok(new_id) => Ok(Recreation {
            name,
            old_id,
            old_temp_name: temp_name,
            new_id,
            was_running,
        }),
        Err(err) => {
            log(format!("Creating the new container failed: {err}; restoring the original"));
            // Remove a half-created container holding the name, if any.
            if let Ok(i) = api.inspect_container(&name, None::<InspectContainerOptions>).await {
                if i.id.as_deref() != Some(old_id.as_str()) {
                    let _ = actions::remove(api, &name, true, false).await;
                }
            }
            let _ = actions::rename(api, &old_id, &name).await;
            if was_running {
                let _ = actions::start(api, &old_id).await;
            }
            Err(err)
        }
    }
}

/// Removes the old container after the new one was verified.
pub async fn finalize(api: &Docker, r: &Recreation) -> Result<()> {
    actions::remove(api, &r.old_id, true, false).await
}

/// Throws away the new container and brings the original back.
pub async fn revert(api: &Docker, r: &Recreation, log: &(dyn Fn(String) + Send + Sync)) -> Result<()> {
    log(format!("Reverting {} to the previous container", r.name));
    let _ = actions::stop(api, &r.new_id, Some(10)).await;
    actions::remove(api, &r.new_id, true, false).await?;
    actions::rename(api, &r.old_id, &r.name).await?;
    if r.was_running {
        actions::start(api, &r.old_id).await?;
    }
    Ok(())
}

/// Finds containers whose network namespace is joined to `id`
/// (`--network container:<id>`, compose `network_mode: service:x`). They
/// lose networking when `id` is recreated and must be recreated too.
pub fn dependents_by_network(entries: &[std::sync::Arc<crate::docker::Entry>], id: &str, name: &str) -> Vec<String> {
    entries
        .iter()
        .filter(|e| {
            let mode = &e.summary.network_mode;
            mode == &format!("container:{id}") || mode == &format!("container:{name}")
        })
        .map(|e| e.summary.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::{ContainerConfig, MountPoint, NetworkSettings};

    fn inspect_fixture() -> ContainerInspectResponse {
        let mut networks = HashMap::new();
        networks.insert(
            "media_default".to_string(),
            EndpointSettings {
                aliases: Some(vec!["abc123def456".into(), "sonarr".into(), "tv".into()]),
                ip_address: Some("172.20.0.5".into()),
                network_id: Some("netid".into()),
                ..Default::default()
            },
        );
        ContainerInspectResponse {
            id: Some(format!("abc123def456{}", "0".repeat(52))),
            name: Some("/sonarr".into()),
            config: Some(ContainerConfig {
                hostname: Some("abc123def456".into()),
                env: Some(vec!["PATH=/usr/bin".into(), "PUID=1000".into(), "TZ=Europe/Berlin".into()]),
                cmd: Some(vec!["/init".into()]),
                labels: Some(HashMap::from([
                    ("build_version".to_string(), "old".to_string()),
                    ("mine".to_string(), "yes".to_string()),
                ])),
                exposed_ports: Some(vec!["8989/tcp".into(), "9999/tcp".into()]),
                image: Some("lscr.io/linuxserver/sonarr:latest".into()),
                ..Default::default()
            }),
            host_config: Some(HostConfig {
                network_mode: Some("media_default".into()),
                binds: Some(vec!["/opt/stacks/media/config/sonarr:/config".into()]),
                ..Default::default()
            }),
            mounts: Some(vec![
                MountPoint {
                    typ: Some("bind".into()),
                    source: Some("/opt/stacks/media/config/sonarr".into()),
                    destination: Some("/config".into()),
                    ..Default::default()
                },
                MountPoint {
                    typ: Some("volume".into()),
                    name: Some("f".repeat(64)),
                    destination: Some("/cache".into()),
                    rw: Some(true),
                    ..Default::default()
                },
            ]),
            network_settings: Some(NetworkSettings {
                networks: Some(networks),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn drops_image_defaults_and_keeps_user_settings() {
        let inspect = inspect_fixture();
        let image = ImageConfig {
            env: Some(vec!["PATH=/usr/bin".into()]),
            cmd: Some(vec!["/init".into()]),
            labels: Some(HashMap::from([("build_version".to_string(), "old".to_string())])),
            exposed_ports: Some(vec!["8989/tcp".into()]),
            ..Default::default()
        };
        let body = body_from_inspect(&inspect, Some(&image), "lscr.io/linuxserver/sonarr:develop");
        assert_eq!(body.image.as_deref(), Some("lscr.io/linuxserver/sonarr:develop"));
        assert_eq!(body.env.unwrap(), vec!["PUID=1000".to_string(), "TZ=Europe/Berlin".to_string()]);
        assert!(body.cmd.is_none(), "image default CMD must not be pinned");
        assert!(body.hostname.is_none(), "id-derived hostname must not be pinned");
        let labels = body.labels.unwrap();
        assert!(labels.contains_key("mine") && !labels.contains_key("build_version"));
        assert_eq!(body.exposed_ports.unwrap(), vec!["9999/tcp".to_string()]);

        let host = body.host_config.unwrap();
        let mounts = host.mounts.unwrap();
        assert_eq!(mounts.len(), 1, "anonymous volume re-attached");
        assert_eq!(mounts[0].target.as_deref(), Some("/cache"));

        let eps = body.networking_config.unwrap().endpoints_config.unwrap();
        let ep = eps.get("media_default").unwrap();
        assert_eq!(ep.aliases.as_ref().unwrap(), &vec!["tv".to_string()]);
        assert!(ep.ip_address.is_none(), "runtime fields are not copied");
    }
}
