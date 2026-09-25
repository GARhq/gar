use chrono::Utc;
use owo_colors::OwoColorize;
use serde::Serialize;
use std::path::Path;

use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::filesystem::FsOps;
use crate::services::user_system::{
    self, ClientUserEntry, ClientUsersCatalog, HomeMeta, UserGroupsCatalog,
};

const DEFAULT_USER_SUBDIRS: &[&str] = &[
    ".config",
    ".cache",
    ".local/share",
    "Desktop",
    "Documents",
    "Downloads",
    "Music",
    "Pictures",
    "Videos",
];

#[derive(Debug, Serialize)]
pub struct AddResult {
    pub username: String,
    pub home: String,
    pub quota: String,
    pub group: String,
    pub catalog_updated: bool,
}

#[derive(Debug, Serialize)]
pub struct DeleteResult {
    pub username: String,
    pub archive: String,
}

#[derive(Debug, Serialize)]
pub struct UserRow {
    pub username: String,
    pub usage: String,
    pub usage_bytes: u64,
    pub quota: String,
    pub quota_bytes: u64,
    pub percent: String,
    pub home: String,
}

pub async fn cmd_add(
    username: &str,
    quota: Option<&str>,
    password: Option<&str>,
    password_hash: Option<&str>,
    group: Option<&str>,
) -> Result<()> {
    let cfg = Config::from_env()?;
    let quota = quota.unwrap_or("20G");
    let group = group.unwrap_or("default");
    let home = cfg.home_base.join(username);

    validate_username(username)?;
    human_to_bytes_check(quota)?;

    if home.exists() {
        return Err(GarError::user(format!(
            "usuario/home ja existe: {} ({})",
            username,
            home.display()
        )));
    }
    if !cfg.home_base.exists() || !user_system::is_mountpoint(&cfg.home_base) {
        return Err(GarError::user(format!(
            "home persistente nao esta montada em {}",
            cfg.home_base.display()
        )));
    }
    let ops = FsOps::for_path(&cfg.home_base)?;

    let final_hash = match password {
        Some(plain) => user_system::hash_password(plain)?,
        None => password_hash
            .map(String::from)
            .unwrap_or_else(|| "!".into()),
    };

    user_system::useradd_system(username, &home.display().to_string()).await?;
    if group != "default" {
        if !crate::services::group_system::group_exists(group) {
            return Err(GarError::user(format!("grupo nao existe: {}", group)));
        }
        user_system::useradd_to_group(username, group).await?;
    }

    ops.create_subvolume(&home).await?;
    bootstrap_home_tree(&home);
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

    upsert_user_catalog(&cfg, username, &final_hash)?;
    upsert_user_groups(&cfg, username, group)?;

    if cfg.json_output {
        output::json(&AddResult {
            username: username.into(),
            home: home.display().to_string(),
            quota: quota.into(),
            group: group.into(),
            catalog_updated: true,
        })?;
    } else {
        output::ok(format!("usuário criado: {}", username));
        println!("  home:   {}", home.display());
        println!("  quota:  {}", quota);
        println!("  grupo:  {}", group);
        println!(
            "  catalog: {}",
            if final_hash == "!" {
                "conta bloqueada (use --password pra login gráfico)"
            } else {
                "atualizado"
            }
        );
    }
    Ok(())
}

pub async fn cmd_list() -> Result<()> {
    let cfg = Config::from_env()?;
    if !cfg.home_base.exists() {
        output::warn(format!(
            "storage de homes ausente em {}",
            cfg.home_base.display()
        ));
        return Ok(());
    }
    if !user_system::is_mountpoint(&cfg.home_base) {
        return Err(GarError::user(format!(
            "home persistente nao esta montada em {}",
            cfg.home_base.display()
        )));
    }

    let mut rows: Vec<UserRow> = std::fs::read_dir(&cfg.home_base)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?;
            if name.starts_with('.') || name == ".archive" {
                return None;
            }
            let usage_b = user_system::dir_size_bytes(&p);
            let quota_str = user_system::read_meta_value(&p, "QUOTA").unwrap_or_default();
            let quota_b = if quota_str.is_empty() {
                0
            } else {
                human_to_bytes_check(&quota_str).unwrap_or(0)
            };
            Some(UserRow {
                username: name.into(),
                usage: user_system::bytes_to_human(usage_b),
                usage_bytes: usage_b,
                quota: if quota_str.is_empty() {
                    "—".into()
                } else {
                    quota_str
                },
                quota_bytes: quota_b,
                percent: if quota_b > 0 {
                    format!("{}%", usage_b * 100 / quota_b)
                } else {
                    "—".into()
                },
                home: p.display().to_string(),
            })
        })
        .collect();
    rows.sort_by(|a, b| a.username.cmp(&b.username));

    if cfg.json_output {
        return output::json(&rows);
    }
    if rows.is_empty() {
        output::warn("Nenhum usuário encontrado.");
        return Ok(());
    }

    output::section("Usuários GAR");
    println!();
    println!(
        "  {:<20} {:<12} {:<10} {:<8} {}",
        "USERNAME".bold(),
        "USO".bold(),
        "QUOTA".bold(),
        "%".bold(),
        "HOME".bold()
    );
    for row in &rows {
        println!(
            "  {:<20} {:<12} {:<10} {:<8} {}",
            row.username, row.usage, row.quota, row.percent, row.home
        );
    }
    println!("\n  Total: {} usuário(s)", rows.len());
    Ok(())
}

pub async fn cmd_delete(username: &str, archive: bool) -> Result<()> {
    let cfg = Config::from_env()?;
    if !archive {
        return Err(GarError::user(
            "delete sem --archive e proibido (seguranca)",
        ));
    }
    if username.is_empty() {
        return Err(GarError::invalid_argument(
            "uso: gar user delete <nome> --archive",
        ));
    }
    let home = cfg.home_base.join(username);
    if !home.exists() {
        return Err(GarError::user(format!(
            "home ausente para {}: {}",
            username,
            home.display()
        )));
    }

    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let archive_path = cfg
        .home_archive_base
        .join(format!("{}-{}", username, stamp));
    std::fs::create_dir_all(&cfg.home_archive_base)?;

    if let Ok(ops) = FsOps::for_path(&home) {
        let snap_path = cfg
            .home_snapshot_base
            .join(format!("{}-{}", username, stamp));
        std::fs::create_dir_all(&cfg.home_snapshot_base)?;
        let _ = ops.snapshot_readonly(&home, &snap_path).await;
    }

    std::fs::rename(&home, &archive_path)?;
    let _ = user_system::userdel(username).await;

    let catalog_path = cfg.runtime_root.join("client-users.json");
    if catalog_path.exists() {
        let mut catalog = ClientUsersCatalog::load(&catalog_path)?;
        catalog.remove(username);
        catalog.save(&catalog_path)?;
    }

    if cfg.json_output {
        output::json(&DeleteResult {
            username: username.into(),
            archive: archive_path.display().to_string(),
        })?;
    } else {
        output::ok(format!("usuário arquivado: {}", username));
        println!("  home arquivada em: {}", archive_path.display());
    }
    Ok(())
}

fn validate_username(s: &str) -> Result<()> {
    if s.is_empty() {
        return Err(GarError::invalid_argument(
            "uso: gar user add <nome> --quota 20G",
        ));
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c == '_' => {}
        _ => {
            return Err(GarError::invalid_argument(format!(
                "nome de usuário inválido: {}",
                s
            )))
        }
    }
    for c in chars {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-') {
            return Err(GarError::invalid_argument(format!(
                "nome de usuário inválido: {}",
                s
            )));
        }
    }
    if s.len() > 31 {
        return Err(GarError::invalid_argument(format!(
            "nome de usuário inválido: {}",
            s
        )));
    }
    Ok(())
}

fn is_builtin_group(name: &str) -> bool {
    matches!(
        name,
        "root" | "users" | "wheel" | "audio" | "video" | "nogroup"
    )
}

pub(crate) fn human_to_bytes_check(human: &str) -> Result<u64> {
    user_system::human_to_bytes(human)
}

fn upsert_user_catalog(cfg: &Config, username: &str, hash: &str) -> Result<()> {
    let path = cfg.runtime_root.join("client-users.json");
    let mut catalog = ClientUsersCatalog::load(&path)?;
    let extra_groups: Vec<String> = user_system::user_groups(username)
        .into_iter()
        .filter(|g| !is_builtin_group(g))
        .collect();
    catalog.upsert(
        username,
        ClientUserEntry {
            uid: user_system::user_uid(username).unwrap_or(0),
            hashed_password: hash.into(),
            extra_groups,
        },
    );
    catalog.save(&path)
}

fn upsert_user_groups(cfg: &Config, username: &str, group: &str) -> Result<()> {
    let path = cfg.runtime_root.join("user-groups.json");
    let mut catalog = UserGroupsCatalog::load(&path)?;
    catalog.set_user_group(username, group);
    catalog.save(&path)
}

fn bootstrap_home_tree(home: &Path) {
    for sub in DEFAULT_USER_SUBDIRS {
        let _ = std::fs::create_dir_all(home.join(sub));
    }
    set_mode(home, 0o700);
    for sub in [".config", ".cache", ".local"] {
        set_mode(&home.join(sub), 0o700);
    }
    set_mode(&home.join(".local/share"), 0o750);
}

fn set_mode(path: &Path, mode: u32) {
    let _ = std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(mode));
}
