//! Tier 1 storage readiness + generation directory validation.
//!
//! Replaces `storage_mount_ready`, `ensure_tier1_ready`, and
//! `ensure_generation_dir` from `ragc/lib/publish.sh` (publish.sh:184-228).
//!
//! ## Semantics
//!
//! - `storage_mount_ready` is **advisory**: returns `bool` (not `Result`)
//!   because the bash version is a pure predicate (`mountpoint -q`).
//! - `ensure_tier1_ready` enforces preconditions for publishing a new
//!   image (tier 1 + http root must exist, data root must be mounted).
//!   Bash exits via `die` on failure; here we return `Err`.
//! - `ensure_generation_dir` validates the artifact set of a generation
//!   directory (bzImage, initrd, `.init_path`, `.kernel_params`,
//!   `manifest.json`) before any rollback/promote uses it.
//!
//! The original bash honored `RAGC_SKIP_STORAGE_CHECKS=1` for hermetic
//! tests. We keep that escape hatch — honored when `Config` is loaded
//! with that env var set — but tests pass the paths explicitly to skip
//! the env dependency.

use std::path::Path;
use std::process::Command;

use crate::config::Config;
use crate::error::{GarError, Result};

/// Returns `true` if either `data_root` or `images_root` is an active
/// mountpoint. Mirrors bash `storage_mount_ready`:
///
/// ```sh
/// mountpoint -q "$DATA_ROOT" || mountpoint -q "$IMAGES_ROOT"
/// ```
///
/// We delegate to `/usr/bin/mountpoint` (a one-shot syscall) instead of
/// parsing `/proc/mounts`, matching the bash behavior. Returns `false`
/// silently when neither path exists — bash would also fail silently.
#[must_use = "storage_mount_ready is a predicate; ignoring the result hides a missing tier"]
#[tracing::instrument(skip_all, fields(data_root = %data_root.display(), images_root = %images_root.display()))]
pub fn storage_mount_ready(data_root: &Path, images_root: &Path) -> bool {
    if let Ok(out) = Command::new("mountpoint").arg("-q").arg(data_root).output() {
        if out.status.success() {
            return true;
        }
    }
    if let Ok(out) = Command::new("mountpoint").arg("-q").arg(images_root).output() {
        if out.status.success() {
            return true;
        }
    }
    false
}

/// Test-only / hermetic hook: when this env var is set to "1", the
/// `ensure_*` helpers treat the storage layer as ready without probing
/// mountpoints. Matches the bash `RAGC_SKIP_STORAGE_CHECKS=1` flag.
pub const SKIP_STORAGE_CHECKS_ENV: &str = "RAGC_SKIP_STORAGE_CHECKS";

fn skip_storage_checks() -> bool {
    std::env::var(SKIP_STORAGE_CHECKS_ENV)
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Ensure that the tier 1 storage is operational: images root and HTTP
/// root directories exist, and the data tier is mounted. Refuses to
/// publish otherwise. Mirrors `ensure_tier1_ready` from publish.sh.
///
/// Honors `RAGC_SKIP_STORAGE_CHECKS=1` (skips the mountpoint probe) —
/// same escape hatch the ragc upstream uses for tests.
#[must_use = "ensure_tier1_ready is a precondition gate; ignoring the Result may publish into a broken tier"]
#[tracing::instrument(skip_all, fields(images_root = %cfg.images_root.display(), http_root = %cfg.http_root.display()))]
pub fn ensure_tier1_ready(cfg: &Config) -> Result<()> {
    if !cfg.images_root.is_dir() {
        return Err(GarError::Publish(format!(
            "Tier 1 indisponivel: diretorio de imagens ausente ({})",
            cfg.images_root.display()
        )));
    }
    if !cfg.http_root.is_dir() {
        return Err(GarError::Publish(format!(
            "HTTP root ausente: {}",
            cfg.http_root.display()
        )));
    }
    if !skip_storage_checks() && !storage_mount_ready(&cfg.data_root, &cfg.images_root) {
        return Err(GarError::Publish(format!(
            "Tier 1 indisponivel: {} nao esta montado de forma pronta para operacao",
            cfg.data_root.display()
        )));
    }
    Ok(())
}

/// Required artifact set for a published generation directory.
///
/// Mirrors the bash `ensure_generation_dir` checks:
/// - directory exists
/// - `bzImage` exists
/// - `initrd` exists
/// - `.init_path` exists
/// - `.kernel_params` exists
/// - manifest validates against the expected build id
#[must_use = "ensure_generation_dir is a precondition gate; ignoring the Result may publish an incomplete generation"]
#[tracing::instrument(skip_all, fields(generation_dir = %generation_dir.display(), generation_id = %generation_id))]
pub fn ensure_generation_dir(generation_dir: &Path, generation_id: &str) -> Result<()> {
    if !generation_dir.is_dir() {
        return Err(GarError::Publish(format!(
            "Geracao ausente: {}",
            generation_dir.display()
        )));
    }
    for artifact in &["bzImage", "initrd", ".init_path", ".kernel_params"] {
        let p = generation_dir.join(artifact);
        if !p.is_file() {
            return Err(GarError::Publish(format!(
                "Artefato incompleto: {} ausente em {}",
                artifact,
                generation_dir.display()
            )));
        }
    }
    // Manifest validation — matches the bash `validate_manifest` call at
    // the tail of the bash function.
    crate::services::manifest::validate(generation_dir, generation_id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// RAII guard for `std::env::set_var` in tests. Restores the previous
    /// value on drop (or removes the var if it was unset before). Prevents
    /// env var leaks across tests when an assertion panics.
    struct ScopedEnv {
        key: String,
        prev: Option<String>,
    }

    impl ScopedEnv {
        fn set(key: &str, value: &str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self {
                key: key.to_string(),
                prev,
            }
        }
    }

    impl Drop for ScopedEnv {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(&self.key, v),
                None => std::env::remove_var(&self.key),
            }
        }
    }

    fn tmp(label: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("gar-storage-{}-{}", label, std::process::id()));
        // Clean stale dir from previous runs in the same PID namespace.
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn make_cfg(images_root: PathBuf, http_root: PathBuf, data_root: PathBuf) -> Config {
        let mut cfg = Config::default();
        cfg.images_root = images_root;
        cfg.http_root = http_root;
        cfg.data_root = data_root;
        cfg
    }

    #[test]
    fn test_storage_mount_ready_false_for_regular_dirs() {
        // Regular dirs are NOT mountpoints — should return false.
        let dir = tmp("mount-false");
        assert!(!storage_mount_ready(&dir, &dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_storage_mount_ready_returns_bool_not_result() {
        // Type-level: this is a predicate, not a fallible gate.
        // We assert the signature returns plain bool via inference:
        let dir = tmp("mount-bool");
        let r: bool = storage_mount_ready(&dir, &dir);
        assert!(!r);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_ensure_tier1_ready_happy_path() {
        // Skip storage check flag bypasses the mountpoint probe.
        let _env = ScopedEnv::set(SKIP_STORAGE_CHECKS_ENV, "1");
        let images = tmp("tier1-img");
        let http = tmp("tier1-http");
        let data = tmp("tier1-data");
        let cfg = make_cfg(images.clone(), http.clone(), data.clone());

        ensure_tier1_ready(&cfg).expect("happy path with skip flag should pass");
        let _ = fs::remove_dir_all(&images);
        let _ = fs::remove_dir_all(&http);
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn test_ensure_tier1_ready_errors_when_images_root_missing() {
        let _env = ScopedEnv::set(SKIP_STORAGE_CHECKS_ENV, "1");
        let http = tmp("tier1-noimg-http");
        let data = tmp("tier1-noimg-data");

        let cfg = make_cfg(
            std::env::temp_dir().join(format!(
                "gar-storage-tier1-noimg-{}-missing",
                std::process::id()
            )),
            http.clone(),
            data.clone(),
        );

        let err = ensure_tier1_ready(&cfg).unwrap_err();
        assert!(matches!(err, GarError::Publish(_)));
        assert!(err.to_string().contains("imagens ausente"));
        let _ = fs::remove_dir_all(&http);
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn test_ensure_tier1_ready_errors_when_http_root_missing() {
        let _env = ScopedEnv::set(SKIP_STORAGE_CHECKS_ENV, "1");
        let images = tmp("tier1-nohttp");
        let data = tmp("tier1-nohttp-data");

        let cfg = make_cfg(
            images.clone(),
            std::env::temp_dir().join(format!(
                "gar-storage-tier1-nohttp-{}-missing",
                std::process::id()
            )),
            data.clone(),
        );

        let err = ensure_tier1_ready(&cfg).unwrap_err();
        assert!(err.to_string().contains("HTTP root ausente"));
        let _ = fs::remove_dir_all(&images);
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn test_ensure_generation_dir_happy_path() {
        let gen = tmp("gen-happy");
        // Build the required artifact set + a valid manifest.
        fs::write(gen.join("bzImage"), b"kernel").unwrap();
        fs::write(gen.join("initrd"), b"initrd").unwrap();
        fs::write(gen.join(".init_path"), "/nix/store/abc-init").unwrap();
        fs::write(gen.join(".kernel_params"), "quiet splash").unwrap();
        // Construct the manifest.json manually with id == generation_id.
        // (Mirrors what manifest::write does, without depending on the
        // private sample_manifest helper in manifest.rs's tests module.)
        let manifest_json = r#"{
            "id": "v20260101-120000",
            "timestamp": "2026-01-01T12:00:00Z",
            "system_path": "/nix/store/abc-system",
            "init_path": "/nix/store/abc-init",
            "artifacts": { "kernel": "bzImage", "initrd": "initrd" },
            "checksums": { "kernel": "deadbeef", "initrd": "cafebabe" },
            "status": "active",
            "target": "desktop-generic",
            "channel": "generic",
            "hardwareClass": "physical-generic"
        }"#;
        fs::write(gen.join("manifest.json"), manifest_json).unwrap();

        ensure_generation_dir(&gen, "v20260101-120000").expect("happy path");
        let _ = fs::remove_dir_all(&gen);
    }

    #[test]
    fn test_ensure_generation_dir_missing_bzimage() {
        let gen = tmp("gen-no-bz");
        fs::write(gen.join("initrd"), b"initrd").unwrap();
        fs::write(gen.join(".init_path"), "/init").unwrap();
        fs::write(gen.join(".kernel_params"), "quiet").unwrap();

        let err = ensure_generation_dir(&gen, "v1").unwrap_err();
        assert!(err.to_string().contains("bzImage"));
        let _ = fs::remove_dir_all(&gen);
    }

    #[test]
    fn test_ensure_generation_dir_missing_init_path() {
        let gen = tmp("gen-no-init");
        fs::write(gen.join("bzImage"), b"k").unwrap();
        fs::write(gen.join("initrd"), b"i").unwrap();
        // missing .init_path
        fs::write(gen.join(".kernel_params"), "q").unwrap();

        let err = ensure_generation_dir(&gen, "v1").unwrap_err();
        assert!(err.to_string().contains(".init_path"));
        let _ = fs::remove_dir_all(&gen);
    }

    #[test]
    fn test_ensure_generation_dir_missing_directory() {
        let missing = std::env::temp_dir().join(format!(
            "gar-storage-gen-missing-{}-{}",
            std::process::id(),
            std::process::id()
        ));
        let err = ensure_generation_dir(&missing, "v1").unwrap_err();
        assert!(err.to_string().contains("Geracao ausente"));
    }

    #[test]
    fn test_scoped_env_restores_previous_value() {
        // Sanity: the guard actually restores — without this, the
        // other tests' set_var calls would leak into this test.
        let prev = std::env::var(SKIP_STORAGE_CHECKS_ENV).ok();
        {
            let _g = ScopedEnv::set(SKIP_STORAGE_CHECKS_ENV, "1");
            assert_eq!(
                std::env::var(SKIP_STORAGE_CHECKS_ENV).ok().as_deref(),
                Some("1")
            );
        }
        assert_eq!(
            std::env::var(SKIP_STORAGE_CHECKS_ENV).ok(),
            prev,
            "ScopedEnv must restore previous value on drop"
        );
    }
}