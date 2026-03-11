//! OS keychain integration for secure credential storage.
//! Uses Windows Credential Manager, macOS Keychain, or Linux libsecret.

use keyring::Entry;

const SERVICE_NAME: &str = "mylittlechart";

/// Store a credential in the OS keychain.
/// key_name format: "{exchange}:{account_id}" e.g. "binance:main"
pub fn store_credential(key_name: &str, secret: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, key_name)
        .map_err(|e| format!("keychain entry error: {}", e))?;
    entry.set_password(secret)
        .map_err(|e| format!("keychain store error: {}", e))?;
    Ok(())
}

/// Retrieve a credential from the OS keychain.
pub fn get_credential(key_name: &str) -> Result<Option<String>, String> {
    let entry = Entry::new(SERVICE_NAME, key_name)
        .map_err(|e| format!("keychain entry error: {}", e))?;
    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("keychain read error: {}", e)),
    }
}

/// Delete a credential from the OS keychain.
pub fn delete_credential(key_name: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, key_name)
        .map_err(|e| format!("keychain entry error: {}", e))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()), // already gone
        Err(e) => Err(format!("keychain delete error: {}", e)),
    }
}

/// List all stored credential names (not the secrets themselves).
/// Note: keyring crate doesn't support listing, so this is a placeholder
/// that would need to be backed by a local index.
pub fn list_credential_names() -> Vec<String> {
    // The keyring crate doesn't support enumeration.
    // Credential names should be tracked in UserProfile or a local index file.
    Vec::new()
}

// =============================================================================
// Exchange API secret helpers
// =============================================================================

/// Store an exchange API secret in the OS keychain.
///
/// The keychain key is formatted as `"exchange:{exchange}:{key_label}"` to
/// avoid collisions with other credential types.
///
/// # Arguments
/// * `exchange` — lowercase exchange identifier, e.g. `"binance"`.
/// * `key_label` — the public API key string, used as the account identifier.
/// * `secret` — the API secret to store.
pub fn store_exchange_secret(exchange: &str, key_label: &str, secret: &str) -> Result<(), String> {
    let key_name = format!("exchange:{}:{}", exchange, key_label);
    store_credential(&key_name, secret)
}

/// Retrieve an exchange API secret from the OS keychain.
///
/// Returns `Ok(Some(secret))` if found, `Ok(None)` if not stored yet.
///
/// # Arguments
/// * `exchange` — lowercase exchange identifier, e.g. `"binance"`.
/// * `key_label` — the public API key string used as the account identifier.
pub fn get_exchange_secret(exchange: &str, key_label: &str) -> Result<Option<String>, String> {
    let key_name = format!("exchange:{}:{}", exchange, key_label);
    get_credential(&key_name)
}

/// Delete an exchange API secret from the OS keychain.
///
/// Silently succeeds if the entry does not exist.
pub fn delete_exchange_secret(exchange: &str, key_label: &str) -> Result<(), String> {
    let key_name = format!("exchange:{}:{}", exchange, key_label);
    delete_credential(&key_name)
}

/// Store an exchange API passphrase in the OS keychain (OKX, KuCoin style).
///
/// The keychain key is formatted as `"passphrase:{exchange}:{key_label}"`.
pub fn store_exchange_passphrase(
    exchange: &str,
    key_label: &str,
    passphrase: &str,
) -> Result<(), String> {
    let key_name = format!("passphrase:{}:{}", exchange, key_label);
    store_credential(&key_name, passphrase)
}

/// Retrieve an exchange API passphrase from the OS keychain.
pub fn get_exchange_passphrase(
    exchange: &str,
    key_label: &str,
) -> Result<Option<String>, String> {
    let key_name = format!("passphrase:{}:{}", exchange, key_label);
    get_credential(&key_name)
}

/// Delete an exchange API passphrase from the OS keychain.
pub fn delete_exchange_passphrase(exchange: &str, key_label: &str) -> Result<(), String> {
    let key_name = format!("passphrase:{}:{}", exchange, key_label);
    delete_credential(&key_name)
}

// =============================================================================
// E2E master key helpers
// =============================================================================

/// Store the E2E encryption master key in the OS keychain.
///
/// The 32-byte key is encoded as lowercase hex before storage so it is
/// handled as a printable string by all OS keychain backends.
pub fn store_e2e_master_key(key: &[u8; 32]) -> Result<(), String> {
    let hex = bytes_to_hex(key);
    store_credential("e2e:master:default", &hex)
}

/// Retrieve the E2E encryption master key from the OS keychain.
///
/// Returns `Ok(None)` if no key has been stored yet (first-time setup).
/// Returns `Err` only on keychain access failures.
pub fn get_e2e_master_key() -> Result<Option<[u8; 32]>, String> {
    match get_credential("e2e:master:default")? {
        None => Ok(None),
        Some(hex) => {
            let bytes = hex_to_bytes_32(&hex)
                .ok_or_else(|| format!("E2E master key in keychain is malformed (expected 64 hex chars, got {} chars)", hex.len()))?;
            Ok(Some(bytes))
        }
    }
}

/// Delete the E2E encryption master key from the OS keychain.
///
/// Silently succeeds if the entry does not exist.
pub fn delete_e2e_master_key() -> Result<(), String> {
    delete_credential("e2e:master:default")
}

// =============================================================================
// Inline hex utilities (no external `hex` crate required)
// =============================================================================

/// Encode a byte slice as a lowercase hex string.
fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

/// Decode a 64-character lowercase hex string into exactly 32 bytes.
/// Returns `None` if the string length is wrong or contains non-hex chars.
fn hex_to_bytes_32(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let hex = hex.as_bytes();
    let mut out = [0u8; 32];
    for (i, chunk) in hex.chunks(2).enumerate() {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

/// Convert a single ASCII hex character to its numeric value.
/// Returns `None` for non-hex characters.
fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytes_to_hex_roundtrip() {
        let key: [u8; 32] = [
            0x00, 0x01, 0x0f, 0x10, 0xff, 0xab, 0xcd, 0xef,
            0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0,
            0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6, 0x07, 0x18,
            0x29, 0x3a, 0x4b, 0x5c, 0x6d, 0x7e, 0x8f, 0x90,
        ];
        let hex = bytes_to_hex(&key);
        assert_eq!(hex.len(), 64);
        let decoded = hex_to_bytes_32(&hex).expect("roundtrip decode failed");
        assert_eq!(decoded, key);
    }

    #[test]
    fn test_hex_to_bytes_32_wrong_length() {
        assert!(hex_to_bytes_32("deadbeef").is_none());
        assert!(hex_to_bytes_32("").is_none());
    }

    #[test]
    fn test_hex_to_bytes_32_invalid_char() {
        // 64 chars but contains 'g' which is not valid hex
        let bad = "gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg";
        assert!(hex_to_bytes_32(bad).is_none());
    }
}
