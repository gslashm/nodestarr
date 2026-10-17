# Development helpers

Small stand-ins for services Nodestarr talks to, so features can be worked
on without the real thing. Python 3.10 or newer, standard library only.

| Tool | What it is |
|---|---|
| `pve-mock.py` | A Proxmox VE API with one node (and a second cluster member with a guest of its own), a few LXCs, a VM and a template, storages, snapshots, vzdump backups, tasks with logs, disks with SMART data, ZFS and LVM pools, package updates and history data. HTTPS with a fresh self-signed certificate on port 8006 (needs `openssl`). |
| `host-helper.py` | A stand-in for the host helper that disk health runs on the nodes over SSH (`src/proxmox/disks/helper.sh`), answering for the mock's nodes. |
| `fakedisks.py` | The disks both of them show, so they tell the same story: a RAID-Z2 with a failing disk and a scrub running, a degraded mirror with a disk pulled, a SnapRAID array with a sleeping disk, an mdadm mirror rebuilding, NVMe system disks, a USB disk with a bogus serial number, and a second node. |
| `smoke-disks.sh` | Linux: builds Nodestarr and runs it against `pve-mock.py` and `host-helper.py` without Docker, reads the disks and pools and runs a self-test. |
| `webhook-sink.py` | Prints every request it receives, pretty-printing JSON. Port 9099. |

```sh
python3 dev/pve-mock.py          # https://127.0.0.1:8006
python3 dev/webhook-sink.py      # http://127.0.0.1:9099
```

Then point Nodestarr at them:

- **Proxmox**: Settings > Proxmox, address `https://127.0.0.1:8006`, token
  ID `nodestarr@pve!nodestarr`, secret
  `11111111-2222-3333-4444-555555555555`, and Test connection to pin the
  certificate. Power actions, snapshots and backups change the mock's
  state and create tasks that finish after a few seconds.
- **Disk health**: on by default once Proxmox is connected (Proxmox >
  Disks). To try host access without SSH, start Nodestarr with
  `NODESTARR_HOST_HELPER="python3 dev/host-helper.py"`: it then runs that
  command instead of `ssh` (with the node's name in `NODESTARR_NODE`). Turn
  on host access in Settings > Proxmox and test the nodes; no key is
  needed. Self-tests run in fast-forward (a short test takes a minute, a long
  one five); the long test of `sdc` fails at a bad block. Reading the
  sleeping disk through the API wakes it, like a real one, so after a few
  reads it's read only daily. The self-test state and start counts live in
  `nodestarr-host-helper-mock.json` in the temp folder; delete it to start
  over.
- **Notifications**: a Webhook channel with
  `http://127.0.0.1:9099/anything`, or a Discord, Slack, ntfy or Gotify
  channel whose address points there, then Send a test.

For Home Assistant, any MQTT broker works, for example
`docker run --rm -p 1883:1883 eclipse-mosquitto:2 mosquitto -c /mosquitto-no-auth.conf`,
watched with `mosquitto_sub -t 'homeassistant/#' -t 'nodestarr/#' -v`.
