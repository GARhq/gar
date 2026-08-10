//! Group system operations.
//!
//! Wraps `groupadd`/`groupdel`/`gpasswd` and manages the per-group storage
//! sector layout (`<storage_base>/<name>` with quota + `.group-meta` + catalog).
//!
//! Catalog format (`<runtime_root>/user-groups.json`):
//! ```json
//! {
//!   "admin":   { "description": "...", "storagePath": "...", "quota": "100G", "gid": 1000, "created_at": "..." },
//!   "users":   { ... },
//!   "lab":     { ... }
//! }
//! ```
//!
//! The catalog key namespace is shared with `user_system::UserGroupsCatalog`
//! (which stores `{ "alice": "default" }`); we use a separate file
//! (`group-catalog.json`) so the two maps never collide.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Per-group storage quota, normalized to IEC bytes at construction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuotaSpec {
    pub human: String,
    pub bytes: u64,
}

impl QuotaSpec {
    pub fn new(human: &str) -> Result<Self> {
        let bytes = crate::services::user_system::human_to_bytes(human)?;
        Ok(Self {
            human: human.into(),
            bytes,
        })
    }
}

/// Per-group metadata file (`<storage_base>/<name>/.group-meta`). Key=value format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMeta {
    pub group: String,
    pub description: String,
    pub gid: u32,
    pub quota: String,
    pub created_at: String,
}

/// Catalog row exposed by `gar group list`.
#[derive(Debug, Clone, Serialize)]
pub struct GroupRow {
    pub name: String,
    pub description: String,
    pub gid: u32,
    pub quota: String,
    pub quota_bytes: u64,
    pub storage_path: String,
    pub usage: String,
}

/// Catalog on disk: `<runtime_root>/group-catalog.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GroupCatalog {
    #[serde(default)]
    pub groups: HashMap<String, CatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub description: String,
    #[serde(rename = "storagePath")]
    pub storage_path: String,
    pub quota: String,
    pub gid: u32,
    #[serde(rename = "created_at")]
    pub created_at: String,
}

impl GroupCatalog {
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

    pub fn upsert(&mut self, name: &str, entry: CatalogEntry) {
        self.groups.insert(name.into(), entry);
    }

    pub fn remove(&mut self, name: &str) -> bool {
        self.groups.remove(name).is_some()
    }

    pub fn get(&self, name: &str) -> Option<&CatalogEntry> {
        self.groups.get(name)
    }
}

/// Per-group permissions file (`<storage_base>/<name>/.group-permissions`).
/// Plain `key=value` lines, free-form for now.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct GroupPermissions {
    pub mode: String,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub extra: HashMap<String, String>,
}

impl GroupPermissions {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).unwrap_or_default();
        Ok(serde_json::from_slice(&bytes).unwrap_or_default())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        atomic_write(path, &serde_json::to_vec_pretty(self)?, 0o640)
    }
}

/// Create a system group (with optional GID).
pub async fn groupadd_system(name: &str, gid: Option<u32>) -> Result<()> {
    let mut args: Vec<String> = vec!["-r".to_string()];
    if let Some(g) = gid {
        args.push("-g".to_string());
        args.push(g.to_string());
    }
    args.push(name.to_string());
    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
    let _ = crate::services::shell::run_success("groupadd", &args_ref).await?;
    Ok(())
}

/// Check if a group exists.
pub fn group_exists(name: &str) -> bool {
    std::process::Command::new("getent")
        .args(["group", name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Resolve GID of an existing group, or None if missing.
pub fn group_gid(name: &str) -> Option<u32> {
    let out = std::process::Command::new("getent")
        .args(["group", name])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout.split(':').nth(2)?.trim().parse().ok()
}

/// Add a user to a supplementary group (idempotent — `gpasswd -a` is no-op on duplicates).
pub async fn group_add_member(group: &str, user: &str) -> Result<()> {
    crate::services::shell::run_success("gpasswd", &["-a", user, group]).await?;
    Ok(())
}

/// Remove a user from a supplementary group.
pub async fn group_remove_member(group: &str, user: &str) -> Result<()> {
    let _ = crate::services::shell::run_success("gpasswd", &["-d", user, group]).await;
    Ok(())
}

/// List all members of a group (parsed from `getent group`).
pub fn group_members(name: &str) -> Vec<String> {
    let out = std::process::Command::new("getent")
        .args(["group", name])
        .output()
        .ok();
    let Some(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    let s = String::from_utf8_lossy(&out.stdout);
    s.split(':')
        .nth(3)
        .map(|m| {
            m.split(',')
                .filter(|x| !x.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Per-sector storage path (`<storage_base>/<name>`).
pub fn sector_path(storage_base: &Path, name: &str) -> PathBuf {
    storage_base.join(name)
}

/// Per-sector metadata path (`.group-meta`).
pub fn meta_path(sector: &Path) -> PathBuf {
    sector.join(".group-meta")
}

/// Per-sector permissions path (`.group-permissions`).
pub fn permissions_path(sector: &Path) -> PathBuf {
    sector.join(".group-permissions")
}

/// Build a `GroupMeta` value at creation time.
pub fn build_meta(name: &str, description: &str, gid: u32, quota: &QuotaSpec) -> GroupMeta {
    GroupMeta {
        group: name.into(),
        description: description.into(),
        gid,
        quota: quota.human.clone(),
        created_at: Utc::now().to_rfc3339(),
    }
}

/// Write `.group-meta` (key=value legacy format compatible with bash ragos).
pub fn write_meta(sector: &Path, meta: &GroupMeta) -> Result<()> {
    std::fs::create_dir_all(sector)?;
    let content = format!(
        "GROUP={}\nDESCRIPTION={}\nGID={}\nQUOTA={}\nCREATED_AT={}\n",
        meta.group, meta.description, meta.gid, meta.quota, meta.created_at
    );
    atomic_write(&meta_path(sector), content.as_bytes(), 0o640)
}

/// Read a single key from `.group-meta` (returns None if missing).
pub fn read_meta_value(sector: &Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(meta_path(sector)).ok()?;
    content.lines().find_map(|l| {
        l.split_once('=')
            .filter(|(k, _)| *k == key)
            .map(|(_, v)| v.into())
    })
}

/// Total bytes used by a sector (`du -sb`).
pub fn sector_usage_bytes(sector: &Path) -> u64 {
    crate::services::user_system::dir_size_bytes(sector)
}

/// Apply quota to a sector path (delegates to FsOps: BTRFS subvolume, XFS project, ZFS dataset).
pub async fn apply_quota(sector: &Path, quota: &QuotaSpec) -> Result<()> {
    let ops = crate::services::filesystem::FsOps::for_path(sector)?;
    ops.enable_quotas(sector.parent().unwrap_or(sector)).await?;
    ops.set_quota(sector, &quota.human).await
}

/// Set sector ownership to `root:<group>` and mode 0750.
pub fn chown_sector(sector: &Path, group: &str) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(sector)?;
    let _ = std::process::Command::new("chown")
        .args([
            "-R",
            &format!("root:{}", group),
            &sector.display().to_string(),
        ])
        .status();
    std::fs::set_permissions(sector, PermissionsExt::from_mode(0o750))?;
    Ok(())
}

/// Catalog file path under runtime root.
pub fn catalog_path(runtime_root: &Path) -> PathBuf {
    runtime_root.join("group-catalog.json")
}

/// Sentinel for the permanent admin group. Refuse to delete it.
pub fn is_permanent(name: &str) -> bool {
    name == "admin"
}

fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    std::fs::set_permissions(path, PermissionsExt::from_mode(mode))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_meta_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("gar-group-meta-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let meta = GroupMeta {
            group: "lab".into(),
            description: "Lab sector".into(),
            gid: 1500,
            quota: "100G".into(),
            created_at: "2026-08-09T10:00:00+00:00".into(),
        };
        write_meta(&tmp, &meta).unwrap();
        let back = read_meta_value(&tmp, "QUOTA").unwrap();
        assert_eq!(back, "100G");
        assert_eq!(read_meta_value(&tmp, "GROUP").unwrap(), "lab");
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_catalog_roundtrip() {
        let mut cat = GroupCatalog::default();
        cat.upsert(
            "lab",
            CatalogEntry {
                description: "Lab sector".into(),
                storage_path: "/srv/data/storage/lab".into(),
                quota: "100G".into(),
                gid: 1500,
                created_at: "2026-08-09T10:00:00+00:00".into(),
            },
        );
        let json = serde_json::to_string(&cat).unwrap();
        let mut back: GroupCatalog = serde_json::from_str(&json).unwrap();
        assert_eq!(back.get("lab").unwrap().gid, 1500);
        assert!(back.remove("lab"));
        assert!(!back.remove("lab"));
    }

    #[test]
    fn test_quota_spec_parses_iec() {
        let q = QuotaSpec::new("20G").unwrap();
        assert_eq!(q.bytes, 21_474_836_480); // 20 * 2^30
        assert_eq!(q.human, "20G");
    }

    #[test]
    fn test_quota_spec_rejects_garbage() {
        assert!(QuotaSpec::new("100XYZ").is_err());
    }

    #[test]
    fn test_is_permanent_admin() {
        assert!(is_permanent("admin"));
        assert!(!is_permanent("lab"));
    }

    #[test]
    fn test_permissions_default_and_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("gar-group-perms-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let perms = GroupPermissions {
            mode: "0750".into(),
            members: vec!["alice".into(), "bob".into()],
            extra: Default::default(),
        };
        let p = permissions_path(&tmp);
        perms.save(&p).unwrap();
        let back = GroupPermissions::load(&p).unwrap();
        assert_eq!(back.mode, "0750");
        assert_eq!(back.members.len(), 2);
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
