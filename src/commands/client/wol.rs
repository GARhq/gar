use serde::Serialize;
use crate::error::{GarError, Result};
use crate::output;
use crate::services::client::{build_magic_packet, normalize_mac, send_wol};

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
pub async fn cmd_wake(
    mac: &str,
    port: u16,
    count: u8,
    broadcast: Option<&str>,
    json_flag: bool,
) -> Result<()> {
    let canonical = normalize_mac(mac).map_err(GarError::invalid_argument)?;
    let bc = broadcast.unwrap_or("255.255.255.255").to_string();

    // Quick dry-build so we fail fast
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
    fn test_cmd_wake_rejects_bad_mac() {
        let r = tokio_test_runtime().block_on(cmd_wake("not-a-mac", 9, 1, None, false));
        assert!(r.is_err(), "bad MAC must error");
    }

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

    fn tokio_test_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime")
    }
}
