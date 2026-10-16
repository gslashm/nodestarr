# Changelog

All notable changes to Nodestarr are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

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
