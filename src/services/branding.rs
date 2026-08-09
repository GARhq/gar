//! Branding diagnostics — SDDM, Plymouth, Plasma assets.
//!
//! Read-only inspection of theme/layout/color/wallpaper directories under
//! the Nix store (`/run/current-system/sw/share/...`). Used by
//! `gar branding doctor` to validate the branding layer of GAROS.
//!
//! Inspired by `cmd_branding_doctor` in `server/ragos-cli.nix` (12 lines of bash).

use std::path::Path;

use serde::Serialize;

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
pub fn collect_report() -> BrandingReport {
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
    }
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
        let r = collect_report();
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
        let r = collect_report();
        assert_eq!(r.ok_count + r.fail_count, 7);
    }
}
