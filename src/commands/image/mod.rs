//! `gar image` subcommand — manages diskless client images.
//!
//! Replaces the legacy `ragc` Bash CLI (commands/{switch,rollback,list,status,gc,doctor}.sh).

use crate::cli::ImageCmd;
use crate::error::Result;

/// Dispatch an `ImageCmd` to its handler.
///
/// K-2026-10-06: extracted match into a separate helper so unit tests
/// can verify dispatch wiring without touching the filesystem or
/// network (no I/O in pure dispatch tests).
pub async fn dispatch(cmd: ImageCmd) -> Result<()> {
    match_image_cmd(cmd).await
}

/// Pure dispatch — no I/O. Maps each `ImageCmd` variant to the
/// corresponding handler. Exposed separately from `dispatch` so tests
/// can verify the wiring without spinning up a Tokio runtime.
pub async fn match_image_cmd(cmd: ImageCmd) -> Result<()> {
    match cmd {
        ImageCmd::Build {
            target,
            channel,
            validate,
            no_validate,
            tmux,
            no_tmux,
            tmux_session,
            tmux_timeout,
        } => {
            build::run(
                target,
                channel,
                build::BuildOptions {
                    validate: !no_validate && validate,
                    tmux: resolve_tmux(tmux, no_tmux),
                    tmux_session,
                    tmux_timeout,
                },
            )
            .await
        }
        ImageCmd::Rollback { target, channel } => rollback::run(target, channel).await,
        ImageCmd::List => list::run().await,
        ImageCmd::Status => status::run().await,
        ImageCmd::Gc { keep } => gc::run(keep).await,
        ImageCmd::Doctor => doctor::run().await,
    }
}

/// Resolve whether tmux orchestration should be used.
///
/// Rules (K-2026-10-06):
/// - `--no-tmux` always wins (off)
/// - `--tmux` always wins (on)
/// - Otherwise: on when stderr is a TTY AND CI=1 is unset AND --json not requested
fn resolve_tmux(tmux_flag: bool, no_tmux_flag: bool) -> bool {
    if no_tmux_flag {
        return false;
    }
    if tmux_flag {
        return true;
    }
    if std::env::var("CI").is_ok() {
        return false;
    }
    if std::env::var("GAR_JSON_OUTPUT").as_deref() == Ok("1") {
        return false;
    }
    // atty::stream(stderr) would be the proper check, but the std lib
    // doesn't expose it. We use a conservative proxy: if stdout is a
    // terminal, we assume interactive. Most CI runs redirect stdout.
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
}

pub mod build;
pub mod doctor;
pub mod gc;
pub mod list;
pub mod rollback;
pub mod status;
