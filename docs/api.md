# API

Everything the web UI does goes through a JSON API under `/api/v1`, so
scripts, dashboards and other tools can do the same.

## Basics

**Authentication.** Create a key in Settings > API keys and send it with
every request:

```sh
curl -H "X-Api-Key: nsr_..." http://<server>:7979/api/v1/containers
```

`Authorization: Bearer nsr_...` works too, and so does `?apikey=nsr_...`
unless it is turned off. Read-only keys can call every `GET` endpoint;
changes need a full-access key. (The web UI uses a session cookie instead;
requests made with a cookie must also send `X-Nodestarr-Request: 1` to
change anything.)

**Containers** are addressed by name (`sonarr`), Compose identity
(`compose:media/sonarr`) or ID (at least 4 characters of it). URL-encode
identities: `compose%3Amedia%2Fsonarr`.

**Errors** come back with a matching status code and
`{"error": {"code": "not_found", "message": "container not found"}}`.
Codes: `bad_request`, `unauthorized`, `forbidden`, `not_found`, `conflict`,
`rate_limited`, `upstream_error` (an app, registry or Proxmox failed),
`docker_error`, `database_error`, `io_error` and `internal_error`.

**Tasks.** Anything that takes longer than a moment (updates, backups,
restores, deploys, recreates) starts a task and answers
`{"task_id": "..."}` right away. Follow it with `GET /tasks/{id}` (status
`queued`, `running`, `succeeded`, `failed` or `cancelled`, progress, the
last message, and the log) or the event stream. Tasks on the same
container or stack run one at a time.

## Endpoints

### Containers

| Method and path | What it does |
|---|---|
| `GET /containers` | All containers with state, version, update status and live usage. `?all=1` includes hidden ones. |
| `POST /containers` | Create a container: `{"spec": {...}, "start": true}` (the spec as returned by `GET /containers/{id}/spec`). |
| `POST /containers/parse-run` | Turn a `docker run` command into a spec: `{"command": "docker run ..."}`. |
| `POST /containers/to-compose` | Turn a spec into a compose service (YAML). |
| `GET /containers/{id}` | Details: mounts, networks, labels, environment (secrets masked), health, app status. |
| `POST /containers/{id}/actions/{action}` | `start`, `stop`, `restart`, `pause`, `unpause`, `kill` (`{"signal": "SIGTERM"}`), `recreate`, `remove` (`{"volumes": true}`). |
| `GET /containers/{id}/logs` | Recent logs: `?tail=500&since=<unix>&timestamps=1`, `&format=text` for plain text. |
| `POST /containers/{id}/exec` | Run a command and get its output: `{"command": "ls /config", "user": "abc"}`. |
| `GET /containers/{id}/stats` | Resource history: `?range=1h` (15m, 1h, 6h, 24h, 7d, 30d). |
| `GET`, `PUT /containers/{id}/prefs` | Container settings (update policy, backups, uptime check, ...). |
| `GET`, `PUT /containers/{id}/spec` | The full container definition (admin), and recreating it with changes (standalone containers). |
| `GET /containers/{id}/inspect` | Docker's raw inspect output (admin). |

### Updates

| Method and path | What it does |
|---|---|
| `GET /updates` | Update status of every container, the schedule and the last check. |
| `POST /updates/check` | Check every container now. |
| `POST /containers/{id}/check` | Check one container now. |
| `POST /containers/{id}/update` | Update one container: `{"tag": "develop", "skip_backup": false}` (both optional). |
| `POST /updates/apply` | Update several: `{"targets": ["sonarr", "radarr"], "skip_backup": false}`. |
| `POST /updates/auto/run` | Run the automatic updates now. |
| `GET /updates/history` | Past updates: `?identity=...&limit=100`. |
| `GET /containers/{id}/tags` | The repository's tags, newest first. |
| `GET /containers/{id}/tag-preview?tag=...` | The compose change switching to a tag would make. |
| `GET /containers/{id}/releases` | Release notes. |
| `POST /containers/{id}/rollback` | Roll back an update: `{"history_id": 12}` (the last one when left out). |
| `POST /containers/{id}/skip`, `PUT /containers/{id}/hold` | Skip the current build; hold or release automatic updates (`{"held": true}`). |

### Stacks

| Method and path | What it does |
|---|---|
| `GET /stacks` | All stacks with their services and states. |
| `POST /stacks` | Create one: `{"name": "x", "compose": "...", "env": "...", "deploy": true}`. |
| `GET /stacks/{name}` | Details and the parsed services. |
| `DELETE /stacks/{name}` | Take it down and remove it: `?remove_files=1&remove_volumes=1`. |
| `POST /stacks/{name}/actions/{action}` | `up`, `down`, `start`, `stop`, `restart`, `pull`, `update` (pull and recreate). Body: `{"services": ["sonarr"]}` to limit it. |
| `GET`, `PUT /stacks/{name}/files` | Read the compose files and `.env` (admin); save `{"files": [{"path": ..., "content": ...}], "deploy": true}`. |
| `POST /stacks/{name}/validate`, `POST /stacks/{name}/diff` | Check or compare edited files before saving. |
| `GET /stacks/{name}/history?path=...` | Saved versions of a file. |
| `POST /stacks/{name}/backup` | Back up every container of the stack. |

### Backups

| Method and path | What it does |
|---|---|
| `GET /backups` | Archives: `?identity=...&limit=...`. |
| `POST /backups/run` | Back up containers or a stack: `{"targets": ["sonarr"]}` or `{"stack": "media"}`. |
| `POST /backups/scheduled/run` | Run the scheduled backup now. |
| `POST /containers/{id}/backups` | Back up one container. |
| `GET /containers/{id}/backup-plan` | What would be backed up and how. |
| `GET /backups/{id}/contents`, `GET /backups/{id}/download` | Look inside or download an archive. |
| `POST /backups/{id}/restore` | `{"files": true, "database": true, "replace": false, "safety_backup": true}`. |
| `PUT /backups/{id}/pin`, `DELETE /backups/{id}` | Pin (`{"pinned": true}`) or delete. |

### Monitoring, uptime and activity

| Method and path | What it does |
|---|---|
| `GET /monitoring/overview` | Host, disks and per-container usage. |
| `GET /monitoring/history` | `?target=host` or a container identity, `&range=24h`. |
| `GET /monitoring/alerts` | Active and recent alerts; `POST /monitoring/alerts/{id}/dismiss`. |
| `GET /uptime`, `GET /uptime/{id}/beats` | Uptime checks and their history. `POST /uptime/run` checks now. |
| `GET /activity` | The activity log: `?kind=update&severity=error&search=...&target=<identity>&before=<id>&limit=50`. |
| `GET /tasks`, `GET /tasks/{id}`, `POST /tasks/{id}/cancel` | Tasks. |

### Proxmox

`GET /proxmox/status` (node, guests, storage), `POST
/proxmox/guests/{vmid}/actions/{action}` (`start`, `shutdown`, `reboot`,
`stop`, `suspend`, `resume`; for Nodestarr's own guest send
`{"confirm_self": true}`), `GET`/`POST /proxmox/guests/{vmid}/snapshots`,
`POST /proxmox/guests/{vmid}/backup`, `GET /proxmox/tasks`, `GET
/proxmox/rrd`, `GET /proxmox/disks`, `GET /proxmox/apt`.

### Nodestarr itself

`GET /health` (no key needed: `{"status": "ok", "docker": true, "version":
"0.1.0"}`), `GET /system/status`, `GET /settings` (secrets masked), `PUT
/settings/{section}`, `GET /settings/export`, `POST /settings/import`, `GET
/system/df`, `POST /system/prune`, `GET /images`, `GET /volumes`, `GET
/networks`, `GET /system/logs`.

## Live events

`GET /api/v1/events` is a server-sent event stream; each message is JSON
like `{"type": "container", "data": {...}}`. Types include `container`,
`container_removed`, `stacks_changed`, `task`, `task_log`, `activity`,
`update`, `update_check`, `uptime`, `app`, `alert`, `toast`, `settings`
and `prefs`. Add `?stats=1` to also receive live resource samples (`stats` and
`host`) every few seconds. A `resync` event means events were missed and
everything should be fetched again; the stream ends with `signed_out` when
the key or session is revoked.

```sh
curl -N -H "X-Api-Key: nsr_..." http://<server>:7979/api/v1/events
```

## WebSockets

- `GET /api/v1/containers/{id}/logs/stream?tail=500&timestamps=1` (and
  `/stacks/{name}/logs/stream`): follows logs. Messages are
  `{"lines": [{"ts": "2026-10-15T08:00:00.123Z", "s": "out", "t": "..."}]}`
  (`s` is `out`, `err` or `tty`; stack streams add `c`, the service),
  `{"end": true}` when the container stops, or `{"error": "..."}`.
- `GET /api/v1/containers/{id}/console?shell=/bin/bash&cols=120&rows=30`
  (admin): an interactive terminal. Send keystrokes as binary frames or
  `{"type": "input", "data": "..."}`, resize with
  `{"type": "resize", "cols": 120, "rows": 30}`; output comes back as
  binary frames, plus `{"type": "ready"}` and `{"type": "exit", "code": 0}`.

## Prometheus

`GET /metrics`; see [Monitoring](monitoring.md#prometheus).
