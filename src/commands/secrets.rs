use crate::cli::SecretsCmd;
use crate::error::{GarError, Result};
use std::io::{Read, Write};
use std::str::FromStr;
use age::x25519::{Identity, Recipient};

pub async fn dispatch(cmd: SecretsCmd) -> Result<()> {
    match cmd {
        SecretsCmd::Encrypt { value, pubkey } => encrypt(&value, &pubkey),
        SecretsCmd::Decrypt { value, identity } => decrypt(&value, &identity),
    }
}

fn encrypt(value: &str, pubkey: &str) -> Result<()> {
    let recipient = Recipient::from_str(pubkey).map_err(|e| {
        GarError::config(format!("Failed to parse Age public key: {}", e))
    })?;

    let encryptor = age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
        .expect("Failed to create encryptor");

    let mut encrypted = vec![];
    let mut writer = encryptor.wrap_output(&mut encrypted).map_err(|e| {
        GarError::config(format!("Failed to wrap output: {}", e))
    })?;

    writer.write_all(value.as_bytes()).map_err(|e| {
        GarError::config(format!("Failed to write secret: {}", e))
    })?;

    writer.finish().map_err(|e| {
        GarError::config(format!("Failed to finish encryption: {}", e))
    })?;

    // Encode to base64 or hex? Age CLI uses ASCII armor.
    // For simplicity, let's output hex or base64. Let's use base64 (standard rust ecosystem usually has base64 crate, wait, let's use hex since it's built-in or just standard print).
    // Actually, age has an armor module!
    // But we don't have age's armor feature enabled explicitly. The user did `cargo add age`.
    // Let's just output base64 encoded. Wait, `gar-cli` might not have `base64`.
    // Let's print it as hex using standard library.
    let hex: String = encrypted.iter().map(|b| format!("{:02x}", b)).collect();
    println!("{}", hex);
    Ok(())
}

fn decrypt(value: &str, identity_path: &std::path::Path) -> Result<()> {
    let identity_str = std::fs::read_to_string(identity_path).map_err(|e| {
        GarError::config(format!("Failed to read identity file: {}", e))
    })?;

    let identity = Identity::from_str(identity_str.trim()).map_err(|e| {
        GarError::config(format!("Failed to parse Age identity: {}", e))
    })?;

    // Parse the hex string back to bytes
    let encrypted = (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16))
        .collect::<std::result::Result<Vec<u8>, _>>()
        .map_err(|e| GarError::config(format!("Failed to parse hex: {}", e)))?;

    let decryptor = age::Decryptor::new(&encrypted[..]).map_err(|e| {
        GarError::config(format!("Failed to create decryptor: {}", e))
    })?;

    let mut reader = decryptor.decrypt(std::iter::once(&identity as &dyn age::Identity)).map_err(|e| {
        GarError::config(format!("Failed to decrypt: {}", e))
    })?;

    let mut decrypted = String::new();
    reader.read_to_string(&mut decrypted).map_err(|e| {
        GarError::config(format!("Failed to read decrypted string: {}", e))
    })?;

    println!("{}", decrypted);
    Ok(())
}
