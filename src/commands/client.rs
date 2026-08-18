//! `gar client` subcommand — client diagnostics.

use owo_colors::OwoColorize;
use serde::Serialize;

use crate::cli::ClientCmd;
use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::client::{
    self, build_magic_packet, json_inventory_path, list_clients, normalize_mac, send_wol,
    ClientListReport, ClientSessionReport,
};

pub async fn dispatch(cmd: ClientCmd) -> Result<()> {
    match cmd {
        ClientCmd::SessionDoctor => cmd_session_doctor().await,
        ClientCmd::List { json } => cmd_list(json).await,
        ClientCmd::Wake {
            mac,
            port,
            count,
            broadcast,
            json,
        } => cmd_wake(&mac, port, count, broadcast.as_deref(), json).await,
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
        output::json(&ClientSessionSummary { report, healthy })?;
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

/// `gar client list` — enumerate known clients from the JSON inventory.
///
/// Phase 0.5 will migrate `/etc/ragos-inventory/clients.nix` → JSON.
/// Until then, an empty list with a clear message is the expected
/// output on a fresh system.
pub async fn cmd_list(json_flag: bool) -> Result<()> {
    let inv_path = std::env::var("GAR_CLIENT_INVENTORY_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| json_inventory_path());

    let clients = list_clients(&inv_path);
    let source = inv_path.display().to_string();
    let count = clients.len();
    let report = ClientListReport {
        clients,
        count,
        source: source.clone(),
    };

    if json_flag {
        output::json(&report)?;
    } else {
        output::section("GAR Client List");
        println!();
        println!("  source: {}", source);
        if count == 0 {
            println!(
                "  (nenhum cliente encontrado — esperado até a Phase 0.5 migrar inventory .nix → .json)"
            );
            output::info("Phase 0.5: migração inventory .nix → /etc/gar/inventory/clients.json");
        } else {
            println!("  {:<17}  {:<15}  {:<16}  status", "mac", "ip", "hostname");
            println!("  {:-<17}  {:-<15}  {:-<16}  ------", "", "", "");
            for c in &report.clients {
                println!(
                    "  {:<17}  {:<15}  {:<16}  {:?}",
                    c.mac, c.ip, c.hostname, c.status
                );
            }
            output::ok(format!("{} cliente(s) listado(s)", count));
        }
    }
    Ok(())
}

/// JSON summary emitted by `gar client wake --json`.
#[derive(Debug, Serialize)]
struct WakeResult {
    mac: String,
    port: u16,
    count_requested: u8,
    broadcast: String,
    packets_sent: usize,
    status: &'static str,
}

/// `gar client wake <mac>` — send Wake-on-LAN magic packet(s).
///
/// Returns `GarError::InvalidArgument` (exit 2) on bad MAC format,
/// `GarError::RuntimeGuard` (exit 1) on socket/send failure.
pub async fn cmd_wake(
    mac: &str,
    port: u16,
    count: u8,
    broadcast: Option<&str>,
    json_flag: bool,
) -> Result<()> {
    let canonical = normalize_mac(mac).map_err(GarError::invalid_argument)?;
    let bc = broadcast.unwrap_or("255.255.255.255").to_string();

    // Quick dry-build so we fail fast with a clear message before touching sockets.
    let _ = build_magic_packet(&canonical).map_err(GarError::invalid_argument)?;

    let sent = send_wol(&canonical, port, count, &bc)
        .map_err(|e| GarError::runtime_guard(format!("WOL send failed: {}", e)))?;

    let result = WakeResult {
        mac: canonical.clone(),
        port,
        count_requested: count,
        broadcast: bc.clone(),
        packets_sent: sent,
        status: "ok",
    };

    if json_flag {
        output::json(&result)?;
    } else {
        output::ok(format!(
            "WOL: {} packets sent to MAC {} via {}:{}",
            sent, canonical, bc, port
        ));
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

    // ----- KCR-001: list_clients + wake -----

    #[test]
    fn test_cmd_list_empty_json() {
        let tmp = std::env::temp_dir().join(format!("gar-cmd-list-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        std::env::set_var("GAR_CLIENT_INVENTORY_PATH", &tmp);
        let r = tokio_test_runtime().block_on(cmd_list(true));
        std::env::remove_var("GAR_CLIENT_INVENTORY_PATH");
        assert!(r.is_ok(), "cmd_list empty should succeed");
    }

    #[test]
    fn test_cmd_wake_rejects_bad_mac() {
        let r = tokio_test_runtime().block_on(cmd_wake("not-a-mac", 9, 1, None, false));
        assert!(r.is_err(), "bad MAC must error");
    }

    // We don't actually broadcast in unit tests (CI may block UDP).
    // Instead, exercise the whole pipeline including socket open on the
    // limited broadcast — this works on Linux loopback/broadcast-capable
    // hosts but is marked #[ignore] so default `cargo test` stays green.
    #[test]
    #[ignore = "requires SO_BROADCAST-capable interface; run with --ignored"]
    fn test_cmd_wake_loopback_send() {
        let r = tokio_test_runtime().block_on(cmd_wake(
            "aa:bb:cc:dd:ee:ff",
            9,
            1,
            Some("127.0.0.1"),
            true,
        ));
        assert!(r.is_ok(), "loopback wake should succeed on Linux");
    }

    /// Minimal single-threaded tokio runtime — saves a `dev-dependency` on
    /// `tokio` macros just for these tests.
    fn tokio_test_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime")
    }
}
