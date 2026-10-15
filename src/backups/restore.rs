//! Restoring a container from a backup archive.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::json;

use crate::activity::{Actor, NewActivity};
use crate::app::App;
use crate::backups::archive::{self, RestoreTarget, build_globs};
use crate::backups::{BackupKind, BackupManifest, ManifestItem, run_container_backup};
use crate::docker::actions;
use crate::error::{AppError, Result};
use crate::notify::Notification;
use crate::settings::Severity;
use crate::tasks::{TaskCtx, TaskSpec};
use crate::updates::check::display_name;

#[derive(Debug, Clone, Deserialize)]
pub struct RestoreOptions {
    /// Delete files that are not in the backup (excluded paths are kept).
    #[serde(default)]
    pub replace: bool,
    /// Restore the database dump.
    #[serde(default = "yes")]
    pub database: bool,
    /// Restore folder contents.
    #[serde(default = "yes")]
    pub files: bool,
    /// Take a safety backup of the current state first.
    #[serde(default = "yes")]
    pub safety_backup: bool,
}

fn yes() -> bool {
    true
}

impl Default for RestoreOptions {
    fn default() -> Self {
        Self {
            replace: false,
            database: true,
            files: true,
            safety_backup: true,
        }
    }
}

pub async fn start_restore(app: &App, backup_id: &str, opts: RestoreOptions, actor: &Actor) -> Result<String> {
    let root = PathBuf::from(&app.settings.get().backups.root);
    let rec = app.backups.get(&root, backup_id).await?;
    let name = rec.target_name.clone().unwrap_or_else(|| "backup".into());
    let spec = TaskSpec::new("restore", format!("Restore {name}"), actor).target(rec.identity.clone(), name);
    let app2 = app.clone();
    let actor2 = actor.clone();
    let backup_id = backup_id.to_string();
    Ok(app.tasks.spawn(spec, move |ctx| async move { run_restore(&app2, &ctx, &backup_id, opts, &actor2).await }))
}

async fn run_restore(app: &App, ctx: &TaskCtx, backup_id: &str, opts: RestoreOptions, actor: &Actor) -> Result<serde_json::Value> {
    let root = PathBuf::from(&app.settings.get().backups.root);
    let rec = app.backups.get(&root, backup_id).await?;
    let archive_path = root.join(&rec.path);
    if !archive_path.is_file() {
        return Err(AppError::not_found(format!("archive {}", archive_path.display())));
    }
    let manifest: BackupManifest = serde_json::from_value(rec.manifest.clone())
        .map_err(|e| AppError::conflict(format!("backup manifest is unreadable: {e}")))?;
    let entry = app.docker.by_identity(&manifest.identity).ok_or_else(|| {
        AppError::conflict(format!(
            "container {} doesn't exist; deploy it first (its compose file is inside the backup)",
            manifest.container
        ))
    })?;
    let name = display_name(app, &entry);

    if opts.safety_backup {
        ctx.log("Taking a safety backup of the current state");
        if let Err(err) = run_container_backup(app, ctx, &entry, BackupKind::PreRestore).await {
            return Err(AppError::conflict(format!("safety backup failed, nothing was changed: {err}")));
        }
    }
    ctx.check_cancelled()?;

    let s = entry.summary.clone();
    let was_running = s.is_running();
    let has_files = opts.files
        && manifest
            .items
            .iter()
            .any(|i| matches!(i, ManifestItem::Mount { .. } | ManifestItem::Native { .. }));
    // Database containers keep running: the dump is replayed into them.
    let is_db_only = manifest.items.iter().all(|i| matches!(i, ManifestItem::Database { .. } | ManifestItem::ComposeFile { .. }));
    if was_running && has_files && !is_db_only {
        ctx.log(format!("Stopping {name}"));
        actions::stop(&app.docker.api, &s.id, crate::updates::apply::stop_timeout(app)).await?;
    }

    let work = root.join(format!(".restore-{}", crate::util::short_id()));
    tokio::fs::create_dir_all(&work).await?;
    let outcome = async {
        // Map archived mounts to the container's current mounts.
        let mut targets = Vec::new();
        let mut docker_targets: Vec<(String, String)> = Vec::new();
        if opts.files {
            for item in &manifest.items {
                if let ManifestItem::Mount { destination, prefix, .. } = item {
                    match s.mounts.iter().find(|m| &m.destination == destination) {
                        Some(m) if m.kind == "bind" && Path::new(&m.source).is_dir() => targets.push(RestoreTarget {
                            prefix: prefix.clone(),
                            dest: PathBuf::from(&m.source),
                        }),
                        Some(_) => docker_targets.push((prefix.clone(), destination.clone())),
                        None => ctx.log(format!("Skipping {destination}: the container no longer mounts it")),
                    }
                }
            }
        }
        let keep = build_globs(&manifest.excludes);
        let replace = opts.replace;
        let archive2 = archive_path.clone();
        let work2 = work.clone();
        let cancel = ctx.cancel_token();
        let mut extract_prefixes: Vec<&'static str> = vec!["native/"];
        if opts.database {
            extract_prefixes.push("db/");
        }
        let docker_prefixes: Vec<String> = docker_targets.iter().map(|(p, _)| p.clone()).collect();
        let ctx2 = ctx.clone();
        let stats = tokio::task::spawn_blocking(move || {
            ctx2.log("Extracting files");
            let mut all_targets = targets;
            for p in &docker_prefixes {
                all_targets.push(RestoreTarget {
                    prefix: p.clone(),
                    dest: work2.join("docker").join(p.trim_end_matches('/').replace('/', "_")),
                });
            }
            archive::restore(&archive2, &all_targets, &work2, &extract_prefixes, replace, &keep, &cancel)
        })
        .await?
        .map_err(|e| AppError::conflict(format!("extracting failed: {e}")))?;
        ctx.log(format!(
            "Restored {} files ({}){}",
            stats.files,
            crate::util::human_bytes(stats.bytes),
            if stats.removed > 0 { format!(", removed {} stale files", stats.removed) } else { String::new() }
        ));

        // Mounts Nodestarr can't see directly go back through Docker.
        for (prefix, destination) in &docker_targets {
            let src = work.join("docker").join(prefix.trim_end_matches('/').replace('/', "_"));
            ctx.log(format!("Uploading {destination} through Docker"));
            upload_dir(app, &s.id, &src, destination).await?;
        }

        // Native app backups are unpacked into the config folder.
        for item in &manifest.items {
            if let ManifestItem::Native { path, .. } = item {
                let zip_path = work.join(path);
                let config = s
                    .mounts
                    .iter()
                    .find(|m| m.destination == "/config" && Path::new(&m.source).is_dir())
                    .map(|m| PathBuf::from(&m.source));
                match config {
                    Some(dir) => {
                        ctx.log("Unpacking the app's own backup into /config");
                        let zp = zip_path.clone();
                        tokio::task::spawn_blocking(move || unzip_into(&zp, &dir)).await??;
                    }
                    None => ctx.log("The app backup could not be unpacked: /config is not visible to Nodestarr"),
                }
            }
        }

        // Database dumps are replayed into the running database.
        if opts.database {
            for item in &manifest.items {
                if let ManifestItem::Database { engine, path, .. } = item {
                    let dump = work.join(path);
                    let info = s.app.as_deref().and_then(crate::docker::apps::by_slug);
                    let plan = info.and_then(|a| a.db).and_then(|e| crate::backups::plan::db_plan(e, &entry));
                    let Some(plan) = plan else {
                        ctx.log(format!("Cannot restore the {engine} dump: database credentials not found"));
                        continue;
                    };
                    if !app.docker.get(&s.id).is_some_and(|e| e.summary.is_running()) {
                        actions::start(&app.docker.api, &s.id).await?;
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                    ctx.log(format!("Replaying the {engine} dump"));
                    let out = crate::docker::exec::from_file(&app.docker.api, &s.id, plan.restore_command.clone(), &dump).await?;
                    let errors = out.lines().filter(|l| l.contains("ERROR")).count();
                    if errors > 0 {
                        ctx.log(format!("{errors} statements reported errors (often harmless 'already exists' notices)"));
                    }
                }
            }
        }
        Ok::<_, AppError>(stats)
    }
    .await;
    let _ = tokio::fs::remove_dir_all(&work).await;

    if was_running && !app.docker.get(&s.id).is_some_and(|e| e.summary.is_running()) {
        ctx.log(format!("Starting {name}"));
        if let Err(err) = actions::start(&app.docker.api, &s.id).await {
            ctx.log(format!("Could not start the container: {err}"));
        }
    }
    match outcome {
        Ok(stats) => {
            app.note(
                NewActivity::new("restore.completed", Severity::Success, format!("{name} restored from the backup of {}", fmt_time(app, manifest.created_at)))
                    .target(&manifest.identity, &name)
                    .actor(actor),
            )
            .await;
            app.notifier.send(Notification::new("restore.completed", Severity::Success, format!("{name} restored"), format!("From the backup taken {}", fmt_time(app, manifest.created_at))).target(&manifest.identity, &name));
            Ok(json!({ "files": stats.files, "bytes": stats.bytes, "removed": stats.removed }))
        }
        Err(err) => {
            app.note(
                NewActivity::new("restore.failed", Severity::Error, format!("{name} restore failed: {err}"))
                    .target(&manifest.identity, &name)
                    .actor(actor),
            )
            .await;
            app.notifier.send(Notification::new("restore.failed", Severity::Error, format!("{name} restore failed"), err.to_string()).target(&manifest.identity, &name));
            Err(err)
        }
    }
}

fn fmt_time(app: &App, ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.with_timezone(&app.settings.get().tz()).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

/// Packs a local folder as tar and uploads it into the container path.
async fn upload_dir(app: &App, container: &str, src: &Path, dest: &str) -> Result<()> {
    let src = src.to_path_buf();
    let data = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
        let mut b = tar::Builder::new(Vec::new());
        b.follow_symlinks(false);
        b.append_dir_all(".", &src)?;
        b.into_inner()
    })
    .await??;
    app.docker
        .api
        .upload_to_container(
            container,
            Some(bollard::query_parameters::UploadToContainerOptions {
                path: dest.to_string(),
                no_overwrite_dir_non_dir: None,
                copy_uidgid: Some("1".into()),
            }),
            bollard::body_full(data.into()),
        )
        .await?;
    Ok(())
}

fn unzip_into(zip_path: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| AppError::conflict(format!("bad zip: {e}")))?;
    let mut restored = std::collections::HashSet::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| AppError::conflict(format!("bad zip: {e}")))?;
        let Some(rel) = f.enclosed_name() else { continue };
        let out = dest.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        restored.insert(out.clone());
        let tmp = out.with_extension("nodestarr-restore-tmp");
        {
            let mut w = std::fs::File::create(&tmp)?;
            std::io::copy(&mut f, &mut w)?;
        }
        if let Ok(meta) = std::fs::metadata(&out) {
            use std::os::unix::fs::MetadataExt;
            let _ = std::os::unix::fs::chown(&tmp, Some(meta.uid()), Some(meta.gid()));
        }
        std::fs::rename(&tmp, &out)?;
    }
    super::archive::drop_stale_sqlite_sidecars(&restored);
    Ok(())
}
