//! One-click and scheduled backups, retention, hooks and restore.

pub mod archive;
pub mod plan;
pub mod restore;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::activity::{Actor, NewActivity};
use crate::app::App;
use crate::db::Db;
use crate::docker::{Entry, actions};
use crate::error::{AppError, Result};
use crate::events::EventBus;
use crate::notify::Notification;
use crate::settings::{Compression, HookWhen, Severity};
use crate::tasks::{TaskCtx, TaskSpec};
use crate::updates::check::display_name;

use self::archive::{ArchiveWriter, MANIFEST_NAME, build_globs};
use self::plan::{Access, Method};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackupKind {
    Manual,
    Scheduled,
    PreUpdate,
    PreRestore,
}

impl BackupKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BackupKind::Manual => "manual",
            BackupKind::Scheduled => "scheduled",
            BackupKind::PreUpdate => "pre-update",
            BackupKind::PreRestore => "pre-restore",
        }
    }
}

/// One item stored in an archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ManifestItem {
    Mount { destination: String, source: String, prefix: String, files: u64, bytes: u64 },
    Database { engine: String, path: String, bytes: u64 },
    Native { path: String, bytes: u64 },
    ComposeFile { original: String, path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub format: u32,
    pub nodestarr_version: String,
    pub identity: String,
    pub name: String,
    pub container: String,
    pub stack: Option<String>,
    pub service: Option<String>,
    pub image: String,
    pub app_version: Option<String>,
    pub created_at: i64,
    pub kind: BackupKind,
    pub method: Method,
    pub items: Vec<ManifestItem>,
    pub excludes: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupRecord {
    pub id: String,
    pub identity: String,
    pub target_name: Option<String>,
    pub path: String,
    pub size: i64,
    pub created_at: i64,
    pub kind: String,
    pub status: String,
    pub pinned: bool,
    pub manifest: serde_json::Value,
    /// The archive is still on disk.
    pub exists: bool,
}

#[derive(Clone)]
pub struct BackupService {
    db: Db,
    #[allow(dead_code)]
    bus: EventBus,
}

impl BackupService {
    pub fn new(db: Db, bus: EventBus) -> Self {
        Self { db, bus }
    }

    /// The backup folder exists and is writable.
    pub fn root_available(&self, app: &App) -> bool {
        let root = PathBuf::from(&app.settings.get().backups.root);
        if !root.is_dir() {
            return false;
        }
        let probe = root.join(format!(".nodestarr-write-test-{}", crate::util::short_id()));
        let ok = std::fs::write(&probe, b"ok").is_ok();
        let _ = std::fs::remove_file(&probe);
        ok
    }

    pub async fn list(&self, root: &Path, identity: Option<String>, limit: u32) -> Result<Vec<BackupRecord>> {
        let root = root.to_path_buf();
        self.db
            .call(move |c| {
                let mut sql = String::from(
                    "SELECT id, identity, target_name, path, size, created_at, kind, status, pinned, manifest FROM backups",
                );
                if identity.is_some() {
                    sql.push_str(" WHERE identity = ?1");
                }
                sql.push_str(&format!(" ORDER BY created_at DESC LIMIT {}", limit.clamp(1, 2000)));
                let mut stmt = c.prepare(&sql)?;
                let map = |r: &rusqlite::Row<'_>| {
                    let path: String = r.get(3)?;
                    let manifest: String = r.get(9)?;
                    Ok(BackupRecord {
                        id: r.get(0)?,
                        identity: r.get(1)?,
                        target_name: r.get(2)?,
                        exists: root.join(&path).is_file(),
                        path,
                        size: r.get(4)?,
                        created_at: r.get(5)?,
                        kind: r.get(6)?,
                        status: r.get(7)?,
                        pinned: r.get::<_, i64>(8)? != 0,
                        manifest: serde_json::from_str(&manifest).unwrap_or_default(),
                    })
                };
                let rows = match &identity {
                    Some(id) => stmt.query_map([id], map)?.collect::<std::result::Result<Vec<_>, _>>()?,
                    None => stmt.query_map([], map)?.collect::<std::result::Result<Vec<_>, _>>()?,
                };
                Ok(rows)
            })
            .await
    }

    pub async fn get(&self, root: &Path, id: &str) -> Result<BackupRecord> {
        let all = self.list(root, None, 2000).await?;
        all.into_iter().find(|b| b.id == id).ok_or_else(|| AppError::not_found("backup"))
    }

    /// Latest successful backup time per identity.
    pub async fn latest_by_identity(&self) -> Result<std::collections::HashMap<String, i64>> {
        self.db
            .call(|c| {
                let mut stmt = c.prepare("SELECT identity, MAX(created_at) FROM backups GROUP BY identity")?;
                let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
                Ok(rows.collect::<std::result::Result<_, _>>()?)
            })
            .await
    }

    pub async fn set_pinned(&self, id: &str, pinned: bool) -> Result<()> {
        let id = id.to_string();
        self.db
            .call(move |c| {
                let n = c.execute("UPDATE backups SET pinned = ?1 WHERE id = ?2", params![pinned as i64, id])?;
                if n == 0 { Err(AppError::not_found("backup")) } else { Ok(()) }
            })
            .await
    }

    pub async fn delete(&self, root: &Path, id: &str) -> Result<()> {
        let rec = self.get(root, id).await?;
        let file = root.join(&rec.path);
        let _ = tokio::fs::remove_file(&file).await;
        let _ = tokio::fs::remove_file(sidecar_path(&file)).await;
        let id = id.to_string();
        self.db
            .call(move |c| {
                c.execute("DELETE FROM backups WHERE id = ?1", [id])?;
                Ok(())
            })
            .await
    }

    async fn insert(&self, rec: &BackupRecord) -> Result<()> {
        let r = rec.clone();
        self.db
            .call(move |c| {
                c.execute(
                    "INSERT OR REPLACE INTO backups (id, identity, target_name, path, size, created_at, kind, status, manifest, pinned)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![r.id, r.identity, r.target_name, r.path, r.size, r.created_at, r.kind, r.status, r.manifest.to_string(), r.pinned as i64],
                )?;
                Ok(())
            })
            .await
    }

    /// Rebuilds the index from sidecar files on disk (after moving the
    /// backup folder or restoring Nodestarr's own database).
    pub async fn reindex(&self, root: &Path) -> Result<usize> {
        let root = root.to_path_buf();
        let found = tokio::task::spawn_blocking(move || {
            let mut out = Vec::new();
            fn walk(dir: &Path, out: &mut Vec<(PathBuf, serde_json::Value)>, depth: u32) {
                if depth > 4 {
                    return;
                }
                let Ok(rd) = std::fs::read_dir(dir) else { return };
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        walk(&p, out, depth + 1);
                    } else if p.to_string_lossy().ends_with(".nodestarr.json") {
                        if let Ok(text) = std::fs::read_to_string(&p) {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                                out.push((p, v));
                            }
                        }
                    }
                }
            }
            walk(&root, &mut out, 0);
            (root, out)
        })
        .await?;
        let (root, found) = found;
        let mut n = 0;
        for (sidecar, v) in found {
            let archive_path = PathBuf::from(sidecar.to_string_lossy().trim_end_matches(".nodestarr.json"));
            let Ok(meta) = std::fs::metadata(&archive_path) else { continue };
            let Some(id) = v.get("id").and_then(|x| x.as_str()).map(str::to_string) else { continue };
            let manifest = v.get("manifest").cloned().unwrap_or_default();
            let rec = BackupRecord {
                id,
                identity: manifest.get("identity").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
                target_name: manifest.get("name").and_then(|x| x.as_str()).map(str::to_string),
                path: archive_path.strip_prefix(&root).unwrap_or(&archive_path).to_string_lossy().into_owned(),
                size: meta.len() as i64,
                created_at: manifest.get("created_at").and_then(|x| x.as_i64()).unwrap_or(0),
                kind: manifest.get("kind").and_then(|x| x.as_str()).unwrap_or("manual").to_string(),
                status: v.get("status").and_then(|x| x.as_str()).unwrap_or("ok").to_string(),
                pinned: false,
                manifest,
                exists: true,
            };
            self.insert(&rec).await?;
            n += 1;
        }
        Ok(n)
    }
}

pub fn sidecar_path(archive: &Path) -> PathBuf {
    PathBuf::from(format!("{}.nodestarr.json", archive.display()))
}

/// Starts a backup task for one container.
pub fn start_backup(app: &App, key: &str, actor: &Actor) -> Result<String> {
    let entry = app.docker.get(key).ok_or_else(|| AppError::not_found("container"))?;
    let name = display_name(app, &entry);
    let spec =
        TaskSpec::new("backup", format!("Back up {name}"), actor).target(entry.summary.identity.clone(), name.clone());
    let app2 = app.clone();
    let actor = actor.clone();
    Ok(app.tasks.spawn(spec, move |ctx| async move {
        let rec = run_container_backup(&app2, &ctx, &entry, BackupKind::Manual).await;
        report(&app2, &name, &entry.summary.identity, &rec, &actor).await;
        let rec = rec?;
        run_hook(&app2, &ctx, true).await;
        Ok(json!({ "id": rec.id, "size": rec.size, "path": rec.path }))
    }))
}

async fn report(app: &App, name: &str, identity: &str, rec: &Result<BackupRecord>, actor: &Actor) {
    match rec {
        Ok(r) => {
            app.note(
                NewActivity::new(
                    "backup.completed",
                    Severity::Success,
                    format!("{name} backed up ({})", crate::util::human_bytes(r.size as u64)),
                )
                .target(identity, name)
                .actor(actor),
            )
            .await;
            app.notifier.send(
                Notification::new(
                    "backup.completed",
                    Severity::Success,
                    format!("{name} backed up"),
                    crate::util::human_bytes(r.size as u64),
                )
                .target(identity, name),
            );
        }
        Err(err) => {
            app.note(
                NewActivity::new("backup.failed", Severity::Error, format!("{name} backup failed: {err}"))
                    .target(identity, name)
                    .actor(actor),
            )
            .await;
            app.notifier.send(
                Notification::new("backup.failed", Severity::Error, format!("{name} backup failed"), err.to_string())
                    .target(identity, name),
            );
        }
    }
}

/// Starts a task that backs up every container of a stack.
pub fn start_stack_backup(app: &App, stack: &str, actor: &Actor) -> Result<String> {
    let entries = app.docker.in_stack(stack);
    if entries.is_empty() {
        return Err(AppError::not_found(format!("containers of stack '{stack}'")));
    }
    let spec = TaskSpec::new("backup", format!("Back up stack {stack}"), actor).lock(format!("stack:{stack}"));
    let app2 = app.clone();
    let actor = actor.clone();
    Ok(app.tasks.spawn(spec, move |ctx| async move {
        let mut ok = 0;
        let mut failed = Vec::new();
        let total = entries.len();
        for (i, entry) in entries.iter().enumerate() {
            ctx.check_cancelled()?;
            let name = display_name(&app2, entry);
            let plan = plan::plan(&app2, entry);
            if !plan.has_content() {
                ctx.log(format!("{name}: nothing to back up"));
                continue;
            }
            ctx.log(format!("— {name}"));
            let rec = run_container_backup(&app2, &ctx, entry, BackupKind::Manual).await;
            report(&app2, &name, &entry.summary.identity, &rec, &actor).await;
            match rec {
                Ok(_) => ok += 1,
                Err(e) => failed.push(format!("{name}: {e}")),
            }
            ctx.progress((i + 1) as f32 / total as f32);
        }
        run_hook(&app2, &ctx, failed.is_empty()).await;
        if failed.is_empty() {
            Ok(json!({ "backed_up": ok }))
        } else {
            Err(AppError::conflict(format!("{} failed: {}", failed.len(), failed.join("; "))))
        }
    }))
}

/// Pauses or stops a container for the duration of a copy and always
/// resumes it, even when the copy fails.
struct Quiesce {
    api: bollard::Docker,
    id: String,
    method: Method,
    was_running: bool,
}

impl Quiesce {
    async fn begin(
        api: &bollard::Docker,
        entry: &Entry,
        method: Method,
        ctx: &TaskCtx,
        timeout: Option<u32>,
    ) -> Result<Self> {
        let s = &entry.summary;
        let was_running = s.state == "running";
        let q = Self { api: api.clone(), id: s.id.clone(), method, was_running };
        if was_running {
            match method {
                Method::Pause => {
                    ctx.log("Pausing the container while its files are copied");
                    actions::pause(api, &s.id).await?;
                }
                Method::Stop => {
                    ctx.log("Stopping the container while its files are copied");
                    actions::stop(api, &s.id, timeout).await?;
                }
                _ => {}
            }
        }
        Ok(q)
    }

    async fn end(self, ctx: &TaskCtx) {
        if !self.was_running {
            return;
        }
        let res = match self.method {
            Method::Pause => actions::unpause(&self.api, &self.id).await,
            Method::Stop => actions::start(&self.api, &self.id).await,
            _ => Ok(()),
        };
        match res {
            Ok(()) if matches!(self.method, Method::Pause | Method::Stop) => ctx.log("Container resumed"),
            Ok(()) => {}
            Err(err) => ctx.log(format!("WARNING: could not resume the container: {err}")),
        }
    }
}

/// Backs up one container. Used by manual, scheduled and pre-update backups.
pub async fn run_container_backup(app: &App, ctx: &TaskCtx, entry: &Entry, kind: BackupKind) -> Result<BackupRecord> {
    let settings = app.settings.get();
    let root = PathBuf::from(&settings.backups.root);
    if !app.backups.root_available(app) {
        return Err(AppError::conflict(format!(
            "backup folder {} is missing or not writable (mount it into the Nodestarr container)",
            root.display()
        )));
    }
    let s = entry.summary.clone();
    let name = display_name(app, entry);
    let plan = plan::plan(app, entry);
    if !plan.has_content() {
        return Err(AppError::conflict("nothing to back up: no configuration folders or database found"));
    }
    for w in &plan.warnings {
        ctx.log(format!("Note: {w}"));
    }

    let id = crate::util::short_id();
    let group = s.stack.clone().unwrap_or_else(|| "standalone".into());
    let ts = chrono::Utc::now().with_timezone(&settings.tz()).format("%Y%m%d-%H%M%S").to_string();
    let ext = match settings.backups.compression {
        Compression::Zstd => "tar.zst",
        Compression::Gzip => "tar.gz",
    };
    let rel_dir = PathBuf::from(crate::util::slug(&group)).join(crate::util::slug(&s.identity));
    let dir = root.join(&rel_dir);
    tokio::fs::create_dir_all(&dir).await?;
    let file_name = format!("{ts}-{}.{ext}", kind.as_str());
    let final_path = dir.join(&file_name);
    let partial = dir.join(format!(".{file_name}.partial"));
    let work = dir.join(format!(".work-{id}"));
    tokio::fs::create_dir_all(&work).await?;

    let result = async {
        let mut items = Vec::new();
        let mut warnings = plan.warnings.clone();
        let mut extra_files: Vec<(PathBuf, String)> = Vec::new();

        // Database dump first (container keeps running; dumps are consistent).
        if let Some(db) = &plan.db {
            ctx.log(format!("Dumping database ({})", db.description));
            let (fname, engine) = match db.engine {
                crate::docker::apps::DbEngine::Mongo => ("mongo.archive", "mongo"),
                crate::docker::apps::DbEngine::Postgres => ("postgres.sql", "postgres"),
                _ => ("mysql.sql", "mysql"),
            };
            let dump = work.join(fname);
            let bytes = crate::docker::exec::to_file(&app.docker.api, &s.id, db.command.clone(), &dump).await?;
            ctx.log(format!("Database dump: {}", crate::util::human_bytes(bytes)));
            items.push(ManifestItem::Database { engine: engine.into(), path: format!("db/{fname}"), bytes });
            extra_files.push((dump, format!("db/{fname}")));
        }

        // The app's own backup (e.g. Sonarr's zip).
        if plan.native {
            ctx.log("Asking the app to create its own backup");
            match crate::integrations::native_backup(app, entry, &work).await {
                Ok(path) => {
                    let fname = path
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "backup.zip".into());
                    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    items.push(ManifestItem::Native { path: format!("native/{fname}"), bytes });
                    extra_files.push((path, format!("native/{fname}")));
                }
                Err(err) => {
                    ctx.log(format!("App backup failed ({err}); falling back to copying files"));
                    warnings.push(format!("native backup failed: {err}"));
                }
            }
        }
        ctx.progress(0.3);

        // Folders not mounted into Nodestarr: fetch through Docker before pausing.
        let mut docker_copies: Vec<(usize, PathBuf)> = Vec::new();
        let include_files = !(plan.native && items.iter().any(|i| matches!(i, ManifestItem::Native { .. })));
        let mounts: Vec<&plan::PlannedMount> = plan.mounts.iter().filter(|m| m.include && include_files).collect();
        for (idx, m) in mounts.iter().enumerate() {
            if m.access == Access::DockerCopy {
                ctx.log(format!("Copying {} through Docker", m.destination));
                let tmp = work.join(format!("mount-{idx}.tar"));
                download_from_container(app, &s.id, &m.destination, &tmp).await?;
                docker_copies.push((idx, tmp));
            }
        }

        let method = plan.method;
        let quiesce = if include_files && mounts.iter().any(|m| m.access == Access::Direct) {
            Some(Quiesce::begin(&app.docker.api, entry, method, ctx, crate::updates::apply::stop_timeout(app)).await?)
        } else {
            None
        };

        let compression = settings.backups.compression;
        let level = settings.backups.zstd_level;
        let excludes = plan.excludes.clone();
        let mounts_owned: Vec<plan::PlannedMount> = mounts.iter().map(|m| (*m).clone()).collect();
        let compose_files = plan.compose_files.clone();
        let partial2 = partial.clone();
        let cancel = ctx.cancel_token();
        let ctx2 = ctx.clone();
        let archive_result =
            tokio::task::spawn_blocking(move || -> std::io::Result<(ArchiveWriter, Vec<ManifestItem>, Vec<String>)> {
                let globs = build_globs(&excludes);
                let mut w = ArchiveWriter::create(&partial2, compression, level)?;
                let mut out_items = Vec::new();
                let mut warns = Vec::new();
                for (idx, m) in mounts_owned.iter().enumerate() {
                    let prefix = format!("files/{idx}/");
                    let stats = if m.access == Access::Direct {
                        ctx2.log(format!("Archiving {} ({})", m.destination, m.source));
                        let mut warn = |msg: String| {
                            if warns.len() < 50 {
                                warns.push(msg);
                            }
                        };
                        w.add_dir(Path::new(&m.source), &prefix, &globs, &cancel, &mut warn)?
                    } else {
                        let Some((_, tar_path)) = docker_copies.iter().find(|(i, _)| *i == idx) else { continue };
                        let f = std::fs::File::open(tar_path)?;
                        w.add_tar_stream(std::io::BufReader::new(f), &prefix, &globs)?
                    };
                    ctx2.log(format!(
                        "{}: {} files, {}",
                        m.destination,
                        stats.files,
                        crate::util::human_bytes(stats.bytes)
                    ));
                    out_items.push(ManifestItem::Mount {
                        destination: m.destination.clone(),
                        source: m.source.clone(),
                        prefix,
                        files: stats.files,
                        bytes: stats.bytes,
                    });
                }
                Ok((w, out_items, warns))
            })
            .await?;

        if let Some(q) = quiesce {
            q.end(ctx).await;
        }
        let (mut writer, mount_items, walk_warnings) =
            archive_result.map_err(|e| AppError::conflict(format!("archiving failed: {e}")))?;
        items.extend(mount_items);
        warnings.extend(walk_warnings);
        ctx.progress(0.8);

        let manifest = BackupManifest {
            format: 1,
            nodestarr_version: crate::VERSION.to_string(),
            identity: s.identity.clone(),
            name: name.clone(),
            container: s.name.clone(),
            stack: s.stack.clone(),
            service: s.service.clone(),
            image: s.image.clone(),
            app_version: s.version.clone(),
            created_at: crate::util::now(),
            kind,
            method,
            items: {
                let mut all = items;
                for f in &compose_files {
                    let fname = Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    all.push(ManifestItem::ComposeFile { original: f.clone(), path: format!("compose/{fname}") });
                }
                all
            },
            excludes: plan.excludes.clone(),
            warnings,
        };
        let manifest_json = serde_json::to_vec_pretty(&manifest)?;
        let extra = extra_files.clone();
        let compose_files2 = compose_files.clone();
        let partial3 = partial.clone();
        let final2 = final_path.clone();
        let size = tokio::task::spawn_blocking(move || -> std::io::Result<u64> {
            for (src, name) in &extra {
                writer.add_file(src, name)?;
            }
            for f in &compose_files2 {
                let fname = Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                writer.add_file(Path::new(f), &format!("compose/{fname}"))?;
            }
            writer.add_bytes(MANIFEST_NAME, &manifest_json)?;
            writer.finish()?;
            std::fs::File::open(&partial3)?.sync_all()?;
            std::fs::rename(&partial3, &final2)?;
            Ok(std::fs::metadata(&final2)?.len())
        })
        .await?
        .map_err(|e| AppError::conflict(format!("writing the archive failed: {e}")))?;

        let rec = BackupRecord {
            id: id.clone(),
            identity: s.identity.clone(),
            target_name: Some(name.clone()),
            path: rel_dir.join(&file_name).to_string_lossy().into_owned(),
            size: size as i64,
            created_at: manifest.created_at,
            kind: kind.as_str().into(),
            status: if manifest.warnings.is_empty() { "ok".into() } else { "partial".into() },
            pinned: false,
            manifest: serde_json::to_value(&manifest)?,
            exists: true,
        };
        let sidecar = json!({ "id": rec.id, "status": rec.status, "manifest": rec.manifest });
        tokio::fs::write(sidecar_path(&final_path), serde_json::to_vec_pretty(&sidecar)?).await?;
        app.backups.insert(&rec).await?;
        ctx.log(format!("Backup written: {} ({})", rec.path, crate::util::human_bytes(size)));
        Ok::<BackupRecord, AppError>(rec)
    }
    .await;

    let _ = tokio::fs::remove_dir_all(&work).await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&partial).await;
    }
    let rec = result?;
    if let Err(err) = apply_retention(app, &s.identity).await {
        ctx.log(format!("Retention cleanup failed: {err}"));
    }
    Ok(rec)
}

/// Streams a folder out of a container into a tar file.
async fn download_from_container(app: &App, container: &str, path: &str, dest: &Path) -> Result<()> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;
    let mut stream = app.docker.api.download_from_container(
        container,
        Some(bollard::query_parameters::DownloadFromContainerOptions { path: path.to_string() }),
    );
    let mut f = tokio::io::BufWriter::new(tokio::fs::File::create(dest).await?);
    while let Some(chunk) = stream.next().await {
        f.write_all(&chunk?).await?;
    }
    f.flush().await?;
    Ok(())
}

/// Deletes archives beyond the retention settings (pinned ones are kept).
pub async fn apply_retention(app: &App, identity: &str) -> Result<usize> {
    let settings = app.settings.get();
    let root = PathBuf::from(&settings.backups.root);
    let prefs = app.prefs.get(identity);
    let keep = prefs.backup.retention_count.unwrap_or(settings.backups.retention_count) as usize;
    let max_age_days = settings.backups.retention_days as i64;
    let pre_update_days = settings.backups.pre_update_retention_days as i64;
    let now = crate::util::now();
    let list = app.backups.list(&root, Some(identity.to_string()), 2000).await?;
    let (pre, regular): (Vec<_>, Vec<_>) =
        list.into_iter().filter(|b| !b.pinned).partition(|b| b.kind == "pre-update" || b.kind == "pre-restore");
    let mut doomed = Vec::new();
    for (i, b) in regular.iter().enumerate() {
        let too_many = keep > 0 && i >= keep;
        let too_old = max_age_days > 0 && now - b.created_at > max_age_days * 86400 && i > 0;
        if too_many || too_old {
            doomed.push(b.id.clone());
        }
    }
    for (i, b) in pre.iter().enumerate() {
        // Always keep the newest pre-update backup.
        if i > 0 && pre_update_days > 0 && now - b.created_at > pre_update_days * 86400 {
            doomed.push(b.id.clone());
        }
        if i >= 10 {
            doomed.push(b.id.clone());
        }
    }
    doomed.sort();
    doomed.dedup();
    for id in &doomed {
        app.backups.delete(&root, id).await?;
    }
    Ok(doomed.len())
}

/// Runs the post-backup hook.
pub async fn run_hook(app: &App, ctx: &TaskCtx, success: bool) {
    let settings = app.settings.get();
    let hook = &settings.backups.hook;
    if !success && hook.when == HookWhen::Success {
        return;
    }
    let timeout = std::time::Duration::from_secs(hook.timeout_secs.max(10) as u64);
    if !hook.command.trim().is_empty() {
        ctx.log(format!("Running post-backup hook: {}", hook.command));
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c")
            .arg(&hook.command)
            .env("NODESTARR_BACKUP_ROOT", &settings.backups.root)
            .env("NODESTARR_BACKUP_STATUS", if success { "ok" } else { "failed" })
            .kill_on_drop(true);
        match tokio::time::timeout(timeout, cmd.output()).await {
            Ok(Ok(out)) => {
                for line in String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .chain(String::from_utf8_lossy(&out.stderr).lines())
                    .take(200)
                {
                    ctx.log(format!("hook: {line}"));
                }
                if !out.status.success() {
                    ctx.log(format!("Hook exited with {}", out.status));
                    app.notifier.send(Notification::new(
                        "backup.failed",
                        Severity::Error,
                        "Post-backup hook failed",
                        format!("{} exited with {}", hook.command, out.status),
                    ));
                }
            }
            Ok(Err(err)) => ctx.log(format!("Hook could not start: {err}")),
            Err(_) => ctx.log("Hook timed out"),
        }
    }
    if !hook.exec_container.trim().is_empty() && !hook.exec_command.trim().is_empty() {
        ctx.log(format!("Running '{}' in {}", hook.exec_command, hook.exec_container));
        let cmd = vec!["sh".to_string(), "-c".to_string(), hook.exec_command.clone()];
        match tokio::time::timeout(
            timeout,
            crate::docker::exec::capture(&app.docker.api, &hook.exec_container, cmd, None),
        )
        .await
        {
            Ok(Ok(out)) => {
                for line in out.stdout.lines().chain(out.stderr.lines()).take(200) {
                    ctx.log(format!("hook: {line}"));
                }
                if out.exit_code != Some(0) {
                    app.notifier.send(Notification::new(
                        "backup.failed",
                        Severity::Error,
                        "Post-backup hook failed",
                        format!("exit code {:?}", out.exit_code),
                    ));
                }
            }
            Ok(Err(err)) => ctx.log(format!("Hook failed: {err}")),
            Err(_) => ctx.log("Hook timed out"),
        }
    }
}

/// Scheduled run over every container with backups enabled.
/// Backs up every container that has backups enabled (the scheduled job;
/// "Run now" uses it too, even when the schedule itself is off).
pub async fn run_scheduled(app: &App) {
    let settings = app.settings.get();
    let actor = Actor::schedule();
    let entries: Vec<Arc<Entry>> = app
        .docker
        .entries()
        .into_iter()
        .filter(|e| app.prefs.get(&e.summary.identity).backup_enabled(&settings))
        .filter(|e| !e.summary.is_self)
        .collect();
    let total = entries.len();
    let mut ok = 0;
    let mut failed = Vec::new();
    for entry in entries {
        if !plan::plan(app, &entry).has_content() {
            continue;
        }
        let name = display_name(app, &entry);
        let spec = TaskSpec::new("backup", format!("Scheduled backup of {name}"), &actor)
            .target(entry.summary.identity.clone(), name.clone());
        let app2 = app.clone();
        let e2 = entry.clone();
        let res = app
            .tasks
            .run(spec, move |ctx| async move {
                let r = run_container_backup(&app2, &ctx, &e2, BackupKind::Scheduled).await?;
                Ok(json!({ "id": r.id, "size": r.size }))
            })
            .await;
        match res {
            Ok(_) => ok += 1,
            Err(err) => failed.push(format!("{name}: {err}")),
        }
    }
    if total == 0 {
        return;
    }
    // Hook once per run.
    let spec = TaskSpec::new("backup", "Post-backup hook", &actor);
    let app2 = app.clone();
    let all_ok = failed.is_empty();
    let has_hook =
        !settings.backups.hook.command.trim().is_empty() || !settings.backups.hook.exec_command.trim().is_empty();
    if has_hook {
        let _ = app
            .tasks
            .run(spec, move |ctx| async move {
                run_hook(&app2, &ctx, all_ok).await;
                Ok(serde_json::Value::Null)
            })
            .await;
    }
    let severity = if failed.is_empty() { Severity::Success } else { Severity::Error };
    let message = if failed.is_empty() {
        format!("{ok} container{} backed up", if ok == 1 { "" } else { "s" })
    } else {
        format!("{ok} backed up, {} failed:\n{}", failed.len(), failed.join("\n"))
    };
    app.note(
        NewActivity::new(
            if failed.is_empty() { "backup.scheduled" } else { "backup.failed" },
            severity,
            format!("Scheduled backup: {message}"),
        )
        .actor(&actor),
    )
    .await;
    if !failed.is_empty() {
        app.notifier.send(
            Notification::new("backup.failed", Severity::Error, "Scheduled backup had failures", message)
                .link(app, "/backups"),
        );
    } else {
        app.notifier.send(Notification::new(
            "backup.completed",
            Severity::Success,
            "Scheduled backup finished",
            message,
        ));
    }
}
