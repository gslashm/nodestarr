# Monitoring, alerts and notifications

## Resource history

Nodestarr samples CPU, memory, network and disk activity of every running
container and of the Docker host once a minute, and every few seconds while
someone has the UI open. Raw samples are kept for 48 hours and 15-minute
averages for 30 days (Settings > Monitoring and alerts). Charts live on
Monitoring > Resources and on each container's Resources tab, from the
last 15 minutes up to 30 days.

Sampling is cheap: one Docker stats request per running container, no
agents, and no sampling at all while history and Home Assistant stats are
off and no one is looking.

## Alerts

Alert rules watch for trouble and notify you when it starts and when it
clears. The defaults:

| Rule | Fires when | Severity |
|---|---|---|
| Host memory | less than 10% of the Docker host's memory is available for 5 minutes | warning |
| Proxmox memory | the Proxmox node uses more than 90% of its memory for 5 minutes | warning |
| Proxmox storage | a Proxmox storage is more than 90% full | warning |
| Disk health | a disk of a Proxmox node needs attention or is failing, judged from its SMART data (temperature aside) | error |
| Disk temperature | a hard disk of a Proxmox node is hotter than 50 °C for 10 minutes (SSDs and NVMe drives: their own limit, or 20 °C more) | warning |
| Pool health | a ZFS pool, mdadm or SnapRAID array or LVM thin pool is degraded, faulted or has errors | error |
| Pool scrub age | a ZFS pool or SnapRAID array wasn't scrubbed for 35 days | info |
| Restart loop | a container restarted 3 times within 10 minutes | error |
| Unhealthy | a container's health check fails for 2 minutes | warning |
| Unexpected exit | a container exits with an error without being stopped | error |
| Out of memory | the kernel killed a container for using too much memory | error |
| Disk | a watched folder's file system is more than 90% full (`/config` and `/backups` by default) | warning |
| Container memory | a container uses more than 90% of its memory limit for 5 minutes | warning |
| Container CPU | a container uses more than 95% CPU for 15 minutes (off by default) | info |
| Backup age | a container with backups on has no backup newer than 48 hours | warning |

Change thresholds, durations and severities, or turn rules off, in
Settings > Monitoring and alerts. Mute a noisy container on its Settings
tab. Active and recent alerts are on Monitoring > Alerts, where you can
dismiss them.

## Uptime checks

Every container with a web UI gets an uptime check automatically: Nodestarr
requests its page every 60 seconds and counts it as down after 3 failures
in a row. Monitoring > Uptime shows the state, response times and a 30-day
history for each check.

- Any response below 500 counts as up, unless you list accepted statuses
  (like `200-399,401`) in Settings > Uptime or on a container.
- A check can require a keyword in the page.
- Add your own checks for anything else: `https://...` addresses, or
  `tcp://host:port` for services without a web page.

## Activity log

Activity lists everything that happened, newest first: updates, backups,
restores, crashes, alerts, sign-ins, settings changes and so on, with who
did it (a user, an API key, a schedule, Home Assistant or Nodestarr itself).
Filter by kind, severity and text. Long actions (updates, backups,
deploys) also have a task with a full log under Activity > Tasks. Activity
is kept for 90 days and task logs for 30.

## Notifications

Add channels in Settings > Notifications. Each one can be tested, limited to
certain events and to a minimum severity.

| Channel | You need |
|---|---|
| Discord | A webhook URL (Server settings > Integrations > Webhooks). Optional bot name and a mention for problems (`@here`, `<@&role id>`). |
| Slack | An incoming webhook URL. |
| Telegram | A bot token from @BotFather and the chat ID; optionally a topic ID. |
| ntfy | The server (`https://ntfy.sh` by default) and topic; optionally a token or username and password. |
| Gotify | The server and an app token. |
| Pushover | Your user key and an app token; optionally a device. |
| Email | SMTP server, port and security (STARTTLS, TLS or none), login, sender and recipients. |
| Webhook | Any URL, method and headers. The body is JSON (below) or your own template. |

Events you can pick from:

| Event | Sent when |
|---|---|
| `update.available` | New updates were found (one message per check run). |
| `update.applied` | An update was installed. |
| `update.failed` | An update failed. |
| `update.rolled_back` | An update failed and was rolled back. |
| `update.auto_summary` | Automatic updates ran (what was updated, deferred or failed). |
| `backup.completed`, `backup.failed` | A backup finished or failed (including the post-backup hook). |
| `restore.completed`, `restore.failed` | A restore finished or failed. |
| `container.crashed`, `container.oom`, `container.unhealthy`, `container.restart_loop` | Container problems. |
| `uptime.down`, `uptime.up` | A web UI stopped or started answering again. |
| `alert.fired`, `alert.resolved` | Resource alerts. |
| `app.health` | An app reports a new health problem (for example "Indexers unavailable" in Sonarr). |
| `proxmox.task` | A Proxmox task started from Nodestarr finished (backups, snapshots). |
| `disk.test` | A SMART self-test on a Proxmox disk failed or didn't finish (passed ones too, if turned on in Settings > Proxmox). |
| `security.login_failed` | Repeated failed sign-ins. |
| `system.update` | A new Nodestarr version is out. |

**Quiet hours** hold notifications at night and send them together when
quiet hours end; problems can be let through right away. **Repeats** of the
same message about the same container within 30 minutes are sent only once.

### Webhook payload

Without a template, a webhook receives:

```json
{
  "kind": "update.applied",
  "severity": "success",
  "title": "Sonarr updated",
  "message": "5.0.0.2569 → 5.0.0.2600",
  "fields": { "Image": "lscr.io/linuxserver/sonarr:develop", "Instance": "Nodestarr" },
  "target": "compose:media/sonarr",
  "target_name": "Sonarr",
  "url": "http://<server>:7979/containers/media-sonarr-1",
  "timestamp": 1792040000,
  "instance": "Nodestarr"
}
```

`url` is set when Settings > General > Nodestarr's address is filled in.
With a template, the same names are available in
[MiniJinja](https://docs.rs/minijinja) syntax, for example:

```json
{"text": "{{ title }}: {{ message }}"}
```

## Prometheus

Metrics are served at `/metrics` in the Prometheus text format. Scraping
needs an API key (`X-Api-Key` header, or `?apikey=` in the URL) unless
"Serve /metrics without a key" is on in Settings > Sign-in and security.

```yaml
scrape_configs:
  - job_name: nodestarr
    metrics_path: /metrics
    params:
      apikey: [nsr_...]
    static_configs:
      - targets: ["<server>:7979"]
```

| Metric | Labels | Meaning |
|---|---|---|
| `nodestarr_info` | `version` | Always 1. |
| `nodestarr_docker_connected` | | 1 when Docker is reachable. |
| `nodestarr_container_running` | `name`, `stack`, `image` | 1 when running. |
| `nodestarr_container_healthy` | `name`, `stack`, `image` | 1 healthy, 0 unhealthy (containers with a health check). |
| `nodestarr_container_restarts` | `name`, `stack`, `image` | Restart count. |
| `nodestarr_container_uptime_seconds` | `name`, `stack`, `image` | Seconds since start. |
| `nodestarr_container_update_available` | `name`, `stack`, `image` | 1 when an update is available. |
| `nodestarr_updates_available` | `type` | Available updates of each type: `major`, `minor`, `patch`, `rebuild`, `unknown`, and also `security` and `breaking` (an update counts once for its size and once for each of those). |
| `nodestarr_container_cpu_percent` | `name`, `stack`, `image` | CPU, 100 = one core. |
| `nodestarr_container_memory_bytes` | `name`, `stack`, `image` | Memory without page cache. |
| `nodestarr_container_network_receive_bytes_per_second` | `name`, `stack`, `image` | |
| `nodestarr_container_network_transmit_bytes_per_second` | `name`, `stack`, `image` | |
| `nodestarr_uptime_check_up` | `check`, `name` | 1 when answering. |
| `nodestarr_uptime_check_latency_milliseconds` | `check`, `name` | Last response time. |
| `nodestarr_backup_last_timestamp_seconds` | `identity` | Time of the newest backup. |
| `nodestarr_alerts_active` | | Firing alerts. |
| `nodestarr_host_memory_total_bytes`, `nodestarr_host_memory_available_bytes` | | Docker host memory. |
| `nodestarr_disk_health` | `node`, `device`, `model`, `serial` | A Proxmox disk's verdict: 0 healthy, 1 watch, 2 needs attention, 3 failing. |
| `nodestarr_disk_smart_passed` | `node`, `device`, `model`, `serial` | 1 when the disk's own SMART health check passes. |
| `nodestarr_disk_temperature_celsius` | `node`, `device`, `model`, `serial` | Temperature at the last reading. |
| `nodestarr_disk_reallocated_sectors`, `nodestarr_disk_pending_sectors` | `node`, `device`, `model`, `serial` | Reallocated sectors (grown defects on SAS) and unreadable sectors waiting to be reallocated. |
| `nodestarr_disk_crc_errors` | `node`, `device`, `model`, `serial` | Interface CRC errors, usually the cable. |
| `nodestarr_disk_power_on_hours` | `node`, `device`, `model`, `serial` | |
| `nodestarr_disk_wear_used_percent` | `node`, `device`, `model`, `serial` | Rated write endurance used (SSDs, NVMe). |
| `nodestarr_disk_asleep` | `node`, `device`, `model`, `serial` | 1 when the disk was spun down at the last check (with host access). |
| `nodestarr_pool_healthy` | `node`, `pool`, `type` | 1 when the pool works with all its redundancy. `type` is `zfs`, `md`, `snapraid` or `lvmthin`. |
| `nodestarr_pool_size_bytes`, `nodestarr_pool_used_bytes` | `node`, `pool`, `type` | |
| `nodestarr_pool_failures_tolerated` | `node`, `pool`, `type` | How many more disks can fail without losing data. |
| `nodestarr_pool_last_scrub_timestamp_seconds` | `node`, `pool`, `type` | When the last scrub finished (ZFS). |

For Home Assistant, see [Home Assistant](home-assistant.md).
