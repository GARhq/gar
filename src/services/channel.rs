//! Channel resolution (generic / lab / rescue) and target metadata.
//!
//! Replaces `ragc/lib/publish.sh` helpers (target_channel,
//! canonical_channel, channel_default_target, channel_*_pointer,
//! active_channel_from_current, resolve_client_target, target_installable).

use std::path::Path;

use crate::cli::{Channel, ImageTarget};
use crate::error::{GarError, Result};

/// Map a target to its canonical channel.
pub fn target_channel(target: ImageTarget) -> Channel {
    match target {
        ImageTarget::DesktopGeneric => Channel::Generic,
        ImageTarget::DesktopLab | ImageTarget::HypervDebug => Channel::Lab,
        ImageTarget::RescueMinimal => Channel::Rescue,
    }
}

/// Map a target string ("desktop-generic", "desktop-lab", etc) to its
/// canonical hardware class. Returns "unknown" if the target is unrecognized.
pub fn target_hardware_class_str(target: &str) -> &'static str {
    match target {
        "desktop-generic" => "physical-generic",
        "desktop-lab" => "physical-lab",
        "hyperv-debug" => "hyperv",
        "rescue-minimal" => "rescue",
        _ => "unknown",
    }
}

/// Map a target to its hardware class.
pub fn target_hardware_class(target: ImageTarget) -> &'static str {
    match target {
        ImageTarget::DesktopGeneric => "physical-generic",
        ImageTarget::DesktopLab => "physical-lab",
        ImageTarget::HypervDebug => "hyperv",
        ImageTarget::RescueMinimal => "rescue",
    }
}

/// Default target for a channel.
pub fn channel_default_target(channel: Channel) -> ImageTarget {
    match channel {
        Channel::Generic => ImageTarget::DesktopGeneric,
        Channel::Lab => ImageTarget::DesktopLab,
        Channel::Rescue => ImageTarget::RescueMinimal,
    }
}

/// Pointer name for the current generation of a channel.
pub fn channel_current_pointer(channel: Channel) -> &'static str {
    match channel {
        Channel::Generic => "current-generic",
        Channel::Lab => "current-lab",
        Channel::Rescue => "current-rescue",
    }
}

/// Pointer name for the previous generation of a channel.
pub fn channel_previous_pointer(channel: Channel) -> &'static str {
    match channel {
        Channel::Generic => "previous-generic",
        Channel::Lab => "previous-lab",
        Channel::Rescue => "previous-rescue",
    }
}

// ===== New helpers migrated from publish.sh in Phase 5.2b =====

/// Normalize a channel string. Empty input defaults to [`Channel::Generic`].
///
/// Mirrors bash `canonical_channel` (publish.sh:43-54): valid values are
/// `"generic"`, `"lab"`, `"rescue"`, and `""` (defaults to generic).
/// Anything else returns `GarError::invalid_argument` (was bash `die`).
///
/// **Improvements over bash:**
/// - Returns a typed [`Channel`] enum instead of printing a string
/// - Uses `Result` for error handling (testable, recoverable)
/// - The empty-string default to `Generic` is documented in the signature,
///   not just by side effect in `printf '${requested_channel:-generic}'`
#[must_use = "canonical_channel returns a Channel; ignoring it is a bug"]
#[tracing::instrument(skip_all, fields(requested = %requested))]
pub fn canonical_channel(requested: &str) -> Result<Channel> {
    match requested {
        "" => Ok(Channel::Generic),
        "generic" => Ok(Channel::Generic),
        "lab" => Ok(Channel::Lab),
        "rescue" => Ok(Channel::Rescue),
        other => Err(GarError::invalid_argument(format!(
            "Canal invalido: {other}. Use: generic, lab ou rescue"
        ))),
    }
}

/// Pointer name for the staged generation of a channel.
///
/// Mirrors bash `channel_staged_pointer` (publish.sh:85-89):
/// `staged-<channel>` for all three channels.
#[must_use]
#[tracing::instrument(skip_all)]
pub fn channel_staged_pointer(channel: Channel) -> &'static str {
    match channel {
        Channel::Generic => "staged-generic",
        Channel::Lab => "staged-lab",
        Channel::Rescue => "staged-rescue",
    }
}

/// Read the channel name from the manifest of the current generation.
///
/// Mirrors bash `active_channel_from_current` (publish.sh:91-102) and the
/// helpers it composes (`pointer_exists`, `pointer_version`,
/// `manifest_read_field`):
///
/// - `pointer_exists` (publish.sh:209-212) uses `[[ -L ]]` — accepts ONLY
///   symlinks, not regular files or anything else. If the entry is not a
///   symlink, return `Ok(None)` (matches the bash `if ! pointer_exists ...`
///   early-return path).
/// - `pointer_version` (publish.sh:214-223) canonicalizes the symlink target
///   with `readlink -f`, validates that the target is a directory AND starts
///   with `$IMAGES_ROOT/v`, then returns its basename. If the symlink is
///   broken (target missing) or points outside `$IMAGES_ROOT`, bash calls
///   `die` — we propagate that as a [`GarError::Build`].
/// - `manifest_read_field "$manifest" channel` returns the `channel` field
///   from the manifest, or `""` if the field is absent. We use the typed
///   [`crate::services::manifest::read`] and tolerate missing `channel` by
///   returning `Ok(None)` (the bash semantic of `""`).
///
/// **Improvements over bash:**
/// - `Result<Option<String>>` instead of empty-string sentinel
/// - Canonicalizes the symlink target via `std::fs::canonicalize` (the
///   Rust equivalent of `readlink -f`)
/// - Delegated manifest parsing is typed, not grep'd
#[must_use]
#[tracing::instrument(skip_all)]
pub fn active_channel_from_current(images_root: &Path) -> Result<Option<String>> {
    let current_link = images_root.join("current");
    let link_meta = match std::fs::symlink_metadata(&current_link) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(GarError::Build(format!(
                "Nao foi possivel ler symlink current: {}",
                e
            )))
        }
    };
    // Bash `pointer_exists` uses `[[ -L ]]` — only symlinks count.
    if !link_meta.file_type().is_symlink() {
        return Ok(None);
    }
    // Canonicalize (matches bash `readlink -f`). If the symlink is broken,
    // canonicalize fails with NotFound — bash `pointer_version` then `die`s.
    let target = std::fs::canonicalize(&current_link).map_err(|e| {
        GarError::Build(format!(
            "Ponteiro quebrado: current -> {} ({})",
            current_link.display(),
            e
        ))
    })?;
    // Bash rejects targets that don't start with $IMAGES_ROOT/v*.
    // We require the canonicalized target to live inside images_root and
    // be a directory named like a build version (starts with 'v').
    let target_str = target.to_string_lossy();
    let prefix = format!("{}/", images_root.display());
    if !target_str.starts_with(&prefix) {
        return Err(GarError::Build(format!(
            "Ponteiro invalido: current aponta para fora de {} ({})",
            images_root.display(),
            target_str
        )));
    }
    let Some(ver) = target.file_name().and_then(|n| n.to_str()) else {
        return Err(GarError::Build(format!(
            "Ponteiro invalido: target sem basename legivel ({})",
            target_str
        )));
    };
    if !ver.starts_with('v') {
        return Err(GarError::Build(format!(
            "Ponteiro invalido: target nao parece versao de build ({})",
            target_str
        )));
    }
    let generation_dir = images_root.join(ver);
    let manifest_path = generation_dir.join("manifest.json");
    if !manifest_path.exists() {
        return Err(GarError::Build(format!(
            "Manifest ausente em {}",
            generation_dir.display()
        )));
    }
    // Read the typed manifest. If `channel` field is missing, serde returns
    // an error — translate that to Ok(None) to match bash's empty-string
    // fallback for `manifest_read_field`.
    match crate::services::manifest::read(&generation_dir) {
        Ok(m) => Ok(Some(m.channel)),
        Err(crate::error::GarError::Json(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Default client target if none specified.
///
/// Mirrors the `DEFAULT_CLIENT_TARGET` global in publish.sh (default: "desktop-generic").
pub const DEFAULT_CLIENT_TARGET: &str = "desktop-generic";

/// Normalize a target string with legacy alias detection.
///
/// Mirrors bash `resolve_client_target` (publish.sh:122-151).
/// Returns `(canonical_target, optional_warning)`:
/// - `canonical_target`: the normalized target name (e.g. `"desktop-generic"`)
/// - `optional_warning`: a deprecation warning if a legacy alias was used
///   (e.g. `"physical-generic"` or `"rescue"`); `None` if no warning.
///
/// **Improvements over bash:**
/// - Returns a `(String, Option<String>)` tuple so the caller decides
///   whether/where to log the warning (bash calls `log_warn` directly,
///   which couples validation and presentation)
/// - `Result` for error handling (was bash `die`)
/// - Validates `Some("")` as "use default" (matching bash
///   `${1:-}` behavior) by passing `None` for `requested`
#[must_use]
#[tracing::instrument(skip_all)]
pub fn resolve_client_target(
    requested: Option<&str>,
    default_target: &str,
) -> Result<(String, Option<String>)> {
    let raw = match requested {
        None => default_target,
        Some(s) if s.is_empty() => default_target,
        Some(s) => s,
    };
    match raw {
        "desktop-generic" | "generic" => Ok(("desktop-generic".into(), None)),
        "physical-generic" => Ok((
            "desktop-generic".into(),
            Some("Alias legado 'physical-generic' detectado; use 'desktop-generic'".into()),
        )),
        "desktop-lab" | "lab" => Ok(("desktop-lab".into(), None)),
        "hyperv-debug" => Ok(("hyperv-debug".into(), None)),
        "rescue-minimal" | "rescue" => {
            let warning = if raw == "rescue" {
                Some("Alias legado 'rescue' detectado; use 'rescue-minimal'".into())
            } else {
                None
            };
            Ok(("rescue-minimal".into(), warning))
        }
        other => Err(GarError::invalid_argument(format!(
            "Target invalido: {other}. \
             Use: desktop-generic, desktop-lab, hyperv-debug ou rescue-minimal"
        ))),
    }
}

/// Build the Nix installable reference for a target.
///
/// Mirrors bash `target_installable` (publish.sh:153-182). Produces a
/// `path:<flake>#nixosConfigurations.ragos-client-<host>.config.system.build.ragosPublishTree`
/// reference suitable for `nix build`.
///
/// **Notes:**
/// - References `nixosConfigurations.ragos-client-*` (not `gar-client-*`)
///   because renaming lives in a dedicated Phase 7+ — not this brief.
/// - If `flake_root` already contains a `:` (i.e. a Nix scheme like
///   `git+file://` or `path:`), it is left untouched; otherwise the
///   `path:` prefix is added. This matches the bash behavior of
///   `[[ "$flake_ref" != *:* ]]`.
/// - Resolves legacy aliases via `resolve_client_target` (so callers
///   can pass `"physical-generic"` or `"rescue"` and get the right tree).
#[must_use]
#[tracing::instrument(skip_all)]
pub fn target_installable(flake_root: &Path, requested_target: Option<&str>) -> Result<String> {
    let (resolved, warning) = resolve_client_target(requested_target, DEFAULT_CLIENT_TARGET)?;
    if let Some(w) = warning.as_deref() {
        tracing::warn!(target: "gar::channel", "{}", w);
    }

    // Operational checkouts in /etc/gar may carry .git/.gitmodules without
    // full submodule metadata. Prefixing with `path:` keeps Nix from
    // re-interpreting the checkout as `git+file` and trying to redo the
    // installer submodule.
    //
    // Bash also runs `realpath -m` to canonicalize the path WITHOUT
    // requiring it to exist (`-m` = missing files ok). We approximate with
    // `std::fs::canonicalize` when the path exists, falling back to the
    // original path otherwise.
    let flake_ref = if flake_root.to_string_lossy().contains(':') {
        flake_root.display().to_string()
    } else {
        let normalized =
            std::fs::canonicalize(flake_root).unwrap_or_else(|_| flake_root.to_path_buf());
        format!("path:{}", normalized.display())
    };

    let config_attr = match resolved.as_str() {
        "desktop-generic" => {
            "nixosConfigurations.ragos-client-desktop-generic.config.system.build.ragosPublishTree"
        }
        "desktop-lab" => {
            "nixosConfigurations.ragos-client-desktop-lab.config.system.build.ragosPublishTree"
        }
        "hyperv-debug" => {
            "nixosConfigurations.ragos-client-hyperv-debug.config.system.build.ragosPublishTree"
        }
        "rescue-minimal" => {
            "nixosConfigurations.ragos-client-rescue-minimal.config.system.build.ragosPublishTree"
        }
        other => {
            return Err(GarError::invalid_argument(format!(
                "Target nao suportado em target_installable: {other}"
            )))
        }
    };

    Ok(format!("{flake_ref}#{config_attr}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // ===== Pre-existing tests (target_channel, hardware_class, default) =====

    #[test]
    fn test_target_channel_mapping() {
        assert_eq!(
            target_channel(ImageTarget::DesktopGeneric),
            Channel::Generic
        );
        assert_eq!(target_channel(ImageTarget::DesktopLab), Channel::Lab);
        assert_eq!(target_channel(ImageTarget::HypervDebug), Channel::Lab);
        assert_eq!(target_channel(ImageTarget::RescueMinimal), Channel::Rescue);
    }

    #[test]
    fn test_hardware_class_mapping() {
        assert_eq!(
            target_hardware_class(ImageTarget::DesktopGeneric),
            "physical-generic"
        );
        assert_eq!(target_hardware_class(ImageTarget::RescueMinimal), "rescue");
    }

    #[test]
    fn test_channel_default_target_roundtrip() {
        for ch in [Channel::Generic, Channel::Lab, Channel::Rescue] {
            let t = channel_default_target(ch);
            assert_eq!(target_channel(t), ch);
        }
    }

    #[test]
    fn test_channel_pointer_names_unique() {
        let ptrs = [
            channel_current_pointer(Channel::Generic),
            channel_current_pointer(Channel::Lab),
            channel_current_pointer(Channel::Rescue),
        ];
        let unique: HashSet<_> = ptrs.iter().collect();
        assert_eq!(unique.len(), 3);
    }

    // ===== 5.2b tests =====

    // --- canonical_channel: 4 tests (generic, lab, rescue, invalid) ---

    #[test]
    fn test_canonical_channel_generic() {
        assert_eq!(canonical_channel("generic").unwrap(), Channel::Generic);
    }

    #[test]
    fn test_canonical_channel_lab() {
        assert_eq!(canonical_channel("lab").unwrap(), Channel::Lab);
    }

    #[test]
    fn test_canonical_channel_rescue() {
        assert_eq!(canonical_channel("rescue").unwrap(), Channel::Rescue);
    }

    #[test]
    fn test_canonical_channel_invalid() {
        assert!(matches!(
            canonical_channel("nope"),
            Err(GarError::InvalidArgument(_))
        ));
        assert!(canonical_channel("genericx").is_err());
    }

    #[test]
    fn test_canonical_channel_empty_defaults_to_generic() {
        // Mirrors bash `${requested_channel:-generic}` behavior.
        assert_eq!(canonical_channel("").unwrap(), Channel::Generic);
    }

    // --- channel_current_pointer_name: 3 tests ---

    #[test]
    fn test_channel_current_pointer_names() {
        assert_eq!(channel_current_pointer(Channel::Generic), "current-generic");
        assert_eq!(channel_current_pointer(Channel::Lab), "current-lab");
        // bash `channel_current_pointer` produces "current-rescue" (NOT just "rescue");
        // preserved for backwards compatibility with existing symlinks in /srv/gar/images/
        assert_eq!(channel_current_pointer(Channel::Rescue), "current-rescue");
    }

    // --- channel_previous_pointer_name: 3 tests ---

    #[test]
    fn test_channel_previous_pointer_names() {
        assert_eq!(
            channel_previous_pointer(Channel::Generic),
            "previous-generic"
        );
        assert_eq!(channel_previous_pointer(Channel::Lab), "previous-lab");
        assert_eq!(channel_previous_pointer(Channel::Rescue), "previous-rescue");
    }

    // --- channel_staged_pointer_name: 3 tests ---

    #[test]
    fn test_channel_staged_pointer_names() {
        assert_eq!(channel_staged_pointer(Channel::Generic), "staged-generic");
        assert_eq!(channel_staged_pointer(Channel::Lab), "staged-lab");
        assert_eq!(channel_staged_pointer(Channel::Rescue), "staged-rescue");
    }

    // --- active_channel_from_current: 4 tests ---

    fn temp_root(label: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "gar-active-channel-{label}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_manifest(gen_dir: &Path, channel: &str) {
        std::fs::create_dir_all(gen_dir).unwrap();
        std::fs::write(
            gen_dir.join("manifest.json"),
            format!(
                r#"{{"id":"v20240101","timestamp":"2024-01-01","system_path":"/nix/store/abc","init_path":"/nix/store/abc/init","artifacts":{{"kernel":"bzImage","initrd":"initrd"}},"checksums":{{"kernel":"deadbeef","initrd":"deadbeef"}},"status":"active","target":"desktop-generic","channel":"{channel}","hardwareClass":"physical-generic"}}"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn test_active_channel_from_current_no_symlink() {
        let root = temp_root("no-symlink");
        assert!(matches!(active_channel_from_current(&root), Ok(None)));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_active_channel_from_current_symlink_no_manifest() {
        let root = temp_root("symlink-no-manifest");
        // Create a symlink to a non-existent build version (broken symlink).
        // Bash `pointer_version` `die`s on broken symlinks, so we mirror that.
        std::os::unix::fs::symlink("v99999999", root.join("current")).unwrap();
        assert!(matches!(
            active_channel_from_current(&root),
            Err(GarError::Build(_))
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_active_channel_from_current_not_a_symlink() {
        // Bash `pointer_exists` uses [[ -L ]] — only symlinks count.
        // A regular file at `current` should return Ok(None), not an error.
        let root = temp_root("not-symlink");
        std::fs::write(root.join("current"), "not a symlink").unwrap();
        assert!(matches!(active_channel_from_current(&root), Ok(None)));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_active_channel_from_current_broken_symlink() {
        // Broken symlink: target doesn't exist. bash `pointer_version` `die`s
        // via `readlink -f`. We propagate as GarError::Build.
        let root = temp_root("broken");
        std::os::unix::fs::symlink("v_does_not_exist", root.join("current")).unwrap();
        assert!(matches!(
            active_channel_from_current(&root),
            Err(GarError::Build(_))
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_active_channel_from_current_target_outside_root() {
        // Symlink pointing outside images_root. bash `pointer_version` rejects
        // targets not starting with $IMAGES_ROOT/v*.
        let root = temp_root("outside");
        std::os::unix::fs::symlink("/tmp", root.join("current")).unwrap();
        let result = active_channel_from_current(&root);
        assert!(
            matches!(result, Err(GarError::Build(_))),
            "expected Err, got {:?}",
            result
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_active_channel_from_current_valid() {
        let root = temp_root("valid");
        let ver = "v20240101";
        write_manifest(&root.join(ver), "generic");
        std::os::unix::fs::symlink(ver, root.join("current")).unwrap();
        let channel = active_channel_from_current(&root).unwrap();
        assert_eq!(channel, Some("generic".into()));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_active_channel_from_current_rescue_channel() {
        let root = temp_root("rescue");
        let ver = "v20240201";
        write_manifest(&root.join(ver), "rescue");
        std::os::unix::fs::symlink(ver, root.join("current")).unwrap();
        let channel = active_channel_from_current(&root).unwrap();
        assert_eq!(channel, Some("rescue".into()));
        std::fs::remove_dir_all(&root).unwrap();
    }

    // --- resolve_client_target: 8 tests (5 aliases + 3 invalid paths) ---

    #[test]
    fn test_resolve_client_target_desktop_generic() {
        let (t, w) = resolve_client_target(Some("desktop-generic"), DEFAULT_CLIENT_TARGET).unwrap();
        assert_eq!(t, "desktop-generic");
        assert!(w.is_none());
    }

    #[test]
    fn test_resolve_client_target_short_alias_generic() {
        let (t, w) = resolve_client_target(Some("generic"), DEFAULT_CLIENT_TARGET).unwrap();
        assert_eq!(t, "desktop-generic");
        assert!(w.is_none());
    }

    #[test]
    fn test_resolve_client_target_legacy_physical_generic_warning() {
        let (t, w) =
            resolve_client_target(Some("physical-generic"), DEFAULT_CLIENT_TARGET).unwrap();
        assert_eq!(t, "desktop-generic");
        assert!(w.is_some());
        assert!(w.unwrap().contains("physical-generic"));
    }

    #[test]
    fn test_resolve_client_target_desktop_lab() {
        let (t, w) = resolve_client_target(Some("desktop-lab"), DEFAULT_CLIENT_TARGET).unwrap();
        assert_eq!(t, "desktop-lab");
        assert!(w.is_none());
    }

    #[test]
    fn test_resolve_client_target_short_alias_lab() {
        let (t, w) = resolve_client_target(Some("lab"), DEFAULT_CLIENT_TARGET).unwrap();
        assert_eq!(t, "desktop-lab");
        assert!(w.is_none());
    }

    #[test]
    fn test_resolve_client_target_hyperv_debug() {
        let (t, w) = resolve_client_target(Some("hyperv-debug"), DEFAULT_CLIENT_TARGET).unwrap();
        assert_eq!(t, "hyperv-debug");
        assert!(w.is_none());
    }

    #[test]
    fn test_resolve_client_target_legacy_rescue_warning() {
        let (t, w) = resolve_client_target(Some("rescue"), DEFAULT_CLIENT_TARGET).unwrap();
        assert_eq!(t, "rescue-minimal");
        assert!(w.is_some());
        assert!(w.unwrap().contains("rescue"));
    }

    #[test]
    fn test_resolve_client_target_rescue_minimal() {
        let (t, w) = resolve_client_target(Some("rescue-minimal"), DEFAULT_CLIENT_TARGET).unwrap();
        assert_eq!(t, "rescue-minimal");
        assert!(w.is_none());
    }

    #[test]
    fn test_resolve_client_target_none_uses_default() {
        let (t, w) = resolve_client_target(None, DEFAULT_CLIENT_TARGET).unwrap();
        assert_eq!(t, "desktop-generic");
        assert!(w.is_none());
    }

    #[test]
    fn test_resolve_client_target_empty_string_uses_default() {
        let (t, w) = resolve_client_target(Some(""), DEFAULT_CLIENT_TARGET).unwrap();
        assert_eq!(t, "desktop-generic");
        assert!(w.is_none());
    }

    #[test]
    fn test_resolve_client_target_invalid() {
        assert!(matches!(
            resolve_client_target(Some("not-a-target"), DEFAULT_CLIENT_TARGET),
            Err(GarError::InvalidArgument(_))
        ));
        assert!(matches!(
            resolve_client_target(Some(""), "bogus-default"),
            Err(GarError::InvalidArgument(_))
        ));
    }

    // --- target_installable: 4 tests (4 targets + 1 with scheme) ---

    #[test]
    fn test_target_installable_desktop_generic() {
        let flake = std::path::Path::new("/etc/gar");
        let s = target_installable(flake, Some("desktop-generic")).unwrap();
        assert_eq!(
            s,
            "path:/etc/gar#nixosConfigurations.ragos-client-desktop-generic.config.system.build.ragosPublishTree"
        );
    }

    #[test]
    fn test_target_installable_desktop_lab() {
        let flake = std::path::Path::new("/etc/gar");
        let s = target_installable(flake, Some("desktop-lab")).unwrap();
        assert!(s.contains("ragos-client-desktop-lab"));
        assert!(s.contains("path:/etc/gar#"));
    }

    #[test]
    fn test_target_installable_hyperv_debug() {
        let flake = std::path::Path::new("/etc/gar");
        let s = target_installable(flake, Some("hyperv-debug")).unwrap();
        assert!(s.contains("ragos-client-hyperv-debug"));
        assert!(s.ends_with(".config.system.build.ragosPublishTree"));
    }

    #[test]
    fn test_target_installable_rescue_minimal() {
        let flake = std::path::Path::new("/etc/gar");
        let s = target_installable(flake, Some("rescue-minimal")).unwrap();
        assert!(s.contains("ragos-client-rescue-minimal"));
    }

    #[test]
    fn test_target_installable_preserves_existing_scheme() {
        // If flake_root already has a scheme (e.g. `git+file://...`),
        // it should NOT get the `path:` prefix added.
        let flake = std::path::Path::new("git+file:///etc/gar");
        let s = target_installable(flake, Some("desktop-generic")).unwrap();
        assert!(s.starts_with("git+file:///etc/gar#"));
        assert!(!s.contains("path:git+"));
    }

    #[test]
    fn test_target_installable_alias_resolves() {
        // Passing a legacy alias should resolve to the canonical target.
        let flake = std::path::Path::new("/etc/gar");
        let s = target_installable(flake, Some("physical-generic")).unwrap();
        assert!(s.contains("ragos-client-desktop-generic"));
        assert!(!s.contains("physical-generic"));
    }

    #[test]
    fn test_target_installable_invalid_target_errors() {
        let flake = std::path::Path::new("/etc/gar");
        assert!(matches!(
            target_installable(flake, Some("not-a-target")),
            Err(GarError::InvalidArgument(_))
        ));
    }
}
