//! `gar image doctor` — health check for image infrastructure.
//!
//! Replaces `ragc doctor` (commands/doctor.sh, 208 LOC).
//! Checks services, mounts, symlinks, manifests, HTTP endpoints.

use std::path::Path;

use serde::Serialize;

use crate::config::Config;
use crate::error::Result;
use crate::output;

const IMAGE_POINTERS: [&str; 4] = ["current", "previous", "rescue", "staged"];
const CHANNELS: [&str; 3] = ["generic", "lab", "rescue"];
const CHANNEL_POINTER_PREFIXES: [&str; 3] = ["current-", "previous-", "staged-"];

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
    collect_infra_checks(&cfg, &mut checks, skip_services, skip_http);
    collect_integrity_checks(&cfg, &mut checks);

    let (report, fail) = tally(checks);
    render(&cfg, &report, fail);

    if fail > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn collect_infra_checks(
    cfg: &Config,
    checks: &mut Vec<Check>,
    skip_services: bool,
    skip_http: bool,
) {
    for svc in ["dnsmasq", "nginx", "nfs-server"] {
        checks.push(check_service(svc, skip_services));
    }

    checks.push(check_dir(&cfg.data_root, "data dir"));
    checks.push(check_dir(&cfg.images_root, "images dir"));
    checks.push(check_dir(&cfg.data_root.join("home"), "home dir"));
    checks.push(check_dir(&cfg.http_root, "http root"));

    checks.push(check_file(
        &cfg.tftp_root.join("EFI/BOOT/BOOTX64.EFI"),
        "BOOTX64.EFI",
    ));
    checks.push(check_symlink(
        &cfg.http_root.join("netboot"),
        "netboot link",
    ));
    checks.push(check_file(&cfg.http_root.join("boot.ipxe"), "boot.ipxe"));
    checks.push(check_file(
        &cfg.http_root.join("current.ipxe"),
        "current.ipxe",
    ));
    checks.push(check_file(
        &cfg.http_root.join("rescue.ipxe"),
        "rescue.ipxe",
    ));

    for ptr in IMAGE_POINTERS {
        let path = cfg.images_root.join(ptr);
        if path.is_symlink() {
            checks.push(check_symlink(&path, &format!("{} link", ptr)));
        }
    }

    for ch in CHANNELS {
        for prefix in CHANNEL_POINTER_PREFIXES {
            let name = format!("{}{}", prefix, ch);
            let path = cfg.images_root.join(&name);
            if path.is_symlink() {
                checks.push(check_symlink(&path, &format!("{} link", name)));
            }
        }
    }

    if !skip_http {
        for (path, label) in [
            ("boot.ipxe", "http boot"),
            ("current.ipxe", "http current"),
            ("netboot/current/manifest.json", "http manifest"),
        ] {
            let url = format!("http://{}:{}/{}", cfg.server_ip, cfg.http_port, path);
            checks.push(check_http(&url, label));
        }
    }

    checks.push(check_active_manifest_count(&cfg.images_root));
}

fn collect_integrity_checks(cfg: &Config, checks: &mut Vec<Check>) {
    if let Some(current_dir) = current_build_dir(&cfg.images_root) {
        checks.push(check_manifest_checksum_kernel(&current_dir));
        checks.push(check_manifest_checksum_initrd(&current_dir));
        checks.push(check_manifest_signature(&current_dir, &cfg.flake_path));
        checks.push(check_public_key_match(&cfg.flake_path));
    } else {
        for name in [
            "manifest-checksum-kernel",
            "manifest-checksum-initrd",
            "manifest-signature",
            "public-key-match",
        ] {
            checks.push(Check {
                name: name.into(),
                status: "SKIP".into(),
                detail: Some("sem build atual (current symlink ausente)".into()),
            });
        }
    }
    checks.push(check_http_serve_content(cfg));
}

fn tally(checks: Vec<Check>) -> (DoctorReport, usize) {
    let ok = checks.iter().filter(|c| c.status == "OK").count();
    let fail = checks.iter().filter(|c| c.status == "FAIL").count();
    let skipped = checks.iter().filter(|c| c.status == "SKIP").count();
    (
        DoctorReport {
            checks,
            ok,
            fail,
            skipped,
        },
        fail,
    )
}

fn render(cfg: &Config, report: &DoctorReport, fail: usize) {
    if cfg.json_output {
        output::json(report).ok();
        return;
    }
    output::header("GAR Image Health Check");
    let rows: Vec<Vec<String>> = report
        .checks
        .iter()
        .map(|c| {
            vec![
                c.name.clone(),
                status_label(&c.status).to_string(),
                c.detail.clone().unwrap_or_default(),
            ]
        })
        .collect();
    output::table(&["CHECK", "STATUS", "DETAIL"], &rows);
    output::blank_line();
    if fail == 0 {
        output::ok(format!("GAR server healthy ({} OK)", report.ok));
    } else {
        output::err(format!(
            "GAR server has issues ({} FAIL, {} OK)",
            fail, report.ok
        ));
        output::blank_line();
        output::panel(
            "Dicas",
            &[
                "Verifique serviços: systemctl status dnsmasq nginx nfs-server".into(),
                "Execute: gar image list".into(),
                "Execute: gar image status".into(),
                "Corrija qualquer divergência antes de promover nova geração".into(),
            ],
        );
    }
}

/// Compact label used inside the `STATUS` column.
fn status_label(status: &str) -> &str {
    match status {
        "OK" => "✓ OK",
        "FAIL" => "✗ FAIL",
        "SKIP" => "▲ SKIP",
        other => other,
    }
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
        .args([
            "-sS",
            "--max-time",
            "2",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            url,
        ])
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
                        .and_then(|v| {
                            v.get("status")
                                .and_then(|s| s.as_str())
                                .map(|s| s == "active")
                        })
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

// ─────────────────────────────────────────────────────────────────────────────
// K-2026-10-06: artifact-level integrity checks (Phase C regression suite).
//
// These five checks catch the class of bug where the manifest, the
// artifacts, the signature, and the public key all "look" present but
// are mutually inconsistent — exactly the silent failure mode that
// bit Phase C of the GAROS rebuild.
// ─────────────────────────────────────────────────────────────────────────────

/// Resolve the `current` build directory, or `None` if no current build
/// is published. Used to anchor the artifact-level checks below.
fn current_build_dir(images_root: &Path) -> Option<std::path::PathBuf> {
    let current = images_root.join("current");
    let target = std::fs::read_link(&current).ok()?;
    let name = target.file_name()?;
    Some(images_root.join(name))
}

/// Recalculate SHA-256 of bzImage and compare with manifest.
fn check_manifest_checksum_kernel(image_path: &Path) -> Check {
    let manifest = match read_manifest(image_path) {
        Ok(m) => m,
        Err(e) => return fail("manifest-checksum-kernel", e),
    };
    let actual = match crate::services::build::sha256_file(&image_path.join("bzImage")) {
        Ok(s) => s,
        Err(e) => return fail("manifest-checksum-kernel", format!("sha256 falhou: {e}")),
    };
    compare_checksum(
        "manifest-checksum-kernel",
        &actual,
        &manifest.checksums.kernel,
    )
}

/// Recalculate SHA-256 of initrd and compare with manifest.
fn check_manifest_checksum_initrd(image_path: &Path) -> Check {
    let manifest = match read_manifest(image_path) {
        Ok(m) => m,
        Err(e) => return fail("manifest-checksum-initrd", e),
    };
    let actual = match crate::services::build::sha256_file(&image_path.join("initrd")) {
        Ok(s) => s,
        Err(e) => return fail("manifest-checksum-initrd", format!("sha256 falhou: {e}")),
    };
    compare_checksum(
        "manifest-checksum-initrd",
        &actual,
        &manifest.checksums.initrd,
    )
}

/// Resolve the runtime keys directory: honor `GAR_KEYS_DIR` if set
/// and non-empty, otherwise fall back to the well-known default.
fn keys_dir() -> std::path::PathBuf {
    match std::env::var("GAR_KEYS_DIR") {
        Ok(v) if !v.is_empty() => std::path::PathBuf::from(v),
        _ => std::path::PathBuf::from("/var/lib/garos/keys"),
    }
}

/// Verify Ed25519 signature of manifest against `keys/garos.pub`.
///
/// `flake_path` is currently unused (the keys directory is the source
/// of trust, not the flake checkout) but is kept in the signature for
/// symmetry with `check_public_key_match` and to make future refactors
/// (e.g. read the expected public key from the flake) easier.
fn check_manifest_signature(image_path: &Path, flake_path: &Path) -> Check {
    let _ = flake_path; // reserved for future use
    let keys_dir = keys_dir();
    if !keys_dir.join("garos.key").is_file() {
        return Check {
            name: "manifest-signature".into(),
            status: "SKIP".into(),
            detail: Some(format!(
                "{} ausente — chaves nao geradas (rode `gar server init-keys` ou garos-init)",
                keys_dir.join("garos.key").display()
            )),
        };
    }
    let manifest_path = image_path.join("manifest.json");
    match crate::services::crypto::verify_manifest(&keys_dir, &manifest_path) {
        Ok(()) => Check {
            name: "manifest-signature".into(),
            status: "OK".into(),
            detail: None,
        },
        Err(e) => fail(
            "manifest-signature",
            format!("assinatura Ed25519 inválida: {e}"),
        ),
    }
}

/// Compare the public key in `<flake>/client/storage/garos.pub` (the
/// pinned source) against the live `keys/garos.pub` (the runtime
/// signer). If they differ, the operational checkout has drifted from
/// the source of truth — clients booted from the old key will reject
/// manifests signed with the new key.
fn check_public_key_match(flake_path: &Path) -> Check {
    let pinned = flake_path.join("client/storage/garos.pub");
    let runtime = keys_dir().join("garos.pub");
    let pinned_bytes = match std::fs::read(&pinned) {
        Ok(b) => b,
        Err(e) => {
            return fail(
                "public-key-match",
                format!("chave pinada ausente em {}: {e}", pinned.display()),
            );
        }
    };
    let runtime_bytes = match std::fs::read(&runtime) {
        Ok(b) => b,
        Err(e) => {
            return fail(
                "public-key-match",
                format!("chave runtime ausente em {}: {e}", runtime.display()),
            );
        }
    };
    if pinned_bytes == runtime_bytes {
        Check {
            name: "public-key-match".into(),
            status: "OK".into(),
            detail: None,
        }
    } else {
        fail(
            "public-key-match",
            format!(
                "pinned != runtime: pinned_sha256={} runtime_sha256={}",
                short_sha(&pinned_bytes),
                short_sha(&runtime_bytes)
            ),
        )
    }
}

/// GET the current bzImage via HTTP and confirm the body matches the
/// file on disk. Catches the case where nginx is serving stale content
/// (e.g. after a `mv` that didn't refresh the symlink, or a
/// worker process with cached file descriptor).
fn check_http_serve_content(cfg: &Config) -> Check {
    let url = format!(
        "http://{}:{}/netboot/current/bzImage",
        cfg.server_ip, cfg.http_port
    );
    let local = cfg.images_root.join("current").join("bzImage");
    let local_sha = match crate::services::build::sha256_file(&local) {
        Ok(s) => s,
        Err(e) => {
            return fail("http-serve-content", format!("local bzImage: {e}"));
        }
    };
    let out = std::process::Command::new("curl")
        .args(["-sS", "--max-time", "5", url.as_str()])
        .output();
    let body = match out {
        Ok(o) if o.status.success() => o.stdout,
        Ok(o) => {
            return fail(
                "http-serve-content",
                format!(
                    "curl exit {}: {}",
                    o.status,
                    String::from_utf8_lossy(&o.stderr)
                ),
            );
        }
        Err(e) => return fail("http-serve-content", format!("curl falhou: {e}")),
    };
    let remote_sha = short_sha(&body);
    if remote_sha == local_sha {
        Check {
            name: "http-serve-content".into(),
            status: "OK".into(),
            detail: None,
        }
    } else {
        fail(
            "http-serve-content",
            format!(
                "divergente: local={} remote={} (nginx pode estar servindo stale)",
                &local_sha[..12],
                &remote_sha[..12.min(remote_sha.len())]
            ),
        )
    }
}

// Helpers for the artifact-level checks above. Internal — not part of
// the public doctor API.

fn fail(name: &str, detail: impl Into<String>) -> Check {
    Check {
        name: name.into(),
        status: "FAIL".into(),
        detail: Some(detail.into()),
    }
}

fn read_manifest(
    image_path: &Path,
) -> std::result::Result<crate::services::manifest::Manifest, String> {
    let p = image_path.join("manifest.json");
    let bytes = std::fs::read(&p).map_err(|e| format!("manifest.json ilegível: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("manifest.json inválido: {e}"))
}

fn compare_checksum(name: &str, actual: &str, expected: &str) -> Check {
    if actual == expected {
        Check {
            name: name.into(),
            status: "OK".into(),
            detail: None,
        }
    } else {
        fail(
            name,
            format!(
                "divergente: disco={} manifest={}",
                &actual[..12.min(actual.len())],
                &expected[..12.min(expected.len())]
            ),
        )
    }
}

fn short_sha(bytes: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    format!("{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

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
        let tmp =
            std::env::temp_dir().join(format!("gar-doctor-missing-{}.nope", std::process::id()));
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

    // K-2026-10-06: 3 new tests for the artifact-level integrity checks.

    /// Confirms `current_build_dir` resolves the symlink target.
    #[test]
    fn test_current_build_dir_resolves_symlink() {
        let tmp = std::env::temp_dir().join(format!("gar-cbdr-{}", std::process::id()));
        let real = tmp.join("v-test-real");
        std::fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink("v-test-real", tmp.join("current")).unwrap();
        let resolved = current_build_dir(&tmp).expect("should resolve");
        assert!(resolved.ends_with("v-test-real"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Confirms `current_build_dir` returns None when the symlink is absent.
    #[test]
    fn test_current_build_dir_missing_returns_none() {
        let tmp = std::env::temp_dir().join(format!("gar-cbdm-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(current_build_dir(&tmp).is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// `compare_checksum` returns OK on match, FAIL on divergence.
    #[test]
    fn test_compare_checksum_match_and_diverge() {
        let ok = compare_checksum("test", "abc123", "abc123");
        assert_eq!(ok.status, "OK");
        let bad = compare_checksum("test", "abc123", "xyz789");
        assert_eq!(bad.status, "FAIL");
        assert!(bad.detail.unwrap().contains("divergente"));
    }

    // K-2026-10-12: regression tests for the 5 artifact-level checks
    // (doctor commit 6c89e48). Each test exercises the FAIL branch of a
    // check by feeding it a deliberately broken fixture.

    #[test]
    fn test_check_manifest_checksum_kernel_fails_on_divergence() {
        // Fixture: bzImage content is X, manifest claims SHA(Y).
        // Must FAIL with "divergente".
        let tmp =
            std::env::temp_dir().join(format!("gar-doc-kernel-{}-{}", std::process::id(), line!()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("bzImage"), b"real-kernel-bytes").unwrap();
        std::fs::write(tmp.join("initrd"), b"i").unwrap();
        // Manifest with a valid Manifest struct (needs all required fields
        // including `version` and `timestamp`) but a wrong kernel SHA.
        let manifest = serde_json::json!({
            "id": "v",
            "timestamp": "2026-01-01T00:00:00Z",
            "version": 1,
            "system_path": "/nix/store/s",
            "init_path": "/nix/store/i",
            "artifacts": {"kernel": "bzImage", "initrd": "initrd", "erofs": ""},
            "checksums": {
                "kernel": "0000000000000000000000000000000000000000000000000000000000000000",
                "initrd": "0000000000000000000000000000000000000000000000000000000000000000",
                "erofs": ""
            },
            "status": "active",
            "target": "t",
            "channel": "c",
            "hardwareClass": "physical-generic",
            "signature": ""
        });
        std::fs::write(
            tmp.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let c = check_manifest_checksum_kernel(&tmp);
        assert_eq!(c.status, "FAIL", "kernel SHA divergence must FAIL");
        let detail = c.detail.as_deref().unwrap_or("<no detail>");
        assert!(
            detail.contains("divergente"),
            "error should mention divergente: {detail}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_check_manifest_checksum_initrd_fails_on_divergence() {
        let tmp =
            std::env::temp_dir().join(format!("gar-doc-initrd-{}-{}", std::process::id(), line!()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("bzImage"), b"k").unwrap();
        std::fs::write(tmp.join("initrd"), b"real-initrd-bytes").unwrap();
        let manifest = serde_json::json!({
            "id": "v",
            "timestamp": "2026-01-01T00:00:00Z",
            "version": 1,
            "system_path": "/nix/store/s",
            "init_path": "/nix/store/i",
            "artifacts": {"kernel": "bzImage", "initrd": "initrd", "erofs": ""},
            "checksums": {
                "kernel": "0000000000000000000000000000000000000000000000000000000000000000",
                "initrd": "0000000000000000000000000000000000000000000000000000000000000000",
                "erofs": ""
            },
            "status": "active",
            "target": "t",
            "channel": "c",
            "hardwareClass": "physical-generic",
            "signature": ""
        });
        std::fs::write(
            tmp.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let c = check_manifest_checksum_initrd(&tmp);
        assert_eq!(c.status, "FAIL", "initrd SHA divergence must FAIL");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    #[serial(env_config)]
    fn test_check_public_key_match_fails_when_keys_diverge() {
        // Two different keys → must FAIL.
        let tmp =
            std::env::temp_dir().join(format!("gar-doc-pkm-{}-{}", std::process::id(), line!()));
        let pinned_dir = tmp.join("flake/client/storage");
        let runtime_dir = tmp.join("runtime-keys");
        std::fs::create_dir_all(&pinned_dir).unwrap();
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(pinned_dir.join("garos.pub"), b"pinned-key-v1").unwrap();
        std::fs::write(runtime_dir.join("garos.pub"), b"runtime-key-v2").unwrap();
        let flake_path = tmp.join("flake");

        // Scoped env override so check uses our runtime dir.
        let prev = std::env::var("GAR_KEYS_DIR").ok();
        std::env::set_var("GAR_KEYS_DIR", runtime_dir.as_os_str());
        let c = check_public_key_match(&flake_path);
        // Restore before assert (so a failing assert still leaves env sane).
        match prev {
            Some(v) => std::env::set_var("GAR_KEYS_DIR", v),
            None => std::env::remove_var("GAR_KEYS_DIR"),
        }
        assert_eq!(c.status, "FAIL", "divergent keys must FAIL");
        assert!(c.detail.unwrap().contains("pinned != runtime"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    #[serial(env_config)]
    fn test_check_public_key_match_passes_when_keys_match() {
        // Same bytes both sides → must pass.
        let tmp =
            std::env::temp_dir().join(format!("gar-doc-pkm-ok-{}-{}", std::process::id(), line!()));
        let pinned_dir = tmp.join("flake/client/storage");
        let runtime_dir = tmp.join("runtime-keys");
        std::fs::create_dir_all(&pinned_dir).unwrap();
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(pinned_dir.join("garos.pub"), b"identical-key").unwrap();
        std::fs::write(runtime_dir.join("garos.pub"), b"identical-key").unwrap();
        let flake_path = tmp.join("flake");

        let prev = std::env::var("GAR_KEYS_DIR").ok();
        std::env::set_var("GAR_KEYS_DIR", runtime_dir.as_os_str());
        let c = check_public_key_match(&flake_path);
        match prev {
            Some(v) => std::env::set_var("GAR_KEYS_DIR", v),
            None => std::env::remove_var("GAR_KEYS_DIR"),
        }
        assert_eq!(c.status, "OK", "matching keys must pass");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_check_public_key_match_fails_when_pinned_missing() {
        // No pinned key on disk → must FAIL (operator must commit it).
        let tmp = std::env::temp_dir().join(format!(
            "gar-doc-pkm-nopin-{}-{}",
            std::process::id(),
            line!()
        ));
        let pinned_dir = tmp.join("flake/client/storage");
        std::fs::create_dir_all(&pinned_dir).unwrap();
        // No garos.pub written.
        let flake_path = tmp.join("flake");
        let c = check_public_key_match(&flake_path);
        assert_eq!(c.status, "FAIL", "missing pinned key must FAIL");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
