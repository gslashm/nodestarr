//! Host access: runs the Nodestarr helper (`helper.sh`) on Proxmox nodes over
//! SSH, for what the Proxmox API can't do: SMART self-tests, complete
//! smartctl data that leaves sleeping disks asleep, mdadm and SnapRAID
//! arrays, and ZFS scrub control.
//!
//! Nodestarr keeps its own ed25519 key in `<data>/ssh/`. Nodes run the helper
//! as the key's forced command, so the key can't open a shell. Each node's
//! host key is pinned on the first connection (under the alias
//! `nodestarr-<node>`, so an address change doesn't matter) and checked on
//! every later one.
//!
//! For development, `NODESTARR_HOST_HELPER` names a command that stands in
//! for the SSH connection (for example `python3 dev/host-helper.py`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;

use crate::error::{AppError, Result};
use crate::settings::HostAccess;

/// The helper protocol this Nodestarr speaks.
pub const HELPER_VERSION: u32 = 1;
pub const HELPER_SCRIPT: &str = include_str!("helper.sh");
pub const HELPER_PATH: &str = "/usr/local/sbin/nodestarr-host";
const DEV_OVERRIDE: &str = "NODESTARR_HOST_HELPER";
/// SSH sessions open at once per node.
const SESSIONS_PER_NODE: usize = 3;

/// What Nodestarr knows about its access to one node.
#[derive(Debug, Clone, Default, Serialize)]
pub struct NodeAccess {
    pub node: String,
    pub address: String,
    /// The last connection worked.
    pub ok: bool,
    pub checked_at: Option<i64>,
    pub error: Option<String>,
    /// Helper protocol version, and whether it's older than this Nodestarr's.
    pub helper: Option<u32>,
    pub outdated: bool,
    pub hostname: Option<String>,
    pub smartctl: Option<String>,
    pub zfs: Option<String>,
    pub mdadm: bool,
    pub md_arrays: bool,
    pub snapraid: Option<String>,
    pub snapraid_conf: bool,
    /// SHA256 fingerprints of the pinned host key(s).
    pub host_keys: Vec<String>,
}

/// A finished helper call.
#[derive(Debug)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

impl Output {
    /// The helper's own complaint ("nodestarr-host: not a disk: …"), if any.
    pub fn message(&self) -> String {
        let line = self.stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
        line.strip_prefix("nodestarr-host: ").unwrap_or(line).to_string()
    }

    pub fn json(&self) -> Result<Value> {
        serde_json::from_str(self.stdout.trim())
            .map_err(|e| AppError::upstream(format!("the host helper sent something unexpected ({e})")))
    }
}

pub struct HostRunner {
    dir: PathBuf,
    nodes: Mutex<HashMap<String, NodeAccess>>,
    limits: Mutex<HashMap<String, Arc<Semaphore>>>,
}

impl HostRunner {
    pub fn new(data_dir: &Path) -> Self {
        Self { dir: data_dir.join("ssh"), nodes: Mutex::new(HashMap::new()), limits: Mutex::new(HashMap::new()) }
    }

    fn key_path(&self) -> PathBuf {
        self.dir.join("id_ed25519")
    }

    fn known_hosts(&self) -> PathBuf {
        self.dir.join("known_hosts")
    }

    fn dev_override() -> Option<Vec<String>> {
        let cmd = std::env::var(DEV_OVERRIDE).ok().filter(|c| !c.trim().is_empty())?;
        shell_words::split(&cmd).ok().filter(|v| !v.is_empty())
    }

    /// Whether helper calls can be made at all (a key exists, or the
    /// development stand-in is set).
    pub fn has_key(&self) -> bool {
        Self::dev_override().is_some() || self.key_path().exists()
    }

    /// Nodestarr's public key ("ssh-ed25519 AAAA… comment").
    pub fn public_key(&self) -> Option<String> {
        std::fs::read_to_string(self.key_path().with_extension("pub"))
            .ok()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
    }

    /// Creates (or replaces) Nodestarr's SSH key with `ssh-keygen`.
    pub async fn create_key(&self, comment: &str) -> Result<String> {
        std::fs::create_dir_all(&self.dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700));
        }
        let key = self.key_path();
        let _ = std::fs::remove_file(&key);
        let _ = std::fs::remove_file(key.with_extension("pub"));
        let out = tokio::process::Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-C", comment, "-f"])
            .arg(&key)
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => AppError::bad_request(
                    "ssh-keygen isn't installed where Nodestarr runs (it comes with the OpenSSH client)",
                ),
                _ => AppError::Io(e),
            })?;
        if !out.status.success() {
            return Err(AppError::upstream(format!(
                "ssh-keygen failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        self.public_key().ok_or_else(|| AppError::upstream("ssh-keygen didn't write a public key"))
    }

    /// Shell commands that install the helper and authorize the key on a node.
    pub fn install_commands(&self, user: &str) -> Option<String> {
        let key = self.public_key()?;
        let blob = key.split_whitespace().nth(1).unwrap_or(&key).to_string();
        let ssh_dir = if user == "root" { "/root/.ssh".to_string() } else { format!("/home/{user}/.ssh") };
        Some(format!(
            "cat > {HELPER_PATH} <<'NODESTARR_HELPER_EOF'\n{script}NODESTARR_HELPER_EOF\n\
             chmod 0755 {HELPER_PATH}\n\
             mkdir -p {ssh_dir} && touch {ssh_dir}/authorized_keys\n\
             grep -qF '{blob}' {ssh_dir}/authorized_keys || \
             echo 'restrict,command=\"{HELPER_PATH}\" {key}' >> {ssh_dir}/authorized_keys\n",
            script = HELPER_SCRIPT,
        ))
    }

    fn alias(node: &str) -> String {
        format!("nodestarr-{node}")
    }

    /// Fingerprints of the host key pinned for a node.
    pub fn host_keys(&self, node: &str) -> Vec<String> {
        let alias = Self::alias(node);
        let text = std::fs::read_to_string(self.known_hosts()).unwrap_or_default();
        known_host_fingerprints(&text, &alias)
    }

    /// Forgets a node's pinned host key (after the node was reinstalled).
    pub fn forget_host_key(&self, node: &str) -> Result<()> {
        let alias = Self::alias(node);
        let path = self.known_hosts();
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let kept: String = text.lines().filter(|l| !line_matches(l, &alias)).map(|l| format!("{l}\n")).collect();
        crate::util::fsx::atomic_write(&path, kept.as_bytes())?;
        if let Some(n) = self.nodes.lock().get_mut(node) {
            n.host_keys.clear();
        }
        Ok(())
    }

    pub fn status(&self) -> Vec<NodeAccess> {
        let mut v: Vec<NodeAccess> = self.nodes.lock().values().cloned().collect();
        v.sort_by(|a, b| a.node.cmp(&b.node));
        v
    }

    pub fn node(&self, node: &str) -> Option<NodeAccess> {
        self.nodes.lock().get(node).cloned()
    }

    /// The helper answered on this node at the last attempt, and speaks our protocol.
    pub fn usable(&self, node: &str) -> bool {
        self.nodes.lock().get(node).is_some_and(|n| n.ok && !n.outdated && n.helper.is_some())
    }

    fn record(&self, node: &str, address: &str, result: std::result::Result<(), String>) {
        let mut nodes = self.nodes.lock();
        let n = nodes
            .entry(node.to_string())
            .or_insert_with(|| NodeAccess { node: node.to_string(), ..Default::default() });
        n.address = address.to_string();
        n.checked_at = Some(crate::util::now());
        match result {
            Ok(()) => {
                n.ok = true;
                n.error = None;
            }
            Err(e) => {
                n.ok = false;
                n.error = Some(e);
            }
        }
    }

    fn limit(&self, node: &str) -> Arc<Semaphore> {
        self.limits
            .lock()
            .entry(node.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(SESSIONS_PER_NODE)))
            .clone()
    }

    /// Runs a helper command on a node. `pin` accepts and pins an unknown
    /// host key (only "Test connection" does). SSH and installation problems
    /// are errors; the helper's own failures come back as a non-zero code.
    pub async fn run(
        &self,
        settings: &HostAccess,
        node: &str,
        address: &str,
        args: &[String],
        timeout: Duration,
        pin: bool,
    ) -> Result<Output> {
        let _permit = self.limit(node).acquire_owned().await.map_err(|_| AppError::conflict("shutting down"))?;
        let mut cmd = match Self::dev_override() {
            Some(argv) => {
                let mut c = tokio::process::Command::new(&argv[0]);
                c.args(&argv[1..]).args(args).env("NODESTARR_NODE", node);
                c
            }
            None => {
                if !self.key_path().exists() {
                    return Err(AppError::bad_request("create Nodestarr's SSH key first (Settings > Proxmox)"));
                }
                let host = address.trim().trim_start_matches('[').trim_end_matches(']');
                if host.is_empty() {
                    return Err(AppError::bad_request(format!("no SSH address for node {node}")));
                }
                let mut c = tokio::process::Command::new("ssh");
                let opt = |k: &str, v: &str| ["-o".to_string(), format!("{k}={v}")];
                c.args(["-F", "/dev/null", "-T", "-i"]).arg(self.key_path());
                for (k, v) in [
                    ("BatchMode", "yes"),
                    ("IdentitiesOnly", "yes"),
                    ("PasswordAuthentication", "no"),
                    ("KbdInteractiveAuthentication", "no"),
                    ("StrictHostKeyChecking", if pin { "accept-new" } else { "yes" }),
                    ("HostKeyAlias", &Self::alias(node)),
                    ("HashKnownHosts", "no"),
                    ("UpdateHostKeys", "no"),
                    ("GlobalKnownHostsFile", "/dev/null"),
                    ("ConnectTimeout", "10"),
                    ("ServerAliveInterval", "15"),
                    ("ServerAliveCountMax", "3"),
                    ("LogLevel", "ERROR"),
                ] {
                    c.args(opt(k, v));
                }
                c.args(opt("UserKnownHostsFile", &self.known_hosts().to_string_lossy()));
                c.arg("-p").arg(settings.port.to_string());
                c.arg(format!("{}@{host}", settings.user));
                c.arg("--").args(args);
                c
            }
        };
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        let child = cmd.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => AppError::bad_request(
                "the OpenSSH client (ssh) isn't installed where Nodestarr runs; it's needed for host access",
            ),
            _ => AppError::Io(e),
        })?;
        let out = match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(r) => r?,
            Err(_) => {
                let msg = format!(
                    "{} on {node} didn't finish within {}s",
                    args.first().map(String::as_str).unwrap_or("helper"),
                    timeout.as_secs()
                );
                self.record(node, address, Err(msg.clone()));
                return Err(AppError::upstream(msg));
            }
        };
        let output = Output {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            code: out.status.code().unwrap_or(-1),
        };
        if let Some(problem) = classify(&output, address, settings.port) {
            self.record(node, address, Err(problem.clone()));
            return Err(AppError::upstream(problem));
        }
        self.record(node, address, Ok(()));
        if pin && let Some(n) = self.nodes.lock().get_mut(node) {
            n.host_keys = known_host_fingerprints(
                &std::fs::read_to_string(self.known_hosts()).unwrap_or_default(),
                &Self::alias(node),
            );
        }
        Ok(output)
    }

    /// Asks a node's helper what it can do, and remembers the answer.
    pub async fn info(&self, settings: &HostAccess, node: &str, address: &str, pin: bool) -> Result<NodeAccess> {
        let out = self.run(settings, node, address, &["info".to_string()], Duration::from_secs(25), pin).await?;
        let v = out.json()?;
        let version = v["helper"].as_u64().map(|n| n as u32);
        let s = |k: &str| v[k].as_str().map(str::to_string).filter(|x| !x.is_empty());
        let host_keys = self.host_keys(node);
        let mut nodes = self.nodes.lock();
        let n = nodes
            .entry(node.to_string())
            .or_insert_with(|| NodeAccess { node: node.to_string(), ..Default::default() });
        n.helper = version;
        n.outdated = version.is_none_or(|v| v < HELPER_VERSION);
        n.hostname = s("hostname");
        n.smartctl = s("smartctl");
        n.zfs = s("zfs");
        n.mdadm = v["mdadm"].as_bool().unwrap_or(false);
        n.md_arrays = v["md_arrays"].as_bool().unwrap_or(false);
        n.snapraid = s("snapraid");
        n.snapraid_conf = v["snapraid_conf"].as_bool().unwrap_or(false);
        n.host_keys = host_keys;
        if n.outdated {
            n.error = Some(format!(
                "The helper on {node} is older than this Nodestarr (protocol {} < {HELPER_VERSION}); reinstall it",
                version.unwrap_or(0)
            ));
        }
        if n.smartctl.is_none() {
            n.error = Some(format!("smartctl isn't installed on {node} (apt install smartmontools)"));
        }
        Ok(n.clone())
    }
}

/// Turns SSH and installation failures into something a person can act on.
fn classify(out: &Output, address: &str, port: u16) -> Option<String> {
    let err = out.stderr.as_str();
    let first = err.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
    match out.code {
        255 => Some(
            if err.contains("REMOTE HOST IDENTIFICATION HAS CHANGED") || err.contains("Host key verification failed") {
                "The node's SSH host key doesn't match the one Nodestarr pinned. If the node was reinstalled, forget the old \
             key in Settings > Proxmox and test the connection again."
                .to_string()
            } else if err.contains("No user exists for uid") {
                "The SSH client can't run as Nodestarr's user: its user ID has no entry in /etc/passwd where Nodestarr \
                 runs. Run the container as root (the default)."
                    .into()
            } else if err.contains("Permission denied") {
                "The node didn't accept Nodestarr's key. Run the setup commands from Settings > Proxmox on the node."
                    .into()
            } else if err.contains("Connection refused") {
                format!("SSH refused the connection on {address}:{port}.")
            } else if err.contains("Could not resolve hostname") {
                format!("Can't resolve {address}.")
            } else if err.contains("timed out") || err.contains("No route to host") || err.contains("unreachable") {
                format!("Can't reach {address}:{port} ({first}).")
            } else if first.is_empty() {
                "SSH failed without saying why.".into()
            } else {
                format!("SSH failed: {first}")
            },
        ),
        // The forced command itself is missing.
        126 | 127 if err.contains(HELPER_PATH) || err.contains("not found") || err.contains("No such file") => {
            Some(format!(
                "The helper isn't installed on this node ({HELPER_PATH} is missing). Run the setup commands from \
             Settings > Proxmox on it."
            ))
        }
        _ => None,
    }
}

fn line_matches(line: &str, alias: &str) -> bool {
    line.split_whitespace().next().is_some_and(|hosts| {
        hosts
            .split(',')
            .any(|h| h == alias || h.strip_prefix('[').and_then(|r| r.split_once(']')).is_some_and(|(n, _)| n == alias))
    })
}

/// SHA256 fingerprints (as `ssh-keygen -l` prints them) of an alias's keys.
fn known_host_fingerprints(text: &str, alias: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !l.starts_with('#') && line_matches(l, alias))
        .filter_map(|l| {
            let mut parts = l.split_whitespace().skip(1);
            let kind = parts.next()?;
            let blob = base64::engine::general_purpose::STANDARD.decode(parts.next()?).ok()?;
            let digest = Sha256::digest(&blob);
            Some(format!(
                "{} SHA256:{}",
                kind.trim_start_matches("ssh-").to_ascii_uppercase(),
                base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest)
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_of_pinned_keys() {
        let text = "# comment\nnodestarr-pve ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl\n\
                    nodestarr-pve2 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl\n";
        let fp = known_host_fingerprints(text, "nodestarr-pve");
        assert_eq!(fp.len(), 1);
        assert!(fp[0].starts_with("ED25519 SHA256:"), "{fp:?}");
        assert!(line_matches("[nodestarr-pve]:2222 ssh-ed25519 AAAA", "nodestarr-pve"));
        assert!(!line_matches("nodestarr-pve2 ssh-ed25519 AAAA", "nodestarr-pve"));
    }

    #[test]
    fn explains_ssh_failures() {
        let out = |code: i32, stderr: &str| Output { stdout: String::new(), stderr: stderr.into(), code };
        let msg = |o: Output| classify(&o, "10.0.0.2", 22).unwrap_or_default();
        assert!(msg(out(255, "root@10.0.0.2: Permission denied (publickey).")).contains("didn't accept"));
        assert!(msg(out(255, "Host key verification failed.")).contains("pinned"));
        assert!(msg(out(255, "ssh: connect to host 10.0.0.2 port 22: Connection refused")).contains("refused"));
        assert!(msg(out(127, "sh: 1: /usr/local/sbin/nodestarr-host: not found")).contains("isn't installed"));
        assert!(classify(&out(65, "nodestarr-host: not a disk: /dev/x"), "a", 22).is_none());
        assert_eq!(out(65, "nodestarr-host: not a disk: /dev/x").message(), "not a disk: /dev/x");
    }

    #[test]
    fn install_commands_embed_the_script() {
        let dir = tempfile::tempdir().unwrap();
        let r = HostRunner::new(dir.path());
        assert!(r.install_commands("root").is_none());
        std::fs::create_dir_all(dir.path().join("ssh")).unwrap();
        std::fs::write(dir.path().join("ssh/id_ed25519.pub"), "ssh-ed25519 AAAAKEY nodestarr@test\n").unwrap();
        let cmds = r.install_commands("root").unwrap();
        assert!(cmds.contains("NODESTARR_HELPER_EOF\nchmod 0755 /usr/local/sbin/nodestarr-host"));
        assert!(cmds.contains("grep -qF 'AAAAKEY' /root/.ssh/authorized_keys"));
        assert!(
            cmds.contains("restrict,command=\"/usr/local/sbin/nodestarr-host\" ssh-ed25519 AAAAKEY nodestarr@test")
        );
        assert!(!HELPER_SCRIPT.contains("NODESTARR_HELPER_EOF"));
    }
}
