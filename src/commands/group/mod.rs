//! `gar group` subcommand — manages groups.

pub mod core;
pub mod defaults;
pub mod members;
pub mod permissions;

use crate::cli::GroupCmd;
use crate::error::Result;

pub async fn dispatch(cmd: GroupCmd) -> Result<()> {
    match cmd {
        GroupCmd::Add {
            groupname,
            description,
            storage_quota,
        } => core::cmd_add(&groupname, description.as_deref(), storage_quota.as_deref()).await,
        GroupCmd::List => core::cmd_list().await,
        GroupCmd::Delete { groupname, archive } => core::cmd_delete(&groupname, archive).await,
        GroupCmd::Chmod { groupname, perms } => permissions::cmd_chmod(&groupname, &perms).await,
        GroupCmd::Members {
            groupname,
            add,
            remove,
        } => members::cmd_members(&groupname, add.as_deref(), remove.as_deref()).await,
        GroupCmd::Permissions { groupname } => permissions::cmd_permissions(&groupname).await,
        GroupCmd::EnsureDefaults => defaults::cmd_ensure_defaults().await,
    }
}
