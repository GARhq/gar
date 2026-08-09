//! Generic shell process spawning helpers.
//!
//! Use specific service modules (git, nix, btrfs, etc) for known tools.
//! Use these helpers only for ad-hoc commands.

use std::path::Path;

use tokio::process::Command;

use crate::error::{GarError, Result};

/// Run a command and fail if exit code != 0.
pub async fn run_success(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|e| GarError::CommandNotFound(format!("{}: {}", program, e)))?;

    if !output.status.success() {
        return Err(GarError::CommandFailed {
            program: program.into(),
            args: args
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(" "),
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into())
}

/// Run a command in a specific working directory.
pub async fn run_success_in_dir(dir: &Path, program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .map_err(|e| GarError::CommandNotFound(format!("{}: {}", program, e)))?;

    if !output.status.success() {
        return Err(GarError::CommandFailed {
            program: program.into(),
            args: args
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(" "),
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into())
}

/// Replace current process with the given command (shell-out / exec).
pub async fn exec_in_dir(dir: &Path, program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .await
        .map_err(|e| GarError::CommandNotFound(format!("{}: {}", program, e)))?;

    std::process::exit(status.code().unwrap_or(1));
}
