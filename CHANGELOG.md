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

### Changed

- The stack page's Compose tab is now the Editor, at the same address.

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
