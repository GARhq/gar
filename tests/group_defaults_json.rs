use assert_cmd::Command;
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;

fn write_executable(path: &std::path::Path, contents: &str) {
    fs::write(path, contents).expect("write test command");
    let mut permissions = fs::metadata(path)
        .expect("read test command metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make test command executable");
}

#[test]
fn ensure_defaults_json_emits_one_document() {
    let temp = tempdir().expect("create temporary test directory");
    let bin_dir = temp.path().join("bin");
    let state_dir = temp.path().join("groups");
    let storage_dir = temp.path().join("storage");
    let runtime_dir = temp.path().join("runtime");
    fs::create_dir_all(&bin_dir).expect("create test bin directory");
    fs::create_dir_all(&state_dir).expect("create fake group state directory");

    write_executable(
        &bin_dir.join("getent"),
        "#!/bin/sh\nif [ \"$1\" = group ] && [ -f \"$GAR_GROUP_TEST_STATE/$2\" ]; then\n  printf '%s:x:2000:\\n' \"$2\"\n  exit 0\nfi\nexit 2\n",
    );
    write_executable(
        &bin_dir.join("groupadd"),
        "#!/bin/sh\nfor name do last=$name; done\n: > \"$GAR_GROUP_TEST_STATE/$last\"\n",
    );
    write_executable(&bin_dir.join("chown"), "#!/bin/sh\nexit 0\n");

    let system_path = std::env::var_os("PATH").unwrap_or_default();
    let test_path = std::env::join_paths(
        std::iter::once(bin_dir.clone()).chain(std::env::split_paths(&system_path)),
    )
    .expect("compose test command path");
    let output = Command::cargo_bin("gar")
        .expect("locate gar binary")
        .args(["group", "ensure-defaults"])
        .env("GAR_JSON_OUTPUT", "true")
        .env("GAR_STORAGE_BASE", &storage_dir)
        .env("GAR_RUNTIME_ROOT", &runtime_dir)
        .env("GAR_GROUP_TEST_STATE", &state_dir)
        .env("PATH", test_path)
        .output()
        .expect("run ensure-defaults with isolated fake system commands");

    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout)
        .expect("stdout should contain exactly one valid JSON document");
    assert_eq!(result["created"], json!(["admin", "users", "lab"]));
    assert_eq!(result["already_existed"], json!([]));
}
