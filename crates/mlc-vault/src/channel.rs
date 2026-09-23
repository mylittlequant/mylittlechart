//! Channel post signing: a per-channel Ed25519 identity derived
//! deterministically from the vault key, so a channel owner's signing key
//! exists the moment their vault does — same posture as
//! [`crate::dm::DmIdentity::from_vault_key`] (owner ruling 2026-09-09: never
//! a separate secret to generate, persist, or publish by hand) — with a
//! public channel's posts verifiable by any reader who never touches the
//! vault at all ([`verify_post`] takes only the public key and the post
//! body).
//!
//! Plan: `docs/mlc/plans/community-messenger-arc-2026-09-24.md` §1.3, §3.1.
//!
//! # Key flow
//!
//! [`ChannelSigningIdentity::from_vault_key`] derives an Ed25519 keypair via
//! `HKDF-SHA256(ikm = vault key, salt = None, info =
//! [`crate::crypto::HKDF_CHANNEL_SIGNING_INFO`] || channel_id)` — the channel
//! id is folded into the info string (this module's own deviation from the
//! plan's illustrative sketch, which omitted a per-channel id: §1.3's own
//! schema gives every channel exactly one signing key per `user_id`, but a
//! caller who owns more than one channel still needs those keys to be
//! unrelated to each other, so this module derives per `(vault key,
//! channel_id)` rather than per vault key alone). No HKDF salt, same
//! reasoning [`crate::dm::DmIdentity::from_vault_key`] documents: the vault
//! key is already uniformly random, so the info string alone gives this
//! derivation the domain separation it needs from the vault's other HKDF
//! uses. [`ChannelSigningIdentity::sign_post`] is deterministic EdDSA (no
//! RNG involved) over the post body's raw bytes; [`verify_post`] is the
//! standalone, vault-free counterpart any reader of a public channel can
//! call.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use sha2::Sha256;

use crate::crypto;
use crate::vault::VaultKey;

/// An Ed25519 signing identity for one channel, deterministically derived
/// from the vault key plus the channel's own id.
///
/// Does not implement `Debug`/`Clone`/`Serialize` — like [`crate::dm::
/// DmIdentity`], it is never persisted; a caller derives it fresh from the
/// vault key whenever it needs to sign (on vault unlock, at boot if already
/// unlocked) and drops it when the vault seals.
pub struct ChannelSigningIdentity {
    /// The public Ed25519 verifying key, safe to publish — `PUT /api/
    /// conversations/signing-key` (plan §1.3).
    pub public: [u8; 32],
    secret: SigningKey,
}

impl ChannelSigningIdentity {
    /// Derive this channel's signing keypair from the already-unlocked
    /// [`VaultKey`] and the channel's stable server-assigned id — see this
    /// module's doc for the exact HKDF derivation and why the channel id is
    /// folded in.
    pub fn from_vault_key(key: &VaultKey, channel_id: &str) -> Self {
        let hk = Hkdf::<Sha256>::new(None, key);
        let mut info = Vec::with_capacity(crypto::HKDF_CHANNEL_SIGNING_INFO.len() + channel_id.len());
        info.extend_from_slice(crypto::HKDF_CHANNEL_SIGNING_INFO);
        info.extend_from_slice(channel_id.as_bytes());

        let mut secret_bytes = [0u8; 32];
        hk.expand(&info, &mut secret_bytes)
            .expect("HKDF expand should never fail for 32-byte output");
        let secret = SigningKey::from_bytes(&secret_bytes);
        let public = secret.verifying_key().to_bytes();

        Self { public, secret }
    }

    /// Sign `body` — the channel post's plaintext bytes (the
    /// `MessagePayload` JSON, plan §1.1) — with deterministic EdDSA.
    pub fn sign_post(&self, body: &[u8]) -> [u8; 64] {
        self.secret.sign(body).to_bytes()
    }
}

/// Verify a channel post's signature against its author's published public
/// key. Needs only `public` — a public channel's posts are verifiable by
/// anyone, without ever touching (or even knowing about) the author's vault,
/// matching the plan's "the server verifies nothing itself; verification is
/// a client-side trust decision" posture for `visibility = 'public'`
/// channels (§1.3).
///
/// `false` for a malformed `public` (not a valid Ed25519 point) as well as
/// for an actual signature mismatch — both are "do not trust this post",
/// not two cases a caller needs to distinguish.
pub fn verify_post(public: &[u8; 32], body: &[u8], sig: &[u8; 64]) -> bool {
    let Ok(verifying_key) = VerifyingKey::from_bytes(public) else {
        return false;
    };
    let signature = Signature::from_bytes(sig);
    verifying_key.verify_strict(body, &signature).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::VaultKey;

    #[test]
    fn sign_verify_round_trip() {
        let key: VaultKey = [9u8; 32];
        let identity = ChannelSigningIdentity::from_vault_key(&key, "chan-1");

        let sig = identity.sign_post(b"hello channel");
        assert!(verify_post(&identity.public, b"hello channel", &sig));
    }

    #[test]
    fn tampered_body_fails_verify() {
        let key: VaultKey = [9u8; 32];
        let identity = ChannelSigningIdentity::from_vault_key(&key, "chan-1");

        let sig = identity.sign_post(b"hello channel");
        assert!(!verify_post(&identity.public, b"hello CHANNEL", &sig));
    }

    #[test]
    fn tampered_signature_fails_verify() {
        let key: VaultKey = [9u8; 32];
        let identity = ChannelSigningIdentity::from_vault_key(&key, "chan-1");

        let mut sig = identity.sign_post(b"hello channel");
        sig[0] ^= 0xFF;
        assert!(!verify_post(&identity.public, b"hello channel", &sig));
    }

    #[test]
    fn wrong_public_key_fails_verify() {
        let key: VaultKey = [9u8; 32];
        let identity_a = ChannelSigningIdentity::from_vault_key(&key, "chan-1");
        let identity_b = ChannelSigningIdentity::from_vault_key(&key, "chan-2");

        let sig = identity_a.sign_post(b"hello channel");
        assert!(!verify_post(&identity_b.public, b"hello channel", &sig));
    }

    #[test]
    fn from_vault_key_is_deterministic() {
        let key: VaultKey = [9u8; 32];
        let a = ChannelSigningIdentity::from_vault_key(&key, "chan-1");
        let b = ChannelSigningIdentity::from_vault_key(&key, "chan-1");
        assert_eq!(a.public, b.public, "same vault key + channel id must derive the same public key");
    }

    #[test]
    fn different_channel_ids_diverge() {
        let key: VaultKey = [9u8; 32];
        let a = ChannelSigningIdentity::from_vault_key(&key, "chan-1");
        let b = ChannelSigningIdentity::from_vault_key(&key, "chan-2");
        assert_ne!(a.public, b.public, "different channel ids under the same vault key must derive different keys");
    }

    #[test]
    fn different_vault_keys_diverge() {
        let key_a: VaultKey = [9u8; 32];
        let key_b: VaultKey = [10u8; 32];
        let a = ChannelSigningIdentity::from_vault_key(&key_a, "chan-1");
        let b = ChannelSigningIdentity::from_vault_key(&key_b, "chan-1");
        assert_ne!(a.public, b.public, "different vault keys must derive different public keys");
    }
}
