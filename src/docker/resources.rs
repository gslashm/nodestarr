//! Images, volumes and networks: listings with usage, removal, creation and
//! cleanup ("prune").
//!
//! Cleanup is deliberately conservative: rollback images that Nodestarr keeps for
//! undoing updates survive an "unused images" cleanup unless asked otherwise,
//! and only anonymous volumes are pruned unless named ones are requested too.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use bollard::models::{Ipam, IpamConfig, NetworkCreateRequest, VolumeCreateRequest};
use bollard::query_parameters::{
    ListImagesOptionsBuilder, ListNetworksOptions, ListVolumesOptions, PruneBuildOptionsBuilder,
    PruneContainersOptions, PruneImagesOptionsBuilder, PruneNetworksOptions, PruneVolumesOptionsBuilder,
    RemoveImageOptionsBuilder, RemoveVolumeOptionsBuilder,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::App;
use crate::docker::images::ImageRow;
use crate::docker::model::LABEL_PROJECT;
use crate::error::{AppError, Result};
use crate::updates::apply::ROLLBACK_REPO_PREFIX;

/// Computing disk usage walks every volume, so give Docker time.
const DF_TIMEOUT: Duration = Duration::from_secs(180);

/// Networks Docker creates itself; they can't be removed.
const BUILTIN_NETWORKS: &[&str] = &["bridge", "host", "none"];

fn container_names_by<F>(app: &App, mut keys: F) -> HashMap<String, Vec<String>>
where
    F: FnMut(&crate::docker::model::ContainerSummary) -> Vec<String>,
{
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for c in app.docker.list() {
        for k in keys(&c) {
            map.entry(k).or_default().push(c.name.clone());
        }
    }
    map
}

// ---------------------------------------------------------------- images

pub async fn images(app: &App) -> Result<Vec<ImageRow>> {
    let list = app.docker.api.list_images(Some(ListImagesOptionsBuilder::new().all(false).build())).await?;
    let mut users = container_names_by(app, |c| vec![c.image_id.clone()]);
    let mut rows: Vec<ImageRow> = list
        .into_iter()
        .map(|i| {
            let tags: Vec<String> = i.repo_tags.into_iter().filter(|t| t != "<none>:<none>").collect();
            let rollback = !tags.is_empty() && tags.iter().all(|t| t.starts_with(ROLLBACK_REPO_PREFIX));
            ImageRow {
                containers: users.remove(&i.id).unwrap_or_default(),
                dangling: tags.is_empty(),
                rollback,
                tags,
                digests: i.repo_digests.into_iter().filter(|d| d != "<none>@<none>").collect(),
                size: i.size,
                created: i.created,
                id: i.id,
            }
        })
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.created));
    Ok(rows)
}

pub async fn remove_image(app: &App, id: &str, force: bool) -> Result<Vec<String>> {
    let resp =
        app.docker.api.remove_image(id, Some(RemoveImageOptionsBuilder::new().force(force).build()), None).await?;
    app.docker.invalidate_images();
    Ok(resp.into_iter().filter_map(|r| r.deleted.or(r.untagged)).collect())
}

// ---------------------------------------------------------------- volumes

#[derive(Debug, Clone, Serialize)]
pub struct VolumeRow {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    pub created: Option<i64>,
    /// Compose project that created the volume.
    pub stack: Option<String>,
    /// Created implicitly for a container (a `VOLUME` in the image).
    pub anonymous: bool,
    /// Containers (running or not) that mount it.
    pub containers: Vec<String>,
    /// Bytes on disk, when Docker reports it (`sizes=true`).
    pub size: Option<i64>,
    pub labels: BTreeMap<String, String>,
}

fn looks_anonymous(name: &str, labels: &HashMap<String, String>) -> bool {
    labels.contains_key("com.docker.volume.anonymous")
        || (name.len() == 64 && name.chars().all(|c| c.is_ascii_hexdigit()))
}

pub async fn volumes(app: &App, with_sizes: bool) -> Result<Vec<VolumeRow>> {
    let resp = app.docker.api.list_volumes(None::<ListVolumesOptions>).await?;
    let mut users = container_names_by(app, |c| {
        c.mounts.iter().filter(|m| m.kind == "volume").filter_map(|m| m.name.clone()).collect()
    });
    let sizes = if with_sizes { disk_usage(app).await?.volume_sizes } else { HashMap::new() };
    let mut rows: Vec<VolumeRow> = resp
        .volumes
        .unwrap_or_default()
        .into_iter()
        .map(|v| VolumeRow {
            anonymous: looks_anonymous(&v.name, &v.labels),
            stack: v.labels.get(LABEL_PROJECT).cloned(),
            containers: users.remove(&v.name).unwrap_or_default(),
            size: sizes.get(&v.name).copied().filter(|s| *s >= 0),
            created: v.created_at.as_ref().and_then(|d| crate::util::parse_rfc3339(&d.to_string())),
            labels: v.labels.into_iter().collect(),
            driver: v.driver,
            mountpoint: v.mountpoint,
            name: v.name,
        })
        .collect();
    rows.sort_by(|a, b| a.anonymous.cmp(&b.anonymous).then_with(|| a.name.cmp(&b.name)));
    Ok(rows)
}

pub async fn remove_volume(app: &App, name: &str) -> Result<()> {
    app.docker.api.remove_volume(name, Some(RemoveVolumeOptionsBuilder::new().force(false).build())).await?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewVolume {
    pub name: String,
    #[serde(default)]
    pub driver: Option<String>,
    #[serde(default)]
    pub driver_opts: HashMap<String, String>,
}

pub async fn create_volume(app: &App, v: NewVolume) -> Result<String> {
    validate_name(&v.name, "volume")?;
    let created = app
        .docker
        .api
        .create_volume(VolumeCreateRequest {
            name: Some(v.name.clone()),
            driver: v.driver.filter(|d| !d.is_empty()),
            driver_opts: (!v.driver_opts.is_empty()).then_some(v.driver_opts),
            labels: Some(HashMap::from([("nodestarr.created".to_string(), "true".to_string())])),
            ..Default::default()
        })
        .await?;
    Ok(created.name)
}

// ---------------------------------------------------------------- networks

#[derive(Debug, Clone, Serialize)]
pub struct NetworkRow {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub internal: bool,
    pub ipv6: bool,
    pub subnets: Vec<String>,
    pub gateways: Vec<String>,
    pub stack: Option<String>,
    /// Containers attached (from Nodestarr's inventory, by network name).
    pub containers: Vec<String>,
    pub builtin: bool,
    pub created: Option<i64>,
}

pub async fn networks(app: &App) -> Result<Vec<NetworkRow>> {
    let list = app.docker.api.list_networks(None::<ListNetworksOptions>).await?;
    let mut users = container_names_by(app, |c| c.networks.clone());
    let mut rows: Vec<NetworkRow> = list
        .into_iter()
        .map(|n| {
            let name = n.name.unwrap_or_default();
            let configs: Vec<IpamConfig> = n.ipam.and_then(|i| i.config).unwrap_or_default();
            let labels = n.labels.unwrap_or_default();
            NetworkRow {
                id: n.id.unwrap_or_default(),
                containers: users.remove(&name).unwrap_or_default(),
                builtin: BUILTIN_NETWORKS.contains(&name.as_str()),
                driver: n.driver.unwrap_or_default(),
                scope: n.scope.unwrap_or_default(),
                internal: n.internal.unwrap_or(false),
                ipv6: n.enable_ipv6.unwrap_or(false),
                subnets: configs.iter().filter_map(|c| c.subnet.clone()).collect(),
                gateways: configs.iter().filter_map(|c| c.gateway.clone()).collect(),
                stack: labels.get(LABEL_PROJECT).cloned(),
                created: n.created.as_ref().and_then(|d| crate::util::parse_rfc3339(&d.to_string())),
                name,
            }
        })
        .collect();
    rows.sort_by(|a, b| b.builtin.cmp(&a.builtin).then_with(|| a.name.cmp(&b.name)));
    Ok(rows)
}

pub async fn remove_network(app: &App, id: &str) -> Result<()> {
    if BUILTIN_NETWORKS.contains(&id) {
        return Err(AppError::bad_request("Docker's built-in networks can't be removed"));
    }
    app.docker.api.remove_network(id).await?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewNetwork {
    pub name: String,
    /// "bridge" (default) or "macvlan"/"ipvlan" (needs a parent interface option).
    #[serde(default)]
    pub driver: Option<String>,
    #[serde(default)]
    pub internal: bool,
    #[serde(default)]
    pub subnet: Option<String>,
    #[serde(default)]
    pub gateway: Option<String>,
    #[serde(default)]
    pub options: HashMap<String, String>,
}

pub async fn create_network(app: &App, n: NewNetwork) -> Result<String> {
    validate_name(&n.name, "network")?;
    if let Some(s) = n.subnet.as_deref().filter(|s| !s.is_empty()) {
        s.parse::<ipnet::IpNet>()
            .map_err(|_| AppError::bad_request(format!("'{s}' is not a subnet like 172.30.0.0/24")))?;
    }
    let subnet = n.subnet.filter(|s| !s.is_empty());
    let gateway = n.gateway.filter(|s| !s.is_empty());
    let ipam = (subnet.is_some() || gateway.is_some()).then(|| Ipam {
        config: Some(vec![IpamConfig { subnet, gateway, ..Default::default() }]),
        ..Default::default()
    });
    let resp = app
        .docker
        .api
        .create_network(NetworkCreateRequest {
            name: n.name,
            driver: Some(n.driver.filter(|d| !d.is_empty()).unwrap_or_else(|| "bridge".into())),
            internal: Some(n.internal),
            attachable: Some(true),
            ipam,
            options: (!n.options.is_empty()).then_some(n.options),
            labels: Some(HashMap::from([("nodestarr.created".to_string(), "true".to_string())])),
            ..Default::default()
        })
        .await?;
    Ok(resp.id)
}

fn validate_name(name: &str, what: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 128
        && name.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if ok {
        Ok(())
    } else {
        Err(AppError::bad_request(format!(
            "{what} names use letters, digits, '.', '_' and '-', and start with a letter or digit"
        )))
    }
}

// ---------------------------------------------------------------- disk usage

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct UsageGroup {
    pub count: i64,
    pub active: i64,
    /// Bytes on disk.
    pub size: i64,
    /// Bytes a cleanup could free.
    pub reclaimable: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DiskUsage {
    pub images: UsageGroup,
    pub containers: UsageGroup,
    pub volumes: UsageGroup,
    pub build_cache: UsageGroup,
    /// Volume name to size (-1 when Docker couldn't measure it).
    #[serde(skip)]
    pub volume_sizes: HashMap<String, i64>,
}

pub async fn disk_usage(app: &App) -> Result<DiskUsage> {
    let v = super::raw::get_json(&app.boot.docker_host, "/system/df?verbose=1", DF_TIMEOUT).await?;
    Ok(parse_df(&v))
}

fn i(v: &Value) -> i64 {
    v.as_i64().unwrap_or(0)
}

fn items(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}

/// Understands both the API >= 1.52 shape (`ImageUsage`, ...) and the older
/// one (`LayersSize`, `Images`, `Containers`, `Volumes`, `BuildCache`).
fn parse_df(v: &Value) -> DiskUsage {
    let mut out = DiskUsage::default();
    let volume_items: &[Value];
    if v.get("ImageUsage").is_some() || v.get("VolumeUsage").is_some() {
        let group = |key: &str| {
            let g = &v[key];
            UsageGroup {
                count: i(&g["TotalCount"]),
                active: i(&g["ActiveCount"]),
                size: i(&g["TotalSize"]),
                reclaimable: i(&g["Reclaimable"]),
            }
        };
        out.images = group("ImageUsage");
        out.containers = group("ContainerUsage");
        out.volumes = group("VolumeUsage");
        out.build_cache = group("BuildCacheUsage");
        volume_items = items(&v["VolumeUsage"]["Items"]);
    } else {
        let images = items(&v["Images"]);
        out.images = UsageGroup {
            count: images.len() as i64,
            active: images.iter().filter(|x| i(&x["Containers"]) > 0).count() as i64,
            size: i(&v["LayersSize"]),
            reclaimable: images
                .iter()
                .filter(|x| i(&x["Containers"]) <= 0)
                .map(|x| (i(&x["Size"]) - i(&x["SharedSize"]).max(0)).max(0))
                .sum(),
        };
        let containers = items(&v["Containers"]);
        let running = |x: &Value| x["State"].as_str() == Some("running");
        out.containers = UsageGroup {
            count: containers.len() as i64,
            active: containers.iter().filter(|x| running(x)).count() as i64,
            size: containers.iter().map(|x| i(&x["SizeRw"]).max(0)).sum(),
            reclaimable: containers.iter().filter(|x| !running(x)).map(|x| i(&x["SizeRw"]).max(0)).sum(),
        };
        let cache = items(&v["BuildCache"]);
        out.build_cache = UsageGroup {
            count: cache.len() as i64,
            active: cache.iter().filter(|x| x["InUse"].as_bool() == Some(true)).count() as i64,
            size: cache.iter().map(|x| i(&x["Size"]).max(0)).sum(),
            reclaimable: cache
                .iter()
                .filter(|x| x["InUse"].as_bool() != Some(true) && x["Shared"].as_bool() != Some(true))
                .map(|x| i(&x["Size"]).max(0))
                .sum(),
        };
        volume_items = items(&v["Volumes"]);
        let size = |x: &Value| i(&x["UsageData"]["Size"]).max(0);
        let refs = |x: &Value| i(&x["UsageData"]["RefCount"]);
        out.volumes = UsageGroup {
            count: volume_items.len() as i64,
            active: volume_items.iter().filter(|x| refs(x) > 0).count() as i64,
            size: volume_items.iter().map(size).sum(),
            reclaimable: volume_items.iter().filter(|x| refs(x) == 0).map(size).sum(),
        };
    }
    out.volume_sizes = volume_items
        .iter()
        .filter_map(|x| Some((x["Name"].as_str()?.to_string(), x["UsageData"]["Size"].as_i64().unwrap_or(-1))))
        .collect();
    out
}

// ---------------------------------------------------------------- cleanup

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PruneRequest {
    /// Untagged leftovers from updates and builds.
    pub dangling_images: bool,
    /// Every image no container uses (rollback images are kept unless
    /// `include_rollback` is set).
    pub unused_images: bool,
    pub include_rollback: bool,
    /// Stopped containers.
    pub containers: bool,
    /// Volumes no container uses: anonymous ones only, unless `named_volumes`.
    pub volumes: bool,
    pub named_volumes: bool,
    /// Custom networks no container uses.
    pub networks: bool,
    pub build_cache: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PruneReport {
    pub images_removed: usize,
    pub containers_removed: usize,
    pub volumes_removed: usize,
    pub networks_removed: usize,
    pub build_cache_removed: usize,
    pub reclaimed: i64,
    pub errors: Vec<String>,
}

pub async fn prune(app: &App, req: &PruneRequest, log: &(dyn Fn(String) + Send + Sync)) -> Result<PruneReport> {
    let api = &app.docker.api;
    let mut r = PruneReport::default();

    if req.containers {
        log("Removing stopped containers".into());
        // Nodestarr's own short-lived helpers and leftovers are fine to remove;
        // everything else stopped goes too, which is what was asked for.
        match api.prune_containers(None::<PruneContainersOptions>).await {
            Ok(p) => {
                r.containers_removed = p.containers_deleted.map(|v| v.len()).unwrap_or(0);
                r.reclaimed += p.space_reclaimed.unwrap_or(0);
            }
            Err(e) => r.errors.push(format!("containers: {e}")),
        }
    }

    if req.unused_images {
        log("Removing images no container uses".into());
        let rows = images(app).await?;
        let mut removed_any = false;
        for img in rows.iter().filter(|i| i.containers.is_empty() && (req.include_rollback || !i.rollback)) {
            match api.remove_image(&img.id, Some(RemoveImageOptionsBuilder::new().force(false).build()), None).await {
                Ok(_) => {
                    r.images_removed += 1;
                    r.reclaimed += img.size;
                    removed_any = true;
                    log(format!("  removed {}", img.tags.first().cloned().unwrap_or_else(|| short(&img.id))));
                }
                // Images shared by tag with a used image, or in use by a
                // container Docker knows and Nodestarr hides, are skipped.
                Err(e) => tracing::debug!(image = %img.id, %e, "image kept"),
            }
        }
        if removed_any {
            app.docker.invalidate_images();
        }
    } else if req.dangling_images {
        log("Removing dangling images".into());
        let filters = HashMap::from([("dangling", vec!["true"])]);
        match api.prune_images(Some(PruneImagesOptionsBuilder::new().filters(&filters).build())).await {
            Ok(p) => {
                r.images_removed =
                    p.images_deleted.map(|v| v.iter().filter(|d| d.deleted.is_some()).count()).unwrap_or(0);
                r.reclaimed += p.space_reclaimed.unwrap_or(0);
            }
            Err(e) => r.errors.push(format!("images: {e}")),
        }
    }

    if req.volumes {
        log(if req.named_volumes {
            "Removing unused volumes (including named ones)"
        } else {
            "Removing unused anonymous volumes"
        }
        .into());
        let mut builder = PruneVolumesOptionsBuilder::new();
        if req.named_volumes {
            builder = builder.filters(&HashMap::from([("all", vec!["true"])]));
        }
        match api.prune_volumes(Some(builder.build())).await {
            Ok(p) => {
                r.volumes_removed = p.volumes_deleted.map(|v| v.len()).unwrap_or(0);
                r.reclaimed += p.space_reclaimed.unwrap_or(0);
            }
            Err(e) => r.errors.push(format!("volumes: {e}")),
        }
    }

    if req.networks {
        log("Removing unused networks".into());
        match api.prune_networks(None::<PruneNetworksOptions>).await {
            Ok(p) => r.networks_removed = p.networks_deleted.map(|v| v.len()).unwrap_or(0),
            Err(e) => r.errors.push(format!("networks: {e}")),
        }
    }

    if req.build_cache {
        log("Removing build cache".into());
        match api.prune_build(Some(PruneBuildOptionsBuilder::new().all(true).build())).await {
            Ok(p) => {
                r.build_cache_removed = p.caches_deleted.map(|v| v.len()).unwrap_or(0);
                r.reclaimed += p.space_reclaimed.unwrap_or(0);
            }
            Err(e) => r.errors.push(format!("build cache: {e}")),
        }
    }
    Ok(r)
}

fn short(id: &str) -> String {
    id.trim_start_matches("sha256:").chars().take(12).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_new_df_shape() {
        let v = json!({
            "ImageUsage": { "ActiveCount": 1, "TotalCount": 3, "TotalSize": 900, "Reclaimable": 400 },
            "ContainerUsage": { "ActiveCount": 1, "TotalCount": 1, "TotalSize": 12 },
            "VolumeUsage": { "ActiveCount": 1, "TotalCount": 2, "TotalSize": 50, "Items": [
                { "Name": "db", "UsageData": { "Size": 50, "RefCount": 1 } },
                { "Name": "old", "UsageData": { "Size": -1, "RefCount": 0 } }
            ]},
            "BuildCacheUsage": { "TotalCount": 0 }
        });
        let d = parse_df(&v);
        assert_eq!(d.images, UsageGroup { count: 3, active: 1, size: 900, reclaimable: 400 });
        assert_eq!(d.containers.size, 12);
        assert_eq!(d.volume_sizes.get("db"), Some(&50));
        assert_eq!(d.volume_sizes.get("old"), Some(&-1));
    }

    #[test]
    fn parses_legacy_df_shape() {
        let v = json!({
            "LayersSize": 1000,
            "Images": [
                { "Containers": 1, "Size": 600, "SharedSize": 100 },
                { "Containers": 0, "Size": 500, "SharedSize": 100 }
            ],
            "Containers": [
                { "State": "running", "SizeRw": 10 },
                { "State": "exited", "SizeRw": 5 }
            ],
            "Volumes": [
                { "Name": "data", "UsageData": { "Size": 70, "RefCount": 1 } },
                { "Name": "stale", "UsageData": { "Size": 30, "RefCount": 0 } }
            ],
            "BuildCache": [ { "Size": 8, "InUse": false, "Shared": false } ]
        });
        let d = parse_df(&v);
        assert_eq!(d.images, UsageGroup { count: 2, active: 1, size: 1000, reclaimable: 400 });
        assert_eq!(d.containers, UsageGroup { count: 2, active: 1, size: 15, reclaimable: 5 });
        assert_eq!(d.volumes, UsageGroup { count: 2, active: 1, size: 100, reclaimable: 30 });
        assert_eq!(d.build_cache.reclaimable, 8);
        assert_eq!(d.volume_sizes.len(), 2);
    }

    #[test]
    fn anonymous_volumes() {
        let none = HashMap::new();
        assert!(looks_anonymous(&"a".repeat(64), &none));
        assert!(!looks_anonymous("media_config", &none));
        let labeled = HashMap::from([("com.docker.volume.anonymous".to_string(), String::new())]);
        assert!(looks_anonymous("x", &labeled));
    }

    #[test]
    fn names() {
        assert!(validate_name("media_net", "network").is_ok());
        assert!(validate_name("-bad", "network").is_err());
        assert!(validate_name("has space", "volume").is_err());
    }
}
