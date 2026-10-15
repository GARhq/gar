//! Integration tests for `gar::services::discovery`.
//!
//! Demonstrates how downstream crates (and our own `tests/`) can
//! consume the `gar` library crate. The same fixtures used in the
//! unit tests inside `services/discovery.rs` are recreated here, so
//! the external view stays honest even if the internal layout
//! changes.

use gar::services::discovery;

fn fresh_tmp(label: &str) -> std::path::PathBuf {
    let tmp =
        std::env::temp_dir().join(format!("gar-int-discovery-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    tmp
}

#[test]
fn test_iter_version_dirs_only_returns_v_dirs() {
    let tmp = fresh_tmp("only-v");
    for name in ["v20260101-100000", "v20260102-100000", "not-a-version"] {
        std::fs::create_dir_all(tmp.join(name)).unwrap();
    }
    // Add a non-directory entry that would otherwise look like a v*.
    std::fs::write(tmp.join("v20260103-fake"), b"not a dir").unwrap();

    let mut found: Vec<String> = discovery::iter_version_dirs(&tmp)
        .unwrap()
        .into_iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    found.sort();

    assert_eq!(found, vec!["v20260101-100000", "v20260102-100000"]);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_iter_version_dirs_empty_root() {
    let tmp = fresh_tmp("empty");
    let found = discovery::iter_version_dirs(&tmp).unwrap();
    assert!(found.is_empty());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_iter_version_dirs_with_mtime_orders_by_mtime_desc() {
    let tmp = fresh_tmp("mtime");
    // Two v* dirs with explicit, well-separated mtimes. We avoid
    // `create_dir_all` + sleep because some filesystems (BTRFS,
    // ext4 with noatime) collapse sub-second deltas.
    let a = tmp.join("v_old");
    let b = tmp.join("v_new");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();

    // `touch -d @<epoch>` is the most portable way to set a
    // deterministic mtime without pulling in the `filetime` crate.
    let _ = std::process::Command::new("touch")
        .arg("-d")
        .arg("@1700000000")
        .arg(&a)
        .status();
    let _ = std::process::Command::new("touch")
        .arg("-d")
        .arg("@1800000000")
        .arg(&b)
        .status();

    // The helper returns mtimes unordered; callers (e.g. `gc.rs`)
    // sort with `Reverse` to get newest-first. We exercise that
    // exact contract here.
    let mut pairs = discovery::iter_version_dirs_with_mtime(&tmp).unwrap();
    pairs.sort_by_key(|(mtime, _)| std::cmp::Reverse(*mtime));

    let names: Vec<String> = pairs
        .into_iter()
        .map(|(_, p)| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["v_new", "v_old"]);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_has_manifest_true_and_false() {
    let tmp = fresh_tmp("has-manifest");
    let with = tmp.join("v_with");
    let without = tmp.join("v_without");
    std::fs::create_dir_all(&with).unwrap();
    std::fs::create_dir_all(&without).unwrap();
    std::fs::write(with.join("manifest.json"), "{}").unwrap();

    assert!(discovery::has_manifest(&with));
    assert!(!discovery::has_manifest(&without));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_read_pointer_resolves_symlink() {
    let tmp = fresh_tmp("read-pointer");
    let target = tmp.join("v20260201-100000");
    std::fs::create_dir_all(&target).unwrap();
    std::os::unix::fs::symlink("v20260201-100000", tmp.join("current")).unwrap();

    assert_eq!(
        discovery::read_pointer(&tmp, "current").as_deref(),
        Some("v20260201-100000")
    );
    assert!(discovery::read_pointer(&tmp, "missing").is_none());
    let _ = std::fs::remove_dir_all(&tmp);
}
