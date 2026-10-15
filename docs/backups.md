# Backups and restore

Nodestarr backs up what you'd need to rebuild an app: its configuration,
its database, and how it is deployed. Media and other bulk data are left
out on purpose; those belong in your storage's own backups.

## What a backup contains

Each backup is one compressed archive per container (`.tar.zst`, or
`.tar.gz` if you prefer), with:

- **Configuration folders.** Nodestarr decides per mount, and shows its
  reasoning on the container's Backups tab:
  - known config folders of recognized apps (like `/config` for LinuxServer
    images) are included;
  - named volumes and folders inside the stack's folder are included;
  - media and download folders of recognized apps, anything under a data
    folder (Settings > Backups > Data folders: `/data`, `/mnt`, `/media`,
    `/srv` and `/downloads` by default) and system paths are left out.

  You can override any of this per mount (Auto, Include, Skip).
- **A database dump** for database containers (PostgreSQL with
  `pg_dumpall`, MySQL and MariaDB with `mysqldump`/`mariadb-dump`, MongoDB
  with `mongodump`), using the credentials already in the container's
  environment. The database's raw files are skipped since the dump is
  consistent and smaller.
- **The app's own backup**, optionally, for the *arr apps (it asks Sonarr,
  Radarr and the like to make one through their API).
- **The stack's compose files and `.env`**, so the archive says how the app
  was deployed.
- A manifest (`nodestarr-backup.json`, also saved next to the archive)
  listing what is inside and how it was made.

Caches, logs and other junk are skipped: `*.log`, `logs/*.txt` and
`.cache` everywhere, plus app specific folders such as Plex's `Cache`,
`Metadata`, `Media` and `Codecs` (Plex downloads them again), or the *arr
apps' `MediaCover` and logs. Add your own patterns
in Settings > Backups or on a container.

## Consistent copies

Copying files while an app writes to them can produce a broken copy,
especially of SQLite databases. The strategy decides how Nodestarr avoids
that:

| Strategy | What happens |
|---|---|
| Auto (default) | Database containers are dumped. Apps known to keep SQLite databases (the *arr apps, Plex, Tautulli, ...) are paused while their files are copied, usually for a few seconds. Others are copied while running. |
| Pause | The container is frozen while its files are copied. |
| Stop | The container is stopped and started again afterwards. |
| Live | Files are copied while the container runs. |

Set the default in Settings > Backups and override it per container.

Folders that Nodestarr can see at the same path are read directly. Others
(named volumes, or folders not mounted into Nodestarr) are streamed through
Docker, which works but is slower.

## Running backups

- **Back up now**: on a container, a stack, or the Backups page (all
  included containers).
- **On a schedule**: Settings > Backups > Schedule (03:30 every night by
  default, off until you turn it on). Every container is included unless it
  opts out; turn off "Include every container unless it opts out" to
  choose them one by one instead.
- **Before updates**: with Settings > Updates > Back up before updating, the
  container is backed up right before each update, and the update stops if
  the backup fails.

Archives are stored as
`<backup folder>/<stack>/<container>/<date>-<kind>.tar.zst` (standalone
containers under `standalone/`), where the kind is `manual`, `scheduled`,
`pre-update` or `pre-restore`.

### Retention

Settings > Backups keeps 7 archives per container by default (0 keeps
all), optionally only up to a maximum age. Pre-update backups are kept for
14 days. Pinned archives are never deleted. A container can have its own
count.

### After each run: off-site copies

Settings > Backups > After each backup run runs a command when backups finish,
either in Nodestarr's own environment or inside another container. The
variables `NODESTARR_BACKUP_ROOT` and `NODESTARR_BACKUP_STATUS` (`ok` or
`failed`) are set. You can run it only after successful runs, or always.

For example, to sync backups to Google Drive through an rclone crypt
remote, run a small rclone container next to Nodestarr:

```yaml
services:
  rclone:
    image: rclone/rclone:latest
    container_name: rclone
    restart: unless-stopped
    # Stay up, so Nodestarr can run commands in it.
    entrypoint: ["sleep", "infinity"]
    volumes:
      - /opt/stacks/rclone/config:/config/rclone
      - /mnt/backup/nodestarr:/backups:ro
```

and set the hook to run in container `rclone`:

```sh
rclone sync /backups gcrypt:nodestarr --transfers 4
```

Failed hooks send a "backup failed" notification.

## Restoring

Open the archive (Backups > Archives, or a container's Backups tab), check
its contents, and choose Restore. You decide:

- **Files**, **database**, or both.
- **Replace**: also delete files that aren't in the backup (otherwise
  restored files are written over the current ones and anything else is
  left alone). Excluded paths are never touched.
- **Safety backup**: back up the current state first, so the restore can
  be undone (on by default).

Nodestarr stops the container, restores the files, replays the database
dump into the (running) database, and starts the container again if it was
running before.

Archives can also be downloaded, and opened without Nodestarr:

```sh
mkdir restore && tar --zstd -xf 20261015-033000-scheduled.tar.zst -C restore
```

Inside: `files/<n>/` for each backed up folder (the manifest says which is
which), `db/` for dumps, `native/` for app backups and `compose/` for the
stack files.

## Nodestarr's own database

Each scheduled run (and "Back up all now") also saves a consistent copy of
Nodestarr's database, with your settings, users, API keys and history, as
`<backup folder>/nodestarr-database/nodestarr-<date>.db`. As many copies are
kept as the archive retention (30 when that is unlimited). They contain
password hashes and API keys, so they are only readable by their owner.

To restore one, stop Nodestarr, replace its database, and start it again:

```sh
cd /opt/stacks/nodestarr
docker compose stop
rm -f config/nodestarr.db-wal config/nodestarr.db-shm
cp /mnt/backup/nodestarr/nodestarr-database/nodestarr-20261015-033000.db config/nodestarr.db
docker compose start
```

If you moved or lost the backup index, Backups > Reindex finds the
archives in the backup folder again from their manifests.
