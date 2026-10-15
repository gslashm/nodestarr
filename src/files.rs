//! File editing with history: compose files, `.env` files and app config
//! files (config.xml, sabnzbd.ini, ...).
//!
//! Every save through Nodestarr stores the previous content (compressed) so any
//! edit can be reviewed and undone. Access to app config folders is confined
//! to the container's own bind mounts.

use std::path::{Path, PathBuf};

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::activity::Actor;
use crate::db::Db;
use crate::docker::Entry;
use crate::error::{AppError, Result};
use crate::util::fsx;

/// Files larger than this open read-only in the editor.
pub const MAX_EDIT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct FileVersion {
    pub id: i64,
    pub path: String,
    pub ts: i64,
    pub actor: Option<String>,
    pub size: i64,
    pub note: Option<String>,
}

/// Records `previous` as a version of `path`.
pub async fn record_version(db: &Db, path: &str, previous: &[u8], actor: &Actor, note: Option<String>) -> Result<()> {
    let compressed = zstd::bulk::compress(previous, 3).map_err(|e| AppError::Other(e.into()))?;
    let (path, actor, size) = (path.to_string(), actor.0.clone(), previous.len() as i64);
    db.call(move |c| {
        c.execute(
            "INSERT INTO file_versions (path, ts, actor, size, content, note) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![path, crate::util::now(), actor, size, compressed, note],
        )?;
        Ok(())
    })
    .await
}

pub async fn list_versions(db: &Db, path: &str) -> Result<Vec<FileVersion>> {
    let path = path.to_string();
    db.call(move |c| {
        let mut stmt = c.prepare(
            "SELECT id, path, ts, actor, size, note FROM file_versions WHERE path = ?1 ORDER BY ts DESC, id DESC LIMIT 50",
        )?;
        let rows = stmt.query_map([&path], |r| {
            Ok(FileVersion {
                id: r.get(0)?,
                path: r.get(1)?,
                ts: r.get(2)?,
                actor: r.get(3)?,
                size: r.get(4)?,
                note: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    })
    .await
}

pub async fn version_content(db: &Db, id: i64) -> Result<(String, String)> {
    let row: Option<(String, i64, Vec<u8>)> = db
        .call(move |c| {
            use rusqlite::OptionalExtension;
            Ok(c.query_row("SELECT path, size, content FROM file_versions WHERE id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?)
        })
        .await?;
    let (path, size, blob) = row.ok_or_else(|| AppError::not_found("file version"))?;
    // `size` is the original length, so the buffer is allocated exactly once.
    let raw = zstd::bulk::decompress(&blob, size.max(0) as usize).map_err(|e| AppError::Other(e.into()))?;
    Ok((path, String::from_utf8_lossy(&raw).into_owned()))
}

/// Writes `content` to `path` atomically, recording the old content first.
/// Returns false when the content was unchanged.
pub async fn write_with_history(
    db: &Db,
    path: &Path,
    content: &str,
    actor: &Actor,
    note: Option<String>,
) -> Result<bool> {
    let previous = match tokio::fs::read(path).await {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    if previous.as_deref() == Some(content.as_bytes()) {
        return Ok(false);
    }
    if let Some(prev) = &previous {
        record_version(db, &path.to_string_lossy(), prev, actor, note).await?;
    }
    let path_owned = path.to_path_buf();
    let data = content.as_bytes().to_vec();
    tokio::task::spawn_blocking(move || fsx::atomic_write(&path_owned, &data)).await??;
    Ok(true)
}

/// Opaque version tag for optimistic concurrency (mtime + size).
pub fn etag(meta: &std::fs::Metadata) -> String {
    use std::os::unix::fs::MetadataExt;
    format!("{:x}-{:x}", meta.mtime_nsec() as u64 ^ (meta.mtime() as u64) << 20, meta.size())
}

// ---------------------------------------------------------------------------
// Config folder browser
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct BrowseRoot {
    /// Container path of the mount (the root's id in requests).
    pub destination: String,
    /// Host path.
    pub source: String,
    /// Whether Nodestarr can see this path (it must be mounted into Nodestarr at the same path).
    pub accessible: bool,
    /// Looks like media/bulk data rather than configuration.
    pub data: bool,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirEntry {
    pub name: String,
    /// "file", "dir" or "link".
    pub kind: &'static str,
    pub size: u64,
    pub modified: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileContent {
    pub path: String,
    pub content: Option<String>,
    pub size: u64,
    pub modified: Option<i64>,
    pub etag: String,
    pub binary: bool,
    pub too_large: bool,
    pub language: &'static str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveFile {
    pub root: String,
    pub path: String,
    pub content: String,
    /// The etag the client loaded; saving fails if the file changed since.
    pub etag: Option<String>,
}

/// Bind mounts of a container that can be browsed.
pub fn roots(entry: &Entry, data_roots: &[String]) -> Vec<BrowseRoot> {
    let app = entry.summary.app.as_deref().and_then(crate::docker::apps::by_slug);
    entry
        .summary
        .mounts
        .iter()
        .filter(|m| m.kind == "bind")
        .map(|m| {
            let is_data_root = data_roots.iter().any(|r| Path::new(&m.source).starts_with(r));
            let is_app_data =
                app.is_some_and(|a| a.data_paths.iter().any(|d| Path::new(&m.destination).starts_with(d)));
            BrowseRoot {
                destination: m.destination.clone(),
                source: m.source.clone(),
                accessible: Path::new(&m.source).is_dir(),
                data: is_data_root || is_app_data,
                read_only: !m.rw,
            }
        })
        .filter(|r| Path::new(&r.source).is_dir() || !Path::new(&r.source).exists())
        .collect()
}

fn root_for(entry: &Entry, destination: &str) -> Result<PathBuf> {
    let m = entry
        .summary
        .mounts
        .iter()
        .find(|m| m.kind == "bind" && m.destination == destination)
        .ok_or_else(|| AppError::not_found(format!("mount {destination}")))?;
    let p = PathBuf::from(&m.source);
    if !p.is_dir() {
        return Err(AppError::conflict(format!(
            "{} is not visible to Nodestarr; mount it into the Nodestarr container at the same path",
            m.source
        )));
    }
    Ok(p)
}

pub async fn list_dir(entry: &Entry, root: &str, rel: &str) -> Result<Vec<DirEntry>> {
    let base = root_for(entry, root)?;
    let rel = rel.to_string();
    tokio::task::spawn_blocking(move || {
        let dir = fsx::confine(&base, &rel)?;
        let mut out = Vec::new();
        for item in std::fs::read_dir(&dir)?.flatten().take(5000) {
            let Ok(meta) = item.path().symlink_metadata() else { continue };
            let kind = if meta.file_type().is_symlink() {
                "link"
            } else if meta.is_dir() {
                "dir"
            } else {
                "file"
            };
            use std::os::unix::fs::MetadataExt;
            out.push(DirEntry {
                name: item.file_name().to_string_lossy().into_owned(),
                kind,
                size: if meta.is_file() { meta.len() } else { 0 },
                modified: Some(meta.mtime()),
            });
        }
        out.sort_by(|a, b| {
            (a.kind != "dir").cmp(&(b.kind != "dir")).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(out)
    })
    .await?
}

pub fn language_for(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "yml" | "yaml" => "yaml",
        "json" => "json",
        "xml" | "plist" => "xml",
        "ini" | "conf" | "cfg" | "cnf" | "toml" | "properties" => "ini",
        "sh" | "bash" => "shell",
        "py" => "python",
        "js" | "mjs" => "javascript",
        "env" => "ini",
        _ if lower.ends_with(".env") || lower.ends_with("/.env") || lower == ".env" => "ini",
        _ => "text",
    }
}

pub async fn read_file(entry: &Entry, root: &str, rel: &str) -> Result<FileContent> {
    let base = root_for(entry, root)?;
    let rel = rel.to_string();
    tokio::task::spawn_blocking(move || {
        let path = fsx::confine(&base, &rel)?;
        let meta = std::fs::metadata(&path)?;
        if !meta.is_file() {
            return Err(AppError::bad_request("not a regular file"));
        }
        use std::os::unix::fs::MetadataExt;
        let too_large = meta.len() > MAX_EDIT_BYTES;
        let (content, binary) = if too_large {
            (None, false)
        } else {
            let bytes = std::fs::read(&path)?;
            if fsx::looks_binary(&bytes) {
                (None, true)
            } else {
                (Some(String::from_utf8_lossy(&bytes).into_owned()), false)
            }
        };
        Ok(FileContent {
            path: path.to_string_lossy().into_owned(),
            content,
            size: meta.len(),
            modified: Some(meta.mtime()),
            etag: etag(&meta),
            binary,
            too_large,
            language: language_for(&rel),
        })
    })
    .await?
}

/// Saves a file inside a container's mount, checking the etag first.
pub async fn save_file(db: &Db, entry: &Entry, req: &SaveFile, actor: &Actor) -> Result<FileContent> {
    let base = root_for(entry, &req.root)?;
    let rel = req.path.clone();
    let path = tokio::task::spawn_blocking(move || fsx::confine(&base, &rel)).await??;
    if let (Some(expected), Ok(meta)) = (&req.etag, std::fs::metadata(&path))
        && &etag(&meta) != expected
    {
        return Err(AppError::conflict("the file changed on disk since you opened it; reload to see the new version"));
    }
    if req.content.len() as u64 > MAX_EDIT_BYTES {
        return Err(AppError::bad_request("file is too large to save from the editor"));
    }
    write_with_history(db, &path, &req.content, actor, Some(format!("edited in {}", entry.summary.name))).await?;
    read_file(entry, &req.root, &req.path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn history_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("compose.yaml");
        let actor = Actor::user("jake");
        assert!(write_with_history(&db, &p, "v1", &actor, None).await.unwrap());
        assert!(!write_with_history(&db, &p, "v1", &actor, None).await.unwrap());
        assert!(write_with_history(&db, &p, "v2", &actor, None).await.unwrap());
        let versions = list_versions(&db, &p.to_string_lossy()).await.unwrap();
        assert_eq!(versions.len(), 1);
        let (_, content) = version_content(&db, versions[0].id).await.unwrap();
        assert_eq!(content, "v1");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "v2");
    }

    #[test]
    fn languages() {
        assert_eq!(language_for("config.xml"), "xml");
        assert_eq!(language_for("sabnzbd.ini"), "ini");
        assert_eq!(language_for("compose.yaml"), "yaml");
        assert_eq!(language_for(".env"), "ini");
    }
}
