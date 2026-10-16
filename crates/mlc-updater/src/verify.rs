//! Ed25519 signature verification for OTA binary updates.
//!
//! The signature is computed over the **raw binary bytes** of the release
//! file — the same bytes that SHA-256 is computed over in `download.rs`.
//! This means a single Ed25519 sign/verify covers the entire executable,
//! including any padding or embedded resources.
//!
//! # Key rotation
//!
//! [`allowed_public_keys`] is the 32-byte Ed25519 public key from
//! `MLC_UPDATE_PUBLIC_KEY` (64 hex characters) at build time, or empty when
//! that value is unset. During a key rotation, ship a build that trusts the
//! new key. See the PRD (`prd-code-signing.md` §7.2) for the full procedure.
//!
//! # Transition period
//!
//! Until the first signed release ships, `signature` will be `None` or an
//! empty string in manifests. Callers should treat those cases as warnings
//! rather than hard errors. See [`VerifyResult::Unsigned`].

use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

// =============================================================================
// Trusted public keys
// =============================================================================

const fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// `MLC_UPDATE_PUBLIC_KEY` is 64 hex chars (32 bytes). Anything else is no key.
const fn parse_pubkey_hex(hex: &str) -> Option<[u8; 32]> {
    let bytes = hex.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        let hi = match hex_nibble(bytes[i * 2]) {
            Some(v) => v,
            None => return None,
        };
        let lo = match hex_nibble(bytes[i * 2 + 1]) {
            Some(v) => v,
            None => return None,
        };
        out[i] = (hi << 4) | lo;
        i += 1;
    }
    Some(out)
}

/// Key from `MLC_UPDATE_PUBLIC_KEY`, when the build supplied 64 hex characters.
static UPDATE_PUBLIC_KEY: Option<[u8; 32]> = match option_env!("MLC_UPDATE_PUBLIC_KEY") {
    Some(hex) => parse_pubkey_hex(hex),
    None => None,
};

/// Trusted Ed25519 public keys.
///
/// Empty when `MLC_UPDATE_PUBLIC_KEY` is unset or not 64 hex characters.
/// Unsigned manifests then stay [`VerifyResult::Unsigned`].
pub fn allowed_public_keys() -> &'static [[u8; 32]] {
    match UPDATE_PUBLIC_KEY.as_ref() {
        Some(key) => std::slice::from_ref(key),
        None => &[],
    }
}

// =============================================================================
// Verification result
// =============================================================================

/// Result of a signature verification attempt.
#[derive(Debug)]
pub enum VerifyResult {
    /// Signature verified against a trusted public key — safe to install.
    Valid,
    /// No signature was provided (empty or absent). During the transition
    /// period this is treated as a warning and install is allowed. After the
    /// transition period `do_install()` should be changed to reject this case.
    Unsigned,
    /// Signature provided but verification failed against all trusted keys.
    /// The update MUST be rejected — do not install.
    Invalid(String),
    /// Signature string could not be decoded (bad base64, wrong length, etc.).
    /// The update MUST be rejected.
    FormatError(String),
}

// =============================================================================
// Public API
// =============================================================================

/// Verify an Ed25519 signature over `binary_data` against all trusted keys.
///
/// The `signature_b64` argument must be a standard (RFC 4648) base64-encoded
/// 64-byte Ed25519 signature — the same format produced by `tools/signer/`.
///
/// Returns [`VerifyResult::Valid`] if any key in [`allowed_public_keys`]
/// accepts the signature.  All other variants indicate either an unsigned
/// release (allowed during transition) or a security violation (must reject).
pub fn verify_binary_signature(binary_data: &[u8], signature_b64: &str) -> VerifyResult {
    let trimmed = signature_b64.trim();

    if trimmed.is_empty() {
        return VerifyResult::Unsigned;
    }

    // Strip any internal whitespace (e.g. from URL query param decoding where + became space).
    let cleaned: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();

    // Decode standard base64.
    let sig_bytes = match base64::engine::general_purpose::STANDARD.decode(&cleaned) {
        Ok(b) => b,
        Err(e) => return VerifyResult::FormatError(format!("base64 decode failed: {}", e)),
    };

    // Ed25519 signatures are always exactly 64 bytes.
    if sig_bytes.len() != 64 {
        return VerifyResult::FormatError(format!(
            "invalid signature length: {} bytes (expected 64)",
            sig_bytes.len()
        ));
    }

    let sig_array: [u8; 64] = match sig_bytes.try_into() {
        Ok(a) => a,
        Err(_) => return VerifyResult::FormatError("signature byte conversion failed".into()),
    };
    let signature = Signature::from_bytes(&sig_array);

    // The signer signs SHA256(binary), not the raw binary.
    // We must hash first so verify sees the same message.
    let hash = Sha256::digest(binary_data);

    // Try each trusted public key.  The first match is sufficient.
    for pubkey_bytes in allowed_public_keys() {
        let verifying_key = match VerifyingKey::from_bytes(pubkey_bytes) {
            Ok(k) => k,
            Err(e) => {
                log::warn!("[Updater] Skipping malformed public key in allowed_public_keys: {}", e);
                continue;
            }
        };

        if verifying_key.verify_strict(&hash, &signature).is_ok() {
            return VerifyResult::Valid;
        }
    }

    VerifyResult::Invalid(
        "signature does not match any trusted public key".into(),
    )
}

// =============================================================================
// Downgrade / rollback protection
// =============================================================================

/// Returns `true` if `proposed` is less than or equal to `current`,
/// meaning installing it would constitute a downgrade or re-install.
///
/// On parse failure the function returns `false` (don't block the install on
/// a version string it can't understand — log a warning and proceed).
pub fn is_downgrade(current: &str, proposed: &str) -> bool {
    match (
        semver::Version::parse(current),
        semver::Version::parse(proposed),
    ) {
        (Ok(c), Ok(p)) => p <= c,
        _ => {
            log::warn!(
                "[Updater] Could not parse versions for downgrade check: current='{}' proposed='{}'",
                current, proposed
            );
            false
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    // Fixed 32-byte test secret key A — do NOT use outside of tests.
    const TEST_SECRET_A: [u8; 32] = [
        0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60,
        0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0x44,
        0xda, 0x08, 0x9a, 0x7e, 0xad, 0xde, 0x71, 0x03,
        0x1b, 0xde, 0x0b, 0x9c, 0x6d, 0x6c, 0x5b, 0xdb,
    ];

    // Fixed 32-byte test secret key B (different from A) — do NOT use outside of tests.
    const TEST_SECRET_B: [u8; 32] = [
        0x4c, 0xcd, 0x08, 0x9b, 0x28, 0xff, 0x96, 0xda,
        0x9d, 0xb6, 0xc3, 0x46, 0xec, 0x11, 0x4e, 0x0f,
        0x5b, 0x8a, 0x31, 0x9f, 0x35, 0xab, 0xa6, 0x24,
        0xda, 0x8c, 0xf6, 0xed, 0x4d, 0x0b, 0x64, 0x5c,
    ];

    fn make_signing_key(secret: &[u8; 32]) -> SigningKey {
        SigningKey::from_bytes(secret)
    }

    fn sign_b64(signing_key: &SigningKey, data: &[u8]) -> String {
        // Match the real signer: sign SHA256(data), not data directly.
        let hash = Sha256::digest(data);
        let signature = signing_key.sign(&hash);
        base64::engine::general_purpose::STANDARD.encode(signature.to_bytes())
    }

    #[test]
    fn test_unsigned_empty_string() {
        match verify_binary_signature(b"hello", "") {
            VerifyResult::Unsigned => {}
            other => panic!("expected Unsigned, got {:?}", other),
        }
    }

    #[test]
    fn test_unsigned_whitespace_only() {
        match verify_binary_signature(b"hello", "   ") {
            VerifyResult::Unsigned => {}
            other => panic!("expected Unsigned, got {:?}", other),
        }
    }

    #[test]
    fn test_malformed_base64() {
        match verify_binary_signature(b"hello", "not_valid_base64!!!") {
            VerifyResult::FormatError(_) => {}
            other => panic!("expected FormatError, got {:?}", other),
        }
    }

    #[test]
    fn test_wrong_length_signature() {
        // 32 bytes in base64 — too short.
        let short = base64::engine::general_purpose::STANDARD.encode(&[0u8; 32]);
        match verify_binary_signature(b"hello", &short) {
            VerifyResult::FormatError(_) => {}
            other => panic!("expected FormatError, got {:?}", other),
        }
    }

    #[test]
    fn test_valid_signature_with_real_key() {
        let signing_key = make_signing_key(&TEST_SECRET_A);
        let pubkey_bytes = signing_key.verifying_key().to_bytes();
        let data = b"binary payload bytes";
        let sig_b64 = sign_b64(&signing_key, data);

        // Verify round-trip: sign_b64 signs SHA256(data), verify checks SHA256(data).
        let verifying_key = VerifyingKey::from_bytes(&pubkey_bytes).unwrap();
        let sig_bytes = base64::engine::general_purpose::STANDARD.decode(&sig_b64).unwrap();
        let sig_array: [u8; 64] = sig_bytes.try_into().unwrap();
        let signature = Signature::from_bytes(&sig_array);
        let hash = Sha256::digest(data);
        assert!(verifying_key.verify_strict(&hash, &signature).is_ok());
    }

    #[test]
    fn test_signature_wrong_key_rejected() {
        let signing_key_a = make_signing_key(&TEST_SECRET_A);
        let signing_key_b = make_signing_key(&TEST_SECRET_B);
        let wrong_pubkey = signing_key_b.verifying_key().to_bytes();
        let data = b"binary payload";
        let sig_b64 = sign_b64(&signing_key_a, data);

        let verifying_key = VerifyingKey::from_bytes(&wrong_pubkey).unwrap();
        let sig_bytes = base64::engine::general_purpose::STANDARD.decode(&sig_b64).unwrap();
        let sig_array: [u8; 64] = sig_bytes.try_into().unwrap();
        let signature = Signature::from_bytes(&sig_array);
        let hash = Sha256::digest(data);
        assert!(verifying_key.verify_strict(&hash, &signature).is_err());
    }

    #[test]
    fn test_signature_tampered_data_rejected() {
        let signing_key = make_signing_key(&TEST_SECRET_A);
        let pubkey_bytes = signing_key.verifying_key().to_bytes();
        let original = b"original binary";
        let tampered = b"tampered binary!";
        let sig_b64 = sign_b64(&signing_key, original);

        let verifying_key = VerifyingKey::from_bytes(&pubkey_bytes).unwrap();
        let sig_bytes = base64::engine::general_purpose::STANDARD.decode(&sig_b64).unwrap();
        let sig_array: [u8; 64] = sig_bytes.try_into().unwrap();
        let signature = Signature::from_bytes(&sig_array);
        let hash = Sha256::digest(tampered);
        assert!(verifying_key.verify_strict(&hash, &signature).is_err());
    }

    #[test]
    fn test_is_downgrade_same_version() {
        assert!(is_downgrade("1.0.0", "1.0.0"), "same version is a downgrade");
    }

    #[test]
    fn test_is_downgrade_older_version() {
        assert!(is_downgrade("2.0.0", "1.9.9"), "older version is a downgrade");
    }

    #[test]
    fn test_is_downgrade_newer_version() {
        assert!(!is_downgrade("1.0.0", "1.0.1"), "newer version is not a downgrade");
    }

    #[test]
    fn test_is_downgrade_invalid_version_strings() {
        // Should return false (don't block on unparseable versions).
        assert!(!is_downgrade("bad", "also-bad"));
    }
}
