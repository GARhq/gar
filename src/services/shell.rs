//! Generic shell process spawning helpers.
//!
//! Use specific service modules (git, nix, btrfs, etc) for known tools.
//! Use these helpers only for ad-hoc commands.

use std::path::Path;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use tokio::process::Command;

use crate::error::{GarError, Result};

const SPINNER_TICKS: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "✓"];

/// Build the standard "Rodando X..." spinner shared by `run_success*`.
fn new_spinner(program: &str) -> ProgressBar {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(SPINNER_TICKS)
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message(format!("Rodando {}...", program));
    spinner.enable_steady_tick(Duration::from_millis(100));
    spinner
}

/// Build a `CommandFailed` error from a finished output.
fn failure(program: &str, args: &[&str], code: Option<i32>, stderr: Vec<u8>) -> GarError {
    GarError::CommandFailed {
        program: program.into(),
        args: args
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(" "),
        code: code.unwrap_or(-1),
        stderr: String::from_utf8_lossy(&stderr).into(),
    }
}

/// Inner run: spawns the command, surfaces failures uniformly. The
/// optional `cwd` distinguishes the directory-scoped helper from the
/// plain one without duplicating the rest of the logic.
async fn run_inner(program: &str, args: &[&str], cwd: Option<&Path>) -> Result<String> {
    let spinner = new_spinner(program);

    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let output_result = cmd.output().await;
    spinner.finish_and_clear();
    let output =
        output_result.map_err(|e| GarError::CommandNotFound(format!("{}: {}", program, e)))?;

    if !output.status.success() {
        return Err(failure(program, args, output.status.code(), output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into())
}

/// Run a command and fail if exit code != 0.
pub async fn run_success(program: &str, args: &[&str]) -> Result<String> {
    run_inner(program, args, None).await
}

/// Run a command in a specific working directory.
pub async fn run_success_in_dir(dir: &Path, program: &str, args: &[&str]) -> Result<String> {
    run_inner(program, args, Some(dir)).await
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
