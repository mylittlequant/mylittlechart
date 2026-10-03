//! Proof-of-work for the login challenge.
//!
//! The wasm transport calls [`solve_pow`] before the two public sign-in
//! posts. Native builds compile this module so the search can be tested on
//! the host; they do not draw a challenge of their own.

use sha2::{Digest, Sha256};

/// Counters tried before [`solve_pow`] gives up. `1 << 22`.
const MAX_TRIES: u64 = 1 << 22;
/// Difficulties above this are refused without searching.
const MAX_DIFFICULTY: u32 = 24;

/// Search a nonce whose SHA-256 preimage has `difficulty` leading zero bits.
///
/// The preimage is the UTF-8 of `id + "." + nonce`. The nonce is
/// `format!("{:016x}", counter)` for a `u64` counter, starting at zero.
///
/// Returns `None` when `id` is not 32 bytes long, `difficulty` is above 24,
/// or no counter below `1 << 22` hits.
pub fn solve_pow(id: &str, difficulty: u32) -> Option<String> {
    if id.len() != 32 || difficulty > MAX_DIFFICULTY {
        return None;
    }
    for counter in 0..MAX_TRIES {
        let nonce = format!("{counter:016x}");
        if pow_leading_zero_bits(id, &nonce) >= difficulty {
            return Some(nonce);
        }
    }
    None
}

/// The `X-Login-Challenge` value: `id.nonce`.
pub fn challenge_header(id: &str, nonce: &str) -> String {
    format!("{id}.{nonce}")
}

fn pow_leading_zero_bits(id: &str, nonce: &str) -> u32 {
    let mut hasher = Sha256::new();
    hasher.update(id.as_bytes());
    hasher.update(b".");
    hasher.update(nonce.as_bytes());
    leading_zero_bits(hasher.finalize().as_ref())
}

/// Leading zero bits of `hash`, counting from the most significant bit.
fn leading_zero_bits(hash: &[u8]) -> u32 {
    let mut bits = 0u32;
    for byte in hash {
        if *byte != 0 {
            return bits + byte.leading_zeros();
        }
        bits += 8;
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::{challenge_header, solve_pow};
    use sha2::{Digest, Sha256};

    #[test]
    fn difficulty_8_yields_a_16_hex_nonce_whose_hash_has_8_leading_zero_bits() {
        let id = "00000000000000000000000000000000";
        let nonce = solve_pow(id, 8).expect("a solution within the try cap");
        assert_eq!(nonce.len(), 16, "{nonce}");
        assert!(
            nonce.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "{nonce}"
        );
        let header = challenge_header(id, &nonce);
        assert_eq!(header, format!("{id}.{nonce}"));
        assert_eq!(header.matches('.').count(), 1);

        let digest = Sha256::digest(header.as_bytes());
        let bytes: &[u8] = digest.as_ref();
        let mut bits = 0u32;
        for byte in bytes {
            if *byte != 0 {
                bits += byte.leading_zeros();
                break;
            }
            bits += 8;
        }
        assert!(bits >= 8, "leading zero bits {bits}");
    }

    #[test]
    fn a_short_id_or_an_absurd_difficulty_is_refused() {
        assert!(solve_pow("00", 8).is_none());
        assert!(solve_pow("00000000000000000000000000000000", 25).is_none());
    }
}
