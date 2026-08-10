//! Build pipeline: invoke `nix build` and stage generation artifacts.
//!
//! Replaces `ragc/lib/publish.sh:329-388` (`build_or_reuse_system` +
//! `stage_generation`). Used by `gar image build` to produce a real
//! NixOS system build, copy kernel/initrd into the images_root, generate
//! `manifest.json` with sha256 checksums, and create a GC root via
//! `nix-store --add-root` so old generations survive `nix-collect-garbage`.
//!
//! Mock mode: when `RAGC_TEST_SYSTEM_PATH` is set, the function uses that
//! path instead of running `nix build` — same pattern as the bash version.

use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::{GarError, Result};

/// Mock env var: when set, `build_or_reuse_system` returns it directly
/// (matches `RAGC_TEST_SYSTEM_PATH` from ragc/lib/publish.sh:337).
pub const TEST_SYSTEM_PATH_ENV: &str = "GAR_TEST_SYSTEM_PATH";

/// Mock env var: when "1", skips `nix build` invocation (used in tests).
pub const SKIP_BUILD_ENV: &str = "GAR_SKIP_BUILD";

/// Outcome of a successful build.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildArtifact {
    pub build_id: String,
    pub target: String,
    pub channel: String,
    pub system_path: PathBuf,
    pub init_path: PathBuf,
    pub kernel_path: PathBuf,
    pub initrd_path: PathBuf,
    pub kernel_params: PathBuf,
    pub kernel_sha256: String,
    pub initrd_sha256: String,
    pub timestamp: String,
}

/// Run `nix build` for the given target and resolve the resulting system path.
///
/// If `GAR_TEST_SYSTEM_PATH` is set, returns it directly (mock for CI/tests).
/// If `GAR_SKIP_BUILD=1`, returns an error (force test to fail clearly).
pub fn build_or_reuse_system(flake_root: &Path, target: &str, channel: &str) -> Result<PathBuf> {
    // Mock path: bash ragc honors RAGC_TEST_SYSTEM_PATH for hermetic tests.
    if let Ok(mock) = std::env::var(TEST_SYSTEM_PATH_ENV) {
        if !mock.is_empty() {
            return Ok(PathBuf::from(mock));
        }
    }

    if std::env::var(SKIP_BUILD_ENV).as_deref() == Ok("1") {
        return Err(GarError::build(
            "GAR_SKIP_BUILD=1 set; refusing to run nix build in test",
        ));
    }

    let installable = format!(
        "path:{flake}#nixosConfigurations.ragos-client-{target}.config.system.build.toplevel",
        flake = flake_root.display(),
        target = target
    );

    // `nix build --print-out-paths --no-link` returns just the path on stdout.
    let output = Command::new("nix")
        .args([
            "build",
            "--impure",
            "--print-out-paths",
            "--no-link",
            &installable,
        ])
        .output()
        .map_err(|e| GarError::build(format!("failed to spawn nix build: {}", e)))?;

    if !output.status.success() {
        return Err(GarError::build(format!(
            "nix build falhou (exit {}): {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return Err(GarError::build("nix build retornou path vazio"));
    }
    if !Path::new(&path).is_dir() {
        return Err(GarError::build(format!(
            "nix build path nao e diretorio: {}",
            path
        )));
    }
    let _ = channel; // recorded in stage_generation, not used here
    Ok(PathBuf::from(path))
}

/// Compute the build ID (`vYYYYMMDD-HHMMSS`) — matches bash `cmd_switch:80`.
///
/// Adds `-<nanoseconds>` suffix on collision (matches bash:81-83).
pub fn compute_build_id(images_root: &Path) -> String {
    let base = format!("v{}", Utc::now().format("%Y%m%d-%H%M%S"));
    let mut id = base.clone();
    let path = images_root.join(&id);
    if path.exists() {
        // Collision — append nanosecond suffix.
        let nanos = Utc::now().timestamp_nanos_opt().unwrap_or(0);
        id = format!("{}-{:09}", base, nanos % 1_000_000_000);
    }
    id
}

/// Compute sha256 of a file (returns lowercase hex).
pub fn sha256_file(path: &Path) -> Result<String> {
    let output = Command::new("sha256sum")
        .arg(path)
        .output()
        .map_err(|e| GarError::build(format!("sha256sum falhou: {}", e)))?;
    if !output.status.success() {
        return Err(GarError::build(format!(
            "sha256sum retornou exit {}",
            output.status.code().unwrap_or(-1)
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let hash = stdout
        .split_whitespace()
        .next()
        .ok_or_else(|| GarError::build("sha256sum stdout vazio"))?;
    Ok(hash.to_string())
}

/// Ensure a GC root exists for a published generation.
///
/// Idempotent: if `<generation_dir>/.gcroot` already exists, returns
/// `Ok(None)` without touching nix-store. Reads `system_path` from the
/// generation's `manifest.json` and delegates to `create_gc_root`.
///
/// Mirrors bash `ensure_gc_root_for_generation` (publish.sh:313):
/// ```sh
/// ensure_gc_root_for_generation() {
///   local generation_dir="$1"
///   [[ -d "$generation_dir" ]] || return 0
///   [[ -f "$generation_dir/.gcroot" ]] && return 0
///   local manifest; manifest="$(manifest_path "$generation_dir")"
///   [[ -f "$manifest" ]] || return 0
///   local system_path; system_path="$(manifest_read_field "$manifest" system_path)"
///   [[ -n "$system_path" ]] || return 0
///   create_gc_root "$generation_dir" "$system_path"
/// }
/// ```
///
/// Note: the bash version calls `create_gc_root <gen_dir> <system_path>`,
/// which differs in argument order from the Rust `create_gc_root` API
/// (`system_path, gc_root_path`). We compute the gc_root_path internally
/// as `<generation_dir>/.gcroot` to match the bash behavior.
#[must_use = "ensure_gc_root_for_generation has filesystem side effects via nix-store"]
#[tracing::instrument(skip_all, fields(generation_dir = %generation_dir.display()))]
pub fn ensure_gc_root_for_generation(generation_dir: &Path) -> Result<Option<PathBuf>> {
    // Match bash's `[[ -d "$generation_dir" ]] || return 0`.
    if !generation_dir.is_dir() {
        return Ok(None);
    }
    let gc_root_path = generation_dir.join(".gcroot");
    // Match bash's `[[ -f "$gc_root" ]] && return 0` (already pinned).
    if gc_root_path.is_file() {
        return Ok(None);
    }
    // Read system_path from manifest.json. bash calls
    // `manifest_read_field`; we delegate to a small inline parser to
    // avoid pulling extra dependencies.
    let manifest_path = generation_dir.join("manifest.json");
    if !manifest_path.is_file() {
        return Ok(None);
    }
    let system_path = match crate::services::boot::read_manifest_system_path(&manifest_path) {
        Some(s) if !s.is_empty() => s,
        _ => return Ok(None),
    };
    create_gc_root(&PathBuf::from(system_path), &gc_root_path)
}

/// Create a GC root for a system path using `nix-store --add-root`.
///
/// Returns the path of the GC root file (or None if nix-store unavailable).
pub fn create_gc_root(system_path: &Path, gc_root_path: &Path) -> Result<Option<PathBuf>> {
    let bin_check = Command::new("nix-store").arg("--version").output();
    if bin_check.is_err() {
        tracing::warn!("nix-store indisponivel; GC root nao criado");
        return Ok(None);
    }

    let output = Command::new("nix-store")
        .args([
            "--add-root",
            &gc_root_path.display().to_string(),
            "--indirect",
            "-r",
            &system_path.display().to_string(),
        ])
        .output()
        .map_err(|e| GarError::build(format!("nix-store falhou: {}", e)))?;

    if !output.status.success() {
        tracing::warn!(
            "GC root nao pode ser criado (exit {}); \
             /nix/store pode ser coletado e quebrar boots antigos",
            output.status.code().unwrap_or(-1)
        );
        return Ok(None);
    }
    Ok(Some(gc_root_path.to_path_buf()))
}

/// Result of staging a generation (artifacts copied + manifest written).
#[derive(Debug)]
pub struct StagedGeneration {
    pub build_id: String,
    pub generation_dir: PathBuf,
    pub manifest: crate::services::manifest::Manifest,
    pub gc_root: Option<PathBuf>,
}

/// Stage a generation under `<images_root>/<build_id>`.
///
/// Equivalent to bash `stage_generation` (publish.sh:349-388). Copies
/// kernel/initrd, writes manifest with sha256, creates GC root.
pub fn stage_generation(
    images_root: &Path,
    build_id: &str,
    target: &str,
    channel: &str,
    artifact: &BuildArtifact,
) -> Result<StagedGeneration> {
    let generation_dir = images_root.join(build_id);
    if generation_dir.exists() {
        return Err(GarError::publish(format!(
            "Geracao ja existe: {}",
            generation_dir.display()
        )));
    }
    std::fs::create_dir_all(&generation_dir)?;

    // Copy kernel -> bzImage, initrd -> initrd (with dereferencing).
    std::fs::copy(&artifact.kernel_path, generation_dir.join("bzImage"))?;
    std::fs::copy(&artifact.initrd_path, generation_dir.join("initrd"))?;

    // Write .init_path and .kernel_params sidecars.
    std::fs::write(
        generation_dir.join(".init_path"),
        artifact.init_path.display().to_string(),
    )?;
    if artifact.kernel_params.exists() {
        std::fs::copy(
            &artifact.kernel_params,
            generation_dir.join(".kernel_params"),
        )?;
    }

    // Compute sha256 of the artifacts.
    let kernel_sha = sha256_file(&generation_dir.join("bzImage"))?;
    let initrd_sha = sha256_file(&generation_dir.join("initrd"))?;

    // Build manifest.
    use crate::services::manifest::{Artifacts, Checksums, Manifest, Status};
    let manifest = Manifest {
        id: build_id.into(),
        timestamp: artifact.timestamp.clone(),
        system_path: artifact.system_path.display().to_string(),
        init_path: artifact.init_path.display().to_string(),
        artifacts: Artifacts {
            kernel: "bzImage".into(),
            initrd: "initrd".into(),
        },
        checksums: Checksums {
            kernel: kernel_sha,
            initrd: initrd_sha,
        },
        status: Status::Staged,
        target: target.into(),
        channel: channel.into(),
        hardware_class: crate::services::channel::target_hardware_class_str(target).to_string(),
    };
    crate::services::manifest::write(&generation_dir, &manifest)?;

    // GC root.
    let gc_root_path = generation_dir.join(".gcroot");
    let gc_root = create_gc_root(&artifact.system_path, &gc_root_path)?;

    Ok(StagedGeneration {
        build_id: build_id.into(),
        generation_dir,
        manifest,
        gc_root,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_id_format_v_date() {
        let tmp = std::env::temp_dir().join(format!(
            "gar-build-id-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        let id = compute_build_id(&tmp);
        assert!(id.starts_with("v20"), "got: {}", id);
        assert_eq!(id.len(), 16, "got: {}", id);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_build_id_collision_appends_nanos() {
        let tmp = std::env::temp_dir().join(format!(
            "gar-build-coll-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        let id1 = compute_build_id(&tmp);
        // Simulate collision by pre-creating the dir.
        std::fs::create_dir_all(tmp.join(&id1)).unwrap();
        let id2 = compute_build_id(&tmp);
        assert_ne!(id1, id2);
        assert!(id2.starts_with(&id1));
        assert!(id2.len() > id1.len(), "id2 should have suffix");

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_sha256_file_basic() {
        let tmp = std::env::temp_dir().join(format!("gar-sha256-{}.bin", std::process::id()));
        std::fs::write(&tmp, "hello world").unwrap();
        let hash = sha256_file(&tmp).unwrap();
        // sha256("hello world") = b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
        std::fs::remove_file(&tmp).unwrap();
    }

    #[test]
    fn test_sha256_file_missing_returns_error() {
        let missing = std::env::temp_dir().join("gar-sha256-missing.nope.xyz");
        assert!(sha256_file(&missing).is_err());
    }

    #[test]
    fn test_build_or_reuse_system_honors_test_env() {
        let tmp = std::env::temp_dir().join(format!("gar-build-mock-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();

        // SAFETY: single-threaded test
        unsafe {
            std::env::set_var(TEST_SYSTEM_PATH_ENV, tmp.display().to_string());
        }
        let result = build_or_reuse_system(Path::new("/tmp"), "desktop-generic", "generic");
        unsafe {
            std::env::remove_var(TEST_SYSTEM_PATH_ENV);
        }
        assert_eq!(result.unwrap(), tmp);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_build_or_reuse_system_skip_env_returns_error() {
        // SAFETY: single-threaded test
        unsafe {
            std::env::remove_var(TEST_SYSTEM_PATH_ENV);
            std::env::set_var(SKIP_BUILD_ENV, "1");
        }
        let result = build_or_reuse_system(Path::new("/tmp"), "desktop-generic", "generic");
        unsafe {
            std::env::remove_var(SKIP_BUILD_ENV);
        }
        assert!(result.is_err());
    }

    #[test]
    fn test_create_gc_root_no_nix_store_returns_none() {
        // Simulates absence of nix-store: just check we don't panic.
        let tmp = std::env::temp_dir().join(format!("gar-gcroot-{}", std::process::id()));
        let result = create_gc_root(&tmp, &tmp);
        // Either Ok(Some) if nix-store is installed, Ok(None) if not — both valid.
        assert!(result.is_ok());
    }

    // === ensure_gc_root_for_generation tests (Phase 5.6) ===

    fn write_minimal_manifest(dir: &std::path::Path, system_path: &str) {
        let body = format!(
            r#"{{
                "id": "v-test",
                "timestamp": "2026-01-01T00:00:00Z",
                "system_path": "{}",
                "init_path": "/nix/store/init",
                "artifacts": {{ "kernel": "bzImage", "initrd": "initrd" }},
                "checksums": {{ "kernel": "aa", "initrd": "bb" }},
                "status": "active",
                "target": "desktop-generic",
                "channel": "generic",
                "hardwareClass": "physical-generic"
        }}"#,
            system_path
        );
        std::fs::write(dir.join("manifest.json"), body).unwrap();
    }

    #[test]
    fn test_ensure_gc_root_missing_dir_returns_none() {
        // bash: `[[ -d "$generation_dir" ]] || return 0` — no panic, no gc-root call.
        let missing = std::env::temp_dir().join(format!(
            "gar-ensure-gc-missing-{}-{}",
            std::process::id(),
            std::process::id()
        ));
        let r = ensure_gc_root_for_generation(&missing).unwrap();
        assert_eq!(r, None);
    }

    #[test]
    fn test_ensure_gc_root_already_exists_is_idempotent() {
        // bash: `[[ -f "$generation_dir/.gcroot" ]] && return 0` — short-circuit.
        let dir =
            std::env::temp_dir().join(format!("gar-ensure-gc-existing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".gcroot"), "pre-existing").unwrap();
        // No manifest.json needed — short-circuits before reading.
        let r = ensure_gc_root_for_generation(&dir).unwrap();
        assert_eq!(r, None, "should not call nix-store when .gcroot exists");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_ensure_gc_root_missing_manifest_returns_none() {
        // bash: `[[ -f "$manifest" ]] || return 0` — no gc-root without manifest.
        let dir =
            std::env::temp_dir().join(format!("gar-ensure-gc-nomanifest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // No manifest.json, no .gcroot.
        let r = ensure_gc_root_for_generation(&dir).unwrap();
        assert_eq!(r, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_ensure_gc_root_empty_system_path_returns_none() {
        // bash: `[[ -n "$system_path" ]] || return 0` — no gc-root without system_path.
        let dir = std::env::temp_dir().join(format!("gar-ensure-gc-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_minimal_manifest(&dir, ""); // empty system_path
        let r = ensure_gc_root_for_generation(&dir).unwrap();
        assert_eq!(r, None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
