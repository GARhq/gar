use std::env;
use std::process::Command;
use serde::Serialize;
use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::{generations, nix};

#[derive(Debug, Serialize)]
pub struct StatusReport {
    pub flake_path: String,
    pub target_host: String,
    pub runtime_root: String,
    pub directory_exists: bool,
    pub current_generation: String,
    pub nixos_rebuild_available: bool,
}

/// `gar server check` — nix flake check.
pub async fn cmd_check(inventory: Option<std::path::PathBuf>, allow_empty: bool) -> Result<()> {
    let cfg = Config::from_env()?;
    output::section("==> gar server check");
    if let Some(inv_path) = inventory {
        if !inv_path.exists() {
            return Err(GarError::config(format!(
                "arquivo de inventário ausente em {}",
                inv_path.display()
            )));
        }
        let errors = nix::validate_inventory(&cfg.flake_path, &inv_path, allow_empty).await?;
        if !errors.is_empty() {
            return Err(GarError::validation(errors.join("\n")));
        }
        output::ok(format!(
            "validação do inventário OK para {}",
            inv_path.display()
        ));
    } else {
        if !cfg.flake_path.is_dir() {
            return Err(GarError::config(format!(
                "flake local ausente em {}",
                cfg.flake_path.display()
            )));
        }
        nix::flake_check(&cfg.flake_path).await?;
        output::ok(format!("flake check OK em {}", cfg.flake_path.display()));
    }
    Ok(())
}

/// `gar server repl` — nix repl (interactive).
pub fn cmd_repl() -> Result<()> {
    let cfg = Config::from_env()?;
    if !cfg.flake_path.is_dir() {
        return Err(GarError::config(format!(
            "flake local ausente em {}",
            cfg.flake_path.display()
        )));
    }
    let status = Command::new("nix")
        .args([
            "repl",
            "--expr",
            &format!("builtins.getFlake \"{}\"", cfg.flake_path.display()),
        ])
        .status()?;
    std::process::exit(status.code().unwrap_or(1));
}

/// `gar server path` — print operational flake path.
pub fn cmd_path() -> Result<()> {
    let cfg = Config::from_env()?;
    println!("{}", cfg.flake_path.display());
    Ok(())
}

/// `gar server enter` — cd to flake + exec bash.
pub fn cmd_enter() -> Result<()> {
    let cfg = Config::from_env()?;
    if !cfg.flake_path.is_dir() {
        return Err(GarError::config(format!(
            "flake local ausente em {}",
            cfg.flake_path.display()
        )));
    }
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
    let status = Command::new(shell)
        .args(["-l"])
        .current_dir(&cfg.flake_path)
        .status()?;
    std::process::exit(status.code().unwrap_or(1));
}

/// `gar server status` — show flake/host/generation state.
pub async fn cmd_status() -> Result<()> {
    let cfg = Config::from_env()?;
    let directory_exists = cfg.flake_path.is_dir();

    let nixos_rebuild_available = Command::new("which")
        .arg("nixos-rebuild")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let report = StatusReport {
        flake_path: cfg.flake_path.display().to_string(),
        target_host: cfg.target_host.clone(),
        runtime_root: cfg.runtime_root.display().to_string(),
        directory_exists,
        current_generation: generations::current_number(),
        nixos_rebuild_available,
    };

    if cfg.json_output {
        output::json(&report)?;
    } else {
        println!("flake_path: {}", report.flake_path);
        println!("target_host: {}", report.target_host);
        println!("runtime_root: {}", report.runtime_root);
        println!(
            "directory_exists: {}",
            if report.directory_exists {
                "sim"
            } else {
                "nao"
            }
        );
        println!("current_generation: {}", report.current_generation);
        println!(
            "nixos_rebuild_available: {}",
            if report.nixos_rebuild_available {
                "sim"
            } else {
                "nao"
            }
        );
    }

    Ok(())
}
