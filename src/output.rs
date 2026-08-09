//! GAR CLI - output formatting (JSON, table, colored text)
//!
//! Centralizes all stdout/stderr formatting so commands stay clean.

use owo_colors::OwoColorize;
use serde::Serialize;

/// Output mode based on --json flag.
#[derive(Debug, Clone, Copy)]
pub enum OutputMode {
    Human,
    Json,
}

impl OutputMode {
    pub fn from_env() -> Self {
        if std::env::var("GAR_JSON_OUTPUT")
            .ok()
            .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false)
        {
            Self::Json
        } else {
            Self::Human
        }
    }

    pub fn is_json(self) -> bool {
        matches!(self, OutputMode::Json)
    }
}

/// Print a success message (green [OK]).
pub fn ok(msg: impl AsRef<str>) {
    println!("{} {}", "[OK]".green().bold(), msg.as_ref());
}

/// Print an info message (blue [INFO]).
pub fn info(msg: impl AsRef<str>) {
    println!("{} {}", "[INFO]".blue().bold(), msg.as_ref());
}

/// Print a warning (yellow [AVISO]).
pub fn warn(msg: impl AsRef<str>) {
    eprintln!("{} {}", "[AVISO]".yellow().bold(), msg.as_ref());
}

/// Print an error (red [ERRO]).
pub fn err(msg: impl AsRef<str>) {
    eprintln!("{} {}", "[ERRO]".red().bold(), msg.as_ref());
}

/// Print a section header (bold).
pub fn section(title: impl AsRef<str>) {
    println!("\n{}", title.as_ref().bold());
}

/// Print JSON value to stdout (for --json mode).
pub fn json<T: Serialize>(value: &T) -> crate::error::Result<()> {
    let s = serde_json::to_string_pretty(value)?;
    println!("{}", s);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_mode_detection() {
        std::env::remove_var("GAR_JSON_OUTPUT");
        assert!(!OutputMode::from_env().is_json());

        std::env::set_var("GAR_JSON_OUTPUT", "1");
        assert!(OutputMode::from_env().is_json());

        std::env::remove_var("GAR_JSON_OUTPUT");
    }

    #[test]
    fn test_json_serialize() {
        let v = serde_json::json!({"status": "ok"});
        json(&v).expect("should serialize");
    }
}
