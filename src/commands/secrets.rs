use crate::cli::SecretsCmd;
use crate::config::Config;
use crate::error::{GarError, Result};
use crate::output;
use age::x25519::{Identity, Recipient};
use serde::Serialize;
use std::io::{Read, Write};
use std::str::FromStr;

#[derive(Debug, Serialize)]
pub struct EncryptResult {
    pub encrypted_hex: String,
}

#[derive(Debug, Serialize)]
pub struct DecryptResult {
    pub decrypted: String,
}

pub async fn dispatch(cmd: SecretsCmd) -> Result<()> {
    match cmd {
        SecretsCmd::Encrypt { value, pubkey } => encrypt(&value, &pubkey),
        SecretsCmd::Decrypt { value, identity } => decrypt(&value, &identity),
    }
}

fn encrypt(value: &str, pubkey: &str) -> Result<()> {
    let recipient = Recipient::from_str(pubkey)
        .map_err(|e| GarError::config(format!("chave publica Age invalida: {}", e)))?;

    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
            .map_err(|e| GarError::config(format!("falha ao criar encryptor: {}", e)))?;

    let mut encrypted = vec![];
    let mut writer = encryptor
        .wrap_output(&mut encrypted)
        .map_err(|e| GarError::config(format!("falha ao preparar saida cifrada: {}", e)))?;

    writer
        .write_all(value.as_bytes())
        .map_err(|e| GarError::config(format!("falha ao escrever segredo: {}", e)))?;

    writer
        .finish()
        .map_err(|e| GarError::config(format!("falha ao finalizar criptografia: {}", e)))?;

    let hex = hex::encode(&encrypted);
    output_encrypt_result(&hex)?;
    Ok(())
}

fn decrypt(value: &str, identity_path: &std::path::Path) -> Result<()> {
    let identity_str = std::fs::read_to_string(identity_path)
        .map_err(|e| GarError::config(format!("falha ao ler arquivo de identidade: {}", e)))?;

    let identity = Identity::from_str(identity_str.trim())
        .map_err(|e| GarError::config(format!("identidade Age invalida: {}", e)))?;

    let encrypted =
        hex::decode(value).map_err(|e| GarError::config(format!("hex invalido: {}", e)))?;

    let decryptor = age::Decryptor::new(&encrypted[..])
        .map_err(|e| GarError::config(format!("falha ao criar decryptor: {}", e)))?;

    let mut reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|e| GarError::config(format!("falha ao decriptografar: {}", e)))?;

    let mut decrypted = String::new();
    reader
        .read_to_string(&mut decrypted)
        .map_err(|e| GarError::config(format!("falha ao ler texto decriptografado: {}", e)))?;

    output_decrypt_result(&decrypted)?;
    Ok(())
}

fn output_encrypt_result(hex: &str) -> Result<()> {
    let cfg = Config::from_env().unwrap_or_default();
    if cfg.json_output {
        output::json(&EncryptResult {
            encrypted_hex: hex.into(),
        })?;
    } else {
        output::ok("Valor criptografado:");
        println!("  {}", hex);
    }
    Ok(())
}

fn output_decrypt_result(decrypted: &str) -> Result<()> {
    let cfg = Config::from_env().unwrap_or_default();
    if cfg.json_output {
        output::json(&DecryptResult {
            decrypted: decrypted.into(),
        })?;
    } else {
        output::ok("Valor decriptografado:");
        println!("  {}", decrypted);
    }
    Ok(())
}
