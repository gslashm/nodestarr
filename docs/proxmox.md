# Proxmox VE

When Docker runs inside a Proxmox LXC or VM, Nodestarr can also show the
Proxmox node and its guests: power actions, snapshots, vzdump backups,
tasks, storage and pending package updates, and the health of the nodes'
disks and pools ([Disk health and pools](disk-health.md)). It talks to the
Proxmox API with an API token.

## Create an API token

Run these on the Proxmox host as root. They create a user for Nodestarr, a
role with just the permissions it uses, and a token:

```sh
pveum user add nodestarr@pve --comment "Nodestarr"
pveum role add NodestarrOps --privs "VM.Audit VM.PowerMgmt VM.Snapshot VM.Snapshot.Rollback VM.Backup Datastore.Audit Datastore.AllocateSpace Sys.Audit Sys.Modify"
pveum acl modify / --users nodestarr@pve --roles NodestarrOps
pveum user token add nodestarr@pve nodestarr --privsep 0
```

The last command prints the token secret once; copy it.

What the permissions are for:

| Privilege | Used for |
|---|---|
| `Sys.Audit` | Node status, tasks and their logs, disks and their SMART data, ZFS and LVM pools, package updates, history charts |
| `Sys.Modify` | Refreshing the package lists (the same as Updates > Refresh in Proxmox) |
| `VM.Audit` | Listing guests, their status, history and snapshots |
| `VM.PowerMgmt` | Start, shut down, reboot, stop, suspend and resume |
| `VM.Snapshot`, `VM.Snapshot.Rollback` | Taking, deleting and rolling back snapshots |
| `VM.Backup`, `Datastore.AllocateSpace` | vzdump backups to a storage |
| `Datastore.Audit` | Storage usage and the backups on a storage |

For monitoring only, give the user the built-in `PVEAuditor` role instead;
the action buttons will then fail with a permission error.

`--privsep 0` makes the token carry the user's permissions. If you prefer
privilege separation (`--privsep 1`), grant the role to the token as well:
`pveum acl modify / --tokens 'nodestarr@pve!nodestarr' --roles NodestarrOps`.

## Connect

Settings > Proxmox:

1. Turn on **Connect to Proxmox VE**.
2. **Address**: the node's API address, for example
   `https://<proxmox_node>:8006`.
3. **API token ID**: `nodestarr@pve!nodestarr`, and the **token secret**.
4. **Certificate**: Proxmox uses a self-signed certificate, so leave **Pin
   its fingerprint** selected and click **Test connection**. Nodestarr reads
   the certificate's fingerprint and fills it in. Compare it with the one
   in Proxmox (Datacenter > your node > System > Certificates, or
   `pvenode cert info` on the host) and save. From then on Nodestarr only
   talks to a server presenting exactly that certificate.

   If Proxmox has a certificate from a trusted authority (for example
   through ACME), choose **Trusted certificate** instead. **Don't check**
   accepts any certificate and is only there for troubleshooting.
5. **Node**: filled in by the test when there is only one.
6. **Nodestarr's guest ID**: the VMID of the LXC or VM that runs Nodestarr.
   Leave it empty to use the guest whose name matches the Docker host's
   name. Stopping, shutting down, rebooting or rolling back that guest
   takes Nodestarr offline, so it asks you to type the guest's name first.
7. **Backup storage**: where "Back up now" sends vzdump backups by default,
   for example `backups`.

After a new Proxmox certificate (renewal, reinstall), the pinned fingerprint
no longer matches and the connection fails until you test and save again.

## What you get

**Proxmox** in the sidebar:

- **Guests**: CPU, memory and root disk of the node; how its CPU, memory,
  disk and network split across its guests, as donut charts (the busiest
  guests each get a slice, the rest share one; CPU and memory also show
  what the node uses itself and what is free, and in a cluster you can
  switch between nodes); every LXC and VM with its state, usage and
  uptime, and per guest: start, shut down, reboot,
  force stop, suspend and resume (VMs), snapshots, "Back up now" (vzdump
  with mode, notes and protection) and history charts.
- **Backups**: the vzdump backups on each storage, with size, notes and
  protection. Restoring stays in the Proxmox interface.
- **Tasks**: recent Proxmox tasks with their logs.
- **Disks**: every physical disk of the nodes with its SMART health over
  time, ZFS, mdadm, SnapRAID and LVM thin pools with the redundancy they
  have left, scrubs and rebuilds, and SMART self-tests, also on schedules.
  See [Disk health and pools](disk-health.md); self-tests, mdadm and
  SnapRAID need [host access](disk-health.md#setting-up-host-access) over
  SSH.
- **Node**: version, kernel, CPU, uptime, load, storage usage, disks with
  their health and wear, pending package updates, and history charts from
  an hour to a year.

Alerts can watch the node's memory and its storages, and the disks' health
and temperature and the pools (Settings > Monitoring and alerts). Finished
backup and snapshot tasks started from Nodestarr can notify you
(`proxmox.task`), and so can failed self-tests (`disk.test`).

Nodestarr reads Proxmox every 20 seconds while a Proxmox page is open, and
about once a minute otherwise for the alerts.
