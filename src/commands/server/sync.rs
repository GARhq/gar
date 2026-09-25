use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::{git, runtime_guard};

/// `gar server sync` — fetch + pull + submodule update.
pub async fn cmd_sync() -> Result<()> {
    let cfg = Config::from_env()?;
    output::section("==> gar server sync");
    runtime_guard::reexec_as_root_if_needed("server sync")?;
    if !cfg.flake_path.is_dir() {
        return Err(GarError::config(format!(
            "flake local ausente em {}",
            cfg.flake_path.display()
        )));
    }
    git::sync_full(&cfg.flake_path).await?;
    output::ok(format!("sync concluído em {}", cfg.flake_path.display()));
    Ok(())
}
