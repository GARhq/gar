//! `gar client` subcommand — client diagnostics.

use owo_colors::OwoColorize;
use serde::Serialize;

use crate::cli::ClientCmd;
use crate::config::Config;
use crate::error::Result;
use crate::output;
use crate::services::client::{self, ClientSessionReport};

pub async fn dispatch(cmd: ClientCmd) -> Result<()> {
    match cmd {
        ClientCmd::SessionDoctor => cmd_session_doctor().await,
    }
}

#[derive(Debug, Serialize)]
struct ClientSessionSummary {
    report: ClientSessionReport,
    healthy: bool,
}

pub async fn cmd_session_doctor() -> Result<()> {
    let cfg = Config::from_env()?;
    let inv_path = std::env::var("GAR_INVENTORY_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| client::default_inventory_path());

    let report = client::collect_report(&cfg.images_root, &inv_path);
    let healthy = report.fail_count == 0;

    if cfg.json_output {
        output::json(&ClientSessionSummary {
            report,
            healthy,
        })?;
    } else {
        output::section("GAR Client Session Doctor");
        println!();

        // Manifest
        match &report.manifest {
            Some(m) => {
                println!("  [{}] cliente/current manifest", "OK".green().bold());
                println!("        id:        {}", m.id);
                if !m.target.is_empty() {
                    println!("        target:    {}", m.target);
                }
                if !m.channel.is_empty() {
                    println!("        canal:     {}", m.channel);
                }
                if !m.timestamp.is_empty() {
                    println!("        timestamp: {}", m.timestamp);
                }
                if !m.status.is_empty() {
                    println!("        status:    {}", m.status);
                }
            }
            None => {
                println!(
                    "  [{}] cliente/current manifest: indisponivel",
                    "GAP".yellow().bold()
                );
                println!(
                    "        esperado em: {}/current/manifest.json",
                    cfg.images_root.display()
                );
            }
        }
        println!();

        // NFS exports
        println!("  nfs exports:");
        if report.nfs_exports.is_empty() {
            println!("    (exportfs nao retornou dados — pode nao ter nfs-server rodando)");
        } else {
            for line in report.nfs_exports.lines() {
                println!("    {}", line);
            }
        }
        println!();

        // Inventory
        println!("  inventario:");
        println!("    arquivo: {}", report.inventory_path);
        if report.inventory.is_empty() {
            println!("    (vazio ou nao encontrado)");
        } else {
            for line in report.inventory.lines() {
                println!("    {}", line);
            }
        }
        println!();

        if healthy {
            output::ok(format!(
                "client session OK ({}/{} checks passed)",
                report.ok_count,
                report.ok_count + report.fail_count
            ));
        } else {
            output::warn(format!(
                "client session com gaps ({}/{} checks OK, {} missing)",
                report.ok_count,
                report.ok_count + report.fail_count,
                report.fail_count
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summary_has_healthy_field() {
        let tmp_img = std::env::temp_dir().join(format!("gar-client-cmd-img-{}", std::process::id()));
        std::fs::create_dir_all(&tmp_img).unwrap();
        let tmp_inv = std::env::temp_dir().join(format!("gar-client-cmd-inv-{}", std::process::id()));
        std::fs::write(&tmp_inv, "").unwrap();

        let r = client::collect_report(&tmp_img, &tmp_inv);
        let healthy = r.fail_count == 0;
        let json = serde_json::to_string(&ClientSessionSummary {
            report: r,
            healthy,
        })
        .unwrap();
        assert!(json.contains("\"healthy\""));

        std::fs::remove_dir_all(&tmp_img).unwrap();
        std::fs::remove_file(&tmp_inv).unwrap();
    }
}
