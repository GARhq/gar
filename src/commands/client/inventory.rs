use crate::error::Result;
use crate::output;
use crate::services::client::{json_inventory_path, list_clients, ClientListReport};

/// `gar client list` — enumerate known clients from the JSON inventory.
pub async fn cmd_list(json_flag: bool) -> Result<()> {
    let inv_path = std::env::var("GAR_CLIENT_INVENTORY_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| json_inventory_path());

    let clients = list_clients(&inv_path);
    // GAROS pós-migração usa /etc/garos-inventory/clients.nix. Quando o
    // usuário não sobrescreve via env var, exibimos o path canônico
    // atual em vez do path antigo (clients.json) para não confundir
    // quem lê o output com referências a uma migração que não existe.
    let source = std::env::var("GAR_CLIENT_INVENTORY_PATH")
        .unwrap_or_else(|_| "/etc/garos-inventory/clients.nix".to_string());
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
        output::blank_line();
        output::kv("source", source.clone());
        if count == 0 {
            output::info("(nenhum cliente no inventory — verifique /etc/garos-inventory/clients.nix ou --inventory <path>)");
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
