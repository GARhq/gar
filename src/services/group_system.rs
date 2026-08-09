//! Group system operations (groupadd, gpasswd).

use crate::error::Result;

/// Create a system group (with optional GID).
pub async fn groupadd_system(name: &str, gid: Option<u32>) -> Result<()> {
    let mut args: Vec<String> = vec!["-r".to_string()];
    if let Some(g) = gid {
        args.push("-g".to_string());
        args.push(g.to_string());
    }
    args.push(name.to_string());
    let args_ref: Vec<&str> = args.iter().map(String::as_str).collect();
    let _ = crate::services::shell::run_success("groupadd", &args_ref).await?;
    Ok(())
}

/// Check if a group exists.
pub fn group_exists(name: &str) -> bool {
    std::process::Command::new("getent")
        .args(["group", name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}