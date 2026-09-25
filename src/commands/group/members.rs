use serde::Serialize;
use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::group_system;

#[derive(Debug, Serialize)]
pub struct GroupMembersResult {
    pub group: String,
    pub added: Option<String>,
    pub removed: Option<String>,
}

pub async fn cmd_members(name: &str, add: Option<&str>, remove: Option<&str>) -> Result<()> {
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
