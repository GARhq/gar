//! Snapshot primitives for the GAR CLI.
//!
//! Shared between `gar image gc` (which takes a `images-pre-gc-*`
//! snapshot before destructive operations) and any future caller
//! that needs to capture a point-in-time view of a directory.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::{GarError, Result};

/// Default directory where pre-GC snapshots are written. Kept
/// as a constant so callers don't repeat the literal.
pub const DEFAULT_SNAPSHOTS_ROOT: &str = "/srv/data/snapshots";

/// Name prefix for snapshots taken before a GC pass.
pub const SNAPSHOT_PREFIX: &str = "images-pre-gc-";

/// Create a read-only snapshot of `images_root` under
/// `{snapshots_root}/{SNAPSHOT_PREFIX}{ts}`. Tries BTRFS first;
/// falls back to `cp -al` (hard-link copy) when the source is not
/// on a BTRFS subvolume.
///
/// Returns `Some(path)` on success, `None` if both backends fail.
pub async fn take(images_root: &Path, snapshots_root: &str) -> Result<Option<String>> {
    std::fs::create_dir_all(snapshots_root).ok();

    let snapshot_name = format!(
        "{SNAPSHOT_PREFIX}{}",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );
    let path = format!("{snapshots_root}/{snapshot_name}");

    let snap_ok = btrfs(images_root, &path)
        .await
        .or_else(|_| cp_al(images_root, &path));
    if snap_ok.is_ok() {
        Ok(Some(path))
    } else {
        Ok(None)
    }
}

/// Snapshot `src` to `dst` using `btrfs subvolume snapshot -r`.
/// Returns `Err` when `src` is not a BTRFS subvolume.
pub async fn btrfs(src: &Path, dst: &str) -> Result<()> {
    // Probe: is `src` a BTRFS subvolume? `btrfs subvolume show` exits
    // 0 only when the path is one.
    let check = std::process::Command::new("btrfs")
        .args(["subvolume", "show", &src.display().to_string()])
        .output();
    match check {
        Ok(o) if o.status.success() => {
            crate::services::btrfs::snapshot_readonly(src, Path::new(dst))
                .await
                .map_err(|e| GarError::gc(format!("btrfs snapshot falhou: {e}")))
        }
        _ => Err(GarError::gc("btrfs não disponível ou src não é subvolume")),
    }
}

/// Snapshot `src` to `dst` using `cp -al` — a hard-link copy that
/// shares inode tables with the source and rolls back cheaply via
/// `rm -rf` of the destination.
pub fn cp_al(src: &Path, dst: &str) -> Result<()> {
    let status = std::process::Command::new("cp")
        .args(["-al", &src.display().to_string(), dst])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(GarError::gc(format!(
            "cp -al falhou: exit {}",
            status.code().unwrap_or(-1)
        )))
    }
}

/// Keep the `keep` newest `{SNAPSHOT_PREFIX}*` snapshots under
/// `snapshots_root`; delete the rest. Per-entry delete failures
/// are silently ignored — prune is best-effort.
pub fn prune(snapshots_root: &str, keep: u32) -> Result<()> {
    let path = PathBuf::from(snapshots_root);
    if !path.exists() {
        return Ok(());
    }

    let mut snaps: Vec<_> = std::fs::read_dir(&path)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with(SNAPSHOT_PREFIX))
        .collect();

    snaps.sort_by_key(|e| {
        e.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH)
    });
    snaps.reverse();

    if snaps.len() > keep as usize {
        for s in &snaps[keep as usize..] {
            let _ = std::fs::remove_dir_all(s.path());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prune_no_root_is_noop() {
        let tmp =
            std::env::temp_dir().join(format!("gar-snap-prune-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(prune(tmp.to_str().unwrap(), 5).is_ok());
    }
}
