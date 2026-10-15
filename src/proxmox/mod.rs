//! Proxmox VE: host and guest status, power actions, snapshots, vzdump
//! backups, tasks, disks and pending host updates.

pub mod client;

use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::activity::{Actor, NewActivity};
use crate::app::App;
use crate::docker::DockerService;
use crate::error::{AppError, Result};
use crate::notify::Notification;
use crate::settings::{Severity, SettingsStore};
use crate::tasks::{TaskCtx, TaskSpec};

use self::client::PveClient;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeStatus {
    pub node: String,
    pub cpu: f64,
    pub cpus: u64,
    pub cpu_model: String,
    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub rootfs_used: u64,
    pub rootfs_total: u64,
    pub loadavg: Vec<String>,
    pub uptime: u64,
    pub pve_version: String,
    pub kernel: String,
    pub ksm_shared: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Guest {
    pub vmid: u32,
    pub name: String,
    /// "lxc" or "qemu".
    pub kind: String,
    pub node: String,
    pub status: String,
    pub cpu: f64,
    pub cpus: f64,
    pub mem: u64,
    pub maxmem: u64,
    pub disk: u64,
    pub maxdisk: u64,
    pub netin: u64,
    pub netout: u64,
    pub uptime: u64,
    pub tags: Vec<String>,
    pub template: bool,
    pub is_self: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Storage {
    pub storage: String,
    pub kind: String,
    pub content: Vec<String>,
    pub used: u64,
    pub total: u64,
    pub active: bool,
    pub shared: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Overview {
    pub node: NodeStatus,
    pub guests: Vec<Guest>,
    pub storages: Vec<Storage>,
    pub fetched_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub snaptime: Option<i64>,
    pub parent: Option<String>,
    #[serde(default)]
    pub vmstate: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupFile {
    pub volid: String,
    pub ctime: Option<i64>,
    pub size: Option<u64>,
    pub vmid: Option<u32>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub protected: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PveTask {
    pub upid: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub status: Option<String>,
    pub starttime: Option<i64>,
    pub endtime: Option<i64>,
    pub user: Option<String>,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AptUpdate {
    #[serde(rename = "Package")]
    pub package: String,
    #[serde(rename = "Title", default)]
    pub title: String,
    #[serde(rename = "OldVersion", default)]
    pub old_version: String,
    #[serde(rename = "Version", default)]
    pub version: String,
    #[serde(rename = "Priority", default)]
    pub priority: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Disk {
    pub devpath: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub serial: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub health: Option<String>,
    #[serde(default)]
    pub wearout: Option<Value>,
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub used: Option<String>,
}

#[derive(Clone)]
pub struct ProxmoxService {
    inner: Arc<Inner>,
}

struct Inner {
    settings: SettingsStore,
    client: Mutex<Option<(String, PveClient)>>,
    overview: RwLock<Option<Arc<Overview>>>,
    docker: DockerService,
}

fn v_u64(v: &Value) -> u64 {
    v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)).or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0)
}

fn v_f64(v: &Value) -> f64 {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(0.0)
}

impl ProxmoxService {
    /// `docker` provides the Docker host's name, used to recognize the guest
    /// Berth runs in when no guest ID is configured.
    pub fn new(settings: SettingsStore, docker: DockerService) -> Self {
        Self {
            inner: Arc::new(Inner {
                settings,
                docker,
                client: Mutex::new(None),
                overview: RwLock::new(None),
            }),
        }
    }

    /// The guest Berth runs in: the configured ID, or the one guest whose
    /// name matches the Docker host's name.
    pub fn self_vmid(&self) -> Option<u32> {
        if let Some(id) = self.inner.settings.get().proxmox.self_vmid {
            return Some(id);
        }
        let overview = self.inner.overview.read().clone()?;
        detect_self(&overview.guests, &self.inner.docker.info().name)
    }

    pub fn enabled(&self) -> bool {
        self.inner.settings.get().proxmox.enabled
    }

    fn client(&self) -> Result<PveClient> {
        let s = self.inner.settings.get();
        if !s.proxmox.enabled {
            return Err(AppError::conflict("Proxmox integration is not enabled (Settings > Proxmox)"));
        }
        let key = format!("{}|{}|{}|{:?}|{}", s.proxmox.url, s.proxmox.token_id, s.proxmox.token_secret, s.proxmox.tls_mode, s.proxmox.fingerprint);
        let mut guard = self.inner.client.lock();
        if let Some((k, c)) = guard.as_ref() {
            if *k == key {
                return Ok(c.clone());
            }
        }
        let c = PveClient::new(&s.proxmox)?;
        *guard = Some((key, c.clone()));
        Ok(c)
    }

    async fn node(&self, c: &PveClient) -> Result<String> {
        let configured = self.inner.settings.get().proxmox.node.clone();
        if !configured.is_empty() {
            return Ok(configured);
        }
        let nodes: Vec<Value> = c.get("/nodes").await?;
        nodes
            .iter()
            .filter_map(|n| n["node"].as_str())
            .next()
            .map(str::to_string)
            .ok_or_else(|| AppError::upstream("Proxmox reported no nodes"))
    }

    /// Cached overview (refreshed when older than `max_age` seconds).
    pub async fn overview(&self, max_age: i64) -> Result<Arc<Overview>> {
        if let Some(o) = self.inner.overview.read().clone() {
            if crate::util::now() - o.fetched_at <= max_age {
                return Ok(o);
            }
        }
        let c = self.client()?;
        let node = self.node(&c).await?;
        let status_path = format!("/nodes/{node}/status");
        let (status, resources) = tokio::join!(c.get::<Value>(&status_path), c.get::<Vec<Value>>("/cluster/resources"));
        let status = status?;
        let resources = resources?;
        let configured_self = self.inner.settings.get().proxmox.self_vmid;
        let mut ov = Overview {
            node: NodeStatus {
                node: node.clone(),
                cpu: v_f64(&status["cpu"]),
                cpus: v_u64(&status["cpuinfo"]["cpus"]),
                cpu_model: status["cpuinfo"]["model"].as_str().unwrap_or_default().to_string(),
                mem_used: v_u64(&status["memory"]["used"]),
                mem_total: v_u64(&status["memory"]["total"]),
                swap_used: v_u64(&status["swap"]["used"]),
                swap_total: v_u64(&status["swap"]["total"]),
                rootfs_used: v_u64(&status["rootfs"]["used"]),
                rootfs_total: v_u64(&status["rootfs"]["total"]),
                loadavg: status["loadavg"]
                    .as_array()
                    .map(|a| a.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).collect())
                    .unwrap_or_default(),
                uptime: v_u64(&status["uptime"]),
                pve_version: status["pveversion"].as_str().unwrap_or_default().to_string(),
                kernel: status["kversion"].as_str().unwrap_or_default().to_string(),
                ksm_shared: v_u64(&status["ksm"]["shared"]),
            },
            guests: Vec::new(),
            storages: Vec::new(),
            fetched_at: crate::util::now(),
        };
        for r in &resources {
            match r["type"].as_str() {
                Some(t @ ("lxc" | "qemu")) => {
                    let vmid = v_u64(&r["vmid"]) as u32;
                    ov.guests.push(Guest {
                        vmid,
                        name: r["name"].as_str().unwrap_or_default().to_string(),
                        kind: t.to_string(),
                        node: r["node"].as_str().unwrap_or_default().to_string(),
                        status: r["status"].as_str().unwrap_or_default().to_string(),
                        cpu: v_f64(&r["cpu"]),
                        cpus: v_f64(&r["maxcpu"]),
                        mem: v_u64(&r["mem"]),
                        maxmem: v_u64(&r["maxmem"]),
                        disk: v_u64(&r["disk"]),
                        maxdisk: v_u64(&r["maxdisk"]),
                        netin: v_u64(&r["netin"]),
                        netout: v_u64(&r["netout"]),
                        uptime: v_u64(&r["uptime"]),
                        tags: r["tags"].as_str().map(|t| t.split([';', ',']).filter(|x| !x.is_empty()).map(str::to_string).collect()).unwrap_or_default(),
                        template: v_u64(&r["template"]) == 1,
                        is_self: Some(vmid) == configured_self,
                    });
                }
                Some("storage") if r["node"].as_str() == Some(node.as_str()) => {
                    ov.storages.push(Storage {
                        storage: r["storage"].as_str().unwrap_or_default().to_string(),
                        kind: r["plugintype"].as_str().unwrap_or_default().to_string(),
                        content: r["content"].as_str().map(|c| c.split(',').map(str::to_string).collect()).unwrap_or_default(),
                        used: v_u64(&r["disk"]),
                        total: v_u64(&r["maxdisk"]),
                        active: r["status"].as_str() == Some("available"),
                        shared: v_u64(&r["shared"]) == 1,
                    });
                }
                _ => {}
            }
        }
        ov.guests.sort_by_key(|g| g.vmid);
        ov.storages.sort_by(|a, b| a.storage.cmp(&b.storage));
        if configured_self.is_none() {
            if let Some(found) = detect_self(&ov.guests, &self.inner.docker.info().name) {
                for g in ov.guests.iter_mut() {
                    g.is_self = g.vmid == found;
                }
            }
        }
        let ov = Arc::new(ov);
        *self.inner.overview.write() = Some(ov.clone());
        Ok(ov)
    }

    pub fn cached_overview(&self) -> Option<Arc<Overview>> {
        self.inner.overview.read().clone()
    }

    async fn guest(&self, vmid: u32) -> Result<Guest> {
        let ov = self.overview(30).await?;
        ov.guests
            .iter()
            .find(|g| g.vmid == vmid)
            .cloned()
            .ok_or_else(|| AppError::not_found(format!("guest {vmid}")))
    }

    /// RRD data for the node (vmid None) or a guest.
    pub async fn rrd(&self, vmid: Option<u32>, timeframe: &str) -> Result<Vec<Value>> {
        let tf = match timeframe {
            "hour" | "day" | "week" | "month" | "year" => timeframe,
            _ => "hour",
        };
        let c = self.client()?;
        let path = match vmid {
            None => {
                let node = self.node(&c).await?;
                format!("/nodes/{node}/rrddata?timeframe={tf}&cf=AVERAGE")
            }
            Some(id) => {
                let g = self.guest(id).await?;
                format!("/nodes/{}/{}/{id}/rrddata?timeframe={tf}&cf=AVERAGE", g.node, g.kind)
            }
        };
        c.get(&path).await
    }

    pub async fn snapshots(&self, vmid: u32) -> Result<Vec<Snapshot>> {
        let g = self.guest(vmid).await?;
        let c = self.client()?;
        let mut list: Vec<Snapshot> = c.get(&format!("/nodes/{}/{}/{vmid}/snapshot", g.node, g.kind)).await?;
        list.retain(|s| s.name != "current");
        list.sort_by_key(|s| std::cmp::Reverse(s.snaptime.unwrap_or(0)));
        Ok(list)
    }

    pub async fn backups(&self, storage: &str) -> Result<Vec<BackupFile>> {
        let c = self.client()?;
        let node = self.node(&c).await?;
        let mut list: Vec<BackupFile> = c.get(&format!("/nodes/{node}/storage/{storage}/content?content=backup")).await?;
        list.sort_by_key(|b| std::cmp::Reverse(b.ctime.unwrap_or(0)));
        Ok(list)
    }

    pub async fn tasks(&self, limit: u32) -> Result<Vec<PveTask>> {
        let c = self.client()?;
        let node = self.node(&c).await?;
        c.get(&format!("/nodes/{node}/tasks?limit={}", limit.clamp(1, 500))).await
    }

    pub async fn task_log(&self, upid: &str) -> Result<Vec<String>> {
        let c = self.client()?;
        let node = node_from_upid(upid).ok_or_else(|| AppError::bad_request("invalid task id"))?;
        let lines: Vec<Value> = c
            .get(&format!("/nodes/{node}/tasks/{}/log?limit=1000", percent_encode(upid)))
            .await?;
        Ok(lines.iter().filter_map(|l| l["t"].as_str().map(str::to_string)).collect())
    }

    pub async fn apt_updates(&self) -> Result<Vec<AptUpdate>> {
        let c = self.client()?;
        let node = self.node(&c).await?;
        c.get(&format!("/nodes/{node}/apt/update")).await
    }

    pub async fn disks(&self) -> Result<Vec<Disk>> {
        let c = self.client()?;
        let node = self.node(&c).await?;
        c.get(&format!("/nodes/{node}/disks/list")).await
    }

    /// Checks connectivity and credentials with the given (unsaved) settings.
    pub async fn test(settings: &crate::settings::Proxmox) -> Result<Value> {
        let mut s = settings.clone();
        let mut fetched = None;
        if s.tls_mode == crate::settings::TlsMode::Fingerprint && s.fingerprint.trim().is_empty() {
            let fp = client::fetch_fingerprint(&s.url).await?;
            s.fingerprint = fp.clone();
            fetched = Some(fp);
        }
        let c = PveClient::new(&s)?;
        let version: Value = c.get("/version").await?;
        let nodes: Vec<Value> = c.get("/nodes").await?;
        Ok(json!({
            "version": version["version"],
            "release": version["release"],
            "nodes": nodes.iter().filter_map(|n| n["node"].as_str()).collect::<Vec<_>>(),
            "fingerprint": fetched,
        }))
    }
}

fn node_from_upid(upid: &str) -> Option<String> {
    // UPID:node:pid:pstart:starttime:type:id:user:
    upid.split(':').nth(1).map(str::to_string).filter(|n| !n.is_empty())
}

fn percent_encode(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestAction {
    Start,
    Shutdown,
    Stop,
    Reboot,
    Suspend,
    Resume,
}

impl GuestAction {
    fn path(self) -> &'static str {
        match self {
            GuestAction::Start => "start",
            GuestAction::Shutdown => "shutdown",
            GuestAction::Stop => "stop",
            GuestAction::Reboot => "reboot",
            GuestAction::Suspend => "suspend",
            GuestAction::Resume => "resume",
        }
    }

    fn disruptive(self) -> bool {
        !matches!(self, GuestAction::Start | GuestAction::Resume)
    }
}

/// Waits for a Proxmox task (UPID) and streams its log into the Berth task.
async fn follow_upid(app: &App, ctx: &TaskCtx, upid: &str) -> Result<()> {
    let c = app.proxmox.client()?;
    let node = node_from_upid(upid).ok_or_else(|| AppError::upstream("Proxmox returned an invalid task id"))?;
    let enc = percent_encode(upid);
    let mut shown = 0usize;
    loop {
        if ctx.is_cancelled() {
            return Err(AppError::conflict("stopped following the task (it keeps running on Proxmox)"));
        }
        let status: Value = c.get(&format!("/nodes/{node}/tasks/{enc}/status")).await?;
        if let Ok(lines) = c.get::<Vec<Value>>(&format!("/nodes/{node}/tasks/{enc}/log?start={shown}&limit=500")).await {
            for l in &lines {
                if let Some(t) = l["t"].as_str() {
                    if t != "no content" {
                        ctx.log(t.to_string());
                    }
                }
            }
            shown += lines.len();
        }
        if status["status"].as_str() == Some("stopped") {
            let exit = status["exitstatus"].as_str().unwrap_or("unknown");
            return if exit == "OK" {
                Ok(())
            } else {
                Err(AppError::upstream(format!("Proxmox task ended with: {exit}")))
            };
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// The guest whose name is the Docker host's name, when exactly one running
/// guest matches. In an LXC the host name is the guest's name in Proxmox.
pub fn detect_self(guests: &[Guest], host_name: &str) -> Option<u32> {
    let host = host_name.trim();
    if host.is_empty() {
        return None;
    }
    let mut matches = guests
        .iter()
        .filter(|g| !g.template && g.status == "running" && g.name.eq_ignore_ascii_case(host));
    match (matches.next(), matches.next()) {
        (Some(g), None) => Some(g.vmid),
        _ => None,
    }
}

fn guard_self(app: &App, vmid: u32, confirm_self: bool) -> Result<()> {
    if app.proxmox.self_vmid() == Some(vmid) && !confirm_self {
        return Err(AppError::conflict(
            "this guest runs Berth itself; confirm to continue (Berth will go offline)",
        ));
    }
    Ok(())
}

pub async fn guest_action(app: &App, vmid: u32, action: GuestAction, confirm_self: bool, actor: &Actor) -> Result<String> {
    if action.disruptive() {
        guard_self(app, vmid, confirm_self)?;
    }
    let g = app.proxmox.guest(vmid).await?;
    let spec = TaskSpec::new("proxmox", format!("{} {} {}", capitalize(action.path()), g.kind.to_uppercase(), g.name), actor).lock(format!("pve:{vmid}"));
    let app2 = app.clone();
    let actor2 = actor.clone();
    Ok(app.tasks.spawn(spec, move |ctx| async move {
        let c = app2.proxmox.client()?;
        let upid: String = c.post(&format!("/nodes/{}/{}/{vmid}/status/{}", g.node, g.kind, action.path()), &[]).await?;
        follow_upid(&app2, &ctx, &upid).await?;
        app2.note(NewActivity::new("proxmox.task", Severity::Info, format!("{} {} ({vmid})", capitalize(action.path()), g.name)).actor(&actor2)).await;
        let _ = app2.proxmox.overview(0).await;
        Ok(json!({ "upid": upid }))
    }))
}

pub async fn create_snapshot(app: &App, vmid: u32, name: &str, description: &str, include_ram: bool, actor: &Actor) -> Result<String> {
    let valid = !name.is_empty()
        && name.len() <= 40
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !valid {
        return Err(AppError::bad_request("snapshot names start with a letter and use letters, digits, '-' and '_' (max 40)"));
    }
    let g = app.proxmox.guest(vmid).await?;
    let spec = TaskSpec::new("proxmox", format!("Snapshot {} ({name})", g.name), actor).lock(format!("pve:{vmid}"));
    let (app2, name, description, actor2) = (app.clone(), name.to_string(), description.to_string(), actor.clone());
    Ok(app.tasks.spawn(spec, move |ctx| async move {
        let c = app2.proxmox.client()?;
        let mut form = vec![("snapname", name.clone()), ("description", description)];
        if include_ram && g.kind == "qemu" {
            form.push(("vmstate", "1".into()));
        }
        let upid: String = c.post(&format!("/nodes/{}/{}/{vmid}/snapshot", g.node, g.kind), &form).await?;
        follow_upid(&app2, &ctx, &upid).await?;
        app2.note(NewActivity::new("proxmox.task", Severity::Success, format!("Snapshot '{name}' of {} created", g.name)).actor(&actor2)).await;
        Ok(json!({ "upid": upid }))
    }))
}

pub async fn delete_snapshot(app: &App, vmid: u32, name: &str, actor: &Actor) -> Result<String> {
    let g = app.proxmox.guest(vmid).await?;
    let spec = TaskSpec::new("proxmox", format!("Delete snapshot {name} of {}", g.name), actor).lock(format!("pve:{vmid}"));
    let (app2, name) = (app.clone(), name.to_string());
    Ok(app.tasks.spawn(spec, move |ctx| async move {
        let c = app2.proxmox.client()?;
        let upid: String = c.delete(&format!("/nodes/{}/{}/{vmid}/snapshot/{}", g.node, g.kind, percent_encode(&name))).await?;
        follow_upid(&app2, &ctx, &upid).await?;
        Ok(json!({ "upid": upid }))
    }))
}

pub async fn rollback_snapshot(app: &App, vmid: u32, name: &str, confirm_self: bool, actor: &Actor) -> Result<String> {
    guard_self(app, vmid, confirm_self)?;
    let g = app.proxmox.guest(vmid).await?;
    let spec = TaskSpec::new("proxmox", format!("Roll back {} to {name}", g.name), actor).lock(format!("pve:{vmid}"));
    let (app2, name, actor2) = (app.clone(), name.to_string(), actor.clone());
    Ok(app.tasks.spawn(spec, move |ctx| async move {
        let c = app2.proxmox.client()?;
        let upid: String = c.post(&format!("/nodes/{}/{}/{vmid}/snapshot/{}/rollback", g.node, g.kind, percent_encode(&name)), &[]).await?;
        follow_upid(&app2, &ctx, &upid).await?;
        app2.note(NewActivity::new("proxmox.task", Severity::Warning, format!("{} rolled back to snapshot '{name}'", g.name)).actor(&actor2)).await;
        Ok(json!({ "upid": upid }))
    }))
}

#[derive(Debug, Clone, Deserialize)]
pub struct BackupRequest {
    pub storage: Option<String>,
    /// "snapshot", "suspend" or "stop".
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub protected: bool,
}

fn default_mode() -> String {
    "snapshot".into()
}

pub async fn backup_now(app: &App, vmid: u32, req: BackupRequest, actor: &Actor) -> Result<String> {
    if !matches!(req.mode.as_str(), "snapshot" | "suspend" | "stop") {
        return Err(AppError::bad_request("mode must be snapshot, suspend or stop"));
    }
    let g = app.proxmox.guest(vmid).await?;
    let storage = req
        .storage
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| Some(app.settings.get().proxmox.backup_storage.clone()).filter(|s| !s.is_empty()))
        .ok_or_else(|| AppError::bad_request("choose a backup storage (or set a default in Settings > Proxmox)"))?;
    let spec = TaskSpec::new("proxmox", format!("vzdump {} to {storage}", g.name), actor).lock(format!("pve:{vmid}"));
    let (app2, actor2) = (app.clone(), actor.clone());
    Ok(app.tasks.spawn(spec, move |ctx| async move {
        let c = app2.proxmox.client()?;
        let mut form = vec![
            ("vmid", vmid.to_string()),
            ("storage", storage.clone()),
            ("mode", req.mode.clone()),
            ("compress", "zstd".to_string()),
        ];
        let notes = if req.notes.is_empty() { "{{guestname}} (Berth)".to_string() } else { req.notes.clone() };
        form.push(("notes-template", notes));
        if req.protected {
            form.push(("protected", "1".into()));
        }
        let upid: String = c.post(&format!("/nodes/{}/vzdump", g.node), &form).await?;
        let result = follow_upid(&app2, &ctx, &upid).await;
        let (sev, msg) = match &result {
            Ok(()) => (Severity::Success, format!("Proxmox backup of {} to {storage} finished", g.name)),
            Err(e) => (Severity::Error, format!("Proxmox backup of {} failed: {e}", g.name)),
        };
        app2.note(NewActivity::new("proxmox.task", sev, msg.clone()).actor(&actor2)).await;
        app2.notifier.send(Notification::new("proxmox.task", sev, "Proxmox backup", msg));
        result.map(|_| json!({ "upid": upid }))
    }))
}

pub async fn refresh_apt(app: &App, actor: &Actor) -> Result<String> {
    let spec = TaskSpec::new("proxmox", "Refresh Proxmox package lists", actor).lock("pve:apt");
    let app2 = app.clone();
    Ok(app.tasks.spawn(spec, move |ctx| async move {
        let c = app2.proxmox.client()?;
        let node = app2.proxmox.node(&c).await?;
        let upid: String = c.post(&format!("/nodes/{node}/apt/update"), &[]).await?;
        follow_upid(&app2, &ctx, &upid).await?;
        let updates = app2.proxmox.apt_updates().await?;
        Ok(json!({ "updates": updates.len() }))
    }))
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Keeps the overview fresh for alerts and the dashboard.
pub fn start(app: App) {
    tokio::spawn(async move {
        loop {
            if app.proxmox.enabled() {
                if let Err(err) = app.proxmox.overview(0).await {
                    tracing::debug!(%err, "Proxmox poll failed");
                }
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

#[cfg(test)]
mod tests {

    fn guest(vmid: u32, name: &str, status: &str) -> Guest {
        Guest {
            vmid,
            name: name.into(),
            kind: "lxc".into(),
            node: "pve".into(),
            status: status.into(),
            cpu: 0.0,
            cpus: 1.0,
            mem: 0,
            maxmem: 0,
            disk: 0,
            maxdisk: 0,
            netin: 0,
            netout: 0,
            uptime: 0,
            tags: Vec::new(),
            template: false,
            is_self: false,
        }
    }

    #[test]
    fn detects_the_guest_by_host_name() {
        let guests = vec![guest(100, "docker", "running"), guest(101, "media", "running"), guest(102, "Docker", "stopped")];
        assert_eq!(detect_self(&guests, "docker"), Some(100));
        assert_eq!(detect_self(&guests, "DOCKER"), Some(100));
        assert_eq!(detect_self(&guests, ""), None);
        assert_eq!(detect_self(&guests, "nas"), None);
        // Ambiguous names are not guessed.
        let twins = vec![guest(100, "docker", "running"), guest(200, "docker", "running")];
        assert_eq!(detect_self(&twins, "docker"), None);
    }
    use super::*;

    #[test]
    fn upid_node() {
        assert_eq!(node_from_upid("UPID:pve:0000F00D:0001:6713:vzdump:100:berth@pve!berth:").as_deref(), Some("pve"));
        assert_eq!(node_from_upid("garbage"), None);
    }
}
