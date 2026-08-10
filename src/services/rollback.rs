//! Rollback state + validation helpers.
//!
//! Replaces `publish.sh:263-291` (`clear_stale_staged_pointer`),
//! `publish.sh:293-299` (`validate_existing_current`), and the rollback
//! state file helpers at `publish.sh:390-442`
//! (`write_pending_rollback`, `load_pending_rollback`, `clear_pending_rollback`,
//! `write_last_rollback`, `load_last_rollback`, `clear_last_rollback`).
//!
//! File format is preserved as `KEY=VALUE` plain text (NOT JSON) — matches
//! bash `source`-friendly semantics so existing on-disk state files in prod
//! continue to work without migration. Newlines in values are not supported
//! (bash heredoc + `source` doesn't handle them either).
//!
//! Idempotency: `clear_*` functions use `remove_file_if_exists` (rm -f
//! semantics) — calling them on a missing file is a no-op, not an error.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{GarError, Result};

/// Default path for the "rollback in progress" marker.
pub const DEFAULT_PENDING_PATH: &str = "/var/lib/ragos/state/rollback.pending";

/// Default path for the "last applied rollback" record.
pub const DEFAULT_LAST_PATH: &str = "/var/lib/ragos/state/rollback.last";

/// A parsed rollback record. Mirrors the bash `source`-able `KEY=VALUE` file.
///
/// `from` and `to` are required (bash version rejects the record if either
/// is empty). `channel` is optional — older records may not have it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollbackRecord {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// When the record was written. NOT stored in the file (bash doesn't
    /// write timestamps); populated by `parse_record` from `mtime`.
    #[serde(skip)]
    pub mtime: Option<DateTime<Utc>>,
}

/// Result of a load attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollbackLoadOutcome {
    /// File existed and was parsed successfully.
    Loaded(RollbackRecord),
    /// File does not exist (bash exit-1 from `[[ -f ]]`).
    Missing,
    /// File existed but could not be parsed (corrupted/truncated).
    Invalid(String),
}

impl RollbackLoadOutcome {
    pub fn into_option(self) -> Option<RollbackRecord> {
        match self {
            Self::Loaded(r) => Some(r),
            _ => None,
        }
    }

    pub fn is_loaded(&self) -> bool {
        matches!(self, Self::Loaded(_))
    }
}

/// Write a pending rollback marker atomically (write-to-temp + rename).
///
/// Equivalent to bash `write_pending_rollback` (publish.sh:390-402).
/// Format:
/// ```text
/// source=<from>
/// target=<to>
/// channel=<channel_or_empty>
/// ```
#[must_use = "write_pending_rollback writes a state file as a side effect"]
#[tracing::instrument(skip_all, fields(path = %path.display()))]
pub fn write_pending_rollback(
    path: &Path,
    from: &str,
    to: &str,
    channel: Option<&str>,
) -> Result<()> {
    write_record(path, from, to, channel)
}

/// Load a pending rollback marker.
///
/// Equivalent to bash `load_pending_rollback` (publish.sh:404-409):
/// - missing file → `Missing`
/// - empty `source` or `target` → `Invalid`
/// - corrupt format → `Invalid`
#[must_use = "load_pending_rollback returns the parsed record or a miss/invalid outcome"]
#[tracing::instrument(skip_all, fields(path = %path.display()))]
pub fn load_pending_rollback(path: &Path) -> Result<RollbackLoadOutcome> {
    load_record(path)
}

/// Clear the pending rollback marker (idempotent).
///
/// Equivalent to bash `clear_pending_rollback` (publish.sh:411-413):
/// `rm -f` — no error if the file is missing.
#[must_use = "clear_pending_rollback removes the marker as a side effect"]
#[tracing::instrument(skip_all, fields(path = %path.display()))]
pub fn clear_pending_rollback(path: &Path) -> Result<()> {
    remove_file_if_exists(path)
}

/// Write the last-applied rollback record atomically.
///
/// Equivalent to bash `write_last_rollback` (publish.sh:415-427).
/// Same format as `write_pending_rollback`.
#[must_use = "write_last_rollback writes a state file as a side effect"]
#[tracing::instrument(skip_all, fields(path = %path.display()))]
pub fn write_last_rollback(path: &Path, from: &str, to: &str, channel: Option<&str>) -> Result<()> {
    write_record(path, from, to, channel)
}

/// Load the last-applied rollback record.
///
/// Equivalent to bash `load_last_rollback` (publish.sh:429-434).
#[must_use = "load_last_rollback returns the parsed record or a miss/invalid outcome"]
#[tracing::instrument(skip_all, fields(path = %path.display()))]
pub fn load_last_rollback(path: &Path) -> Result<RollbackLoadOutcome> {
    load_record(path)
}

/// Clear the last-applied rollback record (idempotent).
///
/// Equivalent to bash `clear_last_rollback` (publish.sh:436-442).
#[must_use = "clear_last_rollback removes the record as a side effect"]
#[tracing::instrument(skip_all, fields(path = %path.display()))]
pub fn clear_last_rollback(path: &Path) -> Result<()> {
    remove_file_if_exists(path)
}

/// Validate that the existing `current` pointer (if any) is well-formed.
///
/// Equivalent to bash `validate_existing_current` (publish.sh:293-299).
///
/// Semantics:
/// - No `current` pointer → `Ok(())` (nothing to validate; safe to write).
/// - `current` exists, symlink target resolves → `Ok(())`.
/// - `current` exists but target is missing (broken symlink) → `Err(invalid_state)`.
/// - `current` target directory doesn't exist → `Err(invalid_state)`.
///
/// Returns the resolved current path (if valid) for callers that want to
/// inspect it without re-reading.
#[must_use = "validate_existing_current checks pointer health as a precondition"]
#[tracing::instrument(skip_all, fields(images_root = %images_root.display()))]
pub fn validate_existing_current(images_root: &Path) -> Result<Option<PathBuf>> {
    let current_link = images_root.join("current");
    if !current_link.is_symlink() {
        return Ok(None);
    }
    let target = std::fs::read_link(&current_link)?;
    let resolved = images_root.join(
        target
            .file_name()
            .ok_or_else(|| GarError::validation("current symlink target has no filename"))?,
    );
    if !resolved.is_dir() {
        return Err(GarError::validation(format!(
            "current aponta para '{}' mas o diretorio nao existe",
            resolved.display()
        )));
    }
    Ok(Some(resolved))
}

/// Remove a stale `staged` pointer (top-level + per-channel), warn on each removal.
///
/// Equivalent to bash `clear_stale_staged_pointer` (publish.sh:263-291).
///
/// Cleans:
/// 1. Top-level `staged` pointer (`<images_root>/staged`)
/// 2. Per-channel `staged-generic`, `staged-lab`, `staged-rescue` pointers
///
/// Each removal is logged via `tracing::warn!`. Returns the number of
/// pointers cleared (useful for tests + audit).
#[must_use = "clear_stale_staged_pointer removes stale symlinks as a side effect"]
#[tracing::instrument(skip_all, fields(images_root = %images_root.display()))]
pub fn clear_stale_staged_pointer(images_root: &Path) -> Result<usize> {
    use crate::cli::Channel;
    use crate::services::channel::channel_staged_pointer;
    let mut cleared = 0;

    // 1. Top-level staged
    let top_staged = images_root.join("staged");
    if top_staged.is_symlink() {
        if let Some(target) = read_symlink_target(&top_staged)? {
            tracing::warn!(target: "gar::rollback", "Removendo ponteiro staged residual: {}", target);
            std::fs::remove_file(&top_staged)?;
            cleared += 1;
        }
    }

    // 2. Per-channel staged
    for channel in [Channel::Generic, Channel::Lab, Channel::Rescue] {
        let name = channel_staged_pointer(channel);
        let link = images_root.join(name);
        if link.is_symlink() {
            if let Some(target) = read_symlink_target(&link)? {
                tracing::warn!(
                    target: "gar::rollback",
                    "Removendo ponteiro {} residual: {}",
                    name,
                    target
                );
                std::fs::remove_file(&link)?;
                cleared += 1;
            }
        }
    }
    Ok(cleared)
}

// -------- internal helpers --------

fn write_record(path: &Path, from: &str, to: &str, channel: Option<&str>) -> Result<()> {
    if from.is_empty() || to.is_empty() {
        return Err(GarError::invalid_argument(format!(
            "write {}: from e to sao obrigatorios",
            path.display()
        )));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    let content = format!(
        "source={}\ntarget={}\nchannel={}\n",
        from,
        to,
        channel.unwrap_or(""),
    );
    std::fs::write(&tmp, content)?;
    // Atomic rename. If rename fails (e.g. cross-device), remove tmp to avoid leak.
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(GarError::Io(e));
    }
    Ok(())
}

fn load_record(path: &Path) -> Result<RollbackLoadOutcome> {
    if !path.is_file() {
        return Ok(RollbackLoadOutcome::Missing);
    }
    let content = std::fs::read_to_string(path)?;
    let mtime = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(DateTime::from);

    let mut from = None;
    let mut to = None;
    let mut channel = None;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "source" => from = Some(v.trim().to_string()),
                "target" => to = Some(v.trim().to_string()),
                "channel" => {
                    let v = v.trim();
                    channel = if v.is_empty() {
                        None
                    } else {
                        Some(v.to_string())
                    };
                }
                _ => {} // ignore unknown keys (forward compat)
            }
        }
    }

    match (&from, &to) {
        (Some(from), Some(to)) => Ok(RollbackLoadOutcome::Loaded(RollbackRecord {
            from: from.clone(),
            to: to.clone(),
            channel,
            mtime,
        })),
        _ => Ok(RollbackLoadOutcome::Invalid(format!(
            "missing required field(s): source={:?}, target={:?}",
            from.as_deref().unwrap_or(""),
            to.as_deref().unwrap_or("")
        ))),
    }
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(GarError::Io(e)),
    }
}

fn read_symlink_target(link: &Path) -> Result<Option<String>> {
    match std::fs::read_link(link) {
        Ok(target) => Ok(target.to_str().map(|s| s.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(GarError::Io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gar-rb-{}-{}-{}",
            name,
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    // ---- write_pending_rollback ----

    #[test]
    fn test_write_pending_happy_path() {
        let dir = tmp("wp-happy");
        let p = dir.join("rollback.pending");
        write_pending_rollback(&p, "v20260809-100000", "v20260808-090000", Some("generic"))
            .unwrap();
        let content = fs::read_to_string(&p).unwrap();
        assert!(content.contains("source=v20260809-100000"));
        assert!(content.contains("target=v20260808-090000"));
        assert!(content.contains("channel=generic"));
        cleanup(&dir);
    }

    #[test]
    fn test_write_pending_rejects_empty_fields() {
        let dir = tmp("wp-empty");
        let p = dir.join("rollback.pending");
        assert!(write_pending_rollback(&p, "", "v1", None).is_err());
        assert!(write_pending_rollback(&p, "v1", "", None).is_err());
        assert!(!p.exists(), "no file should be created on validation error");
        cleanup(&dir);
    }

    #[test]
    fn test_write_pending_atomic_via_rename() {
        // Verify no leftover .tmp files after write.
        let dir = tmp("wp-atomic");
        let p = dir.join("rollback.pending");
        write_pending_rollback(&p, "a", "b", None).unwrap();
        let tmps: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.contains(".tmp."))
                    .unwrap_or(false)
            })
            .collect();
        assert_eq!(tmps.len(), 0, "atomic rename should leave no .tmp files");
        cleanup(&dir);
    }

    // ---- load_pending_rollback ----

    #[test]
    fn test_load_pending_happy_path() {
        let dir = tmp("lp-happy");
        let p = dir.join("rollback.pending");
        write_pending_rollback(&p, "from-v", "to-v", Some("lab")).unwrap();
        let outcome = load_pending_rollback(&p).unwrap();
        match outcome {
            RollbackLoadOutcome::Loaded(rec) => {
                assert_eq!(rec.from, "from-v");
                assert_eq!(rec.to, "to-v");
                assert_eq!(rec.channel, Some("lab".into()));
                assert!(rec.mtime.is_some());
            }
            _ => panic!("expected Loaded, got {:?}", outcome),
        }
        cleanup(&dir);
    }

    #[test]
    fn test_load_pending_missing_returns_missing() {
        let dir = tmp("lp-miss");
        let p = dir.join("nope.pending");
        let outcome = load_pending_rollback(&p).unwrap();
        assert!(matches!(outcome, RollbackLoadOutcome::Missing));
        cleanup(&dir);
    }

    #[test]
    fn test_load_pending_invalid_missing_target() {
        let dir = tmp("lp-invalid");
        let p = dir.join("rollback.pending");
        fs::write(&p, "source=from-only\n").unwrap(); // no target
        let outcome = load_pending_rollback(&p).unwrap();
        assert!(matches!(outcome, RollbackLoadOutcome::Invalid(_)));
        cleanup(&dir);
    }

    // ---- clear_pending_rollback ----

    #[test]
    fn test_clear_pending_idempotent_on_existing() {
        let dir = tmp("cp-exists");
        let p = dir.join("rollback.pending");
        write_pending_rollback(&p, "a", "b", None).unwrap();
        assert!(p.exists());
        clear_pending_rollback(&p).unwrap();
        assert!(!p.exists());
        cleanup(&dir);
    }

    #[test]
    fn test_clear_pending_idempotent_on_missing() {
        let dir = tmp("cp-miss");
        let p = dir.join("rollback.pending");
        // Missing — should not error.
        clear_pending_rollback(&p).unwrap();
        cleanup(&dir);
    }

    #[test]
    fn test_clear_pending_does_not_remove_other_files() {
        let dir = tmp("cp-isolation");
        let p = dir.join("rollback.pending");
        let other = dir.join("rollback.last");
        write_pending_rollback(&p, "a", "b", None).unwrap();
        write_last_rollback(&other, "c", "d", None).unwrap();
        clear_pending_rollback(&p).unwrap();
        assert!(!p.exists());
        assert!(other.exists(), "clear_pending must not touch rollback.last");
        cleanup(&dir);
    }

    // ---- write_last_rollback / load_last_rollback / clear_last_rollback ----

    #[test]
    fn test_write_load_clear_last_roundtrip() {
        let dir = tmp("last-roundtrip");
        let p = dir.join("rollback.last");
        write_last_rollback(&p, "vX", "vY", Some("rescue")).unwrap();
        let outcome = load_last_rollback(&p).unwrap();
        match outcome {
            RollbackLoadOutcome::Loaded(rec) => {
                assert_eq!(rec.from, "vX");
                assert_eq!(rec.to, "vY");
                assert_eq!(rec.channel, Some("rescue".into()));
            }
            _ => panic!("expected Loaded"),
        }
        clear_last_rollback(&p).unwrap();
        assert!(matches!(
            load_last_rollback(&p).unwrap(),
            RollbackLoadOutcome::Missing
        ));
        cleanup(&dir);
    }

    #[test]
    fn test_write_last_rejects_empty_fields() {
        let dir = tmp("last-empty");
        let p = dir.join("rollback.last");
        assert!(write_last_rollback(&p, "", "v1", None).is_err());
        assert!(write_last_rollback(&p, "v1", "", None).is_err());
        cleanup(&dir);
    }

    #[test]
    fn test_load_last_missing_returns_missing() {
        let dir = tmp("last-miss");
        let p = dir.join("nope.last");
        let outcome = load_last_rollback(&p).unwrap();
        assert!(matches!(outcome, RollbackLoadOutcome::Missing));
        cleanup(&dir);
    }

    // ---- validate_existing_current ----

    #[test]
    fn test_validate_no_current_returns_ok_none() {
        let dir = tmp("vc-none");
        // No 'current' symlink → Ok(None).
        let result = validate_existing_current(&dir).unwrap();
        assert!(result.is_none());
        cleanup(&dir);
    }

    #[test]
    fn test_validate_valid_current_returns_ok_some() {
        let dir = tmp("vc-ok");
        // Create a real generation directory and a 'current' symlink to it.
        let gen_dir = dir.join("v20260809-100000");
        fs::create_dir_all(&gen_dir).unwrap();
        std::os::unix::fs::symlink("v20260809-100000", dir.join("current")).unwrap();
        let result = validate_existing_current(&dir).unwrap();
        assert_eq!(result, Some(gen_dir));
        cleanup(&dir);
    }

    #[test]
    fn test_validate_broken_current_returns_error() {
        let dir = tmp("vc-broken");
        // Symlink pointing to a non-existent target.
        std::os::unix::fs::symlink("v-nonexistent", dir.join("current")).unwrap();
        let result = validate_existing_current(&dir);
        assert!(result.is_err(), "broken symlink must error");
        cleanup(&dir);
    }

    // ---- clear_stale_staged_pointer ----

    #[test]
    fn test_clear_staged_no_pointers_returns_zero() {
        let dir = tmp("cs-empty");
        let cleared = clear_stale_staged_pointer(&dir).unwrap();
        assert_eq!(cleared, 0);
        cleanup(&dir);
    }

    #[test]
    fn test_clear_staged_removes_top_level() {
        let dir = tmp("cs-top");
        let target = dir.join("v20260809-broken");
        fs::create_dir_all(&target).unwrap();
        std::os::unix::fs::symlink("v20260809-broken", dir.join("staged")).unwrap();
        let cleared = clear_stale_staged_pointer(&dir).unwrap();
        assert_eq!(cleared, 1);
        assert!(!dir.join("staged").exists());
        cleanup(&dir);
    }

    #[test]
    fn test_clear_staged_removes_all_three_channels() {
        let dir = tmp("cs-channels");
        for name in ["staged-generic", "staged-lab", "staged-rescue"] {
            let target = dir.join(format!("v-old-{}", name));
            fs::create_dir_all(&target).unwrap();
            std::os::unix::fs::symlink(format!("v-old-{}", name), dir.join(name)).unwrap();
        }
        let cleared = clear_stale_staged_pointer(&dir).unwrap();
        assert_eq!(cleared, 3);
        for name in ["staged-generic", "staged-lab", "staged-rescue"] {
            assert!(!dir.join(name).exists());
        }
        cleanup(&dir);
    }

    // ---- RollbackLoadOutcome helpers ----

    #[test]
    fn test_outcome_into_option() {
        let r = RollbackRecord {
            from: "a".into(),
            to: "b".into(),
            channel: None,
            mtime: None,
        };
        let o = RollbackLoadOutcome::Loaded(r.clone());
        assert_eq!(o.into_option(), Some(r));
        assert_eq!(RollbackLoadOutcome::Missing.into_option(), None);
        assert_eq!(RollbackLoadOutcome::Invalid("x".into()).into_option(), None);
    }

    #[test]
    fn test_outcome_is_loaded() {
        let r = RollbackRecord {
            from: "a".into(),
            to: "b".into(),
            channel: None,
            mtime: None,
        };
        assert!(RollbackLoadOutcome::Loaded(r).is_loaded());
        assert!(!RollbackLoadOutcome::Missing.is_loaded());
        assert!(!RollbackLoadOutcome::Invalid("x".into()).is_loaded());
    }
}
