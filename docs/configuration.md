# Configuration

Almost everything is set in the web UI (Settings, and Settings on each
container) and stored in Nodestarr's database. A handful of values that are
needed before the database opens come from environment variables or
command line flags.

## Environment variables

| Variable | Flag | Default | Meaning |
|---|---|---|---|
| `NODESTARR_DATA_DIR` | `--data-dir` | `/config` | Folder for the database. |
| `NODESTARR_LISTEN` | `--listen` | `0.0.0.0:7979` | Address and port of the web UI and API. |
| `DOCKER_HOST` | `--docker-host` | `unix:///var/run/docker.sock` | How to reach Docker, for example `tcp://docker-proxy:2375`. |
| `NODESTARR_LOG` | `--log` | `info` | Log filter, for example `debug` or `nodestarr=debug,info`. |
| `NODESTARR_WORKERS` | `--workers` | `2` | Worker threads. Two is plenty; Nodestarr is mostly idle. |
| `NODESTARR_ADMIN_USER` | | | Creates this admin on the first start instead of showing the setup page. |
| `NODESTARR_ADMIN_PASSWORD` | | | Password for `NODESTARR_ADMIN_USER` (at least 8 characters). |
| `NODESTARR_URL_BASE` | | | Overrides the URL path setting, for when a wrong value locked you out. |
| `TZ` | | `UTC` | Time zone for schedules and times, unless one is set in Settings > General. |

The admin variables are only read while there are no users.

## Data folder

Everything Nodestarr knows lives in one SQLite database,
`nodestarr.db` in the data folder (with its `-wal` and `-shm` companions):
settings, users and sessions, API keys, container settings, update and
backup history, the activity log, task logs, metrics, uptime results and
saved versions of edited files. It is only readable by its owner because it
holds password hashes and API keys.

Each scheduled backup run saves a consistent copy of it in the backup
folder (see [Backups](backups.md#nodestarrs-own-database)).

## Settings

Settings are grouped like the Settings page. Every field has a short
explanation in the UI; this is the overview.

**General**: the instance name (shown in the sidebar, browser tab and
notifications), the host address used for links to container web UIs, the
address of Nodestarr itself for links in notifications, the URL path when
served under a path, the time zone, whether to tell you about new Nodestarr
versions, and the look: theme, accent color, density, 12 or 24 hour times,
grouping by stack, and an optional icon URL template (see below).

**Sign-in and security**: sign-in through Nodestarr's form or an
authenticating reverse proxy, trusted proxies, the local network bypass,
session lengths (24 hours, or 30 days with "keep me signed in"), API keys in
URLs, and public access to `/metrics`. See [Security](security.md).

**Docker**: folders that hold stacks (default `/opt/stacks`; one sub-folder
per stack), the Compose command (detected automatically), the stop timeout,
containers to hide by name pattern, the shells tried for the console, and
how often Nodestarr resyncs with Docker as a safety net (5 minutes).

**Updates**: when to check (every 6 hours by default), the default policy
for new containers, when automatic updates run (04:00), how long an update
must be out before it is installed automatically (6 hours), which newer
version tags to report, verification and rollback, backups before updates,
how many old images to keep for rollbacks (2), the Plex guard, registry
credentials and an optional GitHub token for release notes. See
[Updates](updates.md).

**Backups**: the backup folder, the schedule (03:30, off until you turn it
on), whether containers are included unless they opt out, retention,
compression, the default strategy, data folders to leave out, exclude
patterns and the post-backup hook. See [Backups](backups.md).

**App integrations**: on or off, how often apps are polled (5 minutes),
reading API keys from app config files, and qBittorrent credentials.

**Notifications**: channels, quiet hours and how long repeats are combined.
See [Monitoring](monitoring.md#notifications).

**Monitoring and alerts**: resource history (sampled every minute; raw data
kept 48 hours, 15-minute averages 30 days), disks to watch, alert rules,
and how long the activity log and task logs are kept.

**Uptime**: checks for container web UIs (every 60 seconds, down after 3
failures), accepted HTTP statuses, and your own HTTP and TCP checks.

**Proxmox** and **Home Assistant**: see [Proxmox](proxmox.md) and
[Home Assistant](home-assistant.md).

**Import and export**: download all settings and container settings as a
JSON file, with or without passwords and tokens, and import them on another
Nodestarr (all or only some sections). Secrets left out of a file keep their
current values on import.

## Container settings

Each container has its own Settings tab. Container settings are keyed by the
Compose project and service (or the container name for standalone
containers), so they survive updates and recreation.

- **General**: display name, icon, web address (`{host}` is replaced with
  the address you opened Nodestarr with), tags, notes, hiding it from
  lists, and muting its alerts.
- **Updates**: policy (ignore, tell me, automatic) and how far automatic
  updates may move a pinned version, the minimum age, and holds. Empty
  fields follow Settings > Updates.
- **Backups**: included or not, the strategy, which mounts to include
  (each can be Auto, Include or Skip), extra exclude patterns, whether to
  use the app's own backup, and its own retention.
- **Uptime check**: on or off, a different address, accepted statuses and a
  keyword the page must contain.
- **Integration**: the address and API key (or username and password) used
  to talk to the app, when the automatic ones don't fit.

## Docker labels

Labels on your containers can set a few things up front:

| Label | Example | Meaning |
|---|---|---|
| `nodestarr.name` | `Sonarr (anime)` | Display name. |
| `nodestarr.url` | `https://sonarr.example.com` | Web address for the Open button. Without it, Nodestarr uses Traefik `Host()` rules or the published port. |
| `nodestarr.app` | `sonarr` | Which app this is, when the image name doesn't say (see the list below). |
| `nodestarr.self` | `true` | Marks Nodestarr's own container, if it isn't detected automatically. |

Settings made in the UI take precedence over labels.

Nodestarr recognizes these apps by their image and uses what it knows about
them (web port, config folders, what to leave out of backups, databases,
version labels, integrations). The value is what `nodestarr.app` takes, and
what `{name}` becomes in the icon URL template.

| App | Value | App | Value | App | Value |
|---|---|---|---|---|---|
| Sonarr | `sonarr` | Deluge | `deluge` | Wyoming Whisper | `wyoming-whisper` |
| Radarr | `radarr` | Gluetun | `gluetun` | Wyoming Piper | `wyoming-piper` |
| Lidarr | `lidarr` | Plex | `plex` | cloudflared | `cloudflared` |
| Readarr | `readarr` | Jellyfin | `jellyfin` | Caddy | `caddy` |
| Whisparr | `whisparr` | Emby | `emby` | Traefik | `traefik` |
| Prowlarr | `prowlarr` | Tdarr | `tdarr` | Nginx Proxy Manager | `nginx-proxy-manager` |
| Bazarr | `bazarr` | Tdarr Node | `tdarr_node` | nginx | `nginx` |
| Overseerr | `overseerr` | Audiobookshelf | `audiobookshelf` | Portainer | `portainer` |
| Jellyseerr | `jellyseerr` | Navidrome | `navidrome` | Dockge | `dockge` |
| Seerr | `seerr` | Kavita | `kavita` | Uptime Kuma | `uptime-kuma` |
| Ombi | `ombi` | Komga | `komga` | Homepage | `homepage` |
| Tautulli | `tautulli` | Calibre-Web | `calibre-web` | Vaultwarden | `vaultwarden` |
| Recyclarr | `recyclarr` | Calibre | `calibre` | AdGuard Home | `adguardhome` |
| Unpackerr | `unpackerr` | Immich | `immich` | Pi-hole | `pihole` |
| Notifiarr | `notifiarr` | Immich ML | `immich-ml` | WireGuard | `wireguard` |
| autobrr | `autobrr` | PhotoPrism | `photoprism` | rclone | `rclone` |
| Maintainerr | `maintainerr` | Nextcloud | `nextcloud` | Watchtower | `watchtower` |
| Wizarr | `wizarr` | Syncthing | `syncthing` | Gitea | `gitea` |
| Kometa | `kometa` | File Browser | `filebrowser` | code-server | `code-server` |
| FlareSolverr | `flaresolverr` | Paperless-ngx | `paperless` | PostgreSQL | `postgres` |
| Byparr | `byparr` | Home Assistant | `homeassistant` | PostgreSQL (Immich) | `immich-postgres` |
| Spottarr | `spottarr` | Frigate | `frigate` | MySQL | `mysql` |
| SABnzbd | `sabnzbd` | Zigbee2MQTT | `zigbee2mqtt` | MariaDB | `mariadb` |
| NZBGet | `nzbget` | Mosquitto | `mosquitto` | Redis | `redis` |
| qBittorrent | `qbittorrent` | Node-RED | `node-red` | MongoDB | `mongo` |
| Transmission | `transmission` | ESPHome | `esphome` |  |  |

## App icons

Nodestarr doesn't ship third-party logos; it draws a colored monogram for
each app. To show real icons, host an icon set you are allowed to use and
set Settings > General > Icon URL template, for example
`https://icons.example.com/png/{name}.png`. `{name}` becomes the app's
value from the table above. A container's own icon setting overrides it.

## Command line

The binary has a few maintenance commands. In a container, run them with
`docker exec nodestarr nodestarr <command>`.

| Command | What it does |
|---|---|
| `nodestarr` or `nodestarr serve` | Runs the server. |
| `nodestarr reset-password --username NAME [--password PW]` | Sets a new password (prints a generated one if you leave out `--password`) and signs the user out everywhere. |
| `nodestarr disable-2fa --username NAME` | Turns off two-step sign-in for a user. |
| `nodestarr healthcheck` | Exits with 0 when the local server answers (used by the image's health check). |
| `nodestarr version` | Prints the version. |
