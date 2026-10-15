# Installation

Nodestarr runs on the machine that runs Docker. The usual way is a container
next to your other stacks; a plain binary with systemd works too.

## Requirements

- Linux with Docker Engine and the Docker Compose plugin (v2 or newer).
  Nodestarr is tested with Docker Engine 29 and Compose 5. Compose is only
  needed for stacks; standalone containers work without it.
- x86-64 or 64-bit ARM.
- Memory: Nodestarr itself idles at about 20 MB (see
  [Resource use](#resource-use)).

## Run it with Docker Compose

The repository contains a `compose.yaml` that builds the image from source.
Put the repository with your other stacks and start it:

```sh
git clone https://github.com/gslashm/nodestarr /opt/stacks/nodestarr
cd /opt/stacks/nodestarr
docker compose up -d --build
```

The first build takes a few minutes (it compiles the server and the web UI);
later builds reuse the cached dependencies.

### What the volumes are for

```yaml
volumes:
  - /var/run/docker.sock:/var/run/docker.sock
  - /opt/stacks:/opt/stacks
  - ./config:/config
  - ./backups:/backups
```

- **The Docker socket** is how Nodestarr sees and manages containers.
  Anything with access to it can control Docker, so treat Nodestarr's
  admin accounts accordingly (see [Security](security.md)).
- **Your stacks folder, at the same path.** This matters: Compose resolves
  relative paths in a compose file (`./sonarr:/config`) from the stack's
  folder, and Docker needs those paths as they are on the host. Mount every
  folder that holds stacks (Settings > Docker > Stack folders) at the same
  path inside the container. Backups and the config file editor reach app
  folders through these mounts too.
- **`/config`** holds Nodestarr's database (settings, users, history,
  metrics). Each scheduled backup run also saves a copy of it in the backup
  folder.
- **`/backups`** is where backup archives go. Point it at storage with
  room to spare, ideally not the boot disk.

If your apps keep their config outside the stacks folder (for example in
`/srv/appdata`), mount that folder at the same path as well so backups and
the file editor can read it.

### Backups on a Proxmox host's storage

When Docker runs in a Proxmox LXC, first make the host folder available
inside the LXC, then mount it into Nodestarr. On the Proxmox host:

```sh
mkdir -p /mnt/storage/backup/nodestarr
# Unprivileged LXCs map root to uid 100000 on the host.
chown 100000:100000 /mnt/storage/backup/nodestarr
pct set <CTID> -mp1 /mnt/storage/backup/nodestarr,mp=/mnt/backup/nodestarr
```

Restart the LXC, then use `/mnt/backup/nodestarr:/backups` in
`compose.yaml`. (`<CTID>` is the LXC's container ID and `mp1` the next free
mount point; adjust both.)

### Networking

`compose.yaml` uses host networking. That way Nodestarr reaches every
container's web UI the same way you do (for uptime checks and app
integrations) and sees real client addresses (for sign-in rate limits and
the local network bypass).

If you prefer a bridge network, replace `network_mode: host` with
`ports: ["7979:7979"]`. Nodestarr then reaches apps through their published
ports on the Docker host or through networks it shares with them.

### Using a published image

Releases built by this repository's release workflow are published as
`ghcr.io/gslashm/nodestarr`. To use one instead of building, replace
`build: .` and `image: nodestarr:local` with:

```yaml
image: ghcr.io/gslashm/nodestarr:latest
```

Published images can update Nodestarr from its own UI (System > Nodestarr).

## First start

Open `http://<server>:7979`. On the first start Nodestarr has no users and
prints a one-time setup code to its log:

```sh
docker logs nodestarr 2>&1 | grep "Setup code"
```

Enter the code on the setup page and choose your admin username and
password. To skip this step (for automated installs), set
`NODESTARR_ADMIN_USER` and `NODESTARR_ADMIN_PASSWORD` the first time it
starts; they are ignored once a user exists.

Then, in Settings:

1. **Backups**: check the backup folder and turn on the schedule.
2. **Notifications**: add a channel (Discord, email, ...) and send a test.
3. **Updates**: decide how often to check and whether anything may update
   automatically.
4. **Proxmox**, **Home Assistant**: connect them if you use them.

### Locked out?

```sh
# New password for a user (prints a generated one if you leave out --password)
docker exec nodestarr nodestarr reset-password --username admin

# Turn off two-step sign-in for a user who lost their authenticator
docker exec nodestarr nodestarr disable-2fa --username admin
```

If a wrong URL path setting locks you out, start Nodestarr once with
`NODESTARR_URL_BASE=` (empty) to override it.

## Behind a reverse proxy

Nodestarr works behind any reverse proxy. Three things matter:

- **WebSockets** for the console and live logs, and **server-sent events**
  for live updates: the proxy must pass `Upgrade` headers and must not
  buffer responses.
- **The Host header** (or `X-Forwarded-Host`) must be passed through;
  Nodestarr refuses WebSocket connections from other sites.
- Add the proxy's address under Settings > Sign-in and security > Trusted
  proxies so Nodestarr uses the real client address from
  `X-Forwarded-For`.

To serve Nodestarr under a path (`https://example.com/nodestarr/`), set
Settings > General > URL path to `/nodestarr`, restart, and forward the
path unchanged.

**Caddy**

```caddyfile
nodestarr.example.com {
    reverse_proxy <server>:7979
}
```

**nginx**

```nginx
map $http_upgrade $connection_upgrade {
    default upgrade;
    ''      close;
}

server {
    server_name nodestarr.example.com;
    # listen and TLS settings ...

    location / {
        proxy_pass http://<server>:7979;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection $connection_upgrade;
        proxy_buffering off;
        proxy_read_timeout 1h;
    }
}
```

**Traefik** (labels on the Nodestarr service, bridge networking)

```yaml
labels:
  - traefik.enable=true
  - traefik.http.routers.nodestarr.rule=Host(`nodestarr.example.com`)
  - traefik.http.services.nodestarr.loadbalancer.server.port=7979
```

To sign in through an authenticating proxy (Authelia, Authentik), see
[Security](security.md#signing-in-through-a-reverse-proxy).

## Without Docker (systemd)

Nodestarr is a single binary with the web UI built in. It needs the Docker
CLI and Compose plugin on the host for stacks.

Build it (needs Rust 1.88 or newer and Node.js 22):

```sh
cd web && npm ci && npm run build && cd ..
cargo build --release --locked
sudo install -m 0755 target/release/nodestarr /usr/local/bin/nodestarr
```

Or copy it out of an image:

```sh
docker create --name ns-tmp nodestarr:local
docker cp ns-tmp:/usr/local/bin/nodestarr ./nodestarr
docker rm ns-tmp
```

Then install the service from `packaging/systemd/nodestarr.service`:

```sh
sudo install -m 0644 packaging/systemd/nodestarr.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now nodestarr
journalctl -u nodestarr | grep "Setup code"
```

The service keeps its data in `/var/lib/nodestarr`. Backups go to
`/backups` unless you change the folder in Settings > Backups.

## Updating Nodestarr

- **Published image:** System > Nodestarr > Update. Nodestarr pulls the new
  image and starts a short-lived helper container that recreates the
  Nodestarr container, waits for it to come up, and switches back to the
  previous version if it doesn't. Turn on "Tell me when a new Nodestarr
  version is out" in Settings > General to be notified.
- **Built from source:** pull the repository and rebuild:

  ```sh
  cd /opt/stacks/nodestarr && git pull && docker compose up -d --build
  ```

- **systemd:** replace the binary and `sudo systemctl restart nodestarr`.

The database schema upgrades itself on start. Take a backup of `/config`
before big upgrades anyway.

## Uninstalling

```sh
cd /opt/stacks/nodestarr && docker compose down
```

Nodestarr keeps a few previous images for rollbacks, tagged
`nodestarr-rollback/...`. Remove them with:

```sh
docker images --format '{{.Repository}}:{{.Tag}}' | grep '^nodestarr-rollback/' | xargs -r docker rmi
```

Your backups stay in the backup folder until you delete them.

## Resource use

Nodestarr is built to stay out of the way. It follows Docker's event stream
instead of polling, samples resources once a minute (every few seconds only
while someone has the UI open), and keeps its history in a compact SQLite
database. Measured with a release build on a host with 18 containers, with
update checks, uptime checks, app integrations, Home Assistant and Proxmox
all turned on, a few minutes after using the web UI:

| | |
|---|---|
| Memory (idle) | about 22 MB resident, of which under 5 MB is data (the rest is the program itself) |
| Memory (peak) | under 60 MB while people sign in: each password check briefly uses 19 MB, and at most two run at once |
| CPU (idle) | under 0.1% of one core |
| Database | about 15 MB once the history is full (48 hours of samples every minute, 30 days of 15-minute averages, and uptime results), and it stays there |
