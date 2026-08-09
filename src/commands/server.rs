//! `gar server` subcommand — manages NixOS server (srv-gar).
//!
//! Replaces `ragos` top-level commands (server/ragos-cli.nix lines 494-567).
//! Commands: sync, switch, test, rollback, update, clean, check, repl, path, enter, status.

use std::env;
use std::process::Command;

use serde::Serialize;

use crate::cli::ServerCmd;
use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::{generations, git, nix, runtime_guard};

/// Dispatch a ServerCmd to its handler.
pub async fn dispatch(cmd: ServerCmd) -> Result<()> {
    match cmd {
        ServerCmd::Sync => cmd_sync().await,
        ServerCmd::Switch => cmd_switch().await,
        ServerCmd::Test => cmd_test().await,
        ServerCmd::Rollback => cmd_rollback().await,
        ServerCmd::Update => cmd_update().await,
        ServerCmd::Clean => cmd_clean().await,
        ServerCmd::Check => cmd_check().await,
        ServerCmd::Repl => cmd_repl(),
        ServerCmd::Path => cmd_path(),
        ServerCmd::Enter => cmd_enter(),
        ServerCmd::Status => cmd_status().await,
    }
}

#[derive(Debug, Serialize)]
pub struct StatusReport {
    pub flake_path: String,
    pub target_host: String,
    pub runtime_root: String,
    pub directory_exists: bool,
    pub current_generation: String,
    pub nixos_rebuild_available: bool,
}

/// `gar server sync` — fetch + pull + submodule update.
pub async fn cmd_sync() -> Result<()> {
    let cfg = Config::from_env()?;
    output::section("==> gar server sync");
    runtime_guard::reexec_as_root_if_needed("server sync")?;
    if !cfg.flake_path.is_dir() {
        return Err(GarError::config(format!(
            "flake local ausente em {}",
            cfg.flake_path.display()
        )));
    }
    git::sync_full(&cfg.flake_path).await?;
    output::ok(format!("sync concluído em {}", cfg.flake_path.display()));
    Ok(())
}

/// `gar server switch` — nixos-rebuild switch.
pub async fn cmd_switch() -> Result<()> {
    let cfg = Config::from_env()?;
    output::section("==> gar server switch");
    runtime_guard::validate(&cfg)?;
    runtime_guard::reexec_as_root_if_needed("server switch")?;
    run_nixos_rebuild(&cfg, "switch")?;
    output::ok("switch concluído");
    Ok(())
}

/// `gar server test` — nixos-rebuild test.
pub async fn cmd_test() -> Result<()> {
    let cfg = Config::from_env()?;
    output::section("==> gar server test");
    runtime_guard::validate(&cfg)?;
    runtime_guard::reexec_as_root_if_needed("server test")?;
    run_nixos_rebuild(&cfg, "test")?;
    output::ok("test concluído");
    Ok(())
}

/// `gar server rollback` — nixos-rebuild switch --rollback.
pub async fn cmd_rollback() -> Result<()> {
    let _cfg = Config::from_env()?;
    output::section("==> gar server rollback");
    runtime_guard::reexec_as_root_if_needed("server rollback")?;
    let status = Command::new("nixos-rebuild")
        .args(["switch", "--rollback"])
        .status()?;
    if !status.success() {
        return Err(GarError::config(format!(
            "nixos-rebuild switch --rollback falhou: exit {}",
            status.code().unwrap_or(-1)
        )));
    }
    output::ok("rollback para geração anterior concluído");
    Ok(())
}

/// `gar server update` — flake update + check + switch.
pub async fn cmd_update() -> Result<()> {
    let cfg = Config::from_env()?;
    output::section("==> gar server update");
    runtime_guard::validate(&cfg)?;
    runtime_guard::reexec_as_root_if_needed("server update")?;
    nix::flake_update(&cfg.flake_path).await?;
    nix::flake_check(&cfg.flake_path).await?;
    run_nixos_rebuild(&cfg, "switch")?;
    output::ok("update + check + switch concluído");
    Ok(())
}

/// `gar server clean` — nh clean all + fallback nix-collect-garbage.
pub async fn cmd_clean() -> Result<()> {
    let cfg = Config::from_env()?;
    output::section("==> gar server clean");
    runtime_guard::reexec_as_root_if_needed("server clean")?;

    // Try nh first
    let nh_check = Command::new("which").arg("nh").output();
    if nh_check.map(|o| o.status.success()).unwrap_or(false) {
        let status = Command::new("nh")
            .args([
                "clean",
                "all",
                "--keep",
                "5",
                "--keep-since",
                "7d",
                "--optimise",
            ])
            .status()?;
        if status.success() {
            output::ok("nh clean all concluído");
            return Ok(());
        }
        output::warn("nh clean falhou, usando fallback");
    } else {
        output::info("nh não disponível, usando fallback manual");
    }

    generations::clean_fallback(&cfg)?;
    output::ok("clean concluído (fallback)");
    Ok(())
}

/// `gar server check` — nix flake check.
pub async fn cmd_check() -> Result<()> {
    let cfg = Config::from_env()?;
    output::section("==> gar server check");
    if !cfg.flake_path.is_dir() {
        return Err(GarError::config(format!(
            "flake local ausente em {}",
            cfg.flake_path.display()
        )));
    }
    nix::flake_check(&cfg.flake_path).await?;
    output::ok(format!("flake check OK em {}", cfg.flake_path.display()));
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
        println!("directory_exists: {}", if report.directory_exists { "sim" } else { "nao" });
        println!("current_generation: {}", report.current_generation);
        println!(
            "nixos_rebuild_available: {}",
            if report.nixos_rebuild_available { "sim" } else { "nao" }
        );
    }

    Ok(())
}

/// Run nixos-rebuild with the appropriate flags.
fn run_nixos_rebuild(cfg: &Config, action: &str) -> Result<()> {
    let installable = cfg.installable();
    let mut cmd = Command::new("nixos-rebuild");
    cmd.env("GAR_ENFORCE_RUNTIME_GUARDS", "1");
    cmd.arg(action);
    if action == "switch" || action == "test" {
        cmd.args(["--impure", "--flake", &installable]);
    }
    let status = cmd.status()?;
    if !status.success() {
        return Err(GarError::config(format!(
            "nixos-rebuild {} falhou: exit {}",
            action,
            status.code().unwrap_or(-1)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_report_serialize() {
        let r = StatusReport {
            flake_path: "/etc/gar".into(),
            target_host: "srv-gar".into(),
            runtime_root: "/var/lib/gar/runtime".into(),
            directory_exists: true,
            current_generation: "42".into(),
            nixos_rebuild_available: true,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("flake_path"));
        assert!(json.contains("srv-gar"));
    }

    #[test]
    fn test_installable_format() {
        let cfg = Config::from_env().unwrap();
        let inst = cfg.installable();
        assert!(inst.starts_with("git+file://"));
        assert!(inst.contains(&cfg.target_host));
    }
}