//! `gar image` subcommand — manages diskless client images.
//!
//! Replaces the legacy `ragc` Bash CLI (commands/{switch,rollback,list,status,gc,doctor}.sh).

use crate::cli::ImageCmd;
use crate::error::Result;

/// Dispatch an `ImageCmd` to its handler.
pub async fn dispatch(cmd: ImageCmd) -> Result<()> {
    match cmd {
        ImageCmd::Build { target, channel } => build::run(target, channel).await,
        ImageCmd::Rollback { target, channel } => rollback::run(target, channel).await,
        ImageCmd::List => list::run().await,
        ImageCmd::Status => status::run().await,
        ImageCmd::Gc { keep } => gc::run(keep).await,
        ImageCmd::Doctor => doctor::run().await,
    }
}

pub mod build;
pub mod rollback;
pub mod list;
pub mod status;
pub mod gc;
pub mod doctor;