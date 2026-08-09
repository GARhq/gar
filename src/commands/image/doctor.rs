//! `gar image doctor` — health check for image infrastructure.
//!
//! Replaces `ragc doctor` (commands/doctor.sh, 208 LOC).
//! Checks services, mounts, symlinks, manifests, HTTP endpoints.

use std::path::Path;

use owo_colors::OwoColorize;
use serde::Serialize;

use crate::config::Config;
use crate::error::Result;
use crate::output;

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub checks: Vec<Check>,
    pub ok: usize,
    pub fail: usize,
    pub skipped: usize,
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: String,
    pub status: String, // "OK", "FAIL", "SKIP"
    pub detail: Option<String>,
}

/// Run the doctor command.
pub async fn run() -> Result<()> {
    let cfg = Config::from_env()?;

    let skip_services = std::env::var("GAR_SKIP_SERVICE_CHECKS").as_deref() == Ok("1");
    let skip_http = std::env::var("GAR_SKIP_HTTP_CHECKS").as_deref() == Ok("1");

    let mut checks = Vec::new();

    // Services
    for svc in &["dnsmasq", "nginx", "nfs-server"] {
        checks.push(check_service(svc, skip_services));
    }

    // Directories
    checks.push(check_dir(&cfg.data_root, "data dir"));
    checks.push(check_dir(&cfg.images_root, "images dir"));
    checks.push(check_dir(&cfg.data_root.join("home"), "home dir"));
    checks.push(check_dir(&cfg.http_root, "http root"));

    // TFTP / iPXE files
    checks.push(check_file(
        &cfg.tftp_root.join("EFI/BOOT/BOOTX64.EFI"),
        "BOOTX64.EFI",
    ));
    checks.push(check_symlink(&cfg.http_root.join("netboot"), "netboot link"));
    checks.push(check_file(&cfg.http_root.join("boot.ipxe"), "boot.ipxe"));
    checks.push(check_file(&cfg.http_root.join("current.ipxe"), "current.ipxe"));
    checks.push(check_file(&cfg.http_root.join("rescue.ipxe"), "rescue.ipxe"));

    // Image pointers
    for ptr in &["current", "previous", "rescue", "staged"] {
        let path = cfg.images_root.join(ptr);
        if path.is_symlink() {
            checks.push(check_symlink(&path, &format!("{} link", ptr)));
        }
    }

    // Channel pointers
    for ch in &["generic", "lab", "rescue"] {
        for prefix in &["current-", "previous-", "staged-"] {
            let name = format!("{}{}", prefix, ch);
            let path = cfg.images_root.join(&name);
            if path.is_symlink() {
                checks.push(check_symlink(&path, &format!("{} link", name)));
            }
        }
    }

    // HTTP endpoints
    if !skip_http {
        for (path, label) in &[
            ("boot.ipxe", "http boot"),
            ("current.ipxe", "http current"),
            ("netboot/current/manifest.json", "http manifest"),
        ] {
            let url = format!("http://{}:{}/{}", cfg.server_ip, cfg.http_port, path);
            checks.push(check_http(&url, label));
        }
    }

    // Active manifest count (should be exactly 1)
    checks.push(check_active_manifest_count(&cfg.images_root));

    // Tally
    let ok = checks.iter().filter(|c| c.status == "OK").count();
    let fail = checks.iter().filter(|c| c.status == "FAIL").count();
    let skipped = checks.iter().filter(|c| c.status == "SKIP").count();

    let report = DoctorReport {
        checks,
        ok,
        fail,
        skipped,
    };

    if cfg.json_output {
        output::json(&report)?;
    } else {
        output::section("GAR Image Health Check");
        println!();
        for c in &report.checks {
            print_check(c);
        }
        println!();
        if fail == 0 {
            output::ok(format!("GAR server healthy ({} OK)", ok));
        } else {
            output::err(format!(
                "GAR server has issues ({} FAIL, {} OK)",
                fail, ok
            ));
            println!();
            println!("Dicas:");
            println!("  - Verifique serviços: systemctl status dnsmasq nginx nfs-server");
            println!("  - Execute: gar image list");
            println!("  - Execute: gar image status");
            println!("  - Corrija qualquer divergência antes de promover nova geração");
        }
    }

    if fail > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn check_service(name: &str, skip: bool) -> Check {
    if skip {
        return Check {
            name: name.into(),
            status: "SKIP".into(),
            detail: None,
        };
    }
    let output_result = std::process::Command::new("systemctl")
        .args(["is-active", "--quiet", name])
        .output();
    let status = match output_result {
        Ok(o) if o.status.success() => "OK",
        _ => "FAIL",
    };
    Check {
        name: name.into(),
        status: status.into(),
        detail: None,
    }
}

fn check_dir(path: &Path, label: &str) -> Check {
    let status = if path.is_dir() { "OK" } else { "FAIL" };
    Check {
        name: label.into(),
        status: status.into(),
        detail: None,
    }
}

fn check_file(path: &Path, label: &str) -> Check {
    let status = if path.is_file() { "OK" } else { "FAIL" };
    Check {
        name: label.into(),
        status: status.into(),
        detail: None,
    }
}

fn check_symlink(path: &Path, label: &str) -> Check {
    if !path.is_symlink() {
        return Check {
            name: label.into(),
            status: "FAIL".into(),
            detail: Some("não é symlink".into()),
        };
    }
    match std::fs::read_link(path) {
        Ok(target) if target.exists() => Check {
            name: label.into(),
            status: "OK".into(),
            detail: None,
        },
        Ok(_) => Check {
            name: label.into(),
            status: "FAIL".into(),
            detail: Some("target não existe".into()),
        },
        Err(e) => Check {
            name: label.into(),
            status: "FAIL".into(),
            detail: Some(format!("readlink falhou: {}", e)),
        },
    }
}

fn check_http(url: &str, label: &str) -> Check {
    let output = std::process::Command::new("curl")
        .args(["-sS", "--max-time", "2", "-o", "/dev/null", "-w", "%{http_code}", url])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let code = String::from_utf8_lossy(&o.stdout);
            let status = if code.starts_with('2') || code.starts_with('3') {
                "OK"
            } else {
                "FAIL"
            };
            Check {
                name: label.into(),
                status: status.into(),
                detail: Some(format!("HTTP {}", code)),
            }
        }
        _ => Check {
            name: label.into(),
            status: "FAIL".into(),
            detail: Some("curl falhou".into()),
        },
    }
}

fn check_active_manifest_count(images_root: &Path) -> Check {
    let count = std::fs::read_dir(images_root)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| {
                    let path = e.path();
                    if !path.is_dir() {
                        return false;
                    }
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !name.starts_with('v') {
                        return false;
                    }
                    // Check status field in manifest
                    let manifest = std::fs::read_to_string(path.join("manifest.json")).ok();
                    manifest
                        .as_ref()
                        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                        .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(|s| s == "active"))
                        .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(0);

    let status = if count == 1 { "OK" } else { "FAIL" };
    Check {
        name: "active manifest count".into(),
        status: status.into(),
        detail: Some(format!("{} (esperado 1)", count)),
    }
}

fn print_check(c: &Check) {
    let status_colored = match c.status.as_str() {
        "OK" => "OK".green().bold().to_string(),
        "FAIL" => "FAIL".red().bold().to_string(),
        "SKIP" => "SKIP".yellow().to_string(),
        _ => c.status.dimmed().to_string(),
    };
    let label_colored = c.name.bold();
    if let Some(detail) = &c.detail {
        println!("  {:<28} {}  ({})", label_colored, status_colored, detail);
    } else {
        println!("  {:<28} {}", label_colored, status_colored);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_dir_existing() {
        let tmp = std::env::temp_dir().join(format!("gar-doctor-{}-dir", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let c = check_dir(&tmp, "test");
        assert_eq!(c.status, "OK");
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn test_check_dir_missing() {
        let tmp = std::env::temp_dir().join(format!("gar-doctor-missing-{}.nope", std::process::id()));
        let c = check_dir(&tmp, "test");
        assert_eq!(c.status, "FAIL");
    }

    #[test]
    fn test_check_file_existing() {
        let tmp = std::env::temp_dir().join(format!("gar-doctor-{}.txt", std::process::id()));
        std::fs::write(&tmp, "test").unwrap();
        let c = check_file(&tmp, "test");
        assert_eq!(c.status, "OK");
        std::fs::remove_file(&tmp).unwrap();
    }

    #[test]
    fn test_check_symlink_broken() {
        let tmp = std::env::temp_dir().join(format!("gar-doctor-{}-link", std::process::id()));
        std::os::unix::fs::symlink("/nonexistent/path", &tmp).unwrap();
        let c = check_symlink(&tmp, "test");
        assert_eq!(c.status, "FAIL");
        std::fs::remove_file(&tmp).unwrap();
    }
}