//! `gar image list` — list all published generations.
//!
//! Replaces `ragc list` (commands/list.sh, 59 LOC).
//! Reads `images_root/v*/manifest.json` and displays a colored table.

use std::path::Path;

use owo_colors::OwoColorize;
use serde::Serialize;

use crate::config::Config;
use crate::error::Result;
use crate::output;
use crate::services::manifest::{self, Status};

#[derive(Debug, Serialize)]
struct GenerationRow {
    version: String,
    size: String,
    timestamp: String,
    target: String,
    channel: String,
    hardware_class: String,
    status: String,
}

/// Run the list command.
pub async fn run() -> Result<()> {
    let cfg = Config::from_env()?;

    if !cfg.images_root.exists() {
        output::warn(format!(
            "Nenhuma versão publicada ainda ({} não existe).",
            cfg.images_root.display()
        ));
        return Ok(());
    }

    // Collect pointers
    let current = read_pointer(&cfg.images_root, "current");
    let previous = read_pointer(&cfg.images_root, "previous");
    let rescue = read_pointer(&cfg.images_root, "rescue");
    let staged = read_pointer(&cfg.images_root, "staged");

    let mut rows: Vec<GenerationRow> = Vec::new();

    for entry in std::fs::read_dir(&cfg.images_root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let version = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) if n.starts_with('v') => n.to_string(),
            _ => continue,
        };

        let size = dir_size(&path);
        let (timestamp, target, channel, hardware_class, status) = match manifest::read(&path) {
            Ok(m) => (m.timestamp, m.target, m.channel, m.hardware_class, m.status),
            Err(_) => {
                // Manifest missing — show unknown
                (
                    "—".into(),
                    "—".into(),
                    "—".into(),
                    "—".into(),
                    Status::Inactive,
                )
            }
        };

        // Override status from pointers if present (more authoritative)
        let status = if Some(&version) == current.as_ref() {
            Status::Active
        } else if Some(&version) == previous.as_ref() {
            Status::Previous
        } else if Some(&version) == staged.as_ref() {
            Status::Staged
        } else if Some(&version) == rescue.as_ref() {
            Status::Rescue
        } else {
            status
        };

        rows.push(GenerationRow {
            version: version.clone(),
            size,
            timestamp,
            target,
            channel,
            hardware_class,
            status: format_status(status),
        });
    }

    // Sort by version descending (newest first)
    rows.sort_by(|a, b| b.version.cmp(&a.version));

    if cfg.json_output {
        output::json(&rows)?;
        return Ok(());
    }

    output::section(format!("Versões em {}", cfg.images_root.display()));
    println!();

    if rows.is_empty() {
        output::warn("Nenhuma versão encontrada.");
        return Ok(());
    }

    // Header
    println!(
        "  {:<25} {:<10} {:<22} {:<18} {:<10} {:<18} {}",
        "VERSION".bold(),
        "SIZE".bold(),
        "TIMESTAMP".bold(),
        "TARGET".bold(),
        "CHANNEL".bold(),
        "HW".bold(),
        "STATUS".bold()
    );

    // Rows
    for row in &rows {
        let status_colored = colorize_status(&row.status);
        println!(
            "  {:<25} {:<10} {:<22} {:<18} {:<10} {:<18} {}",
            row.version,
            row.size,
            row.timestamp,
            row.target,
            row.channel,
            row.hardware_class,
            status_colored,
        );
    }

    println!();
    println!("  Total: {} versão(ões)", rows.len());

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

fn dir_size(path: &Path) -> String {
    let output = std::process::Command::new("du")
        .args(["-sh", &path.display().to_string()])
        .output()
        .ok();
    match output {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .split_whitespace()
            .next()
            .unwrap_or("?")
            .to_string(),
        _ => "?".into(),
    }
}

fn format_status(s: Status) -> String {
    match s {
        Status::Active => "active".into(),
        Status::Previous => "previous".into(),
        Status::Staged => "staged".into(),
        Status::Rescue => "rescue".into(),
        Status::Inactive => "inactive".into(),
    }
}

fn colorize_status(s: &str) -> String {
    match s {
        "active" => format!("{} {}", "*".green().bold(), "active".green().bold()),
        "previous" => format!("{} {}", "←".yellow().bold(), "previous".yellow()),
        "rescue" => format!("{} {}", "+".blue().bold(), "rescue".blue()),
        "staged" => format!("{} {}", "↻".magenta().bold(), "staged".magenta()),
        _ => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_status_strings() {
        assert_eq!(format_status(Status::Active), "active");
        assert_eq!(format_status(Status::Previous), "previous");
        assert_eq!(format_status(Status::Rescue), "rescue");
        assert_eq!(format_status(Status::Staged), "staged");
        assert_eq!(format_status(Status::Inactive), "inactive");
    }

    #[test]
    fn test_read_pointer_nonexistent() {
        let tmp = std::env::temp_dir().join(format!("gar-list-pointer-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(read_pointer(&tmp, "current").is_none());
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
