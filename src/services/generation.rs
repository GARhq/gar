//! Generation metadata for IMAGE generations (bootable diskless images).
//!
//! Read sidecar files written by `gar image build` (stage_generation) and
//! runtime params from `/var/lib/ragos/runtime/params.nix`.
//!
//! Replaces `publish.sh:104-114` (`runtime_source_from_params_file`),
//! `publish.sh:233-243` (`read_generation_init_path`,
//! `read_generation_kernel_params`).
//!
//! Don't confuse with `services/generations.rs` (plural): that module
//! handles NixOS system generations (Nix profile metadata via `nix-env`,
//! `nixos-rebuild --list-generations`). This module handles per-build
//! image artifacts. Different scopes — both kept.

use std::path::Path;

use crate::error::{GarError, Result};

/// Read `.init_path` sidecar from a generation directory.
///
/// Equivalent to bash `read_generation_init_path` (publish.sh:233-237).
/// Returns the literal string content (may contain spaces, quotes, paths).
///
/// Errors:
/// - file missing -> `GarError::invalid_input`
/// - file unreadable -> `GarError::Io`
#[must_use = "read_generation_init_path returns the init path string"]
#[tracing::instrument(skip_all, fields(generation_dir = %path.display()))]
pub fn read_generation_init_path(path: &Path) -> Result<String> {
    let sidecar = path.join(".init_path");
    if !sidecar.is_file() {
        return Err(GarError::invalid_argument(format!(
            "read {}: arquivo nao encontrado",
            sidecar.display()
        )));
    }
    let content = std::fs::read_to_string(&sidecar)?;
    Ok(content.trim().to_string())
}

/// Read `.kernel_params` sidecar and normalize whitespace.
///
/// Equivalent to bash `read_generation_kernel_params` (publish.sh:239-243).
/// The bash version runs `tr '\n' ' ' | sed -E 's/[[:space:]]+/ /g; s/^ //; s/ $//'`
/// to collapse any internal newlines/tabs to single spaces and trim edges.
/// The Rust version preserves that contract byte-for-byte (cmdline is
/// opaque to both, but callers downstream rely on the single-space shape).
///
/// Returns `String` (opaque cmdline) — do NOT parse, callers concat as-is.
#[must_use = "read_generation_kernel_params returns the kernel cmdline string"]
#[tracing::instrument(skip_all, fields(generation_dir = %path.display()))]
pub fn read_generation_kernel_params(path: &Path) -> Result<String> {
    let sidecar = path.join(".kernel_params");
    if !sidecar.is_file() {
        return Err(GarError::invalid_argument(format!(
            "read {}: arquivo nao encontrado",
            sidecar.display()
        )));
    }
    let raw = std::fs::read_to_string(&sidecar)?;
    // Collapse all whitespace runs (incl. newlines) to a single space, trim edges.
    let normalized: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    Ok(normalized)
}

/// Extract the `runtimeSource = "..."` value from a Nix params file.
///
/// Equivalent to bash `runtime_source_from_params_file` (publish.sh:104-114).
/// The awk pattern `runtimeSource[[:space:]]*=` is tolerant: any key starting
/// with `runtimeSource` matches (e.g. `runtimeSource =`, `runtimeSource=`,
/// `runtimeSourceXxx =`). Bash returns the empty string on miss (and the
/// caller is expected to validate the value).
///
/// This Rust version preserves that tolerance: returns the raw captured
/// value, empty string if no match. The caller decides whether the value
/// is acceptable (e.g. `ensure_runtime_contract_for_publish` requires
/// it to equal `"runtime"`).
#[must_use = "runtime_source_from_params_file returns the runtime source value"]
#[tracing::instrument(skip_all, fields(params_file = %path.display()))]
pub fn runtime_source_from_params_file(path: &Path) -> Result<String> {
    if !path.is_file() {
        return Err(GarError::invalid_argument(format!(
            "read {}: arquivo nao encontrado",
            path.display()
        )));
    }
    let content = std::fs::read_to_string(path)?;
    // Pattern mirrors bash: any line whose first whitespace-trimmed token
    // starts with "runtimeSource" followed by optional whitespace and `=`.
    for line in content.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("runtimeSource") {
            continue;
        }
        // Skip the key, find '='
        let after_key = &trimmed["runtimeSource".len()..];
        let after_key = after_key.trim_start();
        let Some(eq_idx) = after_key.find('=') else {
            continue;
        };
        let after_eq = &after_key[eq_idx + 1..];
        let after_eq = after_eq.trim_start();
        // Capture value: either "..." (string) or bareword
        if let Some(rest) = after_eq.strip_prefix('"') {
            // Quoted string — find closing quote (no escape handling, matches bash awk).
            if let Some(close) = rest.find('"') {
                return Ok(rest[..close].to_string());
            }
        }
        // Bareword: take until first whitespace, semicolon, comment, or end of line.
        let bare: String = after_eq
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '#' && *c != ';')
            .collect();
        if !bare.is_empty() {
            return Ok(bare);
        }
    }
    // No match — bash returns empty string. Preserve that contract.
    Ok(String::new())
}

/// Convenience: parse `runtime_source_from_params_file` output as a
/// [`RuntimeSource`] enum for callers that need a constrained value.
///
/// `None` for empty/unrecognized values (matches bash's `|| true` behavior).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeSource {
    /// `runtimeSource = "runtime"` — the canonical GAROS publishing source.
    Runtime,
    /// Any other non-empty value (e.g. a `/nix/store/...` path).
    Other(String),
}

impl RuntimeSource {
    pub fn parse(s: &str) -> Option<Self> {
        if s.is_empty() {
            None
        } else if s == "runtime" {
            Some(Self::Runtime)
        } else {
            Some(Self::Other(s.to_string()))
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Runtime => "runtime",
            Self::Other(s) => s,
        }
    }

    pub fn is_runtime(&self) -> bool {
        matches!(self, Self::Runtime)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gar-gen-{}-{}-{}",
            name,
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    // ---- read_generation_init_path ----

    #[test]
    fn test_read_generation_init_path_valid() {
        let dir = tmp("init-ok");
        fs::write(dir.join(".init_path"), "/nix/store/abc-init/bin/init\n").unwrap();

        let result = read_generation_init_path(&dir).unwrap();
        assert_eq!(result, "/nix/store/abc-init/bin/init");

        cleanup(&dir);
    }

    #[test]
    fn test_read_generation_init_path_trims_trailing_newline() {
        let dir = tmp("init-trim");
        // Multiple trailing newlines — trim should collapse both.
        fs::write(dir.join(".init_path"), "/init/path\n\n").unwrap();

        let result = read_generation_init_path(&dir).unwrap();
        assert_eq!(result, "/init/path");

        cleanup(&dir);
    }

    #[test]
    fn test_read_generation_init_path_missing() {
        let dir = tmp("init-miss");
        // .init_path not created.

        let err = read_generation_init_path(&dir).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("arquivo nao encontrado"),
            "expected 'arquivo nao encontrado', got: {}",
            msg
        );
        assert!(msg.contains(".init_path"));

        cleanup(&dir);
    }

    #[test]
    fn test_read_generation_init_path_preserves_internal_spaces() {
        let dir = tmp("init-space");
        fs::write(dir.join(".init_path"), "/path with spaces/init\n").unwrap();

        let result = read_generation_init_path(&dir).unwrap();
        assert_eq!(result, "/path with spaces/init");

        cleanup(&dir);
    }

    // ---- read_generation_kernel_params ----

    #[test]
    fn test_read_generation_kernel_params_valid() {
        let dir = tmp("kp-ok");
        fs::write(
            dir.join(".kernel_params"),
            "quiet\nloglevel=3\nboot.shell_on_fail\n",
        )
        .unwrap();

        let result = read_generation_kernel_params(&dir).unwrap();
        assert_eq!(result, "quiet loglevel=3 boot.shell_on_fail");

        cleanup(&dir);
    }

    #[test]
    fn test_read_generation_kernel_params_collapses_whitespace() {
        let dir = tmp("kp-collapse");
        // Multiple spaces, tabs, newlines — all should collapse to single space.
        fs::write(dir.join(".kernel_params"), "a\tb  c\n\n\nd").unwrap();

        let result = read_generation_kernel_params(&dir).unwrap();
        assert_eq!(result, "a b c d");

        cleanup(&dir);
    }

    #[test]
    fn test_read_generation_kernel_params_trims_edges() {
        let dir = tmp("kp-trim");
        fs::write(dir.join(".kernel_params"), "\n\n  quiet loglevel=3  \n\n").unwrap();

        let result = read_generation_kernel_params(&dir).unwrap();
        assert_eq!(result, "quiet loglevel=3");

        cleanup(&dir);
    }

    #[test]
    fn test_read_generation_kernel_params_missing() {
        let dir = tmp("kp-miss");

        let err = read_generation_kernel_params(&dir).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("arquivo nao encontrado"));
        assert!(msg.contains(".kernel_params"));

        cleanup(&dir);
    }

    // ---- runtime_source_from_params_file ----

    #[test]
    fn test_runtime_source_canonical_value() {
        let dir = tmp("rt-ok");
        let p = dir.join("params.nix");
        fs::write(
            &p,
            r#"
            { ... }: {
              runtimeSource = "runtime";
              other = "x";
            }
            "#,
        )
        .unwrap();

        let result = runtime_source_from_params_file(&p).unwrap();
        assert_eq!(result, "runtime");

        cleanup(&dir);
    }

    #[test]
    fn test_runtime_source_no_whitespace_around_eq() {
        let dir = tmp("rt-tight");
        // bash awk pattern tolerates whitespace around `=`. Verify Rust does too.
        let p = dir.join("params.nix");
        fs::write(&p, "runtimeSource=\"runtime\"\n").unwrap();

        let result = runtime_source_from_params_file(&p).unwrap();
        assert_eq!(result, "runtime");

        cleanup(&dir);
    }

    #[test]
    fn test_runtime_source_with_indented_block() {
        let dir = tmp("rt-indent");
        let p = dir.join("params.nix");
        fs::write(
            &p,
            "{\n  config = {\n    runtimeSource = \"runtime\";\n  };\n}\n",
        )
        .unwrap();

        let result = runtime_source_from_params_file(&p).unwrap();
        assert_eq!(result, "runtime");

        cleanup(&dir);
    }

    #[test]
    fn test_runtime_source_returns_empty_when_no_match() {
        let dir = tmp("rt-empty");
        let p = dir.join("params.nix");
        fs::write(&p, "{ runtimeChannel = \"generic\"; }\n").unwrap();

        let result = runtime_source_from_params_file(&p).unwrap();
        assert_eq!(result, "");

        cleanup(&dir);
    }

    #[test]
    fn test_runtime_source_stops_at_semicolon() {
        // Bash awk only matches quoted strings; bareword in Nix is unusual
        // but Rust tolerates it and stops at the statement terminator.
        let dir = tmp("rt-semi");
        let p = dir.join("params.nix");
        fs::write(&p, "runtimeSource = runtime;\n").unwrap();

        let result = runtime_source_from_params_file(&p).unwrap();
        assert_eq!(result, "runtime");

        cleanup(&dir);
    }

    #[test]
    fn test_runtime_source_missing_file() {
        let dir = tmp("rt-miss");
        let p = dir.join("nope.nix");

        let err = runtime_source_from_params_file(&p).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("arquivo nao encontrado"));
        assert!(msg.contains("nope.nix"));

        cleanup(&dir);
    }

    // ---- RuntimeSource enum ----

    #[test]
    fn test_runtime_source_enum_parse_runtime() {
        assert_eq!(
            RuntimeSource::parse("runtime"),
            Some(RuntimeSource::Runtime)
        );
    }

    #[test]
    fn test_runtime_source_enum_parse_other() {
        assert_eq!(
            RuntimeSource::parse("/nix/store/abc"),
            Some(RuntimeSource::Other("/nix/store/abc".into()))
        );
    }

    #[test]
    fn test_runtime_source_enum_parse_empty_returns_none() {
        assert_eq!(RuntimeSource::parse(""), None);
    }

    #[test]
    fn test_runtime_source_enum_is_runtime() {
        assert!(RuntimeSource::Runtime.is_runtime());
        assert!(!RuntimeSource::Other("/foo".into()).is_runtime());
    }

    #[test]
    fn test_runtime_source_enum_as_str() {
        assert_eq!(RuntimeSource::Runtime.as_str(), "runtime");
        assert_eq!(
            RuntimeSource::Other("/nix/store/abc".into()).as_str(),
            "/nix/store/abc"
        );
    }
}
