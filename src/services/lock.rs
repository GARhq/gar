//! Global lock (mutex) for mutating operations.
//!
//! Replaces `ragc/lib/lock.sh` (RAGC_LOCK_PATH, acquire/release_global_lock).
//! Uses `flock` semantics via `fs2` crate (not added as dependency yet —
//! currently a stub that uses `mkdir`-based mutual exclusion).
//!
//! TODO: Use fs2 or nix crate for proper flock semantics.

use std::path::Path;

use crate::error::{GarError, Result};

/// Default lock path.
pub const DEFAULT_LOCK_PATH: &str = "/run/gar.lock";

/// Try to acquire the global lock without blocking.
///
/// Returns `Ok(true)` if acquired, `Ok(false)` if held by another process.
pub fn try_acquire(lock_path: &Path) -> Result<bool> {
    if lock_path.exists() {
        // Check if it's stale (older than 5 minutes)
        let metadata = std::fs::metadata(lock_path)?;
        let mtime = metadata.modified()?;
        let age = mtime.elapsed().unwrap_or_default();
        if age.as_secs() > 300 {
            // Stale lock — remove and retry
            std::fs::remove_file(lock_path)?;
        } else {
            return Ok(false);
        }
    }

    // Create lock file (mkdir-style)
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(lock_path, format!("pid={}\n", std::process::id()))?;
    Ok(true)
}

/// Release the global lock.
pub fn release(lock_path: &Path) -> Result<()> {
    if lock_path.exists() {
        std::fs::remove_file(lock_path)?;
    }
    Ok(())
}

/// Run a closure while holding the global lock.
pub fn with_lock<F, T>(lock_path: &Path, label: &str, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    let acquired = try_acquire(lock_path)?;
    if !acquired {
        return Err(GarError::LockHeld);
    }
    tracing::info!(target: "gar::lock", "Lock acquired: {}", label);
    let result = f();
    release(lock_path)?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_path_default() {
        assert_eq!(DEFAULT_LOCK_PATH, "/run/gar.lock");
    }

    #[test]
    fn test_try_acquire_release() {
        let tmp = std::env::temp_dir().join(format!("gar-lock-test-{}.lock", std::process::id()));
        let _ = std::fs::remove_file(&tmp);

        assert!(try_acquire(&tmp).unwrap());
        assert!(try_acquire(&tmp).unwrap_or(false) == false); // Already held
        release(&tmp).unwrap();
        assert!(!tmp.exists());
    }

    #[test]
    fn test_with_lock_runs() {
        let tmp = std::env::temp_dir().join(format!(
            "gar-lock-runs-{}-{}.lock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let result = with_lock(&tmp, "test", || Ok::<i32, GarError>(42));
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_with_lock_blocks_double_acquire() {
        let tmp = std::env::temp_dir().join(format!(
            "gar-lock-blocks-{}-{}.lock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&tmp);

        // Manually create lock file as if held by another process
        std::fs::write(&tmp, "pid=99999\n").unwrap();

        // with_lock should now fail with LockHeld
        let result = with_lock(&tmp, "second", || Ok::<(), GarError>(()));

        let _ = std::fs::remove_file(&tmp);
        assert!(matches!(result, Err(GarError::LockHeld)));
    }
}