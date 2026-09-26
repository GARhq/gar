use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::{generations, nix, runtime_guard};
use std::process::Command;

/// `gar server switch` — nixos-rebuild switch.
pub async fn cmd_switch() -> Result<()> {
    let cfg = Config::from_env()?;
    output::section("==> gar server switch");
    runtime_guard::validate(&cfg)?;
    runtime_guard::reexec_as_root_if_needed("server switch")?;
    run_nh_os(&cfg, "switch")?;
    output::ok("switch concluído");
    Ok(())
}

/// `gar server test` — nixos-rebuild test.
pub async fn cmd_test() -> Result<()> {
    let cfg = Config::from_env()?;
    output::section("==> gar server test");
    runtime_guard::validate(&cfg)?;
    runtime_guard::reexec_as_root_if_needed("server test")?;
    run_nh_os(&cfg, "test")?;
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

/// `gar server update` — flake update + check + safe switch.
pub async fn cmd_update() -> Result<()> {
    let cfg = Config::from_env()?;
    output::section("==> gar server update (Safe Update Flow)");
    runtime_guard::validate(&cfg)?;
    runtime_guard::reexec_as_root_if_needed("server update")?;

    nix::flake_update(&cfg.flake_path).await?;
    nix::flake_check(&cfg.flake_path).await?;

    output::info("Aplicando configuração em modo de teste (nh os test)...");
    run_nh_os(&cfg, "test")?;

    output::info("Checando a saúde dos serviços críticos...");
    std::thread::sleep(std::time::Duration::from_secs(3));

    let critical_services = ["nginx", "dnsmasq", "nfs-server"];
    let mut fails = 0;
    for svc in critical_services.iter() {
        let st = Command::new("systemctl")
            .args(["is-active", "--quiet", svc])
            .status()?;
        if !st.success() {
            output::warn(&format!("[ERRO] Serviço {} falhou após atualização!", svc));
            fails += 1;
        }
    }

    if fails > 0 {
        output::warn("[CRÍTICO] Atualização quebrou a infraestrutura. Revertendo (rollback)...");
        let _ = Command::new("nixos-rebuild")
            .args(["switch", "--rollback"])
            .status();
        return Err(GarError::config(
            "Update falhou nos health checks. Rollback concluído.",
        ));
    }

    output::info("Serviços estáveis. Consolidando no boot (nh os switch)...");
    run_nh_os(&cfg, "switch")?;

    output::ok("Atualização GAROS concluída sem quedas! 🎉");
    Ok(())
}

/// `gar server clean` — nh clean all + fallback.
pub async fn cmd_clean() -> Result<()> {
    let cfg = Config::from_env()?;
    output::section("==> gar server clean");
    runtime_guard::reexec_as_root_if_needed("server clean")?;

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

/// Run nh os with the appropriate flags.
pub fn run_nh_os(cfg: &Config, action: &str) -> Result<()> {
    let mut cmd = Command::new("nh");
    cmd.env("GAR_ENFORCE_RUNTIME_GUARDS", "1");
    cmd.arg("os");
    cmd.arg(action);
    cmd.arg(&cfg.flake_path);
    cmd.args(["--hostname", &cfg.target_host]);

    let status = cmd.status()?;
    if !status.success() {
        return Err(GarError::config(format!(
            "nh os {} falhou: exit {}",
            action,
            status.code().unwrap_or(-1)
        )));
    }
    Ok(())
}
