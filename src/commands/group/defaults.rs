use crate::config::Config;
use crate::error::Result;
use crate::output;
use crate::services::group_system;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct GroupEnsureDefaultsResult {
    pub created: Vec<String>,
    pub already_existed: Vec<String>,
}

pub async fn cmd_ensure_defaults() -> Result<()> {
    let cfg = Config::from_env()?;
    let defaults: &[(&str, &str, &str)] = &[
        ("admin", "GAROS administrators", "10G"),
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
        let result = super::core::add_group(&cfg, name, Some(description), Some(quota)).await?;
        if result.catalog_updated {
            created.push(name.to_string());
        } else {
            already.push(name.to_string());
        }
    }
    if cfg.json_output {
        output::json(&GroupEnsureDefaultsResult {
            created,
            already_existed: already,
        })?;
    } else {
        output::ok("default groups ensured");
        if !created.is_empty() {
            output::kv("criados", created.join(", "));
        }
        if !already.is_empty() {
            output::kv("preexistentes", already.join(", "));
        }
    }
    Ok(())
}
