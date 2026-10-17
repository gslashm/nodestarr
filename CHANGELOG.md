# Changelog

All notable changes to Nodestarr are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **Stack editor**: a stack's services as forms instead of YAML: the
  image (with a tag picker and what the image sets up by itself), ports,
  folders and volumes, networks, health check, resources, start order and
  more, with the YAML alongside and each change marked. Anything the forms
  don't cover can be edited as YAML, so nothing in a file is out of reach.
  Edits keep the files' comments, quoting and layout.
- **One list of each container's environment**: variables from the compose
  file, env files, the stack's `.env` and the image together, with where
  each one comes from, the value the container ends up with, variables that
  aren't set, and values that differ from the running container. A change
  goes back to where the value lives, and secrets move to `.env` in one
  click.
- **Variables**: the stack's `.env` as a list showing which services use
  each variable, the ones compose needs that aren't set yet, and the ones
  nothing uses. Renaming a variable renames its references too.
- **Dockerfiles** as cards, one per instruction, grouped by build stage,
  with drag to reorder, starting templates and suggestions; `.dockerignore`
  as a list of patterns. Switching a service to a Dockerfile starts one
  from its current image.
- **New stacks** from a gallery of common apps (free host ports are picked
  and passwords generated), from `docker run` commands, or from containers
  started by hand.
- The **Edit** tab of a stack's container edits its service there, and can
  save and recreate just that container.
- Any text file in a stack's folder can be opened and edited, with history.
- Undo and redo across all of a stack's files, a review of every change
  before saving, checks for common mistakes (undeclared volumes and
  networks, port clashes, variables that aren't set), and links straight
  to a service, section or file.
- API: `GET /stacks/{name}/file`, `GET /stacks/{name}/tree`,
  `GET /images/inspect` and `GET /images/tags`. Saving stack files can
  create new files, deploy only some services and rebuild images; new
  stacks can come with extra files.
- **Dismiss "needs attention" items** on the dashboard: hide one until the
  situation changes (a new crash, a newer version, a different app
  problem), or ignore that kind of item for a container for good. Dismissed
  items are one click away and can be shown again; the Containers page's
  "Needs attention" filter and the sidebar count leave them out too.
  API: `PUT /containers/{id}/dismiss`.
- **Update types**: every update is a major, minor or patch version, a
  rebuild of the same version, or of unknown size, and is marked when its
  release notes mention security fixes or breaking changes, with the line
  that says so. Nodestarr reads every release between the running version
  and the new one. The Updates page filters and sorts by type and says for
  each update whether automatic updates will install it, and when, or why
  it waits. Confirming an update warns about breaking changes. Home
  Assistant shows the type as the update's release summary, and
  `nodestarr_updates_available{type=...}` counts updates by type.
- **Proxmox usage by guest**: donut charts on the Proxmox page show how
  each node's CPU, memory, disk and network split across its guests,
  with what the node uses itself and what is free. Pointing at a guest
  highlights it in all four; in a cluster, switch between nodes.
  `GET /proxmox/status` lists the cluster's nodes (`overview.nodes`).
- **Disk health** (Proxmox > Disks): every physical disk of the Proxmox
  nodes with its SMART data kept as history and judged into a verdict
  (healthy, watch, needs attention, failing) with plain-language reasons
  that weigh how the numbers changed over the last 30 days. A page per disk
  has history charts from a week to everything kept (two years by
  default), its self-tests, the SMART
  attributes explained, where it sits in the case and its warranty, and
  everything that changed. Disks keep their history when their `/dev`
  names change or they move between controllers or nodes.
- **Pools**: ZFS pools, mdadm and SnapRAID arrays and LVM thin pools, drawn
  as drive bays, with how many more disk failures each survives, capacity
  and growth (and when it'll be full), scrub, resilver and rebuild progress,
  parity disks, and what needs attention. ZFS scrubs can be started,
  paused and stopped.
- **SMART self-tests**: short, long and conveyance tests started by hand or
  on schedules ("a weekly short and a monthly long test" in one click),
  queued while a pool is rebuilding or scrubbing, with their progress, the
  result read from the drive's own log, and a notification when one fails
  (`disk.test`).
- **Host access**: optional SSH to the nodes, through a helper script the
  key is locked to, for self-tests, mdadm and SnapRAID, full `smartctl`
  data and leaving sleeping disks asleep. Without it, disks that spin down
  between readings are noticed and read only about once a day.
- Alert rules for disk health, disk temperature, pool health and scrub age;
  disks and pools in Home Assistant (temperature and problem sensors, pool
  use) and in the Prometheus metrics. API: `/proxmox/disk-health/...` and
  `/proxmox/host-access/...`.
- **Automatic updates by type**: choose which types install on their own
  (by default rebuilds, patch and minor versions and updates of unknown
  size), let security fixes install regardless of their size (except major
  versions), and hold back updates whose release notes mention breaking
  changes. Each container can have its own choice.
- New builds of official images (nginx, postgres, ...) show their version,
  read from the image's version variable.

### Changed

- The stack page's Compose tab is now the Editor, at the same address.
- Automatic updates no longer take "how far a version tag may move":
  Settings > Updates > Install on their own lists update types instead,
  with a separate switch for changing tags. Existing choices carry over:
  "new builds only" keeps tags as they are, and "patch", "minor" or "major"
  switch tags up to that size. Two things are now held back that weren't:
  updates whose release notes mention breaking changes, and, unless "major"
  was chosen, major versions arriving as a new build of a tag like
  `latest` ("patch" also holds back minor ones).
- Release notes cover the 30 newest releases (was 15), and GitHub's rate
  limit is remembered for a few minutes instead of being asked again by
  every check.
- API: containers have `auto_install` and `min_age_hours` instead of
  `auto_scope`; the settings field `updates.default_auto_scope` is now
  `updates.auto_install`, and a container's `update.auto_scope` is
  `update.auto_install`. Older settings files still import.

### Fixed

- A container replaced during an update or rollback could linger on the
  dashboard as a stopped duplicate of its replacement.
- On long pages (such as Settings > Updates), scrolling over the sidebar
  scrolled the whole page out of view.

## [0.1.0] - 2026-10-15

The first release.

### Added

- **Containers and stacks**: finds every container and Compose stack,
  including stacks that aren't running, and keeps the list live through
  Docker's event stream. Shows version, update status, state, uptime and
  resource use as a table or a dashboard grouped by stack.
- **Actions** for single containers, whole stacks or a selection: start,
  stop, restart, pause, kill, recreate, update, back up and remove.
- **Logs and consoles**: live logs with search, a "problems only" filter and
  download, merged logs for stacks, a terminal per container, and one-off
  commands.
- **Editing**: a Compose editor with validation, a diff before saving,
  deploy and version history; new stacks from templates or `docker run`
  commands; a form to create and edit standalone containers; a config file
  editor for app files with versions.
- **Updates**: checks for new builds and newer version tags, with release
  notes. Per-container policies (ignore, notify, or update automatically
  within a scope and after a minimum age), tag and branch changes with a
  preview, a backup before each update, verification and automatic
  rollback, manual rollback, holds, skipped builds, a Plex guard and
  self-updates.
- **Backups**: config folders, compose files, database dumps (PostgreSQL,
  MySQL and MariaDB, MongoDB) and the apps' own backups, on demand or on a
  schedule, with retention, a hook after each run, and restore. Scheduled
  runs also keep copies of Nodestarr's own database.
- **Monitoring**: resource history for the host and every container,
  alerts, uptime checks, and an activity log.
- **Integrations** with Sonarr, Radarr, Lidarr, Readarr, Whisparr,
  Prowlarr, Bazarr, Seerr (and Overseerr, Jellyseerr), SABnzbd,
  qBittorrent, Plex, Tautulli, Tdarr, Immich and Home Assistant, with API
  keys read from the apps' config files.
- **Notifications** to Discord, Slack, Telegram, ntfy, Gotify, Pushover,
  email and webhooks, with event filters and quiet hours.
- **Home Assistant** over MQTT discovery, **Prometheus** metrics, and a
  **REST API** with full-access and read-only keys.
- **Proxmox VE**: the node and its guests with power actions, snapshots,
  vzdump backups, tasks, disks and package updates, over a pinned
  certificate.
- **Security**: admin and viewer roles, two-step sign-in with recovery
  codes, session management that also ends live connections, and sign-in
  through an authenticating reverse proxy.
- **Packaging**: a Docker image for amd64 and arm64, a Compose file, and a
  systemd unit for running without Docker.
