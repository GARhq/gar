use chrono::Utc;
use serde::Serialize;

use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::filesystem::FsOps;
use crate::services::user_system::{self, HomeMeta};

#[derive(Debug, Serialize)]
struct ResizeResult {
    username: String,
    home: String,
    old_quota: String,
    new_quota: String,
    usage_bytes: u64,
}

pub async fn cmd_resize(username: &str, quota: &str, force: bool) -> Result<()> {
    let cfg = Config::from_env()?;
    let home = cfg.home_base.join(username);
    if !home.exists() {
        return Err(GarError::user(format!(
            "home ausente para {}: {}",
            username,
            home.display()
        )));
    }
    let quota_b = super::core::human_to_bytes_check(quota)?;
    let usage_b = user_system::dir_size_bytes(&home);
    let current = user_system::read_meta_value(&home, "QUOTA").unwrap_or_default();

    if usage_b > quota_b && !force {
        return Err(GarError::user(format!(
            "nova quota {} e menor que o uso atual {}; use --force se quiser prosseguir",
            quota,
            user_system::bytes_to_human(usage_b)
        )));
    }

    let ops = FsOps::for_path(&cfg.home_base)?;
    ops.enable_quotas(&cfg.home_base).await?;
    ops.set_quota(&home, quota).await?;

    HomeMeta::write(
        &home,
        &HomeMeta {
            user: username.into(),
            home: home.display().to_string(),
            quota: quota.into(),
            updated_at: Utc::now().to_rfc3339(),
        },
    )?;

    if cfg.json_output {
        output::json(&ResizeResult {
            username: username.into(),
            home: home.display().to_string(),
            old_quota: current,
            new_quota: quota.into(),
            usage_bytes: usage_b,
        })?;
    } else {
        output::ok(format!("usuário redimensionado: {}", username));
        println!("  uso atual:     {}", user_system::bytes_to_human(usage_b));
        println!(
            "  quota anterior: {}",
            if current.is_empty() { "—" } else { &current }
        );
        println!("  nova quota:    {}", quota);
    }
    Ok(())
}

pub async fn cmd_quota_sync() -> Result<()> {
    let cfg = Config::from_env()?;
    if !cfg.home_base.exists() {
        output::warn("storage de homes ausente");
        return Ok(());
    }
    if !user_system::is_mountpoint(&cfg.home_base) {
        return Err(GarError::user("home nao esta montada"));
    }
    let ops = FsOps::for_path(&cfg.home_base)?;
    ops.enable_quotas(&cfg.home_base).await?;

    let mut synced = 0;
    let mut skipped = 0;
    for entry in std::fs::read_dir(&cfg.home_base)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let Some(quota) = user_system::read_meta_value(&path, "QUOTA").filter(|q| !q.is_empty())
        else {
            skipped += 1;
            continue;
        };
        match ops.set_quota(&path, &quota).await {
            Ok(()) => synced += 1,
            Err(e) => output::warn(format!("falha ao sincronizar {}: {}", name, e)),
        }
    }
    output::ok(format!(
        "quotas sincronizadas ({} sincronizadas, {} sem metadata)",
        synced, skipped
    ));
    Ok(())
}
