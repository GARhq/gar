//! User system operations. Filesystem-agnostic: quotas/subvolumes/snapshots
//! delegate to `services::filesystem::FsOps` (BTRFS/XFS/ZFS).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{GarError, Result};
pub use crate::services::filesystem::{
    bytes_to_human, detect as detect_fs, human_to_bytes, FsOps, FsType,
};

/// Harness-socket path used by NixOS for interactive shells.
const NIX_INTERACTIVE_SHELL: &str = "/run/current-system/sw/bin/bash";

pub async fn useradd_system(username: &str, home: &str) -> Result<()> {
    crate::services::shell::run_success(
        "useradd",
        &["-M", "-d", home, "-s", NIX_INTERACTIVE_SHELL, "-U", username],
    )
    .await?;
    Ok(())
}

pub async fn useradd_to_group(username: &str, group: &str) -> Result<()> {
    crate::services::shell::run_success("usermod", &["-a", "-G", group, username]).await?;
    Ok(())
}

pub async fn userdel_from_group(username: &str, group: &str) -> Result<()> {
    let _ = crate::services::shell::run_success("gpasswd", &["-d", username, group]).await;
    Ok(())
}

pub async fn userdel(username: &str) -> Result<()> {
    // Exit code 6 from userdel = "user does not exist" — treat as success.
    match crate::services::shell::run_success("userdel", &[username]).await {
        Err(GarError::CommandFailed { code: 6, .. }) => Ok(()),
        other => other.map(|_| ()),
    }
}

pub fn hash_password(plain: &str) -> Result<String> {
    let output = std::process::Command::new("openssl")
        .args(["passwd", "-6", plain])
        .output()?;
    if !output.status.success() {
        return Err(GarError::User(format!(
            "openssl passwd failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn user_exists(username: &str) -> bool {
    cmd_ok("id", &["-u", username])
}

pub fn user_uid(username: &str) -> Option<u32> {
    cmd_stdout("id", &["-u", username]).and_then(|s| s.trim().parse().ok())
}

pub fn user_groups(username: &str) -> HashSet<String> {
    cmd_stdout("id", &["-Gn", username])
        .map(|s| s.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default()
}

pub fn user_shadow_hash(username: &str) -> Option<String> {
    cmd_stdout("getent", &["shadow", username]).and_then(|s| {
        s.split(':').nth(1).filter(|v| !v.is_empty()).map(str::to_string)
    })
}

pub fn fs_type(path: &Path) -> Option<String> {
    let fs = detect_fs(path);
    (fs != FsType::Unknown).then(|| fs.as_str().to_string())
}

pub fn is_mountpoint(path: &Path) -> bool {
    cmd_ok("mountpoint", &["-q", &path.display().to_string()])
}

pub fn is_btrfs(path: &Path) -> bool {
    detect_fs(path) == FsType::Btrfs
}

pub fn supports_quota(path: &Path) -> bool {
    detect_fs(path).supports_quota()
}

pub fn dir_size_bytes(path: &Path) -> u64 {
    cmd_stdout("du", &["-sb", &path.display().to_string()])
        .and_then(|s| s.split_whitespace().next().and_then(|n| n.parse().ok()))
        .unwrap_or(0)
}

pub fn qgroup_info(path: &Path) -> Option<String> {
    FsOps::for_path(path).ok()?.qgroup_info(path)
}

pub fn mount_info(path: &Path) -> Option<String> {
    let s = cmd_stdout("findmnt", &["--target", &path.display().to_string()])?;
    (!s.trim().is_empty()).then(|| s)
}

fn cmd_ok(prog: &str, args: &[&str]) -> bool {
    std::process::Command::new(prog)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn cmd_stdout(prog: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(prog).args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientUserEntry {
    pub uid: u32,
    pub hashed_password: String,
    pub extra_groups: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ClientUsersCatalog {
    #[serde(flatten)]
    pub users: HashMap<String, ClientUserEntry>,
}

impl ClientUsersCatalog {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).unwrap_or_default();
        Ok(serde_json::from_slice(&bytes).unwrap_or_default())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        atomic_write(path, &serde_json::to_vec_pretty(self)?, 0o644)
    }

    pub fn upsert(&mut self, username: &str, entry: ClientUserEntry) {
        self.users.insert(username.into(), entry);
    }

    pub fn remove(&mut self, username: &str) {
        self.users.remove(username);
    }

    pub fn get(&self, username: &str) -> Option<&ClientUserEntry> {
        self.users.get(username)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct UserGroupsCatalog {
    #[serde(default)]
    pub user_groups: HashMap<String, String>,
}

impl UserGroupsCatalog {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).unwrap_or_default();
        Ok(serde_json::from_slice(&bytes).unwrap_or_default())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        atomic_write(path, &serde_json::to_vec_pretty(self)?, 0o644)
    }

    pub fn set_user_group(&mut self, username: &str, group: &str) {
        self.user_groups.insert(username.into(), group.into());
    }
}

/// Per-user metadata file (`<home>/.garos-home-meta`). Legacy key=value format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeMeta {
    pub user: String,
    pub home: String,
    pub quota: String,
    pub updated_at: String,
}

impl HomeMeta {
    pub fn path_for(home: &Path) -> PathBuf {
        home.join(".garos-home-meta")
    }

    pub fn load(home: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(Self::path_for(home))?;
        let mut map: HashMap<String, String> = content
            .lines()
            .filter_map(|l| l.split_once('=').map(|(k, v)| (k.into(), v.into())))
            .collect();
        Ok(Self {
            user: map.remove("USER").unwrap_or_default(),
            home: map.remove("HOME").unwrap_or_default(),
            quota: map.remove("QUOTA").unwrap_or_default(),
            updated_at: map.remove("UPDATED_AT").unwrap_or_default(),
        })
    }

    pub fn write(home: &Path, meta: &HomeMeta) -> Result<()> {
        let content = format!(
            "USER={}\nHOME={}\nQUOTA={}\nUPDATED_AT={}\n",
            meta.user, meta.home, meta.quota, meta.updated_at
        );
        atomic_write(&Self::path_for(home), content.as_bytes(), 0o640)
    }
}

pub fn read_meta_value(home: &Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(HomeMeta::path_for(home)).ok()?;
    content
        .lines()
        .find_map(|l| l.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v.into()))
}

fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(mode))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_human_to_bytes_valid() {
        assert_eq!(human_to_bytes("1G").unwrap(), 1_073_741_824);
    }

    #[test]
    fn test_human_to_bytes_invalid() {
        assert!(human_to_bytes("invalid").is_err());
    }

    #[test]
    fn test_bytes_to_human_format() {
        assert!(bytes_to_human(1_073_741_824).contains('G'));
    }

    #[test]
    fn test_user_exists_works() {
        assert!(user_exists("root"));
        assert!(!user_exists("nonexistent-user-12345"));
    }

    #[test]
    fn test_user_uid_root() {
        assert_eq!(user_uid("root"), Some(0));
    }

    #[test]
    fn test_user_groups_root() {
        let groups = user_groups("root");
        assert!(groups.contains("root"));
    }

    #[test]
    fn test_user_shadow_hash_returns_value_or_none() {
        // Either readable (euid 0) or restricted (no perms) — both are valid.
        let _ = user_shadow_hash("root");
    }

    #[test]
    fn test_client_users_catalog_roundtrip() {
        let mut cat = ClientUsersCatalog::default();
        cat.upsert(
            "alice",
            ClientUserEntry {
                uid: 1000,
                hashed_password: "$6$xxx".into(),
                extra_groups: vec!["users".into()],
            },
        );
        let json = serde_json::to_string(&cat).unwrap();
        let back: ClientUsersCatalog = serde_json::from_str(&json).unwrap();
        assert_eq!(back.get("alice").unwrap().uid, 1000);
    }

    #[test]
    fn test_user_groups_catalog_roundtrip() {
        let mut cat = UserGroupsCatalog::default();
        cat.set_user_group("alice", "users");
        let json = serde_json::to_string(&cat).unwrap();
        let back: UserGroupsCatalog = serde_json::from_str(&json).unwrap();
        assert_eq!(back.user_groups.get("alice"), Some(&"users".to_string()));
    }

    #[test]
    fn test_home_meta_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("gar-meta-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let meta = HomeMeta {
            user: "alice".into(),
            home: tmp.display().to_string(),
            quota: "20G".into(),
            updated_at: "2026-08-08T12:00:00+00:00".into(),
        };
        HomeMeta::write(&tmp, &meta).unwrap();
        let back = HomeMeta::load(&tmp).unwrap();
        assert_eq!(back.user, "alice");
        assert_eq!(back.quota, "20G");
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_read_meta_value_legacy() {
        let tmp = std::env::temp_dir().join(format!("gar-legacy-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join(".garos-home-meta"), "USER=alice\nQUOTA=10G\n").unwrap();
        assert_eq!(read_meta_value(&tmp, "QUOTA"), Some("10G".into()));
        assert_eq!(read_meta_value(&tmp, "MISSING"), None);
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
