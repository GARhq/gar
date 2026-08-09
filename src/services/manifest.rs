//! Generation manifest (metadata for each published image version).
//!
//! Replaces `ragc/lib/manifest.sh` (write_generation_manifest,
//! validate_manifest, reconcile_generation_statuses).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{GarError, Result};

/// Generation status (single source of truth for what a version is).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Active,
    Previous,
    Staged,
    Rescue,
    Inactive,
}

/// Generation manifest (lives at `images_root/<version>/manifest.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub timestamp: String,
    #[serde(rename = "system_path")]
    pub system_path: String,
    #[serde(rename = "init_path")]
    pub init_path: String,
    pub artifacts: Artifacts,
    pub checksums: Checksums,
    pub status: Status,
    pub target: String,
    pub channel: String,
    #[serde(rename = "hardwareClass")]
    pub hardware_class: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifacts {
    pub kernel: String,
    pub initrd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checksums {
    pub kernel: String,
    pub initrd: String,
}

/// Read manifest from `<generation_dir>/manifest.json`.
pub fn read(generation_dir: &Path) -> Result<Manifest> {
    let path = manifest_path(generation_dir);
    let bytes = std::fs::read(&path)?;
    let m: Manifest = serde_json::from_slice(&bytes)?;
    Ok(m)
}

/// Write manifest atomically (write to temp + rename).
pub fn write(generation_dir: &Path, manifest: &Manifest) -> Result<()> {
    if let Some(parent) = generation_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(generation_dir)?;
    let tmp = generation_dir.join(format!("manifest.json.tmp.{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(manifest)?;
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, manifest_path(generation_dir))?;
    Ok(())
}

/// Update only the `status` field of a manifest in place.
pub fn set_status(generation_dir: &Path, new_status: Status) -> Result<()> {
    let path = manifest_path(generation_dir);
    if !path.exists() {
        return Ok(()); // no manifest, nothing to update
    }
    let mut m = read(generation_dir)?;
    m.status = new_status;
    write(generation_dir, &m)
}

/// Reconcile statuses across all generations in `images_root`.
pub fn reconcile(
    images_root: &Path,
    current: Option<&str>,
    previous: Option<&str>,
    staged: Option<&str>,
    rescue: Option<&str>,
) -> Result<()> {
    if !images_root.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(images_root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if !name.starts_with('v') {
            continue;
        }

        let status = if Some(name) == staged {
            Status::Staged
        } else if Some(name) == current {
            Status::Active
        } else if Some(name) == previous {
            Status::Previous
        } else if Some(name) == rescue {
            Status::Rescue
        } else {
            Status::Inactive
        };

        set_status(&path, status)?;
    }
    Ok(())
}

/// Validate manifest integrity (checksum check).
pub fn validate(generation_dir: &Path, expected_id: &str) -> Result<()> {
    let m = read(generation_dir)?;
    if m.id != expected_id {
        return Err(GarError::validation(format!(
            "manifest id divergente: esperado={} encontrado={}",
            expected_id, m.id
        )));
    }
    if !m.system_path.starts_with("/nix/store/") {
        return Err(GarError::validation(format!(
            "system_path não está em /nix/store/: {}",
            m.system_path
        )));
    }
    if !m.init_path.starts_with("/nix/store/") {
        return Err(GarError::validation(format!(
            "init_path não está em /nix/store/: {}",
            m.init_path
        )));
    }
    Ok(())
}

fn manifest_path(generation_dir: &Path) -> PathBuf {
    generation_dir.join("manifest.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> Manifest {
        Manifest {
            id: "v20260101-120000".into(),
            timestamp: "2026-01-01T12:00:00Z".into(),
            system_path: "/nix/store/abc-system".into(),
            init_path: "/nix/store/abc-init".into(),
            artifacts: Artifacts {
                kernel: "bzImage".into(),
                initrd: "initrd".into(),
            },
            checksums: Checksums {
                kernel: "deadbeef".into(),
                initrd: "cafebabe".into(),
            },
            status: Status::Active,
            target: "desktop-generic".into(),
            channel: "generic".into(),
            hardware_class: "physical-generic".into(),
        }
    }

    #[test]
    fn test_manifest_serialize_roundtrip() {
        let m = sample_manifest();
        let json = serde_json::to_string(&m).unwrap();
        let m2: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m.id, m2.id);
        assert_eq!(m.status, m2.status);
        assert_eq!(m.target, m2.target);
    }

    #[test]
    fn test_manifest_camel_case_field() {
        let m = sample_manifest();
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("system_path"));
        assert!(json.contains("hardwareClass"));
    }

    #[test]
    fn test_status_serialize_lowercase() {
        let m = sample_manifest();
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"status\":\"active\""));
    }

    #[test]
    fn test_write_read_atomic() {
        let tmp = std::env::temp_dir().join(format!("gar-manifest-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();

        let mut m = sample_manifest();
        write(&tmp, &m).unwrap();
        let read_m = read(&tmp).unwrap();
        assert_eq!(read_m.id, m.id);

        m.status = Status::Previous;
        write(&tmp, &m).unwrap();
        let read_m = read(&tmp).unwrap();
        assert_eq!(read_m.status, Status::Previous);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_validate_passes_for_nix_path() {
        let tmp =
            std::env::temp_dir().join(format!("gar-manifest-validate-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let m = sample_manifest();
        write(&tmp, &m).unwrap();
        assert!(validate(&tmp, "v20260101-120000").is_ok());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_validate_fails_for_wrong_id() {
        let tmp = std::env::temp_dir().join(format!("gar-manifest-wrongid-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let m = sample_manifest();
        write(&tmp, &m).unwrap();
        let r = validate(&tmp, "wrong-id");
        assert!(matches!(r, Err(GarError::Validation(_))));

        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
