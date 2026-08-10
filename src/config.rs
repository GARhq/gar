//! GAR CLI - configuration and paths
//!
//! Reads env vars (with defaults) and exposes typed accessors.
//! Centralizes all path/env resolution so commands stay clean.

use std::path::PathBuf;

use crate::error::{GarError, Result};

/// GAR-wide configuration. Cheap to clone (all fields are PathBuf/String).
#[derive(Debug, Clone, Default)]
pub struct Config {
    // Flake / repo
    pub flake_path: PathBuf,
    pub target_host: String,

    // Images (cliente diskless)
    pub images_root: PathBuf,
    pub http_root: PathBuf,
    pub data_root: PathBuf,
    pub tftp_root: PathBuf,

    // Server (NixOS)
    pub server_ip: String,
    pub http_port: u16,

    // Runtime / state
    pub runtime_root: PathBuf,
    pub audit_dir: PathBuf,

    // Storage (BTRFS tier1)
    pub home_base: PathBuf,
    pub home_archive_base: PathBuf,
    pub home_snapshot_base: PathBuf,
    pub storage_base: PathBuf,
    pub storage_archive: PathBuf,

    // Lock
    pub lock_path: PathBuf,

    // GC tuning
    pub keep_versions: u32,
    pub gc_grace_seconds: u64,
    pub gc_snapshot_keep: u32,

    // Output
    pub json_output: bool,
    pub verbose: bool,
    pub no_color: bool,
}

impl Config {
    /// Load config from env vars with defaults.
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            // Flake / repo
            flake_path: env_path("GAR_FLAKE_PATH", "/etc/gar")?,
            target_host: env_string("GAR_TARGET_HOST", "srv-gar"),

            // Images
            images_root: env_path("GAR_IMAGES_ROOT", "/srv/data/images")?,
            http_root: env_path("GAR_HTTP_ROOT", "/srv/data/http")?,
            data_root: env_path("GAR_DATA_ROOT", "/srv/data")?,
            tftp_root: env_path("GAR_TFTP_ROOT", "/srv/tftp")?,

            // Server
            server_ip: env_string("GAR_SERVER_IP", "127.0.0.1"),
            http_port: env_u16("GAR_HTTP_PORT", 8080)?,

            // Runtime
            runtime_root: env_path("GAR_RUNTIME_ROOT", "/var/lib/gar/runtime")?,
            audit_dir: env_path("GAR_AUDIT_DIR", "/var/lib/gar/audit")?,

            // Storage
            home_base: env_path("GAR_HOME_BASE", "/srv/data/home")?,
            home_archive_base: env_path("GAR_HOME_ARCHIVE_BASE", "/srv/data/home/.archive")?,
            home_snapshot_base: env_path("GAR_HOME_SNAPSHOT_BASE", "/srv/data/snapshots/users")?,
            storage_base: env_path("GAR_STORAGE_BASE", "/srv/data/storage")?,
            storage_archive: env_path("GAR_STORAGE_ARCHIVE", "/srv/data/storage/.archive")?,

            // Lock
            lock_path: env_path("GAR_LOCK_PATH", "/var/lib/gar/installer.lock")?,

            // GC
            keep_versions: env_u32("GAR_KEEP_VERSIONS", 5)?,
            gc_grace_seconds: env_u64("GAR_GC_GRACE_SECONDS", 900)?,
            gc_snapshot_keep: env_u32("GAR_GC_SNAPSHOT_KEEP", 7)?,

            // Output
            json_output: env_bool("GAR_JSON_OUTPUT", false),
            verbose: env_bool("GAR_VERBOSE", false),
            no_color: env_bool("GAR_NO_COLOR", false),
        })
    }

    /// Get the `installable` reference for nixos-rebuild (e.g. `git+file:///etc/gar#srv-gar`).
    pub fn installable(&self) -> String {
        format!(
            "git+file://{}#{}",
            self.flake_path.display(),
            self.target_host
        )
    }
}

fn env_string(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn env_bool(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(default)
}

fn env_u16(name: &str, default: u16) -> Result<u16> {
    match std::env::var(name) {
        Ok(v) => v
            .parse()
            .map_err(|_| GarError::config(format!("env var {}={} is not a valid u16", name, v))),
        Err(_) => Ok(default),
    }
}

fn env_u32(name: &str, default: u32) -> Result<u32> {
    match std::env::var(name) {
        Ok(v) => v
            .parse()
            .map_err(|_| GarError::config(format!("env var {}={} is not a valid u32", name, v))),
        Err(_) => Ok(default),
    }
}

fn env_u64(name: &str, default: u64) -> Result<u64> {
    match std::env::var(name) {
        Ok(v) => v
            .parse()
            .map_err(|_| GarError::config(format!("env var {}={} is not a valid u64", name, v))),
        Err(_) => Ok(default),
    }
}

fn env_path(name: &str, default: &str) -> Result<PathBuf> {
    let raw = std::env::var(name).unwrap_or_else(|_| default.to_string());
    let path = PathBuf::from(&raw);
    if path.as_os_str().is_empty() {
        return Err(GarError::config(format!(
            "env var {} resolved to empty path",
            name
        )));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_loads() {
        let cfg = Config::from_env().expect("config should load with defaults");
        assert_eq!(cfg.target_host, "srv-gar");
        assert_eq!(cfg.http_port, 8080);
        assert_eq!(cfg.keep_versions, 5);
    }

    #[test]
    fn test_installable_format() {
        let cfg = Config {
            flake_path: PathBuf::from("/etc/gar"),
            target_host: "srv-gar".into(),
            images_root: PathBuf::from("/x"),
            http_root: PathBuf::from("/x"),
            data_root: PathBuf::from("/x"),
            tftp_root: PathBuf::from("/x"),
            server_ip: "127.0.0.1".into(),
            http_port: 8080,
            runtime_root: PathBuf::from("/x"),
            audit_dir: PathBuf::from("/x"),
            home_base: PathBuf::from("/x"),
            home_archive_base: PathBuf::from("/x"),
            home_snapshot_base: PathBuf::from("/x"),
            storage_base: PathBuf::from("/x"),
            storage_archive: PathBuf::from("/x"),
            lock_path: PathBuf::from("/x"),
            keep_versions: 5,
            gc_grace_seconds: 900,
            gc_snapshot_keep: 7,
            json_output: false,
            verbose: false,
            no_color: false,
        };
        assert_eq!(cfg.installable(), "git+file:///etc/gar#srv-gar");
    }

    #[test]
    fn test_env_u16_invalid() {
        std::env::set_var("GAR_HTTP_PORT", "not-a-number");
        let r = Config::from_env();
        std::env::remove_var("GAR_HTTP_PORT");
        assert!(r.is_err());
    }
}
