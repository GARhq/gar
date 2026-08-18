//! Client diagnostics — image manifest, NFS exports, inventory.
//!
//! Read-only inspection of what an incoming client would see on the network:
//! the current published image manifest, active NFS exports, and the
//! GAROS client inventory.
//!
//! Also exposes client *enumeration* (`list_clients`) and Wake-on-LAN
//! (`send_wol` + `build_magic_packet`) used by the `gar client list` and
//! `gar client wake` subcommands. These functions are the contract surface
//! for the `garos-control-web` Adapter (corporate panel).
//!
//! Inspired by `cmd_client_session_doctor` in `server/ragos-cli.nix`
//! (11 lines of bash). All operations are best-effort: missing files
//! produce empty sections rather than errors, so the doctor can run
//! in CI sandboxes without runtime dependencies.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Active image manifest under `<images_root>/current/manifest.json`.
/// Only the `id` field is required; everything else is passed through
/// for future use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientManifest {
    pub id: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub status: String,
}

/// Resolves and parses the current client image manifest.
///
/// Returns `Ok(None)` if the manifest file does not exist (CI / fresh host).
/// Returns `Err` only on parse errors — corrupted manifest is a real failure.
pub fn current_manifest(images_root: &Path) -> Result<Option<ClientManifest>, String> {
    let path = images_root.join("current").join("manifest.json");
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("read {}: {}", path.display(), e))?;
    let m: ClientManifest =
        serde_json::from_slice(&bytes).map_err(|e| format!("parse {}: {}", path.display(), e))?;
    Ok(Some(m))
}

/// Runs `exportfs -v` and returns its stdout. Empty string if the binary is missing.
pub fn nfs_exports() -> String {
    let Ok(out) = std::process::Command::new("exportfs").arg("-v").output() else {
        return String::new();
    };
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Reads the GAROS inventory file at the canonical path.
pub fn inventory_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Canonical inventory path (`/etc/ragos-inventory/clients.nix` by default).
pub fn default_inventory_path() -> PathBuf {
    PathBuf::from("/etc/ragos-inventory/clients.nix")
}

/// Aggregate report for `gar client session-doctor`.
#[derive(Debug, Serialize)]
pub struct ClientSessionReport {
    pub manifest: Option<ClientManifest>,
    pub manifest_available: bool,
    pub nfs_exports: String,
    pub inventory: String,
    pub inventory_path: String,
    pub ok_count: usize,
    pub fail_count: usize,
}

/// Run all client checks and assemble a report.
///
/// `images_root` and `inventory_path` are passed in so the function is
/// pure and testable (no hardcoded paths leaking into CI).
pub fn collect_report(images_root: &Path, inventory_path: &Path) -> ClientSessionReport {
    let manifest = match current_manifest(images_root) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("gar client session-doctor: manifest parse failed: {}", e);
            None
        }
    };
    let manifest_available = manifest.is_some();
    let nfs_exports = nfs_exports();
    let inventory = inventory_text(inventory_path);
    let inventory_path_str = inventory_path.display().to_string();

    let mut ok = 0usize;
    let mut fail = 0usize;
    if manifest_available {
        ok += 1;
    } else {
        fail += 1;
    }
    if !nfs_exports.is_empty() {
        ok += 1;
    } else {
        fail += 1;
    }
    if !inventory.is_empty() {
        ok += 1;
    } else {
        fail += 1;
    }

    ClientSessionReport {
        manifest,
        manifest_available,
        nfs_exports,
        inventory,
        inventory_path: inventory_path_str,
        ok_count: ok,
        fail_count: fail,
    }
}

// ----------------------------------------------------------------------------
// Client enumeration + Wake-on-LAN (contract surface for garos-control-web)
// ----------------------------------------------------------------------------

/// Status string used by `gar client list` JSON payload.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClientStatus {
    Online,
    Offline,
    Unknown,
}

impl ClientStatus {
    /// Map a free-form status string into the canonical enum. Public so
    /// other crates (or future ingestion helpers) can reuse the same
    /// normalization without re-implementing the vocabulary.
    #[allow(dead_code)] // exposed for future inventory parsers; not yet wired into a path
    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "online" | "up" | "alive" => Self::Online,
            "offline" | "down" | "dead" => Self::Offline,
            _ => Self::Unknown,
        }
    }
}

/// One row of `gar client list` — Adapter contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientRecord {
    pub mac: String,
    #[serde(default)]
    pub ip: String,
    #[serde(default)]
    pub hostname: String,
    pub status: ClientStatus,
}

/// Aggregate list response — matches `gar client list --json` schema.
#[derive(Debug, Serialize, Deserialize)]
pub struct ClientListReport {
    pub clients: Vec<ClientRecord>,
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub source: String,
}

/// Preferred canonical JSON inventory path (Phase 0.5 migration target).
pub fn json_inventory_path() -> PathBuf {
    PathBuf::from("/etc/gar/inventory/clients.json")
}

/// Read clients from JSON inventory. Returns empty Vec (no error) if file
/// is missing — the migration from `.nix` to `.json` is Phase 0.5, not
/// blocking the Adapter.
pub fn list_clients(json_path: &Path) -> Vec<ClientRecord> {
    if !json_path.exists() {
        return Vec::new();
    }
    let Ok(bytes) = std::fs::read(json_path) else {
        return Vec::new();
    };
    // Accept either `{ "clients": [...] }` or bare `[...]`.
    match serde_json::from_slice::<Vec<ClientRecord>>(&bytes) {
        Ok(v) => v,
        Err(_) => match serde_json::from_slice::<ClientListReport>(&bytes) {
            Ok(r) => r.clients,
            Err(_) => Vec::new(),
        },
    }
}

/// Normalize MAC string to canonical `XX:XX:XX:XX:XX:XX` lowercase form.
/// Accepts `XX:XX:...` or `XX-XX-...` separators.
pub fn normalize_mac(raw: &str) -> Result<String, String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_lowercase();
    if cleaned.len() != 12 {
        return Err(format!(
            "MAC '{}' has {} hex digits, expected 12",
            raw,
            cleaned.len()
        ));
    }
    let bytes: Vec<u8> = (0..6)
        .map(|i| u8::from_str_radix(&cleaned[i * 2..i * 2 + 2], 16))
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| format!("MAC '{}' contains invalid hex: {}", raw, e))?;
    Ok(bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(":"))
}

/// Build a Wake-on-LAN magic packet.
///
/// Layout: 6 bytes `0xFF` followed by the 6-byte MAC repeated 16 times.
/// Total length: 102 bytes.
pub fn build_magic_packet(mac_canonical: &str) -> Result<Vec<u8>, String> {
    let parts: Vec<&str> = mac_canonical.split(':').collect();
    if parts.len() != 6 {
        return Err(format!(
            "MAC '{}' must have 6 octets separated by ':'",
            mac_canonical
        ));
    }
    let mac_bytes: Vec<u8> = parts
        .iter()
        .map(|p| u8::from_str_radix(p, 16))
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| format!("invalid hex octet in '{}': {}", mac_canonical, e))?;

    let mut packet = Vec::with_capacity(102);
    packet.extend_from_slice(&[0xFFu8; 6]);
    for _ in 0..16 {
        packet.extend_from_slice(&mac_bytes);
    }
    Ok(packet)
}

/// Send Wake-on-LAN magic packets. Returns count of packets actually sent.
///
/// `broadcast` defaults to `255.255.255.255` (limited broadcast). Caller
/// may pass a subnet-directed broadcast (e.g. `192.168.1.255`) to avoid
/// router blocking on segmented networks.
pub fn send_wol(
    mac_canonical: &str,
    port: u16,
    count: u8,
    broadcast: &str,
) -> Result<usize, String> {
    use std::net::UdpSocket;

    let packet = build_magic_packet(mac_canonical)?;

    // SO_BROADCAST is required on most platforms to send to a broadcast addr.
    let socket = UdpSocket::bind(("0.0.0.0", 0)).map_err(|e| format!("bind UDP socket: {}", e))?;
    socket
        .set_broadcast(true)
        .map_err(|e| format!("set SO_BROADCAST: {}", e))?;

    let dest = (broadcast, port);
    let mut sent = 0usize;
    for _ in 0..count {
        match socket.send_to(&packet, dest) {
            Ok(n) if n == packet.len() => sent += 1,
            Ok(n) => {
                return Err(format!(
                    "short send to {:?}: {} of {} bytes",
                    dest,
                    n,
                    packet.len()
                ))
            }
            Err(e) => return Err(format!("send to {:?} failed: {}", dest, e)),
        }
    }
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_parses_minimal() {
        let tmp = std::env::temp_dir().join(format!("gar-client-mf-{}", std::process::id()));
        let current = tmp.join("current");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(
            current.join("manifest.json"),
            r#"{"id": "v20260809-120000", "target": "desktop-generic"}"#,
        )
        .unwrap();
        let m = current_manifest(&tmp).unwrap().unwrap();
        assert_eq!(m.id, "v20260809-120000");
        assert_eq!(m.target, "desktop-generic");
        assert_eq!(m.channel, ""); // default
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_manifest_returns_none_when_missing() {
        let tmp =
            std::env::temp_dir().join(format!("gar-client-mf-missing-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let m = current_manifest(&tmp).unwrap();
        assert!(m.is_none());
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_manifest_returns_err_on_corrupted_json() {
        let tmp = std::env::temp_dir().join(format!("gar-client-mf-bad-{}", std::process::id()));
        let current = tmp.join("current");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(current.join("manifest.json"), "not-json-at-all").unwrap();
        let r = current_manifest(&tmp);
        assert!(r.is_err());
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_inventory_text_reads_or_empty() {
        let tmp = std::env::temp_dir().join(format!("gar-client-inv-{}", std::process::id()));
        std::fs::write(&tmp, "{ clients = []; }\n").unwrap();
        assert_eq!(inventory_text(&tmp), "{ clients = []; }\n");
        let missing =
            std::env::temp_dir().join(format!("gar-client-inv-no-{}.nope", std::process::id()));
        assert_eq!(inventory_text(&missing), "");
    }

    #[test]
    fn test_collect_report_counts() {
        let tmp_img =
            std::env::temp_dir().join(format!("gar-client-cr-img-{}", std::process::id()));
        let current = tmp_img.join("current");
        std::fs::create_dir_all(&current).unwrap();
        std::fs::write(current.join("manifest.json"), r#"{"id":"v1"}"#).unwrap();

        let tmp_inv =
            std::env::temp_dir().join(format!("gar-client-cr-inv-{}", std::process::id()));
        std::fs::write(&tmp_inv, "{ clients = []; }\n").unwrap();

        let r = collect_report(&tmp_img, &tmp_inv);
        assert!(r.manifest_available);
        // nfs_exports is best-effort — may be empty in CI; just count it
        assert_eq!(r.ok_count + r.fail_count, 3);

        std::fs::remove_dir_all(&tmp_img).unwrap();
        std::fs::remove_file(&tmp_inv).unwrap();
    }

    #[test]
    fn test_report_serializes_with_all_fields() {
        let tmp_img =
            std::env::temp_dir().join(format!("gar-client-cs-img-{}", std::process::id()));
        std::fs::create_dir_all(&tmp_img).unwrap();
        let tmp_inv =
            std::env::temp_dir().join(format!("gar-client-cs-inv-{}", std::process::id()));
        std::fs::write(&tmp_inv, "").unwrap();
        let r = collect_report(&tmp_img, &tmp_inv);
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("manifest_available"));
        assert!(json.contains("nfs_exports"));
        assert!(json.contains("inventory_path"));
        std::fs::remove_dir_all(&tmp_img).unwrap();
        std::fs::remove_file(&tmp_inv).unwrap();
    }

    // ----- KCR-001: list_clients + WOL -----

    #[test]
    fn test_normalize_mac_accepts_colon_and_dash() {
        assert_eq!(
            normalize_mac("AA:BB:CC:DD:EE:FF").unwrap(),
            "aa:bb:cc:dd:ee:ff"
        );
        assert_eq!(
            normalize_mac("AA-BB-CC-DD-EE-FF").unwrap(),
            "aa:bb:cc:dd:ee:ff"
        );
        assert_eq!(
            normalize_mac("aabb.ccdd.eeff").unwrap(),
            "aa:bb:cc:dd:ee:ff"
        );
    }

    #[test]
    fn test_normalize_mac_rejects_wrong_length() {
        assert!(normalize_mac("AA:BB:CC").is_err());
        assert!(normalize_mac("not-a-mac").is_err());
        assert!(normalize_mac("").is_err());
    }

    #[test]
    fn test_build_magic_packet_layout() {
        let pkt = build_magic_packet("aa:bb:cc:dd:ee:ff").unwrap();
        assert_eq!(pkt.len(), 102);
        // First 6 bytes must be 0xFF.
        assert!(pkt[..6].iter().all(|b| *b == 0xFF));
        // Bytes 6..12 = MAC, repeated 16 times.
        let mac = [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
        for rep in 0..16 {
            let start = 6 + rep * 6;
            assert_eq!(&pkt[start..start + 6], &mac[..]);
        }
    }

    #[test]
    fn test_build_magic_packet_rejects_bad_canonical() {
        assert!(build_magic_packet("aa:bb:cc").is_err());
        assert!(build_magic_packet("zz:bb:cc:dd:ee:ff").is_err());
    }

    #[test]
    fn test_list_clients_returns_empty_when_missing() {
        let path = std::env::temp_dir().join(format!(
            "gar-client-list-missing-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let v = list_clients(&path);
        assert!(v.is_empty());
    }

    #[test]
    fn test_list_clients_parses_bare_array() {
        let path =
            std::env::temp_dir().join(format!("gar-client-list-arr-{}.json", std::process::id()));
        std::fs::write(
            &path,
            r#"[
                {"mac":"aa:bb:cc:dd:ee:01","ip":"192.168.1.10","hostname":"lab-01","status":"online"},
                {"mac":"aa:bb:cc:dd:ee:02","ip":"","hostname":"","status":"unknown"}
            ]"#,
        )
        .unwrap();
        let v = list_clients(&path);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].mac, "aa:bb:cc:dd:ee:01");
        assert_eq!(v[0].status, ClientStatus::Online);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_list_clients_parses_wrapped_object() {
        let path =
            std::env::temp_dir().join(format!("gar-client-list-obj-{}.json", std::process::id()));
        std::fs::write(
            &path,
            r#"{"clients":[
                {"mac":"aa:bb:cc:dd:ee:03","ip":"10.0.0.1","hostname":"srv","status":"offline"}
            ]}"#,
        )
        .unwrap();
        let v = list_clients(&path);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].hostname, "srv");
        assert_eq!(v[0].status, ClientStatus::Offline);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_list_clients_swallows_corrupt_json() {
        let path =
            std::env::temp_dir().join(format!("gar-client-list-bad-{}.json", std::process::id()));
        std::fs::write(&path, "not json at all").unwrap();
        let v = list_clients(&path);
        assert!(v.is_empty());
        std::fs::remove_file(&path).unwrap();
    }
}
