//! Legacy DM decrypt (M11): resolves `org.mylittlechart.legacy_dm` timeline
//! items into plaintext using this account's vault-derived
//! [`mlc_vault::dm::DmIdentity`] and the room's own
//! `org.mylittlechart.legacy_dm_key` state
//! ([`crate::room::state::RoomState::legacy_dm_keys`]) -- a pre-Matrix
//! shell decrypted these same messages under the exact same scheme
//! (`mlc_vault::dm`'s own module doc), so this module reproduces that
//! decrypt path exactly rather than re-deriving it, and migrated history
//! reads identically through this crate's own timeline.
//!
//! # AAD binding
//!
//! [`mlc_vault::dm::message_aad`] binds conversation id plus the SENDING
//! identity's own key fingerprint -- the one shared copy of the shells'
//! own `dm_aad` helper (`mlc-app-shell-vello/src/main.rs`,
//! `mlc-app-shell-web/src/cloud_session.rs`), so a message either shell
//! sealed before migration opens identically here: `sender_public` below
//! is the SENDER's own legacy key (this account's, or the peer's),
//! whichever side actually sent it.
//!
//! # Retry
//!
//! [`LegacyDmDecryptReason::MissingPeerKey`]/`LegacyKeyMismatch` are
//! retryable once the room's own `legacy_dm_key` state changes
//! (`crate::core::MessengerCore`'s own retry hook, triggered whenever
//! [`crate::room::state::RoomState::legacy_dm_keys`] changes shape).
//! [`LegacyDmDecryptReason::NoLegacyIdentity`] is not retryable within the
//! same [`crate::core::MessengerCore`] session: the identity a core was
//! [`crate::core::MessengerCore::open`]ed with never changes for that
//! core's own lifetime, and this crate's "timeline content is never
//! persisted" doctrine (`crate::core`'s own module doc) means every
//! `LegacyDm` item is re-decrypted fresh on every open anyway -- a shell
//! that derives the identity later simply reopens the core.

use crate::ids::UserId;
use crate::room::state::RoomState;
use mlc_vault::dm::{conversation_key, fingerprint, message_aad, open_message, DmCiphertext, DmIdentity};

/// Why a `LegacyDm` timeline item could not be decrypted (yet). See this
/// module's own "Retry" doc for which of these a later `legacy_dm_key`
/// state change can resolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LegacyDmDecryptReason {
    /// This core was opened with no `legacy_dm_identity` at all (e.g. the
    /// vault was sealed) -- permanent for this core's own session (this
    /// module's own doc).
    NoLegacyIdentity,
    /// The room holds no `org.mylittlechart.legacy_dm_key` state for this
    /// account, for the peer, or for both.
    MissingPeerKey,
    /// This account's own `legacy_dm_key` state entry does not match the
    /// vault-derived identity's public key.
    LegacyKeyMismatch,
    /// [`open_message`] failed -- wrong key, tampered ciphertext/AAD, or
    /// malformed base64.
    DecryptFailed,
}

/// One legacy DM item's decrypt outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LegacyDmOutcome {
    /// Decrypted -- the message's plain-text body (`String::from_utf8_lossy`,
    /// mirroring the pre-migration shells' own `DmMessageBody::Plain`
    /// conversion exactly -- this module's own doc).
    Plain(String),
    /// Could not be decrypted (yet).
    Undecryptable(LegacyDmDecryptReason),
}

fn decode_public_key(public_key_b64: &str) -> Option<[u8; 32]> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(public_key_b64).ok()?.try_into().ok()
}

fn decode_nonce(nonce_b64: &str) -> Option<[u8; 12]> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(nonce_b64).ok()?.try_into().ok()
}

fn decode_ciphertext(ciphertext_b64: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(ciphertext_b64).ok()
}

/// Resolves `room`'s own [`RoomState::legacy_dm_keys`] into `(my_public,
/// peer_public)`, or the [`LegacyDmDecryptReason`] naming why it could not
/// -- shared by [`decrypt_legacy_dm_item`]'s own missing-key and mismatch
/// checks. `peer` is "the other member" (this module's own doc): whichever
/// entry in `legacy_dm_keys` is not `my_user_id`'s own.
fn resolve_keys(room: &RoomState, my_user_id: &UserId) -> Result<([u8; 32], [u8; 32]), LegacyDmDecryptReason> {
    let my_entry = room.legacy_dm_keys.get(my_user_id).ok_or(LegacyDmDecryptReason::MissingPeerKey)?;
    let my_public = decode_public_key(&my_entry.public_key_b64).ok_or(LegacyDmDecryptReason::DecryptFailed)?;
    let (_, peer_entry) = room
        .legacy_dm_keys
        .iter()
        .find(|(user_id, _)| *user_id != my_user_id)
        .ok_or(LegacyDmDecryptReason::MissingPeerKey)?;
    let peer_public = decode_public_key(&peer_entry.public_key_b64).ok_or(LegacyDmDecryptReason::DecryptFailed)?;
    Ok((my_public, peer_public))
}

/// Decrypts one `org.mylittlechart.legacy_dm` timeline item's ciphertext --
/// see this module's own doc. `identity` is `None` exactly when this core
/// was opened with no [`crate::core::CoreSecrets::legacy_dm_identity`].
/// `sender` decides which side's key fingerprint binds the AAD (this
/// module's own "AAD binding" doc).
pub(crate) fn decrypt_legacy_dm_item(
    identity: Option<&DmIdentity>,
    room: &RoomState,
    my_user_id: &UserId,
    sender: &UserId,
    legacy_conversation_id: i64,
    nonce_b64: &str,
    ciphertext_b64: &str,
) -> LegacyDmOutcome {
    let Some(identity) = identity else {
        return LegacyDmOutcome::Undecryptable(LegacyDmDecryptReason::NoLegacyIdentity);
    };
    let (my_public, peer_public) = match resolve_keys(room, my_user_id) {
        Ok(keys) => keys,
        Err(reason) => return LegacyDmOutcome::Undecryptable(reason),
    };
    if my_public != identity.public {
        return LegacyDmOutcome::Undecryptable(LegacyDmDecryptReason::LegacyKeyMismatch);
    }
    let Some(nonce) = decode_nonce(nonce_b64) else {
        return LegacyDmOutcome::Undecryptable(LegacyDmDecryptReason::DecryptFailed);
    };
    let Some(ciphertext) = decode_ciphertext(ciphertext_b64) else {
        return LegacyDmOutcome::Undecryptable(LegacyDmDecryptReason::DecryptFailed);
    };

    let sender_public = if sender == my_user_id { identity.public } else { peer_public };
    let key = conversation_key(&identity.secret, &peer_public, &legacy_conversation_id.to_string());
    let aad = message_aad(legacy_conversation_id, &fingerprint(&sender_public));
    let sealed = DmCiphertext { nonce, ciphertext };
    match open_message(&key, &sealed, &aad) {
        Ok(plaintext) => LegacyDmOutcome::Plain(String::from_utf8_lossy(&plaintext).into_owned()),
        Err(_) => LegacyDmOutcome::Undecryptable(LegacyDmDecryptReason::DecryptFailed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::events::LegacyDmKeyContent;
    use mlc_vault::dm::seal_message;
    use std::collections::BTreeMap;

    fn seal_b64(key: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> (String, String) {
        use base64::Engine;
        let sealed = seal_message(key, plaintext, aad);
        (
            base64::engine::general_purpose::STANDARD.encode(sealed.nonce),
            base64::engine::general_purpose::STANDARD.encode(sealed.ciphertext),
        )
    }

    fn room_with_keys(entries: &[(&UserId, [u8; 32], i64)]) -> RoomState {
        use base64::Engine;
        let mut room = RoomState::new();
        let mut legacy_dm_keys = BTreeMap::new();
        for (user_id, public, conversation_id) in entries {
            legacy_dm_keys.insert(
                (*user_id).clone(),
                LegacyDmKeyContent {
                    public_key_b64: base64::engine::general_purpose::STANDARD.encode(public),
                    legacy_conversation_id: *conversation_id,
                },
            );
        }
        room.legacy_dm_keys = legacy_dm_keys;
        room
    }

    #[test]
    fn legacy_dm_decrypts_with_the_vault_identity_and_peer_key() {
        let me = UserId::parse("@me:example.org").expect("valid user id");
        let peer = UserId::parse("@peer:example.org").expect("valid user id");
        let identity = DmIdentity::from_vault_key(&[1u8; 32]);
        let peer_identity = DmIdentity::generate();
        let conversation_id = 42;

        let key = conversation_key(&identity.secret, &peer_identity.public, &conversation_id.to_string());
        let aad = message_aad(conversation_id, &fingerprint(&peer_identity.public));
        let (nonce_b64, ciphertext_b64) = seal_b64(&key, b"hello from before matrix", &aad);

        let room = room_with_keys(&[(&me, identity.public, conversation_id), (&peer, peer_identity.public, conversation_id)]);

        let outcome = decrypt_legacy_dm_item(Some(&identity), &room, &me, &peer, conversation_id, &nonce_b64, &ciphertext_b64);
        assert_eq!(outcome, LegacyDmOutcome::Plain("hello from before matrix".to_string()));
    }

    #[test]
    fn legacy_dm_sent_by_self_decrypts_with_own_fingerprint_in_aad() {
        let me = UserId::parse("@me:example.org").expect("valid user id");
        let peer = UserId::parse("@peer:example.org").expect("valid user id");
        let identity = DmIdentity::from_vault_key(&[2u8; 32]);
        let peer_identity = DmIdentity::generate();
        let conversation_id = 7;

        let key = conversation_key(&identity.secret, &peer_identity.public, &conversation_id.to_string());
        let aad = message_aad(conversation_id, &fingerprint(&identity.public));
        let (nonce_b64, ciphertext_b64) = seal_b64(&key, b"my own message", &aad);

        let room = room_with_keys(&[(&me, identity.public, conversation_id), (&peer, peer_identity.public, conversation_id)]);

        let outcome = decrypt_legacy_dm_item(Some(&identity), &room, &me, &me, conversation_id, &nonce_b64, &ciphertext_b64);
        assert_eq!(outcome, LegacyDmOutcome::Plain("my own message".to_string()));
    }

    #[test]
    fn missing_peer_key_leaves_item_undecrypted_with_reason() {
        let me = UserId::parse("@me:example.org").expect("valid user id");
        let peer = UserId::parse("@peer:example.org").expect("valid user id");
        let identity = DmIdentity::from_vault_key(&[3u8; 32]);
        let room = room_with_keys(&[(&me, identity.public, 1)]); // no peer entry yet

        let outcome = decrypt_legacy_dm_item(Some(&identity), &room, &me, &peer, 1, "AAAAAAAAAAAAAAAAAAAA", "AAAA");
        assert_eq!(outcome, LegacyDmOutcome::Undecryptable(LegacyDmDecryptReason::MissingPeerKey));
    }

    #[test]
    fn wrong_identity_reports_key_mismatch() {
        let me = UserId::parse("@me:example.org").expect("valid user id");
        let peer = UserId::parse("@peer:example.org").expect("valid user id");
        let stale_public = DmIdentity::generate().public; // room state names a DIFFERENT key than ours
        let identity = DmIdentity::from_vault_key(&[4u8; 32]);
        let peer_identity = DmIdentity::generate();
        let room = room_with_keys(&[(&me, stale_public, 1), (&peer, peer_identity.public, 1)]);

        let outcome = decrypt_legacy_dm_item(Some(&identity), &room, &me, &peer, 1, "AAAAAAAAAAAAAAAAAAAA", "AAAA");
        assert_eq!(outcome, LegacyDmOutcome::Undecryptable(LegacyDmDecryptReason::LegacyKeyMismatch));
    }

    #[test]
    fn late_key_state_retries_decryption() {
        let me = UserId::parse("@me:example.org").expect("valid user id");
        let peer = UserId::parse("@peer:example.org").expect("valid user id");
        let identity = DmIdentity::from_vault_key(&[5u8; 32]);
        let peer_identity = DmIdentity::generate();
        let conversation_id = 9;

        let key = conversation_key(&identity.secret, &peer_identity.public, &conversation_id.to_string());
        let aad = message_aad(conversation_id, &fingerprint(&peer_identity.public));
        let (nonce_b64, ciphertext_b64) = seal_b64(&key, b"arrived before the peer key backfilled", &aad);

        // First attempt: only this account's own key has arrived so far.
        let room_before = room_with_keys(&[(&me, identity.public, conversation_id)]);
        let first = decrypt_legacy_dm_item(Some(&identity), &room_before, &me, &peer, conversation_id, &nonce_b64, &ciphertext_b64);
        assert_eq!(first, LegacyDmOutcome::Undecryptable(LegacyDmDecryptReason::MissingPeerKey));

        // The peer's own `legacy_dm_key` state event lands later -- a
        // retry with the now-updated room state succeeds, without
        // re-fetching anything about the message itself.
        let room_after =
            room_with_keys(&[(&me, identity.public, conversation_id), (&peer, peer_identity.public, conversation_id)]);
        let retried = decrypt_legacy_dm_item(Some(&identity), &room_after, &me, &peer, conversation_id, &nonce_b64, &ciphertext_b64);
        assert_eq!(retried, LegacyDmOutcome::Plain("arrived before the peer key backfilled".to_string()));
    }

    #[test]
    fn no_identity_reports_no_legacy_identity() {
        let me = UserId::parse("@me:example.org").expect("valid user id");
        let peer = UserId::parse("@peer:example.org").expect("valid user id");
        let room = room_with_keys(&[]);
        let outcome = decrypt_legacy_dm_item(None, &room, &me, &peer, 1, "AAAAAAAAAAAAAAAAAAAA", "AAAA");
        assert_eq!(outcome, LegacyDmOutcome::Undecryptable(LegacyDmDecryptReason::NoLegacyIdentity));
    }
}
