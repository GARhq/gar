//! Runtime guard for GAR server commands.
//!
//! Replaces `validate_runtime_guard` from ragos-cli.nix.
//! Verifies that the operational checkout exists, has required runtime
//! files, and `/boot` resolves to a real device.

use std::path::Path;

use crate::config::Config;
use crate::error::{GarError, Result};

/// Validate the GAR runtime is intact before applying changes.
///
/// Checks:
/// - `flake_path` is a directory
/// - `params.nix` exists in runtime_root
/// - `hardware-configuration.nix` exists in runtime_root
/// - `/boot` resolves to a real device (not placeholder)
pub fn validate(cfg: &Config) -> Result<()> {
    require_flake_dir(&cfg.flake_path)?;
    require_runtime_file(cfg, "params.nix")?;
    require_runtime_file(cfg, "hardware-configuration.nix")?;

    // Resolve /boot via nix eval (best-effort)
    let boot_device = resolve_boot_device(cfg).unwrap_or_default();
    if boot_device.is_empty() {
        return Err(GarError::runtime_guard(
            "nao foi possivel resolver /boot para o installable",
        ));
    }
    match boot_device.as_str() {
        "/dev/disk/by-label/ESP" | "/dev/disk/by-label/nixos" => {
            return Err(GarError::runtime_guard(format!(
                "/boot ainda resolve para placeholder ({})",
                boot_device
            )));
        }
        _ => {}
    }

    Ok(())
}

fn require_flake_dir(flake_path: &Path) -> Result<()> {
    if !flake_path.is_dir() {
        return Err(GarError::runtime_guard(format!(
            "flake local ausente em {}",
            flake_path.display()
        )));
    }
    Ok(())
}

fn require_runtime_file(cfg: &Config, file_name: &str) -> Result<()> {
    let path = cfg.runtime_root.join(file_name);
    if !path.is_file() {
        return Err(GarError::runtime_guard(format!(
            "runtime ausente em {} (faltando {})",
            cfg.runtime_root.display(),
            file_name
        )));
    }
    Ok(())
}

fn resolve_boot_device(cfg: &Config) -> Result<String> {
    let output = std::process::Command::new("nix")
        .args([
            "eval",
            "--impure",
            "--raw",
            "--expr",
            &format!(
                "let flake = builtins.getFlake \"git+file://{}\"; in flake.nixosConfigurations.\"{}\".config.fileSystems.\"/boot\".device",
                cfg.flake_path.display(),
                cfg.target_host
            ),
        ])
        .output()?;

    if !output.status.success() {
        return Err(GarError::runtime_guard(format!(
            "nix eval falhou: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Re-exec the current process with root privileges (sudo) when needed.
///
/// Returns `Ok(true)` if re-exec happened, `Ok(false)` if already root or
/// the action doesn't require root.
pub fn reexec_as_root_if_needed(action: &str) -> Result<bool> {
    // Already root? (euid=0)
    let uid_output = std::process::Command::new("id")
        .arg("-u")
        .output()
        .map_err(|e| crate::error::GarError::runtime_guard(format!("id falhou: {}", e)))?;
    let uid_str = String::from_utf8_lossy(&uid_output.stdout);
    if uid_str.trim() == "0" {
        return Ok(false);
    }

    // Sudo available?
    let sudo_check = std::process::Command::new("sudo").arg("--version").output();
    if sudo_check.is_err() {
        return Err(crate::error::GarError::runtime_guard(format!(
            "requer root ou sudo para executar: {}",
            action
        )));
    }

    // Re-exec via sudo (current args)
    let args: Vec<String> = std::env::args().collect();
    let status = std::process::Command::new("sudo")
        .args(&args)
        .env("GAR_SUDO_REENTRY", "1")
        .status()
        .map_err(|e| crate::error::GarError::runtime_guard(format!("sudo falhou: {}", e)))?;

    std::process::exit(status.code().unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_require_flake_dir_missing() {
        let r = require_flake_dir(Path::new("/nonexistent/path/that/should/not/exist"));
        assert!(matches!(r, Err(GarError::RuntimeGuard(_))));
    }

    #[test]
    fn test_require_flake_dir_existing() {
        let tmp = std::env::temp_dir().join(format!("gar-runtime-{}-flake", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(require_flake_dir(&tmp).is_ok());
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_validate_fails_for_missing_flake() {
        let mut cfg = Config::from_env().unwrap();
        cfg.flake_path = Path::new("/nonexistent/path").to_path_buf();
        let r = validate(&cfg);
        assert!(matches!(r, Err(GarError::RuntimeGuard(_))));
    }
}
