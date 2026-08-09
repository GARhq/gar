//! `gar server` subcommand — manages NixOS server (srv-gar).

use crate::cli::ServerCmd;
use crate::error::Result;

pub async fn dispatch(cmd: ServerCmd) -> Result<()> {
    match cmd {
        ServerCmd::Sync => todo!(),
        ServerCmd::Switch => todo!(),
        ServerCmd::Test => todo!(),
        ServerCmd::Rollback => todo!(),
        ServerCmd::Update => todo!(),
        ServerCmd::Clean => todo!(),
        ServerCmd::Check => todo!(),
        ServerCmd::Repl => todo!(),
        ServerCmd::Path => todo!(),
        ServerCmd::Enter => todo!(),
        ServerCmd::Status => todo!(),
    }
}