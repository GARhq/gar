use serde::Serialize;
use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::group_system::{self, GroupPermissions};

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
}
