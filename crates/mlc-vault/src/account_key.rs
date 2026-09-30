//! The one key of an anonymous / invite account.
//!
//! The client draws `K` (32 random bytes) and shows it once. Two unrelated
//! secrets are derived from it with HKDF-SHA256 (salt [`HKDF_SALT`]):
//!
//! * `C = HKDF(K, info = "auth")` — the login credential. It is the only value
//!   that ever leaves the device (hex on the wire); the server stores its
//!   SHA-256.
//! * `V = HKDF(K, info = "vault")` — the vault key. It never leaves the
//!   device and keys the account's vault in place of the passphrase-derived
//!   key OAuth accounts use.
//!
//! A server that knows `C` (even `K`'s hash of it) learns nothing about `V`:
//! the two are independent HKDF outputs of a 256-bit secret.
//!
//! Display form: Crockford base32, groups of four, separated by `-`. 256 bits
//! are 52 symbols (13 groups; the last symbol carries one data bit and four
//! zero pad bits) followed by one checksum group — 20 bits of
//! `SHA-256("mlc/anon/v1/check" || K)` — so a mistyped key is refused on the
//! device before anything is sent. Parsing is forgiving about case,
//! separators and the Crockford look-alikes (`O`→`0`, `I`/`L`→`1`).

use hkdf::Hkdf;
use rand::RngCore;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use crate::crypto::VaultKey;

/// HKDF salt shared by both derivations.
pub const HKDF_SALT: &[u8] = b"mlc/anon/v1";
/// HKDF info of the login credential `C`.
pub const HKDF_AUTH_INFO: &[u8] = b"auth";
/// HKDF info of the vault key `V`.
pub const HKDF_VAULT_INFO: &[u8] = b"vault";
/// Domain prefix of the display checksum.
const CHECK_DOMAIN: &[u8] = b"mlc/anon/v1/check";

/// Bytes of `K`.
pub const KEY_BYTES: usize = 32;
/// Symbols that carry `K` (256 bits, padded to 260).
const BODY_SYMBOLS: usize = 52;
/// Symbols of the checksum group.
const CHECK_SYMBOLS: usize = 4;
/// All symbols of the display form.
const TOTAL_SYMBOLS: usize = BODY_SYMBOLS + CHECK_SYMBOLS;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Why a typed key was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountKeyError {
    /// The wrong number of symbols (after dropping separators).
    Length,
    /// A character outside the Crockford alphabet.
    Character,
    /// The checksum group does not match: a typo somewhere.
    Checksum,
}

impl std::fmt::Display for AccountKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Length => "account key has the wrong length",
            Self::Character => "account key has a character that cannot be part of a key",
            Self::Checksum => "account key checksum does not match",
        })
    }
}

impl std::error::Error for AccountKeyError {}

/// `K`. Zeroed on drop; `Debug` never prints it.
#[derive(Clone, PartialEq, Eq)]
pub struct AccountKey([u8; KEY_BYTES]);

impl std::fmt::Debug for AccountKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AccountKey(<redacted>)")
    }
}

impl Drop for AccountKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl AccountKey {
    /// A fresh key from the platform CSPRNG (`crypto.getRandomValues` in the
    /// browser, the OS generator natively).
    pub fn generate() -> Self {
        let mut bytes = [0u8; KEY_BYTES];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self(bytes)
    }

    /// A key from known bytes (tests, vectors).
    pub fn from_bytes(bytes: [u8; KEY_BYTES]) -> Self {
        Self(bytes)
    }

    /// The raw bytes of `K`.
    pub fn as_bytes(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }

    /// The display form: 14 groups of four symbols, the last one the checksum.
    pub fn display(&self) -> String {
        let mut symbols = encode_bits(&self.0, BODY_SYMBOLS);
        symbols.extend(encode_bits(&checksum_bytes(&self.0), CHECK_SYMBOLS));
        let mut out = String::with_capacity(TOTAL_SYMBOLS + TOTAL_SYMBOLS / 4);
        for (i, symbol) in symbols.iter().enumerate() {
            if i > 0 && i % 4 == 0 {
                out.push('-');
            }
            out.push(char::from(ALPHABET[usize::from(*symbol)]));
        }
        out
    }

    /// Parse a typed or pasted key. Case, `-`, spaces and the look-alikes
    /// `O`/`I`/`L` are tolerated; a wrong length, a foreign character or a
    /// checksum mismatch is an error.
    pub fn parse(text: &str) -> Result<Self, AccountKeyError> {
        let mut symbols: Vec<u8> = Vec::with_capacity(TOTAL_SYMBOLS);
        for ch in text.chars() {
            if ch == '-' || ch.is_whitespace() {
                continue;
            }
            symbols.push(decode_symbol(ch).ok_or(AccountKeyError::Character)?);
        }
        if symbols.len() != TOTAL_SYMBOLS {
            return Err(AccountKeyError::Length);
        }
        let (body, check) = symbols.split_at(BODY_SYMBOLS);
        let bytes = decode_body(body).ok_or(AccountKeyError::Checksum)?;
        let expected = encode_bits(&checksum_bytes(&bytes), CHECK_SYMBOLS);
        if expected != check {
            return Err(AccountKeyError::Checksum);
        }
        Ok(Self(bytes))
    }

    /// The login credential `C`.
    pub fn credential(&self) -> [u8; 32] {
        derive(&self.0, HKDF_AUTH_INFO)
    }

    /// `C` as 64 lowercase hex characters — the only form that goes on a wire.
    pub fn credential_hex(&self) -> String {
        hex::encode(self.credential())
    }

    /// The vault key `V`. Never sent anywhere.
    pub fn vault_key(&self) -> VaultKey {
        derive(&self.0, HKDF_VAULT_INFO)
    }
}

/// Whether `text` is a well-formed credential: 64 hex characters.
pub fn is_credential_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn derive(ikm: &[u8; KEY_BYTES], info: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT), ikm);
    let mut out = [0u8; 32];
    // A 32-byte expand is far below HKDF-SHA256's 255 * 32 byte limit, so this
    // cannot fail; the zeroed output is what a failure would leave behind.
    if hk.expand(info, &mut out).is_err() {
        out = [0u8; 32];
    }
    out
}

/// The first 20 bits of the checksum digest, left-aligned in 3 bytes.
fn checksum_bytes(key: &[u8; KEY_BYTES]) -> [u8; 3] {
    let mut hasher = Sha256::new();
    hasher.update(CHECK_DOMAIN);
    hasher.update(key);
    let digest = hasher.finalize();
    [digest[0], digest[1], digest[2]]
}

/// `count` 5-bit symbols from the big-endian bit string of `bytes`, zero
/// padded on the right.
fn encode_bits(bytes: &[u8], count: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(count);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut iter = bytes.iter();
    while out.len() < count {
        if bits < 5 {
            match iter.next() {
                Some(b) => {
                    acc = (acc << 8) | u32::from(*b);
                    bits += 8;
                }
                None => {
                    acc <<= 5 - bits;
                    bits = 5;
                }
            }
        }
        bits -= 5;
        out.push(((acc >> bits) & 0x1f) as u8);
        acc &= (1 << bits) - 1;
    }
    out
}

/// The 32 bytes behind 52 symbols; `None` when the four pad bits are not zero.
fn decode_body(symbols: &[u8]) -> Option<[u8; KEY_BYTES]> {
    let mut out = [0u8; KEY_BYTES];
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut idx = 0usize;
    for symbol in symbols {
        acc = (acc << 5) | u32::from(*symbol);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            if let Some(slot) = out.get_mut(idx) {
                *slot = ((acc >> bits) & 0xff) as u8;
                idx += 1;
            }
            acc &= (1 << bits) - 1;
        }
    }
    (acc == 0 && idx == KEY_BYTES).then_some(out)
}

fn decode_symbol(ch: char) -> Option<u8> {
    let up = ch.to_ascii_uppercase();
    let up = match up {
        'O' => '0',
        'I' | 'L' => '1',
        other => other,
    };
    let byte = u8::try_from(up).ok()?;
    ALPHABET.iter().position(|a| *a == byte).and_then(|p| u8::try_from(p).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seq() -> AccountKey {
        let mut bytes = [0u8; KEY_BYTES];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = i as u8;
        }
        AccountKey::from_bytes(bytes)
    }

    // Vectors computed independently (Python hmac/hashlib HKDF-SHA256).
    const SEQ_DISPLAY: &str = "000G-40R4-0M30-E209-185G-R38E-1W81-24GK-2GAH-C5RR-34D1-P70X-3RFG-ZHQJ";
    const SEQ_C: &str = "b94a08ef8cf6f09519d070ee44d3118a7d6e0dcc27968fb6885cd6ff7fa16302";
    const SEQ_V: &str = "781cffd79dba499fc386a3e598b502c3ede9ee1c9e35edc4560d17674edfc55b";
    const ZERO_DISPLAY: &str = "0000-0000-0000-0000-0000-0000-0000-0000-0000-0000-0000-0000-0000-T51E";
    const ZERO_C: &str = "d5d727091e9fe9690b8939f1658e4ca82aaf555715a0eba367df28549075bb49";
    const ZERO_V: &str = "4aa12604fa764d28f30e7cec4bf2ff7824e532e7e172af9a96a0d6a7063907b1";
    const FF_DISPLAY: &str = "ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZG-3VZE";
    const FF_C: &str = "9573944a326c5cb4b1065dbac74b6368526d26c51bfdd58c432e61a7036ed066";
    const FF_V: &str = "2c5b112dbcd8ea767466664013a0e5b1913d35b34e68aee1e5367aa3f62bd09f";

    #[test]
    fn kdf_known_answers() {
        for (key, display, c, v) in [
            (seq(), SEQ_DISPLAY, SEQ_C, SEQ_V),
            (AccountKey::from_bytes([0u8; 32]), ZERO_DISPLAY, ZERO_C, ZERO_V),
            (AccountKey::from_bytes([0xffu8; 32]), FF_DISPLAY, FF_C, FF_V),
        ] {
            assert_eq!(key.display(), display);
            assert_eq!(key.credential_hex(), c);
            assert_eq!(hex::encode(key.vault_key()), v);
        }
    }

    #[test]
    fn credential_and_vault_key_are_independent() {
        let key = seq();
        assert_ne!(key.credential(), key.vault_key());
        assert_ne!(&key.credential(), key.as_bytes());
        assert_ne!(&key.vault_key(), key.as_bytes());
    }

    #[test]
    fn display_round_trips_and_has_fourteen_groups() {
        for _ in 0..64 {
            let key = AccountKey::generate();
            let shown = key.display();
            assert_eq!(shown.len(), TOTAL_SYMBOLS + 13);
            assert_eq!(shown.split('-').count(), 14);
            assert!(shown.split('-').all(|g| g.len() == 4));
            let parsed = AccountKey::parse(&shown).expect("a displayed key parses");
            assert_eq!(parsed, key);
        }
    }

    #[test]
    fn parse_tolerates_case_separators_and_lookalikes() {
        let shown = seq().display();
        assert_eq!(AccountKey::parse(&shown.to_lowercase()).ok(), Some(seq()));
        assert_eq!(AccountKey::parse(&shown.replace('-', " ")).ok(), Some(seq()));
        assert_eq!(AccountKey::parse(&shown.replace('-', "")).ok(), Some(seq()));
        assert_eq!(AccountKey::parse(&format!("  {shown}\n")).ok(), Some(seq()));
        // O reads as 0, I and L as 1.
        let lookalike = shown.replace('0', "O").replace('1', "l");
        assert_ne!(lookalike, shown);
        assert_eq!(AccountKey::parse(&lookalike).ok(), Some(seq()));
    }

    #[test]
    fn every_one_character_typo_is_refused() {
        let shown = seq().display();
        for (pos, original) in shown.char_indices().filter(|(_, c)| *c != '-') {
            for replacement in ALPHABET.iter().map(|b| char::from(*b)) {
                if replacement == original {
                    continue;
                }
                let mut typo = shown.clone();
                typo.replace_range(pos..pos + 1, &replacement.to_string());
                assert!(
                    AccountKey::parse(&typo).is_err(),
                    "typo {replacement} at {pos} must be refused"
                );
            }
        }
    }

    #[test]
    fn a_swapped_pair_of_symbols_is_refused() {
        let shown = seq().display();
        let mut chars: Vec<char> = shown.chars().collect();
        chars.swap(3, 5);
        let swapped: String = chars.into_iter().collect();
        assert_ne!(swapped, shown);
        assert!(AccountKey::parse(&swapped).is_err());
    }

    #[test]
    fn wrong_length_and_foreign_characters_are_told_apart() {
        let shown = seq().display();
        assert_eq!(AccountKey::parse(""), Err(AccountKeyError::Length));
        assert_eq!(AccountKey::parse(&shown[..shown.len() - 1]), Err(AccountKeyError::Length));
        assert_eq!(AccountKey::parse(&format!("{shown}0")), Err(AccountKeyError::Length));
        let mut with_u = shown.clone();
        with_u.replace_range(0..1, "U");
        assert_eq!(AccountKey::parse(&with_u), Err(AccountKeyError::Character));
        assert_eq!(AccountKey::parse(&shown.replace('-', "*")), Err(AccountKeyError::Character));
    }

    #[test]
    fn debug_never_prints_the_key() {
        let key = seq();
        let printed = format!("{key:?}");
        assert!(!printed.contains("000G"));
        assert!(printed.contains("redacted"));
    }

    #[test]
    fn credential_shape_check() {
        assert!(is_credential_hex(&seq().credential_hex()));
        assert!(!is_credential_hex(SEQ_DISPLAY));
        assert!(!is_credential_hex(&SEQ_C[..63]));
        assert!(!is_credential_hex(&SEQ_C.replace('b', "g")));
    }
}
