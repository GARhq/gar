//! `gar branding` subcommand — branding diagnostics.

use crate::cli::BrandingCmd;
use crate::error::Result;

pub async fn dispatch(cmd: BrandingCmd) -> Result<()> {
    match cmd {
        BrandingCmd::Doctor => todo!(),
    }
}