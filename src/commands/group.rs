//! `gar group` subcommand — manages groups.

use crate::cli::GroupCmd;
use crate::error::Result;

pub async fn dispatch(cmd: GroupCmd) -> Result<()> {
    match cmd {
        GroupCmd::Add { .. } => todo!(),
        GroupCmd::List => todo!(),
        GroupCmd::Delete { .. } => todo!(),
        GroupCmd::Chmod { .. } => todo!(),
        GroupCmd::Members { .. } => todo!(),
        GroupCmd::Permissions { .. } => todo!(),
        GroupCmd::EnsureDefaults => todo!(),
    }
}
