// Starting points for new services and Dockerfiles.
//
// Service templates use stack variables (`${TZ}`, `${PUID}`, `${MEDIA_DIR}`)
// so several apps share one setting in .env; adding a template adds the
// variables it needs. `{name}` in strings becomes the service's name, so
// config folders land in `./sonarr` next to the compose file.

export interface TemplateVar {
  name: string;
  /** Suggested value; secrets get a generated one. */
  value: string;
  label: string;
  secret?: boolean;
}

export interface ServiceTemplate {
  slug: string;
  name: string;
  category: 'Media' | 'Downloads' | 'Home automation' | 'Network' | 'Tools' | 'Databases' | 'Web';
  description: string;
  service: Record<string, unknown>;
  vars?: string[];
  /** Named volumes to declare at the top level. */
  volumes?: string[];
  /** Extra files created next to the compose file (paths relative to the stack folder). */
  files?: { path: string; content: string }[];
  /** Shown after adding it. */
  note?: string;
}

/** Variables templates share, with their suggested values. */
export function stackVariables(): Record<string, TemplateVar> {
  let tz = 'Etc/UTC';
  try {
    tz = Intl.DateTimeFormat().resolvedOptions().timeZone || tz;
  } catch {
    // Keep UTC.
  }
  return {
    PUID: { name: 'PUID', value: '1000', label: 'User ID the apps run as' },
    PGID: { name: 'PGID', value: '1000', label: 'Group ID the apps run as' },
    TZ: { name: 'TZ', value: tz, label: 'Time zone' },
    MEDIA_DIR: { name: 'MEDIA_DIR', value: '/mnt/media', label: 'Folder with your media' },
    DOWNLOADS_DIR: { name: 'DOWNLOADS_DIR', value: '/mnt/downloads', label: 'Folder for downloads' },
    POSTGRES_PASSWORD: { name: 'POSTGRES_PASSWORD', value: '', label: 'PostgreSQL password', secret: true },
    MARIADB_ROOT_PASSWORD: { name: 'MARIADB_ROOT_PASSWORD', value: '', label: 'MariaDB root password', secret: true },
    MARIADB_PASSWORD: { name: 'MARIADB_PASSWORD', value: '', label: 'MariaDB app password', secret: true },
    MONGO_PASSWORD: { name: 'MONGO_PASSWORD', value: '', label: 'MongoDB password', secret: true },
    PIHOLE_PASSWORD: { name: 'PIHOLE_PASSWORD', value: '', label: 'Pi-hole web password', secret: true },
    TUNNEL_TOKEN: { name: 'TUNNEL_TOKEN', value: '', label: 'Cloudflare tunnel token (from the Zero Trust dashboard)' },
    PLEX_CLAIM: { name: 'PLEX_CLAIM', value: '', label: 'Plex claim token (plex.tv/claim, optional)' },
    ZIGBEE_ADAPTER: { name: 'ZIGBEE_ADAPTER', value: '/dev/ttyUSB0', label: 'Zigbee adapter device' },
  };
}

/** A strong random password (letters and digits, so it needs no quoting anywhere). */
export function generatePassword(length = 24): string {
  const chars = 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789';
  const bytes = new Uint8Array(length);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => chars[b % chars.length]).join('');
}

const LSIO_ENV = { PUID: '${PUID}', PGID: '${PGID}', TZ: '${TZ}' };
const LSIO_VARS = ['PUID', 'PGID', 'TZ'];

function lsio(slug: string, name: string, category: ServiceTemplate['category'], description: string, ports: string[], extra: string[] = [], env: Record<string, string> = {}, vars: string[] = []): ServiceTemplate {
  return {
    slug,
    name,
    category,
    description,
    service: {
      image: `lscr.io/linuxserver/${slug}:latest`,
      restart: 'unless-stopped',
      environment: { ...LSIO_ENV, ...env },
      ports,
      volumes: ['./{name}:/config', ...extra],
    },
    vars: [...LSIO_VARS, ...vars],
  };
}

export const SERVICE_TEMPLATES: ServiceTemplate[] = [
  lsio('sonarr', 'Sonarr', 'Media', 'TV series manager', ['8989:8989'], ['${MEDIA_DIR}/tv:/tv', '${DOWNLOADS_DIR}:/downloads'], {}, ['MEDIA_DIR', 'DOWNLOADS_DIR']),
  lsio('radarr', 'Radarr', 'Media', 'Movie manager', ['7878:7878'], ['${MEDIA_DIR}/movies:/movies', '${DOWNLOADS_DIR}:/downloads'], {}, ['MEDIA_DIR', 'DOWNLOADS_DIR']),
  lsio('lidarr', 'Lidarr', 'Media', 'Music manager', ['8686:8686'], ['${MEDIA_DIR}/music:/music', '${DOWNLOADS_DIR}:/downloads'], {}, ['MEDIA_DIR', 'DOWNLOADS_DIR']),
  lsio('prowlarr', 'Prowlarr', 'Media', 'Indexer manager for the *arr apps', ['9696:9696']),
  lsio('bazarr', 'Bazarr', 'Media', 'Subtitles for Sonarr and Radarr', ['6767:6767'], ['${MEDIA_DIR}/movies:/movies', '${MEDIA_DIR}/tv:/tv'], {}, ['MEDIA_DIR']),
  {
    slug: 'plex',
    name: 'Plex',
    category: 'Media',
    description: 'Media server',
    service: {
      image: 'lscr.io/linuxserver/plex:latest',
      restart: 'unless-stopped',
      network_mode: 'host',
      environment: { ...LSIO_ENV, VERSION: 'docker', PLEX_CLAIM: '${PLEX_CLAIM:-}' },
      volumes: ['./{name}:/config', '${MEDIA_DIR}/tv:/tv', '${MEDIA_DIR}/movies:/movies'],
    },
    vars: [...LSIO_VARS, 'MEDIA_DIR', 'PLEX_CLAIM'],
    note: 'Plex uses the host network, so it is reachable on port 32400. For hardware transcoding, add /dev/dri under Resources > Devices.',
  },
  lsio('jellyfin', 'Jellyfin', 'Media', 'Free media server', ['8096:8096'], ['${MEDIA_DIR}/tv:/data/tvshows', '${MEDIA_DIR}/movies:/data/movies'], {}, ['MEDIA_DIR']),
  lsio('tautulli', 'Tautulli', 'Media', 'Plex statistics and monitoring', ['8181:8181']),
  {
    slug: 'seerr',
    name: 'Seerr',
    category: 'Media',
    description: 'Requests for Plex, Jellyfin and Emby',
    service: {
      image: 'ghcr.io/seerr-team/seerr:latest',
      restart: 'unless-stopped',
      init: true,
      environment: { TZ: '${TZ}', LOG_LEVEL: 'info' },
      ports: ['5055:5055'],
      volumes: ['./{name}:/app/config'],
    },
    vars: ['TZ'],
  },
  {
    slug: 'audiobookshelf',
    name: 'Audiobookshelf',
    category: 'Media',
    description: 'Audiobook and podcast server',
    service: {
      image: 'ghcr.io/advplyr/audiobookshelf:latest',
      restart: 'unless-stopped',
      environment: { TZ: '${TZ}' },
      ports: ['13378:80'],
      volumes: ['./{name}/config:/config', './{name}/metadata:/metadata', '${MEDIA_DIR}/audiobooks:/audiobooks', '${MEDIA_DIR}/podcasts:/podcasts'],
    },
    vars: ['TZ', 'MEDIA_DIR'],
  },
  {
    slug: 'navidrome',
    name: 'Navidrome',
    category: 'Media',
    description: 'Music streaming server',
    service: {
      image: 'deluan/navidrome:latest',
      restart: 'unless-stopped',
      user: '${PUID}:${PGID}',
      ports: ['4533:4533'],
      volumes: ['./{name}:/data', '${MEDIA_DIR}/music:/music:ro'],
    },
    vars: ['PUID', 'PGID', 'MEDIA_DIR'],
  },
  lsio('qbittorrent', 'qBittorrent', 'Downloads', 'Torrent client', ['8080:8080', '6881:6881', '6881:6881/udp'], ['${DOWNLOADS_DIR}:/downloads'], { WEBUI_PORT: '8080', TORRENTING_PORT: '6881' }, ['DOWNLOADS_DIR']),
  lsio('sabnzbd', 'SABnzbd', 'Downloads', 'Usenet downloader', ['8085:8080'], ['${DOWNLOADS_DIR}:/downloads', './{name}/incomplete:/incomplete-downloads'], {}, ['DOWNLOADS_DIR']),
  lsio('transmission', 'Transmission', 'Downloads', 'Lightweight torrent client', ['9091:9091', '51413:51413', '51413:51413/udp'], ['${DOWNLOADS_DIR}:/downloads', './{name}/watch:/watch'], {}, ['DOWNLOADS_DIR']),
  {
    slug: 'homeassistant',
    name: 'Home Assistant',
    category: 'Home automation',
    description: 'Home automation hub',
    service: {
      image: 'ghcr.io/home-assistant/home-assistant:stable',
      restart: 'unless-stopped',
      network_mode: 'host',
      privileged: true,
      environment: { TZ: '${TZ}' },
      volumes: ['./{name}:/config', '/etc/localtime:/etc/localtime:ro', '/run/dbus:/run/dbus:ro'],
    },
    vars: ['TZ'],
    note: 'Home Assistant uses the host network; open it on port 8123.',
  },
  {
    slug: 'mosquitto',
    name: 'Mosquitto',
    category: 'Home automation',
    description: 'MQTT broker',
    service: {
      image: 'eclipse-mosquitto:2',
      restart: 'unless-stopped',
      ports: ['1883:1883'],
      volumes: ['./{name}/config:/mosquitto/config', './{name}/data:/mosquitto/data', './{name}/log:/mosquitto/log'],
    },
    files: [
      {
        path: '{name}/config/mosquitto.conf',
        content:
          'listener 1883\npersistence true\npersistence_location /mosquitto/data/\nlog_dest file /mosquitto/log/mosquitto.log\n\n# Anyone on the network can connect. Once it works, add users with\n# mosquitto_passwd and switch to a password file.\nallow_anonymous true\n',
      },
    ],
  },
  {
    slug: 'zigbee2mqtt',
    name: 'Zigbee2MQTT',
    category: 'Home automation',
    description: 'Zigbee devices over MQTT',
    service: {
      image: 'koenkk/zigbee2mqtt:latest',
      restart: 'unless-stopped',
      environment: { TZ: '${TZ}' },
      ports: ['8080:8080'],
      volumes: ['./{name}:/app/data', '/run/udev:/run/udev:ro'],
      devices: ['${ZIGBEE_ADAPTER}:/dev/ttyACM0'],
    },
    vars: ['TZ', 'ZIGBEE_ADAPTER'],
  },
  {
    slug: 'esphome',
    name: 'ESPHome',
    category: 'Home automation',
    description: 'Firmware for ESP devices',
    service: {
      image: 'ghcr.io/esphome/esphome:stable',
      restart: 'unless-stopped',
      network_mode: 'host',
      volumes: ['./{name}:/config', '/etc/localtime:/etc/localtime:ro'],
    },
  },
  {
    slug: 'nginx-proxy-manager',
    name: 'Nginx Proxy Manager',
    category: 'Network',
    description: 'Reverse proxy with free certificates',
    service: {
      image: 'jc21/nginx-proxy-manager:latest',
      restart: 'unless-stopped',
      ports: ['80:80', '81:81', '443:443'],
      volumes: ['./{name}/data:/data', './{name}/letsencrypt:/etc/letsencrypt'],
    },
    note: 'The admin panel is on port 81.',
  },
  {
    slug: 'adguardhome',
    name: 'AdGuard Home',
    category: 'Network',
    description: 'Network-wide ad blocking DNS',
    service: {
      image: 'adguard/adguardhome:latest',
      restart: 'unless-stopped',
      ports: ['53:53/tcp', '53:53/udp', '3000:3000/tcp', '8053:80/tcp'],
      volumes: ['./{name}/work:/opt/adguardhome/work', './{name}/conf:/opt/adguardhome/conf'],
    },
    note: 'Finish the setup on port 3000.',
  },
  {
    slug: 'pihole',
    name: 'Pi-hole',
    category: 'Network',
    description: 'Network-wide ad blocking DNS',
    service: {
      image: 'pihole/pihole:latest',
      restart: 'unless-stopped',
      environment: { TZ: '${TZ}', FTLCONF_webserver_api_password: '${PIHOLE_PASSWORD}' },
      ports: ['53:53/tcp', '53:53/udp', '8081:80/tcp'],
      volumes: ['./{name}:/etc/pihole'],
      cap_add: ['NET_ADMIN', 'SYS_TIME', 'SYS_NICE'],
    },
    vars: ['TZ', 'PIHOLE_PASSWORD'],
  },
  {
    slug: 'cloudflared',
    name: 'Cloudflare Tunnel',
    category: 'Network',
    description: 'Reach your apps without opening ports',
    service: {
      image: 'cloudflare/cloudflared:latest',
      restart: 'unless-stopped',
      command: 'tunnel --no-autoupdate run',
      environment: { TUNNEL_TOKEN: '${TUNNEL_TOKEN}' },
    },
    vars: ['TUNNEL_TOKEN'],
  },
  {
    slug: 'vaultwarden',
    name: 'Vaultwarden',
    category: 'Tools',
    description: 'Password manager (Bitwarden compatible)',
    service: {
      image: 'vaultwarden/server:latest',
      restart: 'unless-stopped',
      environment: { DOMAIN: 'https://vault.example.com', SIGNUPS_ALLOWED: 'true' },
      ports: ['8080:80'],
      volumes: ['./{name}:/data'],
    },
    note: 'Set DOMAIN to the address you open it at, and turn SIGNUPS_ALLOWED off once your account exists.',
  },
  {
    slug: 'uptime-kuma',
    name: 'Uptime Kuma',
    category: 'Tools',
    description: 'Status pages and uptime checks',
    service: {
      image: 'louislam/uptime-kuma:2',
      restart: 'unless-stopped',
      ports: ['3001:3001'],
      volumes: ['./{name}:/app/data'],
    },
  },
  {
    slug: 'homepage',
    name: 'Homepage',
    category: 'Tools',
    description: 'Dashboard for your services',
    service: {
      image: 'ghcr.io/gethomepage/homepage:latest',
      restart: 'unless-stopped',
      environment: { HOMEPAGE_ALLOWED_HOSTS: 'localhost:3000' },
      ports: ['3000:3000'],
      volumes: ['./{name}:/app/config', '/var/run/docker.sock:/var/run/docker.sock:ro'],
    },
    note: 'Add the address you open it at to HOMEPAGE_ALLOWED_HOSTS.',
  },
  lsio('syncthing', 'Syncthing', 'Tools', 'File synchronization', ['8384:8384', '22000:22000/tcp', '22000:22000/udp', '21027:21027/udp'], ['./{name}/data:/data1']),
  {
    slug: 'portainer',
    name: 'Portainer',
    category: 'Tools',
    description: 'Container management UI',
    service: {
      image: 'portainer/portainer-ce:lts',
      restart: 'unless-stopped',
      ports: ['9443:9443'],
      volumes: ['/var/run/docker.sock:/var/run/docker.sock', 'portainer_data:/data'],
    },
    volumes: ['portainer_data'],
  },
  {
    slug: 'postgres',
    name: 'PostgreSQL',
    category: 'Databases',
    description: 'Relational database',
    service: {
      image: 'postgres:17-alpine',
      restart: 'unless-stopped',
      environment: { POSTGRES_USER: 'app', POSTGRES_PASSWORD: '${POSTGRES_PASSWORD}', POSTGRES_DB: 'app' },
      volumes: ['./{name}:/var/lib/postgresql/data'],
      healthcheck: { test: ['CMD-SHELL', 'pg_isready -U $${POSTGRES_USER} -d $${POSTGRES_DB}'], interval: '10s', timeout: '5s', retries: 5 },
    },
    vars: ['POSTGRES_PASSWORD'],
  },
  {
    slug: 'mariadb',
    name: 'MariaDB',
    category: 'Databases',
    description: 'MySQL-compatible database',
    service: {
      image: 'mariadb:11',
      restart: 'unless-stopped',
      environment: { MARIADB_ROOT_PASSWORD: '${MARIADB_ROOT_PASSWORD}', MARIADB_DATABASE: 'app', MARIADB_USER: 'app', MARIADB_PASSWORD: '${MARIADB_PASSWORD}' },
      volumes: ['./{name}:/var/lib/mysql'],
      healthcheck: { test: ['CMD', 'healthcheck.sh', '--connect', '--innodb_initialized'], interval: '10s', timeout: '5s', retries: 5 },
    },
    vars: ['MARIADB_ROOT_PASSWORD', 'MARIADB_PASSWORD'],
  },
  {
    slug: 'redis',
    name: 'Redis',
    category: 'Databases',
    description: 'In-memory cache and store',
    service: {
      image: 'redis:7-alpine',
      restart: 'unless-stopped',
      command: 'redis-server --save 60 1 --loglevel warning',
      volumes: ['./{name}:/data'],
      healthcheck: { test: ['CMD', 'redis-cli', 'ping'], interval: '10s', timeout: '5s', retries: 5 },
    },
  },
  {
    slug: 'mongo',
    name: 'MongoDB',
    category: 'Databases',
    description: 'Document database',
    service: {
      image: 'mongo:8',
      restart: 'unless-stopped',
      environment: { MONGO_INITDB_ROOT_USERNAME: 'admin', MONGO_INITDB_ROOT_PASSWORD: '${MONGO_PASSWORD}' },
      volumes: ['./{name}:/data/db'],
    },
    vars: ['MONGO_PASSWORD'],
  },
  {
    slug: 'nginx',
    name: 'nginx',
    category: 'Web',
    description: 'Web server for a static site',
    service: {
      image: 'nginx:alpine',
      restart: 'unless-stopped',
      ports: ['8080:80'],
      volumes: ['./{name}/html:/usr/share/nginx/html:ro'],
    },
    files: [{ path: '{name}/html/index.html', content: '<!doctype html>\n<title>It works</title>\n<h1>It works</h1>\n' }],
  },
  {
    slug: 'caddy',
    name: 'Caddy',
    category: 'Web',
    description: 'Web server and reverse proxy with automatic HTTPS',
    service: {
      image: 'caddy:2',
      restart: 'unless-stopped',
      ports: ['80:80', '443:443', '443:443/udp'],
      volumes: ['./Caddyfile:/etc/caddy/Caddyfile', 'caddy_data:/data', 'caddy_config:/config'],
    },
    volumes: ['caddy_data', 'caddy_config'],
    files: [{ path: 'Caddyfile', content: '# Replace with your domain and the service to proxy to.\nexample.com {\n\treverse_proxy app:8080\n}\n' }],
  },
];

/** Replaces `{name}` in every string of a template body. */
export function instantiate<T>(value: T, name: string): T {
  if (typeof value === 'string') return value.replaceAll('{name}', name) as T;
  if (Array.isArray(value)) return value.map((v) => instantiate(v, name)) as T;
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, instantiate(v, name)])) as T;
  }
  return value;
}

/** App slug for an image (for icons), like the server's catalog does it. */
export function appSlug(image: string): string | null {
  const ref = image.split('@')[0];
  const base = ref.slice(ref.lastIndexOf('/') + 1).split(':')[0].toLowerCase();
  if (!base) return null;
  if (base === 'server') return ref.includes('vaultwarden') ? 'vaultwarden' : null;
  if (base === 'home-assistant') return 'homeassistant';
  if (base === 'pms-docker') return 'plex';
  if (base === 'immich-server') return 'immich';
  const t = SERVICE_TEMPLATES.find((x) => x.slug === base || x.slug === base.replace(/^docker-/, '').split(/[-_]/)[0]);
  return t?.slug ?? base.replace(/^docker-/, '').replace(/-server$/, '');
}

// ------------------------------------------------------------ Dockerfiles

export interface DockerfileTemplate {
  key: string;
  name: string;
  description: string;
  content: (image: string) => string;
  ignore: string;
}

const IGNORE_COMMON = '.git\n.gitignore\n.env\n*.md\nDockerfile*\n.dockerignore\n';

export const DOCKERFILE_TEMPLATES: DockerfileTemplate[] = [
  {
    key: 'extend',
    name: 'Add to an existing image',
    description: 'Start from an image and install extra packages or files',
    content: (image) =>
      /alpine/.test(image)
        ? `FROM ${image || 'alpine:3.22'}\n\n# Extra packages\nRUN apk add --no-cache curl\n`
        : `FROM ${image || 'debian:bookworm-slim'}\n\n# Extra packages\nRUN apt-get update && \\\n    apt-get install -y --no-install-recommends curl && \\\n    rm -rf /var/lib/apt/lists/*\n`,
    ignore: IGNORE_COMMON,
  },
  {
    key: 'node',
    name: 'Node.js app',
    description: 'npm install, copy the app, run it as the node user',
    content: () =>
      'FROM node:22-alpine\nWORKDIR /app\n\n# Dependencies first, so they are cached between builds\nCOPY package*.json ./\nRUN npm ci --omit=dev\n\nCOPY . .\nUSER node\nEXPOSE 3000\nCMD ["node", "server.js"]\n',
    ignore: `${IGNORE_COMMON}node_modules\nnpm-debug.log\n`,
  },
  {
    key: 'python',
    name: 'Python app',
    description: 'pip install from requirements.txt, run as a regular user',
    content: () =>
      'FROM python:3.13-slim\nENV PYTHONDONTWRITEBYTECODE=1 \\\n    PYTHONUNBUFFERED=1\nWORKDIR /app\n\nCOPY requirements.txt .\nRUN pip install --no-cache-dir -r requirements.txt\n\nCOPY . .\nRUN useradd --create-home app\nUSER app\nEXPOSE 8000\nCMD ["python", "app.py"]\n',
    ignore: `${IGNORE_COMMON}__pycache__\n*.pyc\n.venv\n`,
  },
  {
    key: 'go',
    name: 'Go app',
    description: 'Build in one stage, ship a tiny image',
    content: () =>
      'FROM golang:1.25-alpine AS build\nWORKDIR /src\nCOPY go.mod go.sum ./\nRUN go mod download\nCOPY . .\nRUN CGO_ENABLED=0 go build -o /out/app .\n\nFROM gcr.io/distroless/static-debian12\nCOPY --from=build /out/app /app\nUSER nonroot:nonroot\nEXPOSE 8080\nENTRYPOINT ["/app"]\n',
    ignore: `${IGNORE_COMMON}bin\n`,
  },
  {
    key: 'static',
    name: 'Static website',
    description: 'Serve a folder of HTML with nginx',
    content: () => 'FROM nginx:1.29-alpine\nCOPY ./site /usr/share/nginx/html\nEXPOSE 80\n',
    ignore: IGNORE_COMMON,
  },
];
