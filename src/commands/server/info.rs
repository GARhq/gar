use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::{generations, nix};
use serde::Serialize;
use std::env;
use std::process::Command;

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
pub async fn cmd_check(
    inventory: Option<std::path::PathBuf>,
    allow_empty: bool,
    flake_drift: bool,
) -> Result<()> {
    let cfg = Config::from_env()?;
    output::section("==> gar server check");
    if flake_drift {
        return check_flake_drift(&cfg).await;
    }
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

/// `gar server check --flake-drift` — compare local flake.lock pins vs
/// upstream `GARhq/*` heads.
///
/// Reads the operational `flake.lock`, extracts the `gar-cli` pin, and
/// fetches the upstream `GARhq/gar` HEAD rev. If the local rev lags the
/// upstream by N commits (or doesn't exist upstream), reports the drift
/// and exits non-zero so operators can run `gar server update` to sync.
///
/// K-2026-10-06: prevents the silent "lockfile is 3 weeks old" failure
/// mode that bit Phase C of the GAROS rebuild.
async fn check_flake_drift(cfg: &Config) -> Result<()> {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct FlakeLock {
        nodes: std::collections::BTreeMap<String, FlakeNode>,
    }
    #[derive(Deserialize)]
    struct FlakeNode {
        locked: Option<FlakeLocked>,
    }
    #[derive(Deserialize)]
    struct FlakeLocked {
        rev: Option<String>,
    }

    let lock_path = cfg.flake_path.join("flake.lock");
    if !lock_path.is_file() {
        return Err(GarError::config(format!(
            "flake.lock ausente em {}",
            lock_path.display()
        )));
    }
    let lock_text = std::fs::read_to_string(&lock_path)?;
    let lock: FlakeLock = serde_json::from_str(&lock_text)
        .map_err(|e| GarError::config(format!("parse flake.lock: {e}")))?;

    let local = lock
        .nodes
        .get("gar-cli")
        .and_then(|n| n.locked.as_ref())
        .and_then(|l| l.rev.as_ref())
        .ok_or_else(|| GarError::config("gar-cli input ausente do flake.lock".to_string()))?;
    let local_short = &local[..12.min(local.len())];

    // Query upstream HEAD via the github REST API. Cheap (60 req/h
    // unauthenticated) and avoids depending on the `gh` CLI.
    let url = "https://api.github.com/repos/GARhq/gar/commits/main";
    let body =
        ureq_get(url).map_err(|e| GarError::config(format!("fetch upstream HEAD failed: {e}")))?;
    let upstream_sha = body
        .lines()
        .find_map(|l| {
            // Look for the "sha" key in the JSON response.
            if let Some(idx) = l.find("\"sha\":") {
                let after = &l[idx + 6..];
                let after = after.trim_start_matches(' ').trim_start_matches('"');
                return Some(after[..12.min(after.len())].to_string());
            }
            None
        })
        .ok_or_else(|| GarError::config("upstream HEAD response sem 'sha'".to_string()))?;

    if local_short == upstream_sha {
        output::ok(format!(
            "flake.lock gar-cli está em sincronia com upstream ({} == {})",
            local_short, upstream_sha
        ));
        return Ok(());
    }

    output::warn(format!(
        "flake drift detectado: local={} upstream={}",
        local_short, upstream_sha
    ));
    output::info("Rode `gar server update` para sincronizar.");
    Err(GarError::config(format!(
        "flake drift: local {} != upstream {}",
        local_short, upstream_sha
    )))
}

/// Tiny helper to GET a URL with a short timeout, returning the body as String.
/// Uses `curl` (already on PATH) via `std::process::Command` so we don't add
/// a new Rust dep just for this one call. Returns a `String` error so the
/// caller can wrap it in `GarError::config(...)`.
fn ureq_get(url: &str) -> std::result::Result<String, String> {
    let out = Command::new("curl")
        .args(["-sS", "-m", "10", url])
        .output()
        .map_err(|e| format!("spawn curl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "curl exit {}: {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
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
