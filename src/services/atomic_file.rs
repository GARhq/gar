//! Atomic file operations (write to temp + rename).
//!
//! Prevents partial reads on crash. Used by user/group metadata,
//! install plan, runtime manifest, etc.

use std::path::Path;

use crate::error::{GarError, Result};

/// Write bytes to a file atomically (write to temp + rename).
pub async fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| GarError::config(format!("path sem parent: {}", path.display())))?;
    tokio::fs::create_dir_all(dir).await?;

    let tmp = dir.join(format!(
        ".{}.tmp.{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        std::process::id()
    ));

    tokio::fs::write(&tmp, contents).await?;
    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}
