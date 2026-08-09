//! `gar client` subcommand — client diagnostics.

use crate::cli::ClientCmd;
use crate::error::Result;

pub async fn dispatch(cmd: ClientCmd) -> Result<()> {
    match cmd {
        ClientCmd::SessionDoctor => todo!(),
    }
}
