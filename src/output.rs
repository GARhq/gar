//! GAR CLI — output formatting.
//!
//! This module owns every decision about *how* output looks and
//! *where* it goes. Callers should never reach for `println!` or
//! `eprintln!` directly; they pick a [`Level`] and a message, and
//! the module picks the color, symbol, and stream.
//!
//! ## Design contract
//!
//! - **Stdout** is reserved for successful operation output — both
//!   human-readable and JSON. A program that streams JSON must be
//!   safe to pipe.
//! - **Stderr** carries warnings, errors, and the spinner /
//!   progress signals. Stderr is never machine-parsed.
//! - **Color** is automatically disabled when stdout/stderr is not
//!   a TTY (piped to a file, captured by CI, etc.). Callers never
//!   have to think about this.
//! - **Symbols** (✓ ● ▲ ✗) are part of the brand; they are fixed at
//!   module scope and reused everywhere. Callers don't pick them.

use std::io::IsTerminal;

use owo_colors::OwoColorize;
use serde::Serialize;

/// Severity / intent of a single line of output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Operation succeeded.
    Success,
    /// Neutral, contextual information.
    Info,
    /// Recoverable anomaly worth surfacing.
    Warning,
    /// Operation failed (the caller decides whether to exit).
    Error,
}

impl Level {
    /// Symbol that prefixes the line. Stable across releases —
    /// changing these is a breaking change for anyone parsing the
    /// output.
    fn symbol(self) -> &'static str {
        match self {
            Level::Success => "✓",
            Level::Info => "●",
            Level::Warning => "▲",
            Level::Error => "✗",
        }
    }

    /// Apply the level's brand color to the symbol string.
    fn styled_symbol(self) -> String {
        // We honor our own `use_color_for` gate so the rest of the
        // crate doesn't need to thread the decision through. The
        // color helpers short-circuit when stdout isn't a TTY.
        let s = self.symbol();
        if !use_color_for(Stream::Stdout) {
            return s.to_string();
        }
        match self {
            Level::Success => s.green().bold().to_string(),
            Level::Info => s.cyan().bold().to_string(),
            Level::Warning => s.yellow().bold().to_string(),
            Level::Error => s.red().bold().to_string(),
        }
    }

    /// Which stream this level writes to. Warnings and errors go
    /// to stderr so successful JSON output can be piped safely.
    fn stream(self) -> Stream {
        match self {
            Level::Success | Level::Info => Stream::Stdout,
            Level::Warning | Level::Error => Stream::Stderr,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stream {
    Stdout,
    Stderr,
}

fn use_color_for(stream: Stream) -> bool {
    // `NO_COLOR` is the de-facto cross-tool convention.
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    match stream {
        Stream::Stdout => std::io::stdout().is_terminal(),
        Stream::Stderr => std::io::stderr().is_terminal(),
    }
}

/// Internal: render one `<symbol> <message>` line to the right stream.
/// All callers funnel through here so color/symbol/stream live in
/// exactly one place.
fn print_level(level: Level, msg: &str) {
    let line = format!("{} {}", level.styled_symbol(), msg);
    match level.stream() {
        Stream::Stdout => println!("{line}"),
        Stream::Stderr => eprintln!("{line}"),
    }
}

// ─── Top-level helpers (public surface every command uses) ──────────────
//
// These are the legacy-friendly entry points. They are thin
// wrappers around [`print_level`] — every decision about color,
// stream, and symbol lives in one place (`Level` impl above).

/// Print a success message: green ✓ prefix on stdout.
pub fn ok(msg: impl AsRef<str>) {
    print_level(Level::Success, msg.as_ref());
}

/// Print an informational message: cyan ● prefix on stdout.
pub fn info(msg: impl AsRef<str>) {
    print_level(Level::Info, msg.as_ref());
}

/// Print a warning: yellow ▲ prefix on stderr.
pub fn warn(msg: impl AsRef<str>) {
    print_level(Level::Warning, msg.as_ref());
}

/// Print an error: red ✗ prefix on stderr.
pub fn err(msg: impl AsRef<str>) {
    print_level(Level::Error, msg.as_ref());
}

// ─── Richer surfaces (use these for new code) ──────────────────────────

/// Print a section header — bold title underlined with `─`.
pub fn header(title: impl AsRef<str>) {
    let t = title.as_ref();
    let bar = "─".repeat(t.chars().count().max(8));
    if use_color_for(Stream::Stdout) {
        println!("\n{}\n{}", t.bold().underline(), bar.dimmed());
    } else {
        println!("\n{t}\n{bar}");
    }
}

/// Legacy alias for [`header`]. Kept so existing callers
/// (`output::section("...")`) keep compiling during the rollout.
pub fn section(title: impl AsRef<str>) {
    header(title);
}

/// One key/value pair rendered as `key ............ value` with
/// the key dimmed. Width is fixed at 24 to keep tables aligned.
pub fn kv(key: &str, value: impl AsRef<str>) {
    if use_color_for(Stream::Stdout) {
        let dimmed = key.dimmed();
        println!("  {dimmed:<24} {}", value.as_ref());
    } else {
        println!("  {key:<24} {}", value.as_ref());
    }
}

/// Render a two-column table from `headers` and `rows`. Wraps the
/// `tabled` crate so callers don't have to know its API.
pub fn table(headers: &[&str], rows: &[Vec<String>]) {
    if rows.is_empty() {
        return;
    }
    let mut builder = tabled::builder::Builder::default();
    builder.push_record(headers.iter().copied());
    for row in rows {
        let cells: Vec<&str> = row.iter().map(String::as_str).collect();
        builder.push_record(cells);
    }
    let mut built = builder.build();
    let tbl = built.with(tabled::settings::Style::modern());
    println!("\n{tbl}");
}

/// Render a boxed panel with a title and 1+ lines of body content.
/// Useful for "summary" sections that should stand out.
pub fn panel(title: &str, lines: &[String]) {
    let width = lines
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(20)
        .max(title.chars().count() + 2);
    let top = format!(
        "╭─ {} {}",
        title,
        "─".repeat(width.saturating_sub(title.chars().count() + 3))
    );
    let bot = format!("╰{}", "─".repeat(width + 3));
    if use_color_for(Stream::Stdout) {
        let top_c = top.cyan();
        let bot_c = bot.cyan();
        println!("\n{top_c}");
        for line in lines {
            println!("│  {line}");
        }
        println!("{bot_c}");
    } else {
        println!("\n{top}");
        for line in lines {
            println!("│  {line}");
        }
        println!("{bot}");
    }
}

// ─── Machine output ─────────────────────────────────────────────────────

/// Emit a value as pretty-printed JSON to stdout. Used when the
/// `--json` flag is on.
pub fn json<T: Serialize>(value: &T) -> crate::error::Result<()> {
    let s = serde_json::to_string_pretty(value)?;
    println!("{s}");
    Ok(())
}

// ─── TTY detection (exposed for tests) ──────────────────────────────────

/// True when ANSI color codes will be emitted to stdout.
pub fn stdout_is_tty() -> bool {
    use_color_for(Stream::Stdout)
}

/// True when ANSI color codes will be emitted to stderr.
pub fn stderr_is_tty() -> bool {
    use_color_for(Stream::Stderr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level_symbols_are_stable() {
        // Symbols are part of the brand; changing them is a
        // breaking change for anyone parsing the output.
        assert_eq!(Level::Success.symbol(), "✓");
        assert_eq!(Level::Info.symbol(), "●");
        assert_eq!(Level::Warning.symbol(), "▲");
        assert_eq!(Level::Error.symbol(), "✗");
    }

    #[test]
    fn test_level_streams() {
        assert_eq!(Level::Success.stream(), Stream::Stdout);
        assert_eq!(Level::Info.stream(), Stream::Stdout);
        assert_eq!(Level::Warning.stream(), Stream::Stderr);
        assert_eq!(Level::Error.stream(), Stream::Stderr);
    }

    #[test]
    fn test_kv_does_not_panic() {
        kv("Server IP", "10.10.10.254");
        kv("HTTP Port", "8080");
    }

    #[test]
    fn test_table_renders_one_row() {
        table(
            &["VERSION", "STATUS"],
            &[vec!["v1".into(), "active".into()]],
        );
    }

    #[test]
    fn test_table_empty_rows_renders_nothing() {
        table(&["A"], &[]);
    }

    #[test]
    fn test_panel_renders_title_and_lines() {
        panel("Resumo", &["v1 (active)".into(), "v2 (previous)".into()]);
    }

    #[test]
    fn test_header_does_not_panic() {
        header("GAR Image Health Check");
    }

    #[test]
    fn test_json_serialize() {
        let v = serde_json::json!({"status": "ok"});
        json(&v).expect("should serialize");
    }
}
