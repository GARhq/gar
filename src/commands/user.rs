//! `gar user` subcommand — manages users, homes, quotas, client catalog.
//!
//! Replaces `ragos user <add|resize|list|delete|doctor|quota-sync|activity>`
//! from server/ragos-cli.nix (lines 572-857).
//!
//! Clean Code: each command is its own async fn, with a small dispatcher.

use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;
use owo_colors::OwoColorize;
use serde::Serialize;

use crate::cli::UserCmd;
use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::user_system::{
    self, ClientUserEntry, ClientUsersCatalog, HomeMeta, UserGroupsCatalog,
};

/// Dispatch a UserCmd to its handler.
pub async fn dispatch(cmd: UserCmd) -> Result<()> {
    match cmd {
        UserCmd::Add {
            username,
            quota,
            password,
            password_hash,
            group,
        } => cmd_add(&username, quota.as_deref(), password.as_deref(), password_hash.as_deref(), group.as_deref()).await,
        UserCmd::Resize { username, quota, force } => cmd_resize(&username, &quota, force).await,
        UserCmd::List => cmd_list().await,
        UserCmd::Delete { username, archive } => cmd_delete(&username, archive).await,
        UserCmd::Doctor { username } => cmd_doctor(&username).await,
        UserCmd::QuotaSync => cmd_quota_sync().await,
        UserCmd::Activity { username } => cmd_activity(&username).await,
    }
}

#[derive(Debug, Serialize)]
pub struct AddResult {
    pub username: String,
    pub home: String,
    pub quota: String,
    pub group: String,
    pub catalog_updated: bool,
}

/// `gar user add <name> --quota 20G [--password|--password-hash] [--group]`
pub async fn cmd_add(
    username: &str,
    quota: Option<&str>,
    password: Option<&str>,
    password_hash: Option<&str>,
    group: Option<&str>,
) -> Result<()> {
    let cfg = Config::from_env()?;

    if username.is_empty() {
        return Err(GarError::invalid_argument("uso: gar user add <nome> --quota 20G"));
    }
    if !is_valid_username(username) {
        return Err(GarError::invalid_argument(format!(
            "nome de usuário inválido: {}",
            username
        )));
    }

    let quota = quota.unwrap_or("20G");
    if human_to_bytes_check(quota).is_err() {
        return Err(GarError::user(format!("quota inválida: {}", quota)));
    }

    let group = group.unwrap_or("default");
    let home = cfg.home_base.join(username);

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

    if !user_system::is_btrfs(&cfg.home_base) {
        return Err(GarError::user(format!(
            "storage de homes em {} nao e btrfs",
            cfg.home_base.display()
        )));
    }

    // Hash password if provided
    let final_hash = if let Some(plain) = password {
        user_system::hash_password(plain)?
    } else if let Some(h) = password_hash {
        h.to_string()
    } else {
        "!".to_string() // locked
    };

    // 1. Create system account
    user_system::useradd_system(username, &home.display().to_string()).await?;

    // 2. Add to supplementary group if requested
    if group != "default" {
        if !crate::services::group_system::group_exists(group) {
            return Err(GarError::user(format!("grupo nao existe: {}", group)));
        }
        user_system::useradd_to_group(username, group).await?;
    }

    // 3. Create home (btrfs subvolume if btrfs)
    create_home_dir(&home, &cfg.home_base).await?;

    // 4. Bootstrap home tree
    bootstrap_home_tree(&home).await?;

    // 5. Enable btrfs quotas + apply quota
    user_system::enable_btrfs_quotas(&cfg.home_base).await?;
    user_system::set_quota(&home, quota).await?;

    // 6. Write metadata
    let meta = HomeMeta {
        user: username.into(),
        home: home.display().to_string(),
        quota: quota.into(),
        updated_at: Utc::now().to_rfc3339(),
    };
    HomeMeta::write(&home, &meta)?;

    // 7. Update client-users catalog
    let catalog_path = cfg.runtime_root.join("client-users.json");
    let mut catalog = ClientUsersCatalog::load(&catalog_path)?;
    let uid = user_system::user_uid(username).unwrap_or(0);
    let extra_groups: Vec<String> = user_system::user_groups(username)
        .into_iter()
        .filter(|g| !is_builtin_group(g))
        .collect();
    catalog.upsert(
        username,
        ClientUserEntry {
            uid,
            description: format!("GAR User {}", username),
            hashed_password: final_hash.clone(),
            extra_groups,
            group_gids: Default::default(),
        },
    );
    catalog.save(&catalog_path)?;

    // 8. Update user-groups catalog
    let groups_path = cfg.runtime_root.join("user-groups.json");
    let mut user_groups = UserGroupsCatalog::load(&groups_path)?;
    user_groups.set_user_group(username, group);
    user_groups.save(&groups_path)?;

    let result = AddResult {
        username: username.into(),
        home: home.display().to_string(),
        quota: quota.into(),
        group: group.into(),
        catalog_updated: true,
    };

    if cfg.json_output {
        output::json(&result)?;
    } else {
        output::ok(format!("usuário criado: {}", username));
        println!("  home:   {}", result.home);
        println!("  quota:  {}", result.quota);
        println!("  grupo:  {}", result.group);
        if final_hash == "!" {
            println!("  catalog: conta bloqueada (use --password pra login gráfico)");
        } else {
            println!("  catalog: atualizado");
        }
    }
    Ok(())
}

/// `gar user resize <name> --quota 40G [--force]`
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
    if human_to_bytes_check(quota).is_err() {
        return Err(GarError::user(format!("quota inválida: {}", quota)));
    }

    let usage_b = user_system::dir_size_bytes(&home);
    let quota_b = human_to_bytes_check(quota)?;

    let current = user_system::read_meta_value(&home, "QUOTA").unwrap_or_default();

    if usage_b > quota_b && !force {
        return Err(GarError::user(format!(
            "nova quota {} e menor que o uso atual {}; use --force se quiser prosseguir",
            quota,
            user_system::bytes_to_human(usage_b)
        )));
    }

    user_system::enable_btrfs_quotas(&cfg.home_base).await?;
    user_system::set_quota(&home, quota).await?;

    let meta = HomeMeta {
        user: username.into(),
        home: home.display().to_string(),
        quota: quota.into(),
        updated_at: Utc::now().to_rfc3339(),
    };
    HomeMeta::write(&home, &meta)?;

    if cfg.json_output {
        output::json(&serde_json::json!({
            "username": username,
            "home": home.display().to_string(),
            "old_quota": current,
            "new_quota": quota,
            "usage_bytes": usage_b,
        }))?;
    } else {
        output::ok(format!("usuário redimensionado: {}", username));
        println!("  uso atual:     {}", user_system::bytes_to_human(usage_b));
        println!("  quota anterior: {}", if current.is_empty() { "—" } else { &current });
        println!("  nova quota:    {}", quota);
    }
    Ok(())
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

/// `gar user list` — table of homes with usage/quota/%.
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

    let mut rows: Vec<UserRow> = Vec::new();
    for entry in std::fs::read_dir(&cfg.home_base)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let username = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if username.starts_with('.') || username == ".archive" {
            continue;
        }

        let usage_b = user_system::dir_size_bytes(&path);
        let usage = user_system::bytes_to_human(usage_b);

        let quota_str = user_system::read_meta_value(&path, "QUOTA").unwrap_or_default();
        let quota_b = if quota_str.is_empty() {
            0
        } else {
            human_to_bytes_check(&quota_str).unwrap_or(0)
        };

        let percent = if quota_b > 0 {
            format!("{}%", usage_b * 100 / quota_b)
        } else {
            "—".into()
        };

        rows.push(UserRow {
            username: username.into(),
            usage,
            usage_bytes: usage_b,
            quota: if quota_str.is_empty() { "—".into() } else { quota_str },
            quota_bytes: quota_b,
            percent,
            home: path.display().to_string(),
        });
    }

    rows.sort_by(|a, b| a.username.cmp(&b.username));

    if cfg.json_output {
        output::json(&rows)?;
        return Ok(());
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
    println!();
    println!("  Total: {} usuário(s)", rows.len());
    Ok(())
}

/// `gar user delete <name> --archive` (archive required).
pub async fn cmd_delete(username: &str, archive: bool) -> Result<()> {
    let cfg = Config::from_env()?;

    if !archive {
        return Err(GarError::user(
            "delete sem --archive e proibido (seguranca)",
        ));
    }
    if username.is_empty() {
        return Err(GarError::invalid_argument("uso: gar user delete <nome> --archive"));
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
    let archive_path = cfg.home_archive_base.join(format!("{}-{}", username, stamp));

    std::fs::create_dir_all(&cfg.home_archive_base)?;

    // BTRFS snapshot (read-only) before move
    if user_system::is_btrfs(&home) {
        std::fs::create_dir_all(&cfg.home_snapshot_base)?;
        let snap_path = cfg.home_snapshot_base.join(format!("{}-{}", username, stamp));
        let _ = crate::services::shell::run_success(
            "btrfs",
            &[
                "subvolume",
                "snapshot",
                "-r",
                &home.display().to_string(),
                &snap_path.display().to_string(),
            ],
        )
        .await;
    }

    // Move home → archive
    std::fs::rename(&home, &archive_path)?;
    let _ = user_system::userdel(username).await;

    // Remove from client-users catalog
    let catalog_path = cfg.runtime_root.join("client-users.json");
    if catalog_path.exists() {
        let mut catalog = ClientUsersCatalog::load(&catalog_path)?;
        catalog.remove(username);
        catalog.save(&catalog_path)?;
    }

    if cfg.json_output {
        output::json(&serde_json::json!({
            "username": username,
            "archive": archive_path.display().to_string(),
        }))?;
    } else {
        output::ok(format!("usuário arquivado: {}", username));
        println!("  home arquivada em: {}", archive_path.display());
    }
    Ok(())
}

/// `gar user doctor <name>` — detailed info.
pub async fn cmd_doctor(username: &str) -> Result<()> {
    if username.is_empty() {
        return Err(GarError::invalid_argument("uso: gar user doctor <nome>"));
    }
    let cfg = Config::from_env()?;
    let home = cfg.home_base.join(username);

    if !home.exists() {
        return Err(GarError::user(format!(
            "home ausente para {}: {}",
            username,
            home.display()
        )));
    }

    let usage_b = user_system::dir_size_bytes(&home);
    let usage = user_system::bytes_to_human(usage_b);
    let quota = user_system::read_meta_value(&home, "QUOTA").unwrap_or_default();

    let catalog_path = cfg.runtime_root.join("client-users.json");
    let catalog_status: String = if catalog_path.exists() {
        match ClientUsersCatalog::load(&catalog_path) {
            Ok(c) if c.get(username).is_some() => "presente".into(),
            _ => "ausente".into(),
        }
    } else {
        "indisponivel".into()
    };

    let stat_info = std::fs::metadata(&home).ok();
    let (owner, mode) = stat_info
        .map(|m| {
            use std::os::unix::fs::MetadataExt;
            let uid = m.uid();
            let gid = m.gid();
            let mode = m.mode() & 0o7777;
            let owner = std::process::Command::new("getent")
                .args(["passwd", &uid.to_string()])
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        let s = String::from_utf8_lossy(&o.stdout);
                        s.split(':').next().map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| uid.to_string());
            let group = std::process::Command::new("getent")
                .args(["group", &gid.to_string()])
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        let s = String::from_utf8_lossy(&o.stdout);
                        s.split(':').next().map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| gid.to_string());
            (format!("{}:{}", owner, group), format!("{:o}", mode))
        })
        .unwrap_or_else(|| ("?".into(), "?".into()));

    let fstype = user_system::fs_type(&home).unwrap_or_else(|| "desconhecido".into());
    let mount = user_system::mount_info(&home).unwrap_or_else(|| "(indisponivel)".into());
    let qgroup = user_system::qgroup_info(&home).unwrap_or_else(|| "(indisponivel)".into());

    if cfg.json_output {
        output::json(&serde_json::json!({
            "username": username,
            "home": home.display().to_string(),
            "filesystem": fstype,
            "owner": owner,
            "mode": mode,
            "usage_bytes": usage_b,
            "quota": quota,
            "catalog": catalog_status,
            "mount": mount,
            "qgroup": qgroup,
        }))?;
    } else {
        println!("  usuario: {}", username);
        println!("  home:    {}", home.display());
        println!("  filesystem: {}", fstype);
        println!("  owner:  {}", owner);
        println!("  modo:   {}", mode);
        println!("  uso:    {}", usage);
        println!("  quota:  {}", if quota.is_empty() { "—" } else { &quota });
        println!("  catalog_cliente: {}", catalog_status);
        println!("  montagem:");
        println!("{}", mount.lines().map(|l| format!("    {}", l)).collect::<Vec<_>>().join("\n"));
        println!("  qgroup:");
        println!("{}", qgroup.lines().map(|l| format!("    {}", l)).collect::<Vec<_>>().join("\n"));
    }

    Ok(())
}

/// `gar user quota-sync` — sync btrfs qgroups with `.garos-home-meta`.
pub async fn cmd_quota_sync() -> Result<()> {
    let cfg = Config::from_env()?;

    if !cfg.home_base.exists() {
        output::warn("storage de homes ausente");
        return Ok(());
    }
    if !user_system::is_mountpoint(&cfg.home_base) {
        return Err(GarError::user("home nao esta montada"));
    }

    user_system::enable_btrfs_quotas(&cfg.home_base).await?;

    let mut synced = 0;
    let mut skipped = 0;

    for entry in std::fs::read_dir(&cfg.home_base)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if name.starts_with('.') {
            continue;
        }

        let quota = user_system::read_meta_value(&path, "QUOTA");
        let quota = match quota {
            Some(q) if !q.is_empty() => q,
            _ => {
                skipped += 1;
                continue;
            }
        };

        if let Err(e) = user_system::set_quota(&path, &quota).await {
            output::warn(format!("falha ao sincronizar {}: {}", name, e));
        } else {
            synced += 1;
        }
    }

    output::ok(format!(
        "quotas sincronizadas ({} sincronizadas, {} sem metadata)",
        synced, skipped
    ));
    Ok(())
}

/// `gar user activity <name>` — login history from audit log.
pub async fn cmd_activity(username: &str) -> Result<()> {
    let cfg = Config::from_env()?;
    let audit_file = cfg.audit_dir.join("login-history.json");

    if !audit_file.exists() {
        println!("sem registro de auditoria para {}", username);
        return Ok(());
    }

    let bytes = std::fs::read(&audit_file)?;
    let json: serde_json::Value = serde_json::from_slice(&bytes)?;

    let sessions = json
        .get("sessions")
        .and_then(|s| s.get(username))
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));

    if cfg.json_output {
        output::json(&sessions)?;
    } else {
        if let Some(arr) = sessions.as_array() {
            if arr.is_empty() {
                println!("sem sessões registradas para {}", username);
                return Ok(());
            }
            for s in arr {
                let ts = s.get("timestamp").and_then(|v| v.as_str()).unwrap_or("?");
                let action = s.get("action").and_then(|v| v.as_str()).unwrap_or("?");
                let tty = s.get("tty").and_then(|v| v.as_str()).unwrap_or("?");
                let ip = s.get("ip").and_then(|v| v.as_str()).unwrap_or("?");
                println!("[{}] [{}] tty={} ip={}", ts, action, tty, ip);
            }
        } else {
            println!("formato inesperado em audit log");
        }
    }
    Ok(())
}

// ---------- Internal helpers ----------

fn is_valid_username(s: &str) -> bool {
    // [a-z_][a-z0-9_-]{0,30}
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() || c == '_' => {}
        _ => return false,
    }
    for c in chars {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-') {
            return false;
        }
    }
    s.len() <= 31
}

fn is_builtin_group(name: &str) -> bool {
    matches!(
        name,
        "" | "audio"
            | "nogroup"
            | "root"
            | "users"
            | "video"
            | "wheel"
    )
}

fn human_to_bytes_check(human: &str) -> Result<u64> {
    user_system::human_to_bytes(human)
}

async fn create_home_dir(home: &Path, home_base: &Path) -> Result<()> {
    if user_system::is_btrfs(home_base) {
        let _ = crate::services::shell::run_success(
            "btrfs",
            &["subvolume", "create", &home.display().to_string()],
        )
        .await?;
    } else {
        std::fs::create_dir_all(home)?;
    }
    Ok(())
}

async fn bootstrap_home_tree(home: &Path) -> Result<()> {
    for sub in [".config", ".cache", ".local/share", "Desktop", "Documents", "Downloads", "Music", "Pictures", "Videos"] {
        std::fs::create_dir_all(home.join(sub))?;
    }
    // Permissions
    std::fs::set_permissions(home, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    for sub in [".config", ".cache", ".local"] {
        std::fs::set_permissions(home.join(sub), std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    }
    std::fs::set_permissions(home.join(".local/share"), std::os::unix::fs::PermissionsExt::from_mode(0o750))?;
    Ok(())
}

#[allow(dead_code)]
fn _silence_unused_hashmap_warning() -> HashMap<String, String> {
    HashMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_username() {
        assert!(is_valid_username("alice"));
        assert!(is_valid_username("user_1"));
        assert!(is_valid_username("user-1"));
        assert!(!is_valid_username("Alice")); // uppercase
        assert!(!is_valid_username("1user")); // starts with digit
        assert!(!is_valid_username("user with space"));
        assert!(!is_valid_username(""));
        assert!(!is_valid_username(&"a".repeat(40))); // too long
    }

    #[test]
    fn test_is_builtin_group() {
        assert!(is_builtin_group("root"));
        assert!(is_builtin_group("users"));
        assert!(is_builtin_group("audio"));
        assert!(!is_builtin_group("garhq"));
    }

    #[test]
    fn test_add_result_serialize() {
        let r = AddResult {
            username: "alice".into(),
            home: "/srv/data/home/alice".into(),
            quota: "20G".into(),
            group: "default".into(),
            catalog_updated: true,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("alice"));
        assert!(json.contains("20G"));
    }
}