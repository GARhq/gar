use std::os::unix::fs::MetadataExt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::user_system::{self, ClientUsersCatalog};

#[derive(Debug, Serialize)]
struct DoctorReport {
    username: String,
    home: String,
    filesystem: String,
    owner: String,
    mode: String,
    usage_bytes: u64,
    quota: String,
    catalog: String,
    mount: String,
    qgroup: String,
}

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
    let quota = user_system::read_meta_value(&home, "QUOTA").unwrap_or_default();
    let catalog_path = cfg.runtime_root.join("client-users.json");
    let catalog_status = match ClientUsersCatalog::load(&catalog_path).ok() {
        Some(c) if c.get(username).is_some() => "presente",
        _ => "ausente",
    };
    let (owner, mode) = owner_mode(&home);
    let fstype = user_system::fs_type(&home).unwrap_or_else(|| "desconhecido".into());
    let mount = user_system::mount_info(&home).unwrap_or_else(|| "(indisponivel)".into());
    let qgroup = user_system::qgroup_info(&home).unwrap_or_else(|| "(indisponivel)".into());

    if cfg.json_output {
        output::json(&DoctorReport {
            username: username.into(),
            home: home.display().to_string(),
            filesystem: fstype,
            owner,
            mode,
            usage_bytes: usage_b,
            quota,
            catalog: catalog_status.into(),
            mount,
            qgroup,
        })?;
    } else {
        output::section(&format!("Doctor para usuário '{}'", username));
        println!("  home:        {}", home.display());
        println!("  filesystem:  {}", fstype);
        println!("  owner:       {}", owner);
        println!("  modo:        {}", mode);
        println!("  uso:         {}", user_system::bytes_to_human(usage_b));
        println!(
            "  quota:       {}",
            if quota.is_empty() { "—" } else { &quota }
        );
        println!("  catalog_cliente: {}", catalog_status);
        println!("  montagem:");
        for line in mount.lines() {
            println!("    {}", line);
        }
        println!("  qgroup:");
        for line in qgroup.lines() {
            println!("    {}", line);
        }
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct Session {
    timestamp: String,
    action: String,
    tty: String,
    ip: String,
}

pub async fn cmd_activity(username: &str) -> Result<()> {
    let cfg = Config::from_env()?;
    let audit_file = cfg.audit_dir.join("login-history.json");
    if !audit_file.exists() {
        if cfg.json_output {
            let empty: Vec<Session> = vec![];
            return output::json(&empty);
        }
        output::info(format!("sem registro de auditoria para {}", username));
        return Ok(());
    }
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&audit_file)?)?;
    let sessions: Vec<Session> = json
        .get("sessions")
        .and_then(|s| s.get(username))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    if cfg.json_output {
        return output::json(&sessions);
    }
    if sessions.is_empty() {
        output::info(format!("sem sessões registradas para {}", username));
        return Ok(());
    }
    output::section(&format!("Atividade de '{}'", username));
    for s in &sessions {
        println!("  [{}] [{}] tty={} ip={}", s.timestamp, s.action, s.tty, s.ip);
    }
    Ok(())
}

fn owner_mode(path: &Path) -> (String, String) {
    let Ok(m) = std::fs::metadata(path) else {
        return ("?".into(), "?".into());
    };
    let owner = name_from_getent("passwd", m.uid()).unwrap_or_else(|| m.uid().to_string());
    let group = name_from_getent("group", m.gid()).unwrap_or_else(|| m.gid().to_string());
    (
        format!("{}:{}", owner, group),
        format!("{:o}", m.mode() & 0o7777),
    )
}

fn name_from_getent(kind: &str, id: u32) -> Option<String> {
    let output = std::process::Command::new("getent")
        .args([kind, &id.to_string()])
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .split(':')
            .next()
            .unwrap_or("")
            .to_string()
    })
}
