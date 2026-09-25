//! End-to-end DM primitives: X25519 identity keypairs, per-conversation key
//! derivation, and AES-256-GCM sealed messages.
//!
//! Plan: `docs/mlc/plans/social-sharing-access-gates-and-agent-control-
//! 2026-09-05.md` §3 item 6 + §6 — DMs are paid, end-to-end; the identity
//! keypair is derived from the vault key on demand (owner ruling
//! 2026-09-09: never a separate secret to generate, persist, or publish by
//! hand — see [`DmIdentity::from_vault_key`]); the server holds only
//! ciphertext plus visible metadata (who/when).
//!
//! # Curve library: `curve25519-dalek`, not `x25519-dalek`
//!
//! `x25519-dalek` is not in the workspace `Cargo.lock`. `curve25519-dalek`
//! 4.1.3 already is — pulled in transitively via `ed25519-dalek` (used
//! elsewhere in the workspace for update-signature verification). X25519
//! needs nothing `x25519-dalek` would add on top of it:
//! [`curve25519_dalek::montgomery::MontgomeryPoint::mul_clamped`] /
//! `mul_base_clamped` apply RFC 7748 clamping and the scalar multiplication
//! themselves — that is the entire primitive. Depending on
//! `curve25519-dalek` directly from this crate (previously only a
//! transitive dependency here) promotes an already-locked, already-vetted
//! crate instead of adding a second curve implementation for the same math.
//!
//! # Key flow
//!
//! [`DmIdentity::from_vault_key`] is the real entry point: it derives the
//! X25519 keypair deterministically from the profile's already-unlocked
//! [`VaultKey`] via HKDF-SHA256, so the identity exists the moment the vault
//! does, on every device that holds the same vault, with nothing to
//! generate, publish by hand, or lose — publication to the server is an
//! automatic background step the caller drives off this, never a user
//! action. [`DmIdentity::generate`] mints an unrelated random X25519 keypair
//! from OS entropy and exists for tests that need two independent
//! identities (e.g. exercising [`conversation_key`] between "alice" and
//! "bob") — production callers never call it. [`conversation_key`] takes
//! each side's own secret and the other side's public key, runs ECDH, and
//! derives a symmetric key via HKDF-SHA256 (info = `"mylittlechart-dm-v1"`
//! || `conversation_id`) — both parties land on the identical key without
//! ever transmitting it. [`seal_message`] / [`open_message`] wrap
//! AES-256-GCM over that key, with `aad` bound in (recommended: conversation
//! id + sender id) so the server-visible metadata cannot be spliced onto a
//! different message's ciphertext without the tag failing to verify on
//! open.
//!
//! # Contributory behaviour
//!
//! As with any X25519 use, a peer offering a low-order public key can force
//! a fixed (all-zero or otherwise degenerate) shared secret. This module
//! does not reject such keys inline — matching the plain X25519 primitive —
//! so a caller accepting a stranger's public key for the first time should
//! verify it out-of-band via [`fingerprint`] before trusting a conversation
//! key derived from it, the same way the plan's UI is expected to show a
//! fingerprint for the user to confirm.

use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use curve25519_dalek::montgomery::MontgomeryPoint;
use curve25519_dalek::scalar::clamp_integer;
use hkdf::Hkdf;
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::crypto;
use crate::vault::{VaultError, VaultKey};

/// Legacy sidecar file name a pre-derivation build wrote a DM identity
/// keypair to, next to `vault.enc`. [`DmIdentity`] no longer reads or writes
/// this file — [`DmIdentity::from_vault_key`] derives the identity fresh
/// from the vault key every time — but the name stays here so a caller that
/// still sees the file on disk (an install that predates this change) can
/// name it in a one-time diagnostic without this module needing to know
/// where it lives. The file itself is never deleted by anything in this
/// crate.
pub const DM_IDENTITY_FILE_NAME: &str = "dm_identity.enc";

/// An X25519 keypair identifying one account for DMs.
///
/// `secret` is [`Zeroizing`]-wrapped so it self-wipes on drop. The type does
/// not implement `Debug`/`Clone`/`Serialize` — it is never persisted at all
/// (see [`Self::from_vault_key`]); the only way the secret half leaves this
/// module is by reading the field directly (e.g. into [`conversation_key`]).
pub struct DmIdentity {
    /// The public X25519 point, safe to publish (e.g. via the server's
    /// fingerprint listing — see [`fingerprint`]).
    pub public: [u8; 32],
    /// The private scalar. Never leaves the process, and never touches disk.
    pub secret: Zeroizing<[u8; 32]>,
}

impl DmIdentity {
    /// Generate a fresh X25519 identity keypair from OS entropy.
    ///
    /// The raw random scalar is RFC-7748-clamped once, at generation time;
    /// clamping is idempotent, so it is safe (and done again, harmlessly)
    /// by [`MontgomeryPoint::mul_clamped`]/`mul_base_clamped` on every use.
    ///
    /// Test-only in practice — see this module's doc for why a production
    /// caller derives its identity from the vault key instead
    /// ([`Self::from_vault_key`]) rather than minting one of these.
    pub fn generate() -> Self {
        let mut secret_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut secret_bytes);
        let secret_bytes = clamp_integer(secret_bytes);
        let public = MontgomeryPoint::mul_base_clamped(secret_bytes).to_bytes();
        Self {
            public,
            secret: Zeroizing::new(secret_bytes),
        }
    }

    /// Derive this profile's DM identity keypair from its already-unlocked
    /// [`VaultKey`] — deterministic, so the same vault key always derives
    /// the same keypair, on any device that holds it, and there is nothing
    /// to generate, persist, or publish by hand: the caller derives this
    /// fresh every time it needs the identity (on vault unlock, at boot if
    /// already unlocked, after a profile switch — see `mlc-app-shell-vello`'s
    /// call sites), and drops it when the vault seals.
    ///
    /// `HKDF-SHA256(ikm = vault key, salt = None, info =
    /// "mylittlechart dm identity v1") -> 32 raw bytes`, then RFC-7748-clamped
    /// exactly as [`Self::generate`] clamps a random scalar. No salt: the
    /// vault key is already a uniformly-random 32-byte AES key (never a
    /// low-entropy passphrase), so the info string alone gives this
    /// derivation the domain separation it needs from the vault's other
    /// HKDF uses ([`crate::crypto::HKDF_VAULT_INFO`],
    /// [`crate::crypto::HKDF_SYNC_INFO`], [`crate::crypto::HKDF_RECOVERY_INFO`],
    /// this module's own [`crate::crypto::HKDF_DM_IDENTITY_INFO`]).
    pub fn from_vault_key(key: &VaultKey) -> Self {
        let hk = Hkdf::<Sha256>::new(None, key);
        let mut secret_bytes = [0u8; 32];
        hk.expand(crypto::HKDF_DM_IDENTITY_INFO, &mut secret_bytes)
            .expect("HKDF expand should never fail for 32-byte output");
        let secret_bytes = clamp_integer(secret_bytes);
        let public = MontgomeryPoint::mul_base_clamped(secret_bytes).to_bytes();
        Self {
            public,
            secret: Zeroizing::new(secret_bytes),
        }
    }
}

/// Derive the symmetric key for one DM conversation.
///
/// `my_secret` is this side's [`DmIdentity::secret`]; `their_public` is the
/// other side's [`DmIdentity::public`]; `conversation_id` is a stable
/// server-assigned identifier for the conversation. Symmetric: whichever
/// side calls this with its own secret and the other's public key lands on
/// the identical key, because X25519 ECDH commutes
/// (`my_secret * their_public == their_secret * my_public`).
///
/// `ECDH(my_secret, their_public) → HKDF-SHA256(info =
/// "mylittlechart-dm-v1" || conversation_id)`. No HKDF salt (`None`) —
/// the ECDH output is already high-entropy shared secret material, and the
/// info string plus conversation id give the domain separation this needs
/// (a different conversation between the same two identities derives a
/// different key).
pub fn conversation_key(
    my_secret: &[u8; 32],
    their_public: &[u8; 32],
    conversation_id: &str,
) -> Zeroizing<[u8; 32]> {
    let shared = Zeroizing::new(MontgomeryPoint(*their_public).mul_clamped(*my_secret).to_bytes());

    let mut info = Vec::with_capacity(crypto::HKDF_DM_INFO.len() + conversation_id.len());
    info.extend_from_slice(crypto::HKDF_DM_INFO);
    info.extend_from_slice(conversation_id.as_bytes());

    let hk = Hkdf::<Sha256>::new(None, &*shared);
    let mut key = [0u8; 32];
    hk.expand(&info, &mut key)
        .expect("HKDF expand should never fail for 32-byte output");
    Zeroizing::new(key)
}

/// A sealed DM payload: a random nonce plus the AES-256-GCM ciphertext
/// (the 16-byte GCM tag is appended to `ciphertext`, per the `aes-gcm`
/// crate's convention — there is no separate tag field).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DmCiphertext {
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
}

/// Encrypt `plaintext` under a per-conversation `key` (see
/// [`conversation_key`]), binding `aad` into the GCM tag.
///
/// `aad` should be the server-visible metadata the ciphertext travels with
/// (recommended: conversation id + sender id) — [`open_message`] fails
/// unless the caller supplies the exact same `aad`, so that metadata cannot
/// be swapped onto a different message's ciphertext without detection.
///
/// Draws a fresh random nonce via [`OsRng`] on every call — nonces must
/// never repeat under the same key.
pub fn seal_message(key: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> DmCiphertext {
    let cipher = Aes256Gcm::new_from_slice(key)
        .expect("Aes256Gcm::new_from_slice cannot fail for a statically-sized 32-byte key");

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, Payload { msg: plaintext, aad })
        .expect("AES-256-GCM encryption cannot fail for a valid key and a fresh nonce");

    DmCiphertext {
        nonce: nonce_bytes,
        ciphertext,
    }
}

/// Decrypt a [`DmCiphertext`] produced by [`seal_message`].
///
/// `aad` must equal exactly what was passed to `seal_message` — a mismatch
/// (wrong conversation id, wrong sender id, tampered nonce, or tampered
/// ciphertext) fails as [`VaultError::DecryptionFailed`], never a silent
/// wrong answer.
pub fn open_message(
    key: &[u8; 32],
    sealed: &DmCiphertext,
    aad: &[u8],
) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .expect("Aes256Gcm::new_from_slice cannot fail for a statically-sized 32-byte key");
    let nonce = Nonce::from_slice(&sealed.nonce);

    let plaintext = cipher
        .decrypt(
            nonce,
            Payload {
                msg: &sealed.ciphertext,
                aad,
            },
        )
        .map_err(|_| VaultError::DecryptionFailed)?;

    Ok(Zeroizing::new(plaintext))
}

/// AAD binding for one DM message's seal/open — conversation id plus the
/// SENDING identity's own key fingerprint. Mirrors, byte for byte, the
/// `dm_aad` helper both app shells carry their own private copy of
/// (`mlc-app-shell-vello/src/main.rs`'s `dm_aad`,
/// `mlc-app-shell-web/src/cloud_session.rs`'s `dm_aad`:
/// `format!("{}|{}", conversation_id, sender_fingerprint)`) — this is the
/// ONE shared copy every later caller (`mlc-messenger`'s own legacy-DM
/// decrypt path included) should call instead of re-deriving the format
/// string a third time; the two shells keep their own copies (each is a
/// free function in a binary crate this workspace does not want a shared
/// library dependency edge into) but must never let them drift from this
/// one's exact output.
pub fn message_aad(conversation_id: i64, sender_fingerprint: &str) -> Vec<u8> {
    format!("{conversation_id}|{sender_fingerprint}").into_bytes()
}

/// Hex-encoded SHA-256 fingerprint of a DM public key — safe for the server
/// to store and list, and for a user to read aloud or compare visually with
/// a contact's, since it never reveals the private half.
pub fn fingerprint(public: &[u8; 32]) -> String {
    hex::encode(crypto::fingerprint_sha256(public))
}

/// Case-insensitive comparison of `public`'s fingerprint against `expected`
/// (e.g. one a user copied from a contact's profile).
///
/// Not constant-time: a fingerprint is a public identifier, not a secret —
/// unlike [`crate::crypto::constant_time_eq`], which guards actual bearer
/// secrets, there is nothing to protect by hiding comparison timing here.
pub fn verify_fingerprint(public: &[u8; 32], expected: &str) -> bool {
    fingerprint(public).eq_ignore_ascii_case(expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_sides_derive_the_same_conversation_key() {
        let alice = DmIdentity::generate();
        let bob = DmIdentity::generate();

        let k_alice = conversation_key(&alice.secret, &bob.public, "conv-1");
        let k_bob = conversation_key(&bob.secret, &alice.public, "conv-1");

        assert_eq!(*k_alice, *k_bob, "ECDH must commute: both sides land on the same key");
    }

    #[test]
    fn different_conversation_ids_derive_different_keys() {
        let alice = DmIdentity::generate();
        let bob = DmIdentity::generate();

        let k1 = conversation_key(&alice.secret, &bob.public, "conv-1");
        let k2 = conversation_key(&alice.secret, &bob.public, "conv-2");

        assert_ne!(*k1, *k2, "different conversation ids must derive different keys");
    }

    #[test]
    fn seal_open_round_trip() {
        let alice = DmIdentity::generate();
        let bob = DmIdentity::generate();
        let key = conversation_key(&alice.secret, &bob.public, "conv-1");
        let aad = b"conv-1|alice";

        let sealed = seal_message(&key, b"hello bob", aad);
        let opened = open_message(&key, &sealed, aad).expect("valid ciphertext and aad must open");
        assert_eq!(&*opened, b"hello bob");
    }

    #[test]
    fn tampered_ciphertext_fails_to_open() {
        let alice = DmIdentity::generate();
        let bob = DmIdentity::generate();
        let key = conversation_key(&alice.secret, &bob.public, "conv-1");
        let aad = b"conv-1|alice";

        let mut sealed = seal_message(&key, b"hello bob", aad);
        if let Some(byte) = sealed.ciphertext.last_mut() {
            *byte ^= 0xFF;
        }
        assert!(open_message(&key, &sealed, aad).is_err());
    }

    #[test]
    fn tampered_aad_fails_to_open() {
        let alice = DmIdentity::generate();
        let bob = DmIdentity::generate();
        let key = conversation_key(&alice.secret, &bob.public, "conv-1");

        let sealed = seal_message(&key, b"hello bob", b"conv-1|alice");
        assert!(open_message(&key, &sealed, b"conv-1|mallory").is_err());
    }

    #[test]
    fn from_vault_key_is_deterministic() {
        let key: VaultKey = [7u8; 32];
        let a = DmIdentity::from_vault_key(&key);
        let b = DmIdentity::from_vault_key(&key);
        assert_eq!(a.public, b.public, "same vault key must derive the same public key");
        assert_eq!(*a.secret, *b.secret, "same vault key must derive the same secret scalar");
    }

    #[test]
    fn from_vault_key_different_keys_diverge() {
        let key_a: VaultKey = [7u8; 32];
        let key_b: VaultKey = [8u8; 32];
        let a = DmIdentity::from_vault_key(&key_a);
        let b = DmIdentity::from_vault_key(&key_b);
        assert_ne!(a.public, b.public, "different vault keys must derive different public keys");
    }

    #[test]
    fn vault_derived_identities_still_agree_on_a_conversation_key() {
        let alice_vault_key: VaultKey = [1u8; 32];
        let bob_vault_key: VaultKey = [2u8; 32];
        let alice = DmIdentity::from_vault_key(&alice_vault_key);
        let bob = DmIdentity::from_vault_key(&bob_vault_key);

        let k_alice = conversation_key(&alice.secret, &bob.public, "conv-1");
        let k_bob = conversation_key(&bob.secret, &alice.public, "conv-1");

        assert_eq!(*k_alice, *k_bob, "vault-derived identities must still commute under ECDH");
    }

    #[test]
    fn message_aad_matches_the_shells_own_dm_aad_format_string() {
        // Byte-for-byte against `format!("{}|{}", conversation_id,
        // sender_fingerprint)` — both shells' own `dm_aad` copies (this
        // function's own doc) — transcribed literally here rather than
        // re-deriving the same format call, so a future edit to either
        // side has to change this literal too, not just the code that
        // produces it.
        assert_eq!(message_aad(42, "deadbeef"), b"42|deadbeef".to_vec());
        assert_eq!(message_aad(-1, ""), b"-1|".to_vec());
        assert_eq!(message_aad(0, "abc123"), b"0|abc123".to_vec());
    }

    #[test]
    fn fingerprint_round_trips_through_verify() {
        let identity = DmIdentity::generate();
        let fp = fingerprint(&identity.public);
        assert!(verify_fingerprint(&identity.public, &fp));
        assert!(verify_fingerprint(&identity.public, &fp.to_uppercase()));

        let other = DmIdentity::generate();
        assert!(!verify_fingerprint(&other.public, &fp));
    }
}
