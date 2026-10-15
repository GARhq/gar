//! `gar image status` — show active generation status.
//!
//! Replaces `ragc status` (commands/status.sh, 74 LOC).
//! Shows current version, URLs, channel pointers.

use std::path::Path;

use serde::Serialize;

use crate::config::Config;
use crate::error::Result;
use crate::output;
use crate::services::manifest;

#[derive(Debug, Serialize)]
pub struct StatusReport {
    pub server_ip: String,
    pub http_port: u16,
    pub images_root: String,
    pub http_root: String,
    pub current: Option<CurrentStatus>,
    pub previous: Option<String>,
    pub rescue: Option<String>,
    pub staged: Option<String>,
    pub channels: Vec<ChannelPointer>,
}

#[derive(Debug, Serialize)]
pub struct CurrentStatus {
    pub version: String,
    pub target: String,
    pub channel: String,
    pub hardware_class: String,
    pub timestamp: String,
    pub system_path: String,
    pub init_path: String,
}

#[derive(Debug, Serialize)]
pub struct ChannelPointer {
    pub channel: String,
    pub current: Option<String>,
    pub previous: Option<String>,
    pub staged: Option<String>,
}

/// `(channel, current-pointer, previous-pointer, staged-pointer)` — the
/// well-known per-channel symlinks rooted at `images_root`.
const CHANNEL_POINTERS: &[(&str, &str, &str, &str)] = &[
    (
        "generic",
        "current-generic",
        "previous-generic",
        "staged-generic",
    ),
    ("lab", "current-lab", "previous-lab", "staged-lab"),
    (
        "rescue",
        "current-rescue",
        "previous-rescue",
        "staged-rescue",
    ),
];

/// Run the status command.
pub async fn run() -> Result<()> {
    let cfg = Config::from_env()?;

    let current = read_pointer_status(&cfg.images_root, "current")?;
    let previous = read_pointer(&cfg.images_root, "previous");
    let rescue = read_pointer(&cfg.images_root, "rescue");
    let staged = read_pointer(&cfg.images_root, "staged");

    let channels: Vec<ChannelPointer> = CHANNEL_POINTERS
        .iter()
        .map(|(ch, cur, prev, stg)| ChannelPointer {
            channel: (*ch).into(),
            current: read_pointer(&cfg.images_root, cur),
            previous: read_pointer(&cfg.images_root, prev),
            staged: read_pointer(&cfg.images_root, stg),
        })
        .collect();

    let report = StatusReport {
        server_ip: cfg.server_ip.clone(),
        http_port: cfg.http_port,
        images_root: cfg.images_root.display().to_string(),
        http_root: cfg.http_root.display().to_string(),
        current,
        previous: previous.clone(),
        rescue: rescue.clone(),
        staged: staged.clone(),
        channels,
    };

    if cfg.json_output {
        output::json(&report)?;
        return Ok(());
    }

    // Human-readable
    output::section("Status GAR Image");
    println!();
    println!("  Servidor  : {}", cfg.server_ip);
    println!("  HTTP Port : {}", cfg.http_port);
    println!("  Imagens   : {}", cfg.images_root.display());
    println!("  HTTP Root : {}", cfg.http_root.display());
    println!();

    if let Some(cur) = &report.current {
        println!("  Versão ativa : {}", cur.version);
        println!("  Target ativo : {}", cur.target);
        println!("  Canal ativo  : {}", cur.channel);
        println!("  Classe HW    : {}", cur.hardware_class);
        println!("  Timestamp    : {}", cur.timestamp);
        println!("  System path  : {}", cur.system_path);
        println!();
        println!(
            "  Kernel URL   : http://{}:{}/netboot/current/bzImage",
            cfg.server_ip, cfg.http_port
        );
        println!(
            "  Initrd URL   : http://{}:{}/netboot/current/initrd",
            cfg.server_ip, cfg.http_port
        );
        println!(
            "  iPXE URL     : http://{}:{}/boot.ipxe",
            cfg.server_ip, cfg.http_port
        );
    } else {
        output::warn("Nenhuma versão ativa. Execute: gar image build");
    }

    if let Some(prev) = &report.previous {
        println!();
        println!("  Versão ant.  : {}", prev);
    }
    if let Some(rsc) = &report.rescue {
        println!("  Versão rescue: {}", rsc);
    }
    if let Some(stg) = &report.staged {
        println!("  Versão staged: {}", stg);
    }

    println!();
    for ch in &report.channels {
        if ch.current.is_some() || ch.previous.is_some() || ch.staged.is_some() {
            print_channel_pointers(ch);
        }
    }

    let count = count_versions(&cfg.images_root);
    println!();
    println!("  Versões armazenadas: {}", count);

    Ok(())
}

fn read_pointer(images_root: &Path, name: &str) -> Option<String> {
    let path = images_root.join(name);
    if path.is_symlink() {
        std::fs::read_link(&path)
            .ok()
            .and_then(|t| t.file_name().map(|n| n.to_string_lossy().into_owned()))
    } else {
        None
    }
}

/// Render the three per-channel pointer lines (current / previous /
/// staged) using a single table-driven pass instead of three
/// open-coded `if let Some(...)` blocks.
fn print_channel_pointers(ch: &ChannelPointer) {
    const POINTER_LABELS: &[&str] = &["current", "previous", "staged"];
    let slots: [&Option<String>; 3] = [&ch.current, &ch.previous, &ch.staged];
    for (slot, label) in slots.iter().zip(POINTER_LABELS.iter()) {
        if let Some(v) = slot.as_ref() {
            println!("  Canal {:<7} {}: {}", ch.channel, label, v);
        }
    }
}

fn read_pointer_status(images_root: &Path, name: &str) -> Result<Option<CurrentStatus>> {
    let ver = match read_pointer(images_root, name) {
        Some(v) => v,
        None => return Ok(None),
    };
    let dir = images_root.join(&ver);
    let m = manifest::read(&dir)?;
    Ok(Some(CurrentStatus {
        version: ver,
        target: m.target,
        channel: m.channel,
        hardware_class: m.hardware_class,
        timestamp: m.timestamp,
        system_path: m.system_path,
        init_path: m.init_path,
    }))
}

fn count_versions(images_root: &Path) -> usize {
    crate::services::discovery::iter_version_dirs(images_root)
        .map(|v| v.len())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_pointer_no_symlink() {
        let tmp =
            std::env::temp_dir().join(format!("gar-status-{}-{}", "test", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        // No symlink exists
        assert!(read_pointer(&tmp, "current").is_none());
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_count_versions() {
        let tmp = std::env::temp_dir().join(format!("gar-status-count-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();

        // Create v* directories
        for v in ["v20260101-120000", "v20260102-120000", "not-a-version"] {
            std::fs::create_dir_all(tmp.join(v)).unwrap();
        }

        assert_eq!(count_versions(&tmp), 2); // Only v* counted

        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
