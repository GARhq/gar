//! User system operations (useradd, usermod, userdel).

use crate::error::{GarError, Result};

/// Create a system user (no password, no shell) - bootstrap user.
pub async fn useradd_system(username: &str, home: &str) -> Result<()> {
    let _ = crate::services::shell::run_success(
        "useradd",
        &[
            "-M",
            "-d",
            home,
            "-s",
            "/run/current-system/sw/bin/bash",
            "-U",
            username,
        ],
    )
    .await?;
    Ok(())
}

/// Add user to a supplementary group.
pub async fn useradd_to_group(username: &str, group: &str) -> Result<()> {
    let _ = crate::services::shell::run_success(
        "usermod",
        &["-a", "-G", group, username],
    )
    .await?;
    Ok(())
}

/// Remove a user from a group.
pub async fn userdel_from_group(username: &str, group: &str) -> Result<()> {
    let _ = crate::services::shell::run_success("gpasswd", &["-d", username, group]).await;
    Ok(())
}

/// Delete a user (best-effort).
pub async fn userdel(username: &str) -> Result<()> {
    match crate::services::shell::run_success("userdel", &[username]).await {
        Ok(_) => Ok(()),
        Err(GarError::CommandFailed { code: 6, .. }) => Ok(()), // user does not exist
        Err(e) => Err(e),
    }
}

/// Hash a plaintext password using SHA-512 crypt.
pub fn hash_password(plain: &str) -> Result<String> {
    let output = std::process::Command::new("openssl")
        .args(["passwd", "-6", plain])
        .output()?;
    if !output.status.success() {
        return Err(GarError::User(format!(
            "openssl passwd failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}