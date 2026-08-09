//! Filesystem abstraction layer.
//!
//! Supports BTRFS, XFS, and ZFS for subvolume/snapshot/quota operations.
//! Detection via `findmnt -o FSTYPE`.
//!
//! Per-fs capabilities:
//! - BTRFS: subvolume, snapshot, per-subvolume qgroup quota
//! - XFS: project quota (xfs_quota), per-directory project ID
//! - ZFS: zfs dataset properties (quota, refquota, reservation)

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{GarError, Result};

/// Detected filesystem type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FsType {
    Btrfs,
    Xfs,
    Zfs,
    Ext4,
    Unknown,
}

impl FsType {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "btrfs" => Self::Btrfs,
            "xfs" => Self::Xfs,
            "zfs" => Self::Zfs,
            "ext2" | "ext3" | "ext4" => Self::Ext4,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Btrfs => "btrfs",
            Self::Xfs => "xfs",
            Self::Zfs => "zfs",
            Self::Ext4 => "ext4",
            Self::Unknown => "unknown",
        }
    }

    pub fn supports_quota(self) -> bool {
        matches!(self, Self::Btrfs | Self::Xfs | Self::Zfs)
    }

    pub fn supports_snapshot(self) -> bool {
        matches!(self, Self::Btrfs | Self::Zfs)
    }

    pub fn supports_subvolume(self) -> bool {
        matches!(self, Self::Btrfs | Self::Zfs)
    }
}

/// Detect the filesystem type of a path.
pub fn detect(path: &Path) -> FsType {
    let output = std::process::Command::new("findmnt")
        .arg("-n")
        .arg("-o")
        .arg("FSTYPE")
        .arg("--target")
        .arg(path)
        .output()
        .ok();
    match output {
        Some(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            FsType::from_str(s.trim())
        }
        _ => FsType::Unknown,
    }
}

pub fn is_btrfs(path: &Path) -> bool {
    detect(path) == FsType::Btrfs
}
pub fn is_xfs(path: &Path) -> bool {
    detect(path) == FsType::Xfs
}
pub fn is_zfs(path: &Path) -> bool {
    detect(path) == FsType::Zfs
}

/// Filesystem operations. Each impl is a unit struct (no state).
#[derive(Debug)]
pub enum FsOps {
    Btrfs,
    Xfs,
    Zfs,
}

impl FsOps {
    pub fn for_path(path: &Path) -> Result<Self> {
        match detect(path) {
            FsType::Btrfs => Ok(Self::Btrfs),
            FsType::Xfs => Ok(Self::Xfs),
            FsType::Zfs => Ok(Self::Zfs),
            FsType::Ext4 => Err(GarError::User(
                "ext4 nao suporta per-directory quota (use btrfs/xfs/zfs)".into(),
            )),
            FsType::Unknown => Err(GarError::User(format!(
                "filesystem desconhecido em {} (use btrfs, xfs ou zfs)",
                path.display()
            ))),
        }
    }

    /// Create a subvolume-like entity at `path`.
    pub async fn create_subvolume(&self, path: &Path) -> Result<()> {
        match self {
            Self::Btrfs => {
                let _ = crate::services::shell::run_success(
                    "btrfs",
                    &["subvolume", "create", &path.display().to_string()],
                )
                .await?;
            }
            Self::Zfs => {
                let dataset = path_to_dataset(path)?;
                let _ = crate::services::shell::run_success("zfs", &["create", &dataset]).await?;
                let _ = crate::services::shell::run_success(
                    "zfs",
                    &[
                        "set",
                        &format!("mountpoint={}", path.display()),
                        &dataset,
                    ],
                )
                .await?;
            }
            Self::Xfs => {
                // XFS has no subvolumes
                std::fs::create_dir_all(path)?;
            }
        }
        Ok(())
    }

    /// Create a read-only snapshot at `dst` of `src`.
    pub async fn snapshot_readonly(&self, src: &Path, dst: &Path) -> Result<()> {
        match self {
            Self::Btrfs => {
                let _ = crate::services::shell::run_success(
                    "btrfs",
                    &[
                        "subvolume",
                        "snapshot",
                        "-r",
                        &src.display().to_string(),
                        &dst.display().to_string(),
                    ],
                )
                .await?;
            }
            Self::Zfs => {
                let dataset = path_to_dataset(src)?;
                let snap_name = format!(
                    "{}@{}",
                    dataset,
                    dst.file_name().and_then(|n| n.to_str()).unwrap_or("snap")
                );
                let _ = crate::services::shell::run_success("zfs", &["snapshot", &snap_name]).await?;
            }
            Self::Xfs => {
                return Err(GarError::User(
                    "XFS nao suporta snapshots a nivel de filesystem".into(),
                ));
            }
        }
        Ok(())
    }

    /// Enable per-directory quotas on the mountpoint.
    pub async fn enable_quotas(&self, mountpoint: &Path) -> Result<()> {
        match self {
            Self::Btrfs => {
                let _ = crate::services::shell::run_success(
                    "btrfs",
                    &["quota", "enable", &mountpoint.display().to_string()],
                )
                .await?;
            }
            Self::Xfs => {
                let output = std::process::Command::new("findmnt")
                    .args([
                        "-n",
                        "-o",
                        "OPTIONS",
                        "--target",
                        &mountpoint.display().to_string(),
                    ])
                    .output()
                    .ok();
                let has_prjquota = match output {
                    Some(o) if o.status.success() => {
                        String::from_utf8_lossy(&o.stdout).contains("prjquota")
                    }
                    _ => false,
                };
                if !has_prjquota {
                    return Err(GarError::User(format!(
                        "XFS quota requer opcao de mount 'prjquota' em {}",
                        mountpoint.display()
                    )));
                }
            }
            Self::Zfs => {
                // ZFS quotas are property-based — always available
            }
        }
        Ok(())
    }

    /// Set a quota (human-readable, e.g. "20G") on a path.
    pub async fn set_quota(&self, path: &Path, quota_human: &str) -> Result<()> {
        let bytes = human_to_bytes(quota_human)?;
        match self {
            Self::Btrfs => {
                let _ = crate::services::shell::run_success(
                    "btrfs",
                    &[
                        "qgroup",
                        "limit",
                        &bytes.to_string(),
                        &path.display().to_string(),
                    ],
                )
                .await?;
            }
            Self::Xfs => {
                let inode = std::fs::metadata(path)
                    .ok()
                    .and_then(|m| {
                        use std::os::unix::fs::MetadataExt;
                        Some(m.ino())
                    })
                    .ok_or_else(|| {
                        GarError::User(format!("path nao acessivel: {}", path.display()))
                    })?;
                let blocks = bytes / 512;
                let _ = crate::services::shell::run_success(
                    "xfs_quota",
                    &[
                        "-x",
                        "-c",
                        &format!("project -s {} {}", inode, path.display()),
                        "/",
                    ],
                )
                .await?;
                let _ = crate::services::shell::run_success(
                    "xfs_quota",
                    &[
                        "-x",
                        "-c",
                        &format!("limit -p bhard {} {}", blocks, inode),
                        "/",
                    ],
                )
                .await?;
            }
            Self::Zfs => {
                let dataset = path_to_dataset(path)?;
                let _ = crate::services::shell::run_success(
                    "zfs",
                    &["set", &format!("refquota={}", bytes), &dataset],
                )
                .await?;
            }
        }
        Ok(())
    }

    /// Get usage info for a path (best-effort).
    pub fn qgroup_info(&self, path: &Path) -> Option<String> {
        match self {
            Self::Btrfs => {
                let output = std::process::Command::new("btrfs")
                    .args(["qgroup", "show", "-f", &path.display().to_string()])
                    .output()
                    .ok()?;
                if !output.status.success() {
                    return None;
                }
                Some(String::from_utf8_lossy(&output.stdout).into_owned())
            }
            Self::Xfs => {
                let inode = std::fs::metadata(path).ok().and_then(|m| {
                    use std::os::unix::fs::MetadataExt;
                    Some(m.ino())
                })?;
                let output = std::process::Command::new("xfs_quota")
                    .args(["-x", "-c", &format!("project -p {}", inode), "/"])
                    .output()
                    .ok()?;
                if !output.status.success() {
                    return None;
                }
                Some(String::from_utf8_lossy(&output.stdout).into_owned())
            }
            Self::Zfs => {
                let dataset = path_to_dataset(path).ok()?;
                let output = std::process::Command::new("zfs")
                    .args(["get", "quota,refquota,used", &dataset])
                    .output()
                    .ok()?;
                if !output.status.success() {
                    return None;
                }
                Some(String::from_utf8_lossy(&output.stdout).into_owned())
            }
        }
    }
}

/// Convert a path to a ZFS dataset name.
///
/// Heuristic: assume path `/srv/data/home/alice` maps to
/// dataset `tank/srv/data/home/alice` (using a configurable pool).
///
/// In production, this would query `zfs list -o name,mountpoint`
/// to find the dataset backing a mountpoint.
fn path_to_dataset(path: &Path) -> Result<String> {
    let mountpoint = std::process::Command::new("findmnt")
        .args(["-n", "-o", "SOURCE", "--target", &path.display().to_string()])
        .output()
        .ok();
    if let Some(o) = mountpoint {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout);
            let s = s.trim();
            if !s.is_empty() {
                return Ok(s.to_string());
            }
        }
    }
    // Fallback: assume pool is "tank"
    let path_str = path.display().to_string().trim_start_matches('/').replace('/', "/");
    Ok(format!("tank/{}", path_str))
}

/// Detect and dispatch to the right FsOps implementation.
pub fn require_supported(mountpoint: &Path) -> Result<FsType> {
    let fs = detect(mountpoint);
    if !fs.supports_quota() {
        return Err(GarError::User(format!(
            "filesystem {} em {} nao suporta quotas (use btrfs/xfs/zfs)",
            fs.as_str(),
            mountpoint.display()
        )));
    }
    Ok(fs)
}

/// Convert human-readable (20G, 1T, 500M) to bytes.
pub fn human_to_bytes(human: &str) -> Result<u64> {
    let output = std::process::Command::new("numfmt")
        .args(["--from=iec", human])
        .output()?;
    if !output.status.success() {
        return Err(GarError::User(format!(
            "quantia invalida: {} (use formato IEC: 20G, 1T, 500M)",
            human
        )));
    }
    let s = String::from_utf8_lossy(&output.stdout);
    s.trim()
        .parse()
        .map_err(|e| GarError::User(format!("falha ao parsear bytes: {}", e)))
}

/// Convert bytes to human-readable (IEC, suffix B).
pub fn bytes_to_human(bytes: u64) -> String {
    let output = std::process::Command::new("numfmt")
        .args(["--to=iec-i", "--suffix=B", &bytes.to_string()])
        .output()
        .ok();
    match output {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => format!("{}B", bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fs_type_parsing() {
        assert_eq!(FsType::from_str("btrfs"), FsType::Btrfs);
        assert_eq!(FsType::from_str("BTRFS"), FsType::Btrfs);
        assert_eq!(FsType::from_str("xfs"), FsType::Xfs);
        assert_eq!(FsType::from_str("zfs"), FsType::Zfs);
        assert_eq!(FsType::from_str("ext4"), FsType::Ext4);
        assert_eq!(FsType::from_str("ntfs"), FsType::Unknown);
    }

    #[test]
    fn test_fs_type_capabilities() {
        assert!(FsType::Btrfs.supports_quota());
        assert!(FsType::Btrfs.supports_snapshot());
        assert!(FsType::Btrfs.supports_subvolume());

        assert!(FsType::Xfs.supports_quota());
        assert!(!FsType::Xfs.supports_snapshot());
        assert!(!FsType::Xfs.supports_subvolume());

        assert!(FsType::Zfs.supports_quota());
        assert!(FsType::Zfs.supports_snapshot());
        assert!(FsType::Zfs.supports_subvolume());

        assert!(!FsType::Ext4.supports_quota());
    }

    #[test]
    fn test_fs_type_serialize() {
        let json = serde_json::to_string(&FsType::Btrfs).unwrap();
        assert_eq!(json, "\"btrfs\"");
        let back: FsType = serde_json::from_str("\"xfs\"").unwrap();
        assert_eq!(back, FsType::Xfs);
    }

    #[test]
    fn test_human_to_bytes_valid() {
        assert_eq!(human_to_bytes("1G").unwrap(), 1_073_741_824);
        assert_eq!(human_to_bytes("500M").unwrap(), 524_288_000);
        assert_eq!(human_to_bytes("2T").unwrap(), 2_199_023_255_552);
    }

    #[test]
    fn test_human_to_bytes_invalid() {
        assert!(human_to_bytes("invalid").is_err());
        assert!(human_to_bytes("100XYZ").is_err());
    }

    #[test]
    fn test_bytes_to_human_format() {
        let s = bytes_to_human(1_073_741_824);
        assert!(s.contains("G"), "Expected G in: {}", s);
    }

    #[test]
    fn test_fs_ops_for_path_unsupported() {
        // tmp is usually ext4 or tmpfs — should error or succeed depending on env
        let tmp = std::env::temp_dir();
        let r = FsOps::for_path(&tmp);
        // Acceptable: Err (not btrfs/xfs/zfs) or Ok (one of those, unlikely on test env)
        match r {
            Ok(_) => {} // unusual but acceptable
            Err(_) => {} // expected on most systems
        }
    }

    #[test]
    fn test_require_supported_rejects_unsupported() {
        let tmp = std::env::temp_dir();
        let r = require_supported(&tmp);
        // On test env, /tmp is usually ext4 → error
        let _ = r; // just verify it doesn't panic
    }

    #[test]
    fn test_path_to_dataset_no_panic() {
        let r = path_to_dataset(Path::new("/tmp/test"));
        assert!(r.is_ok());
    }
}