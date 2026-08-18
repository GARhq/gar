//! Nix operations (flake update, check, repl).

use std::path::Path;

use crate::error::Result;

/// Run `nix flake update` in the given directory.
pub async fn flake_update(flake_dir: &Path) -> Result<()> {
    let _ =
        crate::services::shell::run_success_in_dir(flake_dir, "nix", &["flake", "update"]).await?;
    Ok(())
}

/// Run `nix flake check` in the given directory.
pub async fn flake_check(flake_dir: &Path) -> Result<()> {
    let _ =
        crate::services::shell::run_success_in_dir(flake_dir, "nix", &["flake", "check"]).await?;
    Ok(())
}

/// Spawn `nix repl` on the local flake (interactive).
pub async fn flake_repl(flake_dir: &Path) -> Result<()> {
    crate::services::shell::exec_in_dir(flake_dir, "nix", &["repl"]).await
}

/// Validate clients inventory using nix-instantiate.
pub async fn validate_inventory(
    flake_dir: &Path,
    inventory_path: &Path,
    allow_empty: bool,
) -> Result<Vec<String>> {
    let lib_path = flake_dir.join("server/network/clients-inventory-lib.nix");
    if !lib_path.exists() {
        return Err(crate::error::GarError::config(format!(
            "clients-inventory-lib.nix não encontrado em {}",
            lib_path.display()
        )));
    }

    let require_non_empty = if allow_empty { "false" } else { "true" };

    // Format the Nix expression
    let expr = format!(
        r#"
let
  lib = import <nixpkgs/lib>;
  inventoryLib = import {} {{ inherit lib; }};
  validated = inventoryLib.validateInventoryWithPolicy {{
    inventory = import {};
    requireNonEmpty = {};
  }};
in
builtins.toJSON (
  builtins.map
    (assertion: assertion.message)
    (builtins.filter (assertion: !assertion.assertion) validated.assertions)
)
"#,
        lib_path.to_string_lossy(),
        inventory_path.to_string_lossy(),
        require_non_empty
    );

    let output = tokio::process::Command::new("nix-instantiate")
        .args(&["--eval", "--strict", "--raw", "--expr", &expr])
        .output()
        .await
        .map_err(|e| crate::error::GarError::CommandNotFound(format!("nix-instantiate: {}", e)))?;

    if !output.status.success() {
        return Err(crate::error::GarError::CommandFailed {
            program: "nix-instantiate".into(),
            args: "--eval --strict --raw --expr ...".into(),
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into(),
        });
    }

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let errors: Vec<String> = serde_json::from_str(&stdout_str)
        .map_err(|e| crate::error::GarError::config(format!("Falha ao parsear JSON do nix-instantiate: {}", e)))?;

    Ok(errors)
}
