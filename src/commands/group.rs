//! `gar group` subcommand — manages groups.

use chrono::Utc;
use owo_colors::OwoColorize;
use serde::Serialize;

use crate::cli::GroupCmd;
use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::group_system::{
    self, CatalogEntry, GroupCatalog, GroupPermissions, QuotaSpec,
};

pub async fn dispatch(cmd: GroupCmd) -> Result<()> {
    match cmd {
        GroupCmd::Add {
            groupname,
            description,
            storage_quota,
        } => {
            cmd_add(
                &groupname,
                description.as_deref(),
                storage_quota.as_deref(),
            )
            .await
        }
        GroupCmd::List => cmd_list().await,
        GroupCmd::Delete { groupname, archive } => cmd_delete(&groupname, archive).await,
        GroupCmd::Chmod { groupname, perms } => cmd_chmod(&groupname, &perms).await,
        GroupCmd::Members { groupname, add, remove } => {
            cmd_members(&groupname, add.as_deref(), remove.as_deref()).await
        }
        GroupCmd::Permissions { groupname } => cmd_permissions(&groupname).await,
        GroupCmd::EnsureDefaults => cmd_ensure_defaults().await,
    }
}

#[derive(Debug, Serialize)]
pub struct GroupAddResult {
    pub name: String,
    pub description: String,
    pub gid: u32,
    pub storage: String,
    pub quota: String,
    pub catalog_updated: bool,
}

#[derive(Debug, Serialize)]
pub struct GroupDeleteResult {
    pub name: String,
    pub archive: String,
}

#[derive(Debug, Serialize)]
pub struct GroupMembersResult {
    pub group: String,
    pub added: Option<String>,
    pub removed: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GroupChmodResult {
    pub group: String,
    pub mode: String,
}

#[derive(Debug, Serialize)]
pub struct GroupPermissionsResult {
    pub group: String,
    pub mode: String,
    pub members: Vec<String>,
    pub extras: std::collections::HashMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct GroupEnsureDefaultsResult {
    pub created: Vec<String>,
    pub already_existed: Vec<String>,
}

pub async fn cmd_add(
    name: &str,
    description: Option<&str>,
    storage_quota: Option<&str>,
) -> Result<()> {
    let cfg = Config::from_env()?;
    if name.is_empty() {
        return Err(GarError::invalid_argument(
            "uso: gar group add <nome> [--description DESC] [--storage-quota 100G]",
        ));
    }
    if group_system::is_permanent(name) && group_system::group_exists(name) {
        // admin exists: idempotent — succeed without re-creating
        return report_existing(name, &cfg);
    }
    let description = description.unwrap_or("RAGOS Group");
    let quota_str = storage_quota.unwrap_or("100G");
    let quota = QuotaSpec::new(quota_str)?;
    let sector = group_system::sector_path(&cfg.storage_base, name);

    if sector.exists() {
        return Err(GarError::user(format!(
            "setor do grupo ja existe: {} ({})",
            name,
            sector.display()
        )));
    }

    if !group_system::group_exists(name) {
        group_system::groupadd_system(name, None).await?;
    }
    let gid = group_system::group_gid(name)
        .ok_or_else(|| GarError::user(format!("grupo criado mas gid nao resolvido: {}", name)))?;

    group_system::chown_sector(&sector, name)?;
    let meta = group_system::build_meta(name, description, gid, &quota);
    group_system::write_meta(&sector, &meta)?;
    if let Err(e) = group_system::apply_quota(&sector, &quota).await {
        output::warn(format!(
            "quota nao aplicada (filesystem pode nao suportar): {}",
            e
        ));
    }

    let cat_path = group_system::catalog_path(&cfg.runtime_root);
    let mut catalog = GroupCatalog::load(&cat_path)?;
    catalog.upsert(
        name,
        CatalogEntry {
            description: description.into(),
            storage_path: sector.display().to_string(),
            quota: quota.human.clone(),
            gid,
            created_at: meta.created_at.clone(),
        },
    );
    catalog.save(&cat_path)?;

    if cfg.json_output {
        output::json(&GroupAddResult {
            name: name.into(),
            description: description.into(),
            gid,
            storage: sector.display().to_string(),
            quota: quota.human.clone(),
            catalog_updated: true,
        })?;
    } else {
        output::ok(format!("grupo criado: {}", name));
        println!("  descricao:  {}", description);
        println!("  gid:        {}", gid);
        println!("  setor:      {}", sector.display());
        println!("  quota:      {}", quota.human);
    }
    Ok(())
}

fn report_existing(name: &str, cfg: &Config) -> Result<()> {
    let cat_path = group_system::catalog_path(&cfg.runtime_root);
    let catalog = GroupCatalog::load(&cat_path)?;
    let entry = catalog.get(name);
    if cfg.json_output {
        let (description, storage, quota, gid) = match entry {
            Some(e) => (e.description.clone(), e.storage_path.clone(), e.quota.clone(), e.gid),
            None => (
                "RAGOS Group".into(),
                group_system::sector_path(&cfg.storage_base, name).display().to_string(),
                "100G".into(),
                group_system::group_gid(name).unwrap_or(0),
            ),
        };
        output::json(&GroupAddResult {
            name: name.into(),
            description,
            gid,
            storage,
            quota,
            catalog_updated: false,
        })?;
    } else {
        output::ok(format!("grupo permanente '{}' ja existe (idempotente)", name));
    }
    Ok(())
}

pub async fn cmd_list() -> Result<()> {
    let cfg = Config::from_env()?;
    if !cfg.storage_base.exists() {
        output::warn(format!(
            "storage de grupos ausente em {}",
            cfg.storage_base.display()
        ));
        return Ok(());
    }
    let cat_path = group_system::catalog_path(&cfg.runtime_root);
    let catalog = GroupCatalog::load(&cat_path)?;

    let mut rows: Vec<group_system::GroupRow> = std::fs::read_dir(&cfg.storage_base)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?.to_string();
            if name.starts_with('.') {
                return None;
            }
            let entry = catalog.get(&name);
            let (description, quota_str, gid, _) = match entry {
                Some(e) => (
                    e.description.clone(),
                    e.quota.clone(),
                    e.gid,
                    (),
                ),
                None => (
                    "—".into(),
                    "—".into(),
                    group_system::group_gid(&name).unwrap_or(0),
                    (),
                ),
            };
            let quota_bytes = if quota_str == "—" {
                0
            } else {
                QuotaSpec::new(&quota_str).map(|q| q.bytes).unwrap_or(0)
            };
            let usage_b = group_system::sector_usage_bytes(&p);
            Some(group_system::GroupRow {
                name,
                description,
                gid,
                quota: quota_str,
                quota_bytes,
                storage_path: p.display().to_string(),
                usage: crate::services::user_system::bytes_to_human(usage_b),
            })
        })
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));

    if cfg.json_output {
        return output::json(&rows);
    }
    if rows.is_empty() {
        output::warn("Nenhum grupo encontrado.");
        return Ok(());
    }
    output::section("Grupos GAR");
    println!();
    println!(
        "  {:<16} {:<8} {:<10} {:<10} {}",
        "GRUPO".bold(),
        "GID".bold(),
        "QUOTA".bold(),
        "USO".bold(),
        "SETOR".bold()
    );
    for r in &rows {
        println!(
            "  {:<16} {:<8} {:<10} {:<10} {}",
            r.name, r.gid, r.quota, r.usage, r.storage_path
        );
    }
    println!("\n  Total: {} grupo(s)", rows.len());
    Ok(())
}

pub async fn cmd_delete(name: &str, archive: bool) -> Result<()> {
    let cfg = Config::from_env()?;
    if name.is_empty() {
        return Err(GarError::invalid_argument(
            "uso: gar group delete <nome> --archive",
        ));
    }
    if group_system::is_permanent(name) {
        return Err(GarError::user(format!(
            "grupo '{}' e permanente e nao pode ser deletado",
            name
        )));
    }
    if !archive {
        return Err(GarError::user(
            "delete sem --archive e proibido (seguranca)",
        ));
    }
    let sector = group_system::sector_path(&cfg.storage_base, name);
    if !sector.exists() {
        return Err(GarError::user(format!(
            "setor do grupo ausente: {}",
            sector.display()
        )));
    }
    std::fs::create_dir_all(&cfg.storage_archive)?;
    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let archive_path = cfg.storage_archive.join(format!("{}-{}", name, stamp));
    std::fs::rename(&sector, &archive_path)?;

    // Remove from catalog
    let cat_path = group_system::catalog_path(&cfg.runtime_root);
    let mut catalog = GroupCatalog::load(&cat_path)?;
    catalog.remove(name);
    catalog.save(&cat_path)?;

    if cfg.json_output {
        output::json(&GroupDeleteResult {
            name: name.into(),
            archive: archive_path.display().to_string(),
        })?;
    } else {
        output::ok(format!("grupo arquivado: {}", name));
        println!("  setor arquivado em: {}", archive_path.display());
    }
    Ok(())
}

pub async fn cmd_members(
    name: &str,
    add: Option<&str>,
    remove: Option<&str>,
) -> Result<()> {
    let cfg = Config::from_env()?;
    if name.is_empty() {
        return Err(GarError::invalid_argument(
            "uso: gar group members <nome> [--add user|--remove user]",
        ));
    }
    if !group_system::group_exists(name) {
        return Err(GarError::user(format!("grupo nao existe: {}", name)));
    }
    if add.is_none() && remove.is_none() {
        // Listing mode
        let members = group_system::group_members(name);
        if cfg.json_output {
            output::json(&serde_json::json!({"group": name, "members": members}))?;
        } else {
            output::section(&format!("Membros de '{}'", name));
            if members.is_empty() {
                println!("  (nenhum)");
            } else {
                for m in &members {
                    println!("  {}", m);
                }
            }
        }
        return Ok(());
    }
    if let Some(user) = add {
        group_system::group_add_member(name, user).await?;
    }
    if let Some(user) = remove {
        group_system::group_remove_member(name, user).await?;
    }
    if cfg.json_output {
        output::json(&GroupMembersResult {
            group: name.into(),
            added: add.map(str::to_string),
            removed: remove.map(str::to_string),
        })?;
    } else {
        output::ok(format!("membros de '{}' atualizados", name));
        if let Some(u) = add {
            println!("  adicionado:  {}", u);
        }
        if let Some(u) = remove {
            println!("  removido:    {}", u);
        }
    }
    Ok(())
}

pub async fn cmd_chmod(name: &str, perms: &str) -> Result<()> {
    let cfg = Config::from_env()?;
    let sector = group_system::sector_path(&cfg.storage_base, name);
    if !sector.exists() {
        return Err(GarError::user(format!(
            "setor do grupo ausente: {}",
            sector.display()
        )));
    }
    let mode = parse_mode(perms)?;

    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&sector, PermissionsExt::from_mode(mode))?;
    // Persist into permissions file so `gar group permissions` reflects it.
    let perms_path = group_system::permissions_path(&sector);
    let mut current = GroupPermissions::load(&perms_path).unwrap_or_default();
    current.mode = format!("{:o}", mode & 0o7777);
    current.save(&perms_path)?;

    if cfg.json_output {
        output::json(&GroupChmodResult {
            group: name.into(),
            mode: format!("{:o}", mode & 0o7777),
        })?;
    } else {
        output::ok(format!("modo do grupo '{}' atualizado", name));
        println!("  modo: {:o}", mode & 0o7777);
    }
    Ok(())
}

pub async fn cmd_permissions(name: &str) -> Result<()> {
    let cfg = Config::from_env()?;
    let sector = group_system::sector_path(&cfg.storage_base, name);
    if !sector.exists() {
        return Err(GarError::user(format!(
            "setor do grupo ausente: {}",
            sector.display()
        )));
    }
    let perms_path = group_system::permissions_path(&sector);
    let members = group_system::group_members(name);
    let perms = if perms_path.exists() {
        GroupPermissions::load(&perms_path)?
    } else {
        GroupPermissions {
            mode: "0750".into(),
            members: members.clone(),
            extra: Default::default(),
        }
    };
    if cfg.json_output {
        output::json(&GroupPermissionsResult {
            group: name.into(),
            mode: perms.mode,
            members,
            extras: perms.extra,
        })?;
    } else {
        output::section(&format!("Permissões do grupo '{}'", name));
        println!("  setor: {}", sector.display());
        println!("  modo:  {}", perms.mode);
        println!("  membros:");
        if members.is_empty() {
            println!("    (nenhum)");
        } else {
            for m in &members {
                println!("    {}", m);
            }
        }
        if !perms.extra.is_empty() {
            println!("  extras:");
            for (k, v) in &perms.extra {
                println!("    {}={}", k, v);
            }
        }
    }
    Ok(())
}

pub async fn cmd_ensure_defaults() -> Result<()> {
    let cfg = Config::from_env()?;
    let defaults: &[(&str, &str, &str)] = &[
        ("admin", "RAGOS administrators", "10G"),
        ("users", "Default user sector", "1T"),
        ("lab", "Laboratory sector", "500G"),
    ];
    let mut created = Vec::new();
    let mut already = Vec::new();
    for (name, description, quota) in defaults {
        if group_system::group_exists(name) {
            already.push(name.to_string());
            continue;
        }
        cmd_add(name, Some(description), Some(quota)).await?;
        created.push(name.to_string());
    }
    if cfg.json_output {
        output::json(&GroupEnsureDefaultsResult {
            created,
            already_existed: already,
        })?;
    } else {
        output::ok("default groups ensured");
        if !created.is_empty() {
            println!("  criados:    {}", created.join(", "));
        }
        if !already.is_empty() {
            println!("  preexistente: {}", already.join(", "));
        }
    }
    Ok(())
}

fn parse_mode(s: &str) -> Result<u32> {
    let s = s.trim().trim_start_matches('0');
    let v = u32::from_str_radix(s, 8)
        .map_err(|_| GarError::invalid_argument(format!("modo invalido (octal): {}", s)))?;
    if v > 0o7777 {
        return Err(GarError::invalid_argument(format!(
            "modo fora do range (max 7777): {:o}",
            v
        )));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mode_basic() {
        assert_eq!(parse_mode("750").unwrap(), 0o750);
        assert_eq!(parse_mode("0750").unwrap(), 0o750);
        assert_eq!(parse_mode("  777 ").unwrap(), 0o777);
    }

    #[test]
    fn test_parse_mode_rejects_non_octal() {
        assert!(parse_mode("999").is_err()); // 9 is not octal
        assert!(parse_mode("0o750").is_err());
    }

    #[test]
    fn test_parse_mode_rejects_overflow() {
        assert!(parse_mode("10000").is_err());
    }

    #[test]
    fn test_add_result_serializes() {
        let r = GroupAddResult {
            name: "lab".into(),
            description: "Lab sector".into(),
            gid: 1500,
            storage: "/srv/data/storage/lab".into(),
            quota: "100G".into(),
            catalog_updated: true,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"name\":\"lab\""));
        assert!(json.contains("\"gid\":1500"));
    }
}
