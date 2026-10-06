use std::path::Path;
use std::process::Command;

/// Generate or reuse Ed25519 keys for signing client images.
///
/// Keys default to `/var/lib/garos/keys/` (production). Tests can override
/// via the `GAR_KEYS_DIR` environment variable. The matching public key
/// must be committed to `GAROS/client/storage/garos.pub` for the Nix
/// build to remain pure.
pub fn ensure_keys(keys_dir: &Path) -> Result<(), crate::error::GarError> {
    let keys_dir = if let Ok(env_dir) = std::env::var("GAR_KEYS_DIR") {
        if !env_dir.is_empty() {
            std::path::PathBuf::from(env_dir)
        } else {
            keys_dir.to_path_buf()
        }
    } else {
        keys_dir.to_path_buf()
    };

    let priv_path = keys_dir.join("garos.key");
    let pub_path = keys_dir.join("garos.pub");

    if !priv_path.exists() {
        std::fs::create_dir_all(&keys_dir)?;

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

    let output = Command::new("openssl")
        .args(["pkeyutl", "-sign", "-inkey"])
        .arg(&priv_path)
        .args(["-rawin", "-in"])
        .arg(manifest_path)
        .args(["-out"])
        .arg(&sig_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(crate::error::GarError::build(format!(
            "Failed to sign manifest: {stderr}"
        )));
    }

    Ok(())
}

/// Verify an Ed25519 signature against a manifest, using the public
/// key committed to `GAROS/client/storage/garos.pub`.
///
/// This is the production-side counterpart of `sign_manifest` and the
/// same operation that runs in the client initrd (`erofs-builder.nix`).
/// Use it to validate a manifest pulled from the server before trusting
/// its contents (e.g. before applying an image update).
pub fn verify_manifest(
    keys_dir: &Path,
    manifest_path: &Path,
) -> Result<(), crate::error::GarError> {
    let pub_path = keys_dir.join("garos.pub");
    let sig_path = manifest_path.with_extension("sig");

    if !pub_path.exists() {
        return Err(crate::error::GarError::build(format!(
            "chave pública ausente em {}",
            pub_path.display()
        )));
    }
    if !sig_path.exists() {
        return Err(crate::error::GarError::build(format!(
            "assinatura ausente em {}",
            sig_path.display()
        )));
    }

    let output = Command::new("openssl")
        .args([
            "pkeyutl",
            "-verify",
            "-pubin",
            "-inkey",
        ])
        .arg(&pub_path)
        .args(["-rawin", "-in"])
        .arg(manifest_path)
        .args(["-sigfile"])
        .arg(&sig_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(crate::error::GarError::build(format!(
            "assinatura INVÁLIDA (manifest possivelmente MITM): {stderr}{stdout}"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_then_verify_roundtrip() {
        let tmp = std::env::temp_dir().join(format!(
            "gar-crypto-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();

        // Skip if no openssl in PATH (CI environment).
        if std::process::Command::new("openssl")
            .arg("version")
            .output()
            .is_err()
        {
            let _ = std::fs::remove_dir_all(&tmp);
            return;
        }

        // Use nix shell wrapper if running in pure-nix shell.
        let openssl_cmd = std::env::var("PATH").unwrap_or_default();
        let _ = openssl_cmd;

        // Generate keys
        let key_dir = tmp.join("keys");
        std::fs::create_dir_all(&key_dir).unwrap();
        let status = std::process::Command::new("openssl")
            .args(["genpkey", "-algorithm", "ed25519", "-out"])
            .arg(key_dir.join("garos.key"))
            .status()
            .expect("openssl available");
        assert!(status.success(), "keygen failed");
        let status = std::process::Command::new("openssl")
            .args(["pkey", "-in"])
            .arg(key_dir.join("garos.key"))
            .args(["-pubout", "-out"])
            .arg(key_dir.join("garos.pub"))
            .status()
            .expect("openssl available");
        assert!(status.success(), "pubout failed");

        // Sign a manifest
        let manifest = tmp.join("manifest.json");
        std::fs::write(&manifest, br#"{"id":"v1","checksum":"abc"}"#).unwrap();
        ensure_keys(&key_dir).unwrap();
        sign_manifest(&key_dir, &manifest).unwrap();
        assert!(manifest.with_extension("sig").exists());

        // Verify (positive case)
        verify_manifest(&key_dir, &manifest).expect("verify must succeed");

        // Tamper and reject
        std::fs::write(&manifest, br#"{"id":"v2","checksum":"xyz"}"#).unwrap();
        assert!(verify_manifest(&key_dir, &manifest).is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
