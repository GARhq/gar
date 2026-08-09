//! BTRFS operations (subvolume, quota, snapshot).
//!
//! Wraps btrfs-progs for tier1 storage management.

use std::path::Path;

use crate::error::Result;

/// Create a BTRFS subvolume (no-op if already exists).
pub async fn create_subvolume(path: &Path) -> Result<()> {
    let _ = crate::services::shell::run_success(
        "btrfs",
        &["subvolume", "create", &path.display().to_string()],
    )
    .await?;
    Ok(())
}

/// Take a read-only BTRFS snapshot.
pub async fn snapshot_readonly(src: &Path, dst: &Path) -> Result<()> {
    let _ = crate::services::shell::run_success(
        "btrfs",
        &[
            "subvolume",
            "snapshot",
            "-r",
            &src.display().to_string(),
            &dst.display().to_string(),
        ],
    )
    .await?;
    Ok(())
}

/// Apply a qgroup limit (bytes) to a path.
pub async fn set_quota(path: &Path, bytes: &str) -> Result<()> {
    let _ = crate::services::shell::run_success(
        "btrfs",
        &["qgroup", "limit", bytes, &path.display().to_string()],
    )
    .await?;
    Ok(())
}
