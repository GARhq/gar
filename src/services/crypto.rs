use std::path::Path;
use std::process::Command;

pub fn ensure_keys(keys_dir: &Path) -> Result<(), crate::error::GarError> {
    let priv_path = keys_dir.join("garos.key");
    let pub_path = keys_dir.join("garos.pub");

    if !priv_path.exists() {
        std::fs::create_dir_all(keys_dir)?;

        let status = Command::new("openssl")
            .args(["genpkey", "-algorithm", "ed25519", "-out"])
            .arg(&priv_path)
            .status()?;

        if !status.success() {
            return Err(crate::error::GarError::build(
                "Failed to generate private key",
            ));
        }

        let status = Command::new("openssl")
            .args(["pkey", "-in"])
            .arg(&priv_path)
            .args(["-pubout", "-out"])
            .arg(&pub_path)
            .status()?;

        if !status.success() {
            return Err(crate::error::GarError::build(
                "Failed to generate public key",
            ));
        }
    }

    // The public key must be manually copied to GAROS/client/storage/garos.pub and committed to git
    // for the Nix build to remain pure and reproducible across environments.
    if !priv_path.exists() {
        tracing::info!("Notice: New keys generated. Please copy {} to your repository at GAROS/client/storage/garos.pub", pub_path.display());
    }

    Ok(())
}

pub fn sign_manifest(keys_dir: &Path, manifest_path: &Path) -> Result<(), crate::error::GarError> {
    let priv_path = keys_dir.join("garos.key");
    let sig_path = manifest_path.with_extension("sig");

    let status = Command::new("openssl")
        .args(["pkeyutl", "-sign", "-inkey"])
        .arg(&priv_path)
        .args(["-rawin", "-in"])
        .arg(manifest_path)
        .args(["-out"])
        .arg(&sig_path)
        .status()?;

    if !status.success() {
        return Err(crate::error::GarError::build("Failed to sign manifest"));
    }

    Ok(())
}
