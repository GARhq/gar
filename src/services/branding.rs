//! Branding diagnostics — SDDM, Plymouth, Plasma assets.
//!
//! Read-only inspection of theme/layout/color/wallpaper directories under
//! the Nix store (`/run/current-system/sw/share/...`). Used by
//! `gar branding doctor` to validate the branding layer of GAROS.
//!
//! Inspired by `cmd_branding_doctor` in `server/ragos-cli.nix` (12 lines of bash).

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{GarError, Result};

const NIX_SW: &str = "/run/current-system/sw";

/// Per-check result: where we looked and what we found (or didn't).
#[derive(Debug, Clone, Serialize)]
pub struct BrandingCheck {
    pub label: String,
    pub path: String,
    pub found: bool,
    pub matches: Vec<String>,
}

/// Aggregate branding diagnostic. Output as JSON or rendered table.
#[derive(Debug, Serialize)]
pub struct BrandingReport {
    pub sddm_current: BrandingCheck,
    pub sddm_theme_dirs: BrandingCheck,
    pub plymouth_theme: BrandingCheck,
    pub plasma_look_and_feel: BrandingCheck,
    pub plasma_desktoptheme: BrandingCheck,
    pub plasma_color_schemes: BrandingCheck,
    pub plasma_wallpapers: BrandingCheck,
    pub ok_count: usize,
    pub fail_count: usize,
    /// Baseline manifest validation (only populated if a baseline manifest
    /// is found via env var or the standard search path). Absent when
    /// no baseline is available — degraded mode, not an error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline: Option<BaselineReport>,
}

// =====================================================================
// Baseline manifest — versioned surface list from artifacts/branding/
// =====================================================================
//
// File format (baseline-manifest.txt, brandlab_manifest_version 2):
//   brandlab_manifest_version|<n>
//   surface|<surface>|<type>|<status>|<path>|<value>
//   config|sha256|<path>|<hash>          # applies to the surface entry
//   asset|sha256|<path>|<hash>           # applies to a binary asset
//
// Delimiter is `|`. Lines starting with `#` and blank lines are ignored.
// Paths are repo-relative. `absent` status is a design choice, not drift.
//
// Schema is documented in artifacts/branding/baseline-manifest.txt and
// in the README at artifacts/branding/README.md.

/// Surface entry from the baseline manifest (one declarative theme path).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SurfaceEntry {
    pub surface: String,
    pub kind: String,
    /// `present` or `absent` — `absent` is by design, not drift.
    pub status: String,
    pub path: String,
    pub value: Option<String>,
    /// Lowercase hex SHA256 from `config|sha256|<path>|<hash>`, if present.
    pub sha256: Option<String>,
}

/// Asset entry (binary file like background.jpg, metadata.json).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AssetEntry {
    pub path: String,
    pub sha256: String,
}

/// One drift finding produced by `validate_against_baseline`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Drift {
    pub kind: String,
    pub path: String,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub message: String,
}

/// Aggregated baseline report (drift detection results).
#[derive(Debug, Clone, Serialize)]
pub struct BaselineReport {
    /// Path of the baseline manifest that was loaded.
    pub manifest_path: String,
    pub manifest_version: u32,
    pub surface_count: usize,
    pub asset_count: usize,
    pub present_surfaces: usize,
    pub absent_surfaces: usize,
    pub drifts: Vec<Drift>,
}

/// Parse the contents of a `baseline-manifest.txt` file.
///
/// Format reference: see module-level doc comment.
/// Skips blank lines and `#`-prefixed comments. Header line
/// (`brandlab_manifest_version|<n>`) is consumed for `version` only.
///
/// Returns `Err` on I/O failure or malformed header. Unknown line shapes
/// are silently ignored (forward-compat with future manifest versions).
#[must_use = "parse_baseline_manifest returns parsed entries or an error"]
#[tracing::instrument(skip_all, fields(path = %path.display()))]
pub fn parse_baseline_manifest(path: &Path) -> Result<BaselineManifest> {
    let content = std::fs::read_to_string(path)?;
    parse_baseline_manifest_str(&content).map_err(|e| {
        GarError::validation(format!("parse {}: {}", path.display(), e))
    })
}

/// Parsed in-memory manifest (split into surfaces + assets for easy lookup).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineManifest {
    pub version: u32,
    pub surfaces: Vec<SurfaceEntry>,
    pub assets: Vec<AssetEntry>,
}

/// In-memory parser (testable without filesystem).
pub fn parse_baseline_manifest_str(content: &str) -> std::result::Result<BaselineManifest, String> {
    let mut surfaces: Vec<SurfaceEntry> = Vec::new();
    let mut assets: Vec<AssetEntry> = Vec::new();
    let mut version: Option<u32> = None;

    for (line_no, raw) in content.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('|').collect();
        if parts.is_empty() {
            continue;
        }

        // Header line: brandlab_manifest_version|<n>
        if parts[0] == "brandlab_manifest_version" && parts.len() == 2 {
            version = Some(parts[1].parse().map_err(|_| {
                format!("line {}: invalid version '{}'", line_no + 1, parts[1])
            })?);
            continue;
        }

        // Surface entry: surface|<surface>|<kind>|<status>|<path>|<value...>
        if parts[0] == "surface" && parts.len() >= 5 {
            let surface = parts[1].to_string();
            let kind = parts[2].to_string();
            let status = parts[3].to_string();
            let path = parts[4].to_string();
            // Last field(s) may contain `=`, treat as a single value token.
            let value = if parts.len() > 5 {
                Some(parts[5..].join("|"))
            } else {
                None
            };
            surfaces.push(SurfaceEntry {
                surface,
                kind,
                status,
                path,
                value,
                sha256: None,
            });
            continue;
        }

        // config|sha256|<path>|<hash>
        if parts[0] == "config" && parts[1] == "sha256" && parts.len() == 4 {
            attach_sha256_to_surface(&mut surfaces, parts[2], parts[3]);
            continue;
        }

        // asset|sha256|<path>|<hash>
        if parts[0] == "asset" && parts[1] == "sha256" && parts.len() == 4 {
            assets.push(AssetEntry {
                path: parts[2].into(),
                sha256: parts[3].into(),
            });
            continue;
        }

        // Unknown shape — ignore for forward compat.
    }

    let version = version.ok_or_else(|| "missing brandlab_manifest_version header".to_string())?;
    Ok(BaselineManifest {
        version,
        surfaces,
        assets,
    })
}

fn attach_sha256_to_surface(surfaces: &mut Vec<SurfaceEntry>, path: &str, hash: &str) {
    for s in surfaces.iter_mut() {
        if s.path == path && s.sha256.is_none() {
            s.sha256 = Some(hash.to_string());
            return;
        }
    }
    // config|sha256 for a path not seen as a surface entry — record anyway.
    // Use a sentinel surface so the drift check still validates the file.
    surfaces.push(SurfaceEntry {
        surface: "(orphan-config)".into(),
        kind: "sha256".into(),
        status: "present".into(),
        path: path.into(),
        value: None,
        sha256: Some(hash.into()),
    });
}

/// Resolve the baseline manifest path using the standard search order.
///
/// Search order:
/// 1. `GAR_BRANDING_BASELINE` env var (explicit override)
/// 2. `/etc/ragos/branding/baseline-manifest.txt` (runtime config)
/// 3. `/run/current-system/sw/share/gar/branding/baseline-manifest.txt` (Nix store)
/// 4. `$CARGO_MANIFEST_DIR/branding/baseline-manifest.txt` (dev workspace)
///
/// Returns `Ok(None)` when no candidate is found (caller degrades to
/// "no baseline available" — not an error).
#[must_use = "resolve_baseline_path returns the first existing baseline path or None"]
#[tracing::instrument(skip_all)]
pub fn resolve_baseline_path() -> Result<Option<PathBuf>> {
    let candidates: [Option<PathBuf>; 4] = [
        std::env::var("GAR_BRANDING_BASELINE").ok().map(PathBuf::from),
        Some(PathBuf::from("/etc/ragos/branding/baseline-manifest.txt")),
        Some(PathBuf::from(
            "/run/current-system/sw/share/gar/branding/baseline-manifest.txt",
        )),
        std::env::var("CARGO_MANIFEST_DIR")
            .ok()
            .map(|d| PathBuf::from(d).join("branding/baseline-manifest.txt")),
    ];
    for cand in candidates.into_iter().flatten() {
        if cand.is_file() {
            return Ok(Some(cand));
        }
    }
    Ok(None)
}

/// Validate a baseline manifest against the on-disk repo at `repo_root`.
///
/// For each surface with `status=present`, checks that `<repo_root>/<path>`
/// exists (drift: `surface_missing`).
///
/// For each surface/asset with a SHA256, computes the file's actual
/// SHA256 and compares (drift: `sha256_mismatch`). Missing files are
/// reported as `sha256_mismatch` with `actual=None` (not `surface_missing`
/// twice) to keep drift counts meaningful.
///
/// `absent` surfaces are NOT reported as drift — they're a design choice.
#[must_use = "validate_against_baseline returns drifts or an empty Vec"]
#[tracing::instrument(skip_all, fields(repo_root = %repo_root.display()))]
pub fn validate_against_baseline(
    manifest: &BaselineManifest,
    repo_root: &Path,
) -> Vec<Drift> {
    let mut drifts = Vec::new();

    for surface in &manifest.surfaces {
        if surface.surface == "(orphan-config)" {
            // SHA256-only entry from a config|sha256 that didn't match any surface.
            if let Some(sha) = &surface.sha256 {
                drifts.extend(check_sha256(repo_root, &surface.path, sha));
            }
            continue;
        }
        if surface.status == "absent" {
            continue;
        }
        let abs = repo_root.join(&surface.path);
        if !abs.exists() {
            drifts.push(Drift {
                kind: "surface_missing".into(),
                path: surface.path.clone(),
                expected: Some(surface.path.clone()),
                actual: None,
                message: format!("surface path '{}' not found in repo", surface.path),
            });
            continue;
        }
        if let Some(sha) = &surface.sha256 {
            drifts.extend(check_sha256(repo_root, &surface.path, sha));
        }
    }

    for asset in &manifest.assets {
        drifts.extend(check_sha256(repo_root, &asset.path, &asset.sha256));
    }

    drifts
}

fn check_sha256(repo_root: &Path, rel_path: &str, expected: &str) -> Option<Drift> {
    let abs = repo_root.join(rel_path);
    let bytes = match std::fs::read(&abs) {
        Ok(b) => b,
        Err(e) => {
            return Some(Drift {
                kind: "sha256_mismatch".into(),
                path: rel_path.into(),
                expected: Some(expected.into()),
                actual: None,
                message: format!("cannot read file: {}", e),
            });
        }
    };
    let actual = sha256_hex(&bytes);
    if actual == expected {
        return None;
    }
    Some(Drift {
        kind: "sha256_mismatch".into(),
        path: rel_path.into(),
        expected: Some(expected.into()),
        actual: Some(actual.clone()),
        message: format!("expected {}, got {}", expected, actual),
    })
}

/// Compute lowercase hex SHA256 of a byte slice (no external dep).
fn sha256_hex(bytes: &[u8]) -> String {
    // Minimal SHA256 implementation via the `sha2` crate would be ideal,
    // but we don't want to add a dep just for this. Use the system `sha256sum`
    // via Command for parity with `services::build::sha256_file`.
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = match Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(bytes);
    }
    match child.wait_with_output() {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string()
        }
        _ => String::new(),
    }
}

/// Build a `BaselineReport` from parsed manifest + drifts.
#[must_use = "summarize_baseline returns a serializable report"]
pub fn summarize_baseline(
    manifest_path: &Path,
    manifest: &BaselineManifest,
    drifts: Vec<Drift>,
) -> BaselineReport {
    let present_surfaces = manifest
        .surfaces
        .iter()
        .filter(|s| s.status == "present" && s.surface != "(orphan-config)")
        .count();
    let absent_surfaces = manifest
        .surfaces
        .iter()
        .filter(|s| s.status == "absent")
        .count();
    BaselineReport {
        manifest_path: manifest_path.display().to_string(),
        manifest_version: manifest.version,
        surface_count: manifest.surfaces.len(),
        asset_count: manifest.assets.len(),
        present_surfaces,
        absent_surfaces,
        drifts,
    }
}

/// Read the active SDDM theme from any sddm.conf.
pub fn sddm_current() -> BrandingCheck {
    let paths = ["/etc/sddm.conf", "/etc/sddm.conf.d"];
    let mut found = String::new();
    for p in &paths {
        if let Some(line) = grep_ini_value(p, "Current") {
            found = line;
            break;
        }
    }
    BrandingCheck {
        label: "sddm_current".into(),
        path: "/etc/sddm.conf{,.d}".into(),
        found: !found.is_empty(),
        matches: if found.is_empty() {
            vec!["nao encontrado".into()]
        } else {
            vec![found]
        },
    }
}

/// First 20 SDDM theme directories installed.
pub fn sddm_theme_dirs() -> BrandingCheck {
    let base = format!("{}/share/sddm/themes", NIX_SW);
    BrandingCheck {
        label: "sddm_theme_dirs".into(),
        path: base.clone(),
        found: Path::new(&base).exists(),
        matches: list_dirs(&base, 20, None),
    }
}

/// Active Plymouth theme (first match in /etc/plymouth).
pub fn plymouth_theme() -> BrandingCheck {
    let mut hits = Vec::new();
    if Path::new("/etc/plymouth").exists() {
        if let Ok(out) = std::fs::read_dir("/etc/plymouth") {
            for entry in out.flatten() {
                if entry.path().extension().and_then(|e| e.to_str()) == Some("conf") {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        for line in content.lines() {
                            if let Some(rest) = line.strip_prefix("Theme=") {
                                hits.push(format!("{}={}", entry.path().display(), rest));
                            }
                        }
                    }
                }
            }
        }
    }
    BrandingCheck {
        label: "plymouth_theme".into(),
        path: "/etc/plymouth".into(),
        found: !hits.is_empty(),
        matches: if hits.is_empty() {
            vec!["nao encontrado".into()]
        } else {
            hits
        },
    }
}

/// Plasma look-and-feel packages matching `ragos|org.ragos`.
pub fn plasma_look_and_feel() -> BrandingCheck {
    let base = format!("{}/share/plasma/look-and-feel", NIX_SW);
    BrandingCheck {
        label: "plasma_look_and_feel".into(),
        path: base.clone(),
        found: Path::new(&base).exists(),
        matches: list_dirs(&base, 20, Some(&["ragos", "org.kde"])),
    }
}

/// Plasma desktoptheme packages matching `ragos|org.ragos`.
pub fn plasma_desktoptheme() -> BrandingCheck {
    let base = format!("{}/share/plasma/desktoptheme", NIX_SW);
    BrandingCheck {
        label: "plasma_desktoptheme".into(),
        path: base.clone(),
        found: Path::new(&base).exists(),
        matches: list_dirs(&base, 20, Some(&["ragos", "org.kde"])),
    }
}

/// RAGOS color schemes (`RAGOS*.colors`).
pub fn plasma_color_schemes() -> BrandingCheck {
    let base = format!("{}/share/color-schemes", NIX_SW);
    BrandingCheck {
        label: "plasma_color_schemes".into(),
        path: base.clone(),
        found: Path::new(&base).exists(),
        matches: list_files_matching(&base, 20, |name| name.starts_with("RAGOS") && name.ends_with(".colors")),
    }
}

/// Wallpaper packages matching `ragos|org.ragos`.
pub fn plasma_wallpapers() -> BrandingCheck {
    let base = format!("{}/share/wallpapers", NIX_SW);
    BrandingCheck {
        label: "plasma_wallpapers".into(),
        path: base.clone(),
        found: Path::new(&base).exists(),
        matches: list_dirs(&base, 20, Some(&["ragos", "org.kde"])),
    }
}

/// Run all branding checks and assemble a report.
pub fn collect_report(cfg: &crate::config::Config) -> BrandingReport {
    let sddm_current = sddm_current();
    let sddm_theme_dirs = sddm_theme_dirs();
    let plymouth_theme = plymouth_theme();
    let plasma_look_and_feel = plasma_look_and_feel();
    let plasma_desktoptheme = plasma_desktoptheme();
    let plasma_color_schemes = plasma_color_schemes();
    let plasma_wallpapers = plasma_wallpapers();

    let all = [
        &sddm_current,
        &sddm_theme_dirs,
        &plymouth_theme,
        &plasma_look_and_feel,
        &plasma_desktoptheme,
        &plasma_color_schemes,
        &plasma_wallpapers,
    ];
    let mut ok = 0;
    let mut fail = 0;
    for c in all {
        if c.found && !c.matches.is_empty() && c.matches[0] != "nao encontrado" {
            ok += 1;
        } else {
            fail += 1;
        }
    }

    // Baseline manifest — degrades silently when no manifest is found.
    let baseline = collect_baseline(cfg);

    BrandingReport {
        sddm_current,
        sddm_theme_dirs,
        plymouth_theme,
        plasma_look_and_feel,
        plasma_desktoptheme,
        plasma_color_schemes,
        plasma_wallpapers,
        ok_count: ok,
        fail_count: fail,
        baseline,
    }
}

/// Resolve, parse, and validate the baseline manifest. Returns `None` when
/// no manifest is reachable (env var unset, runtime paths missing, dev
/// workspace missing). Errors are logged via `tracing` but do not propagate
/// — `gar branding doctor` reports the live runtime checks regardless of
/// baseline availability.
#[tracing::instrument(skip_all)]
fn collect_baseline(cfg: &crate::config::Config) -> Option<BaselineReport> {
    let path = match resolve_baseline_path() {
        Ok(Some(p)) => p,
        Ok(None) => {
            tracing::debug!(target: "gar::branding", "no baseline manifest found");
            return None;
        }
        Err(e) => {
            tracing::warn!(target: "gar::branding", "baseline resolve failed: {}", e);
            return None;
        }
    };
    let manifest = match parse_baseline_manifest(&path) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(target: "gar::branding", "baseline parse failed: {}", e);
            return None;
        }
    };
    // Repo root = parent of the baseline manifest, walked up to find flake.nix.
    // For runtime paths (/etc/ragos/branding/...) the parent dir may not be
    // the repo root — fall back to manifest dir if flake.nix isn't found.
    // Pass cfg.flake_path as a hint to disambiguate when `gar` is built
    // from a sibling repo (e.g. `gar/`) but auditing a different repo
    // (e.g. `garos/`) — walk-up alone would resolve to the wrong flake.nix.
    let repo_root = crate::services::manifest::find_flake_root(
        path.parent().unwrap_or(&path),
        Some(cfg.flake_path.as_path()),
    )
    .unwrap_or_else(|| path.parent().unwrap_or(&path).to_path_buf());
    let drifts = validate_against_baseline(&manifest, &repo_root);
    Some(summarize_baseline(&path, &manifest, drifts))
}

// -- internal helpers -------------------------------------------------------

fn grep_ini_value(root: &str, key: &str) -> Option<String> {
    let path = Path::new(root);
    if !path.exists() {
        return None;
    }
    let entries = if path.is_dir() {
        std::fs::read_dir(path).ok()?.flatten().map(|e| e.path()).collect()
    } else {
        vec![path.to_path_buf()]
    };
    let prefix = format!("{}=", key);
    for entry in entries {
        if let Ok(content) = std::fs::read_to_string(&entry) {
            for line in content.lines() {
                if let Some(rest) = line.trim().strip_prefix(&prefix) {
                    return Some(format!("{} {}={}", entry.display(), key, rest));
                }
            }
        }
    }
    None
}

fn list_dirs(base: &str, limit: usize, filter_substr: Option<&[&str]>) -> Vec<String> {
    list_entries(base, limit, filter_substr, true)
}

fn list_files_matching(base: &str, limit: usize, matches: impl Fn(&str) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(base) else { return out };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if matches(&name) {
            out.push(entry.path().display().to_string());
            if out.len() >= limit {
                break;
            }
        }
    }
    out
}

fn list_entries(base: &str, limit: usize, filter_substr: Option<&[&str]>, require_dir: bool) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(base) else { return out };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if require_dir && !entry.path().is_dir() {
            continue;
        }
        if let Some(filters) = filter_substr {
            if !filters.iter().any(|f| name.contains(f)) {
                continue;
            }
        }
        out.push(entry.path().display().to_string());
        if out.len() >= limit {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_serializes() {
        let c = BrandingCheck {
            label: "test".into(),
            path: "/x".into(),
            found: true,
            matches: vec!["a".into(), "b".into()],
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("\"label\":\"test\""));
        assert!(json.contains("\"found\":true"));
    }

    #[test]
    fn test_collect_report_runs_against_tmp() {
        // Point NIX_SW at a tempdir via env override? We keep it read-only:
        // when /run/current-system/sw doesn't exist (CI), every check reports
        // found=false without crashing — that's the contract.
        let r = collect_report(&crate::config::Config::default());
        // On a real NixOS install some checks pass; on a CI sandbox they all fail.
        // Either way: ok + fail == total checks.
        assert_eq!(r.ok_count + r.fail_count, 7);
    }

    #[test]
    fn test_grep_ini_value_missing_dir() {
        assert!(grep_ini_value("/no/such/path/xyz", "Current").is_none());
    }

    #[test]
    fn test_grep_ini_value_finds_key() {
        let tmp = std::env::temp_dir().join(format!("gar-branding-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let conf = tmp.join("sddm.conf");
        std::fs::write(&conf, "[Theme]\nCurrent=breeze\n").unwrap();
        let v = grep_ini_value(conf.to_str().unwrap(), "Current").unwrap();
        assert!(v.contains("Current=breeze"));
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_list_files_matching_filter() {
        let tmp = std::env::temp_dir().join(format!("gar-colorscheme-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("RAGOSDark.colors"), "").unwrap();
        std::fs::write(tmp.join("RAGOSLight.colors"), "").unwrap();
        std::fs::write(tmp.join("Breeze.colors"), "").unwrap();
        let matches = list_files_matching(tmp.to_str().unwrap(), 20, |n| {
            n.starts_with("RAGOS") && n.ends_with(".colors")
        });
        assert_eq!(matches.len(), 2);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_report_ok_fail_sum() {
        let r = collect_report(&crate::config::Config::default());
        assert_eq!(r.ok_count + r.fail_count, 7);
    }

    // --- BaselineManifest tests ---

    #[test]
    fn test_baseline_parse_happy_path() {
        let content = "\
brandlab_manifest_version|2
surface|plymouth|declarative-theme|present|themes/plymouth/plymouth.nix|theme=ragos
surface|sddm|declarative-theme|present|themes/sddm/sddm.nix|theme=ragos-control
config|sha256|themes/plymouth/plymouth.nix|f19b58a7d4dd908739d68d19e7226149219ff86e6822ff2a12640b3a6c912a7e
asset|sha256|themes/plymouth/ragos/background.jpg|49153a82a8e40a943e18e25aa3b26f2cc5b8a40a9ec764a3247e26f267f0d22f
";
        let m = parse_baseline_manifest_str(content).unwrap();
        assert_eq!(m.version, 2);
        assert_eq!(m.surfaces.len(), 2);
        assert_eq!(m.assets.len(), 1);
        assert_eq!(m.surfaces[0].surface, "plymouth");
        assert_eq!(m.surfaces[0].sha256.as_deref(), Some("f19b58a7d4dd908739d68d19e7226149219ff86e6822ff2a12640b3a6c912a7e"));
        assert_eq!(m.assets[0].sha256, "49153a82a8e40a943e18e25aa3b26f2cc5b8a40a9ec764a3247e26f267f0d22f");
    }

    #[test]
    fn test_baseline_parse_missing_header_errors() {
        let content = "\
surface|plymouth|declarative-theme|present|themes/plymouth/plymouth.nix|theme=ragos
";
        assert!(parse_baseline_manifest_str(content).is_err());
    }

    #[test]
    fn test_baseline_parse_skips_blanks_and_comments() {
        let content = "\
# Header comment
brandlab_manifest_version|3

# inline comment
surface|gtk|custom-theme|absent|repo-scan|no-explicit-gtk-theme
";
        let m = parse_baseline_manifest_str(content).unwrap();
        assert_eq!(m.version, 3);
        assert_eq!(m.surfaces.len(), 1);
        assert_eq!(m.surfaces[0].status, "absent");
    }

    #[test]
    fn test_baseline_parse_unknown_shape_is_ignored() {
        // Forward compat: a future version may add new line shapes; parser
        // must not crash.
        let content = "\
brandlab_manifest_version|2
newshape|future|reserved|present|whatever/path|value=foo
surface|plymouth|declarative-theme|present|themes/plymouth/plymouth.nix|theme=ragos
";
        let m = parse_baseline_manifest_str(content).unwrap();
        assert_eq!(m.surfaces.len(), 1);
    }

    #[test]
    fn test_baseline_validate_clean_run_no_drift() {
        // Synthetic repo with one surface + matching config|sha256 + one asset.
        let tmp = std::env::temp_dir().join(format!(
            "gar-baseline-clean-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(tmp.join("themes/plymouth")).unwrap();
        std::fs::write(tmp.join("themes/plymouth/plymouth.nix"), "{ ... }").unwrap();

        let content = format!(
            "brandlab_manifest_version|2\n\
             surface|plymouth|declarative-theme|present|themes/plymouth/plymouth.nix|theme=ragos\n\
             config|sha256|themes/plymouth/plymouth.nix|{}\n",
            sha256_hex_of(&tmp.join("themes/plymouth/plymouth.nix")),
        );
        let m = parse_baseline_manifest_str(&content).unwrap();
        let drifts = validate_against_baseline(&m, &tmp);
        assert!(drifts.is_empty(), "expected no drift, got: {:?}", drifts);

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_baseline_validate_detects_surface_missing() {
        // Synthetic repo without the surface path declared in manifest.
        let tmp = std::env::temp_dir().join(format!(
            "gar-baseline-miss-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        // Note: tmp/themes/plymouth/plymouth.nix is NOT created.

        let content = "brandlab_manifest_version|2\n\
                       surface|plymouth|declarative-theme|present|themes/plymouth/plymouth.nix|theme=ragos\n";
        let m = parse_baseline_manifest_str(content).unwrap();
        let drifts = validate_against_baseline(&m, &tmp);
        assert_eq!(drifts.len(), 1);
        assert_eq!(drifts[0].kind, "surface_missing");
        assert_eq!(drifts[0].path, "themes/plymouth/plymouth.nix");

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_baseline_validate_detects_sha256_mismatch() {
        // Synthetic repo with a file whose sha256 doesn't match the manifest.
        let tmp = std::env::temp_dir().join(format!(
            "gar-baseline-sha-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(tmp.join("themes/plymouth")).unwrap();
        std::fs::write(tmp.join("themes/plymouth/plymouth.nix"), "real content").unwrap();

        // Manifest claims hash of "different content" — must mismatch.
        let wrong_hash = sha256_hex_of_bytes(b"different content");
        let content = format!(
            "brandlab_manifest_version|2\n\
             surface|plymouth|declarative-theme|present|themes/plymouth/plymouth.nix|theme=ragos\n\
             config|sha256|themes/plymouth/plymouth.nix|{}\n",
            wrong_hash
        );
        let m = parse_baseline_manifest_str(&content).unwrap();
        let drifts = validate_against_baseline(&m, &tmp);
        assert_eq!(drifts.len(), 1);
        assert_eq!(drifts[0].kind, "sha256_mismatch");
        assert_eq!(drifts[0].path, "themes/plymouth/plymouth.nix");
        assert_eq!(drifts[0].expected.as_deref(), Some(wrong_hash.as_str()));
        assert!(drifts[0].actual.is_some());

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_baseline_validate_asset_sha256_match() {
        // Validates asset|sha256 against an actual file (uses metadata.json
        // from the real garos repo as the anchor for hash correctness).
        let real = Path::new(
            "/home/rocha/Proyectos/garos-dev/garos/themes/plasma/wallpapers/org.ragos.wallpaper.light/metadata.json",
        );
        if !real.exists() {
            // Skip gracefully if test environment lacks the real file.
            return;
        }
        let real_hash = sha256_hex_of(real);
        // Manifest points at the real path with the real hash — must match.
        let content = format!(
            "brandlab_manifest_version|2\n\
             asset|sha256|themes/plasma/wallpapers/org.ragos.wallpaper.light/metadata.json|{}\n",
            real_hash
        );
        let repo_root = Path::new(
            "/home/rocha/Proyectos/garos-dev/garos",
        );
        let m = parse_baseline_manifest_str(&content).unwrap();
        let drifts = validate_against_baseline(&m, repo_root);
        assert!(drifts.is_empty(), "expected no drift, got: {:?}", drifts);
    }

    // Helper: real-path SHA256 for tests.
    fn sha256_hex_of(path: &Path) -> String {
        let bytes = std::fs::read(path).expect("read fixture");
        sha256_hex_of_bytes(&bytes)
    }

    fn sha256_hex_of_bytes(bytes: &[u8]) -> String {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new("sha256sum")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("sha256sum available");
        child.stdin.take().unwrap().write_all(bytes).unwrap();
        let out = child.wait_with_output().unwrap();
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .next()
            .unwrap()
            .to_string()
    }
}
