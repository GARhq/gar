//! `gar user` subcommand — manages users.

pub mod core;
pub mod info;
pub mod quota;

use crate::cli::UserCmd;
use crate::error::Result;

pub async fn dispatch(cmd: UserCmd) -> Result<()> {
    match cmd {
        UserCmd::Add {
            username,
            quota,
            password,
            password_hash,
            group,
        } => {
            core::cmd_add(
                &username,
                quota.as_deref(),
                password.as_deref(),
                password_hash.as_deref(),
                group.as_deref(),
            )
            .await
        }
        UserCmd::List => core::cmd_list().await,
        UserCmd::Delete { username, archive } => core::cmd_delete(&username, archive).await,
        UserCmd::Resize {
            username,
            quota,
            force,
        } => quota::cmd_resize(&username, &quota, force).await,
        UserCmd::QuotaSync => quota::cmd_quota_sync().await,
        UserCmd::Doctor { username } => info::cmd_doctor(&username).await,
        UserCmd::Activity { username } => info::cmd_activity(&username).await,
    }
}
