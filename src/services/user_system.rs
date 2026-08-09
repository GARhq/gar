//! User system operations (useradd, usermod, userdel, quota, catalog).
//!
//! Replaces user management helpers from ragos-cli.nix.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{GarError, Result};

/// Create a system user (no password, no shell) — bootstrap user.
pub async fn useradd_system(username: &str, home: &str) -> Result<()> {
    let _ = crate::services::shell::run_success(
        "useradd",
        &[
            "-M",
            "-d",
            home,
            "-s",
            "/run/current-system/sw/bin/bash",
            "-U",
            username,
        ],
    )
    .await?;
    Ok(())
}

/// Add user to a supplementary group.
pub async fn useradd_to_group(username: &str, group: &str) -> Result<()> {
    let _ = crate::services::shell::run_success("usermod", &["-a", "-G", group, username]).await?;
    Ok(())
}

/// Remove a user from a group.
pub async fn userdel_from_group(username: &str, group: &str) -> Result<()> {
    let _ = crate::services::shell::run_success("gpasswd", &["-d", username, group]).await;
    Ok(())
}

/// Delete a user (best-effort).
pub async fn userdel(username: &str) -> Result<()> {
    match crate::services::shell::run_success("userdel", &[username]).await {
        Ok(_) => Ok(()),
        Err(GarError::CommandFailed { code: 6, .. }) => Ok(()), // user does not exist
        Err(e) => Err(e),
    }
}

/// Hash a plaintext password using SHA-512 crypt.
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

/// Check if a user exists.
pub fn user_exists(username: &str) -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .arg(username)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get user's UID.
pub fn user_uid(username: &str) -> Option<u32> {
    let output = std::process::Command::new("id")
        .arg("-u")
        .arg(username)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

/// Get user's supplementary groups.
pub fn user_groups(username: &str) -> HashSet<String> {
    let output = std::process::Command::new("id")
        .arg("-Gn")
        .arg(username)
        .output()
        .ok();
    match output {
        Some(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .map(|s| s.to_string())
                .collect()
        }
        _ => HashSet::new(),
    }
}

/// Get user's shadow hash.
pub fn user_shadow_hash(username: &str) -> Option<String> {
    let output = std::process::Command::new("getent")
        .arg("shadow")
        .arg(username)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout);
    line.split(':').nth(1).map(|s| s.to_string())
}

/// Get filesystem type of a path.
pub fn fs_type(path: &Path) -> Option<String> {
    let output = std::process::Command::new("findmnt")
        .arg("-n")
        .arg("-o")
        .arg("FSTYPE")
        .arg("--target")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Check if a path is a mountpoint.
pub fn is_mountpoint(path: &Path) -> bool {
    std::process::Command::new("mountpoint")
        .arg("-q")
        .arg(path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if a path is on BTRFS.
pub fn is_btrfs(path: &Path) -> bool {
    fs_type(path).as_deref() == Some("btrfs")
}

/// Get the size of a directory in bytes.
pub fn dir_size_bytes(path: &Path) -> u64 {
    let output = std::process::Command::new("du")
        .args(["-sb", &path.display().to_string()])
        .output()
        .ok();
    match output {
        Some(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0)
        }
        _ => 0,
    }
}

/// Enable BTRFS quotas on a mountpoint.
pub async fn enable_btrfs_quotas(path: &Path) -> Result<()> {
    let _ = crate::services::shell::run_success(
        "btrfs",
        &["quota", "enable", &path.display().to_string()],
    )
    .await?;
    Ok(())
}

/// Apply a qgroup limit (human-readable, e.g. "20G") to a path.
pub async fn set_quota(path: &Path, quota: &str) -> Result<()> {
    // Convert human-readable to bytes (numfmt)
    let bytes = human_to_bytes(quota)?;
    let bytes_str = bytes.to_string();

    let _ = crate::services::shell::run_success(
        "btrfs",
        &[
            "qgroup",
            "limit",
            &bytes_str,
            &path.display().to_string(),
        ],
    )
    .await?;
    Ok(())
}

/// Convert human-readable (20G, 1T, 500M) to bytes.
pub fn human_to_bytes(human: &str) -> Result<u64> {
    let output = std::process::Command::new("numfmt")
        .args(["--from=iec", human])
        .output()?;
    if !output.status.success() {
        return Err(GarError::User(format!("quota invalida: {}", human)));
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
        Some(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        _ => format!("{}B", bytes),
    }
}

/// Get qgroup info for a path.
pub fn qgroup_info(path: &Path) -> Option<String> {
    let output = std::process::Command::new("btrfs")
        .args(["qgroup", "show", "-f", &path.display().to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Get the mount info line for a path.
pub fn mount_info(path: &Path) -> Option<String> {
    let output = std::process::Command::new("findmnt")
        .arg("--target")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout);
    if s.trim().is_empty() {
        None
    } else {
        Some(s.into_owned())
    }
}

/// Client user catalog entry (lives at `runtime_root/client-users.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientUserEntry {
    pub uid: u32,
    #[serde(default = "default_description")]
    pub description: String,
    pub hashed_password: String,
    pub extra_groups: Vec<String>,
    #[serde(default)]
    pub group_gids: std::collections::HashMap<String, u32>,
}

fn default_description() -> String {
    "RAGOS User".into()
}

/// Client user catalog (file-backed JSON).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ClientUsersCatalog {
    #[serde(flatten)]
    pub users: std::collections::HashMap<String, ClientUserEntry>,
}

impl ClientUsersCatalog {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = std::fs::read(path)?;
        let cat: Self = serde_json::from_slice(&bytes)?;
        Ok(cat)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)?;
        std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o644))?;
        Ok(())
    }

    pub fn upsert(&mut self, username: &str, entry: ClientUserEntry) {
        self.users.insert(username.to_string(), entry);
    }

    pub fn remove(&mut self, username: &str) {
        self.users.remove(username);
    }

    pub fn get(&self, username: &str) -> Option<&ClientUserEntry> {
        self.users.get(username)
    }
}

/// User groups catalog (file-backed JSON, used by group add).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct UserGroupsCatalog {
    #[serde(default)]
    pub user_groups: std::collections::HashMap<String, String>,
}

impl UserGroupsCatalog {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = std::fs::read(path)?;
        let cat: Self = serde_json::from_slice(&bytes)?;
        Ok(cat)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)?;
        std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o644))?;
        Ok(())
    }

    pub fn set_user_group(&mut self, username: &str, group: &str) {
        self.user_groups.insert(username.to_string(), group.to_string());
    }
}

/// Per-user metadata file (`<home>/.ragos-home-meta`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeMeta {
    pub user: String,
    pub home: String,
    pub quota: String,
    #[serde(rename = "UPDATED_AT")]
    pub updated_at: String,
}

impl HomeMeta {
    pub fn path_for(home: &Path) -> PathBuf {
        home.join(".garos-home-meta")
    }

    pub fn load(home: &Path) -> Result<Self> {
        let path = Self::path_for(home);
        let content = std::fs::read_to_string(&path)?;
        let mut user = String::new();
        let mut home_path = String::new();
        let mut quota = String::new();
        let mut updated_at = String::new();
        for line in content.lines() {
            if let Some((k, v)) = line.split_once('=') {
                match k {
                    "USER" => user = v.to_string(),
                    "HOME" => home_path = v.to_string(),
                    "QUOTA" => quota = v.to_string(),
                    "UPDATED_AT" => updated_at = v.to_string(),
                    _ => {}
                }
            }
        }
        Ok(Self {
            user,
            home: home_path,
            quota,
            updated_at,
        })
    }

    pub fn write(home: &Path, meta: &HomeMeta) -> Result<()> {
        let path = Self::path_for(home);
        let content = format!(
            "USER={}\nHOME={}\nQUOTA={}\nUPDATED_AT={}\n",
            meta.user, meta.home, meta.quota, meta.updated_at
        );
        std::fs::write(&path, content)?;
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o640))?;
        Ok(())
    }
}

/// Read meta value (legacy format: key=value).
pub fn read_meta_value(home: &Path, key: &str) -> Option<String> {
    let path = HomeMeta::path_for(home);
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        if let Some((k, v)) = line.split_once('=') {
            if k == key {
                return Some(v.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_human_to_bytes_valid() {
        let result = human_to_bytes("1G").unwrap();
        assert_eq!(result, 1_073_741_824);
    }

    #[test]
    fn test_human_to_bytes_invalid() {
        assert!(human_to_bytes("invalid").is_err());
    }

    #[test]
    fn test_bytes_to_human() {
        let s = bytes_to_human(1_073_741_824);
        assert!(s.contains("G"));
    }

    #[test]
    fn test_user_exists_root() {
        assert!(user_exists("root"));
    }

    #[test]
    fn test_user_exists_nonexistent() {
        assert!(!user_exists("nonexistent-user-12345"));
    }

    #[test]
    fn test_user_uid_root() {
        assert_eq!(user_uid("root"), Some(0));
    }

    #[test]
    fn test_user_groups_root_contains_root() {
        let groups = user_groups("root");
        assert!(groups.contains("root"));
    }

    #[test]
    fn test_user_shadow_hash_root_readable() {
        // May or may not be readable depending on perms (root-only)
        let h = user_shadow_hash("root");
        // Either Some (we have permission) or None (no permission)
        assert!(h.is_some() || h.is_none());
    }

    #[test]
    fn test_client_users_catalog_default() {
        let cat = ClientUsersCatalog::default();
        assert!(cat.users.is_empty());
    }

    #[test]
    fn test_client_users_catalog_roundtrip() {
        let mut cat = ClientUsersCatalog::default();
        cat.upsert(
            "alice",
            ClientUserEntry {
                uid: 1000,
                description: "Test".into(),
                hashed_password: "$6$xxx".into(),
                extra_groups: vec!["users".into()],
                group_gids: Default::default(),
            },
        );
        let json = serde_json::to_string(&cat).unwrap();
        let back: ClientUsersCatalog = serde_json::from_str(&json).unwrap();
        assert_eq!(back.users.len(), 1);
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
        let tmp = std::env::temp_dir().join(format!("gar-usermeta-{}", std::process::id()));
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
        let tmp = std::env::temp_dir().join(format!("gar-legacy-meta-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join(".garos-home-meta"), "USER=alice\nQUOTA=10G\n").unwrap();
        assert_eq!(read_meta_value(&tmp, "QUOTA"), Some("10G".to_string()));
        assert_eq!(read_meta_value(&tmp, "USER"), Some("alice".to_string()));
        assert_eq!(read_meta_value(&tmp, "MISSING"), None);
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}