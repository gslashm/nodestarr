//! Backup archives: tar streams compressed with zstd (or gzip).
//!
//! Layout inside an archive:
//!
//! ```text
//! files/<n>/...            contents of the n-th backed-up mount
//! db/<engine>.sql          database dump (or db/mongo.archive)
//! native/<file>.zip        the app's own backup (e.g. Sonarr)
//! compose/<file>           compose file(s) and .env of the stack
//! nodestarr-backup.json        manifest (also stored next to the archive)
//! ```

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use tokio_util::sync::CancellationToken;

use crate::settings::Compression;

pub const MANIFEST_NAME: &str = "nodestarr-backup.json";
/// Files up to this size are read into memory before archiving so a file
/// that changes during a live copy can't corrupt the tar stream.
const BUFFER_LIMIT: u64 = 64 * 1024 * 1024;

pub fn build_globs(patterns: &[String]) -> GlobSet {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        if let Ok(g) = GlobBuilder::new(p).literal_separator(false).build() {
            b.add(g);
        }
    }
    b.build().unwrap_or_else(|_| GlobSet::empty())
}

/// True when a directory and everything below it is excluded.
fn dir_excluded(globs: &GlobSet, rel: &str) -> bool {
    globs.is_match(rel) || globs.is_match(format!("{rel}/__nodestarr_probe__"))
}

#[derive(Debug, Default, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct DirStats {
    pub files: u64,
    pub bytes: u64,
    pub skipped: u64,
}

pub struct ArchiveWriter {
    builder: tar::Builder<Box<dyn Write + Send>>,
}

impl ArchiveWriter {
    pub fn create(path: &Path, compression: Compression, level: i32) -> io::Result<Self> {
        let file = BufWriter::with_capacity(256 * 1024, File::create(path)?);
        let writer: Box<dyn Write + Send> = match compression {
            Compression::Zstd => Box::new(zstd::stream::write::Encoder::new(file, level)?.auto_finish()),
            Compression::Gzip => Box::new(flate2::write::GzEncoder::new(file, flate2::Compression::new(6))),
        };
        let mut builder = tar::Builder::new(writer);
        builder.follow_symlinks(false);
        builder.mode(tar::HeaderMode::Complete);
        Ok(Self { builder })
    }

    /// Adds a directory tree under `prefix`, skipping excluded paths.
    pub fn add_dir(
        &mut self,
        src: &Path,
        prefix: &str,
        excludes: &GlobSet,
        cancel: &CancellationToken,
        warn: &mut dyn FnMut(String),
    ) -> io::Result<DirStats> {
        let mut stats = DirStats::default();
        self.walk(src, src, prefix, excludes, cancel, warn, &mut stats)?;
        Ok(stats)
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        &mut self,
        base: &Path,
        dir: &Path,
        prefix: &str,
        excludes: &GlobSet,
        cancel: &CancellationToken,
        warn: &mut dyn FnMut(String),
        stats: &mut DirStats,
    ) -> io::Result<()> {
        let mut entries: Vec<_> = match std::fs::read_dir(dir) {
            Ok(rd) => rd.flatten().collect(),
            Err(err) => {
                warn(format!("cannot read {}: {err}", dir.display()));
                return Ok(());
            }
        };
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            if cancel.is_cancelled() {
                return Err(io::Error::other("cancelled"));
            }
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            let meta = match std::fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let name = format!("{prefix}{rel}");
            if meta.is_dir() {
                if dir_excluded(excludes, &rel) {
                    stats.skipped += 1;
                    continue;
                }
                self.builder.append_dir(&name, &path)?;
                self.walk(base, &path, prefix, excludes, cancel, warn, stats)?;
            } else if meta.is_file() {
                if excludes.is_match(&rel) {
                    stats.skipped += 1;
                    continue;
                }
                match self.append_file(&path, &name, &meta) {
                    Ok(size) => {
                        stats.files += 1;
                        stats.bytes += size;
                    }
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                    Err(err) => {
                        // Only buffered (small) files can fail here, before
                        // anything was written, so the archive stays valid.
                        warn(format!("skipped {rel}: {err}"));
                        stats.skipped += 1;
                    }
                }
            } else if meta.file_type().is_symlink() {
                if excludes.is_match(&rel) {
                    continue;
                }
                if let Ok(target) = std::fs::read_link(&path) {
                    let mut header = tar::Header::new_gnu();
                    header.set_metadata_in_mode(&meta, tar::HeaderMode::Complete);
                    header.set_entry_type(tar::EntryType::Symlink);
                    header.set_size(0);
                    self.builder.append_link(&mut header, &name, &target)?;
                }
            }
            // Sockets, FIFOs and devices are skipped.
        }
        Ok(())
    }

    fn append_file(&mut self, path: &Path, name: &str, meta: &std::fs::Metadata) -> io::Result<u64> {
        let mut header = tar::Header::new_gnu();
        header.set_metadata_in_mode(meta, tar::HeaderMode::Complete);
        if meta.len() <= BUFFER_LIMIT {
            let mut data = Vec::with_capacity(meta.len() as usize);
            File::open(path)?.read_to_end(&mut data)?;
            header.set_size(data.len() as u64);
            header.set_cksum();
            self.builder.append_data(&mut header, name, data.as_slice())?;
            Ok(data.len() as u64)
        } else {
            let file = File::open(path)?;
            header.set_size(meta.len());
            header.set_cksum();
            // Streamed: if the file shrinks while being read, tar errors out
            // and the whole backup fails rather than writing a broken entry.
            self.builder.append_data(&mut header, name, file.take(meta.len()))?;
            Ok(meta.len())
        }
    }

    /// Adds a single file under `name`.
    pub fn add_file(&mut self, src: &Path, name: &str) -> io::Result<u64> {
        let meta = std::fs::metadata(src)?;
        self.append_file(src, name, &meta)
    }

    /// Adds in-memory content (the manifest) as a regular file owned by root.
    pub fn add_bytes(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(crate::util::now() as u64);
        header.set_cksum();
        self.builder.append_data(&mut header, name, data)
    }

    /// Re-packs a tar stream (from Docker's archive API) under `prefix`,
    /// dropping the top-level folder Docker adds and excluded paths.
    pub fn add_tar_stream(&mut self, reader: impl Read, prefix: &str, excludes: &GlobSet) -> io::Result<DirStats> {
        let mut stats = DirStats::default();
        let mut archive = tar::Archive::new(reader);
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.to_string_lossy().into_owned();
            // Docker prefixes entries with the copied folder's name.
            let rel = match path.split_once('/') {
                Some((_, rest)) => rest.to_string(),
                None => continue,
            };
            if rel.is_empty() || excludes.is_match(&rel) || excludes.is_match(format!("{rel}/__nodestarr_probe__")) {
                stats.skipped += 1;
                continue;
            }
            let mut header = entry.header().clone();
            let name = format!("{prefix}{rel}");
            if header.entry_type().is_hard_link() {
                // Hard link targets point at Docker's own prefix; rare in
                // config folders, so they are skipped.
                stats.skipped += 1;
                continue;
            }
            if header.entry_type().is_symlink() {
                if let Some(target) = entry.link_name()? {
                    let target = target.into_owned();
                    self.builder.append_link(&mut header, &name, target)?;
                }
                continue;
            }
            let size = header.size()?;
            self.builder.append_data(&mut header, &name, &mut entry)?;
            if header.entry_type().is_file() {
                stats.files += 1;
                stats.bytes += size;
            }
        }
        Ok(stats)
    }

    pub fn finish(mut self) -> io::Result<()> {
        self.builder.finish()?;
        let mut inner = self.builder.into_inner()?;
        inner.flush()
    }
}

fn open_reader(path: &Path) -> io::Result<Box<dyn Read>> {
    let mut file = BufReader::new(File::open(path)?);
    let mut magic = [0u8; 4];
    let n = file.get_mut().read(&mut magic)?;
    drop(file);
    let file = BufReader::with_capacity(256 * 1024, File::open(path)?);
    if n >= 4 && magic == [0x28, 0xb5, 0x2f, 0xfd] {
        Ok(Box::new(zstd::stream::read::Decoder::with_buffer(file)?))
    } else if n >= 2 && magic[..2] == [0x1f, 0x8b] {
        Ok(Box::new(flate2::read::GzDecoder::new(file)))
    } else {
        Ok(Box::new(file))
    }
}

/// Reads the manifest from inside an archive.
pub fn read_manifest(path: &Path) -> io::Result<Option<serde_json::Value>> {
    let mut archive = tar::Archive::new(open_reader(path)?);
    for entry in archive.entries()? {
        let mut entry = entry?;
        if entry.path()?.to_string_lossy() == MANIFEST_NAME {
            let mut text = String::new();
            entry.read_to_string(&mut text)?;
            return Ok(serde_json::from_str(&text).ok());
        }
    }
    Ok(None)
}

/// Lists archive entries (for the restore preview).
pub fn list_entries(path: &Path, limit: usize) -> io::Result<Vec<(String, u64)>> {
    let mut out = Vec::new();
    let mut archive = tar::Archive::new(open_reader(path)?);
    for entry in archive.entries()? {
        let entry = entry?;
        if entry.header().entry_type().is_file() {
            out.push((entry.path()?.to_string_lossy().into_owned(), entry.header().size().unwrap_or(0)));
            if out.len() >= limit {
                break;
            }
        }
    }
    Ok(out)
}

/// A path inside the archive must be relative and must not climb out.
fn safe_relative(rel: &str) -> Option<PathBuf> {
    let p = Path::new(rel);
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(x) => out.push(x),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if out.as_os_str().is_empty() { None } else { Some(out) }
}

/// Where each archive prefix goes during a restore.
pub struct RestoreTarget {
    pub prefix: String,
    pub dest: PathBuf,
}

#[derive(Debug, Default, Clone, Copy, serde::Serialize)]
pub struct RestoreStats {
    pub files: u64,
    pub bytes: u64,
    pub removed: u64,
}

/// Extracts `files/<n>/...` prefixes into their destinations and saves
/// single files (dumps, native backups) under `extract_dir`.
///
/// In replace mode, files present on disk but absent from the archive are
/// deleted first, except paths matching `keep` (the excludes used when the
/// backup was taken, so e.g. Plex metadata survives).
pub fn restore(
    archive_path: &Path,
    targets: &[RestoreTarget],
    extract_dir: &Path,
    extract_prefixes: &[&str],
    replace: bool,
    keep: &GlobSet,
    cancel: &CancellationToken,
) -> io::Result<RestoreStats> {
    let mut stats = RestoreStats::default();
    if replace {
        // First pass: collect what the archive contains per target.
        let mut present: Vec<HashSet<PathBuf>> = vec![HashSet::new(); targets.len()];
        let mut archive = tar::Archive::new(open_reader(archive_path)?);
        for entry in archive.entries()? {
            let entry = entry?;
            let path = entry.path()?.to_string_lossy().into_owned();
            for (i, t) in targets.iter().enumerate() {
                if let Some(rel) = path.strip_prefix(&t.prefix).and_then(safe_relative) {
                    present[i].insert(rel);
                }
            }
        }
        for (i, t) in targets.iter().enumerate() {
            stats.removed += remove_absent(&t.dest, &t.dest, &present[i], keep)?;
        }
    }

    let mut archive = tar::Archive::new(open_reader(archive_path)?);
    archive.set_preserve_permissions(true);
    archive.set_preserve_mtime(true);
    archive.set_overwrite(true);
    // Ownership can only be restored when running as root.
    archive.set_preserve_ownerships(rustix::process::geteuid().is_root());
    let mut restored: HashSet<PathBuf> = HashSet::new();
    for entry in archive.entries()? {
        if cancel.is_cancelled() {
            return Err(io::Error::other("cancelled"));
        }
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().into_owned();
        if let Some(target) = targets.iter().find(|t| path.starts_with(&t.prefix)) {
            let Some(rel) = safe_relative(&path[target.prefix.len()..]) else { continue };
            let dest = target.dest.join(&rel);
            if entry.header().entry_type().is_file() {
                stats.files += 1;
                stats.bytes += entry.header().size().unwrap_or(0);
                restored.insert(dest.clone());
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            entry.unpack(&dest)?;
        } else if extract_prefixes.iter().any(|p| path.starts_with(p)) {
            let Some(rel) = safe_relative(&path) else { continue };
            let dest = extract_dir.join(rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            entry.unpack(&dest)?;
        }
    }
    drop_stale_sqlite_sidecars(&restored);
    Ok(stats)
}

/// A restored SQLite database must not be combined with a WAL or journal
/// left over from the current copy: sidecars that weren't restored along
/// with their database are removed. Sidecars that were in the backup belong
/// to it (they hold committed transactions) and are kept, whatever order the
/// archive listed them in.
pub fn drop_stale_sqlite_sidecars(restored: &HashSet<PathBuf>) {
    for db in restored {
        let name = db.to_string_lossy();
        if !(name.ends_with(".db") || name.ends_with(".sqlite") || name.ends_with(".sqlite3")) {
            continue;
        }
        for suffix in ["-wal", "-shm", "-journal"] {
            let sidecar = PathBuf::from(format!("{name}{suffix}"));
            if !restored.contains(&sidecar) {
                let _ = std::fs::remove_file(&sidecar);
            }
        }
    }
}

fn remove_absent(base: &Path, dir: &Path, present: &HashSet<PathBuf>, keep: &GlobSet) -> io::Result<u64> {
    let mut removed = 0;
    let Ok(rd) = std::fs::read_dir(dir) else { return Ok(0) };
    for entry in rd.flatten() {
        let path = entry.path();
        let rel = path.strip_prefix(base).unwrap_or(&path).to_path_buf();
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.is_dir() {
            if dir_excluded(keep, &rel_str) {
                continue;
            }
            removed += remove_absent(base, &path, present, keep)?;
            if !present.contains(&rel) && std::fs::read_dir(&path).map(|mut r| r.next().is_none()).unwrap_or(false) {
                let _ = std::fs::remove_dir(&path);
            }
        } else if !present.contains(&rel) && !keep.is_match(&rel_str) {
            std::fs::remove_file(&path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_with_excludes_and_restore() {
        let src = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(src.path().join("MediaCover/1")).unwrap();
        std::fs::create_dir_all(src.path().join("logs")).unwrap();
        std::fs::write(src.path().join("config.xml"), "<Config/>").unwrap();
        std::fs::write(src.path().join("sonarr.db"), "db").unwrap();
        std::fs::write(src.path().join("MediaCover/1/poster.jpg"), "img").unwrap();
        std::fs::write(src.path().join("logs/sonarr.txt"), "log").unwrap();
        std::os::unix::fs::symlink("config.xml", src.path().join("link.xml")).unwrap();

        let out = tempfile::tempdir().unwrap();
        let archive_path = out.path().join("b.tar.zst");
        let globs = build_globs(&["MediaCover/**".into(), "logs/**".into()]);
        let mut w = ArchiveWriter::create(&archive_path, Compression::Zstd, 3).unwrap();
        let mut warnings = Vec::new();
        let stats = w
            .add_dir(src.path(), "files/0/", &globs, &CancellationToken::new(), &mut |m| warnings.push(m))
            .unwrap();
        w.add_bytes(MANIFEST_NAME, br#"{"version":1}"#).unwrap();
        w.finish().unwrap();
        assert_eq!(stats.files, 2);
        assert!(warnings.is_empty());

        let names: Vec<String> = list_entries(&archive_path, 100).unwrap().into_iter().map(|(n, _)| n).collect();
        assert!(names.contains(&"files/0/config.xml".to_string()));
        assert!(!names.iter().any(|n| n.contains("MediaCover") || n.contains("logs")));
        assert_eq!(read_manifest(&archive_path).unwrap().unwrap()["version"], 1);

        // Restore into a folder holding a stale WAL, an excluded cache and a stray file.
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(dest.path().join("sonarr.db-wal"), "stale").unwrap();
        std::fs::create_dir_all(dest.path().join("MediaCover")).unwrap();
        std::fs::write(dest.path().join("MediaCover/keep.jpg"), "k").unwrap();
        std::fs::write(dest.path().join("stray.txt"), "x").unwrap();
        let targets = vec![RestoreTarget { prefix: "files/0/".into(), dest: dest.path().to_path_buf() }];
        let extract = tempfile::tempdir().unwrap();
        let r = restore(&archive_path, &targets, extract.path(), &[], true, &globs, &CancellationToken::new()).unwrap();
        assert_eq!(r.files, 2);
        assert_eq!(std::fs::read_to_string(dest.path().join("config.xml")).unwrap(), "<Config/>");
        assert!(!dest.path().join("sonarr.db-wal").exists(), "stale WAL removed");
        assert!(dest.path().join("MediaCover/keep.jpg").exists(), "excluded content kept");
        assert!(!dest.path().join("stray.txt").exists(), "replace mode removes stray files");
        assert!(std::fs::symlink_metadata(dest.path().join("link.xml")).unwrap().file_type().is_symlink());
    }

    #[test]
    fn archived_wal_survives_any_entry_order() {
        // An archive listing the WAL before its database (as other tools may).
        let out = tempfile::tempdir().unwrap();
        let archive_path = out.path().join("b.tar.zst");
        let mut w = ArchiveWriter::create(&archive_path, Compression::Zstd, 3).unwrap();
        w.add_bytes("files/0/app.db-wal", b"committed").unwrap();
        w.add_bytes("files/0/app.db", b"db").unwrap();
        w.finish().unwrap();

        let dest = tempfile::tempdir().unwrap();
        std::fs::write(dest.path().join("app.db-shm"), "stale").unwrap();
        let targets = vec![RestoreTarget { prefix: "files/0/".into(), dest: dest.path().to_path_buf() }];
        let extract = tempfile::tempdir().unwrap();
        let globs = build_globs(&[]);
        restore(&archive_path, &targets, extract.path(), &[], false, &globs, &CancellationToken::new()).unwrap();
        assert_eq!(std::fs::read_to_string(dest.path().join("app.db-wal")).unwrap(), "committed");
        assert!(!dest.path().join("app.db-shm").exists(), "stale shared-memory file removed");
    }

    #[test]
    fn rejects_unsafe_paths() {
        assert!(safe_relative("../etc/passwd").is_none());
        assert!(safe_relative("/abs").is_none());
        assert_eq!(safe_relative("a/./b").unwrap(), PathBuf::from("a/b"));
    }

    #[test]
    fn excluded_dirs_are_pruned() {
        let g = build_globs(&["**/Cache/**".into(), "*.log".into()]);
        assert!(dir_excluded(&g, "Library/Plex/Cache"));
        assert!(!dir_excluded(&g, "Library/Plex"));
        assert!(g.is_match("a/b/c.log"));
    }
}
