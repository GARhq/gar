//! NixOS rebuild wrappers (switch/test/rollback).

use crate::error::Result;

/// Run `nixos-rebuild switch` with the given installable.
pub async fn switch(installable: &str) -> Result<()> {
    let _ = crate::services::shell::run_success(
        "nixos-rebuild",
        &["switch", "--impure", "--flake", installable],
    )
    .await?;
    Ok(())
}

/// Run `nixos-rebuild test` with the given installable.
pub async fn test(installable: &str) -> Result<()> {
    let _ = crate::services::shell::run_success(
        "nixos-rebuild",
        &["test", "--impure", "--flake", installable],
    )
    .await?;
    Ok(())
}

/// Run `nixos-rebuild switch --rollback`.
pub async fn rollback() -> Result<()> {
    let _ = crate::services::shell::run_success("nixos-rebuild", &["switch", "--rollback"]).await?;
    Ok(())
}
