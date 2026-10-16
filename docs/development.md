# Development

## Layout

```text
src/            the server (Rust)
web/            the web UI (Svelte 5, TypeScript, Vite), embedded into the binary
docs/           documentation
dev/            stand-ins for Proxmox and webhooks (see dev/README.md)
packaging/      systemd unit
.github/        CI and release workflows
Dockerfile, compose.yaml
```

### Server modules

| Module | Responsibility |
|---|---|
| `app` | Builds the shared `App` state and starts the background services. |
| `config`, `cli` | Environment variables and flags; maintenance commands. |
| `http` | Routes (`http/api/*`), authentication middleware, the event stream (`sse`), WebSockets for logs and consoles (`ws`), the embedded UI (`assets`). |
| `auth` | Users, Argon2id passwords, sessions, TOTP, API keys, revocation of live connections. |
| `settings`, `prefs` | Settings (one JSON document, sections, secret masking) and per-container settings. |
| `db` | SQLite with one connection in WAL mode, migrations, snapshots. |
| `docker` | The container inventory (follows Docker's event stream), actions, recreation, specs and `docker run` parsing, logs, exec, stats, the app catalog (`apps`), images, volumes and networks. |
| `compose` | Stacks: discovery, files (and the stack editor's confined access to a stack's folder), validation, editing image tags (also through `.env`), running the Compose CLI. |
| `registry` | A small OCI distribution client (digests, tags, image configs) and version parsing. |
| `updates` | Checks, automatic updates, applying with verification and rollback, release notes. |
| `backups` | Plans, archives (tar with zstd or gzip), database dumps, retention, hooks, restore. |
| `monitor` | Resource sampling and history, alerts, uptime checks, host stats. |
| `integrations` | API clients for the supported apps, API key discovery. |
| `notify` | Notification channels, quiet hours and de-duplication. |
| `mqtt`, `metrics` | Home Assistant over MQTT; Prometheus metrics. |
| `proxmox` | Proxmox VE API client (certificate pinning) and guest operations. |
| `tasks`, `scheduler`, `activity`, `events` | Long-running operations with logs and locks; cron jobs; the activity log; the in-process event bus. |
| `selfupdate`, `files`, `ops` | Updating Nodestarr's own container; config file editing with versions; shared container and stack operations. |
| `error`, `logging`, `util` | The error type and its API mapping; log setup and the in-memory log ring for System > Logs; helpers (atomic writes, path confinement, CIDR lists). |

How it fits together: one process, a small Tokio runtime (2 worker
threads), and services that wait on events rather than poll. The Docker
inventory follows Docker's event stream and publishes changes on an event
bus; each browser tab gets them through one server-sent event stream.
Anything slow runs as a task with a log, progress and locks (on the
container, its stack, and for updates a global one), so operations on the
same thing never overlap.

### UI structure

```text
web/src/
  App.svelte          shell, routes
  lib/                api client, live store (event stream), session, router, formatting, helpers
  lib/compose/        the stack editor's model: YAML edits that keep the file's layout, .env files,
                      interpolation, environments, Dockerfiles, checks and templates
  components/         shared building blocks (tables, dialogs, editors, charts, logs, terminal)
  components/studio/  the stack editor (service forms, environment, variables, Dockerfiles)
  pages/              one folder per area: container/, stack/, monitoring/, proxmox/, settings/, system/
```

The stack editor works on text: a visual edit parses the current file,
changes the parsed document and writes it back. Serializing a document
normalizes layout, so the result is merged line by line into the original
(`lib/compose/merge3.ts`) and kept only if it parses to exactly the same
data as the edit; otherwise the plainly serialized file is used.

Pages and heavy components (CodeMirror, xterm.js, uPlot) load on demand.
The design system is described in [web/DESIGN.md](../web/DESIGN.md).

## Building and running

You need Rust 1.88 or newer and Node.js 22.

```sh
# The UI, rebuilt on change, on http://localhost:5173 (talks to a running
# Nodestarr on :7979; set NODESTARR_DEV_API to use another one)
cd web && npm ci && npm run dev

# The server, with a local data folder
NODESTARR_DATA_DIR=./data cargo run
```

Debug builds read the UI from `web/dist` at runtime, so `npm run build`
is enough to see UI changes there. Release builds embed it:

```sh
cd web && npm run build && cd ..
cargo build --release      # target/release/nodestarr
docker build -t nodestarr .
```

## Checks

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cd web && npm run check && npm test
```

CI runs all of these on every push and pull request, and builds the image.
The UI tests use Node's built-in test runner on the pure helpers in
`web/src/lib`.

## Conventions

- **Words**: plain, short and in sentence case; buttons say what happens.
  See the design notes.
- **Settings**: new fields get `#[serde(default)]` so existing databases
  load. Fields holding secrets must match a name in
  `src/settings/secrets.rs`, so they are masked before reaching the browser.
- **Database**: never edit an applied migration; add a new file to
  `src/db/migrations` and to the list in `src/db/mod.rs`.
- **Long operations**: start a task (`TaskSpec`); targeting a container
  also locks its stack. Report progress with `ctx.log` and `ctx.progress`.
- **Events**: add the variant to `src/events.rs` and handle it in
  `web/src/lib/live.svelte.ts`.
- **Permissions**: changes need an admin (enforced in the middleware);
  endpoints that reveal secrets call `require_admin()` themselves.

## Releasing

1. Update the version in `Cargo.toml` and `web/package.json`, and add the
   changes to `CHANGELOG.md`.
2. Tag the commit (`git tag v0.2.0 && git push --tags`).
3. The release workflow builds the image for amd64 and arm64, publishes it
   as `ghcr.io/gslashm/nodestarr` with the version and `latest` tags, and
   creates a GitHub release. Images built there know the repository, so
   Nodestarr can announce new versions and show release notes.
