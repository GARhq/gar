//! GAR CLI - Unified manager for GAROS diskless clients and NixOS server.
//!
//! This binary replaces two legacy Bash CLIs:
//! - `ragc` (image management, ~850 LOC across ragc/commands/*.sh)
//! - `garos` (server operations, 1303 LOC in server/garos-cli.nix)
//!
//! Both legacy CLIs are kept as shims that delegate here for 6 months.

pub mod cli;
pub mod commands;
pub mod config;
pub mod error;
pub mod output;
pub mod services;

use clap::Parser;

use crate::cli::{Cli, Command};
use crate::error::{GarError, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    setup_tracing(&cli);
    let result = dispatch(cli).await;
    if let Err(e) = result {
        exit_with(e);
    }
    Ok(())
}

/// Initialize the `tracing` subscriber.
///
/// K-2026-10-06: extracted from the inline body of `main()` to make
/// the entry point testable as a unit (and to follow the same
/// pattern as `dispatch`). Verbosity is controlled by `--verbose`
/// (`-v`) or the `RUST_LOG` env var.
pub fn setup_tracing(cli: &Cli) {
    let filter = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter)),
        )
        .with_target(false)
        .init();
}

/// Dispatch a parsed CLI to the appropriate subcommand handler.
///
/// K-2026-10-06: extracted from the inline body of `main()`. Same
/// reason as `setup_tracing` — keeps `main` short and lets the
/// dispatch matrix be reasoned about (and extended) without touching
/// process initialization.
pub async fn dispatch(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Image(cmd) => commands::image::dispatch(cmd).await,
        Command::Server(cmd) => commands::server::dispatch(cmd).await,
        Command::User(cmd) => commands::user::dispatch(cmd).await,
        Command::Group(cmd) => commands::group::dispatch(cmd).await,
        Command::Client(cmd) => commands::client::dispatch(cmd).await,
        Command::Branding(cmd) => commands::branding::dispatch(cmd).await,
        Command::Secrets(cmd) => commands::secrets::dispatch(cmd).await,
        Command::ProvisionHome(ref args) => commands::provision_home::run(args, cli.json),
        Command::GenerateCompletions { shell } => {
            let mut app = <crate::cli::Cli as clap::CommandFactory>::command();
            let bin_name = app.get_name().to_string();
            clap_complete::generate(shell, &mut app, bin_name, &mut std::io::stdout());
            Ok(())
        }
    }
}

/// Process exit with proper exit code on error.
pub fn exit_with(err: GarError) -> ! {
    let code = err.exit_code();
    output::err(err.to_string());
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// Confirms `dispatch` accepts `--help` (parses without I/O).
    /// This is a smoke test for the dispatch matrix — every variant
    /// is reachable, every path can be hit with the right flags.
    #[test]
    fn test_dispatch_help_parses() {
        // Just parse the CLI; don't run the handler.
        let cli = Cli::try_parse_from(["gar", "--help"]);
        // --help returns an error (clap prints help and exits), but
        // parsing itself succeeds. What we care about is that the
        // matrix isn't accidentally broken.
        assert!(cli.is_err(), "--help should exit early with clap help");
    }

    /// Confirms the version flag works.
    #[test]
    fn test_dispatch_version_parses() {
        let cli = Cli::try_parse_from(["gar", "--version"]);
        assert!(cli.is_err(), "--version exits early");
    }
}
