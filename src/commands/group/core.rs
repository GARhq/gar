use chrono::Utc;
use owo_colors::OwoColorize;
use serde::Serialize;

use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::group_system::{self, CatalogEntry, GroupCatalog, QuotaSpec};

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
        return report_existing(name, &cfg);
    }
    let description = description.unwrap_or("GAROS Group");
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
            Some(e) => (
                e.description.clone(),
                e.storage_path.clone(),
                e.quota.clone(),
                e.gid,
            ),
            None => (
                "GAROS Group".into(),
                group_system::sector_path(&cfg.storage_base, name)
                    .display()
                    .to_string(),
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
        output::ok(format!(
            "grupo permanente '{}' ja existe (idempotente)",
            name
        ));
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
                Some(e) => (e.description.clone(), e.quota.clone(), e.gid, ()),
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

#[cfg(test)]
mod tests {
    use super::*;

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
