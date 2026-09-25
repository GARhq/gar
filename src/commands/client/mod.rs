//! `gar client` subcommand — client diagnostics.

pub mod diagnostics;
pub mod inventory;
pub mod wol;

use crate::cli::ClientCmd;
use crate::error::Result;

pub async fn dispatch(cmd: ClientCmd) -> Result<()> {
    match cmd {
        ClientCmd::SessionDoctor => diagnostics::cmd_session_doctor().await,
        ClientCmd::List { json } => inventory::cmd_list(json).await,
        ClientCmd::Wake {
            mac,
            port,
            count,
            broadcast,
            json,
        } => wol::cmd_wake(&mac, port, count, broadcast.as_deref(), json).await,
    }
}
