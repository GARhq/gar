//! Client diagnostics — image manifest, NFS exports, inventory.
//!
//! Read-only inspection of what an incoming client would see on the network:
//! the current published image manifest, active NFS exports, and the
//! GAROS client inventory.
//!
//! Inspired by `cmd_client_session_doctor` in `server/ragos-cli.nix`
//! (11 lines of bash). All operations are best-effort: missing files
//! produce empty sections rather than errors, so the doctor can run
//! in CI sandboxes without runtime dependencies.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Active image manifest under `<images_root>/current/manifest.json`.
/// Only the `id` field is required; everything else is passed through
/// for future use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientManifest {
    pub id: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub status: String,
}

/// Resolves and parses the current client image manifest.
///
/// Returns `Ok(None)` if the manifest file does not exist (CI / fresh host).
/// Returns `Err` only on parse errors — corrupted manifest is a real failure.
pub fn current_manifest(images_root: &Path) -> Result<Option<ClientManifest>, String> {
    let path = images_root.join("current").join("manifest.json");
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("read {}: {}", path.display(), e))?;
    let m: ClientManifest = serde_json::from_slice(&bytes)
        .map_err(|e| format!("parse {}: {}", path.display(), e))?;
    Ok(Some(m))
}

/// Runs `exportfs -v` and returns its stdout. Empty string if the binary is missing.
pub fn nfs_exports() -> String {
    let Ok(out) = std::process::Command::new("exportfs").arg("-v").output() else {
        return String::new();
    };
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Reads the GAROS inventory file at the canonical path.
pub fn inventory_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Canonical inventory path (`/etc/ragos-inventory/clients.nix` by default).
pub fn default_inventory_path() -> PathBuf {
    PathBuf::from("/etc/ragos-inventory/clients.nix")
}

/// Aggregate report for `gar client session-doctor`.
#[derive(Debug, Serialize)]
pub struct ClientSessionReport {
    pub manifest: Option<ClientManifest>,
    pub manifest_available: bool,
    pub nfs_exports: String,
    pub inventory: String,
    pub inventory_path: String,
    pub ok_count: usize,
    pub fail_count: usize,
}

/// Run all client checks and assemble a report.
///
/// `images_root` and `inventory_path` are passed in so the function is
/// pure and testable (no hardcoded paths leaking into CI).
pub fn collect_report(images_root: &Path, inventory_path: &Path) -> ClientSessionReport {
    let manifest = match current_manifest(images_root) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("gar client session-doctor: manifest parse failed: {}", e);
            None
        }
    };
    let manifest_available = manifest.is_some();
    let nfs_exports = nfs_exports();
    let inventory = inventory_text(inventory_path);
    let inventory_path_str = inventory_path.display().to_string();

    let mut ok = 0usize;
    let mut fail = 0usize;
    if manifest_available {
        ok += 1;
    } else {
        fail += 1;
    }
    if !nfs_exports.is_empty() {
        ok += 1;
    } else {
        fail += 1;
    }
    if !inventory.is_empty() {
        ok += 1;
    } else {
        fail += 1;
    }

    ClientSessionReport {
        manifest,
        manifest_available,
        nfs_exports,
        inventory,
        inventory_path: inventory_path_str,
        ok_count: ok,
        fail_count: fail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_parses_minimal() {
        let tmp = std::env::temp_dir().join(format!("gar-client-mf-{}", std::process::id()));
        let current = tmp.join("current");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(
            current.join("manifest.json"),
            r#"{"id": "v20260809-120000", "target": "desktop-generic"}"#,
        )
        .unwrap();
        let m = current_manifest(&tmp).unwrap().unwrap();
        assert_eq!(m.id, "v20260809-120000");
        assert_eq!(m.target, "desktop-generic");
        assert_eq!(m.channel, ""); // default
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_manifest_returns_none_when_missing() {
        let tmp = std::env::temp_dir().join(format!("gar-client-mf-missing-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let m = current_manifest(&tmp).unwrap();
        assert!(m.is_none());
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_manifest_returns_err_on_corrupted_json() {
        let tmp = std::env::temp_dir().join(format!("gar-client-mf-bad-{}", std::process::id()));
        let current = tmp.join("current");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(current.join("manifest.json"), "not-json-at-all").unwrap();
        let r = current_manifest(&tmp);
        assert!(r.is_err());
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_inventory_text_reads_or_empty() {
        let tmp = std::env::temp_dir().join(format!("gar-client-inv-{}", std::process::id()));
        std::fs::write(&tmp, "{ clients = []; }\n").unwrap();
        assert_eq!(inventory_text(&tmp), "{ clients = []; }\n");
        let missing = std::env::temp_dir().join(format!("gar-client-inv-no-{}.nope", std::process::id()));
        assert_eq!(inventory_text(&missing), "");
    }

    #[test]
    fn test_collect_report_counts() {
        let tmp_img = std::env::temp_dir().join(format!("gar-client-cr-img-{}", std::process::id()));
        let current = tmp_img.join("current");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(current.join("manifest.json"), r#"{"id":"v1"}"#).unwrap();

        let tmp_inv = std::env::temp_dir().join(format!("gar-client-cr-inv-{}", std::process::id()));
        std::fs::write(&tmp_inv, "{ clients = []; }\n").unwrap();

        let r = collect_report(&tmp_img, &tmp_inv);
        assert!(r.manifest_available);
        // nfs_exports is best-effort — may be empty in CI; just count it
        assert_eq!(r.ok_count + r.fail_count, 3);

        std::fs::remove_dir_all(&tmp_img).unwrap();
        std::fs::remove_file(&tmp_inv).unwrap();
    }

    #[test]
    fn test_report_serializes_with_all_fields() {
        let tmp_img = std::env::temp_dir().join(format!("gar-client-cs-img-{}", std::process::id()));
        std::fs::create_dir_all(&tmp_img).unwrap();
        let tmp_inv = std::env::temp_dir().join(format!("gar-client-cs-inv-{}", std::process::id()));
        std::fs::write(&tmp_inv, "").unwrap();
        let r = collect_report(&tmp_img, &tmp_inv);
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("manifest_available"));
        assert!(json.contains("nfs_exports"));
        assert!(json.contains("inventory_path"));
        std::fs::remove_dir_all(&tmp_img).unwrap();
        std::fs::remove_file(&tmp_inv).unwrap();
    }
}
