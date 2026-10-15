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

    // Live server pointers: which versions are current/previous/staged/rescue.
    let pointers = Pointers::collect(&cfg.images_root);

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
        let m = manifest::read(&path).ok();
        let (timestamp, target, channel, hardware_class, status) = match m {
            Some(m) => (m.timestamp, m.target, m.channel, m.hardware_class, m.status),
            None => manifest_unknown_row(),
        };

        // Pointers (current/previous/staged/rescue) are more authoritative
        // than the manifest's own `status` field — they reflect what the
        // server is actually serving right now.
        let status = pointer_status(&version, &pointers).unwrap_or(status);

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

/// Snapshot of the four top-level generation pointers.
struct Pointers {
    current: Option<String>,
    previous: Option<String>,
    staged: Option<String>,
    rescue: Option<String>,
}

impl Pointers {
    fn collect(images_root: &Path) -> Self {
        Self {
            current: read_pointer(images_root, "current"),
            previous: read_pointer(images_root, "previous"),
            staged: read_pointer(images_root, "staged"),
            rescue: read_pointer(images_root, "rescue"),
        }
    }

    /// Returns the most authoritative status for `version` based on which
    /// pointer (if any) currently points to it. `None` means the version
    /// is not pointed to by any of the live pointers — caller should fall
    /// back to the manifest's own status.
    fn status_for(&self, version: &str) -> Option<Status> {
        if self.current.as_deref() == Some(version) {
            Some(Status::Active)
        } else if self.previous.as_deref() == Some(version) {
            Some(Status::Previous)
        } else if self.staged.as_deref() == Some(version) {
            Some(Status::Staged)
        } else if self.rescue.as_deref() == Some(version) {
            Some(Status::Rescue)
        } else {
            None
        }
    }
}

/// Resolve a `Status` for `version` using the live pointers, falling back
/// to the manifest's recorded status when the version is not currently
/// pointed to.
fn pointer_status(version: &str, pointers: &Pointers) -> Option<Status> {
    pointers.status_for(version)
}

/// Placeholder row used when a `v*` directory has no readable manifest.
fn manifest_unknown_row() -> (String, String, String, String, Status) {
    (
        "—".into(),
        "—".into(),
        "—".into(),
        "—".into(),
        Status::Inactive,
    )
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
