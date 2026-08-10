//! Global lock (mutex) for mutating operations using real `flock(2)` syscalls.
//!
//! Replaces `ragc/lib/lock.sh` (`acquire_global_lock`, `release_global_lock`,
//! `with_global_lock`). Uses the `nix::fcntl::Flock<T>` wrapper for
//! kernel-level mutex with RAII Drop semantics, timeout via polling,
//! and owner tracking (pid + cmdline) in a `.owner` sidecar file.
//!
//! Design notes:
//! - `Flock::lock(file, FlockArg::LockExclusiveNonblock)` is non-blocking.
//! - `Flock::lock(file, FlockArg::LockExclusive)` blocks forever; we wrap
//!   with `acquire_with_timeout` that polls every 100ms to match bash
//!   `flock -w 60`.
//! - Owner metadata (`pid`, `command`, `started_at`) is written to a sidecar
//!   file `<lock_path>.owner` so other processes can see who holds it.
//! - Drop on `Flock<T>` closes the fd; the kernel auto-releases the lock.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use nix::fcntl::{Flock, FlockArg};

use crate::error::{GarError, Result};

/// Default lock path (`/run/gar.lock`).
pub const DEFAULT_LOCK_PATH: &str = "/run/gar.lock";

/// Default timeout in seconds for `with_lock` (matches bash `flock -w 60`).
pub const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Owner metadata written to `<lock_path>.owner`.
#[derive(Debug, Clone)]
pub struct Owner {
    pub pid: u32,
    pub command: String,
    pub started_at: String,
}

impl Owner {
    fn current() -> Self {
        let pid = std::process::id();
        let command = std::env::args().collect::<Vec<_>>().join(" ");
        let started_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        Self {
            pid,
            command,
            started_at,
        }
    }

    fn format(&self) -> String {
        format!(
            "pid={}\ncommand={}\nstarted_at={}\n",
            self.pid, self.command, self.started_at
        )
    }
}

/// Result of an acquisition attempt.
///
/// `Acquired` carries an `Flock<File>` whose Drop releases the lock.
#[derive(Debug)]
pub enum AcquireOutcome {
    /// Acquired successfully. The Flock<File> Drop releases on scope exit.
    Acquired(Flock<File>, Owner),
    /// Held by another process (we did not acquire).
    HeldByOther { owner: Option<Owner> },
}

/// Open the lock file (creating parents as needed).
fn open_lock_file(lock_path: &Path) -> Result<File> {
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)
        .map_err(|e| {
            GarError::lock(format!(
                "Nao foi possivel abrir lock file {}: {}",
                lock_path.display(),
                e
            ))
        })
}

/// Try to acquire the global lock without blocking.
///
/// Returns `Ok(AcquireOutcome::Acquired(flock, owner))` if acquired —
/// the `flock` value MUST be kept alive (e.g. via `with_lock`) or Drop
/// will release immediately.
///
/// Returns `Ok(AcquireOutcome::HeldByOther { .. })` if held by another.
pub fn try_acquire(lock_path: &Path) -> Result<AcquireOutcome> {
    let file = open_lock_file(lock_path)?;
    match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
        Ok(flock) => {
            let owner = Owner::current();
            write_owner(&owner_path(lock_path), &owner)?;
            Ok(AcquireOutcome::Acquired(flock, owner))
        }
        Err((file_back, nix::errno::Errno::EWOULDBLOCK)) => {
            // Held by another — read owner sidecar if present.
            let _ = file_back;
            let owner = read_owner(&owner_path(lock_path)).ok();
            Ok(AcquireOutcome::HeldByOther { owner })
        }
        Err((_, e)) => Err(GarError::lock(format!("flock falhou: {}", e))),
    }
}

/// Acquire with timeout (matches bash `flock -w TIMEOUT`).
///
/// Polls every 100ms. Returns `AcquireOutcome::HeldByOther` on timeout.
pub fn acquire_with_timeout(lock_path: &Path, timeout: Duration) -> Result<AcquireOutcome> {
    let start = Instant::now();
    loop {
        let outcome = try_acquire(lock_path)?;
        match outcome {
            AcquireOutcome::Acquired(flock, owner) => {
                return Ok(AcquireOutcome::Acquired(flock, owner))
            }
            AcquireOutcome::HeldByOther { owner } => {
                if start.elapsed() >= timeout {
                    return Ok(AcquireOutcome::HeldByOther { owner });
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// Explicitly release the lock (the owner sidecar is removed).
///
/// `with_lock` already calls this on Drop via `Flock<File>`. This function
/// is for callers who want to release early without dropping the guard.
pub fn release(lock_path: &Path) -> Result<()> {
    let owner_path = owner_path(lock_path);
    if owner_path.exists() {
        let _ = std::fs::remove_file(&owner_path);
    }
    Ok(())
}

/// Run a closure while holding the global lock with default 60s timeout.
pub fn with_lock<F, T>(lock_path: &Path, label: &str, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    with_lock_timeout(
        lock_path,
        label,
        Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        f,
    )
}

/// Run a closure while holding the global lock with custom timeout.
pub fn with_lock_timeout<F, T>(lock_path: &Path, label: &str, timeout: Duration, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    match acquire_with_timeout(lock_path, timeout)? {
        AcquireOutcome::Acquired(flock, owner) => {
            tracing::info!(target: "gar::lock", "Lock acquired by {}: {}", owner.pid, label);
            let result = f();
            drop(flock); // explicit release before owner sidecar cleanup
            release(lock_path)?;
            result
        }
        AcquireOutcome::HeldByOther { owner } => {
            let detail = match owner {
                Some(o) => format!(
                    "pid={} command={} started_at={}",
                    o.pid, o.command, o.started_at
                ),
                None => "owner metadata indisponivel".to_string(),
            };
            Err(GarError::LockHeld(format!(
                "Operacao bloqueada por lock global ({}): {}",
                lock_path.display(),
                detail
            )))
        }
    }
}

/// Resolve the owner metadata file path (public for tests).
pub fn owner_path(lock_path: &Path) -> PathBuf {
    lock_path.with_extension("owner")
}

fn write_owner(path: &Path, owner: &Owner) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    f.write_all(owner.format().as_bytes())?;
    f.sync_all()?;
    Ok(())
}

fn read_owner(path: &Path) -> Result<Owner> {
    let content = std::fs::read_to_string(path)?;
    let mut pid = 0u32;
    let mut command = String::new();
    let mut started_at = String::new();
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("pid=") {
            pid = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("command=") {
            command = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("started_at=") {
            started_at = rest.to_string();
        }
    }
    Ok(Owner {
        pid,
        command,
        started_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn temp_lock(name: &str) -> PathBuf {
        env::temp_dir().join(format!("gar-flock-{}-{}.lock", name, std::process::id()))
    }

    fn cleanup(path: &Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(owner_path(path));
    }

    #[test]
    fn test_acquire_and_release() {
        let path = temp_lock("basic");
        cleanup(&path);

        // Acquire, hold in scope, then drop and re-acquire.
        {
            let outcome = try_acquire(&path).unwrap();
            assert!(matches!(outcome, AcquireOutcome::Acquired(_, _)));
            // Drop here releases the lock
        }
        // After Drop, another try_acquire must succeed.
        let outcome2 = try_acquire(&path).unwrap();
        assert!(matches!(outcome2, AcquireOutcome::Acquired(_, _)));
        cleanup(&path);
    }

    #[test]
    fn test_double_acquire_blocked_immediately() {
        let path = temp_lock("double");
        cleanup(&path);

        let first = try_acquire(&path).unwrap();
        assert!(matches!(first, AcquireOutcome::Acquired(_, _)));

        // Hold first alive in scope to keep lock
        let _guard = first;

        // Second attempt must fail-fast with HeldByOther.
        let second = try_acquire(&path).unwrap();
        match second {
            AcquireOutcome::HeldByOther { owner } => {
                assert!(owner.is_some(), "owner sidecar should be present");
                let o = owner.unwrap();
                assert_eq!(o.pid, std::process::id());
            }
            _ => panic!("expected HeldByOther"),
        }

        cleanup(&path);
    }

    #[test]
    fn test_owner_metadata_persists() {
        let path = temp_lock("owner");
        cleanup(&path);

        let outcome = try_acquire(&path).unwrap();
        let owner_pid = match &outcome {
            AcquireOutcome::Acquired(_, owner) => owner.pid,
            _ => panic!("expected Acquired"),
        };
        // Keep the flock alive to inspect sidecar.
        let _guard = match outcome {
            AcquireOutcome::Acquired(f, _) => f,
            _ => unreachable!(),
        };

        // Sidecar file must exist with correct pid.
        let op = owner_path(&path);
        assert!(op.exists(), "owner sidecar missing");
        let read = read_owner(&op).unwrap();
        assert_eq!(read.pid, owner_pid);
        assert!(!read.started_at.is_empty());

        cleanup(&path);
    }

    #[test]
    fn test_acquire_with_timeout_blocks_then_times_out() {
        let path = temp_lock("timeout");
        cleanup(&path);

        // Hold the lock in scope.
        let first = try_acquire(&path).unwrap();
        assert!(matches!(first, AcquireOutcome::Acquired(_, _)));
        let _guard = match first {
            AcquireOutcome::Acquired(f, _) => f,
            _ => unreachable!(),
        };

        // Second caller should time out after 500ms.
        let start = Instant::now();
        let outcome = acquire_with_timeout(&path, Duration::from_millis(500)).unwrap();
        let elapsed = start.elapsed();
        assert!(matches!(outcome, AcquireOutcome::HeldByOther { .. }));
        assert!(
            elapsed >= Duration::from_millis(450),
            "timed out too early: {:?}",
            elapsed
        );
        assert!(
            elapsed < Duration::from_millis(800),
            "timed out too late: {:?}",
            elapsed
        );

        cleanup(&path);
    }

    #[test]
    fn test_with_lock_runs_callback() {
        let path = temp_lock("with");
        cleanup(&path);

        let result: i32 = with_lock(&path, "test-cb", || Ok(42)).unwrap();
        assert_eq!(result, 42);

        // After with_lock returns, lock should be free.
        let next = try_acquire(&path).unwrap();
        assert!(matches!(next, AcquireOutcome::Acquired(_, _)));
        cleanup(&path);
    }

    #[test]
    fn test_with_lock_returns_lock_held_when_contended() {
        let path = temp_lock("with-held");
        cleanup(&path);

        // Hold it ourselves in scope.
        let first = try_acquire(&path).unwrap();
        assert!(matches!(first, AcquireOutcome::Acquired(_, _)));
        let _guard = match first {
            AcquireOutcome::Acquired(f, _) => f,
            _ => unreachable!(),
        };

        // Try to acquire via with_lock with short timeout — must fail.
        let r: Result<()> =
            with_lock_timeout(&path, "contended", Duration::from_millis(300), || Ok(()));
        assert!(matches!(r, Err(GarError::LockHeld(_))));

        cleanup(&path);
    }

    #[test]
    fn test_default_lock_path_unchanged() {
        assert_eq!(DEFAULT_LOCK_PATH, "/run/gar.lock");
        assert_eq!(DEFAULT_TIMEOUT_SECS, 60);
    }

    #[test]
    fn test_drop_releases_lock() {
        let path = temp_lock("drop");
        cleanup(&path);

        // Acquire, then explicitly drop the Flock. After drop, re-acquire works.
        let outcome = try_acquire(&path).unwrap();
        let flock = match outcome {
            AcquireOutcome::Acquired(f, _) => f,
            _ => panic!("expected Acquired"),
        };
        // Cannot re-acquire while held.
        assert!(matches!(
            try_acquire(&path).unwrap(),
            AcquireOutcome::HeldByOther { .. }
        ));
        drop(flock);
        // Now should succeed.
        let outcome2 = try_acquire(&path).unwrap();
        assert!(matches!(outcome2, AcquireOutcome::Acquired(_, _)));
        cleanup(&path);
    }
}
