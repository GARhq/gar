//! `gar image status` — show active generation status.
//!
//! Replaces `ragc status` (commands/status.sh, 74 LOC).
//! Shows current version, URLs, channel pointers.

use std::path::Path;

use serde::Serialize;

use crate::config::Config;
use crate::error::Result;
use crate::output;
use crate::services::discovery;
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
    let previous = discovery::read_pointer(&cfg.images_root, "previous");
    let rescue = discovery::read_pointer(&cfg.images_root, "rescue");
    let staged = discovery::read_pointer(&cfg.images_root, "staged");

    let channels: Vec<ChannelPointer> = CHANNEL_POINTERS
        .iter()
        .map(|(ch, cur, prev, stg)| ChannelPointer {
            channel: (*ch).into(),
            current: discovery::read_pointer(&cfg.images_root, cur),
            previous: discovery::read_pointer(&cfg.images_root, prev),
            staged: discovery::read_pointer(&cfg.images_root, stg),
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
    output::header("Status GAR Image");
    output::blank_line();

    output::kv("Servidor", cfg.server_ip.clone());
    output::kv("HTTP Port", cfg.http_port.to_string());
    output::kv("Imagens", cfg.images_root.display().to_string());
    output::kv("HTTP Root", cfg.http_root.display().to_string());
    output::blank_line();

    if let Some(cur) = &report.current {
        output::kv("Versão ativa", cur.version.clone());
        output::kv("Target ativo", cur.target.clone());
        output::kv("Canal ativo", cur.channel.clone());
        output::kv("Classe HW", cur.hardware_class.clone());
        output::kv("Timestamp", cur.timestamp.clone());
        output::kv("System path", cur.system_path.clone());
        output::blank_line();
        output::kv(
            "Kernel URL",
            format!(
                "http://{}:{}/netboot/current/bzImage",
                cfg.server_ip, cfg.http_port
            ),
        );
        output::kv(
            "Initrd URL",
            format!(
                "http://{}:{}/netboot/current/initrd",
                cfg.server_ip, cfg.http_port
            ),
        );
        output::kv(
            "iPXE URL",
            format!("http://{}:{}/boot.ipxe", cfg.server_ip, cfg.http_port),
        );
    } else {
        output::warn("Nenhuma versão ativa. Execute: gar image build");
    }

    if let Some(prev) = &report.previous {
        output::blank_line();
        output::kv("Versão ant.", prev.clone());
    }
    if let Some(rsc) = &report.rescue {
        output::kv("Versão rescue", rsc.clone());
    }
    if let Some(stg) = &report.staged {
        output::kv("Versão staged", stg.clone());
    }

    output::blank_line();
    let channel_rows = build_channel_rows(&report.channels);
    if !channel_rows.is_empty() {
        output::table(&["CANAL", "POINTER", "VERSÃO"], &channel_rows);
        output::blank_line();
    }

    let count = count_versions(&cfg.images_root);
    output::info(format!("Versões armazenadas: {count}"));

    Ok(())
}

/// Flatten per-channel pointers into rows for `output::table`. Only
/// non-empty slots are emitted so the table never shows blank cells.
fn build_channel_rows(channels: &[ChannelPointer]) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for ch in channels {
        for (label, slot) in [
            ("current", &ch.current),
            ("previous", &ch.previous),
            ("staged", &ch.staged),
        ] {
            if let Some(v) = slot.as_ref() {
                rows.push(vec![ch.channel.clone(), (*label).into(), (*v).clone()]);
            }
        }
    }
    rows
}

fn read_pointer_status(images_root: &Path, name: &str) -> Result<Option<CurrentStatus>> {
    let ver = match discovery::read_pointer(images_root, name) {
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
    discovery::iter_version_dirs(images_root)
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
        assert!(discovery::read_pointer(&tmp, "current").is_none());
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
