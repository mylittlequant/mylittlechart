//! Zero-trust vault for encrypting profile data at rest.
//!
//! All profile data is encrypted with AES-256-GCM using a key derived from
//! the user's passphrase via PBKDF2 + HKDF.
//!
//! Key derivation: passphrase → PBKDF2-HMAC-SHA256 (600K iter, 16-byte salt)
//!                 → HKDF-SHA256 (info="mylittlechart-sync-v1") → 32-byte AES key
//!
//! Blob format: [12-byte nonce][ciphertext][16-byte GCM tag]

use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use aes_gcm::aead::Aead;
use hkdf::Hkdf;
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;

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

/// Number of PBKDF2 iterations (OWASP 2023 minimum for HMAC-SHA256).
const PBKDF2_ITERATIONS: u32 = 600_000;

/// HKDF info string for domain separation.
const HKDF_INFO: &[u8] = b"mylittlechart-sync-v1";

/// AES-GCM nonce size in bytes.
const NONCE_SIZE: usize = 12;

/// Generate a random 16-byte salt for PBKDF2.
pub fn generate_salt() -> [u8; 16] {
    use rand::Rng;
    rand::thread_rng().gen()
}

/// Derive a 32-byte AES-256 key from a passphrase and salt.
///
/// Uses PBKDF2-HMAC-SHA256 (600K iterations) followed by HKDF-SHA256
/// for domain separation.
pub fn derive_key(passphrase: &str, salt: &[u8; 16]) -> [u8; 32] {
    // Step 1: PBKDF2
    let mut intermediate = [0u8; 32];
    pbkdf2_hmac::<Sha256>(
        passphrase.as_bytes(),
        salt,
        PBKDF2_ITERATIONS,
        &mut intermediate,
    );

    // Step 2: HKDF for domain separation
    let hk = Hkdf::<Sha256>::new(Some(salt), &intermediate);
    let mut key = [0u8; 32];
    hk.expand(HKDF_INFO, &mut key)
        .expect("HKDF expand should never fail for 32-byte output");

    // Zero the intermediate key
    intermediate.fill(0);

    key
}

/// Encrypt plaintext bytes with AES-256-GCM.
///
/// Returns: `[12-byte nonce][ciphertext + 16-byte tag]`
pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Vec<u8> {
    use aes_gcm::aead::generic_array::GenericArray;
    use rand::Rng;

    let cipher = Aes256Gcm::new(GenericArray::from_slice(key));
    let nonce_bytes: [u8; NONCE_SIZE] = rand::thread_rng().gen();
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .expect("AES-GCM encryption should never fail");

    let mut blob = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);
    blob
}

/// Decrypt a blob produced by `encrypt()`.
///
/// Returns the original plaintext, or `VaultError::DecryptionFailed` if the
/// passphrase is wrong or the data is corrupted.
pub fn decrypt(key: &[u8; 32], blob: &[u8]) -> Result<Vec<u8>, VaultError> {
    use aes_gcm::aead::generic_array::GenericArray;

    if blob.len() < NONCE_SIZE + 16 {
        return Err(VaultError::InvalidBlob);
    }

    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_SIZE);
    let cipher = Aes256Gcm::new(GenericArray::from_slice(key));
    let nonce = Nonce::from_slice(nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| VaultError::DecryptionFailed)
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

/// Load salt from a hex file, or generate and save a new one.
pub fn load_or_create_salt(salt_path: &std::path::Path) -> Result<[u8; 16], VaultError> {
    if salt_path.exists() {
        let hex_str = std::fs::read_to_string(salt_path)?;
        let hex_str = hex_str.trim();
        if hex_str.len() == 32 {
            let mut salt = [0u8; 16];
            if let Ok(bytes) = hex::decode(hex_str) {
                if bytes.len() == 16 {
                    salt.copy_from_slice(&bytes);
                    return Ok(salt);
                }
            }
        }
        // Invalid salt file — regenerate
    }

    let salt = generate_salt();
    if let Some(parent) = salt_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(salt_path, hex::encode(salt))?;
    Ok(salt)
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
}
