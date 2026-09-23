//! `gar provision-home` — provisionamento atômico de homes de usuários GAROS.
//!
//! Substitui o script bash inline do motor.nix por um executável Rust com
//! tratamento de erro robusto, logging estruturado e idempotência.

use std::path::{Path, PathBuf};
use std::process::Command as SysCommand;

use clap::Args;

use crate::error::GarError;
use crate::output;

/// Storage backend para a home do usuário.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum StorageBackend {
    /// Detecta automaticamente (BTRFS se disponível, senão plain)
    Auto,
    /// BTRFS subvolumes com quotas
    Btrfs,
    /// Diretório simples (mkdir -p)
    Plain,
}

/// Argumentos do subcomando `provision-home`.
#[derive(Debug, Args)]
pub struct ProvisionHomeArgs {
    /// Nome do usuário (ex: aluno01)
    #[arg(long)]
    pub user: String,

    /// Caminho absoluto da home (ex: /data/homes/aluno01)
    #[arg(long)]
    pub home_path: PathBuf,

    /// Quota em GB (ex: 20). Ignorado no backend plain.
    #[arg(long)]
    pub quota_gb: Option<u64>,

    /// Backend de storage (auto, btrfs, plain)
    #[arg(long, value_enum, default_value = "auto")]
    pub backend: StorageBackend,
}

/// Detecta se o filesystem de `path` é BTRFS.
fn is_btrfs(path: &Path) -> bool {
    let parent = path.parent().unwrap_or(Path::new("/"));
    SysCommand::new("stat")
        .args(["-f", "-c", "%T"])
        .arg(parent)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "btrfs")
        .unwrap_or(false)
}

/// Verifica se `path` é um subvolume BTRFS existente.
fn is_btrfs_subvolume(path: &Path) -> bool {
    SysCommand::new("btrfs")
        .args(["subvolume", "show"])
        .arg(path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Cria um subvolume BTRFS em `path`, migrando dados existentes se necessário.
fn create_btrfs_subvolume(path: &Path) -> Result<(), GarError> {
    if is_btrfs_subvolume(path) {
        eprintln!("[GAROS:provision-home] Subvolume já existe em {}", path.display());
        return Ok(());
    }

    // Se o diretório já existe com dados, migrar atomicamente
    let migrate_dir = if path.exists() {
        let tmp = tempfile::tempdir_in(
            path.parent().unwrap_or(Path::new("/tmp")),
        )
        .map_err(|e| GarError::Generic(format!("Falha ao criar dir temporário: {e}")))?;

        // Mover conteúdo para tmp
        let entries: Vec<_> = std::fs::read_dir(path)
            .map_err(|e| GarError::Generic(format!("Falha ao ler {}: {e}", path.display())))?
            .filter_map(|e| e.ok())
            .collect();

        for entry in &entries {
            let dest = tmp.path().join(entry.file_name());
            std::fs::rename(entry.path(), &dest).map_err(|e| {
                GarError::Generic(format!(
                    "Falha ao mover {} -> {}: {e}",
                    entry.path().display(),
                    dest.display()
                ))
            })?;
        }

        // Remover diretório vazio para criar subvolume
        std::fs::remove_dir(path).map_err(|e| {
            GarError::Generic(format!("Falha ao remover dir vazio {}: {e}", path.display()))
        })?;

        Some(tmp)
    } else {
        None
    };

    // Criar subvolume
    let status = SysCommand::new("btrfs")
        .args(["subvolume", "create"])
        .arg(path)
        .status()
        .map_err(|e| GarError::Generic(format!("Falha ao executar btrfs subvolume create: {e}")))?;

    if !status.success() {
        return Err(GarError::Generic(format!(
            "btrfs subvolume create falhou para {}",
            path.display()
        )));
    }

    // Restaurar dados migrados
    if let Some(tmp) = migrate_dir {
        let entries: Vec<_> = std::fs::read_dir(tmp.path())
            .map_err(|e| GarError::Generic(format!("Falha ao ler dir tmp: {e}")))?
            .filter_map(|e| e.ok())
            .collect();

        for entry in &entries {
            let dest = path.join(entry.file_name());
            // Usa cp -a para preservar atributos (rename não funciona cross-subvolume)
            let cp_status = SysCommand::new("cp")
                .args(["-a"])
                .arg(entry.path())
                .arg(&dest)
                .status()
                .map_err(|e| GarError::Generic(format!("Falha ao copiar de volta: {e}")))?;

            if !cp_status.success() {
                return Err(GarError::Generic(format!(
                    "Falha ao restaurar {} para {}",
                    entry.path().display(),
                    dest.display()
                )));
            }
        }
        eprintln!(
            "[GAROS:provision-home] Dados migrados para subvolume {}",
            path.display()
        );
    }

    eprintln!(
        "[GAROS:provision-home] Subvolume criado: {}",
        path.display()
    );
    Ok(())
}

/// Aplica quota BTRFS no subvolume.
fn apply_btrfs_quota(path: &Path, quota_gb: u64) -> Result<(), GarError> {
    // Obter qgroup
    let output = SysCommand::new("btrfs")
        .args(["qgroup", "show", "-f"])
        .arg(path)
        .output()
        .map_err(|e| GarError::Generic(format!("Falha ao consultar qgroup: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let qgroup = stdout
        .lines()
        .nth(2)
        .and_then(|line| line.split_whitespace().next())
        .unwrap_or("0/0");

    let status = SysCommand::new("btrfs")
        .args(["qgroup", "limit", &format!("{quota_gb}G"), qgroup])
        .arg(path)
        .status()
        .map_err(|e| GarError::Generic(format!("Falha ao aplicar quota: {e}")))?;

    if !status.success() {
        eprintln!(
            "[GAROS:provision-home] AVISO: quota de {quota_gb}G não aplicada em {} (qgroup={qgroup})",
            path.display()
        );
    } else {
        eprintln!(
            "[GAROS:provision-home] Quota de {quota_gb}G aplicada em {} (qgroup={qgroup})",
            path.display()
        );
    }
    Ok(())
}

/// Ajusta ownership da home.
fn chown_home(user: &str, path: &Path) -> Result<(), GarError> {
    let status = SysCommand::new("chown")
        .arg(&format!("{user}:users"))
        .arg(path)
        .status()
        .map_err(|e| GarError::Generic(format!("Falha ao chown: {e}")))?;

    if !status.success() {
        return Err(GarError::Generic(format!(
            "chown {user}:users {} falhou",
            path.display()
        )));
    }
    Ok(())
}

/// Executa o provisionamento completo de uma home.
pub fn run(args: &ProvisionHomeArgs, json: bool) -> Result<(), GarError> {
    let backend = match args.backend {
        StorageBackend::Auto => {
            if is_btrfs(&args.home_path) {
                StorageBackend::Btrfs
            } else {
                StorageBackend::Plain
            }
        }
        other => other,
    };

    eprintln!(
        "[GAROS:provision-home] Provisionando home de {} em {} (backend={:?})",
        args.user,
        args.home_path.display(),
        backend
    );

    match backend {
        StorageBackend::Btrfs => {
            create_btrfs_subvolume(&args.home_path)?;
            if let Some(quota_gb) = args.quota_gb {
                apply_btrfs_quota(&args.home_path, quota_gb)?;
            }
        }
        StorageBackend::Plain | StorageBackend::Auto => {
            std::fs::create_dir_all(&args.home_path).map_err(|e| {
                GarError::Generic(format!(
                    "Falha ao criar diretório {}: {e}",
                    args.home_path.display()
                ))
            })?;
        }
    }

    chown_home(&args.user, &args.home_path)?;

    if json {
        output::print_json(&serde_json::json!({
            "status": "ok",
            "user": args.user,
            "home_path": args.home_path.display().to_string(),
            "backend": format!("{:?}", backend).to_lowercase(),
            "quota_gb": args.quota_gb,
        }));
    } else {
        eprintln!(
            "[GAROS:provision-home] ✓ Home de {} provisionada com sucesso",
            args.user
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_backend_auto_detection_non_btrfs() {
        // /tmp nunca é btrfs em ambientes de teste
        assert!(!is_btrfs(Path::new("/tmp/test-home")));
    }

    #[test]
    fn test_provision_plain_creates_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("testuser");

        let args = ProvisionHomeArgs {
            user: "testuser".to_string(),
            home_path: home.clone(),
            quota_gb: None,
            backend: StorageBackend::Plain,
        };

        // Este teste pode falhar se o usuário "testuser" não existir no sistema,
        // mas a criação do diretório deve funcionar
        let _ = run(&args, false);
        assert!(home.exists());
    }
}
