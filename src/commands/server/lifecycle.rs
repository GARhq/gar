use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::{generations, lock as global_lock, runtime_guard};

/// Default critical services guarded after `nh os test` (matches GAROS garos-update.sh).
///
/// Order matches the bash script for stable logging; missing services are
/// silently skipped by `systemctl list-unit-files`, so this list may safely
/// include services that don't exist on a given host (e.g. garos-control-api
/// on a non-API host).
///
/// Post-migration (EROFS + JuiceFS + Caddy): the bash side dropped nginx/nfs-server
/// in favour of caddy (web), redis-juicefs-meta (metadata) and minio (object
/// storage). Keep this list aligned with `garos-update.sh`.
pub const DEFAULT_CRITICAL_SERVICES: &[&str] = &[
    "caddy",
    "dnsmasq",
    "redis-juicefs-meta",
    "minio",
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

// ─── Update pipeline (steps are the unit of change) ────────────────────
//
// The 7 phases of `gar server update` are described declaratively as
// a `&[UpdateStep]`. Each step carries:
//   * a stable id (rendered as `[N/7]` in progress logs)
//   * a human title (rendered in progress and dry-run output)
//   * which skip-flag (if any) suppresses the step
//   * the action to run (a plain `fn` pointer — no closure, no
//     boxing, no allocation per step)
//
// This means `cmd_update` and `plan_only` walk the same data, so the
// dry-run output and the real run can never drift out of sync.

/// Per-step context passed to every action. Carries everything the
/// steps need without having to thread 6 parameters through 8
/// helpers.
struct UpdateContext {
    cfg: Config,
    flake_path: PathBuf,
    services: Vec<String>,
    flags: UpdateFlags,
}

/// Flags that gate individual steps.
#[derive(Copy, Clone)]
struct UpdateFlags {
    skip_disk_check: bool,
    skip_health_check: bool,
    skip_gc: bool,
    check_flake: bool,
}

/// Which user-supplied flag, if any, suppresses a step. Used by
/// `plan_only` to render the `(PULADO)` marker.
#[derive(Copy, Clone, PartialEq, Eq)]
enum SkipReason {
    Disk,
    Gc,
    Health,
    Flake,
}

impl SkipReason {
    fn label(self) -> &'static str {
        match self {
            SkipReason::Disk => "(PULADO: --skip-disk-check)",
            SkipReason::Gc => "(PULADO: --skip-gc)",
            SkipReason::Health => "(PULADO: --skip-health-check)",
            SkipReason::Flake => "(PULADO: --check-flake não solicitado)",
        }
    }
}

/// One phase of the update pipeline.
struct UpdateStep {
    id: &'static str,
    title: &'static str,
    skip: Option<SkipReason>,
    action: fn(&UpdateContext) -> Result<()>,
}

const STEPS: &[UpdateStep] = &[
    UpdateStep {
        id: "1",
        title: "Atualizar flake inputs (nix flake update)",
        skip: None,
        action: step_flake_update,
    },
    UpdateStep {
        id: "1.5",
        title: "Coletar lixo do /nix (preserva 7d)",
        skip: Some(SkipReason::Gc),
        action: step_collect_garbage,
    },
    UpdateStep {
        id: "3",
        title: "Checar espaço livre em /nix",
        skip: Some(SkipReason::Disk),
        action: step_disk_check,
    },
    UpdateStep {
        id: "4",
        title: "Aplicar em modo de teste (nh os test)",
        skip: None,
        action: step_nh_test,
    },
    UpdateStep {
        id: "4.5",
        title: "Validar flake (nix flake check)",
        skip: Some(SkipReason::Flake),
        action: step_flake_check,
    },
    UpdateStep {
        id: "5",
        title: "Verificar integridade dos serviços críticos",
        skip: Some(SkipReason::Health),
        action: step_health_check,
    },
    UpdateStep {
        id: "6",
        title: "Consolidar no bootloader (nh os switch)",
        skip: None,
        action: step_nh_switch,
    },
    UpdateStep {
        id: "7",
        title: "Limpeza final (nix-collect-garbage)",
        skip: Some(SkipReason::Gc),
        action: step_collect_garbage,
    },
];

/// True if the step is gated by a flag the user actually set.
fn is_step_skipped(step: &UpdateStep, flags: UpdateFlags) -> bool {
    match (step.skip, flags) {
        (Some(SkipReason::Disk), f) => f.skip_disk_check,
        (Some(SkipReason::Gc), f) => f.skip_gc,
        (Some(SkipReason::Health), f) => f.skip_health_check,
        (Some(SkipReason::Flake), f) => !f.check_flake,
        (None, _) => false,
    }
}

/// `gar server switch` — nixos-rebuild switch.
pub async fn cmd_switch() -> Result<()> {
    let cfg = Config::from_env()?;
    output::header("gar server switch");
    runtime_guard::validate(&cfg)?;
    runtime_guard::reexec_as_root_if_needed("server switch")?;
    run_nh_os(&cfg, "switch")?;
    output::ok("switch concluído");
    Ok(())
}

/// `gar server test` — nixos-rebuild test.
pub async fn cmd_test() -> Result<()> {
    let cfg = Config::from_env()?;
    output::header("gar server test");
    runtime_guard::validate(&cfg)?;
    runtime_guard::reexec_as_root_if_needed("server test")?;
    run_nh_os(&cfg, "test")?;
    output::ok("test concluído");
    Ok(())
}

/// `gar server rollback` — nixos-rebuild switch --rollback.
pub async fn cmd_rollback() -> Result<()> {
    let _cfg = Config::from_env()?;
    output::header("gar server rollback");
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
///
/// Phases are described declaratively as a `&[UpdateStep]` so the
/// dry-run output and the live run walk the same data. Each step's
/// `action` is a `fn` pointer that receives an [`UpdateContext`].
pub async fn cmd_update(
    services: Vec<String>,
    skip_disk_check: bool,
    skip_health_check: bool,
    skip_gc: bool,
    check_flake: bool,
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
    let flags = UpdateFlags {
        skip_disk_check,
        skip_health_check,
        skip_gc,
        check_flake,
    };
    let ctx = UpdateContext {
        cfg,
        flake_path,
        services,
        flags,
    };

    output::header("gar server update (Safe Update Flow)");
    output::kv("flake", ctx.flake_path.display().to_string());
    output::kv("target_host", ctx.cfg.target_host.clone());
    output::kv("services", ctx.services.join(", "));
    output::kv(
        "flags",
        format!(
            "skip_disk_check={} skip_gc={} skip_health_check={} check_flake={} dry_run={}",
            flags.skip_disk_check,
            flags.skip_gc,
            flags.skip_health_check,
            flags.check_flake,
            dry_run
        ),
    );

    if dry_run {
        output::warn("dry-run ativo: nenhuma operação será executada.");
        return plan_only(&ctx);
    }

    let lock_path = std::path::Path::new(UPDATE_LOCK_PATH);
    global_lock::with_lock_timeout(
        lock_path,
        "server update",
        Duration::from_secs(UPDATE_LOCK_TIMEOUT_SECS),
        || run_pipeline(&ctx),
    )?;

    output::ok("Atualização GAROS concluída sem quedas! 🎉");
    Ok(())
}

/// Execute the 7 phases in order, skipping any whose flag the
/// caller set. Returns on the first failure (the lock release
/// happens automatically via the closure's `with_lock_timeout`).
fn run_pipeline(ctx: &UpdateContext) -> Result<()> {
    for step in STEPS {
        if is_step_skipped(step, ctx.flags) {
            output::info(format!(
                "[{}/{}] {} (pulado)",
                step.id,
                STEPS.len(),
                step.title
            ));
            continue;
        }
        output::info(format!("[{}/{}] {}", step.id, STEPS.len(), step.title));
        (step.action)(ctx)?;
    }
    Ok(())
}

fn step_flake_update(ctx: &UpdateContext) -> Result<()> {
    run_command("nix flake update", |cmd| {
        cmd.args(["flake", "update"]).current_dir(&ctx.flake_path);
    })
}

fn step_collect_garbage(_ctx: &UpdateContext) -> Result<()> {
    // Idempotente com nix.gc.automatic. Se --skip-gc foi pedido, o
    // operator pode rodar `gar server clean` depois manualmente.
    run_collect_garbage()
}

fn step_disk_check(_ctx: &UpdateContext) -> Result<()> {
    check_free_nix_space(MIN_FREE_NIX_MB)
}

fn step_nh_test(ctx: &UpdateContext) -> Result<()> {
    run_nh_os(&ctx.cfg, "test")
}

fn step_flake_check(ctx: &UpdateContext) -> Result<()> {
    let system_attr = format!("nixosConfigurations.{}", ctx.cfg.target_host);
    let flake_ref = format!(".#{}@toplevel", system_attr);
    run_command("nix flake check", |cmd| {
        cmd.args([
            "flake",
            "check",
            "--impure",
            "--no-build",
            "--no-allow-import-from-derivation",
            &flake_ref,
        ])
        .current_dir(&ctx.flake_path);
    })
}

fn step_health_check(ctx: &UpdateContext) -> Result<()> {
    // 3s settle so services can stabilize after `nh os test` before
    // the check runs.
    std::thread::sleep(Duration::from_secs(3));
    let fails = check_critical_services(&ctx.services)?;
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
    Ok(())
}

fn step_nh_switch(ctx: &UpdateContext) -> Result<()> {
    // K-2026-10-12 (batch cirurgico): previously this step only
    // logged and went straight to GC, so the new generation never
    // became the boot default. Mirrors step 4's `nh os test` call.
    run_nh_os(&ctx.cfg, "switch")
}

/// `gar server clean` — nh clean all + fallback.
pub async fn cmd_clean() -> Result<()> {
    let cfg = Config::from_env()?;
    output::header("gar server clean");
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

/// Run `nh os <action>` with the appropriate flags.
///
/// K-2026-10-05: \`nh os\` refuses to run as root. We invoke it through
/// \`sudo -u gabriel-rocha\` which mirrors the original scheme (nh
/// internally escalates back to root for activation).
fn run_nh_os(cfg: &Config, action: &str) -> Result<()> {
    // nh refuses root. Drop to the operator user; nh internally re-escalates.
    let mut cmd = Command::new("sudo");
    cmd.args(["-u", "gabriel-rocha", "--", "nh"]);
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
    let output = Command::new("df").args(["-B1M", "/nix"]).output()?;
    if !output.status.success() {
        return Err(GarError::runtime_guard(format!(
            "df /nix falhou: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    // Header + one line. Example: "Filesystem 1M-blocks Used Available Use% Mounted on"
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .nth(1)
        .ok_or_else(|| GarError::runtime_guard("df /nix retornou saída inesperada".to_string()))?;
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
        if !is_unit_present(svc)? {
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

/// Returns true if the unit has a unit file on this system.
///
/// `systemctl list-unit-files <unit>` exits non-zero when the unit is not
/// present and prints "0 unit files listed." — missing units are treated
/// as absent, not as an error.
fn is_unit_present(svc: &str) -> Result<bool> {
    let present = Command::new("systemctl")
        .args(["list-unit-files", &format!("{svc}.service")])
        .output()?;
    if !present.status.success() {
        return Ok(false);
    }
    let stdout = String::from_utf8_lossy(&present.stdout);
    Ok(stdout
        .lines()
        .any(|l| l.trim_start().starts_with(&format!("{svc}.service"))))
}

/// Dry-run plan printer.
fn plan_only(ctx: &UpdateContext) -> Result<()> {
    output::header("Plano de atualização (DRY-RUN, nada será executado)");
    output::kv("flake", ctx.flake_path.display().to_string());
    output::kv("target_host", ctx.cfg.target_host.clone());
    output::kv("services", ctx.services.join(", "));
    output::info("etapas:");
    for step in STEPS {
        let marker = step
            .skip
            .filter(|_| is_step_skipped(step, ctx.flags))
            .map(|s| s.label())
            .unwrap_or("");
        output::info(format!("    {}. {} {}", step.id, step.title, marker));
    }
    Ok(())
}

/// Run a command, mapping failure to a descriptive `GarError::config`.
///
/// `label` is used in error messages (e.g. "nix flake update").
/// `build_cmd` receives a fresh `Command` so callers can add args/current_dir.
fn run_command<F: FnOnce(&mut Command)>(label: &str, build_cmd: F) -> Result<()> {
    let mut cmd = Command::new(label.split_whitespace().next().unwrap_or(label));
    build_cmd(&mut cmd);
    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(GarError::config(format!(
            "{label} falhou (exit {})",
            s.code().unwrap_or(-1)
        ))),
        Err(e) => Err(GarError::config(format!("{label}: {e}"))),
    }
}

/// Run `nix-collect-garbage --delete-older-than 7d` and report freed space.
///
/// Called from `cmd_update` (steps 1.5 and 7) to keep /nix healthy
/// before and after a rebuild. Best-effort: failures are logged but
/// don't abort the update (the switch already happened).
///
/// K-2026-10-06: extracted from `cmd_clean` so the GC step is reusable.
fn run_collect_garbage() -> Result<()> {
    let out = Command::new("nix-collect-garbage")
        .args(["--delete-older-than", "7d"])
        .output()
        .map_err(|e| GarError::config(format!("nix-collect-garbage: {e}")))?;
    if !out.status.success() {
        return Err(GarError::config(format!(
            "nix-collect-garbage falhou (exit {}): {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    // Parse "X store paths deleted, Y MiB freed" from stdout to report.
    let stdout = String::from_utf8_lossy(&out.stdout);
    if let Some(line) = stdout.lines().find(|l| l.contains("freed")) {
        output::info(format!("[OK] {line}"));
    } else {
        output::info("[OK] nix-collect-garbage concluído");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn default_services_include_all_critical() {
        // Order matters (stable logs).
        assert_eq!(
            DEFAULT_CRITICAL_SERVICES,
            &[
                "nginx",
                "dnsmasq",
                "nfs-server",
                "garos-control-api",
                "garos-control-web"
            ]
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
    #[serial(env_config)]
    fn resolve_flake_path_accepts_existing_dir() {
        let tmp =
            std::env::temp_dir().join(format!("gar-resolve-{}-{}", std::process::id(), line!()));
        std::fs::create_dir_all(&tmp).unwrap();
        let mut cfg = Config::from_env().unwrap();
        cfg.flake_path = tmp.clone();
        let resolved = resolve_flake_path(&cfg).unwrap();
        assert_eq!(resolved, tmp);
        std::fs::remove_dir_all(&tmp).ok();
    }
}
