//! Zero-trust vault for encrypting profile data at rest.
//!
//! All profile data is encrypted with AES-256-GCM using a key derived from
//! the user's passphrase via PBKDF2 + HKDF.
//!
//! Key derivation: passphrase → PBKDF2-HMAC-SHA256 (600K iter, 16-byte salt)
//!                 → master_key → HKDF-SHA256 (info="mylittlechart-vault-v1")
//!                 → 32-byte vault AES key
//!
//! Blob format: [12-byte nonce][ciphertext][16-byte GCM tag]
//!
//! This module delegates all cryptographic primitives to [`crate::crypto`],
//! which provides domain-separated keys for vault (disk) vs. sync (cloud).

use crate::crypto;

/// The derived AES-256 encryption key held in memory for the session.
pub type VaultKey = crypto::VaultKey;

/// Errors from vault operations.
#[derive(Debug)]
pub enum VaultError {
    /// Decryption failed — wrong passphrase or corrupted data.
    DecryptionFailed,
    /// Blob is too short to contain nonce + tag.
    InvalidBlob,
    /// IO error reading/writing vault files.
    Io(std::io::Error),
    /// Serialization error.
    Serde(String),
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VaultError::DecryptionFailed => write!(f, "Decryption failed — wrong passphrase or corrupted data"),
            VaultError::InvalidBlob => write!(f, "Invalid encrypted blob"),
            VaultError::Io(e) => write!(f, "IO error: {}", e),
            VaultError::Serde(e) => write!(f, "Serialization error: {}", e),
        }
    }
}

impl std::error::Error for VaultError {}

impl From<std::io::Error> for VaultError {
    fn from(e: std::io::Error) -> Self {
        VaultError::Io(e)
    }
}

/// Generate a random 16-byte salt for PBKDF2.
pub fn generate_salt() -> [u8; 16] {
    crypto::generate_salt()
}

/// Derive a 32-byte AES-256 vault key from a passphrase and salt.
///
/// Uses PBKDF2-HMAC-SHA256 (600K iterations) to produce a master key,
/// then HKDF-SHA256 with info=`"mylittlechart-vault-v1"` for domain separation
/// from the sync key.
pub fn derive_key(passphrase: &str, salt: &[u8; 16]) -> VaultKey {
    let mut master = crypto::derive_master_key(passphrase, salt);
    let vault_key = crypto::derive_vault_key(&master, salt);
    // Zero the intermediate master key after deriving the vault key
    master.fill(0);
    vault_key
}

/// Encrypt plaintext bytes with AES-256-GCM.
///
/// Returns: `[12-byte nonce][ciphertext + 16-byte tag]`
pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Vec<u8> {
    crypto::encrypt(key, plaintext)
        .expect("AES-GCM encryption should never fail with a valid 32-byte key")
}

/// Decrypt a blob produced by `encrypt()`.
///
/// Returns the original plaintext, or `VaultError::DecryptionFailed` if the
/// passphrase is wrong or the data is corrupted.
pub fn decrypt(key: &[u8; 32], blob: &[u8]) -> Result<Vec<u8>, VaultError> {
    crypto::decrypt(key, blob).map_err(|e| {
        if e.contains("too short") {
            VaultError::InvalidBlob
        } else {
            VaultError::DecryptionFailed
        }
    })
}

/// Encrypt a serializable value to bytes.
pub fn encrypt_json<T: serde::Serialize>(key: &[u8; 32], value: &T) -> Result<Vec<u8>, VaultError> {
    let json = serde_json::to_vec(value)
        .map_err(|e| VaultError::Serde(e.to_string()))?;
    Ok(encrypt(key, &json))
}

/// Decrypt bytes back to a deserializable value.
pub fn decrypt_json<T: serde::de::DeserializeOwned>(key: &[u8; 32], blob: &[u8]) -> Result<T, VaultError> {
    let plaintext = decrypt(key, blob)?;
    serde_json::from_slice(&plaintext)
        .map_err(|e| VaultError::Serde(e.to_string()))
}

/// Read an encrypted file from disk, decrypt it, and deserialize.
pub fn load_encrypted<T: serde::de::DeserializeOwned>(
    key: &[u8; 32],
    path: &std::path::Path,
) -> Result<T, VaultError> {
    let blob = std::fs::read(path)?;
    decrypt_json(key, &blob)
}

/// Serialize a value, encrypt it, and write to disk atomically (tmp + rename).
pub fn save_encrypted<T: serde::Serialize>(
    key: &[u8; 32],
    path: &std::path::Path,
    value: &T,
) -> Result<(), VaultError> {
    let blob = encrypt_json(key, value)?;

    // Atomic write: write to .tmp then rename
    let tmp_path = path.with_extension("enc.tmp");
    std::fs::write(&tmp_path, &blob)?;
    std::fs::rename(&tmp_path, path)?;

    Ok(())
}

/// Validate a passphrase against a vault at explicit paths (for pre-switch validation).
///
/// Returns the derived `VaultKey` on success, or an error string on failure.
pub fn validate_passphrase_at(
    salt_path: &std::path::Path,
    vault_path: &std::path::Path,
    passphrase: &str,
) -> Result<VaultKey, String> {
    let salt = load_or_create_salt(salt_path).map_err(|e| format!("salt: {}", e))?;
    let key = derive_key(passphrase, &salt);
    match load_encrypted::<crate::user_profile::VaultSecrets>(&key, vault_path) {
        Ok(_) => Ok(key),
        Err(_) => Err("Decryption failed — wrong passphrase or corrupted data".to_string()),
    }
}

/// Load salt from a hex file, or generate and save a new one.
pub fn load_or_create_salt(salt_path: &std::path::Path) -> Result<[u8; 16], VaultError> {
    crypto::load_or_create_salt(salt_path)
        .map_err(|e| VaultError::Io(std::io::Error::other(e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_encrypt_decrypt() {
        let key = derive_key("test passphrase", &[1u8; 16]);
        let plaintext = b"hello, zero-trust world!";
        let blob = encrypt(&key, plaintext);
        let decrypted = decrypt(&key, &blob).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_passphrase_fails() {
        let key1 = derive_key("correct", &[1u8; 16]);
        let key2 = derive_key("wrong", &[1u8; 16]);
        let blob = encrypt(&key1, b"secret data");
        assert!(decrypt(&key2, &blob).is_err());
    }

    #[test]
    fn tampered_blob_fails() {
        let key = derive_key("test", &[1u8; 16]);
        let mut blob = encrypt(&key, b"secret data");
        // Flip a byte in the ciphertext
        if let Some(byte) = blob.last_mut() {
            *byte ^= 0xFF;
        }
        assert!(decrypt(&key, &blob).is_err());
    }

    #[test]
    fn json_roundtrip() {
        let key = derive_key("test", &[1u8; 16]);
        let data = vec!["hello".to_string(), "world".to_string()];
        let blob = encrypt_json(&key, &data).unwrap();
        let decoded: Vec<String> = decrypt_json(&key, &blob).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn different_salt_different_key() {
        let key1 = derive_key("same passphrase", &[1u8; 16]);
        let key2 = derive_key("same passphrase", &[2u8; 16]);
        assert_ne!(key1, key2);
    }

    #[test]
    fn salt_load_or_create() {
        let dir = std::env::temp_dir().join("vault_test_salt");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let salt_path = dir.join("salt.hex");

        // First call creates
        let salt1 = load_or_create_salt(&salt_path).unwrap();
        // Second call loads same
        let salt2 = load_or_create_salt(&salt_path).unwrap();
        assert_eq!(salt1, salt2);

        // Verify file content
        let hex_str = std::fs::read_to_string(&salt_path).unwrap();
        assert_eq!(hex_str.len(), 32);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn vault_key_differs_from_sync_key() {
        // Cross-check: vault::derive_key must produce a different result than
        // the sync key that e2e_crypto would derive from the same inputs.
        let salt = [1u8; 16];
        let vault_key = derive_key("passphrase", &salt);
        let master = crate::crypto::derive_master_key("passphrase", &salt);
        let sync_key = crate::crypto::derive_sync_key(&master, &salt);
        assert_ne!(vault_key, sync_key, "vault_key must differ from sync_key");
    }
}
