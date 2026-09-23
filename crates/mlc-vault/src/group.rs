//! Group E2E primitives: a symmetric key per epoch, wrapped for each member
//! via ECDH + HKDF + AES-256-GCM against that member's published DM identity
//! public key (`dm::DmIdentity`) — no second identity table, matching the
//! plan's "group membership reuses `dm_identity_keys`" posture.
//!
//! Plan: `docs/mlc/plans/community-messenger-arc-2026-09-24.md` §1.2, §3.1,
//! §7.
//!
//! # Key flow
//!
//! [`GroupKey::generate`] mints a fresh random 32-byte key for a brand-new
//! group or a re-key — never derived from anything, because the whole point
//! of a re-key is that the new epoch's key is unrelated to every earlier
//! one. [`wrap_group_key`] / [`unwrap_group_key`] move that raw key between
//! members: ECDH(one side's secret, the other side's public) → HKDF-SHA256
//! (info = [`crate::crypto::HKDF_GROUP_WRAP_INFO`] || group id || epoch) →
//! the resulting 32-byte key seals/opens the raw group key via
//! [`dm::seal_message`]/[`dm::open_message`] exactly as a DM ciphertext
//! would — a wrap IS a DM-shaped seal of 32 raw bytes, nothing new at the
//! AEAD layer. [`seal_group_message`] / [`open_group_message`] then use the
//! unwrapped [`GroupKey`] itself to seal/open an actual message body, with
//! AAD binding the conversation id, the epoch, and the message id so the
//! server-visible metadata around a group message cannot be spliced onto a
//! different message's ciphertext. [`rekey`] bundles "generate a fresh
//! epoch key, wrap it for every remaining member" into one call, matching
//! the atomic re-key request body `DELETE /api/conversations/{id}/members/
//! {user_id}` (plan §1.2) needs.
//!
//! # Forward secrecy
//!
//! Adding a member never requires a re-key: [`wrap_group_key`] wraps
//! whatever epoch the caller passes, and a caller only ever passes the
//! CURRENT epoch when onboarding someone new — this module has no notion of
//! "every epoch so far" to wrap, so there is nothing that would hand a new
//! member history from before they joined. Removing a member is the
//! opposite case: [`rekey`] produces a BRAND NEW key ([`GroupKey::
//! generate`], not derived from the retiring key), wrapped only for the
//! members passed in — a member left out of that list can never unwrap the
//! new epoch, because unwrapping needs the wrapper's cooperation (their own
//! wrap, addressed to that member's public key) which the wrapper simply
//! never produces for them.
//!
//! # Edits across a re-key boundary (plan §7 / §6.2)
//!
//! The owner's ruling: an edit is sealed under the message's OWN epoch, and
//! is refused once the group has re-keyed past that epoch — editing must
//! never widen a message's audience by resealing it under a newer epoch's
//! (larger) membership. [`message_epoch_is_still_editable`] is the one
//! place that comparison lives, so every caller (shell driver, server route)
//! enforces the identical rule instead of re-deriving it.

use hkdf::Hkdf;
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::crypto;
use crate::dm::{self, DmCiphertext};
use crate::vault::VaultError;

/// A raw 32-byte symmetric key for one group epoch. [`Zeroizing`]-wrapped so
/// it self-wipes on drop — same posture as [`crate::dm::DmIdentity::secret`].
/// Does not implement `Debug`/`Clone`/`Serialize`: the only way it leaves
/// this module is wrapped ([`wrap_group_key`]) or spent directly on a
/// message ([`seal_group_message`]/[`open_group_message`]).
pub struct GroupKey(pub Zeroizing<[u8; 32]>);

impl GroupKey {
    /// Mint a fresh group key from OS entropy. No derivation — a brand-new
    /// group, or a re-key, needs a key unrelated to anything before it.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        Self(Zeroizing::new(bytes))
    }
}

/// Derive the symmetric key that wraps/unwraps a group key between two
/// identities for one `(group_id, epoch)` pair.
///
/// `ECDH(my_secret, their_public) → HKDF-SHA256(info =
/// HKDF_GROUP_WRAP_INFO || group_id || '|' || epoch)`. The `'|'` delimiter
/// between `group_id` and the decimal `epoch` prevents the ambiguity a bare
/// concatenation would have (`group_id="1"`, `epoch=23` vs. `group_id="12"`,
/// `epoch=3` would otherwise derive the same info string). Symmetric like
/// [`dm::conversation_key`]: whichever side calls this with its own secret
/// and the other's public key lands on the identical key, because X25519
/// ECDH commutes. Does the ECDH step directly (rather than calling
/// [`dm::conversation_key`]) so this module owns its own domain-separated
/// info string end to end — reusing `conversation_key`'s DM-domain info
/// string here would let a group wrap collide with a real DM conversation
/// key whenever a group id happened to equal a conversation id.
fn wrap_transport_key(my_secret: &[u8; 32], their_public: &[u8; 32], group_id: &str, epoch: i64) -> Zeroizing<[u8; 32]> {
    let shared = Zeroizing::new(
        curve25519_dalek::montgomery::MontgomeryPoint(*their_public)
            .mul_clamped(*my_secret)
            .to_bytes(),
    );

    let mut info = Vec::with_capacity(crypto::HKDF_GROUP_WRAP_INFO.len() + group_id.len() + 1 + 20);
    info.extend_from_slice(crypto::HKDF_GROUP_WRAP_INFO);
    info.extend_from_slice(group_id.as_bytes());
    info.push(b'|');
    info.extend_from_slice(epoch.to_string().as_bytes());

    let hk = Hkdf::<Sha256>::new(None, &*shared);
    let mut key = [0u8; 32];
    hk.expand(&info, &mut key)
        .expect("HKDF expand should never fail for 32-byte output");
    Zeroizing::new(key)
}

/// Wrap `key` (a group epoch's raw key) for one member.
///
/// `wrapper_secret` is the ACTOR's own [`dm::DmIdentity::secret`] (the group
/// owner/admin performing the wrap); `member_public` is the target member's
/// published [`dm::DmIdentity::public`]; `group_id` is the conversation's
/// stable server-assigned id; `epoch` is the epoch this key belongs to.
pub fn wrap_group_key(wrapper_secret: &[u8; 32], member_public: &[u8; 32], group_id: &str, epoch: i64, key: &GroupKey) -> DmCiphertext {
    let transport_key = wrap_transport_key(wrapper_secret, member_public, group_id, epoch);
    let aad = wrap_aad(group_id, epoch);
    dm::seal_message(&transport_key, &key.0[..], &aad)
}

/// Unwrap a [`DmCiphertext`] produced by [`wrap_group_key`].
///
/// `my_secret` is the member's own [`dm::DmIdentity::secret`];
/// `wrapper_public` is the wrapper's published [`dm::DmIdentity::public`] at
/// wrap time (`group_key_wraps.wrapper_public_key`, plan §1.2). Fails as
/// [`VaultError::DecryptionFailed`] if `my_secret`/`wrapper_public` do not
/// match the pair the wrap was addressed to, if `group_id`/`epoch` do not
/// match what it was wrapped under, or if the ciphertext was tampered with.
pub fn unwrap_group_key(my_secret: &[u8; 32], wrapper_public: &[u8; 32], group_id: &str, epoch: i64, wrapped: &DmCiphertext) -> Result<GroupKey, VaultError> {
    let transport_key = wrap_transport_key(my_secret, wrapper_public, group_id, epoch);
    let aad = wrap_aad(group_id, epoch);
    let raw = dm::open_message(&transport_key, wrapped, &aad)?;
    let bytes: [u8; 32] = raw.as_slice().try_into().map_err(|_| VaultError::DecryptionFailed)?;
    Ok(GroupKey(Zeroizing::new(bytes)))
}

/// AAD for a key-wrap seal: binds the wrap to its `(group_id, epoch)` so a
/// wrap cannot be replayed as if it belonged to a different group or a
/// different epoch than the one it was actually issued for.
fn wrap_aad(group_id: &str, epoch: i64) -> Vec<u8> {
    let mut aad = Vec::with_capacity(group_id.len() + 1 + 20);
    aad.extend_from_slice(group_id.as_bytes());
    aad.push(b'|');
    aad.extend_from_slice(epoch.to_string().as_bytes());
    aad
}

/// AAD for a group message seal: binds the ciphertext to its conversation
/// id, epoch, and message id, so the server-visible metadata a message
/// travels with (which conversation, which epoch sealed it, which row it
/// is) cannot be spliced onto a different message's ciphertext without the
/// GCM tag failing to verify on open — same posture `dm::seal_message`'s
/// own `aad` documents for DMs, extended with the epoch and message id a
/// group message additionally needs to stay bound to.
fn message_aad(conversation_id: &str, epoch: i64, message_id: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(conversation_id.len() + 1 + 20 + 1 + message_id.len());
    aad.extend_from_slice(conversation_id.as_bytes());
    aad.push(b'|');
    aad.extend_from_slice(epoch.to_string().as_bytes());
    aad.push(b'|');
    aad.extend_from_slice(message_id.as_bytes());
    aad
}

/// Seal a group message body under `key`, bound to `conversation_id`,
/// `epoch`, and `message_id` via [`message_aad`].
pub fn seal_group_message(key: &GroupKey, conversation_id: &str, epoch: i64, message_id: &str, plaintext: &[u8]) -> DmCiphertext {
    let aad = message_aad(conversation_id, epoch, message_id);
    dm::seal_message(&key.0, plaintext, &aad)
}

/// Open a group message body sealed by [`seal_group_message`]. Fails as
/// [`VaultError::DecryptionFailed`] unless `conversation_id`/`epoch`/
/// `message_id` are exactly what it was sealed with, or `key` is not the
/// epoch's key.
pub fn open_group_message(key: &GroupKey, conversation_id: &str, epoch: i64, message_id: &str, sealed: &DmCiphertext) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let aad = message_aad(conversation_id, epoch, message_id);
    dm::open_message(&key.0, sealed, &aad)
}

/// One remaining member's identity for a re-key: their user id (matching
/// the server's `conversation_members.user_id` / `group_key_wraps.
/// member_user_id` columns, plan §1.2) and their currently-published DM
/// public key (fetched in one batch via `GET .../member-keys`).
#[derive(Debug, Clone, Copy)]
pub struct RemainingMember {
    pub user_id: i64,
    pub public_key: [u8; 32],
}

/// One member's wrap of a re-key's new epoch key, addressed by user id —
/// ready to serialize into the `wraps` array of the `DELETE /api/
/// conversations/{id}/members/{user_id}` re-key request body (plan §1.2).
pub struct MemberWrap {
    pub user_id: i64,
    pub wrapped: DmCiphertext,
}

/// The outcome of a re-key: the new epoch number, the fresh group key
/// itself (so the actor's own local cache can be updated immediately,
/// without waiting on a round trip), and one wrap per remaining member.
pub struct RekeyResult {
    pub epoch: i64,
    pub key: GroupKey,
    pub wraps: Vec<MemberWrap>,
}

/// Compute a full re-key: generate a fresh key for `new_epoch` and wrap it
/// for every member in `remaining_members`, under the ACTOR's own
/// `wrapper_secret` ([`dm::DmIdentity::secret`]) — the actor removing a
/// member must already be a current member themselves (plan §1.2: "the
/// ACTOR must already hold the current group key locally"), which is a
/// precondition this function does not itself check (it has no way to know
/// membership; the caller's HTTP route does).
///
/// The result's `wraps` covers exactly `remaining_members` — the removed
/// member is simply never passed in, so they never receive a wrap for
/// `new_epoch` and cannot unwrap it (see this module's doc, "Forward
/// secrecy").
pub fn rekey(wrapper_secret: &[u8; 32], group_id: &str, new_epoch: i64, remaining_members: &[RemainingMember]) -> RekeyResult {
    let key = GroupKey::generate();
    let wraps = remaining_members
        .iter()
        .map(|member| MemberWrap {
            user_id: member.user_id,
            wrapped: wrap_group_key(wrapper_secret, &member.public_key, group_id, new_epoch, &key),
        })
        .collect();
    RekeyResult { epoch: new_epoch, key, wraps }
}

/// Whether a message sealed under `message_epoch` may still be edited, given
/// the group's current epoch is `current_epoch`.
///
/// Owner ruling (plan §7 / §6.2): an edit is sealed under the message's OWN
/// epoch, and is refused once the group has re-keyed past that epoch —
/// resealing under a newer epoch would widen the message's audience to
/// whoever holds the CURRENT epoch key, including someone who joined after
/// the original send but before the edit. Equivalent to `message_epoch ==
/// current_epoch`, named so every caller (shell driver, server route) shares
/// the identical rule instead of re-deriving the comparison independently.
pub fn message_epoch_is_still_editable(message_epoch: i64, current_epoch: i64) -> bool {
    message_epoch == current_epoch
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dm::DmIdentity;

    #[test]
    fn wrap_unwrap_round_trip() {
        let wrapper = DmIdentity::generate();
        let member = DmIdentity::generate();
        let key = GroupKey::generate();

        let wrapped = wrap_group_key(&wrapper.secret, &member.public, "conv-1", 1, &key);
        let unwrapped = unwrap_group_key(&member.secret, &wrapper.public, "conv-1", 1, &wrapped)
            .expect("member must unwrap a wrap addressed to them");

        assert_eq!(*unwrapped.0, *key.0, "unwrapped key must equal the original");
    }

    #[test]
    fn wrong_member_cannot_unwrap() {
        let wrapper = DmIdentity::generate();
        let member = DmIdentity::generate();
        let mallory = DmIdentity::generate();
        let key = GroupKey::generate();

        let wrapped = wrap_group_key(&wrapper.secret, &member.public, "conv-1", 1, &key);

        // Mallory's own secret against the wrapper's public key derives an
        // unrelated transport key — decryption must fail, not silently
        // succeed with wrong bytes.
        assert!(unwrap_group_key(&mallory.secret, &wrapper.public, "conv-1", 1, &wrapped).is_err());
    }

    #[test]
    fn unwrap_fails_with_wrong_epoch() {
        let wrapper = DmIdentity::generate();
        let member = DmIdentity::generate();
        let key = GroupKey::generate();

        let wrapped = wrap_group_key(&wrapper.secret, &member.public, "conv-1", 1, &key);
        assert!(unwrap_group_key(&member.secret, &wrapper.public, "conv-1", 2, &wrapped).is_err());
    }

    #[test]
    fn unwrap_fails_with_wrong_group_id() {
        let wrapper = DmIdentity::generate();
        let member = DmIdentity::generate();
        let key = GroupKey::generate();

        let wrapped = wrap_group_key(&wrapper.secret, &member.public, "conv-1", 1, &key);
        assert!(unwrap_group_key(&member.secret, &wrapper.public, "conv-2", 1, &wrapped).is_err());
    }

    #[test]
    fn seal_open_group_message_round_trip() {
        let key = GroupKey::generate();
        let sealed = seal_group_message(&key, "conv-1", 3, "msg-42", b"hello group");
        let opened = open_group_message(&key, "conv-1", 3, "msg-42", &sealed)
            .expect("valid ciphertext and aad must open");
        assert_eq!(&*opened, b"hello group");
    }

    #[test]
    fn tampered_aad_fails_group_message_open() {
        let key = GroupKey::generate();
        let sealed = seal_group_message(&key, "conv-1", 3, "msg-42", b"hello group");

        assert!(open_group_message(&key, "conv-1", 3, "msg-43", &sealed).is_err(), "wrong message id must fail");
        assert!(open_group_message(&key, "conv-1", 4, "msg-42", &sealed).is_err(), "wrong epoch must fail");
        assert!(open_group_message(&key, "conv-2", 3, "msg-42", &sealed).is_err(), "wrong conversation id must fail");
    }

    #[test]
    fn tampered_ciphertext_fails_group_message_open() {
        let key = GroupKey::generate();
        let mut sealed = seal_group_message(&key, "conv-1", 3, "msg-42", b"hello group");
        if let Some(byte) = sealed.ciphertext.last_mut() {
            *byte ^= 0xFF;
        }
        assert!(open_group_message(&key, "conv-1", 3, "msg-42", &sealed).is_err());
    }

    #[test]
    fn removed_member_cannot_open_a_post_rekey_message() {
        let owner = DmIdentity::generate();
        let removed = DmIdentity::generate();
        let stays = DmIdentity::generate();

        // Epoch 1: both `removed` and `stays` hold the key.
        let epoch1_key = GroupKey::generate();
        let removed_wrap = wrap_group_key(&owner.secret, &removed.public, "conv-1", 1, &epoch1_key);
        let removed_unwrapped = unwrap_group_key(&removed.secret, &owner.public, "conv-1", 1, &removed_wrap)
            .expect("removed member could open epoch 1 before removal");
        assert_eq!(*removed_unwrapped.0, *epoch1_key.0);

        // `owner` removes `removed`: re-key to epoch 2, wrapped only for `stays`.
        let result = rekey(
            &owner.secret,
            "conv-1",
            2,
            &[RemainingMember { user_id: 2, public_key: stays.public }],
        );
        assert_eq!(result.epoch, 2);
        assert_eq!(result.wraps.len(), 1);

        // `stays` can unwrap epoch 2 and open a message sealed under it.
        let stays_wrap = &result.wraps[0];
        let stays_unwrapped = unwrap_group_key(&stays.secret, &owner.public, "conv-1", 2, &stays_wrap.wrapped)
            .expect("remaining member must unwrap the new epoch");

        let post_rekey_message = seal_group_message(&stays_unwrapped, "conv-1", 2, "msg-99", b"removed can't read this");
        let opened = open_group_message(&stays_unwrapped, "conv-1", 2, "msg-99", &post_rekey_message)
            .expect("remaining member opens the post-rekey message with the new epoch key");
        assert_eq!(&*opened, b"removed can't read this");

        // `removed` never received a wrap for epoch 2 at all — there is
        // nothing to unwrap. Confirm the only key they hold (epoch 1) does
        // not open the post-rekey message either.
        assert!(
            open_group_message(&removed_unwrapped, "conv-1", 2, "msg-99", &post_rekey_message).is_err(),
            "the removed member's old epoch-1 key must not open an epoch-2 message"
        );
    }

    #[test]
    fn rekey_produces_exactly_one_wrap_per_remaining_member() {
        let owner = DmIdentity::generate();
        let alice = DmIdentity::generate();
        let bob = DmIdentity::generate();

        let result = rekey(
            &owner.secret,
            "conv-1",
            5,
            &[
                RemainingMember { user_id: 1, public_key: alice.public },
                RemainingMember { user_id: 2, public_key: bob.public },
            ],
        );

        assert_eq!(result.wraps.len(), 2);
        assert_eq!(result.wraps[0].user_id, 1);
        assert_eq!(result.wraps[1].user_id, 2);

        for (member, wrap) in [(&alice, &result.wraps[0]), (&bob, &result.wraps[1])] {
            let unwrapped = unwrap_group_key(&member.secret, &owner.public, "conv-1", 5, &wrap.wrapped)
                .expect("each remaining member must unwrap their own wrap");
            assert_eq!(*unwrapped.0, *result.key.0);
        }
    }

    #[test]
    fn message_epoch_editability_matches_the_owner_ruling() {
        assert!(message_epoch_is_still_editable(3, 3), "same epoch must remain editable");
        assert!(!message_epoch_is_still_editable(3, 4), "a superseded epoch must be refused");
        assert!(!message_epoch_is_still_editable(4, 3), "an epoch ahead of current must never happen but must not be treated as editable");
    }
}
