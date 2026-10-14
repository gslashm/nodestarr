//! Live view of every container, kept current from Docker's event stream.
//!
//! Instead of polling, Berth holds one long-lived `/events` connection and
//! re-inspects only the container that changed. A periodic full resync
//! covers anything missed while the stream was down.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bollard::Docker;
use bollard::models::{ContainerInspectResponse, EventMessage, EventMessageTypeEnum};
use bollard::query_parameters::{EventsOptions, InspectContainerOptions, ListContainersOptions};
use futures_util::{StreamExt, stream};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::docker::images::ImageMeta;
use crate::docker::model::{self, ContainerSummary, SummaryInput};
use crate::events::{Event, EventBus};
use crate::settings::SettingsStore;

/// Placeholder the UI replaces with the host the browser used, when no
/// public host is configured.
pub const BROWSER_HOST: &str = "{host}";

/// A container as Berth knows it.
#[derive(Debug)]
pub struct Entry {
    pub summary: Arc<ContainerSummary>,
    pub inspect: Arc<ContainerInspectResponse>,
}

/// Lifecycle notifications for the alert engine (crashes, OOM, restarts).
#[derive(Debug, Clone)]
pub struct Lifecycle {
    pub id: String,
    pub identity: String,
    pub name: String,
    /// Docker action: "die", "kill", "oom", "start", "restart", "health_status: unhealthy", ...
    pub action: String,
    pub exit_code: Option<i64>,
    pub ts: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DockerInfo {
    pub version: String,
    pub api_version: String,
    pub os: String,
    pub kernel: String,
    pub arch: String,
    pub cpus: i64,
    pub memory: i64,
    pub storage_driver: String,
    pub root_dir: String,
    pub containers: i64,
    pub images: i64,
    pub compose_version: Option<String>,
}

#[derive(Clone)]
pub struct DockerService {
    pub api: Docker,
    inner: Arc<Inner>,
}

struct Inner {
    containers: RwLock<HashMap<String, Arc<Entry>>>,
    images: Mutex<HashMap<String, Arc<ImageMeta>>>,
    bus: EventBus,
    settings: SettingsStore,
    self_id: Option<String>,
    connected: AtomicBool,
    synced: AtomicBool,
    info: RwLock<DockerInfo>,
    lifecycle: broadcast::Sender<Lifecycle>,
    /// Containers with a refresh already scheduled (debounces event bursts).
    pending: Mutex<HashSet<String>>,
}

impl DockerService {
    pub fn new(api: Docker, bus: EventBus, settings: SettingsStore) -> Self {
        let (lifecycle, _) = broadcast::channel(256);
        let self_id = super::detect_self_container_id();
        if let Some(id) = &self_id {
            tracing::info!(container = &id[..12], "running inside a Docker container");
        }
        Self {
            api,
            inner: Arc::new(Inner {
                containers: RwLock::new(HashMap::new()),
                images: Mutex::new(HashMap::new()),
                bus,
                settings,
                self_id,
                connected: AtomicBool::new(false),
                synced: AtomicBool::new(false),
                info: RwLock::new(DockerInfo::default()),
                lifecycle,
                pending: Mutex::new(HashSet::new()),
            }),
        }
    }

    pub fn self_id(&self) -> Option<&str> {
        self.inner.self_id.as_deref()
    }

    pub fn is_connected(&self) -> bool {
        self.inner.connected.load(Ordering::Relaxed)
    }

    pub fn is_synced(&self) -> bool {
        self.inner.synced.load(Ordering::Relaxed)
    }

    pub fn info(&self) -> DockerInfo {
        self.inner.info.read().clone()
    }

    pub fn set_compose_version(&self, v: Option<String>) {
        self.inner.info.write().compose_version = v;
    }

    pub fn subscribe_lifecycle(&self) -> broadcast::Receiver<Lifecycle> {
        self.inner.lifecycle.subscribe()
    }

    /// All containers, sorted by stack then name.
    pub fn list(&self) -> Vec<Arc<ContainerSummary>> {
        let mut v: Vec<Arc<ContainerSummary>> = self
            .inner
            .containers
            .read()
            .values()
            .map(|e| e.summary.clone())
            .collect();
        v.sort_by(|a, b| {
            a.stack
                .as_deref()
                .unwrap_or("~")
                .cmp(b.stack.as_deref().unwrap_or("~"))
                .then_with(|| a.name.cmp(&b.name))
        });
        v
    }

    pub fn entries(&self) -> Vec<Arc<Entry>> {
        self.inner.containers.read().values().cloned().collect()
    }

    /// Looks a container up by full id, id prefix (4+ chars), name or identity.
    pub fn get(&self, key: &str) -> Option<Arc<Entry>> {
        let map = self.inner.containers.read();
        if let Some(e) = map.get(key) {
            return Some(e.clone());
        }
        let key = key.trim_start_matches('/');
        if let Some(e) = map.values().find(|e| e.summary.name == key || e.summary.identity == key) {
            return Some(e.clone());
        }
        if key.len() >= 4 && key.chars().all(|c| c.is_ascii_hexdigit()) {
            let mut matches = map.values().filter(|e| e.summary.id.starts_with(key));
            if let (Some(first), None) = (matches.next(), matches.next()) {
                return Some(first.clone());
            }
        }
        None
    }

    /// Finds the container currently holding an identity.
    pub fn by_identity(&self, identity: &str) -> Option<Arc<Entry>> {
        self.inner
            .containers
            .read()
            .values()
            .find(|e| e.summary.identity == identity)
            .cloned()
    }

    /// Containers in a compose project.
    pub fn in_stack(&self, project: &str) -> Vec<Arc<Entry>> {
        self.inner
            .containers
            .read()
            .values()
            .filter(|e| e.summary.stack.as_deref() == Some(project))
            .cloned()
            .collect()
    }

    /// The host shown in quick links.
    fn public_host(&self) -> String {
        let configured = self.inner.settings.get().general.public_host.trim().to_string();
        if configured.is_empty() { BROWSER_HOST.to_string() } else { configured }
    }

    /// Cached image metadata, fetched on first use.
    pub async fn image_meta(&self, image_id: &str) -> Option<Arc<ImageMeta>> {
        if image_id.is_empty() {
            return None;
        }
        if let Some(m) = self.inner.images.lock().get(image_id) {
            return Some(m.clone());
        }
        match self.api.inspect_image(image_id).await {
            Ok(inspect) => {
                let meta = Arc::new(ImageMeta::from_inspect(inspect));
                self.inner.images.lock().insert(image_id.to_string(), meta.clone());
                Some(meta)
            }
            Err(err) => {
                tracing::debug!(%err, image_id, "image inspect failed");
                None
            }
        }
    }

    /// Fresh image metadata for a reference (tag), bypassing the cache.
    pub async fn inspect_image_fresh(&self, reference: &str) -> Option<ImageMeta> {
        self.api.inspect_image(reference).await.ok().map(ImageMeta::from_inspect)
    }

    pub fn invalidate_images(&self) {
        self.inner.images.lock().clear();
    }

    /// Re-inspects one container and publishes the change (or removal).
    pub async fn refresh(&self, id: &str) {
        match self.api.inspect_container(id, None::<InspectContainerOptions>).await {
            Ok(inspect) => {
                self.upsert(inspect).await;
            }
            Err(err) if crate::error::is_not_found(&err) => self.remove(id),
            Err(err) => tracing::debug!(%err, id, "inspect failed"),
        }
    }

    async fn upsert(&self, inspect: ContainerInspectResponse) -> Option<Arc<Entry>> {
        let image_id = inspect.image.clone().unwrap_or_default();
        let image = self.image_meta(&image_id).await;
        let host = self.public_host();
        let summary = model::summarize(SummaryInput {
            inspect: &inspect,
            image: image.as_deref(),
            public_host: &host,
            self_id: self.inner.self_id.as_deref(),
        })?;
        let id = summary.id.clone();
        let changed;
        let entry = Arc::new(Entry {
            summary: Arc::new(summary),
            inspect: Arc::new(inspect),
        });
        {
            let mut map = self.inner.containers.write();
            changed = map.get(&id).is_none_or(|old| old.summary != entry.summary);
            map.insert(id, entry.clone());
        }
        if changed {
            self.inner.bus.publish(Event::Container(entry.summary.clone()));
        }
        Some(entry)
    }

    fn remove(&self, id: &str) {
        let removed = {
            let mut map = self.inner.containers.write();
            match map.remove(id) {
                Some(e) => Some(e),
                None => {
                    let full = map.keys().find(|k| k.starts_with(id)).cloned();
                    full.and_then(|k| map.remove(&k))
                }
            }
        };
        if let Some(e) = removed {
            self.inner.bus.publish(Event::ContainerRemoved {
                id: e.summary.id.clone(),
                identity: e.summary.identity.clone(),
            });
        }
    }

    /// Lists and inspects every container, replacing the inventory.
    pub async fn full_sync(&self) -> anyhow::Result<()> {
        let list = self
            .api
            .list_containers(Some(ListContainersOptions {
                all: true,
                ..Default::default()
            }))
            .await?;
        let ids: Vec<String> = list.into_iter().filter_map(|c| c.id).collect();
        let seen: HashSet<String> = ids.iter().cloned().collect();
        let this = self.clone();
        stream::iter(ids)
            .map(|id| {
                let this = this.clone();
                async move { this.refresh(&id).await }
            })
            .buffer_unordered(8)
            .collect::<Vec<_>>()
            .await;
        let stale: Vec<String> = self
            .inner
            .containers
            .read()
            .keys()
            .filter(|k| !seen.contains(*k))
            .cloned()
            .collect();
        for id in stale {
            self.remove(&id);
        }
        self.inner.synced.store(true, Ordering::Relaxed);
        Ok(())
    }

    async fn refresh_info(&self) {
        let (info, version) = tokio::join!(self.api.info(), self.api.version());
        let mut cur = self.inner.info.write();
        if let Ok(i) = info {
            cur.os = i.operating_system.unwrap_or_default();
            cur.kernel = i.kernel_version.unwrap_or_default();
            cur.arch = i.architecture.unwrap_or_default();
            cur.cpus = i.ncpu.unwrap_or(0);
            cur.memory = i.mem_total.unwrap_or(0);
            cur.storage_driver = i.driver.unwrap_or_default();
            cur.root_dir = i.docker_root_dir.unwrap_or_default();
            cur.containers = i.containers.unwrap_or(0);
            cur.images = i.images.unwrap_or(0);
        }
        if let Ok(v) = version {
            cur.version = v.version.unwrap_or_default();
            cur.api_version = v.api_version.unwrap_or_default();
        }
    }

    /// Runs forever: initial sync, then follows the event stream with
    /// reconnects, plus periodic resyncs.
    pub async fn run(self) {
        let mut backoff = 1u64;
        loop {
            if !self.is_connected() {
                let _ = super::negotiate(&self.api).await;
            }
            match self.full_sync().await {
                Ok(()) => {
                    if !self.inner.connected.swap(true, Ordering::Relaxed) {
                        tracing::info!(containers = self.inner.containers.read().len(), "connected to Docker");
                    }
                    self.refresh_info().await;
                    backoff = 1;
                }
                Err(err) => {
                    if self.inner.connected.swap(false, Ordering::Relaxed) || backoff == 1 {
                        tracing::warn!(%err, "cannot reach Docker, retrying");
                    }
                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(30);
                    continue;
                }
            }
            self.follow_events().await;
            self.inner.connected.store(false, Ordering::Relaxed);
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    async fn follow_events(&self) {
        let mut filters = HashMap::new();
        filters.insert("type".to_string(), vec!["container".to_string(), "image".to_string()]);
        let since = (crate::util::now() - 2).to_string();
        let mut events = self.api.events(Some(EventsOptions {
            since: Some(since),
            until: None,
            filters: Some(filters),
        }));
        let resync_every = Duration::from_secs(self.inner.settings.get().docker.resync_secs.max(60) as u64);
        let mut resync = tokio::time::interval(resync_every);
        resync.tick().await;
        let mut info_refresh = tokio::time::interval(Duration::from_secs(3600));
        info_refresh.tick().await;
        loop {
            tokio::select! {
                ev = events.next() => match ev {
                    Some(Ok(ev)) => self.handle_event(ev),
                    Some(Err(err)) => {
                        tracing::warn!(%err, "Docker event stream error");
                        return;
                    }
                    None => {
                        tracing::warn!("Docker event stream ended");
                        return;
                    }
                },
                _ = resync.tick() => {
                    if let Err(err) = self.full_sync().await {
                        tracing::warn!(%err, "resync failed");
                        return;
                    }
                }
                _ = info_refresh.tick() => self.refresh_info().await,
            }
        }
    }

    fn handle_event(&self, ev: EventMessage) {
        let action = ev.action.clone().unwrap_or_default();
        let actor = ev.actor.clone().unwrap_or_default();
        let id = actor.id.clone().unwrap_or_default();
        let attrs = actor.attributes.clone().unwrap_or_default();
        match ev.typ {
            Some(EventMessageTypeEnum::CONTAINER) => {
                // Health checks run as execs; ignore their noise.
                if action.starts_with("exec_") || action == "top" || action.starts_with("archive-path") || action == "attach" || action == "resize" {
                    return;
                }
                let name = attrs.get("name").cloned().unwrap_or_default();
                let identity = model::identity_for(&attrs, &name);
                let _ = self.inner.lifecycle.send(Lifecycle {
                    id: id.clone(),
                    identity,
                    name,
                    action: action.clone(),
                    exit_code: attrs.get("exitCode").and_then(|c| c.parse().ok()),
                    ts: ev.time.unwrap_or_else(crate::util::now),
                });
                if action == "destroy" {
                    self.remove(&id);
                } else {
                    self.schedule_refresh(id);
                }
            }
            Some(EventMessageTypeEnum::IMAGE) => {
                // Tags moved (pull, tag, untag, delete): versions and repo
                // digests may have changed for containers using this image.
                // The actor is a reference for some actions and an image id
                // for others, so drop the whole (small, lazily refilled) cache.
                self.inner.images.lock().clear();
                if matches!(action.as_str(), "pull" | "tag" | "untag" | "delete" | "load" | "import") {
                    let affected: Vec<String> = self
                        .inner
                        .containers
                        .read()
                        .values()
                        .filter(|e| e.summary.image_id == id || e.summary.image == id)
                        .map(|e| e.summary.id.clone())
                        .collect();
                    for cid in affected {
                        self.schedule_refresh(cid);
                    }
                }
            }
            _ => {}
        }
    }

    /// Debounced refresh: bursts of events for one container (kill, die,
    /// stop, ...) trigger a single inspect.
    fn schedule_refresh(&self, id: String) {
        if id.is_empty() || !self.inner.pending.lock().insert(id.clone()) {
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            this.inner.pending.lock().remove(&id);
            this.refresh(&id).await;
        });
    }

    /// Republishes every container (e.g. after the public host setting changed).
    pub async fn rebuild_summaries(&self) {
        let entries = self.entries();
        for e in entries {
            let inspect = (*e.inspect).clone();
            self.upsert(inspect).await;
        }
    }
}
