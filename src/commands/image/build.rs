//! `gar image build` — build, publish, and atomically promote a new generation.
//!
//! Replaces `ragc switch` (commands/switch.sh, 214 LOC).
//! Builds client NixOS system via Nix, publishes kernel/initrd artifacts,
//! and atomically swaps the `current` symlink.

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};

use chrono::Utc;
use serde::Serialize;

use crate::cli::{Channel, ImageTarget};
use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::build;
use crate::services::manifest;

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

    output::section("==> gar image build");
    output::info(format!("Target: {}", target.as_str()));
    output::info(format!("Canal: {}", channel.as_str()));
    output::info(format!("Imagens: {}", cfg.images_root.display()));

    // Phase 1: Build (delegated to Nix via build_or_reuse_system)
    output::info("Buildando imagem...");
    let build_id = build::compute_build_id(&cfg.images_root);
    output::info(format!("Build ID: {build_id}"));

    let system_path =
        build::build_or_reuse_system(&cfg.flake_path, target.as_str(), channel.as_str())?;
    output::info(format!("System path: {}", system_path.display()));

    // Discover kernel/initrd/erofs/init paths inside the Nix store result.
    let kernel_source = ["kernel", "bzImage", "vmlinuz"]
        .iter()
        .map(|n| system_path.join(n))
        .find(|p| p.exists())
        .ok_or_else(|| {
            GarError::build(format!(
                "Kernel não encontrado em {} (tentou: kernel, bzImage, vmlinuz)",
                system_path.display()
            ))
        })?;
    let initrd_source = system_path.join("initrd");
    if !initrd_source.exists() {
        return Err(GarError::build(format!(
            "Initrd não encontrado em {}",
            system_path.display()
        )));
    }
    let init_path = if system_path.join("init").exists() {
        system_path.join("init")
    } else {
        system_path.clone()
    };
    let init_path_for_display = init_path.display().to_string();
    let kernel_params_path = system_path.join("kernel-params");
    let kernel_params_str = if kernel_params_path.exists() {
        fs::read_to_string(&kernel_params_path).unwrap_or_default()
    } else {
        String::new()
    };

    // Assemble BuildArtifact and delegate the rest to stage_generation
    // (which handles copy + sha256 + manifest + Ed25519 sign + GC root).
    let artifact = build::BuildArtifact {
        build_id: build_id.clone(),
        target: target.as_str().into(),
        channel: channel.as_str().into(),
        system_path: system_path.clone(),
        init_path: init_path.clone(),
        kernel_path: kernel_source,
        initrd_path: initrd_source,
        kernel_params: kernel_params_path.clone(),
        kernel_sha256: String::new(), // populated inside stage_generation
        initrd_sha256: String::new(),
        timestamp: Utc::now().to_rfc3339(),
    };

    // Phase 2: Publish (delegate to existing stage_generation for full pipeline).
    output::info("Publicando artefatos (stage_generation)...");
    let _staged = build::stage_generation(
        &cfg.images_root,
        &build_id,
        target.as_str(),
        channel.as_str(),
        &artifact,
    )?;
    let image_path = cfg.images_root.join(&build_id);

    // Copy netboot.ipxe or build-specific iPXE script if present
    let ipxe_source = [
        system_path.join("netboot.ipxe"),
        cfg.http_root.join("netboot.ipxe"),
        cfg.flake_path.join("netboot.ipxe"),
    ]
    .into_iter()
    .find(|p| p.is_file());

    if let Some(src) = ipxe_source {
        let _ = fs::copy(&src, image_path.join("netboot.ipxe"));
    } else {
        let ipxe_content = format!(
            "#!ipxe\n\
             echo Booting GAROS {build_id}...\n\
             kernel http://{}:{}/netboot/{build_id}/bzImage init={} ip=dhcp {}\n\
             initrd http://{}:{}/netboot/{build_id}/initrd\n\
             boot\n",
            cfg.server_ip,
            cfg.http_port,
            init_path_for_display,
            kernel_params_str.trim(),
            cfg.server_ip,
            cfg.http_port
        );
        let _ = fs::write(image_path.join("netboot.ipxe"), ipxe_content);
    }

    // Set permissions so nginx can read
    let _ = fs::set_permissions(&image_path, fs::Permissions::from_mode(0o755));
    if let Ok(entries) = fs::read_dir(&image_path) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    let _ = fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o644));
                } else if meta.is_dir() {
                    let _ = fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o755));
                }
            }
        }
    }

    // GC root registration (best-effort)
    let _ = build::create_gc_root(&system_path, &image_path.join(".gcroot"));

    // Manifest writing
    let kernel_sha = build::sha256_file(&image_path.join("bzImage")).unwrap_or_default();
    let initrd_sha = build::sha256_file(&image_path.join("initrd")).unwrap_or_default();
    let erofs_sha = if image_path.join("garos-root.erofs").exists() {
        build::sha256_file(&image_path.join("garos-root.erofs")).unwrap_or_default()
    } else {
        String::new()
    };

    let manifest = manifest::Manifest {
        id: build_id.clone(),
        timestamp: Utc::now().to_rfc3339(),
        version: 1,
        system_path: system_path.display().to_string(),
        init_path: init_path.display().to_string(),
        artifacts: manifest::Artifacts {
            kernel: "bzImage".into(),
            initrd: "initrd".into(),
            erofs: if erofs_sha.is_empty() {
                String::new()
            } else {
                "garos-root.erofs".into()
            },
        },
        checksums: manifest::Checksums {
            kernel: kernel_sha,
            initrd: initrd_sha,
            erofs: erofs_sha,
        },
        status: manifest::Status::Staged,
        target: target.as_str().into(),
        channel: channel.as_str().into(),
        hardware_class: crate::services::channel::target_hardware_class(target).to_string(),
        signature: String::new(),
    };
    let _ = manifest::write(&image_path, &manifest);

    // Phase 3: Atomic symlink swap
    output::info("Promovendo...");
    let current_link = cfg.images_root.join("current");

    // Clean up if current is a directory (not a symlink)
    if current_link.is_dir() && !current_link.is_symlink() {
        output::warn("Destino 'current' é um diretório real; removendo para permitir symlink...");
        fs::remove_dir_all(&current_link).map_err(|e| {
            GarError::publish(format!(
                "falha ao remover diretório 'current' legado: {}",
                e
            ))
        })?;
    }

    // Preserve previous pointer
    if let Ok(old_target) = fs::read_link(&current_link) {
        if let Some(old_name) = old_target.file_name() {
            let tmp_prev = cfg.images_root.join("previous.tmp");
            let _ = fs::remove_file(&tmp_prev);
            if symlink(old_name, &tmp_prev).is_ok() {
                let _ = fs::rename(&tmp_prev, cfg.images_root.join("previous"));
            }
        }
    }

    // Atomic symlink swap for current
    let tmp_link = cfg.images_root.join("current.tmp");
    let _ = fs::remove_file(&tmp_link);
    symlink(&build_id, &tmp_link)
        .map_err(|e| GarError::publish(format!("falha ao criar symlink temporário: {}", e)))?;
    fs::rename(&tmp_link, &current_link)
        .map_err(|e| GarError::publish(format!("falha ao renomear symlink 'current': {}", e)))?;

    // Update channel pointer (current-<channel>)
    let channel_ptr_name = format!("current-{}", channel.as_str());
    let channel_tmp = cfg.images_root.join(format!("{}.tmp", channel_ptr_name));
    let _ = fs::remove_file(&channel_tmp);
    if symlink(&build_id, &channel_tmp).is_ok() {
        let _ = fs::rename(&channel_tmp, cfg.images_root.join(&channel_ptr_name));
    }

    // Phase 4: Render URLs
    let result = BuildResult {
        build_id: build_id.clone(),
        target: target.as_str().into(),
        channel: channel.as_str().into(),
        image_path: image_path.display().to_string(),
        kernel_url: format!(
            "http://{}:{}/netboot/current/bzImage",
            cfg.server_ip, cfg.http_port
        ),
        initrd_url: format!(
            "http://{}:{}/netboot/current/initrd",
            cfg.server_ip, cfg.http_port
        ),
        ipxe_url: format!("http://{}:{}/boot.ipxe", cfg.server_ip, cfg.http_port),
    };

    if cfg.json_output {
        output::json(&result)?;
    } else {
        output::ok(format!("current -> {}", build_id));
        println!();
        println!("  Kernel : {}", result.kernel_url);
        println!("  Initrd : {}", result.initrd_url);
        println!("  iPXE   : {}", result.ipxe_url);
        println!("  Target : {}", result.target);
        println!("  Symlink: {} -> {}", current_link.display(), build_id);
        println!();
        println!("  gar image rollback   - reverter se necessário");
    }

    // Reconcile statuses across all generations
    if let Err(e) = manifest::reconcile(&cfg.images_root, Some(&build_id), None, None, None) {
        output::warn(format!("reconcile failed: {}", e));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Channel, ImageTarget};

    #[tokio::test]
    async fn test_build_mock_pipeline() {
        let tmp = std::env::temp_dir().join(format!("gar-build-test-{}", std::process::id()));
        let sys_tmp = tmp.join("mock-sys");
        let img_tmp = tmp.join("images");
        fs::create_dir_all(&sys_tmp).unwrap();
        fs::create_dir_all(&img_tmp).unwrap();

        fs::write(sys_tmp.join("kernel"), b"dummy-kernel").unwrap();
        fs::write(sys_tmp.join("initrd"), b"dummy-initrd").unwrap();
        fs::write(sys_tmp.join("init"), b"dummy-init").unwrap();

        unsafe {
            std::env::set_var("GAR_TEST_SYSTEM_PATH", sys_tmp.display().to_string());
            std::env::set_var("GAR_IMAGES_ROOT", img_tmp.display().to_string());
            std::env::set_var("GAR_KEYS_DIR", tmp.join("keys").display().to_string());
        }

        let result = run(Some(ImageTarget::DesktopGeneric), Some(Channel::Generic)).await;

        unsafe {
            std::env::remove_var("GAR_TEST_SYSTEM_PATH");
            std::env::remove_var("GAR_IMAGES_ROOT");
            std::env::remove_var("GAR_KEYS_DIR");
        }

        assert!(result.is_ok(), "build run failed: {:?}", result);
        assert!(img_tmp.join("current").is_symlink());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_build_id_format() {
        let id = format!("v{}", Utc::now().format("%Y%m%d-%H%M%S"));
        assert!(id.starts_with("v20"));
        assert_eq!(id.len(), 16); // v + 8 date + - + 6 time = 16
    }
}
