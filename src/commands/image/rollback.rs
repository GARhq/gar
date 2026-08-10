//! `gar image rollback` — revert to previous generation atomically.
//!
//! Replaces `ragc rollback` (commands/rollback.sh, 231 LOC).
//! Uses global lock + atomic symlink swap of current/previous pointers.

use std::path::Path;

use serde::Serialize;

use crate::cli::Channel;
use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::atomic_path;
use crate::services::lock;
use crate::services::manifest;

#[derive(Debug, Serialize)]
pub struct RollbackResult {
    pub source: String,
    pub target: String,
    pub channel: Option<String>,
}

/// Run the rollback command.
pub async fn run(target: Option<String>, channel: Option<Channel>) -> Result<()> {
    let cfg = Config::from_env()?;

    // Acquire global lock (atomic operation)
    let lock_path = &cfg.lock_path;
    let result = lock::with_lock(lock_path, "gar image rollback", || {
        perform_rollback(&cfg, target.as_deref(), channel)
    })?;

    if cfg.json_output {
        output::json(&result)?;
    } else {
        output::ok(format!(
            "Rollback concluído: {} -> {}",
            result.source, result.target
        ));
        if let Some(ch) = &result.channel {
            println!("  Canal: {}", ch);
        }
    }

    Ok(())
}

fn perform_rollback(
    cfg: &Config,
    target: Option<&str>,
    channel: Option<Channel>,
) -> Result<RollbackResult> {
    // 1. Determine current version (from `current` pointer)
    let source = current_version(&cfg.images_root).ok_or_else(|| {
        GarError::Rollback("Nenhuma versão ativa encontrada em 'current'.".into())
    })?;

    // 2. Determine target version
    let target_ver = match target {
        Some("previous") | None => previous_version(&cfg.images_root).ok_or_else(|| {
            GarError::Rollback("Nenhuma versão anterior encontrada em 'previous'.".into())
        })?,
        Some(ver) => {
            // Validate version exists
            let dir = cfg.images_root.join(ver);
            if !dir.exists() {
                return Err(GarError::Rollback(format!(
                    "Versão não encontrada: {}. Use: gar image list",
                    ver
                )));
            }
            ver.to_string()
        }
    };

    // 3. If source == target, no-op
    if source == target_ver {
        output::info(format!(
            "Rollback já converge para {}; nenhuma alteração necessária.",
            target_ver
        ));
        return Ok(RollbackResult {
            source,
            target: target_ver,
            channel: channel.map(|c| c.as_str().to_string()),
        });
    }

    // 4. Atomic swap: target -> current, source -> previous
    let target_dir = cfg.images_root.join(&target_ver);
    let source_dir = cfg.images_root.join(&source);

    atomic_path::atomic_symlink(&target_dir, &cfg.images_root.join("current"))?;
    atomic_path::atomic_symlink(&source_dir, &cfg.images_root.join("previous"))?;

    // 5. Update channel-specific pointers if requested
    if let Some(ch) = channel {
        let cur_ptr = format!("current-{}", ch.as_str());
        let prev_ptr = format!("previous-{}", ch.as_str());
        atomic_path::atomic_symlink(&target_dir, &cfg.images_root.join(&cur_ptr))?;
        atomic_path::atomic_symlink(&source_dir, &cfg.images_root.join(&prev_ptr))?;
    }

    // 6. Reconcile statuses
    let rescue = read_pointer(&cfg.images_root, "rescue");
    manifest::reconcile(
        &cfg.images_root,
        Some(&target_ver),
        Some(&source),
        None,
        rescue.as_deref(),
    )?;

    Ok(RollbackResult {
        source,
        target: target_ver,
        channel: channel.map(|c| c.as_str().to_string()),
    })
}

fn current_version(images_root: &Path) -> Option<String> {
    read_pointer(images_root, "current")
}

fn previous_version(images_root: &Path) -> Option<String> {
    read_pointer(images_root, "previous")
}

fn read_pointer(images_root: &Path, name: &str) -> Option<String> {
    let path = images_root.join(name);
    if path.is_symlink() {
        std::fs::read_link(&path)
            .ok()
            .and_then(|t| t.file_name().map(|n| n.to_string_lossy().into_owned()))
    } else {
        None
    }
}

/// Atomic symlink: create temp symlink + rename (replaces target if exists).
///
/// Migrated to `services::atomic_path::atomic_symlink` (Phase 5.6). The
/// inline copy here was removed; callers now go through the service
/// module so the same semantics are shared with boot/storage code.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_version_no_pointer() {
        let tmp = std::env::temp_dir().join(format!("gar-rollback-cur-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(current_version(&tmp).is_none());
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
