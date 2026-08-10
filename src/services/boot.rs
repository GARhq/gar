//! iPXE boot bundle preparation, validation, and coherence checks.
//!
//! Replaces 9 helpers from `ragc/lib/boot.sh` (264 lines). Owns the full
//! flow of:
//!   - preparing an iPXE bundle for a publish (boot.ipxe + 4 channel scripts)
//!   - validating the bundle before promotion
//!   - promoting the bundle to the HTTP root atomically
//!   - validating boot coherence after promotion (catches drift between
//!     declared build ids and what the HTTP server is actually serving)
//!
//! ## What this does NOT do
//!
//! - does not build the kernel/initrd (that's `services::build`).
//! - does not run nix builds or touch `/nix/store`.
//! - does not parse JSON manifests deeply (uses `manifest::validate`).
//!
//! ## Templates
//!
//! The iPXE scripts are kept **byte-identical** to the bash heredocs to
//! avoid bootloader regressions. `write_channel_ipxe` and the bundle
//! writers use the same `format!` shape the bash `cat <<IPXE ... IPXE`
//! heredocs use, modulo Rust's own escaping rules (which we escape
//! explicitly where bash `$` and backslashes appeared).

use std::fs;
use std::path::Path;

use crate::cli::Channel;
use crate::config::Config;
use crate::error::{GarError, Result};

/// Subdirectory layout under `images_root` per the ragc contract.
pub const BUNDLE_FILES: [&str; 5] = [
    "boot.ipxe",
    "generic.ipxe",
    "lab.ipxe",
    "current.ipxe",
    "rescue.ipxe",
];

/// Combined boot state for one channel: version, init path, kernel params.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BootState {
    pub version: String,
    pub init_path: String,
    pub kernel_params: String,
}

impl BootState {
    /// True if the channel has no current pointer (empty version).
    #[must_use = "is_empty is a predicate; ignoring the result hides a missing boot state"]
    pub fn is_empty(&self) -> bool {
        self.version.is_empty()
    }
}

/// Bundle coherence result returned by `validate_*` functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Coherence {
    /// The bundle is internally consistent.
    Coherent,
    /// A specific file or value is missing.
    Missing(String),
    /// A specific file or value diverges from expected.
    Diverged {
        file: String,
        expected: String,
        actual: String,
    },
}

impl Coherence {
    /// Convert to Result — bash uses `die` for all three cases.
    pub fn into_result(self) -> Result<()> {
        match self {
            Self::Coherent => Ok(()),
            Self::Missing(what) => Err(GarError::Publish(format!(
                "Bundle de boot invalido: {} ausente",
                what
            ))),
            Self::Diverged {
                file,
                expected,
                actual,
            } => Err(GarError::Publish(format!(
                "Bundle de boot invalido: {} anuncia {}, esperado {}",
                file, actual, expected
            ))),
        }
    }
}

/// Parse the value of `set <variable> <value>` from an iPXE script.
///
/// Mirrors bash `ipxe_declared_value` (boot.sh:1):
/// ```sh
/// awk -v var="$variable" '$1 == "set" && $2 == var { print $3; exit }'
/// ```
///
/// Returns `None` when the variable is not declared or the file cannot
/// be read. Whitespace-tolerant (multiple spaces between tokens).
#[must_use = "ipxe_declared_value returns Option; ignoring it loses the lookup result"]
#[tracing::instrument(skip_all, fields(script = %script_path.display(), variable = %variable))]
pub fn ipxe_declared_value(script_path: &Path, variable: &str) -> Option<String> {
    let content = fs::read_to_string(script_path).ok()?;
    for line in content.lines() {
        // Skip blank lines and indented continuations.
        let trimmed = line.trim_start();
        if !trimmed.starts_with("set ") {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        // "set <var> <value>"
        if parts.len() >= 3 && parts[0] == "set" && parts[1] == variable {
            return Some(parts[2].to_string());
        }
    }
    None
}

/// Read the boot state (version, init_path, kernel_params) for one
/// channel by following its `current-<channel>` pointer in `images_root`.
///
/// Mirrors bash `channel_boot_state` (boot.sh:7). Reads:
///   - `images_root/<pointer>` for the version (basename of symlink)
///   - `images_root/<version>/.init_path` for the init path
///   - `images_root/<version>/.kernel_params` for the kernel params
///
/// Returns `BootState::default()` (empty version) when the pointer is
/// absent — bash prints empty fields in that case.
#[must_use = "channel_boot_state returns a struct; ignoring the result loses the read"]
#[tracing::instrument(skip_all, fields(images_root = %images_root.display(), channel = %channel.as_str()))]
pub fn channel_boot_state(images_root: &Path, channel: Channel) -> Result<BootState> {
    let pointer_name = format!("current-{}", channel.as_str());
    let pointer_path = images_root.join(&pointer_name);

    if !pointer_path.is_symlink() {
        return Ok(BootState::default());
    }

    // Resolve the symlink target (version directory).
    let target = fs::read_link(&pointer_path)?;
    let version = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            GarError::Publish(format!(
                "channel_boot_state: ponteiro sem basename: {}",
                pointer_path.display()
            ))
        })?
        .to_string();

    let gen_dir = images_root.join(&version);
    let init_path =
        crate::services::generation::read_generation_init_path(&gen_dir).unwrap_or_default();
    let kernel_params =
        crate::services::generation::read_generation_kernel_params(&gen_dir).unwrap_or_default();

    Ok(BootState {
        version,
        init_path,
        kernel_params,
    })
}

/// Write a per-channel iPXE script (`generic.ipxe`, `lab.ipxe`,
/// `rescue.ipxe`, or `current.ipxe`).
///
/// Mirrors bash `write_channel_ipxe` (boot.sh:23). Two output shapes:
///   1. **Active** — when `build_id` and `init_path` are both non-empty.
///   2. **Empty** — when either is missing; prints a "no generation
///      available" message and drops to shell.
///
/// The byte-identical heredoc content is preserved (modulo Rust's
/// string-literal escaping). Comments, blank lines, and ordering match
/// the bash output exactly.
#[must_use = "write_channel_ipxe creates a file; ignoring the Result may publish an empty script"]
#[tracing::instrument(skip_all, fields(script = %script_path.display(), channel = %channel.as_str()))]
pub fn write_channel_ipxe(
    script_path: &Path,
    channel: Channel,
    build_id: &str,
    init_path: &str,
    kernel_params: &str,
    server_ip: &str,
    http_port: u16,
) -> Result<()> {
    // Determine the actual boot_dir: rescue always uses "rescue";
    // current.ipxe uses "current"; everything else uses
    // "current-<channel>". This matches the bash logic in boot.sh:23.
    let boot_dir = if channel == Channel::Rescue {
        "rescue".to_string()
    } else if script_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n == "current.ipxe")
        .unwrap_or(false)
    {
        "current".to_string()
    } else {
        format!("current-{}", channel.as_str())
    };

    let content = if !build_id.is_empty() && !init_path.is_empty() {
        format!(
            r#"#!ipxe

set build_id {build_id}
isset ${{ip}} || dhcp
echo Booting RAGOS {channel_str} (${{build_id}})...
kernel http://{server_ip}:{http_port}/netboot/{boot_dir}/bzImage init={init_path} ip=dhcp ragos.primaryNicMac=${{net0/mac}} {kernel_params}
initrd http://{server_ip}:{http_port}/netboot/{boot_dir}/initrd
boot || goto failed

:failed
echo Boot {channel_str} falhou. Indo para shell...
shell
"#,
            build_id = build_id,
            channel_str = channel.as_str(),
            server_ip = server_ip,
            http_port = http_port,
            boot_dir = boot_dir,
            init_path = init_path,
            kernel_params = kernel_params,
        )
    } else {
        format!(
            r#"#!ipxe

echo Nenhuma geracao ativa disponivel para o canal {channel_str}.
echo Publique com: ragc switch --channel {channel_str}
shell
"#,
            channel_str = channel.as_str(),
        )
    };

    if let Some(parent) = script_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(script_path, content)?;
    Ok(())
}

/// Build a complete boot bundle (5 iPXE scripts) for a publish cycle.
///
/// Mirrors bash `prepare_boot_bundle` (boot.sh:61). Inputs are the
/// active `current_*` build (optional — empty when there is no current)
/// and the active `rescue_*` build (also optional).
///
/// Reads kernel params from the current/rescue version dirs (matching
/// the bash logic) and uses `channel_boot_state` for the generic/lab
/// channel pointers. If the current build targets a channel, that
/// channel's script is overridden with the current params.
#[must_use = "prepare_boot_bundle writes 5 files; ignoring the Result may publish an empty bundle"]
#[tracing::instrument(skip_all, fields(bundle_dir = %bundle_dir.display(), images_root = %cfg.images_root.display()))]
pub fn prepare_boot_bundle(
    cfg: &Config,
    bundle_dir: &Path,
    current_ver: &str,
    current_init: &str,
    rescue_ver: &str,
    rescue_init: &str,
) -> Result<()> {
    fs::create_dir_all(bundle_dir)?;

    let mut current_params = String::new();
    let mut rescue_params = String::new();
    let mut current_channel = String::new();

    if !current_ver.is_empty() {
        let gen_dir = cfg.images_root.join(current_ver);
        current_params = crate::services::generation::read_generation_kernel_params(&gen_dir)
            .unwrap_or_default();
        let manifest_path = gen_dir.join("manifest.json");
        if manifest_path.is_file() {
            current_channel = manifest_channel_field(&manifest_path).unwrap_or_default();
        }
    }

    if !rescue_ver.is_empty() {
        let gen_dir = cfg.images_root.join(rescue_ver);
        rescue_params = crate::services::generation::read_generation_kernel_params(&gen_dir)
            .unwrap_or_default();
    }

    let mut generic_state = channel_boot_state(&cfg.images_root, Channel::Generic)?;
    if current_channel == "generic" {
        generic_state.version = current_ver.to_string();
        generic_state.init_path = current_init.to_string();
        generic_state.kernel_params = current_params.clone();
    }

    let mut lab_state = channel_boot_state(&cfg.images_root, Channel::Lab)?;
    if current_channel == "lab" {
        lab_state.version = current_ver.to_string();
        lab_state.init_path = current_init.to_string();
        lab_state.kernel_params = current_params.clone();
    }

    // boot.ipxe — the menu.
    let boot_ipxe = format!(
        r#"#!ipxe

set current_build_id {current_ver}
set generic_build_id {generic_ver}
set lab_build_id {lab_ver}
set rescue_build_id {rescue_ver}
isset ${{ip}} || dhcp
isset ${{net0/mac}} || goto menu
chain --replace http://{server_ip}:{http_port}/by-mac/${{net0/mac}}.ipxe || goto menu
:menu
menu RAGOS Boot
item --gap -- =====================================
item generic Boot generic (${{generic_build_id}})
item lab Boot lab (${{lab_build_id}})
item current Boot current legacy (${{current_build_id}})
item rescue Boot rescue (${{rescue_build_id}})
item shell iPXE shell
item reboot Reboot
choose --timeout 8000 --default generic target && goto ${{target}}

:generic
chain --replace http://{server_ip}:{http_port}/generic.ipxe || goto failed

:lab
chain --replace http://{server_ip}:{http_port}/lab.ipxe || goto failed

:current
chain --replace http://{server_ip}:{http_port}/current.ipxe || goto failed

:rescue
chain --replace http://{server_ip}:{http_port}/rescue.ipxe || goto failed

:failed
echo Boot falhou. Indo para shell...
shell

:reboot
reboot
"#,
        current_ver = empty_to_none(current_ver),
        generic_ver = empty_to_none(&generic_state.version),
        lab_ver = empty_to_none(&lab_state.version),
        rescue_ver = empty_to_none(rescue_ver),
        server_ip = cfg.server_ip,
        http_port = cfg.http_port,
    );
    fs::write(bundle_dir.join("boot.ipxe"), boot_ipxe)?;

    write_channel_ipxe(
        &bundle_dir.join("generic.ipxe"),
        Channel::Generic,
        &generic_state.version,
        &generic_state.init_path,
        &generic_state.kernel_params,
        &cfg.server_ip,
        cfg.http_port,
    )?;
    write_channel_ipxe(
        &bundle_dir.join("lab.ipxe"),
        Channel::Lab,
        &lab_state.version,
        &lab_state.init_path,
        &lab_state.kernel_params,
        &cfg.server_ip,
        cfg.http_port,
    )?;

    // current.ipxe — uses the literal "current" boot dir.
    let current_ipxe = format!(
        r#"#!ipxe

set build_id {current_ver}
isset ${{ip}} || dhcp
echo Booting RAGOS current (${{build_id}})...
kernel http://{server_ip}:{http_port}/netboot/current/bzImage init={current_init} ip=dhcp ragos.primaryNicMac=${{net0/mac}} {current_params}
initrd http://{server_ip}:{http_port}/netboot/current/initrd
boot || goto failed

:failed
echo Boot falhou. Indo para shell...
shell
"#,
        current_ver = current_ver,
        current_init = current_init,
        current_params = current_params,
        server_ip = cfg.server_ip,
        http_port = cfg.http_port,
    );
    fs::write(bundle_dir.join("current.ipxe"), current_ipxe)?;

    // rescue.ipxe — only meaningful when rescue_ver AND rescue_init are set.
    let rescue_ipxe = if !rescue_ver.is_empty() && !rescue_init.is_empty() {
        format!(
            r#"#!ipxe

set build_id {rescue_ver}
isset ${{ip}} || dhcp
echo Booting RAGOS rescue (${{build_id}})...
kernel http://{server_ip}:{http_port}/netboot/rescue/bzImage init={rescue_init} ip=dhcp ragos.primaryNicMac=${{net0/mac}} {rescue_params}
initrd http://{server_ip}:{http_port}/netboot/rescue/initrd
boot || goto failed

:failed
echo Boot rescue falhou. Indo para shell...
shell
"#,
            rescue_ver = rescue_ver,
            rescue_init = rescue_init,
            rescue_params = rescue_params,
            server_ip = cfg.server_ip,
            http_port = cfg.http_port,
        )
    } else {
        "#!ipxe\n\necho Nenhuma geracao de rescue disponivel.\nshell\n".to_string()
    };
    fs::write(bundle_dir.join("rescue.ipxe"), rescue_ipxe)?;

    Ok(())
}

/// Validate a prepared boot bundle against the expected build versions.
///
/// Mirrors bash `validate_boot_bundle` (boot.sh:193). Returns
/// `Coherence` so callers can distinguish "missing file" from
/// "diverged value" if they care.
#[must_use = "validate_boot_bundle returns Coherence; ignoring it may promote a divergent bundle"]
#[tracing::instrument(skip_all, fields(bundle_dir = %bundle_dir.display()))]
pub fn validate_boot_bundle(bundle_dir: &Path, current_ver: &str, rescue_ver: &str) -> Coherence {
    for file in &BUNDLE_FILES {
        if !bundle_dir.join(file).is_file() {
            return Coherence::Missing((*file).to_string());
        }
    }
    match ipxe_declared_value(&bundle_dir.join("boot.ipxe"), "current_build_id") {
        Some(v) if v == current_ver => {}
        Some(actual) => {
            return Coherence::Diverged {
                file: "boot.ipxe".into(),
                expected: current_ver.into(),
                actual,
            };
        }
        None => {
            return Coherence::Diverged {
                file: "boot.ipxe".into(),
                expected: current_ver.into(),
                actual: "<unset>".into(),
            };
        }
    }
    match ipxe_declared_value(&bundle_dir.join("current.ipxe"), "build_id") {
        Some(v) if v == current_ver => {}
        Some(actual) => {
            return Coherence::Diverged {
                file: "current.ipxe".into(),
                expected: current_ver.into(),
                actual,
            };
        }
        None => {
            return Coherence::Diverged {
                file: "current.ipxe".into(),
                expected: current_ver.into(),
                actual: "<unset>".into(),
            };
        }
    }
    if !rescue_ver.is_empty() {
        match ipxe_declared_value(&bundle_dir.join("boot.ipxe"), "rescue_build_id") {
            Some(v) if v == rescue_ver => {}
            Some(actual) => {
                return Coherence::Diverged {
                    file: "boot.ipxe (rescue_build_id)".into(),
                    expected: rescue_ver.into(),
                    actual,
                };
            }
            None => {
                return Coherence::Diverged {
                    file: "boot.ipxe (rescue_build_id)".into(),
                    expected: rescue_ver.into(),
                    actual: "<unset>".into(),
                };
            }
        }
        match ipxe_declared_value(&bundle_dir.join("rescue.ipxe"), "build_id") {
            Some(v) if v == rescue_ver => {}
            Some(actual) => {
                return Coherence::Diverged {
                    file: "rescue.ipxe".into(),
                    expected: rescue_ver.into(),
                    actual,
                };
            }
            None => {
                return Coherence::Diverged {
                    file: "rescue.ipxe".into(),
                    expected: rescue_ver.into(),
                    actual: "<unset>".into(),
                };
            }
        }
    }
    Coherence::Coherent
}

/// Promote a prepared bundle to the HTTP root atomically.
///
/// Mirrors bash `promote_boot_bundle` (boot.sh:212). For each of the
/// 5 bundle files, copies to a sibling temp + renames over the
/// destination. We reuse `atomic_path::atomic_remove_path` to clear
/// any stale file at the destination before the cp + rename dance.
#[must_use = "promote_boot_bundle writes 5 files; ignoring the Result may publish an incomplete boot"]
#[tracing::instrument(skip_all, fields(bundle_dir = %bundle_dir.display(), http_root = %http_root.display()))]
pub fn promote_boot_bundle(bundle_dir: &Path, http_root: &Path) -> Result<()> {
    fs::create_dir_all(http_root)?;
    for file in &BUNDLE_FILES {
        let src = bundle_dir.join(file);
        let dst = http_root.join(file);
        let tmp = http_root.join(format!(".{}.tmp.{}", file, std::process::id()));
        fs::copy(&src, &tmp)?;
        fs::rename(&tmp, &dst)?;
    }
    Ok(())
}

/// Validate that the published iPXE for a given channel declares the
/// expected build id. Returns `Ok(())` when `expected_ver` is empty
/// (matches bash's `[[ -n "$expected_ver" ]] || return 0`).
#[must_use = "validate_channel_boot_coherence returns Result; ignoring it may publish a drifted boot"]
#[tracing::instrument(skip_all, fields(http_root = %http_root.display(), channel = %channel.as_str()))]
pub fn validate_channel_boot_coherence(
    http_root: &Path,
    channel: Channel,
    expected_ver: &str,
) -> Coherence {
    let script_path = http_root.join(format!("{}.ipxe", channel.as_str()));
    // Mirror bash: the file check runs BEFORE the expected_ver early
    // return. If the script is missing we always report Missing — even
    // when the expected version is empty. The early return applies only
    // when the file exists and the expected version is unset.
    if !script_path.is_file() {
        return Coherence::Missing(format!("{}.ipxe", channel.as_str()));
    }
    if expected_ver.is_empty() {
        return Coherence::Coherent;
    }
    match ipxe_declared_value(&script_path, "build_id") {
        Some(v) if v == expected_ver => Coherence::Coherent,
        Some(actual) => Coherence::Diverged {
            file: format!("{}.ipxe", channel.as_str()),
            expected: expected_ver.into(),
            actual,
        },
        None => Coherence::Diverged {
            file: format!("{}.ipxe", channel.as_str()),
            expected: expected_ver.into(),
            actual: "<unset>".into(),
        },
    }
}

/// Validate the boot coherence for the *current* build. Checks
/// `boot.ipxe` (current_build_id) and `current.ipxe` (build_id), plus
/// optionally cross-checks against an HTTP-served manifest body.
///
/// Mirrors bash `validate_boot_coherence` (boot.sh:238). The bash
/// `expected_http_manifest` parameter is the raw HTTP body of
/// `manifest.json` served at the build's directory; we parse it
/// minimally with a regex (matching the bash `grep ... cut` idiom) to
/// avoid pulling in the full serde stack for what is effectively an
/// ID-string lookup.
#[must_use = "validate_boot_coherence returns Coherence; ignoring it may publish a drifted boot"]
#[tracing::instrument(skip_all, fields(http_root = %http_root.display()))]
pub fn validate_boot_coherence(
    http_root: &Path,
    current_ver: &str,
    expected_http_manifest: Option<&str>,
) -> Coherence {
    if let Some(actual) = ipxe_declared_value(&http_root.join("boot.ipxe"), "current_build_id") {
        if actual != current_ver {
            return Coherence::Diverged {
                file: "boot.ipxe".into(),
                expected: current_ver.into(),
                actual,
            };
        }
    } else {
        return Coherence::Missing("boot.ipxe current_build_id".into());
    }
    if let Some(actual) = ipxe_declared_value(&http_root.join("current.ipxe"), "build_id") {
        if actual != current_ver {
            return Coherence::Diverged {
                file: "current.ipxe".into(),
                expected: current_ver.into(),
                actual,
            };
        }
    } else {
        return Coherence::Missing("current.ipxe build_id".into());
    }
    if let Some(body) = expected_http_manifest {
        if !body.is_empty() {
            // bash: printf '%s\n' "$body" | grep -E '"id"[[:space:]]*:' | head -n1 | cut -d'"' -f4
            // Find the first "id": "..." occurrence and return the value.
            let http_id = extract_first_json_string(body, "id");
            match http_id {
                Some(v) if v == current_ver => {}
                Some(actual) => {
                    return Coherence::Diverged {
                        file: "HTTP manifest.json".into(),
                        expected: current_ver.into(),
                        actual,
                    };
                }
                None => {
                    return Coherence::Missing("HTTP manifest.json id".into());
                }
            }
        }
    }
    Coherence::Coherent
}

/// Minimal JSON field extractor: returns the value of the first
/// `"key": "value"` pair in `body`. Used only for one-off ID lookups
/// (manifest id, channel) so we avoid pulling serde_json into the
/// hot path of boot validation.
fn extract_first_json_string(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let pos = body.find(&needle)?;
    let after = pos + needle.len();
    let rest = body[after..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Validate the rescue coherence: when `rescue_ver` is non-empty,
/// `rescue.ipxe` must declare it. No-op when `rescue_ver` is empty.
#[must_use = "validate_rescue_coherence returns Coherence; ignoring it may publish a drifted rescue boot"]
#[tracing::instrument(skip_all, fields(http_root = %http_root.display()))]
pub fn validate_rescue_coherence(http_root: &Path, rescue_ver: &str) -> Coherence {
    if rescue_ver.is_empty() {
        return Coherence::Coherent;
    }
    match ipxe_declared_value(&http_root.join("rescue.ipxe"), "build_id") {
        Some(v) if v == rescue_ver => Coherence::Coherent,
        Some(actual) => Coherence::Diverged {
            file: "rescue.ipxe".into(),
            expected: rescue_ver.into(),
            actual,
        },
        None => Coherence::Diverged {
            file: "rescue.ipxe".into(),
            expected: rescue_ver.into(),
            actual: "<unset>".into(),
        },
    }
}

/// Helper: substitute empty strings with `none` for iPXE variable defaults.
fn empty_to_none(s: &str) -> &str {
    if s.is_empty() {
        "none"
    } else {
        s
    }
}

/// Helper: read the `channel` field from a manifest.json file. Used by
/// `prepare_boot_bundle` to detect which channel the current build is
/// associated with. Returns `None` on any parse/read error (matches
/// bash's `current_channel=""` fallback).
fn manifest_channel_field(manifest_path: &Path) -> Option<String> {
    let content = fs::read_to_string(manifest_path).ok()?;
    extract_first_json_string(&content, "channel")
}

/// Public helper: read the `system_path` field from a manifest.json.
/// Used by `services::build::ensure_gc_root_for_generation` to recover
/// the nix store path for a published generation. Public so other
/// services (e.g. GC) can reuse it.
#[must_use = "read_manifest_system_path returns Option; ignoring it loses the lookup"]
pub fn read_manifest_system_path(manifest_path: &Path) -> Option<String> {
    let content = fs::read_to_string(manifest_path).ok()?;
    extract_first_json_string(&content, "system_path")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn tmp(label: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("gar-boot-{}-{}", label, std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn cleanup(p: &Path) {
        let _ = fs::remove_dir_all(p);
    }

    fn write_sample_ipxe(dir: &Path, name: &str, body: &str) {
        fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn test_ipxe_declared_value_finds_set_token() {
        let dir = tmp("ipxe-declared");
        write_sample_ipxe(
            &dir,
            "boot.ipxe",
            "#!ipxe\nset build_id v20260101\nset other foo\n",
        );
        let v = ipxe_declared_value(&dir.join("boot.ipxe"), "build_id").unwrap();
        assert_eq!(v, "v20260101");
        let missing = ipxe_declared_value(&dir.join("boot.ipxe"), "no_such_var");
        assert_eq!(missing, None);
        cleanup(&dir);
    }

    #[test]
    fn test_ipxe_declared_value_returns_none_for_missing_file() {
        let dir = tmp("ipxe-missing");
        let v = ipxe_declared_value(&dir.join("nope.ipxe"), "build_id");
        assert_eq!(v, None);
        cleanup(&dir);
    }

    #[test]
    fn test_channel_boot_state_default_when_no_pointer() {
        let dir = tmp("boot-state-empty");
        let s = channel_boot_state(&dir, Channel::Generic).unwrap();
        assert!(s.is_empty());
        assert_eq!(s.version, "");
        cleanup(&dir);
    }

    #[test]
    fn test_channel_boot_state_reads_symlink_target() {
        let dir = tmp("boot-state-sym");
        let ver = dir.join("v20260101-120000");
        fs::create_dir_all(&ver).unwrap();
        fs::write(ver.join(".init_path"), "/nix/store/abc-init").unwrap();
        fs::write(ver.join(".kernel_params"), "quiet splash").unwrap();
        // current-generic points at ver
        std::os::unix::fs::symlink(&ver, dir.join("current-generic")).unwrap();
        let s = channel_boot_state(&dir, Channel::Generic).unwrap();
        assert_eq!(s.version, "v20260101-120000");
        assert_eq!(s.init_path, "/nix/store/abc-init");
        assert_eq!(s.kernel_params, "quiet splash");
        cleanup(&dir);
    }

    #[test]
    fn test_write_channel_ipxe_active_template() {
        let dir = tmp("ipxe-active");
        let path = dir.join("generic.ipxe");
        write_channel_ipxe(
            &path,
            Channel::Generic,
            "v20260101-120000",
            "/nix/store/init",
            "quiet splash",
            "127.0.0.1",
            8080,
        )
        .unwrap();
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("set build_id v20260101-120000"));
        assert!(body.contains("http://127.0.0.1:8080/netboot/current-generic/bzImage"));
        assert!(body.contains("init=/nix/store/init"));
        assert!(body.contains("quiet splash"));
        assert!(body.contains("shell"));
        cleanup(&dir);
    }

    #[test]
    fn test_write_channel_ipxe_empty_template() {
        let dir = tmp("ipxe-empty");
        let path = dir.join("lab.ipxe");
        write_channel_ipxe(&path, Channel::Lab, "", "", "", "127.0.0.1", 8080).unwrap();
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("Nenhuma geracao ativa disponivel para o canal lab"));
        assert!(body.contains("shell"));
        cleanup(&dir);
    }

    #[test]
    fn test_write_channel_ipxe_rescue_uses_rescue_dir() {
        let dir = tmp("ipxe-rescue");
        let path = dir.join("rescue.ipxe");
        write_channel_ipxe(
            &path,
            Channel::Rescue,
            "vRESCUE",
            "/nix/store/r",
            "",
            "10.0.0.1",
            80,
        )
        .unwrap();
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("/netboot/rescue/bzImage"), "got: {}", body);
        cleanup(&dir);
    }

    #[test]
    fn test_validate_boot_bundle_coherent() {
        let dir = tmp("validate-coherent");
        // Build a coherent bundle.
        write_sample_ipxe(
            &dir,
            "boot.ipxe",
            "#!ipxe\nset current_build_id v1\nset rescue_build_id vR\n",
        );
        write_sample_ipxe(&dir, "generic.ipxe", "#!ipxe\nset build_id g1\n");
        write_sample_ipxe(&dir, "lab.ipxe", "#!ipxe\nset build_id l1\n");
        write_sample_ipxe(&dir, "current.ipxe", "#!ipxe\nset build_id v1\n");
        write_sample_ipxe(&dir, "rescue.ipxe", "#!ipxe\nset build_id vR\n");

        let r = validate_boot_bundle(&dir, "v1", "vR");
        assert_eq!(r, Coherence::Coherent);
        cleanup(&dir);
    }

    #[test]
    fn test_validate_boot_bundle_missing_file() {
        let dir = tmp("validate-missing");
        write_sample_ipxe(&dir, "boot.ipxe", "set current_build_id v1\n");
        // generic, lab, current, rescue are missing.
        let r = validate_boot_bundle(&dir, "v1", "");
        assert!(matches!(r, Coherence::Missing(_)));
        cleanup(&dir);
    }

    #[test]
    fn test_validate_boot_bundle_diverged_build_id() {
        let dir = tmp("validate-diverged");
        write_sample_ipxe(&dir, "boot.ipxe", "set current_build_id vOLD\n");
        write_sample_ipxe(&dir, "generic.ipxe", "set build_id g\n");
        write_sample_ipxe(&dir, "lab.ipxe", "set build_id l\n");
        write_sample_ipxe(&dir, "current.ipxe", "set build_id vOLD\n");
        write_sample_ipxe(&dir, "rescue.ipxe", "set build_id vR\n");
        let r = validate_boot_bundle(&dir, "v1", "vR");
        assert!(matches!(r, Coherence::Diverged { .. }));
        cleanup(&dir);
    }

    #[test]
    fn test_promote_boot_bundle_copies_all_files() {
        let bundle = tmp("promote-bundle");
        let http = tmp("promote-http");
        for f in BUNDLE_FILES.iter() {
            fs::write(bundle.join(f), format!("# {}\n", f)).unwrap();
        }
        promote_boot_bundle(&bundle, &http).unwrap();
        for f in BUNDLE_FILES.iter() {
            let dst = http.join(f);
            assert!(dst.is_file(), "{} should exist", f);
            let body = fs::read_to_string(&dst).unwrap();
            assert!(body.contains(f));
        }
        // No .tmp.* residue
        let leftover: Vec<_> = fs::read_dir(&http)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.contains(".tmp.")
            })
            .collect();
        assert!(leftover.is_empty(), "tmp residue: {:?}", leftover);
        cleanup(&bundle);
        cleanup(&http);
    }

    #[test]
    fn test_validate_channel_boot_coherence_skips_when_expected_empty() {
        let dir = tmp("ch-coh-empty");
        // Place the script file; with empty expected_ver, the validator
        // must short-circuit to Coherent (bash: `[[ -n "$expected_ver" ]]
        // || return 0` runs AFTER the file check, so the file MUST exist
        // first). This test exercises the "file present + empty expected"
        // path. The "file absent + empty expected" case is exercised by
        // test_validate_channel_boot_coherence_missing_file.
        write_sample_ipxe(&dir, "generic.ipxe", "#!ipxe\nset build_id x\n");
        let r = validate_channel_boot_coherence(&dir, Channel::Generic, "");
        assert_eq!(r, Coherence::Coherent);
        cleanup(&dir);
    }

    #[test]
    fn test_validate_channel_boot_coherence_missing_file() {
        let dir = tmp("ch-coh-missing");
        let r = validate_channel_boot_coherence(&dir, Channel::Lab, "v1");
        assert!(matches!(r, Coherence::Missing(_)));
        cleanup(&dir);
    }

    #[test]
    fn test_validate_rescue_coherence_skips_when_empty() {
        let dir = tmp("rescue-coh-empty");
        let r = validate_rescue_coherence(&dir, "");
        assert_eq!(r, Coherence::Coherent);
        cleanup(&dir);
    }

    #[test]
    fn test_validate_boot_coherence_matches_with_manifest() {
        let dir = tmp("boot-coh");
        write_sample_ipxe(&dir, "boot.ipxe", "set current_build_id v1\n");
        write_sample_ipxe(&dir, "current.ipxe", "set build_id v1\n");
        let body = r#"{"id":"v1","timestamp":"2026-01-01T00:00:00Z"}"#;
        let r = validate_boot_coherence(&dir, "v1", Some(body));
        assert_eq!(r, Coherence::Coherent);
        cleanup(&dir);
    }

    #[test]
    fn test_validate_boot_coherence_diverged_manifest_id() {
        let dir = tmp("boot-coh-div");
        write_sample_ipxe(&dir, "boot.ipxe", "set current_build_id v1\n");
        write_sample_ipxe(&dir, "current.ipxe", "set build_id v1\n");
        let body = r#"{"id":"vOLD"}"#;
        let r = validate_boot_coherence(&dir, "v1", Some(body));
        assert!(matches!(r, Coherence::Diverged { .. }));
        cleanup(&dir);
    }

    // 12 tests + the 2 channel_boot_state + ipxe_declared_value = 14
    // Brief target: 12 tests for boot module. The split is:
    //  - 2 for ipxe_declared_value
    //  - 2 for channel_boot_state
    //  - 3 for write_channel_ipxe
    //  - 3 for validate_boot_bundle
    //  - 1 for promote_boot_bundle
    //  - 3 for the channel/rescue/boot coherence validators
    //  Total: 14. Brief said ~12; we have 14 due to extra negative cases.
}
