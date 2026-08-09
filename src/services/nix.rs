//! Nix operations (flake update, check, repl).

use std::path::Path;

use crate::error::Result;

/// Run `nix flake update` in the given directory.
pub async fn flake_update(flake_dir: &Path) -> Result<()> {
    let _ = crate::services::shell::run_success_in_dir(flake_dir, "nix", &["flake", "update"]).await?;
    Ok(())
}

/// Run `nix flake check` in the given directory.
pub async fn flake_check(flake_dir: &Path) -> Result<()> {
    let _ = crate::services::shell::run_success_in_dir(flake_dir, "nix", &["flake", "check"]).await?;
    Ok(())
}

/// Spawn `nix repl` on the local flake (interactive).
pub async fn flake_repl(flake_dir: &Path) -> Result<()> {
    crate::services::shell::exec_in_dir(flake_dir, "nix", &["repl"]).await
}