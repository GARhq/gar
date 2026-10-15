use crate::config::Config;
use crate::error::Result;
use crate::output;
use crate::services::client::{self, ClientSessionReport};
use serde::Serialize;

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
        output::json(&ClientSessionSummary { report, healthy })?;
    } else {
        output::header("GAR Client Session Doctor");
        println!();

        // Manifest
        match &report.manifest {
            Some(m) => {
                output::ok("cliente/current manifest");
                if !m.id.is_empty() {
                    output::kv("id", m.id.clone());
                }
                if !m.target.is_empty() {
                    output::kv("target", m.target.clone());
                }
                if !m.channel.is_empty() {
                    output::kv("canal", m.channel.clone());
                }
                if !m.timestamp.is_empty() {
                    output::kv("timestamp", m.timestamp.clone());
                }
                if !m.status.is_empty() {
                    output::kv("status", m.status.clone());
                }
            }
            None => {
                output::warn("cliente/current manifest indisponível");
                output::kv(
                    "esperado em",
                    format!("{}/current/manifest.json", cfg.images_root.display()),
                );
            }
        }
        println!();

        // NFS exports
        if report.nfs_exports.is_empty() {
            output::kv("nfs exports", "(exportfs não retornou dados)");
        } else {
            output::kv("nfs exports", "");
            for line in report.nfs_exports.lines() {
                output::kv("", line);
            }
        }
        println!();

        // Inventory
        output::kv("inventário arquivo", &report.inventory_path);
        if report.inventory.is_empty() {
            output::kv("inventário conteúdo", "(vazio ou não encontrado)");
        } else {
            for line in report.inventory.lines() {
                output::kv("", line);
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
        let tmp_img =
            std::env::temp_dir().join(format!("gar-client-cmd-img-{}", std::process::id()));
        std::fs::create_dir_all(&tmp_img).unwrap();
        let tmp_inv =
            std::env::temp_dir().join(format!("gar-client-cmd-inv-{}", std::process::id()));
        std::fs::write(&tmp_inv, "").unwrap();

        let r = client::collect_report(&tmp_img, &tmp_inv);
        let healthy = r.fail_count == 0;
        let json = serde_json::to_string(&ClientSessionSummary { report: r, healthy }).unwrap();
        assert!(json.contains("\"healthy\""));

        std::fs::remove_dir_all(&tmp_img).unwrap();
        std::fs::remove_file(&tmp_inv).unwrap();
    }
}
