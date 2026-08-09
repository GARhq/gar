//! Filesystem abstraction. Detects BTRFS/XFS/ZFS and dispatches subvolume,
//! snapshot, and quota operations to the right tool.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{GarError, Result};

const XFS_BLOCK_SIZE: u64 = 512;
const ZFS_DEFAULT_POOL: &str = "tank";

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

pub fn detect(path: &Path) -> FsType {
    let Some(s) = cmd_stdout(
        "findmnt",
        &[
            "-n",
            "-o",
            "FSTYPE",
            "--target",
            &path.display().to_string(),
        ],
    ) else {
        return FsType::Unknown;
    };
    FsType::from_str(s.trim())
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
            other => Err(GarError::User(format!(
                "{} em {} nao suporta per-directory quota (use btrfs/xfs/zfs)",
                other.as_str(),
                path.display()
            ))),
        }
    }

    pub async fn create_subvolume(&self, path: &Path) -> Result<()> {
        match self {
            Self::Btrfs => {
                sh(
                    "btrfs",
                    &["subvolume", "create", &path.display().to_string()],
                )
                .await
            }
            Self::Zfs => {
                let dataset = path_to_dataset(path)?;
                sh("zfs", &["create", &dataset]).await?;
                sh(
                    "zfs",
                    &["set", &format!("mountpoint={}", path.display()), &dataset],
                )
                .await
            }
            Self::Xfs => {
                std::fs::create_dir_all(path)?;
                Ok(())
            }
        }
    }

    pub async fn snapshot_readonly(&self, src: &Path, dst: &Path) -> Result<()> {
        match self {
            Self::Btrfs => {
                sh(
                    "btrfs",
                    &[
                        "subvolume",
                        "snapshot",
                        "-r",
                        &src.display().to_string(),
                        &dst.display().to_string(),
                    ],
                )
                .await
            }
            Self::Zfs => {
                let dataset = path_to_dataset(src)?;
                let snap = format!(
                    "{}@{}",
                    dataset,
                    dst.file_name().and_then(|n| n.to_str()).unwrap_or("snap")
                );
                sh("zfs", &["snapshot", &snap]).await
            }
            Self::Xfs => Err(GarError::User(
                "XFS nao suporta snapshots a nivel de filesystem".into(),
            )),
        }
    }

    pub async fn enable_quotas(&self, mountpoint: &Path) -> Result<()> {
        match self {
            Self::Btrfs => {
                sh(
                    "btrfs",
                    &["quota", "enable", &mountpoint.display().to_string()],
                )
                .await
            }
            Self::Zfs => Ok(()),
            Self::Xfs => {
                let opts = cmd_stdout(
                    "findmnt",
                    &[
                        "-n",
                        "-o",
                        "OPTIONS",
                        "--target",
                        &mountpoint.display().to_string(),
                    ],
                )
                .unwrap_or_default();
                if !opts.contains("prjquota") {
                    return Err(GarError::User(format!(
                        "XFS quota requer opcao de mount 'prjquota' em {}",
                        mountpoint.display()
                    )));
                }
                Ok(())
            }
        }
    }

    pub async fn set_quota(&self, path: &Path, quota_human: &str) -> Result<()> {
        let bytes = human_to_bytes(quota_human)?;
        match self {
            Self::Btrfs => {
                sh(
                    "btrfs",
                    &[
                        "qgroup",
                        "limit",
                        &bytes.to_string(),
                        &path.display().to_string(),
                    ],
                )
                .await
            }
            Self::Xfs => {
                let inode = inode_of(path)?;
                sh(
                    "xfs_quota",
                    &[
                        "-x",
                        "-c",
                        &format!("project -s {} {}", inode, path.display()),
                        "/",
                    ],
                )
                .await?;
                sh(
                    "xfs_quota",
                    &[
                        "-x",
                        "-c",
                        &format!("limit -p bhard {} {}", bytes / XFS_BLOCK_SIZE, inode),
                        "/",
                    ],
                )
                .await
            }
            Self::Zfs => {
                let dataset = path_to_dataset(path)?;
                sh("zfs", &["set", &format!("refquota={}", bytes), &dataset]).await
            }
        }
    }

    pub fn qgroup_info(&self, path: &Path) -> Option<String> {
        match self {
            Self::Btrfs => btrfs_qgroup(path),
            Self::Xfs => {
                let inode = inode_of(path).ok()?;
                cmd_stdout(
                    "xfs_quota",
                    &["-x", "-c", &format!("project -p {}", inode), "/"],
                )
            }
            Self::Zfs => {
                let dataset = path_to_dataset(path).ok()?;
                cmd_stdout("zfs", &["get", "quota,refquota,used", &dataset])
            }
        }
    }
}

fn btrfs_qgroup(path: &Path) -> Option<String> {
    cmd_stdout(
        "btrfs",
        &["qgroup", "show", "-f", &path.display().to_string()],
    )
}

fn inode_of(path: &Path) -> Result<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path)
        .map(|m| m.ino())
        .map_err(|_| GarError::User(format!("path inacessivel: {}", path.display())))
}

fn path_to_dataset(path: &Path) -> Result<String> {
    let target = path.display().to_string();
    if let Some(s) = cmd_stdout("findmnt", &["-n", "-o", "SOURCE", "--target", &target]) {
        let trimmed = s.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }
    let path_str = target.trim_start_matches('/');
    Ok(format!("{}/{}", ZFS_DEFAULT_POOL, path_str))
}

pub fn require_supported(mountpoint: &Path) -> Result<FsType> {
    let fs = detect(mountpoint);
    if !fs.supports_quota() {
        return Err(GarError::User(format!(
            "{} em {} nao suporta quotas (use btrfs/xfs/zfs)",
            fs.as_str(),
            mountpoint.display()
        )));
    }
    Ok(fs)
}

pub fn human_to_bytes(human: &str) -> Result<u64> {
    let s = cmd_stdout("numfmt", &["--from=iec", human]).ok_or_else(|| {
        GarError::User(format!(
            "quantia invalida: {} (use IEC: 20G, 1T, 500M)",
            human
        ))
    })?;
    s.trim()
        .parse()
        .map_err(|e| GarError::User(format!("falha ao parsear bytes: {}", e)))
}

pub fn bytes_to_human(bytes: u64) -> String {
    cmd_stdout("numfmt", &["--to=iec-i", "--suffix=B", &bytes.to_string()])
        .unwrap_or_else(|| format!("{}B", bytes))
}

async fn sh(prog: &str, args: &[&str]) -> Result<()> {
    crate::services::shell::run_success(prog, args).await?;
    Ok(())
}

fn cmd_stdout(prog: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(prog).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
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
        assert!(
            FsType::Btrfs.supports_quota()
                && FsType::Btrfs.supports_snapshot()
                && FsType::Btrfs.supports_subvolume()
        );
        assert!(FsType::Xfs.supports_quota());
        assert!(!FsType::Xfs.supports_snapshot());
        assert!(!FsType::Xfs.supports_subvolume());
        assert!(
            FsType::Zfs.supports_quota()
                && FsType::Zfs.supports_snapshot()
                && FsType::Zfs.supports_subvolume()
        );
        assert!(!FsType::Ext4.supports_quota());
    }

    #[test]
    fn test_fs_type_serialize() {
        assert_eq!(serde_json::to_string(&FsType::Btrfs).unwrap(), "\"btrfs\"");
        assert_eq!(
            serde_json::from_str::<FsType>("\"xfs\"").unwrap(),
            FsType::Xfs
        );
    }

    #[test]
    fn test_human_to_bytes_roundtrip() {
        assert_eq!(human_to_bytes("1G").unwrap(), 1_073_741_824);
        assert_eq!(human_to_bytes("500M").unwrap(), 524_288_000);
        assert_eq!(human_to_bytes("2T").unwrap(), 2_199_023_255_552);
        assert!(human_to_bytes("invalid").is_err());
        assert!(human_to_bytes("100XYZ").is_err());
    }

    #[test]
    fn test_bytes_to_human_format() {
        assert!(bytes_to_human(1_073_741_824).contains('G'));
    }

    #[test]
    fn test_fs_ops_for_path_does_not_panic() {
        let _ = FsOps::for_path(&std::env::temp_dir());
    }

    #[test]
    fn test_require_supported_does_not_panic() {
        let _ = require_supported(&std::env::temp_dir());
    }

    #[test]
    fn test_path_to_dataset_uses_fallback() {
        let r = path_to_dataset(Path::new("/tmp/test")).unwrap();
        assert!(r.starts_with(ZFS_DEFAULT_POOL));
    }
}
