//! Filesystem helpers: atomic writes, path confinement and disk usage.

use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// Writes a file atomically: write to a temp file in the same directory,
/// fsync, then rename over the target. Preserves the existing file's
/// permissions and ownership when replacing it, so editing an app's config
/// file never changes who can read it.
pub fn atomic_write(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("path has no parent directory"))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("path has no file name"))?
        .to_string_lossy()
        .to_string();
    let tmp = dir.join(format!(".{file_name}.berth-tmp-{}", crate::util::short_id()));

    let existing = std::fs::metadata(path).ok();
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(contents)?;
        f.sync_all()?;
    }
    if let Some(meta) = existing {
        use std::os::unix::fs::MetadataExt;
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
        // Ownership can only be changed when running as root; ignore failures.
        let _ = std::os::unix::fs::chown(&tmp, Some(meta.uid()), Some(meta.gid()));
    }
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = std::fs::remove_file(&tmp);
            Err(err)
        }
    }
}

/// Lexically normalizes a path (resolves `.` and `..` without touching the
/// filesystem). Returns `None` if `..` would climb above the root.
pub fn normalize(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::RootDir => out.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() || out.as_os_str().is_empty() {
                    return None;
                }
            }
            Component::Normal(p) => out.push(p),
            Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

/// Resolves `relative` inside `root` and guarantees the result stays inside
/// `root`, following symlinks for the existing part of the path. Used by the
/// config file editor so a request can never read `/etc/shadow` through a
/// crafted path or a symlink planted in a config folder.
pub fn confine(root: &Path, relative: &str) -> std::io::Result<PathBuf> {
    let root_real = std::fs::canonicalize(root)?;
    let rel = relative.trim_start_matches('/');
    let joined = normalize(&root_real.join(rel))
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::PermissionDenied, "path escapes root"))?;

    // Canonicalize the longest existing prefix (the leaf may not exist yet).
    let mut existing = joined.clone();
    let mut tail = Vec::new();
    while !existing.exists() {
        match existing.file_name() {
            Some(name) => {
                tail.push(name.to_os_string());
                existing.pop();
            }
            None => break,
        }
    }
    let mut resolved = std::fs::canonicalize(&existing)?;
    for part in tail.iter().rev() {
        resolved.push(part);
    }
    if resolved.starts_with(&root_real) {
        Ok(resolved)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "path escapes root",
        ))
    }
}

/// Disk usage for the filesystem holding `path`.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct DiskUsage {
    pub total: u64,
    pub used: u64,
    pub available: u64,
}

pub fn disk_usage(path: &Path) -> std::io::Result<DiskUsage> {
    let st = rustix::fs::statvfs(path).map_err(std::io::Error::from)?;
    let frsize = if st.f_frsize > 0 { st.f_frsize } else { st.f_bsize };
    let total = st.f_blocks.saturating_mul(frsize);
    let free = st.f_bfree.saturating_mul(frsize);
    let available = st.f_bavail.saturating_mul(frsize);
    Ok(DiskUsage {
        total,
        used: total.saturating_sub(free),
        available,
    })
}

/// Recursively sums file sizes under `path`, skipping anything the
/// `exclude` predicate rejects (it receives paths relative to `path`).
/// Symlinks are not followed.
pub fn dir_size(path: &Path, exclude: &dyn Fn(&Path) -> bool) -> u64 {
    fn walk(base: &Path, dir: &Path, exclude: &dyn Fn(&Path) -> bool, total: &mut u64) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let rel = p.strip_prefix(base).unwrap_or(&p);
            if exclude(rel) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                walk(base, &p, exclude, total);
            } else if meta.is_file() {
                *total += meta.len();
            }
        }
    }
    let mut total = 0;
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.is_file() {
            return meta.len();
        }
        walk(path, path, exclude, &mut total);
    }
    total
}

/// Heuristic binary detection: NUL bytes in the first 8 KiB.
pub fn looks_binary(data: &[u8]) -> bool {
    data.iter().take(8192).any(|b| *b == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_handles_dots() {
        assert_eq!(normalize(Path::new("/a/b/../c/./d")).unwrap(), PathBuf::from("/a/c/d"));
        assert!(normalize(Path::new("/../etc")).is_none());
    }

    #[test]
    fn confine_blocks_escapes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/file.txt"), "x").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "s").unwrap();

        assert!(confine(&root, "sub/file.txt").is_ok());
        assert!(confine(&root, "/sub/new.txt").is_ok());
        assert!(confine(&root, "../secret.txt").is_err());
        assert!(confine(&root, "sub/../../secret.txt").is_err());

        std::os::unix::fs::symlink(dir.path().join("secret.txt"), root.join("link")).unwrap();
        assert!(confine(&root, "link").is_err(), "symlink out of root must be rejected");
    }

    #[test]
    fn atomic_write_replaces_content() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.yaml");
        atomic_write(&p, b"one").unwrap();
        atomic_write(&p, b"two").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "two");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(leftovers.len(), 1);
    }

    #[test]
    fn dir_size_with_excludes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("cache")).unwrap();
        std::fs::write(dir.path().join("a"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.path().join("cache/b"), vec![0u8; 1000]).unwrap();
        assert_eq!(dir_size(dir.path(), &|_| false), 1100);
        assert_eq!(dir_size(dir.path(), &|p| p.starts_with("cache")), 100);
    }
}
