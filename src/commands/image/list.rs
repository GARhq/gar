//! `gar image list` — list all published generations.
//!
//! Replaces `ragc list` (commands/list.sh, 59 LOC).
//! Reads `images_root/v*/manifest.json` and displays a colored table.

use std::path::Path;

use serde::Serialize;

use crate::config::Config;
use crate::error::Result;
use crate::output;
use crate::services::discovery;
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

    for path in discovery::iter_version_dirs(&cfg.images_root)? {
        let version = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
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

    output::header(format!("Versões em {}", cfg.images_root.display()));
    output::blank_line();

    if rows.is_empty() {
        output::warn("Nenhuma versão encontrada.");
        return Ok(());
    }

    let table_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            vec![
                r.version.clone(),
                r.size.clone(),
                r.timestamp.clone(),
                r.target.clone(),
                r.channel.clone(),
                r.hardware_class.clone(),
                r.status.clone(),
            ]
        })
        .collect();
    output::table(
        &[
            "VERSION",
            "SIZE",
            "TIMESTAMP",
            "TARGET",
            "CHANNEL",
            "HW",
            "STATUS",
        ],
        &table_rows,
    );

    output::blank_line();
    output::info(format!("Total: {} versão(ões)", rows.len()));

    Ok(())
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
            current: discovery::read_pointer(images_root, "current"),
            previous: discovery::read_pointer(images_root, "previous"),
            staged: discovery::read_pointer(images_root, "staged"),
            rescue: discovery::read_pointer(images_root, "rescue"),
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
        assert!(discovery::read_pointer(&tmp, "current").is_none());
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
