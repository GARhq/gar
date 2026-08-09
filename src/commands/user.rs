//! `gar user` subcommand — manages users.

use crate::cli::UserCmd;
use crate::error::Result;

pub async fn dispatch(cmd: UserCmd) -> Result<()> {
    match cmd {
        UserCmd::Add { .. } => todo!(),
        UserCmd::Resize { .. } => todo!(),
        UserCmd::List => todo!(),
        UserCmd::Delete { .. } => todo!(),
        UserCmd::Doctor { .. } => todo!(),
        UserCmd::QuotaSync => todo!(),
        UserCmd::Activity { .. } => todo!(),
    }
}