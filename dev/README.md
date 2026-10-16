# Development helpers

Small stand-ins for services Nodestarr talks to, so features can be worked
on without the real thing. Python 3.10 or newer, standard library only.

| Tool | What it is |
|---|---|
| `pve-mock.py` | A Proxmox VE API with one node (and a second cluster member with a guest of its own), a few LXCs, a VM and a template, storages, snapshots, vzdump backups, tasks with logs, disks, package updates and history data. HTTPS with a fresh self-signed certificate on port 8006 (needs `openssl`). |
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
- **Notifications**: a Webhook channel with
  `http://127.0.0.1:9099/anything`, or a Discord, Slack, ntfy or Gotify
  channel whose address points there, then Send a test.

For Home Assistant, any MQTT broker works, for example
`docker run --rm -p 1883:1883 eclipse-mosquitto:2 mosquitto -c /mosquitto-no-auth.conf`,
watched with `mosquitto_sub -t 'homeassistant/#' -t 'nodestarr/#' -v`.
