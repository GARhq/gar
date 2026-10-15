//! `gar image gc` — garbage collect old generations with BTRFS snapshot.
//!
//! Replaces `ragc gc` (commands/gc.sh, 151 LOC).
//! Policy: preserve pointer generations, recent (within grace),
//! and last N with manifest. Snapshot via btrfs or `cp -al` (hard links).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::discovery;

/// Pointer names looked up by `resolve_pointers` (in lookup order).
const POINTER_NAMES: &[&str] = &["current", "previous", "staged"];

/// Decision made for a single generation during classification.
#[derive(Debug)]
enum KeepReason {
    /// Pointed to by `current` / `previous` / `staged`.
    Pointer,
    /// mtime within the configured grace window.
    Recent,
    /// Has no `manifest.json` — too risky to delete.
    Unknown,
    /// Within the last-N retention policy.
    Retention,
    /// Candidate for removal.
    Remove,
}

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

/// Buckets populated by `classify_generations` and merged into a
/// `GcResult` by `build_report`.
#[derive(Debug, Default)]
struct Classification {
    pointer: Vec<String>,
    retention: Vec<String>,
    recent: Vec<String>,
    unknown: Vec<String>,
    to_remove: Vec<PathBuf>,
    kept: u32,
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

    let pointers = resolve_pointers(&cfg.images_root)?;
    if pointers.is_empty() {
        return Err(GarError::gc(
            "GC requer pelo menos 1 ponteiro (current/previous/staged).",
        ));
    }

    let ranked = load_ranked(&cfg.images_root)?;
    let classification = classify_generations(
        &ranked,
        &pointers,
        GcPolicy {
            keep,
            grace_seconds: cfg.gc_grace_seconds,
            now: SystemTime::now(),
        },
    );

    // Snapshot before any destructive action. If the snapshot fails
    // we refuse the whole GC so we never lose data we couldn't recover.
    let snapshot_path = if !classification.to_remove.is_empty() {
        crate::services::snapshot::take(
            &cfg.images_root,
            crate::services::snapshot::DEFAULT_SNAPSHOTS_ROOT,
        )
        .await
        .map_err(|e| {
            GarError::gc(format!(
                "snapshot falhou — recusando remoções (images_root={}, err={e})",
                cfg.images_root.display()
            ))
        })?
    } else {
        None
    };

    let removed = remove_marked(&classification.to_remove);
    let _ = crate::services::snapshot::prune(
        crate::services::snapshot::DEFAULT_SNAPSHOTS_ROOT,
        cfg.gc_snapshot_keep,
    );

    let result = build_report(classification, removed, snapshot_path);
    report(&cfg, &result, keep);
    Ok(())
}

/// Read `images_root`, collect every `v*` directory, and return them
/// sorted by mtime descending (newest first).
fn load_ranked(images_root: &Path) -> Result<Vec<(SystemTime, PathBuf)>> {
    let mut ranked = discovery::iter_version_dirs_with_mtime(images_root)?;
    ranked.sort_by_key(|r| std::cmp::Reverse(r.0));
    Ok(ranked)
}

/// Inputs that don't change between iterations of `classify_generations`:
/// the retention target, the grace window, and the reference clock.
/// Grouped to keep `decide`'s argument list under clippy's 7-arg cap
/// and to make the policy knobs explicit at the call site.
struct GcPolicy {
    keep: u32,
    grace_seconds: u64,
    now: SystemTime,
}

/// Decide what to do with each generation, populating the four
/// preservation buckets and the `to_remove` list. The `keep` policy
/// counts only generations that *have* a manifest — unknown
/// (manifest-less) entries are quarantined, not counted.
fn classify_generations(
    ranked: &[(SystemTime, PathBuf)],
    pointers: &HashMap<String, &'static str>,
    policy: GcPolicy,
) -> Classification {
    let mut c = Classification::default();

    for (mtime, path) in ranked {
        let ver = dir_version_name(path);
        match decide(path, &ver, mtime, pointers, &mut c.kept, &policy) {
            KeepReason::Pointer => c.pointer.push(ver),
            KeepReason::Recent => c.recent.push(ver),
            KeepReason::Unknown => c.unknown.push(ver),
            KeepReason::Retention => c.retention.push(ver),
            KeepReason::Remove => c.to_remove.push(path.clone()),
        }
    }
    c
}

/// Single-version classifier. Extracted so the rule order is
/// visible at a glance instead of buried in a 35-line `for` body.
fn decide(
    path: &Path,
    ver: &str,
    mtime: &SystemTime,
    pointers: &HashMap<String, &'static str>,
    kept: &mut u32,
    policy: &GcPolicy,
) -> KeepReason {
    // 1. Preserved if any pointer points to it.
    if pointers.contains_key(ver) {
        if discovery::has_manifest(path) {
            *kept += 1;
        }
        return KeepReason::Pointer;
    }

    // 2. Preserved if recent (within grace_seconds).
    if let Ok(age) = policy.now.duration_since(*mtime) {
        if age.as_secs() < policy.grace_seconds {
            if discovery::has_manifest(path) {
                *kept += 1;
            }
            return KeepReason::Recent;
        }
    }

    // 3. Preserved if no manifest (unknown — too risky to delete).
    if !discovery::has_manifest(path) {
        return KeepReason::Unknown;
    }

    // 4. Keep last N (with manifest).
    if *kept < policy.keep {
        *kept += 1;
        return KeepReason::Retention;
    }

    // 5. Mark for removal.
    KeepReason::Remove
}

/// Best-effort `rm -rf` over the marked paths. Returns the names of
/// generations that were actually removed (failures are reported as
/// warnings and do not abort the GC).
fn remove_marked(paths: &[PathBuf]) -> Vec<String> {
    let mut removed = Vec::new();
    for path in paths {
        let ver = dir_version_name(path);
        output::warn(format!("GC: removendo {ver}"));
        match std::fs::remove_dir_all(path) {
            Ok(()) => removed.push(ver),
            Err(e) => output::err(format!("falha ao remover {ver}: {e}")),
        }
    }
    removed
}

fn build_report(
    c: Classification,
    removed: Vec<String>,
    snapshot_path: Option<String>,
) -> GcResult {
    GcResult {
        snapshot_path,
        preserved_pointers: c.pointer,
        preserved_retention: c.retention,
        preserved_recent: c.recent,
        preserved_unknown: c.unknown,
        removed,
        kept: c.kept,
    }
}

fn report(cfg: &Config, result: &GcResult, keep: u32) {
    if cfg.json_output {
        let _ = output::json(result);
        return;
    }

    output::header("Garbage Collection");
    output::blank_line();

    let mut summary_rows: Vec<Vec<String>> = Vec::new();
    if !result.preserved_pointers.is_empty() {
        summary_rows.push(vec![
            "Preservados (ponteiros)".into(),
            result.preserved_pointers.len().to_string(),
        ]);
    }
    if !result.preserved_retention.is_empty() {
        summary_rows.push(vec![
            "Preservados (retenção)".into(),
            result.preserved_retention.len().to_string(),
        ]);
    }
    if !result.preserved_recent.is_empty() {
        summary_rows.push(vec![
            "Preservados (recentes)".into(),
            result.preserved_recent.len().to_string(),
        ]);
    }
    if !result.preserved_unknown.is_empty() {
        summary_rows.push(vec![
            "Preservados (sem manifest)".into(),
            result.preserved_unknown.len().to_string(),
        ]);
    }
    summary_rows.push(vec!["Removidos".into(), result.removed.len().to_string()]);
    summary_rows.push(vec!["Mantidos no total".into(), result.kept.to_string()]);
    output::table(&["CATEGORIA", "QTD"], &summary_rows);
    output::blank_line();

    if let Some(snap) = &result.snapshot_path {
        output::ok(format!("Snapshot de segurança: {snap}"));
    }
    if result.removed.is_empty() {
        output::info(format!(
            "Nada a remover — mantidas as últimas {keep} (além de ponteiros e proteções conservadoras)."
        ));
    } else {
        output::info(format!(
            "{} geração(ões) removida(s).",
            result.removed.len()
        ));
    }
}

fn dir_version_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string()
}

/// Map the names of every published pointer symlink to the
/// version they point at. Used to short-circuit GC for any
/// generation the server is actively serving.
fn resolve_pointers(images_root: &Path) -> Result<HashMap<String, &'static str>> {
    let mut map = HashMap::new();
    for name in POINTER_NAMES {
        let path = images_root.join(name);
        if !path.is_symlink() {
            continue;
        }
        if let Ok(target) = std::fs::read_link(&path) {
            if let Some(ver) = target.file_name().and_then(|n| n.to_str()) {
                map.insert(ver.to_string(), *name);
            }
        }
    }
    Ok(map)
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
