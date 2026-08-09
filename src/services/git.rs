//! Git operations (fetch, pull, submodule).

use std::path::Path;

use crate::error::Result;

/// Fetch all remotes + prune + fast-forward pull + submodule sync/update.
pub async fn sync_full(repo: &Path) -> Result<()> {
    let _ = crate::services::shell::run_success(
        "git",
        &["-C", &repo.display().to_string(), "fetch", "--all", "--prune"],
    )
    .await?;
    let _ = crate::services::shell::run_success(
        "git",
        &["-C", &repo.display().to_string(), "pull", "--ff-only"],
    )
    .await?;

    // Submodules (only if .gitmodules exists)
    let gitmodules = repo.join(".gitmodules");
    if gitmodules.exists() {
        let _ = crate::services::shell::run_success(
            "git",
            &[
                "-C",
                &repo.display().to_string(),
                "submodule",
                "sync",
                "--recursive",
            ],
        )
        .await?;
        let _ = crate::services::shell::run_success(
            "git",
            &[
                "-C",
                &repo.display().to_string(),
                "submodule",
                "update",
                "--init",
                "--recursive",
            ],
        )
        .await?;
    }
    Ok(())
}