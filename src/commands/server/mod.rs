//! `gar server` subcommand — manages NixOS server (srv-gar).
//!
//! Replaces `garos` top-level commands (server/garos-cli.nix lines 494-567).
//! Commands: sync, switch, test, rollback, update, clean, check, repl, path, enter, status.

pub mod info;
pub mod lifecycle;
pub mod sync;

use crate::cli::ServerCmd;
use crate::error::Result;

/// Dispatch a ServerCmd to its handler.
pub async fn dispatch(cmd: ServerCmd) -> Result<()> {
    match cmd {
        ServerCmd::Sync => sync::cmd_sync().await,
        ServerCmd::Switch => lifecycle::cmd_switch().await,
        ServerCmd::Test => lifecycle::cmd_test().await,
        ServerCmd::Rollback => lifecycle::cmd_rollback().await,
        ServerCmd::Update => lifecycle::cmd_update().await,
        ServerCmd::Clean => lifecycle::cmd_clean().await,
        ServerCmd::Check {
            inventory,
            allow_empty,
        } => info::cmd_check(inventory, allow_empty).await,
        ServerCmd::Repl => info::cmd_repl(),
        ServerCmd::Path => info::cmd_path(),
        ServerCmd::Enter => info::cmd_enter(),
        ServerCmd::Status => info::cmd_status().await,
    }
}
