use std::io::Write;
use assert_cmd::Command;
use tempfile::NamedTempFile;

fn get_garos_dir() -> std::path::PathBuf {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().unwrap().join("garos")
}

fn create_temp_inventory(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", content).unwrap();
    file
}

#[test]
fn test_detect_duplicate_mac() {
    let content = r#"
[
  { mac = "aa:aa:aa:aa:aa:aa"; hostname = "tc-a"; ip = "192.168.100.100"; }
  { mac = "aa:aa:aa:aa:aa:aa"; hostname = "tc-b"; ip = "192.168.100.101"; }
]
"#;
    let file = create_temp_inventory(content);
    let mut cmd = Command::cargo_bin("gar").unwrap();
    cmd.env("GAR_FLAKE_PATH", get_garos_dir())
        .args(&["server", "check", "--inventory", file.path().to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("MAC duplicado"));
}

#[test]
fn test_detect_duplicate_hostname() {
    let content = r#"
[
  { mac = "aa:aa:aa:aa:aa:aa"; hostname = "tc-a"; ip = "192.168.100.100"; }
  { mac = "bb:bb:bb:bb:bb:bb"; hostname = "tc-a"; ip = "192.168.100.101"; }
]
"#;
    let file = create_temp_inventory(content);
    let mut cmd = Command::cargo_bin("gar").unwrap();
    cmd.env("GAR_FLAKE_PATH", get_garos_dir())
        .args(&["server", "check", "--inventory", file.path().to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("hostname duplicado"));
}

#[test]
fn test_detect_duplicate_ip() {
    let content = r#"
[
  { mac = "aa:aa:aa:aa:aa:aa"; hostname = "tc-a"; ip = "192.168.100.100"; }
  { mac = "bb:bb:bb:bb:bb:bb"; hostname = "tc-b"; ip = "192.168.100.100"; }
]
"#;
    let file = create_temp_inventory(content);
    let mut cmd = Command::cargo_bin("gar").unwrap();
    cmd.env("GAR_FLAKE_PATH", get_garos_dir())
        .args(&["server", "check", "--inventory", file.path().to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("IP duplicado"));
}

#[test]
fn test_empty_inventory_forbidden_by_default() {
    let content = "[]";
    let file = create_temp_inventory(content);
    let mut cmd = Command::cargo_bin("gar").unwrap();
    cmd.env("GAR_FLAKE_PATH", get_garos_dir())
        .args(&["server", "check", "--inventory", file.path().to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("inventario externo vazio"));
}

#[test]
fn test_empty_inventory_allowed_explicitly() {
    let content = "[]";
    let file = create_temp_inventory(content);
    let mut cmd = Command::cargo_bin("gar").unwrap();
    cmd.env("GAR_FLAKE_PATH", get_garos_dir())
        .args(&["server", "check", "--inventory", file.path().to_str().unwrap(), "--allow-empty"])
        .assert()
        .success();
}

#[test]
fn test_channel_release_track_mismatch() {
    let content = r#"
[
  {
    mac = "52:54:00:64:10:11";
    hostname = "tc-track";
    ip = "192.168.100.110";
    channel = "generic";
    releaseTrack = "pilot";
  }
]
"#;
    let file = create_temp_inventory(content);
    let mut cmd = Command::cargo_bin("gar").unwrap();
    cmd.env("GAR_FLAKE_PATH", get_garos_dir())
        .args(&["server", "check", "--inventory", file.path().to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("channel e releaseTrack incoerentes"));
}

#[test]
fn test_profile_client_profile_mismatch() {
    let content = r#"
[
  {
    mac = "52:54:00:64:10:12";
    hostname = "tc-profile";
    ip = "192.168.100.111";
    profile = "desktop-generic";
    clientProfile = "lab-workstation";
  }
]
"#;
    let file = create_temp_inventory(content);
    let mut cmd = Command::cargo_bin("gar").unwrap();
    cmd.env("GAR_FLAKE_PATH", get_garos_dir())
        .args(&["server", "check", "--inventory", file.path().to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("profile e clientProfile incoerentes"));
}

#[test]
fn test_semantic_combo_mismatch() {
    let content = r#"
[
  {
    mac = "52:54:00:64:10:13";
    hostname = "tc-combo";
    ip = "192.168.100.112";
    releaseTrack = "stable";
    clientProfile = "lab-workstation";
    hardwareClass = "physical-lab";
  }
]
"#;
    let file = create_temp_inventory(content);
    let mut cmd = Command::cargo_bin("gar").unwrap();
    cmd.env("GAR_FLAKE_PATH", get_garos_dir())
        .args(&["server", "check", "--inventory", file.path().to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("combinacao invalida"));
}

#[test]
fn test_uefi_https_reserved() {
    let content = r#"
[
  {
    mac = "52:54:00:64:10:14";
    hostname = "tc-https";
    ip = "192.168.100.113";
    bootMethod = "uefi-https";
  }
]
"#;
    let file = create_temp_inventory(content);
    let mut cmd = Command::cargo_bin("gar").unwrap();
    cmd.env("GAR_FLAKE_PATH", get_garos_dir())
        .args(&["server", "check", "--inventory", file.path().to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("bootMethod=uefi-https ainda e reservado/futuro"));
}
