use crate::error::Result;
use crate::output;
use crate::services::client::{json_inventory_path, list_clients, ClientListReport};

/// `gar client list` — enumerate known clients from the JSON inventory.
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
        output::header("GAR Client List");
        println!();
        output::kv("source", source.clone());
        if count == 0 {
            output::info("(nenhum cliente encontrado — esperado até a Phase 0.5 migrar inventory .nix → .json)");
            output::info("Phase 0.5: migração inventory .nix → /etc/gar/inventory/clients.json");
        } else {
            let rows: Vec<Vec<String>> = report
                .clients
                .iter()
                .map(|c| {
                    vec![
                        c.mac.clone(),
                        c.ip.clone(),
                        c.hostname.clone(),
                        format!("{:?}", c.status),
                    ]
                })
                .collect();
            output::table(&["MAC", "IP", "HOSTNAME", "STATUS"], &rows);
            output::ok(format!("{count} cliente(s) listado(s)"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_list_empty_json() {
        let tmp = std::env::temp_dir().join(format!("gar-cmd-list-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        std::env::set_var("GAR_CLIENT_INVENTORY_PATH", &tmp);
        let r = tokio_test_runtime().block_on(cmd_list(true));
        std::env::remove_var("GAR_CLIENT_INVENTORY_PATH");
        assert!(r.is_ok(), "cmd_list empty should succeed");
    }

    fn tokio_test_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime")
    }
}
