//! `gar image gc` — garbage collect old generations with BTRFS snapshot.
//!
//! Replaces `ragc gc` (commands/gc.sh, 151 LOC).
//! Policy: preserve pointer generations, recent (within grace),
//! and last N with manifest. Snapshot via btrfs or `cp -al` (hard links).

use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

use serde::Serialize;

use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;

#[derive(Debug, Serialize)]
pub struct GcResult {
    pub snapshot_path: Option<String>,
    pub preserved_pointers: Vec<String>,
    pub preserved_retention: Vec<String>,
    pub preserved_recent: Vec<String>,
    pub preserved_unknown: Vec<String>,
    pub removed: Vec<String>,
    pub kept: u32,
}

pub async fn run(keep: Option<u32>) -> Result<()> {
    let cfg = Config::from_env()?;
    let keep = keep.unwrap_or(cfg.keep_versions);

    if !cfg.images_root.exists() {
        output::info("Nada para fazer (images_root não existe).");
        return Ok(());
    }

    if keep == 0 {
        return Err(GarError::gc("keep=0 inválido"));
    }

    let snapshots_root = format!("/srv/data/snapshots");
    let snapshot_keep = cfg.gc_snapshot_keep;
    let grace_seconds = cfg.gc_grace_seconds;

    // Resolve pointer versions
    let pointers = resolve_pointers(&cfg.images_root)?;
    if pointers.is_empty() {
        return Err(GarError::gc(
            "GC requer pelo menos 1 ponteiro (current/previous/staged).",
        ));
    }

    // Collect all v* dirs sorted by mtime descending (newest first)
    let mut ranked: Vec<(SystemTime, std::path::PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(&cfg.images_root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if !path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with('v'))
            .unwrap_or(false)
        {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        ranked.push((mtime, path));
    }
    ranked.sort_by(|a, b| b.0.cmp(&a.0));

    let now = SystemTime::now();

    // Classify each generation
    let mut preserve_ptr = Vec::new();
    let mut preserve_recent = Vec::new();
    let mut preserve_unknown = Vec::new();
    let mut preserve_retention = Vec::new();
    let mut to_remove = Vec::new();
    let mut retention_count = 0u32;

    for (mtime, path) in &ranked {
        let ver = path.file_name().unwrap().to_string_lossy().into_owned();

        // 1. Preserved if any pointer points to it
        if pointers.contains_key(&ver) {
            preserve_ptr.push(ver.clone());
            if path.join("manifest.json").exists() {
                retention_count += 1;
            }
            continue;
        }

        // 2. Preserved if recent (within grace_seconds)
        if let Ok(age) = now.duration_since(*mtime) {
            if age.as_secs() < grace_seconds {
                preserve_recent.push(ver.clone());
                if path.join("manifest.json").exists() {
                    retention_count += 1;
                }
                continue;
            }
        }

        // 3. Preserved if no manifest (unknown — too risky to delete)
        if !path.join("manifest.json").exists() {
            preserve_unknown.push(ver.clone());
            continue;
        }

        // 4. Keep last N (with manifest)
        if retention_count < keep {
            preserve_retention.push(ver.clone());
            retention_count += 1;
            continue;
        }

        // 5. Mark for removal
        to_remove.push(path.clone());
    }

    // Snapshot before removal (BTRFS subvolume or hard-link copy)
    let snapshot_path = if !to_remove.is_empty() {
        std::fs::create_dir_all(&snapshots_root).ok();
        let snapshot_name = format!(
            "images-pre-gc-{}",
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        );
        let path = format!("{}/{}", snapshots_root, snapshot_name);
        let snap_ok = btrfs_snapshot(&cfg.images_root, &path)
            .or_else(|_| cp_al_snapshot(&cfg.images_root, &path))
            .is_ok();

        if !snap_ok {
            return Err(GarError::gc(format!(
                "snapshot falhou — recusando remoções (images_root={}, snapshots={})",
                cfg.images_root.display(),
                snapshots_root
            )));
        }
        Some(path)
    } else {
        None
    };

    // Remove marked generations
    let mut removed: Vec<String> = Vec::new();
    for path in &to_remove {
        let ver = path.file_name().unwrap().to_string_lossy().into_owned();
        output::warn(format!("GC: removendo {}", ver));
        if let Err(e) = std::fs::remove_dir_all(path) {
            output::err(format!("falha ao remover {}: {}", ver, e));
        } else {
            removed.push(ver);
        }
    }

    // Prune old snapshots
    let _ = prune_snapshots(&snapshots_root, snapshot_keep);

    let result = GcResult {
        snapshot_path,
        preserved_pointers: preserve_ptr,
        preserved_retention: preserve_retention,
        preserved_recent: preserve_recent,
        preserved_unknown: preserve_unknown,
        removed,
        kept: retention_count,
    };

    if cfg.json_output {
        output::json(&result)?;
    } else if result.snapshot_path.is_some() {
        output::ok(format!(
            "GC concluído — snapshot: {}",
            result.snapshot_path.as_ref().unwrap()
        ));
    } else {
        output::ok(format!(
            "GC: nada a remover — mantidas as últimas {} (além de ponteiros e proteções conservadoras).",
            keep
        ));
    }

    Ok(())
}

fn resolve_pointers(images_root: &Path) -> Result<HashMap<String, &'static str>> {
    let mut map = HashMap::new();
    for name in &["current", "previous", "staged"] {
        let path = images_root.join(name);
        if path.is_symlink() {
            if let Ok(target) = std::fs::read_link(&path) {
                if let Some(ver) = target.file_name().and_then(|n| n.to_str()) {
                    map.insert(ver.to_string(), *name);
                }
            }
        }
    }
    Ok(map)
}

fn btrfs_snapshot(src: &Path, dst: &str) -> Result<()> {
    // Check if btrfs is available and src is a btrfs subvolume
    let check = std::process::Command::new("btrfs")
        .args(["subvolume", "show", &src.display().to_string()])
        .output();
    match check {
        Ok(o) if o.status.success() => {
            let status = std::process::Command::new("btrfs")
                .args([
                    "subvolume",
                    "snapshot",
                    "-r",
                    &src.display().to_string(),
                    dst,
                ])
                .status()?;
            if !status.success() {
                return Err(GarError::gc(format!(
                    "btrfs subvolume snapshot falhou: exit {}",
                    status.code().unwrap_or(-1)
                )));
            }
            Ok(())
        }
        _ => Err(GarError::gc("btrfs não disponível ou src não é subvolume")),
    }
}

fn cp_al_snapshot(src: &Path, dst: &str) -> Result<()> {
    let status = std::process::Command::new("cp")
        .args(["-al", &src.display().to_string(), dst])
        .status()?;
    if !status.success() {
        return Err(GarError::gc(format!(
            "cp -al falhou: exit {}",
            status.code().unwrap_or(-1)
        )));
    }
    Ok(())
}

fn prune_snapshots(snapshots_root: &str, keep: u32) -> Result<()> {
    let path = std::path::Path::new(snapshots_root);
    if !path.exists() {
        return Ok(());
    }

    let mut snaps: Vec<_> = std::fs::read_dir(path)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("images-pre-gc-")
        })
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
    fn test_resolve_pointers_empty() {
        let tmp = std::env::temp_dir().join(format!("gar-gc-resolve-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let r = resolve_pointers(&tmp).unwrap();
        assert!(r.is_empty());
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}