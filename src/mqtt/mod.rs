//! Home Assistant integration over MQTT with auto-discovery.
//!
//! Every container becomes a device in Home Assistant with:
//! * a running switch (start/stop) and a restart button,
//! * status, version, CPU and memory sensors,
//! * a native `update` entity, so container updates show up in Home
//!   Assistant's Updates panel and can be installed from there.
//!
//! Nodestarr itself is a device with "updates available" and "containers
//! running" sensors and a "check for updates" button.
//!
//! With Proxmox disk health on, every disk of the nodes is a device too (a
//! temperature sensor and a "problem" binary sensor whose attributes carry
//! the verdict and key SMART numbers), and so is every pool (problem and
//! space used).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use rumqttc::{AsyncClient, Event as MqttEvent, LastWill, MqttOptions, Packet, QoS, Transport};
use serde_json::{Value, json};
use tokio::sync::Notify;

use crate::activity::Actor;
use crate::app::App;
use crate::docker::actions::Action;
use crate::events::Event;
use crate::settings::Mqtt as MqttSettings;
use crate::updates::UpdateStatus;

#[derive(Clone, Default)]
pub struct MqttService {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    client: Mutex<Option<AsyncClient>>,
    connected: std::sync::atomic::AtomicBool,
    published: Mutex<HashSet<String>>,
    restart: Notify,
    last_error: Mutex<Option<String>>,
}

impl MqttService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_connected(&self) -> bool {
        self.inner.connected.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn last_error(&self) -> Option<String> {
        self.inner.last_error.lock().clone()
    }

    /// Reconnects with new settings.
    pub fn reload(&self) {
        self.inner.restart.notify_one();
    }

    pub async fn shutdown(&self) {
        let client = self.inner.client.lock().take();
        if let Some(c) = client {
            let _ = c.disconnect().await;
        }
    }
}

fn object_id(identity: &str) -> String {
    crate::util::slug(identity)
}

fn upper_first(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn state_topic(s: &MqttSettings, id: &str) -> String {
    format!("{}/{id}/state", s.base_topic)
}

fn update_topic(s: &MqttSettings, id: &str) -> String {
    format!("{}/{id}/update", s.base_topic)
}

fn command_topic(s: &MqttSettings, id: &str) -> String {
    format!("{}/{id}/set", s.base_topic)
}

fn availability(s: &MqttSettings) -> Value {
    json!([{ "topic": format!("{}/status", s.base_topic) }])
}

fn options(s: &MqttSettings) -> MqttOptions {
    let mut o = MqttOptions::new(s.client_id.clone(), s.host.clone(), s.port);
    o.set_keep_alive(Duration::from_secs(30));
    o.set_clean_session(true);
    o.set_max_packet_size(256 * 1024, 256 * 1024);
    if !s.username.is_empty() {
        o.set_credentials(s.username.clone(), s.password.clone());
    }
    o.set_last_will(LastWill::new(format!("{}/status", s.base_topic), "offline", QoS::AtLeastOnce, true));
    if s.tls {
        o.set_transport(Transport::tls_with_default_config());
    }
    o
}

/// Discovery configs for one container: (topic, payload).
fn discovery(app: &App, s: &MqttSettings, entry: &crate::docker::Entry) -> Vec<(String, Value)> {
    let sm = &entry.summary;
    let id = object_id(&sm.identity);
    let name = crate::updates::check::display_name(app, entry);
    let device = json!({
        "identifiers": [format!("nodestarr_{id}")],
        "name": name,
        "manufacturer": "Nodestarr",
        "model": sm.image,
        "sw_version": sm.version,
        "via_device": "nodestarr",
    });
    let state = state_topic(s, &id);
    let avail = availability(s);
    let prefix = &s.discovery_prefix;
    let mut out = vec![
        (
            format!("{prefix}/sensor/nodestarr_{id}/status/config"),
            json!({
                "name": "Status",
                "unique_id": format!("nodestarr_{id}_status"),
                "state_topic": state,
                "value_template": "{{ value_json.state }}",
                // Health, version and usage as attributes, for automations.
                "json_attributes_topic": state,
                "icon": "mdi:docker",
                "device": device,
                "availability": avail,
            }),
        ),
        (
            format!("{prefix}/sensor/nodestarr_{id}/version/config"),
            json!({
                "name": "Version",
                "unique_id": format!("nodestarr_{id}_version"),
                "state_topic": state,
                "value_template": "{{ value_json.version or 'unknown' }}",
                "icon": "mdi:tag-outline",
                "entity_category": "diagnostic",
                "device": device,
                "availability": avail,
            }),
        ),
        (format!("{prefix}/update/nodestarr_{id}/image/config"), {
            let mut update = json!({
                "name": "Image",
                "unique_id": format!("nodestarr_{id}_update"),
                "state_topic": update_topic(s, &id),
                "device_class": "firmware",
                "device": device,
                "availability": avail,
            });
            // Installable from Home Assistant only when controls are on.
            if s.expose_controls && !sm.is_self {
                update["command_topic"] = json!(command_topic(s, &id));
                update["payload_install"] = json!("update");
            }
            update
        }),
    ];
    if s.expose_controls && !sm.is_self {
        out.push((
            format!("{prefix}/switch/nodestarr_{id}/running/config"),
            json!({
                "name": "Running",
                "unique_id": format!("nodestarr_{id}_running"),
                "state_topic": state,
                "value_template": "{{ 'ON' if value_json.running else 'OFF' }}",
                "command_topic": command_topic(s, &id),
                "payload_on": "start",
                "payload_off": "stop",
                "icon": "mdi:power",
                "device": device,
                "availability": avail,
            }),
        ));
        out.push((
            format!("{prefix}/button/nodestarr_{id}/restart/config"),
            json!({
                "name": "Restart",
                "unique_id": format!("nodestarr_{id}_restart"),
                "command_topic": command_topic(s, &id),
                "payload_press": "restart",
                "device_class": "restart",
                "device": device,
                "availability": avail,
            }),
        ));
    }
    if s.expose_stats {
        out.push((
            format!("{prefix}/sensor/nodestarr_{id}/cpu/config"),
            json!({
                "name": "CPU",
                "unique_id": format!("nodestarr_{id}_cpu"),
                "state_topic": state,
                "value_template": "{{ value_json.cpu }}",
                "unit_of_measurement": "%",
                "state_class": "measurement",
                "icon": "mdi:chip",
                "device": device,
                "availability": avail,
            }),
        ));
        out.push((
            format!("{prefix}/sensor/nodestarr_{id}/memory/config"),
            json!({
                "name": "Memory",
                "unique_id": format!("nodestarr_{id}_memory"),
                "state_topic": state,
                "value_template": "{{ value_json.memory_mb }}",
                "unit_of_measurement": "MB",
                "state_class": "measurement",
                "icon": "mdi:memory",
                "device": device,
                "availability": avail,
            }),
        ));
    }
    out
}

fn nodestarr_discovery(s: &MqttSettings, instance: &str) -> Vec<(String, Value)> {
    let device = json!({
        "identifiers": ["nodestarr"],
        "name": instance,
        "manufacturer": "Nodestarr",
        "model": "Container manager",
        "sw_version": crate::VERSION,
    });
    let prefix = &s.discovery_prefix;
    let summary = format!("{}/summary", s.base_topic);
    let avail = availability(s);
    vec![
        (
            format!("{prefix}/sensor/nodestarr/updates/config"),
            json!({
                "name": "Updates available",
                "unique_id": "nodestarr_updates_available",
                "state_topic": summary,
                "value_template": "{{ value_json.updates }}",
                "icon": "mdi:package-up",
                "device": device,
                "availability": avail,
            }),
        ),
        (
            format!("{prefix}/sensor/nodestarr/running/config"),
            json!({
                "name": "Containers running",
                "unique_id": "nodestarr_containers_running",
                "state_topic": summary,
                "value_template": "{{ value_json.running }}",
                "icon": "mdi:docker",
                "device": device,
                "availability": avail,
            }),
        ),
        (
            format!("{prefix}/button/nodestarr/check/config"),
            json!({
                "name": "Check for updates",
                "unique_id": "nodestarr_check_updates",
                "command_topic": format!("{}/command", s.base_topic),
                "payload_press": "check_updates",
                "icon": "mdi:refresh",
                "device": device,
                "availability": avail,
            }),
        ),
    ]
}

async fn publish_json(client: &AsyncClient, topic: String, payload: &Value, retain: bool) {
    if let Err(err) = client.publish(topic, QoS::AtLeastOnce, retain, payload.to_string()).await {
        tracing::debug!(%err, "MQTT publish failed");
    }
}

fn storage_on(app: &App) -> bool {
    let s = app.settings.get();
    s.proxmox.enabled && s.proxmox.disks.enabled
}

fn disk_object_id(id: &str) -> String {
    object_id(&format!("disk-{id}"))
}

fn pool_object_id(id: &str) -> String {
    object_id(&format!("pool-{id}"))
}

/// Discovery configs for the Proxmox disks and pools.
fn storage_discovery(app: &App, s: &MqttSettings) -> Vec<(String, Value)> {
    if !storage_on(app) {
        return Vec::new();
    }
    let prefix = &s.discovery_prefix;
    let avail = availability(s);
    let mut out = Vec::new();
    for d in app.disks.disks().iter().filter(|d| d.present) {
        let id = disk_object_id(&d.id);
        let state = format!("{}/disk/{id}/state", s.base_topic);
        let model = d.model.clone().unwrap_or_else(|| "Disk".into());
        let name = if d.label.bay.is_empty() {
            format!("{model} {}", d.devpath)
        } else {
            format!("{model} ({})", d.label.bay)
        };
        let device = json!({
            "identifiers": [format!("nodestarr_{id}")],
            "name": name,
            "manufacturer": d.family.clone().or_else(|| d.vendor.clone()).unwrap_or_else(|| "Disk".into()),
            "model": model,
            "serial_number": d.serial,
            "sw_version": d.firmware,
            "via_device": "nodestarr",
        });
        if d.temperature.is_some() {
            out.push((
                format!("{prefix}/sensor/nodestarr_{id}/temperature/config"),
                json!({
                    "name": "Temperature",
                    "unique_id": format!("nodestarr_{id}_temperature"),
                    "state_topic": state,
                    "value_template": "{{ value_json.temperature }}",
                    "device_class": "temperature",
                    "unit_of_measurement": "\u{b0}C",
                    "state_class": "measurement",
                    "device": device,
                    "availability": avail,
                }),
            ));
        }
        out.push((
            format!("{prefix}/binary_sensor/nodestarr_{id}/problem/config"),
            json!({
                "name": "Problem",
                "unique_id": format!("nodestarr_{id}_problem"),
                "state_topic": state,
                "value_template": "{{ value_json.problem }}",
                "json_attributes_topic": state,
                "device_class": "problem",
                "device": device,
                "availability": avail,
            }),
        ));
    }
    for p in app.disks.pools() {
        let id = pool_object_id(&p.id);
        let state = format!("{}/pool/{id}/state", s.base_topic);
        let device = json!({
            "identifiers": [format!("nodestarr_{id}")],
            "name": format!("{} pool {}", p.kind.as_str().to_uppercase(), p.name),
            "manufacturer": "Nodestarr",
            "model": p.layout,
            "via_device": "nodestarr",
        });
        out.push((
            format!("{prefix}/binary_sensor/nodestarr_{id}/problem/config"),
            json!({
                "name": "Problem",
                "unique_id": format!("nodestarr_{id}_problem"),
                "state_topic": state,
                "value_template": "{{ value_json.problem }}",
                "json_attributes_topic": state,
                "device_class": "problem",
                "device": device,
                "availability": avail,
            }),
        ));
        if p.size.is_some() && p.used.is_some() {
            out.push((
                format!("{prefix}/sensor/nodestarr_{id}/used/config"),
                json!({
                    "name": "Used",
                    "unique_id": format!("nodestarr_{id}_used"),
                    "state_topic": state,
                    "value_template": "{{ value_json.used_percent }}",
                    "unit_of_measurement": "%",
                    "state_class": "measurement",
                    "icon": "mdi:database",
                    "device": device,
                    "availability": avail,
                }),
            ));
        }
    }
    out
}

/// The state payload of a disk.
fn disk_state(d: &crate::proxmox::disks::DiskView) -> Value {
    use crate::proxmox::disks::smart::Level;
    let problem = d.verdict >= Level::Warning || d.passed == Some(false);
    json!({
        "problem": if problem { "ON" } else { "OFF" },
        "verdict": d.verdict.as_str(),
        "reason": d.findings.iter().find(|f| f.level >= Level::Watch).map(|f| f.title.clone()),
        "temperature": d.temperature,
        "asleep": d.asleep,
        "device": d.devpath,
        "node": d.node,
        "serial": d.serial,
        "power_on_hours": d.power_on_hours,
        "reallocated_sectors": d.reallocated,
        "pending_sectors": d.pending,
        "crc_errors": d.crc_errors,
        "wear_used_percent": d.wear_used,
        "last_test": d.last_test.as_ref().map(|t| format!("{} {:?}", t.kind, t.outcome).to_lowercase()),
    })
}

/// The state payload of a pool.
fn pool_state(p: &crate::proxmox::disks::pools::Pool) -> Value {
    use crate::proxmox::disks::pools::PoolHealth;
    use crate::proxmox::disks::smart::Level;
    let problem = p.health != PoolHealth::Ok || p.findings.iter().any(|f| f.level >= Level::Warning);
    json!({
        "problem": if problem { "ON" } else { "OFF" },
        "state": p.state,
        "reason": p.findings.iter().find(|f| f.level >= Level::Watch).map(|f| f.title.clone()),
        "used_percent": p.size.zip(p.used).filter(|(s, _)| *s > 0).map(|(s, u)| (u as f64 * 1000.0 / s as f64).round() / 10.0),
        "failures_tolerated": p.tolerance.map(|t| t.remaining.max(0)),
        "scan": p.scan.as_ref().map(|s| format!("{} {}", s.kind, s.state)),
        "scan_progress": p.scan.as_ref().and_then(|s| s.progress).map(|x| (x * 1000.0).round() / 10.0),
        "node": p.node,
    })
}

async fn publish_storage_states(app: &App, client: &AsyncClient, s: &MqttSettings) {
    if !storage_on(app) {
        return;
    }
    for d in app.disks.disks().iter().filter(|d| d.present) {
        let topic = format!("{}/disk/{}/state", s.base_topic, disk_object_id(&d.id));
        publish_json(client, topic, &disk_state(d), true).await;
    }
    for p in app.disks.pools() {
        let topic = format!("{}/pool/{}/state", s.base_topic, pool_object_id(&p.id));
        publish_json(client, topic, &pool_state(&p), true).await;
    }
}

/// Which disks and pools exist: when it changes, discovery is resent.
fn storage_signature(app: &App) -> String {
    if !storage_on(app) {
        return String::new();
    }
    let mut ids: Vec<String> = app
        .disks
        .disks()
        .into_iter()
        .filter(|d| d.present)
        .map(|d| format!("{}:{}:{}", d.id, d.temperature.is_some(), d.label.bay))
        .collect();
    ids.extend(app.disks.pools().into_iter().map(|p| p.id));
    ids.join("|")
}

async fn publish_states(app: &App, client: &AsyncClient, s: &MqttSettings) {
    let stats = crate::monitor::stats::latest(app);
    let updates = app.updates.all();
    let mut running = 0;
    let mut available = 0;
    for e in app.docker.entries() {
        let sm = &e.summary;
        let id = object_id(&sm.identity);
        if sm.is_running() {
            running += 1;
        }
        let st = stats.get(&sm.identity);
        let state = json!({
            "state": sm.state,
            "health": sm.health,
            "running": sm.is_running(),
            "version": sm.version,
            "cpu": st.map(|x| (x.cpu * 10.0).round() / 10.0).unwrap_or(0.0),
            "memory_mb": st.map(|x| x.mem / (1024 * 1024)).unwrap_or(0),
        });
        publish_json(client, state_topic(s, &id), &state, true).await;
        let info = updates.get(&sm.identity);
        let has_update = info.as_ref().is_some_and(|i| i.has_update());
        if has_update {
            available += 1;
        }
        let installed = sm.version.clone().unwrap_or_else(|| sm.short_id().to_string());
        let latest = match info {
            Some(i) if i.status == UpdateStatus::Available || i.status == UpdateStatus::PendingRecreate => i
                .newer_tag
                .as_ref()
                .map(|t| t.tag.clone())
                .or_else(|| i.latest_version.clone())
                .unwrap_or_else(|| format!("{installed} (new build)")),
            _ => installed.clone(),
        };
        let change = info.and_then(|i| i.offered().cloned());
        // Home Assistant shows these under the update (at most 255 characters).
        let summary = change.as_ref().map(|c| crate::util::truncate(&upper_first(&c.summary()), 255));
        let url = change.as_ref().and_then(|c| c.notes.as_ref()).and_then(|n| n.url.clone());
        let upd = json!({
            "installed_version": installed,
            "latest_version": latest,
            "title": crate::updates::check::display_name(app, &e),
            "in_progress": app.tasks.is_busy(&sm.identity),
            "release_summary": summary,
            "release_url": url,
        });
        publish_json(client, update_topic(s, &id), &upd, true).await;
    }
    publish_json(
        client,
        format!("{}/summary", s.base_topic),
        &json!({ "running": running, "updates": available }),
        true,
    )
    .await;
    publish_storage_states(app, client, s).await;
}

async fn publish_discovery(app: &App, client: &AsyncClient, s: &MqttSettings) {
    let mut current = HashSet::new();
    for (topic, payload) in nodestarr_discovery(s, &app.settings.get().general.instance_name) {
        current.insert(topic.clone());
        publish_json(client, topic, &payload, true).await;
    }
    for e in app.docker.entries() {
        if app.prefs.get(&e.summary.identity).hidden {
            continue;
        }
        for (topic, payload) in discovery(app, s, &e) {
            current.insert(topic.clone());
            publish_json(client, topic, &payload, true).await;
        }
    }
    for (topic, payload) in storage_discovery(app, s) {
        current.insert(topic.clone());
        publish_json(client, topic, &payload, true).await;
    }
    // Remove entities of containers that no longer exist.
    let stale: Vec<String> = {
        let mut published = app.mqtt.inner.published.lock();
        let stale = published.difference(&current).cloned().collect();
        *published = current;
        stale
    };
    for topic in stale {
        let _ = client.publish(topic, QoS::AtLeastOnce, true, Vec::<u8>::new()).await;
    }
}

async fn handle_command(app: &App, s: &MqttSettings, topic: &str, payload: &str) {
    let actor = Actor::mqtt();
    if topic == format!("{}/command", s.base_topic) {
        if payload == "check_updates" {
            let app2 = app.clone();
            tokio::spawn(async move {
                crate::updates::check::check_all(&app2, true, &Actor::mqtt()).await;
            });
        }
        return;
    }
    // Container commands only when controls are exposed (Settings > Home
    // Assistant); otherwise Home Assistant can watch but not act.
    if !s.expose_controls {
        return;
    }
    let Some(rest) = topic.strip_prefix(&format!("{}/", s.base_topic)) else { return };
    let Some(id) = rest.strip_suffix("/set") else { return };
    let Some(entry) = app.docker.entries().into_iter().find(|e| object_id(&e.summary.identity) == id) else { return };
    let key = entry.summary.id.clone();
    let result = match payload {
        "start" => crate::ops::container_action(app, &key, Action::Start, Default::default(), &actor).await.map(|_| ()),
        "stop" => crate::ops::container_action(app, &key, Action::Stop, Default::default(), &actor).await.map(|_| ()),
        "restart" => {
            crate::ops::container_action(app, &key, Action::Restart, Default::default(), &actor).await.map(|_| ())
        }
        "update" | "install" => crate::updates::apply::start_update(
            app,
            &key,
            Default::default(),
            &actor,
            crate::updates::apply::Trigger::Mqtt,
        )
        .map(|_| ()),
        _ => return,
    };
    if let Err(err) = result {
        tracing::warn!(%err, container = %entry.summary.name, "Home Assistant command failed");
    }
}

/// Runs the MQTT bridge, reconnecting on errors and on settings changes.
pub fn start(app: App) {
    tokio::spawn(async move {
        let mut settings_rx = app.settings.subscribe();
        loop {
            let s = app.settings.get().mqtt.clone();
            if !s.enabled || s.host.is_empty() {
                app.mqtt.inner.connected.store(false, std::sync::atomic::Ordering::Relaxed);
                tokio::select! {
                    _ = settings_rx.changed() => continue,
                    _ = app.mqtt.inner.restart.notified() => continue,
                }
            }
            run_session(&app, &s, &mut settings_rx).await;
            app.mqtt.inner.connected.store(false, std::sync::atomic::Ordering::Relaxed);
            *app.mqtt.inner.client.lock() = None;
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

/// What the publisher task should send next. Later variants include the
/// earlier ones, so pending requests combine with `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Publish {
    /// Container states and the summary.
    States,
    /// Discovery configs too (containers or settings changed).
    Everything,
    /// Just connected: subscriptions, availability, then everything.
    Connected,
}

/// Coalescing hand-off from the session loop to the publisher task.
#[derive(Default)]
struct Pending {
    wanted: Mutex<Option<Publish>>,
    notify: Notify,
}

impl Pending {
    fn request(&self, what: Publish) {
        {
            let mut wanted = self.wanted.lock();
            *wanted = Some(wanted.map_or(what, |cur| cur.max(what)));
        }
        self.notify.notify_one();
    }

    async fn next(&self) -> Publish {
        loop {
            if let Some(what) = self.wanted.lock().take() {
                return what;
            }
            self.notify.notified().await;
        }
    }
}

/// Aborts the publisher task when the session ends.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// One connection to the broker, until it fails or the settings change.
///
/// rumqttc only sends what is queued while its event loop is polled, and a
/// publish waits when the queue is full. So this loop never waits on the
/// client: publishing runs in a separate task, commands from Home
/// Assistant run in their own tasks, and shutdown uses non-blocking calls.
async fn run_session(
    app: &App,
    s: &MqttSettings,
    settings_rx: &mut tokio::sync::watch::Receiver<Arc<crate::settings::Settings>>,
) {
    let (client, mut eventloop) = AsyncClient::new(options(s), 256);
    *app.mqtt.inner.client.lock() = Some(client.clone());
    let pending = Arc::new(Pending::default());
    let _publisher = AbortOnDrop(tokio::spawn({
        let (app, client, s, pending) = (app.clone(), client.clone(), s.clone(), pending.clone());
        async move {
            loop {
                let what = pending.next().await;
                if what == Publish::Connected {
                    if s.expose_controls {
                        let _ = client.subscribe(format!("{}/+/set", s.base_topic), QoS::AtLeastOnce).await;
                    }
                    let _ = client.subscribe(format!("{}/command", s.base_topic), QoS::AtLeastOnce).await;
                    let _ = client.subscribe(format!("{}/status", s.discovery_prefix), QoS::AtLeastOnce).await;
                    let _ = client.publish(format!("{}/status", s.base_topic), QoS::AtLeastOnce, true, "online").await;
                }
                if what >= Publish::Everything {
                    publish_discovery(&app, &client, &s).await;
                }
                publish_states(&app, &client, &s).await;
            }
        }
    }));

    let mut bus = app.bus.subscribe();
    let mut refresh = tokio::time::interval(Duration::from_secs(300));
    let mut stats_tick =
        tokio::time::interval(Duration::from_secs(app.settings.get().monitoring.sample_secs.max(15) as u64));
    let mut dirty = false;
    let mut storage = storage_signature(app);
    let mut debounce = tokio::time::interval(Duration::from_secs(2));
    let original = s.clone();
    loop {
        tokio::select! {
            ev = eventloop.poll() => match ev {
                Ok(MqttEvent::Incoming(Packet::ConnAck(_))) => {
                    tracing::info!(host = %s.host, "connected to MQTT broker");
                    app.mqtt.inner.connected.store(true, std::sync::atomic::Ordering::Relaxed);
                    *app.mqtt.inner.last_error.lock() = None;
                    pending.request(Publish::Connected);
                }
                Ok(MqttEvent::Incoming(Packet::Publish(p))) => {
                    let payload = String::from_utf8_lossy(&p.payload).trim().to_string();
                    if p.topic == format!("{}/status", s.discovery_prefix) {
                        if payload == "online" {
                            // Home Assistant restarted: resend everything.
                            pending.request(Publish::Everything);
                        }
                    } else {
                        let (app, s, topic) = (app.clone(), s.clone(), p.topic.clone());
                        tokio::spawn(async move { handle_command(&app, &s, &topic, &payload).await });
                    }
                }
                Ok(_) => {}
                Err(err) => {
                    let msg = err.to_string();
                    if app.mqtt.inner.last_error.lock().as_deref() != Some(msg.as_str()) {
                        tracing::warn!(error = %msg, "MQTT connection error");
                    }
                    *app.mqtt.inner.last_error.lock() = Some(msg);
                    return;
                }
            },
            ev = bus.recv() => match ev {
                Ok(Event::Container(_) | Event::ContainerRemoved { .. } | Event::Update { .. } | Event::Task(_) | Event::Prefs { .. }) => dirty = true,
                Ok(Event::Disks) if app.mqtt.is_connected() => {
                    // New or removed disks and pools need discovery; otherwise states will do.
                    let now = storage_signature(app);
                    if now != storage {
                        storage = now;
                        dirty = true;
                    } else {
                        pending.request(Publish::States);
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => dirty = true,
                Err(_) => return,
            },
            _ = debounce.tick() => {
                if dirty && app.mqtt.is_connected() {
                    dirty = false;
                    pending.request(Publish::Everything);
                }
            }
            _ = stats_tick.tick() => {
                if s.expose_stats && app.mqtt.is_connected() {
                    pending.request(Publish::States);
                }
            }
            _ = refresh.tick() => {
                if app.mqtt.is_connected() {
                    pending.request(Publish::States);
                }
            }
            _ = settings_rx.changed() => {
                let next = settings_rx.borrow().mqtt.clone();
                if serde_json::to_value(&next).ok() != serde_json::to_value(&original).ok() {
                    close(&client, &mut eventloop, s).await;
                    return;
                }
            }
            _ = app.mqtt.inner.restart.notified() => {
                close(&client, &mut eventloop, s).await;
                return;
            }
        }
    }
}

/// Says goodbye: marks Nodestarr offline and disconnects, giving the event
/// loop a moment to send both. (If that fails, the broker publishes the
/// last will, which says the same.)
async fn close(client: &AsyncClient, eventloop: &mut rumqttc::EventLoop, s: &MqttSettings) {
    let _ = client.try_publish(format!("{}/status", s.base_topic), QoS::AtLeastOnce, true, "offline");
    let _ = client.try_disconnect();
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match eventloop.poll().await {
                Ok(MqttEvent::Outgoing(rumqttc::Outgoing::Disconnect)) | Err(_) => break,
                Ok(_) => {}
            }
        }
    })
    .await;
}

/// Connects with the given (unsaved) settings and reports whether the broker
/// accepted the connection. Uses a separate client id and no last will, so
/// it doesn't disturb the running bridge.
pub async fn test(s: &MqttSettings) -> crate::error::Result<()> {
    use crate::error::AppError;
    if s.host.trim().is_empty() {
        return Err(AppError::bad_request("enter the broker's host name or IP address"));
    }
    let mut o =
        MqttOptions::new(format!("{}-test-{}", s.client_id, crate::util::random_token(4)), s.host.trim(), s.port);
    o.set_keep_alive(Duration::from_secs(10));
    o.set_clean_session(true);
    if !s.username.is_empty() {
        o.set_credentials(s.username.clone(), s.password.clone());
    }
    if s.tls {
        o.set_transport(Transport::tls_with_default_config());
    }
    let (client, mut eventloop) = AsyncClient::new(o, 4);
    let outcome = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match eventloop.poll().await {
                Ok(MqttEvent::Incoming(Packet::ConnAck(ack))) => {
                    return if ack.code == rumqttc::ConnectReturnCode::Success {
                        Ok(())
                    } else {
                        Err(format!("the broker refused the connection ({:?})", ack.code))
                    };
                }
                Ok(_) => {}
                Err(err) => return Err(err.to_string()),
            }
        }
    })
    .await;
    let _ = client.try_disconnect();
    match outcome {
        Ok(Ok(())) => Ok(()),
        Ok(Err(msg)) => Err(AppError::upstream(msg)),
        Err(_) => Err(AppError::upstream(format!("no answer from {}:{} within 10 seconds", s.host.trim(), s.port))),
    }
}

/// Topic layout, for the docs page in Settings.
pub fn topics(s: &MqttSettings) -> HashMap<&'static str, String> {
    HashMap::from([
        ("availability", format!("{}/status", s.base_topic)),
        ("state", format!("{}/<container>/state", s.base_topic)),
        ("update", format!("{}/<container>/update", s.base_topic)),
        ("commands", format!("{}/<container>/set (start, stop, restart, update)", s.base_topic)),
        ("nodestarr", format!("{}/command (check_updates)", s.base_topic)),
        ("disks", format!("{}/disk/<disk>/state, {}/pool/<pool>/state", s.base_topic, s.base_topic)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_topics() {
        let s = MqttSettings::default();
        assert_eq!(object_id("compose:media/sonarr"), "compose-media-sonarr");
        assert_eq!(state_topic(&s, "x"), "nodestarr/x/state");
        assert_eq!(command_topic(&s, "x"), "nodestarr/x/set");
        let d = nodestarr_discovery(&s, "Nodestarr");
        assert!(d.iter().all(|(t, _)| t.starts_with("homeassistant/")));
        assert_eq!(disk_object_id("WDC_X_WD-1"), object_id("disk-WDC_X_WD-1"));
    }

    #[test]
    fn storage_states() {
        use crate::proxmox::disks::smart::{Finding, Level};
        let mut d = crate::proxmox::disks::DiskView {
            devpath: "/dev/sdc".into(),
            temperature: Some(38.0),
            verdict: Level::Watch,
            findings: vec![Finding {
                level: Level::Watch,
                code: "reallocated".into(),
                title: "8 sectors reallocated".into(),
                detail: String::new(),
            }],
            ..Default::default()
        };
        let v = disk_state(&d);
        assert_eq!(v["problem"], "OFF");
        assert_eq!(v["reason"], "8 sectors reallocated");
        assert_eq!(v["temperature"], 38.0);
        d.verdict = Level::Warning;
        assert_eq!(disk_state(&d)["problem"], "ON");
        let p = crate::proxmox::disks::pools::Pool {
            size: Some(1000),
            used: Some(333),
            health: crate::proxmox::disks::pools::PoolHealth::Degraded,
            ..Default::default()
        };
        let v = pool_state(&p);
        assert_eq!(v["problem"], "ON");
        assert_eq!(v["used_percent"], 33.3);
    }
}
