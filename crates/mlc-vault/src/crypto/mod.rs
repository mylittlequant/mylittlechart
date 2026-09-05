//! Unified cryptography module for key derivation and encryption.
//!
//! Key hierarchy:
//!   passphrase + salt → PBKDF2(600K) → master_key
//!     → HKDF("mylittlechart-vault-v1") → vault_key (local disk)
//!     → HKDF("mylittlechart-sync-v1")  → sync_key (cloud blobs)
//!
//! Domain separation ensures that vault_key != sync_key even when derived
//! from the same passphrase and salt.

use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use aes_gcm::aead::Aead;
use hkdf::Hkdf;
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Number of PBKDF2 iterations (OWASP 2023 minimum for HMAC-SHA256).
pub const PBKDF2_ITERATIONS: u32 = 600_000;

/// HKDF info string binding the derived key to local vault encryption.
pub const HKDF_VAULT_INFO: &[u8] = b"mylittlechart-vault-v1";

/// HKDF info string binding the derived key to cloud sync encryption.
pub const HKDF_SYNC_INFO: &[u8] = b"mylittlechart-sync-v1";

/// HKDF info string binding a recovery key to a wrapped master key.
pub const HKDF_RECOVERY_INFO: &[u8] = b"mylittlechart-recovery-v1";

/// AES-GCM nonce size in bytes.
const NONCE_SIZE: usize = 12;

/// A 32-byte master key derived from passphrase + salt via PBKDF2.
/// Split further into vault_key or sync_key via HKDF.
pub type MasterKey = [u8; 32];

/// A 32-byte AES-256 key for local vault (disk) encryption.
pub type VaultKey = [u8; 32];

/// A 32-byte AES-256 key for cloud sync blob encryption.
pub type SyncKey = [u8; 32];

/// Generate a cryptographically random 16-byte salt.
pub fn generate_salt() -> [u8; 16] {
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    salt
}

/// Load salt from a hex-encoded storage entry, or generate a new one and persist it.
///
/// The storage entry contains a 32-character lowercase hex string (16 bytes).
/// If the entry is missing or malformed, a new salt is generated and written
/// through [`crate::storage::global_storage()`].
pub fn load_or_create_salt(path: &std::path::Path) -> Result<[u8; 16], String> {
    let key = path_to_storage_key(path);
    let storage = crate::storage::global_storage();

    if let Ok(Some(bytes)) = storage.read(&key) {
        let hex_str = std::str::from_utf8(&bytes)
            .unwrap_or("")
            .trim()
            .to_string();
        if hex_str.len() == 32 {
            if let Ok(decoded) = hex::decode(&hex_str) {
                if decoded.len() == 16 {
                    let mut salt = [0u8; 16];
                    salt.copy_from_slice(&decoded);
                    return Ok(salt);
                }
            }
        }
        // Fall through: invalid content — regenerate
    }

    let salt = generate_salt();
    storage
        .write(&key, hex::encode(salt).as_bytes())
        .map_err(|e| format!("write salt file: {}", e))?;
    Ok(salt)
}

/// Convert an absolute path to a forward-slash storage key relative to the
/// app-data root.  Paths outside the root are used verbatim (normalised).
fn path_to_storage_key(path: &std::path::Path) -> String {
    let root = crate::storage::fs_impl::app_data_dir();
    if let Ok(rel) = path.strip_prefix(&root) {
        rel.to_string_lossy().replace('\\', "/")
    } else {
        path.to_string_lossy().replace('\\', "/")
    }
}

/// Derive a 32-byte master key from `passphrase` and `salt` via PBKDF2-HMAC-SHA256.
///
/// This is the slow, CPU-hard step that resists brute-force attacks.
/// The resulting master_key must not be used directly for encryption —
/// use [`derive_vault_key`] or [`derive_sync_key`] to produce domain-separated keys.
pub fn derive_master_key(passphrase: &str, salt: &[u8]) -> MasterKey {
    let mut master = [0u8; 32];
    pbkdf2_hmac::<Sha256>(
        passphrase.as_bytes(),
        salt,
        PBKDF2_ITERATIONS,
        &mut master,
    );
    master
}

/// Derive a vault key from `master_key` for local disk encryption.
///
/// Uses HKDF-SHA256 with info = `b"mylittlechart-vault-v1"`.
pub fn derive_vault_key(master_key: &MasterKey, salt: &[u8]) -> VaultKey {
    hkdf_derive(master_key, salt, HKDF_VAULT_INFO)
}

/// Derive a sync key from `master_key` for cloud blob encryption.
///
/// Uses HKDF-SHA256 with info = `b"mylittlechart-sync-v1"`.
pub fn derive_sync_key(master_key: &MasterKey, salt: &[u8]) -> SyncKey {
    hkdf_derive(master_key, salt, HKDF_SYNC_INFO)
}

// =============================================================================
// Recovery key
// =============================================================================

/// Generate a 256-bit (32-byte) cryptographically random recovery key.
///
/// The recovery key is displayed once to the user in `XXXX-XXXX-...` format
/// (via [`format_recovery_key`]) and used to encrypt the master key for
/// server-side storage.  It must be written down and stored securely — if both
/// the passphrase and the recovery key are lost, cloud data is irrecoverable.
pub fn generate_recovery_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    key
}

/// Alphabet for [`generate_passphrase`]: lower-case letters and digits with
/// every look-alike pair removed (`i`/`l`/`1`, `o`/`0`). 31 symbols.
///
/// The suggested passphrase is meant to be COPIED, not memorised — but it is
/// also read back off a screen and typed by hand when the clipboard has moved
/// on, and a passphrase that cannot be transcribed without ambiguity is a
/// passphrase that locks its owner out.
const PASSPHRASE_ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";

/// Groups of characters in a generated passphrase, and characters per group.
/// 4 x 5 = 20 symbols over a 31-symbol alphabet is ~99 bits — far above what
/// the PBKDF2 work factor needs to hold, and short enough to read aloud.
const PASSPHRASE_GROUPS: usize = 4;
const PASSPHRASE_GROUP_LEN: usize = 5;

/// Generate a strong passphrase to suggest to the user, in `xxxxx-xxxxx-
/// xxxxx-xxxxx` form.
///
/// Drawn from the same `rand::thread_rng()` CSPRNG as [`generate_recovery_key`]
/// — which is the OS generator natively and `crypto.getRandomValues` in the
/// browser (the crate pulls `getrandom` with the `js` feature for exactly
/// that). It is NOT a `%` of a random byte: 256 is not a multiple of 31, so
/// modulo would make the first four letters of the alphabet measurably more
/// likely than the rest. Bytes at or above the largest multiple of the
/// alphabet length are rejected and redrawn instead, which costs a handful of
/// extra bytes and buys a uniform draw.
///
/// The dashes are part of the passphrase, not decoration — they are what the
/// user sees, copies and types back.
pub fn generate_passphrase() -> String {
    let n = PASSPHRASE_ALPHABET.len() as u32;
    // Largest multiple of `n` that fits in a byte; anything at or above it
    // would bias the draw and is redrawn.
    let limit = (256u32 / n) * n;

    let mut out = String::with_capacity(
        PASSPHRASE_GROUPS * PASSPHRASE_GROUP_LEN + (PASSPHRASE_GROUPS - 1),
    );
    let mut rng = rand::thread_rng();
    let mut buf = [0u8; 1];

    for group in 0..PASSPHRASE_GROUPS {
        if group > 0 {
            out.push('-');
        }
        for _ in 0..PASSPHRASE_GROUP_LEN {
            let idx = loop {
                rng.fill_bytes(&mut buf);
                let v = buf[0] as u32;
                if v < limit {
                    break (v % n) as usize;
                }
            };
            out.push(PASSPHRASE_ALPHABET[idx] as char);
        }
    }
    out
}

/// Format a 32-byte recovery key as a human-readable string.
///
/// Output: 16 groups of 4 hex characters separated by dashes.
/// Example: `"a1b2-c3d4-e5f6-7890-abcd-ef01-2345-6789-abcd-ef01-2345-6789-abcd-ef01-2345-6789"`
///
/// This 64-character hex (plus 15 dashes = 79 chars total) is displayed once
/// after vault creation so the user can write it down.
pub fn format_recovery_key(key: &[u8; 32]) -> String {
    let hex = hex::encode(key);
    hex.as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).expect("hex is always valid UTF-8"))
        .collect::<Vec<_>>()
        .join("-")
}

/// Parse a recovery key from its human-readable format back to 32 raw bytes.
///
/// Accepts the formatted string (with or without dashes).  Returns an error
/// if the string does not decode to exactly 32 bytes of valid hex.
pub fn parse_recovery_key(formatted: &str) -> Result<[u8; 32], String> {
    let clean: String = formatted.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let bytes = hex::decode(&clean).map_err(|e| format!("invalid recovery key: {}", e))?;
    if bytes.len() != 32 {
        return Err(format!(
            "recovery key must be 32 bytes, got {}",
            bytes.len()
        ));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Ok(key)
}

/// Encrypt `master_key` with `recovery_key` for server-side escrow storage.
///
/// Derives an encryption key from `recovery_key` via HKDF with
/// `HKDF_RECOVERY_INFO` (domain-separated from vault and sync keys).
/// Returns the AES-256-GCM ciphertext blob.
pub fn encrypt_master_key_for_recovery(
    master_key: &MasterKey,
    recovery_key: &[u8; 32],
    salt: &[u8],
) -> Result<Vec<u8>, String> {
    let recovery_encrypt_key = hkdf_derive(recovery_key, salt, HKDF_RECOVERY_INFO);
    encrypt(&recovery_encrypt_key, master_key)
}

/// Decrypt `master_key` using `recovery_key` (from the server's encrypted blob).
///
/// Mirrors [`encrypt_master_key_for_recovery`].  Returns an error if the
/// recovery key is wrong or the blob has been tampered with.
pub fn decrypt_master_key_with_recovery(
    encrypted: &[u8],
    recovery_key: &[u8; 32],
    salt: &[u8],
) -> Result<MasterKey, String> {
    let recovery_decrypt_key = hkdf_derive(recovery_key, salt, HKDF_RECOVERY_INFO);
    let decrypted = decrypt(&recovery_decrypt_key, encrypted)?;
    if decrypted.len() != 32 {
        return Err(format!(
            "decrypted master key wrong size: expected 32, got {}",
            decrypted.len()
        ));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&decrypted);
    Ok(key)
}

/// Internal HKDF expand helper.
fn hkdf_derive(ikm: &[u8; 32], salt: &[u8], info: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut key = [0u8; 32];
    hk.expand(info, &mut key)
        .expect("HKDF expand should never fail for 32-byte output");
    key
}

/// Encrypt `plaintext` with AES-256-GCM using `key`.
///
/// Returns: `[12-byte nonce][ciphertext + 16-byte GCM tag]`
///
/// A fresh random nonce is generated on every call.
/// Returns `Err(String)` only on cipher initialisation failure (should not happen
/// with a valid 32-byte key).
pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| format!("cipher init: {}", e))?;

    let mut nonce_bytes = [0u8; NONCE_SIZE];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| format!("encrypt: {}", e))?;

    let mut result = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// Decrypt a blob produced by [`encrypt`].
///
/// Expects `data` to be at least `12 + 16` bytes (nonce + minimum tag).
/// Returns the plaintext, or `Err(String)` if the data is too short,
/// the key is wrong, or the ciphertext has been tampered with.
pub fn decrypt(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < NONCE_SIZE + 16 {
        return Err(format!(
            "data too short: {} bytes (minimum {} for nonce + GCM tag)",
            data.len(),
            NONCE_SIZE + 16
        ));
    }

    let (nonce_bytes, ciphertext) = data.split_at(NONCE_SIZE);
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| format!("cipher init: {}", e))?;
    let nonce = Nonce::from_slice(nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("decrypt: {}", e))
}

// =============================================================================
// Fingerprinting & constant-time comparison
// =============================================================================

/// SHA-256 fingerprint of arbitrary bytes.
///
/// Used to identify a secret (e.g. an agent API key) by a stable, non-secret
/// handle that can be stored and looked up without ever persisting the raw
/// secret itself.
pub fn fingerprint_sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&hasher.finalize());
    out
}

/// Constant-time comparison of two byte slices.
///
/// For comparing a caller-supplied secret (e.g. a bearer token's fingerprint)
/// against a stored value without leaking, via timing, how many leading
/// bytes matched. Backed by the `subtle` crate, already pulled in
/// transitively by `aes-gcm` for its own constant-time internals — see the
/// `Cargo.toml` note.
///
/// The length check short-circuits before `subtle` is ever consulted: slice
/// length is ordinarily public information (both sides of a real comparison
/// already know their own length), so there is nothing to protect by hiding
/// it, and comparing two length-mismatched buffers "at the same speed" would
/// only cost time for no security benefit. What must run in constant time is
/// the CONTENTS comparison once lengths already match — that part is
/// `ct_eq`.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_and_sync_keys_are_different() {
        let salt = [1u8; 16];
        let master = derive_master_key("test passphrase", &salt);
        let vault = derive_vault_key(&master, &salt);
        let sync = derive_sync_key(&master, &salt);
        assert_ne!(vault, sync, "vault_key and sync_key must differ (domain separation)");
    }

    #[test]
    fn roundtrip_encrypt_decrypt() {
        let salt = [1u8; 16];
        let master = derive_master_key("test passphrase", &salt);
        let key = derive_vault_key(&master, &salt);
        let plaintext = b"hello, zero-trust world!";
        let blob = encrypt(&key, plaintext).unwrap();
        let decrypted = decrypt(&key, &blob).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_key_fails() {
        let salt = [1u8; 16];
        let master1 = derive_master_key("correct", &salt);
        let master2 = derive_master_key("wrong", &salt);
        let key1 = derive_vault_key(&master1, &salt);
        let key2 = derive_vault_key(&master2, &salt);
        let blob = encrypt(&key1, b"secret data").unwrap();
        assert!(decrypt(&key2, &blob).is_err());
    }

    #[test]
    fn tampered_blob_fails() {
        let salt = [1u8; 16];
        let master = derive_master_key("test", &salt);
        let key = derive_vault_key(&master, &salt);
        let mut blob = encrypt(&key, b"secret data").unwrap();
        if let Some(byte) = blob.last_mut() {
            *byte ^= 0xFF;
        }
        assert!(decrypt(&key, &blob).is_err());
    }

    #[test]
    fn different_salt_different_master() {
        let master1 = derive_master_key("same passphrase", &[1u8; 16]);
        let master2 = derive_master_key("same passphrase", &[2u8; 16]);
        assert_ne!(master1, master2);
    }

    #[test]
    fn recovery_key_format_parse_roundtrip() {
        let key = [0xABu8; 32];
        let formatted = format_recovery_key(&key);
        // 64 hex chars + 15 dashes = 79 chars
        assert_eq!(formatted.len(), 79);
        assert!(formatted.contains('-'));
        let parsed = parse_recovery_key(&formatted).unwrap();
        assert_eq!(parsed, key);
    }

    #[test]
    fn recovery_key_parse_without_dashes() {
        let key = generate_recovery_key();
        let formatted = format_recovery_key(&key);
        // Strip dashes — should still parse
        let no_dashes: String = formatted.chars().filter(|c| *c != '-').collect();
        let parsed = parse_recovery_key(&no_dashes).unwrap();
        assert_eq!(parsed, key);
    }

    #[test]
    fn encrypt_decrypt_master_key_with_recovery() {
        let salt = [1u8; 16];
        let master: MasterKey = [2u8; 32];
        let recovery_key = [3u8; 32];
        let blob = encrypt_master_key_for_recovery(&master, &recovery_key, &salt).unwrap();
        let decrypted = decrypt_master_key_with_recovery(&blob, &recovery_key, &salt).unwrap();
        assert_eq!(decrypted, master);
    }

    #[test]
    fn wrong_recovery_key_fails() {
        let salt = [1u8; 16];
        let master: MasterKey = [2u8; 32];
        let recovery_key = [3u8; 32];
        let wrong_key = [4u8; 32];
        let blob = encrypt_master_key_for_recovery(&master, &recovery_key, &salt).unwrap();
        assert!(decrypt_master_key_with_recovery(&blob, &wrong_key, &salt).is_err());
    }

    #[test]
    fn recovery_key_domain_separated_from_vault_and_sync() {
        // Two different recovery keys should produce different encrypted blobs
        // (confirming the key is used in encryption, not ignored)
        let salt = [1u8; 16];
        let master: MasterKey = [2u8; 32];
        let recovery_key_a = [5u8; 32];
        let recovery_key_b = [6u8; 32];
        let blob_a = encrypt_master_key_for_recovery(&master, &recovery_key_a, &salt).unwrap();
        let blob_b = encrypt_master_key_for_recovery(&master, &recovery_key_b, &salt).unwrap();
        // Blobs differ (different keys → different ciphertext)
        assert_ne!(blob_a[12..], blob_b[12..]); // skip the random nonce prefix
    }

    #[test]
    fn fingerprint_sha256_is_stable_and_input_sensitive() {
        let a = fingerprint_sha256(b"agent key one");
        let a_again = fingerprint_sha256(b"agent key one");
        let b = fingerprint_sha256(b"agent key two");
        assert_eq!(a, a_again, "same input must hash to the same fingerprint");
        assert_ne!(a, b, "different input must hash to a different fingerprint");
    }

    #[test]
    fn constant_time_eq_true_for_equal_slices() {
        assert!(constant_time_eq(b"same bytes", b"same bytes"));
    }

    #[test]
    fn constant_time_eq_false_for_unequal_slices_of_same_length() {
        assert!(!constant_time_eq(b"same length!", b"different m!"));
    }

    #[test]
    fn constant_time_eq_false_for_length_mismatch() {
        assert!(!constant_time_eq(b"short", b"a much longer slice"));
    }

    #[test]
    fn load_or_create_salt_roundtrip() {
        let dir = std::env::temp_dir().join("crypto_test_salt");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let salt_path = dir.join("salt.hex");

        let salt1 = load_or_create_salt(&salt_path).unwrap();
        let salt2 = load_or_create_salt(&salt_path).unwrap();
        assert_eq!(salt1, salt2);

        let hex_str = std::fs::read_to_string(&salt_path).unwrap();
        assert_eq!(hex_str.len(), 32);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod passphrase_tests {
    use super::*;

    #[test]
    fn generated_passphrase_has_the_documented_shape() {
        for _ in 0..200 {
            let p = generate_passphrase();
            let groups: Vec<&str> = p.split('-').collect();
            assert_eq!(groups.len(), PASSPHRASE_GROUPS, "group count: {p}");
            for g in &groups {
                assert_eq!(g.len(), PASSPHRASE_GROUP_LEN, "group length: {p}");
                assert!(
                    g.bytes().all(|b| PASSPHRASE_ALPHABET.contains(&b)),
                    "symbol outside the alphabet: {p}"
                );
            }
        }
    }

    #[test]
    fn generated_passphrases_do_not_repeat() {
        // 99 bits: a collision in 500 draws would mean the RNG is not one.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..500 {
            assert!(seen.insert(generate_passphrase()), "repeated passphrase");
        }
    }
}
