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
use crate::error::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Init tracing
    let filter = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter)),
        )
        .with_target(false)
        .init();

    // Dispatch
    let result = match cli.command {
        Command::Image(cmd) => commands::image::dispatch(cmd).await,
        Command::Server(cmd) => commands::server::dispatch(cmd).await,
        Command::User(cmd) => commands::user::dispatch(cmd).await,
        Command::Group(cmd) => commands::group::dispatch(cmd).await,
        Command::Client(cmd) => commands::client::dispatch(cmd).await,
        Command::Branding(cmd) => commands::branding::dispatch(cmd).await,
        Command::Secrets(cmd) => commands::secrets::dispatch(cmd).await,
        Command::ProvisionHome(ref args) => {
            commands::provision_home::run(args, cli.json).map_err(|e| e.into())
        }
        Command::GenerateCompletions { shell } => {
            let mut app = <crate::cli::Cli as clap::CommandFactory>::command();
            let bin_name = app.get_name().to_string();
            clap_complete::generate(shell, &mut app, bin_name, &mut std::io::stdout());
            Ok(())
        }
    };
    if let Err(e) = result {
        exit_with(e);
    }
    Ok(())
}

/// Process exit with proper exit code on error.
pub fn exit_with(err: error::GarError) -> ! {
    let code = err.exit_code();
    output::err(&err.to_string());
    std::process::exit(code);
}
