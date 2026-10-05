use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::{generations, lock as global_lock, nix, runtime_guard};

/// Default critical services guarded after `nh os test` (matches GAROS garos-update.sh).
///
/// Order matches the bash script for stable logging; missing services are
/// silently skipped by `systemctl list-unit-files`, so this list may safely
/// include services that don't exist on a given host (e.g. garos-control-api
/// on a non-API host).
pub const DEFAULT_CRITICAL_SERVICES: &[&str] = &[
    "nginx",
    "dnsmasq",
    "nfs-server",
    "garos-control-api",
    "garos-control-web",
];

/// Minimum free space required on /nix before a build (3 GiB).
///
/// Matches the bash `garos-update` precheck so historical bash deployments
/// keep the same safety floor.
pub const MIN_FREE_NIX_MB: u64 = 3072;

/// Lock file used by `gar server update` to serialize concurrent updates.
const UPDATE_LOCK_PATH: &str = "/run/gar-update.lock";

/// Lock acquisition timeout (seconds) — matches `flock -w 60` semantics.
const UPDATE_LOCK_TIMEOUT_SECS: u64 = 60;

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

/// `gar server update` — flake update + check + test + safe switch.
///
/// Replicates the safety flow of `garos-update.sh` (lock, disk precheck,
/// health-check, automatic rollback on failure) and accepts flags to
/// override / skip each stage.
pub async fn cmd_update(
    services: Vec<String>,
    skip_disk_check: bool,
    skip_health_check: bool,
    dry_run: bool,
) -> Result<()> {
    let cfg = Config::from_env()?;
    runtime_guard::reexec_as_root_if_needed("server update")?;

    // Resolve flake_path with /etc/garos → cwd fallback (matches bash script).
    let flake_path = resolve_flake_path(&cfg)?;

    let services: Vec<String> = if services.is_empty() {
        DEFAULT_CRITICAL_SERVICES
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        services
    };

    output::section("==> gar server update (Safe Update Flow)");
    output::info(format!("flake       : {}", flake_path.display()));
    output::info(format!("target_host : {}", cfg.target_host));
    output::info(format!(
        "services    : {}",
        services.join(", ")
    ));
    output::info(format!(
        "flags       : skip_disk_check={} skip_health_check={} dry_run={}",
        skip_disk_check, skip_health_check, dry_run
    ));

    if dry_run {
        output::warn("dry-run ativo: nenhuma operação será executada.");
        return plan_only(&flake_path, &cfg, &services, skip_disk_check, skip_health_check);
    }

    let lock_path = std::path::Path::new(UPDATE_LOCK_PATH);
    global_lock::with_lock_timeout(
        lock_path,
        "server update",
        Duration::from_secs(UPDATE_LOCK_TIMEOUT_SECS),
        || {
            // 1) flake update
            output::info("[1/6] Atualizando flake inputs...");
            let rt = tokio::runtime::Handle::current();
            rt.block_on(nix::flake_update(&flake_path))?;

            // 2) flake check
            output::info("[2/6] Validando flake (nix flake check)...");
            rt.block_on(nix::flake_check(&flake_path))?;

            // 3) disk precheck
            if !skip_disk_check {
                output::info("[3/6] Checando espaço livre em /nix...");
                check_free_nix_space(MIN_FREE_NIX_MB)?;
            } else {
                output::info("[3/6] Checagem de disco pulada (--skip-disk-check).");
            }

            // 4) build + test (combined via "test"; the previous generation
            //    is bootable until we run "switch" below).
            output::info("[4/6] Aplicando em modo de teste (nh os test)...");
            run_nh_os_action(&cfg, "test")?;
            // Critical: if test failed, run_nh_os_action returned Err already.

            // 5) health check
            if !skip_health_check {
                output::info("[5/6] Verificando integridade dos serviços críticos...");
                std::thread::sleep(Duration::from_secs(3));
                let fails = check_critical_services(&services)?;
                if fails > 0 {
                    output::warn(format!(
                        "[CRÍTICO] {fails} serviço(s) crítico(s) falhou(aram). Executando rollback..."
                    ));
                    let _ = Command::new("nixos-rebuild")
                        .args(["switch", "--rollback"])
                        .status();
                    return Err(GarError::config(
                        "Update falhou nos health checks. Rollback concluído.",
                    ));
                }
            } else {
                output::info("[5/6] Health check pulado (--skip-health-check).");
            }

            // 6) switch
            output::info("[6/6] Consolidando no bootloader (nh os switch)...");
            run_nh_os_action(&cfg, "switch")?;

            Ok(())
        },
    )?;

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
    run_nh_os_action(cfg, action)
}

/// Internal: actually run `nh os <action>`. Kept separate so we can swap
/// `nh` for `nixos-rebuild` in tests if needed in the future.
fn run_nh_os_action(cfg: &Config, action: &str) -> Result<()> {
    let mut cmd = Command::new("nh");
    cmd.env("GAR_ENFORCE_RUNTIME_GUARDS", "1");
    cmd.arg("os");
    cmd.arg(action);
    cmd.arg(&cfg.flake_path);
    cmd.args(["--hostname", &cfg.target_host]);

    let status = cmd.status()?;
    if !status.success() {
        return Err(GarError::config(format!(
            "nh os {action} falhou: exit {}",
            status.code().unwrap_or(-1)
        )));
    }
    Ok(())
}

/// Resolve the operational flake, applying the bash fallback
/// `/etc/garos` (already in `Config::flake_path` default) → cwd.
fn resolve_flake_path(cfg: &Config) -> Result<std::path::PathBuf> {
    if cfg.flake_path.is_dir() {
        return Ok(cfg.flake_path.clone());
    }
    let cwd = std::env::current_dir()?;
    if cwd.join("flake.nix").is_file() {
        output::warn(format!(
            "flake ausente em {}; usando cwd {}",
            cfg.flake_path.display(),
            cwd.display()
        ));
        return Ok(cwd);
    }
    Err(GarError::runtime_guard(format!(
        "flake local ausente em {} (e cwd {} também não tem flake.nix)",
        cfg.flake_path.display(),
        cwd.display()
    )))
}

/// Ensure `/nix` has at least `min_mb` MiB of free space.
fn check_free_nix_space(min_mb: u64) -> Result<()> {
    let output = Command::new("df")
        .args(["-B1M", "/nix"])
        .output()?;
    if !output.status.success() {
        return Err(GarError::runtime_guard(format!(
            "df /nix falhou: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    // Header + one line. Example: "Filesystem 1M-blocks Used Available Use% Mounted on"
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().nth(1).ok_or_else(|| {
        GarError::runtime_guard("df /nix retornou saída inesperada".to_string())
    })?;
    // 3rd whitespace-separated column = "Available"
    let free_mb: u64 = line
        .split_whitespace()
        .nth(3)
        .ok_or_else(|| GarError::runtime_guard("df /nix sem coluna Available".to_string()))?
        .parse()
        .map_err(|e| GarError::runtime_guard(format!("parse do free_mb: {e}")))?;
    if free_mb < min_mb {
        return Err(GarError::runtime_guard(format!(
            "Espaço insuficiente em /nix. Necessário: {min_mb}MB, Disponível: {free_mb}MB. \
             Execute 'gar server clean' para liberar espaço."
        )));
    }
    output::info(format!(
        "[OK] Espaço livre em /nix: {free_mb}MB (mínimo {min_mb}MB)"
    ));
    Ok(())
}

/// Probe each service. Services that don't have a unit file (e.g. not
/// installed on this host) are skipped without counting as a failure.
/// Returns the number of failures.
fn check_critical_services(services: &[String]) -> Result<u32> {
    let mut fails = 0u32;
    for svc in services {
        // `systemctl list-unit-files <unit>` exits non-zero when the unit
        // is not present and prints "0 unit files listed." — match the
        // bash script semantics: missing unit is NOT a failure.
        let present = Command::new("systemctl")
            .args(["list-unit-files", &format!("{svc}.service")])
            .output()?;
        if !present.status.success() {
            output::info(format!(
                "[skip] {svc}: unit file ausente neste sistema, no falha do update"
            ));
            continue;
        }
        let stdout = String::from_utf8_lossy(&present.stdout);
        let listed = stdout.lines().any(|l| l.trim_start().starts_with(&format!("{svc}.service")));
        if !listed {
            output::info(format!(
                "[skip] {svc}: unit file ausente neste sistema, no falha do update"
            ));
            continue;
        }
        let active = Command::new("systemctl")
            .args(["is-active", "--quiet", svc])
            .status()?;
        if !active.success() {
            output::warn(format!(
                "[ERRO] Serviço {svc} falhou ou está inativo após atualização!"
            ));
            fails += 1;
        } else {
            output::info(format!("[OK] {svc} ativo"));
        }
    }
    Ok(fails)
}

/// Dry-run plan printer.
fn plan_only(
    flake_path: &Path,
    cfg: &Config,
    services: &[String],
    skip_disk_check: bool,
    skip_health_check: bool,
) -> Result<()> {
    let mut plan = String::new();
    plan.push_str("Plano de atualização (DRY-RUN, nada será executado):\n");
    plan.push_str(&format!(
        "  flake       : {}\n",
        flake_path.display()
    ));
    plan.push_str(&format!("  target_host : {}\n", cfg.target_host));
    plan.push_str(&format!("  services    : {}\n", services.join(", ")));
    plan.push_str("  etapas      :\n");
    plan.push_str("    1. nix flake update\n");
    plan.push_str("    2. nix flake check\n");
    plan.push_str(&format!(
        "    3. df /nix >= {MIN_FREE_NIX_MB}MB  {}\n",
        if skip_disk_check { "(PULADO)" } else { "" }
    ));
    plan.push_str("    4. nh os test\n");
    plan.push_str(&format!(
        "    5. systemctl is-active (serviços críticos)  {}\n",
        if skip_health_check { "(PULADO)" } else { "" }
    ));
    plan.push_str("    6. nh os switch\n");
    output::info(plan.trim_end());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_services_include_all_critical() {
        // Order matters (stable logs).
        assert_eq!(
            DEFAULT_CRITICAL_SERVICES,
            &["nginx", "dnsmasq", "nfs-server", "garos-control-api", "garos-control-web"]
        );
    }

    #[test]
    fn min_free_space_is_3gb() {
        assert_eq!(MIN_FREE_NIX_MB, 3072);
    }

    #[test]
    fn lock_path_matches_bash_script() {
        // The bash `garos-update` used /run/garos-update.lock; we keep a
        // gar- namespace file to avoid colliding with non-gar tools.
        assert_eq!(UPDATE_LOCK_PATH, "/run/gar-update.lock");
    }

    #[test]
    fn check_free_nix_space_succeeds_on_normal_fs() {
        // /nix should exist on the test host (we are on NixOS or have /nix).
        let r = check_free_nix_space(1);
        assert!(r.is_ok(), "1MB minimum must always pass: {:?}", r);
    }

    #[test]
    fn check_free_nix_space_fails_when_floor_too_high() {
        let r = check_free_nix_space(u64::MAX);
        assert!(r.is_err(), "u64::MAX floor must fail");
        match r.unwrap_err() {
            GarError::RuntimeGuard(msg) => assert!(msg.contains("/nix")),
            other => panic!("expected RuntimeGuard, got {other:?}"),
        }
    }

    #[test]
    fn check_critical_services_skips_unknown_units() {
        // A clearly-not-installed service name must be skipped without error.
        let r = check_critical_services(&["garos-nonexistent-unit-xyz123".into()]);
        assert!(r.is_ok(), "missing units must be skipped: {:?}", r);
        assert_eq!(r.unwrap(), 0, "missing units must not count as failures");
    }

    #[test]
    fn resolve_flake_path_accepts_existing_dir() {
        let tmp = std::env::temp_dir().join(format!("gar-resolve-{}-{}", std::process::id(), line!()));
        std::fs::create_dir_all(&tmp).unwrap();
        let mut cfg = Config::from_env().unwrap();
        cfg.flake_path = tmp.clone();
        let resolved = resolve_flake_path(&cfg).unwrap();
        assert_eq!(resolved, tmp);
        std::fs::remove_dir_all(&tmp).ok();
    }
}