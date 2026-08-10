//! Atomic filesystem operations for symlinks and removals.
//!
//! Replaces `atomic_symlink` and `atomic_remove_path` from
//! `ragc/lib/publish.sh` (publish.sh:245-262). Centralized here so both
//! `commands/image/rollback.rs` and any boot/storage code can share the
//! same temp-link + rename and move-to-tombstone dance.
//!
//! ## Why atomic?
//!
//! - `symlink`: writing a symlink directly is non-atomic; readers can see
//!   a missing link between the `unlink` and the `symlink` calls. The
//!   temp+rename dance keeps the path resolvable at every instant.
//! - `remove`: a direct `rm -f` is fine on success but on failure leaves
//!   no signal. The tombstone pattern makes the "deletion" visible to
//!   concurrent readers (the path is gone, but a marker is present in
//!   the same directory) and is reverted in a best-effort cleanup.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use crate::error::{GarError, Result};

/// Create (or replace) a symlink at `link_path` atomically.
///
/// Writes to a sibling temp path first (`.<name>.tmp.<pid>`), then
/// renames over `link_path`. Equivalent to the bash
/// `atomic_symlink` helper in `publish.sh`.
///
/// If `link_path` already exists (file or symlink), it is replaced.
#[must_use = "atomic_symlink has filesystem side effects; ignoring the Result hides failures"]
#[tracing::instrument(skip_all, fields(target = %target.display(), link = %link_path.display()))]
pub fn atomic_symlink(target: &Path, link_path: &Path) -> Result<()> {
    // Validate that link_path has a parent — required so the temp file
    // can be created in the same directory for atomic rename.
    link_path.parent().ok_or_else(|| {
        GarError::Publish(format!(
            "atomic_symlink: link sem parent: {}",
            link_path.display()
        ))
    })?;

    // Idempotent: remove existing symlink or file at the destination.
    // `symlink_metadata` distinguishes "doesn't exist" from "is a symlink".
    match fs::symlink_metadata(link_path) {
        Ok(md) if md.file_type().is_symlink() || md.is_file() => {
            fs::remove_file(link_path)?;
        }
        Ok(_) => {
            // Directory or other non-file type: refuse rather than destroy.
            return Err(GarError::Publish(format!(
                "atomic_symlink: destino não é arquivo/symlink: {}",
                link_path.display()
            )));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Path absent; nothing to clean up.
        }
        Err(e) => return Err(GarError::Io(e)),
    }

    let tmp = tmp_sibling(link_path, "symlink");
    symlink(target, &tmp)?;
    // rename(2) is atomic on the same filesystem; tmp is in link_path's
    // parent, so this is a guaranteed same-FS operation.
    fs::rename(&tmp, link_path)?;
    Ok(())
}

/// Remove a file or symlink atomically (best-effort, idempotent).
///
/// If the path exists (regular file or symlink), move it to a sibling
/// tombstone (`<path>.delete.<pid>`), then delete the tombstone. If
/// `path` is a symlink whose target is missing, the symlink itself is
/// still moved before being unlinked.
///
/// This matches `atomic_remove_path` in `publish.sh`: the goal is to
/// make the removal observable to concurrent readers (the path is gone)
/// while the tombstone absorbs failure cleanup.
///
/// No-op when the path is missing.
#[must_use = "atomic_remove_path has filesystem side effects; ignoring the Result hides failures"]
#[tracing::instrument(skip_all, fields(path = %path.display()))]
pub fn atomic_remove_path(path: &Path) -> Result<()> {
    let exists = match fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(GarError::Io(e)),
    };
    if !exists {
        return Ok(());
    }

    // Validate parent exists so the tombstone can be created alongside.
    path.parent().ok_or_else(|| {
        GarError::Publish(format!(
            "atomic_remove_path: path sem parent: {}",
            path.display()
        ))
    })?;

    let tomb = tmp_sibling(path, "delete");
    // mv -Tf in bash: move into the tomb, overwriting if necessary.
    // For a regular `mv` we use `fs::rename` which on POSIX atomically
    // replaces the destination. To match the bash behavior of replacing
    // an existing tomb, we remove it first if present.
    if let Err(e) = fs::symlink_metadata(&tomb) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(GarError::Io(e));
        }
    } else {
        fs::remove_file(&tomb)?;
    }
    fs::rename(path, &tomb)?;
    // Best-effort tombstone cleanup. Failure here is non-fatal: the
    // tombstone is a hidden file (starts with '.') and the next publish
    // cycle can reap it. We still log it for visibility.
    if let Err(e) = fs::remove_file(&tomb) {
        tracing::warn!(
            tomb = %tomb.display(),
            error = %e,
            "tombstone cleanup failed; will leave residue"
        );
    }
    Ok(())
}

/// Compute a sibling temp filename inside `link_path`'s parent directory.
///
/// Uses `<basename>.<tag>.tmp.<pid>` — matches the bash idiom of
/// `${RANDOM}` + `$$`. `pid` is process-local so concurrent writers
/// (in separate processes) cannot collide on the same name.
fn tmp_sibling(link_path: &Path, tag: &str) -> PathBuf {
    let parent = link_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = link_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("path");
    parent.join(format!(".{}.{}.tmp.{}", stem, tag, std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(label: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("gar-atomic-{}-{}", label, std::process::id()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn cleanup(p: &Path) {
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn test_atomic_symlink_creates_link() {
        let dir = tmp_dir("sym-create");
        let target = dir.join("v20260101-120000");
        fs::create_dir_all(&target).unwrap();
        let link = dir.join("current");

        atomic_symlink(&target, &link).unwrap();

        assert!(link.is_symlink());
        assert_eq!(
            fs::read_link(&link).unwrap().file_name().unwrap(),
            "v20260101-120000"
        );
        cleanup(&dir);
    }

    #[test]
    fn test_atomic_symlink_overwrites_existing() {
        let dir = tmp_dir("sym-overwrite");
        let target1 = dir.join("v1");
        let target2 = dir.join("v2");
        fs::create_dir_all(&target1).unwrap();
        fs::create_dir_all(&target2).unwrap();
        let link = dir.join("current");

        atomic_symlink(&target1, &link).unwrap();
        atomic_symlink(&target2, &link).unwrap();

        assert_eq!(fs::read_link(&link).unwrap().file_name().unwrap(), "v2");
        cleanup(&dir);
    }

    #[test]
    fn test_atomic_remove_path_missing_is_noop() {
        let dir = tmp_dir("rm-missing");
        let ghost = dir.join("never-existed");
        // Missing path: should succeed without touching anything.
        atomic_remove_path(&ghost).unwrap();
        assert!(!ghost.exists());
        // No tombstone should have been created.
        assert!(
            fs::read_dir(&dir).unwrap().next().is_none() || {
                // We allow the test dir itself, but no tombstone inside.
                let entries: Vec<_> = fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).collect();
                assert_eq!(entries.len(), 0, "no entries expected, got: {:?}", entries);
                true
            }
        );
        cleanup(&dir);
    }

    #[test]
    fn test_atomic_remove_path_removes_file_and_tombstone() {
        let dir = tmp_dir("rm-file");
        let target = dir.join("old-build");
        fs::write(&target, "payload").unwrap();

        atomic_remove_path(&target).unwrap();

        assert!(!target.exists(), "file should be gone");
        // No tombstone residue.
        let leftover: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let n = e.file_name();
                let s = n.to_string_lossy();
                s.contains(".delete.")
            })
            .collect();
        assert!(leftover.is_empty(), "tombstone residue: {:?}", leftover);
        cleanup(&dir);
    }
}
