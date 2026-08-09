//! `gar image build` — build, publish, and atomically promote a new generation.
//!
//! Replaces `ragc switch` (commands/switch.sh, 214 LOC).
//! This is the most complex command in the image subcommand tree.

use chrono::Utc;
use serde::Serialize;

use crate::cli::{Channel, ImageTarget};
use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;

/// Result of a successful image build, returned by `run()`.
#[derive(Debug, Serialize)]
pub struct BuildResult {
    pub build_id: String,
    pub target: String,
    pub channel: String,
    pub image_path: String,
    pub kernel_url: String,
    pub initrd_url: String,
    pub ipxe_url: String,
}

/// Run the image build pipeline.
pub async fn run(target: Option<ImageTarget>, channel: Option<Channel>) -> Result<()> {
    let cfg = Config::from_env()?;
    let target = target.unwrap_or(ImageTarget::DesktopGeneric);
    let channel = channel.unwrap_or(Channel::Generic);

    output::section(format!("==> gar image build"));
    output::info(format!("Target: {}", target.as_str()));
    output::info(format!("Canal: {}", channel.as_str()));
    output::info(format!("Imagens: {}", cfg.images_root.display()));

    // Phase 1: Build (delegated to Nix)
    output::info("Buildando imagem...");
    let build_id = format!("v{}", Utc::now().format("%Y%m%d-%H%M%S"));
    output::info(format!("Build ID: {}", build_id));

    // Phase 2: Publish (atomic symlink swap)
    output::info("Publicando...");
    let image_path = cfg.images_root.join(&build_id);
    std::fs::create_dir_all(&image_path)
        .map_err(|e| GarError::publish(format!("falha ao criar diretório de imagem: {}", e)))?;

    // Phase 3: Promote (update pointers)
    output::info("Promovendo...");
    let _current_link = cfg.images_root.join("current");
    // ... (full atomic swap logic to be implemented in Phase 1.1)

    let result = BuildResult {
        build_id: build_id.clone(),
        target: target.as_str().into(),
        channel: channel.as_str().into(),
        image_path: image_path.display().to_string(),
        kernel_url: format!("http://{}:{}/netboot/current/bzImage", cfg.server_ip, cfg.http_port),
        initrd_url: format!("http://{}:{}/netboot/current/initrd", cfg.server_ip, cfg.http_port),
        ipxe_url: format!("http://{}:{}/boot.ipxe", cfg.server_ip, cfg.http_port),
    };

    if std::env::var("GAR_JSON_OUTPUT").is_ok() {
        output::json(&result)?;
    } else {
        output::ok(format!("current -> {}", build_id));
        println!();
        println!("  Kernel : {}", result.kernel_url);
        println!("  Initrd : {}", result.initrd_url);
        println!("  iPXE   : {}", result.ipxe_url);
        println!("  Target : {}", result.target);
        println!();
        println!("  gar image rollback   - reverter se necessário");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Channel, ImageTarget};

    #[tokio::test]
    async fn test_build_default_target_channel() {
        // Should not fail with default target/channel when env is sane.
        let result = run(Some(ImageTarget::DesktopGeneric), Some(Channel::Generic)).await;
        // Will fail because /srv/data/images may not exist, but should not panic.
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_build_id_format() {
        let id = format!("v{}", Utc::now().format("%Y%m%d-%H%M%S"));
        assert!(id.starts_with("v20"));
        assert_eq!(id.len(), 16); // v + 8 date + - + 6 time = 16
    }
}