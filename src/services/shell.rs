//! Generic shell process spawning helpers.
//!
//! Use specific service modules (git, nix, btrfs, etc) for known tools.
//! Use these helpers only for ad-hoc commands.

use std::path::Path;

use tokio::process::Command;

use crate::error::{GarError, Result};

/// Run a command and fail if exit code != 0.
pub async fn run_success(program: &str, args: &[&str]) -> Result<String> {
    use indicatif::{ProgressBar, ProgressStyle};
    use std::time::Duration;

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "✓"])
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message(format!("Rodando {}...", program));
    spinner.enable_steady_tick(Duration::from_millis(100));

    let output = Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|e| {
            spinner.finish_and_clear();
            GarError::CommandNotFound(format!("{}: {}", program, e))
        })?;

    spinner.finish_and_clear();

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
    use indicatif::{ProgressBar, ProgressStyle};
    use std::time::Duration;

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "✓"])
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message(format!("Rodando {}...", program));
    spinner.enable_steady_tick(Duration::from_millis(100));

    let output = Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .map_err(|e| {
            spinner.finish_and_clear();
            GarError::CommandNotFound(format!("{}: {}", program, e))
        })?;

    spinner.finish_and_clear();

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
