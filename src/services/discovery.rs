//! Filesystem discovery helpers for GAROS generations.
//!
//! Shared by `gar image list`, `gar image status`, `gar image gc` and
//! anything else that needs to enumerate the published `v*`
//! generation directories under `images_root`.

use std::path::{Path, PathBuf};

/// Return the absolute path of every immediate subdirectory of
/// `images_root` whose name starts with `v` (the GAROS version
/// prefix). Non-directory entries (symlinks, files) are skipped so
/// that `current`, `previous`, `staged` and the per-channel pointers
/// are never mistaken for a generation.
pub fn iter_version_dirs(images_root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(images_root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let is_version = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with('v'))
            .unwrap_or(false);
        if is_version {
            out.push(path);
        }
    }
    Ok(out)
}

/// Same as [`iter_version_dirs`] but additionally returns the
/// directory's modification time (mtime). Useful for policies that
/// preserve recent generations.
pub fn iter_version_dirs_with_mtime(
    images_root: &Path,
) -> std::io::Result<Vec<(std::time::SystemTime, PathBuf)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(images_root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let is_version = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with('v'))
            .unwrap_or(false);
        if !is_version {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        out.push((mtime, path));
    }
    Ok(out)
}

/// True if `path/manifest.json` exists and is a regular file. Used
/// to gate GC decisions and to render missing-manifest rows in
/// `gar image list`.
pub fn has_manifest(path: &Path) -> bool {
    path.join("manifest.json").is_file()
}
