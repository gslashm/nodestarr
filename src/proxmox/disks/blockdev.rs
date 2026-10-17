//! Block devices on a node: which partition belongs to which disk, what is
//! mounted where, and the `/dev/disk/by-*` names pools refer to. Maps pool
//! members (by-id paths, partitions, mount points) to physical disks.
//!
//! The host helper provides the full picture (lsblk, udev links, mounts).
//! Without it, by-id names and kernel names still resolve through the disk
//! identities the Proxmox API reports (serial, WWN, by-id link).

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::Value;

/// A physical disk, as far as mapping is concerned.
#[derive(Debug, Clone, Default)]
pub struct DiskRef {
    pub id: String,
    /// "/dev/sda".
    pub devpath: String,
    pub serial: Option<String>,
    pub wwn: Option<String>,
    /// "/dev/disk/by-id/ata-…", as the Proxmox API reports it.
    pub by_id: Option<String>,
}

/// A mounted filesystem.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Mount {
    pub source: String,
    pub target: String,
    pub fstype: String,
    pub size: Option<u64>,
    pub used: Option<u64>,
    pub avail: Option<u64>,
}

/// What the host helper reports about a node's block devices.
#[derive(Debug, Clone, Default)]
pub struct Topology {
    /// Kernel name → parent kernel name ("sda1" → "sda", "dm-0" → "sda3").
    parent: HashMap<String, String>,
    /// Kernel names of whole disks.
    disks: HashSet<String>,
    /// `/dev/disk/by-*` and `/dev/mapper` link names → kernel name.
    links: HashMap<String, String>,
    /// Partition and filesystem UUIDs → kernel name.
    uuids: HashMap<String, String>,
    pub mounts: Vec<Mount>,
}

fn num_u64(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn s(v: &Value) -> Option<&str> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty())
}

fn basename(path: &str) -> &str {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path)
}

impl Topology {
    /// Builds the topology from the helper's `lsblk -J -l` output, its link
    /// map ({"ata-…": "/dev/sda", …}) and `findmnt -J -b` output.
    pub fn from_helper(lsblk: &Value, links: &Value, mounts: &Value) -> Self {
        let mut t = Topology::default();
        let mut stack: Vec<&Value> = lsblk["blockdevices"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
        while let Some(dev) = stack.pop() {
            if let Some(children) = dev["children"].as_array() {
                stack.extend(children.iter());
            }
            let Some(kname) = s(&dev["kname"]).or_else(|| s(&dev["name"])) else { continue };
            let kname = basename(kname).to_string();
            match s(&dev["type"]) {
                Some("disk") => {
                    t.disks.insert(kname.clone());
                }
                Some("part") | Some("lvm") | Some("crypt") | Some("dm") | None => {}
                // md arrays, loop devices …: neither disks nor partitions.
                Some(_) => {}
            }
            if let Some(p) = s(&dev["pkname"]) {
                t.parent.entry(kname.clone()).or_insert_with(|| basename(p).to_string());
            }
            for key in ["partuuid", "uuid"] {
                if let Some(u) = s(&dev[key]) {
                    t.uuids.entry(u.to_ascii_lowercase()).or_insert_with(|| kname.clone());
                }
            }
        }
        if let Some(map) = links.as_object() {
            for (name, target) in map {
                if let Some(target) = target.as_str() {
                    t.links.insert(name.clone(), basename(target).to_string());
                }
            }
        }
        if let Some(list) = mounts["filesystems"].as_array() {
            t.mounts = list
                .iter()
                .filter_map(|m| {
                    // btrfs subvolumes show as "/dev/sdb1[/@data]".
                    let source = s(&m["source"])?.split('[').next().unwrap_or("").to_string();
                    Some(Mount {
                        source,
                        target: s(&m["target"])?.to_string(),
                        fstype: s(&m["fstype"]).unwrap_or("").to_string(),
                        size: num_u64(&m["size"]),
                        used: num_u64(&m["used"]),
                        avail: num_u64(&m["avail"]),
                    })
                })
                .collect();
        }
        t
    }

    /// The whole disk a block device (kernel name) lives on.
    pub fn disk_of(&self, kname: &str) -> Option<String> {
        let mut cur = kname.to_string();
        for _ in 0..8 {
            if self.disks.contains(&cur) {
                return Some(cur);
            }
            cur = self.parent.get(&cur)?.clone();
        }
        None
    }

    /// The kernel name of the disk behind a path or name a pool uses:
    /// "/dev/disk/by-id/…-part1", "/dev/disk/by-partuuid/…", "/dev/sdb1",
    /// "/dev/mapper/vg-lv", "sdb1".
    pub fn resolve(&self, name: &str) -> Option<String> {
        let base = basename(name);
        let kname = if name.starts_with("/dev/disk/") || name.starts_with("/dev/mapper/") {
            self.links.get(base).cloned().or_else(|| self.uuids.get(&base.to_ascii_lowercase()).cloned())
        } else if let Some(u) = name.strip_prefix("UUID=").or_else(|| name.strip_prefix("PARTUUID=")) {
            self.uuids.get(&u.to_ascii_lowercase()).cloned()
        } else {
            Some(base.to_string())
        }?;
        self.disk_of(&kname)
    }

    /// The filesystem a path lives on (the longest mount point containing it).
    pub fn mount_for(&self, path: &str) -> Option<&Mount> {
        let path = path.trim_end_matches('/');
        self.mounts
            .iter()
            .filter(|m| {
                let t = m.target.trim_end_matches('/');
                t.is_empty() || path == t || path.starts_with(&format!("{t}/"))
            })
            .max_by_key(|m| m.target.len())
    }
}

/// "sda1" → "sda", "nvme0n1p2" → "nvme0n1", "mmcblk0p1" → "mmcblk0".
/// Names that aren't partitions come back unchanged.
pub fn strip_partition(kname: &str) -> &str {
    let base = kname.trim_end_matches(|c: char| c.is_ascii_digit());
    if base.len() == kname.len() {
        return kname;
    }
    if let Some(stem) = base.strip_suffix('p')
        && stem.ends_with(|c: char| c.is_ascii_digit())
        && (stem.starts_with("nvme") || stem.starts_with("mmcblk") || stem.starts_with("loop"))
    {
        return stem;
    }
    if ["sd", "vd", "hd", "xvd"].iter().any(|p| base.starts_with(p)) && base.len() > 2 {
        return base;
    }
    kname
}

/// "ata-WDC_…-part3" → "ata-WDC_…".
fn strip_part_suffix(name: &str) -> &str {
    if let Some(idx) = name.rfind("-part")
        && name[idx + 5..].chars().all(|c| c.is_ascii_digit())
        && idx + 5 < name.len()
    {
        return &name[..idx];
    }
    name
}

fn norm_wwn(w: &str) -> String {
    w.trim().to_ascii_lowercase().trim_start_matches("0x").to_string()
}

/// The disk (index into `disks`) behind a pool member's name.
pub fn find_disk(name: &str, topo: Option<&Topology>, disks: &[DiskRef]) -> Option<usize> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let by_kname = |k: &str| disks.iter().position(|d| basename(&d.devpath) == k);
    if let Some(t) = topo
        && let Some(k) = t.resolve(name)
        && let Some(i) = by_kname(&k)
    {
        return Some(i);
    }
    let base = basename(name);
    let id = strip_part_suffix(base);
    if let Some(i) = disks.iter().position(|d| d.by_id.as_deref().map(basename) == Some(id)) {
        return Some(i);
    }
    if let Some(w) = id.strip_prefix("wwn-")
        && let Some(i) = disks.iter().position(|d| d.wwn.as_deref().is_some_and(|x| norm_wwn(x) == norm_wwn(w)))
    {
        return Some(i);
    }
    if let Some(eui) = id.strip_prefix("nvme-").filter(|e| e.starts_with("eui.") || e.starts_with("nvme."))
        && let Some(i) = disks.iter().position(|d| d.wwn.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(eui)))
    {
        return Some(i);
    }
    // by-id names end in "_<serial>" ("ata-<model>_<serial>").
    if id.contains('_')
        && let Some(i) = disks.iter().position(|d| {
            d.serial
                .as_deref()
                .map(str::trim)
                .is_some_and(|s| s.len() >= 4 && id.ends_with(&format!("_{}", s.replace(' ', "_"))))
        })
    {
        return Some(i);
    }
    by_kname(strip_partition(base))
}

/// A ZFS member that's gone shows up as its GUID with a note like
/// "was /dev/disk/by-id/…-part1": the path it had, if any.
pub fn was_path(note: &str) -> Option<&str> {
    note.split("was ").nth(1).map(|p| p.split_whitespace().next().unwrap_or(p)).filter(|p| p.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn disks() -> Vec<DiskRef> {
        vec![
            DiskRef {
                id: "WDC_A".into(),
                devpath: "/dev/sda".into(),
                serial: Some("WD-WCC7K0000001".into()),
                wwn: Some("0x50014ee2b3c4d5e6".into()),
                by_id: Some("/dev/disk/by-id/ata-WDC_WD40EFRX-68N32N0_WD-WCC7K0000001".into()),
            },
            DiskRef {
                id: "WDC_B".into(),
                devpath: "/dev/sdb".into(),
                serial: Some("WD-WCC7K0000002".into()),
                wwn: Some("0x50014ee2b3c4d5e7".into()),
                by_id: None,
            },
            DiskRef {
                id: "NVME".into(),
                devpath: "/dev/nvme0n1".into(),
                serial: Some("S4EWNX0R123456".into()),
                wwn: Some("eui.0025385b71b07e2f".into()),
                by_id: None,
            },
        ]
    }

    #[test]
    fn partitions() {
        assert_eq!(strip_partition("sda1"), "sda");
        assert_eq!(strip_partition("sdab12"), "sdab");
        assert_eq!(strip_partition("nvme0n1p2"), "nvme0n1");
        assert_eq!(strip_partition("nvme0n1"), "nvme0n1");
        assert_eq!(strip_partition("mmcblk0p1"), "mmcblk0");
        assert_eq!(strip_partition("md0"), "md0");
        assert_eq!(strip_partition("dm-3"), "dm-3");
        assert_eq!(strip_partition("sda"), "sda");
        assert_eq!(strip_part_suffix("ata-X_Y-part3"), "ata-X_Y");
        assert_eq!(strip_part_suffix("ata-X_Y"), "ata-X_Y");
        assert_eq!(strip_part_suffix("ata-X-party"), "ata-X-party");
    }

    #[test]
    fn maps_without_helper() {
        let d = disks();
        let find = |n: &str| find_disk(n, None, &d).map(|i| d[i].id.as_str());
        assert_eq!(find("/dev/disk/by-id/ata-WDC_WD40EFRX-68N32N0_WD-WCC7K0000001-part1"), Some("WDC_A"));
        // No by-id link known: the serial at the end of the name decides.
        assert_eq!(find("/dev/disk/by-id/ata-WDC_WD40EFRX-68N32N0_WD-WCC7K0000002-part9"), Some("WDC_B"));
        assert_eq!(find("/dev/disk/by-id/wwn-0x50014ee2b3c4d5e7-part1"), Some("WDC_B"));
        assert_eq!(find("/dev/disk/by-id/nvme-eui.0025385b71b07e2f-part3"), Some("NVME"));
        assert_eq!(find("/dev/disk/by-id/nvme-Samsung_SSD_970_EVO_Plus_1TB_S4EWNX0R123456-part1"), Some("NVME"));
        assert_eq!(find("/dev/sdb1"), Some("WDC_B"));
        assert_eq!(find("nvme0n1p2"), Some("NVME"));
        assert_eq!(find("/dev/disk/by-partuuid/0b1d2c3e-aaaa-bbbb-cccc-1234567890ab"), None);
        assert_eq!(find(""), None);
    }

    #[test]
    fn maps_with_helper() {
        let lsblk = json!({"blockdevices": [
            {"name": "sda", "kname": "sda", "pkname": null, "type": "disk"},
            {"name": "sda1", "kname": "sda1", "pkname": "sda", "type": "part", "partuuid": "0B1D2C3E-AAAA-BBBB-CCCC-1234567890AB"},
            {"name": "sdb", "kname": "sdb", "pkname": null, "type": "disk"},
            {"name": "sdb1", "kname": "sdb1", "pkname": "sdb", "type": "part", "uuid": "e7c2-11"},
            {"name": "pve-root", "kname": "dm-0", "pkname": "sdb1", "type": "lvm"}
        ]});
        let links = json!({"pve-root": "/dev/dm-0", "ata-Odd_Name_WITHOUT_SERIAL-part1": "/dev/sda1"});
        let mounts = json!({"filesystems": [
            {"source": "/dev/mapper/pve-root", "target": "/", "fstype": "ext4", "size": 100, "used": 50, "avail": 50},
            {"source": "/dev/sda1[/@data]", "target": "/mnt/disk1", "fstype": "btrfs", "size": "4000", "used": "1000", "avail": "3000"}
        ]});
        let t = Topology::from_helper(&lsblk, &links, &mounts);
        let d = disks();
        let find = |n: &str| find_disk(n, Some(&t), &d).map(|i| d[i].id.as_str());
        assert_eq!(find("/dev/disk/by-partuuid/0b1d2c3e-aaaa-bbbb-cccc-1234567890ab"), Some("WDC_A"));
        assert_eq!(find("/dev/disk/by-id/ata-Odd_Name_WITHOUT_SERIAL-part1"), Some("WDC_A"));
        assert_eq!(find("/dev/mapper/pve-root"), Some("WDC_B"));
        assert_eq!(t.resolve("UUID=E7C2-11").as_deref(), Some("sdb"));
        let m = t.mount_for("/mnt/disk1/").unwrap();
        assert_eq!((m.source.as_str(), m.used), ("/dev/sda1", Some(1000)));
        assert_eq!(t.mount_for("/mnt/disk1/sub/dir").unwrap().target, "/mnt/disk1");
        assert_eq!(t.mount_for("/mnt/disk10").unwrap().target, "/");
        assert_eq!(t.resolve(&t.mount_for("/srv").unwrap().source).as_deref(), Some("sdb"));
    }

    #[test]
    fn was_paths() {
        assert_eq!(was_path("was /dev/disk/by-id/ata-X-part1"), Some("/dev/disk/by-id/ata-X-part1"));
        assert_eq!(was_path("(resilvering)"), None);
    }
}
