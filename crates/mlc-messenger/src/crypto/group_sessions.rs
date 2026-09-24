//! [`GroupSessionManager`] -- outbound/inbound Megolm group sessions:
//! lazy creation, rotation, room-key sharing, and decrypt-with-replay-
//! protection (plan §3 `crypto/group_sessions.rs`; research doc §3.1
//! Megolm, §3.3 to-device sharing + its security note, §4.5's UTD
//! taxonomy).
//!
//! # Recipients are the caller's job, rotation is this module's
//!
//! This module never reads [`crate::room::state::RoomState`] or
//! [`crate::crypto::device_tracker::DeviceTracker`] directly (keeps
//! `crypto/` decoupled from `room/`, matching this crate's existing module
//! boundaries). Every entry point that needs to know "who currently gets
//! this room's key" takes two plain caller-supplied inputs instead:
//!
//! - `member_user_ids`: every user whose membership is currently `join` or
//!   `invite` (the caller's own `RoomState::members` filtered accordingly --
//!   task brief's recipient rule: our rooms use `history_visibility:
//!   shared`, so an invitee may read what is sent while they are invited).
//! - `known_devices`: the FULL set of devices this account currently has
//!   on file (via [`crate::crypto::device_tracker::DeviceTracker`]) for
//!   every user who is either a current member or was previously shared
//!   with -- **not** pre-filtered to "current members only". This module
//!   itself filters out blocked devices and non-members to compute the
//!   actual recipient set; a caller passing an incomplete `known_devices`
//!   list will see false "device disappeared" rotations, since this
//!   module can only tell "gone" apart from "just not queried yet" by
//!   what it is given.
//!
//! # Outbound rotation triggers (task brief, binding)
//!
//! [`GroupSessionManager::ensure_outbound_session`] creates a fresh
//! session (new `session_id`, `shared_with` reset to empty) when any of:
//! no session exists yet; [`RoomEncryptionContent::rotation_period_msgs`]
//! messages have been encrypted; [`RoomEncryptionContent::rotation_period_ms`]
//! has elapsed since creation (`now_ms` is caller-supplied -- this crate
//! never calls `std::time` itself, plan §6.7); a previously-shared user is
//! no longer in `member_user_ids`; or a previously-shared `(user, device)`
//! pair is no longer present anywhere in `known_devices` (the device
//! disappeared from the tracked list entirely -- deleted/logged out, not
//! merely blocked). A **new** device of an already-shared member, or a
//! newly joined/invited member, never forces rotation -- they simply
//! become new entries in `shared_with`, sharing the *current* session at
//! its *current* ratchet position (they cannot read earlier messages;
//! this is what [`GroupSession::session_key`] naturally exports).
//!
//! # Sharing
//!
//! A room key is shared over Olm to-device, batched into `sendToDevice`
//! request bodies of at most [`MAX_DEVICES_PER_SEND_TO_DEVICE_REQUEST`]
//! devices each (research doc §3.3's fan-out note) --
//! [`GroupSessionManager::chunk_recipients_for_send_to_device`] plus
//! [`GroupSessionManager::build_room_key_send_to_device_body`] do this;
//! every recipient must already have an Olm session established (a
//! caller's own `/keys/claim` round trip via
//! [`crate::crypto::olm_sessions::OlmSessionManager`] first, same as any
//! other to-device send). The outbound session record (including its
//! updated `shared_with` set) is persisted through
//! [`crate::store::CryptoStore::save_outbound_group_session`] before
//! [`GroupSessionManager::ensure_outbound_session`] returns the
//! [`RoomKeyContent`] to share -- and the actual room message is only ever
//! produced by a *separate*, later call to
//! [`GroupSessionManager::encrypt_event`], so a caller naturally cannot
//! encrypt a message with a session whose sharing state has not already
//! been persisted.
//!
//! # Inbound: bound identity, merge-not-overwrite, replay protection
//!
//! [`GroupSessionManager::receive_room_key`] only ever takes an
//! Olm-verified `(sender_user_id, sender_curve25519, sender_ed25519)`
//! triple as input -- there is no entry point that accepts a bare
//! [`crate::wire::events::RawEvent`], so an `m.room_key`-shaped ROOM
//! event (which could never have been Olm-decrypted) has no path into
//! this module at all (research doc §3.3's security note: room keys are
//! never trusted from anywhere but an Olm-decrypted to-device event).
//! [`GroupSessionManager::accept_room_key_from_to_device`] is the
//! convenience wrapper over a
//! [`crate::crypto::olm_sessions::DecryptedToDevice`] (the type only
//! [`crate::crypto::olm_sessions::OlmSessionManager::decrypt_to_device`]
//! can construct) that most callers actually use.
//!
//! A second `m.room_key` for a session id this device already knows never
//! regresses it to a worse (higher first-known-index) state --
//! [`InboundGroupSession::compare`]/[`InboundGroupSession::merge`] decide
//! whether the incoming share is `Better`/`Worse`/`Equal`/`Unconnected`;
//! only `Worse` (the incoming share is a genuine improvement) replaces the
//! stored session, and it keeps the existing replay index (the same
//! underlying ratchet, just a lower starting point) while rebinding to
//! the new share's own sender identity.
//!
//! [`GroupSessionManager::decrypt_event`] additionally checks, in order:
//! a session exists at all ([`GroupDecryptError::MissingSession`]); the
//! decrypted plaintext's own `room_id` matches the room the ciphertext
//! actually arrived in ([`GroupDecryptError::WrongRoom`] -- a cross-room
//! replay guard); the event's `sender` matches the session's bound user
//! ([`GroupDecryptError::WrongSender`] -- a session from one user can
//! never authenticate an event claimed by another); and finally replay
//! protection over `(session_id, message_index)` -> `(event_id,
//! origin_server_ts)` ([`GroupDecryptError::Replay`] -- the exact same
//! event redelivered is fine and is not an error, a DIFFERENT event or
//! timestamp reusing the same index is).

use crate::crypto::account::OlmAccountState;
use crate::crypto::device_tracker::StoredDevice;
use crate::crypto::olm_sessions::{DecryptedToDevice, OlmSessionManager};
use crate::error::MessengerError;
use crate::ids::{DeviceId, EventId, RoomId, UserId};
use crate::store::CryptoStore;
use crate::wire::events::{MegolmEncryptedContent, RelatesTo, RoomEncryptedContent, RoomEncryptionContent, RoomKeyContent};
use mlc_vodozemac::megolm::{
    DecryptionError as MegolmDecryptionError, GroupSession, InboundGroupSession, MegolmMessage, SessionConfig,
    SessionKey, SessionOrdering,
};
use mlc_vodozemac::{Curve25519PublicKey, DecodeError as VodozemacDecodeError, Ed25519PublicKey};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The only Megolm algorithm this crate speaks (research doc §3.1).
const MEGOLM_ALGORITHM: &str = "m.megolm.v1.aes-sha2";

/// The Matrix `sendToDevice` fan-out cap this crate splits large room-key
/// shares at (research doc §3.3's batching note; task brief: "≤ 250
/// devices per request").
pub const MAX_DEVICES_PER_SEND_TO_DEVICE_REQUEST: usize = 250;

/// The UI-facing reason a timeline item could not be decrypted (plan
/// `lib.rs`'s `MessengerEvent::Utd` contract set -- kept an exhaustive,
/// stable set independent of this crate's own internal error taxonomy so
/// the UI-contract crate never has to track every internal Megolm/Olm
/// error variant).
///
/// Only [`UtdReason::MissingSession`] and [`UtdReason::Unknown`] are ever
/// produced by this piece (M7) -- see [`GroupDecryptError::utd_reason`]'s
/// own doc for the exact mapping and why `Replay`/`WrongRoom`/
/// `WrongSender` collapse to `Unknown` rather than getting their own
/// variants here. The `Withheld*`/`SenderIdentityNotVerified` variants are
/// hooks for later pieces: `crypto::withheld` (M8) produces
/// `WithheldBlacklisted`/`WithheldUnverified`/`WithheldUnauthorised` from
/// an `m.room_key.withheld` code, and `crypto::verification`/
/// `cross_signing` (M8/M10) produce `SenderIdentityNotVerified`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UtdReason {
    /// No inbound session for this `(room, session_id)` exists yet.
    MissingSession,
    /// The sender explicitly withheld the key: this device is blocked.
    /// Never produced by this piece -- a hook for `crypto::withheld` (M8).
    WithheldBlacklisted,
    /// The sender explicitly withheld the key: this device is unverified.
    /// Never produced by this piece -- a hook for `crypto::withheld` (M8).
    WithheldUnverified,
    /// The sender explicitly withheld the key: this user is not
    /// authorised. Never produced by this piece -- a hook for
    /// `crypto::withheld` (M8).
    WithheldUnauthorised,
    /// The sender's identity has not been verified. Never produced by
    /// this piece -- a hook for `crypto::verification`/`cross_signing`
    /// (M8/M10).
    SenderIdentityNotVerified,
    /// Every other reason this piece's own checks reject a message for
    /// (a replay, a cross-room or cross-sender mismatch, a malformed
    /// ciphertext, ...) -- see [`GroupDecryptError::utd_reason`].
    Unknown,
}

/// The plaintext a Megolm-encrypted room event actually carries (task
/// brief's "Encrypt" rule: `{type, content, room_id}`) -- shared shape for
/// both [`GroupSessionManager::encrypt_event`]'s input and
/// [`GroupSessionManager::decrypt_event`]'s output. Embedding `room_id`
/// inside the ciphertext itself (not just relying on whatever room the
/// wire event happened to arrive in) is what makes the cross-room replay
/// check in [`GroupSessionManager::decrypt_event`] possible at all.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoomEventPlaintext {
    /// The inner event's own type (e.g. `"m.room.message"`).
    #[serde(rename = "type")]
    pub event_type: String,
    /// The inner event's own content.
    pub content: serde_json::Value,
    /// The room this plaintext was encrypted for.
    pub room_id: RoomId,
}

/// The result of [`GroupSessionManager::ensure_outbound_session`].
#[derive(Debug, Clone)]
pub struct OutboundSessionUpdate {
    /// The (possibly just-rotated) outbound session's id.
    pub session_id: String,
    /// `true` iff this call created a brand-new session (either none
    /// existed yet, or a rotation trigger fired).
    pub rotated: bool,
    /// Recipients that have never been shared this session
    /// (`shared_with`, before this call) and must now receive
    /// `room_key_content` over Olm to-device. Empty when nothing changed.
    pub new_recipients: Vec<StoredDevice>,
    /// The `m.room_key` to-device payload for `new_recipients`, or `None`
    /// when `new_recipients` is empty (nothing to share).
    pub room_key_content: Option<RoomKeyContent>,
}

/// The durable record behind [`crate::store::CryptoStore::
/// outbound_group_session`]/[`crate::store::CryptoStore::
/// save_outbound_group_session`] -- an opaque blob at that layer; this
/// module is the only one that interprets it (same convention as
/// `crypto::account`'s `AccountRecord`).
#[derive(Serialize, Deserialize)]
struct OutboundGroupSessionRecord {
    session: mlc_vodozemac::megolm::GroupSessionPickle,
    /// Wall-clock milliseconds this session was created at (caller-supplied
    /// `now_ms`, plan §6.7) -- compared against `rotation_period_ms`.
    created_at_ms: i64,
    /// Every `(user, device)` this session has ever been shared with.
    /// Reset to empty on every rotation.
    shared_with: BTreeSet<(UserId, DeviceId)>,
}

/// One replay-protection entry: the first `(event_id, origin_server_ts)`
/// this device ever saw a given Megolm `message_index` decrypted under.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ReplayEntry {
    event_id: EventId,
    origin_server_ts: i64,
}

/// The durable record behind [`crate::store::CryptoStore::
/// inbound_group_session`]/[`crate::store::CryptoStore::
/// save_inbound_group_session`] -- an opaque blob at that layer, same
/// convention as [`OutboundGroupSessionRecord`].
#[derive(Serialize, Deserialize)]
struct InboundGroupSessionRecord {
    session: mlc_vodozemac::megolm::InboundGroupSessionPickle,
    /// The user this session is bound to -- the Olm-verified sender of
    /// the `m.room_key` that (most recently, if merged) produced this
    /// record. A room event whose `sender` differs is never authenticated
    /// by this session (module doc).
    bound_user_id: UserId,
    /// The bound sender device's Curve25519 identity key, base64.
    bound_sender_curve25519: String,
    /// The bound sender device's Ed25519 signing key, base64.
    bound_sender_ed25519: String,
    /// Every message index this device has already decrypted under this
    /// session, and what it was first seen as -- module doc's replay
    /// protection.
    replay_index: BTreeMap<u32, ReplayEntry>,
}

/// Every way [`GroupSessionManager::decrypt_event`] can fail. See the
/// module doc's "Inbound" section for the exact check order these
/// variants correspond to.
#[derive(Debug, thiserror::Error)]
pub enum GroupDecryptError {
    /// No inbound session for `(room_id, session_id)` exists yet -- a
    /// real, structural UTD (missing key), not a transient one.
    #[error("no inbound Megolm session for room {room_id} session {session_id}")]
    MissingSession {
        /// The room the caller asked to decrypt in.
        room_id: RoomId,
        /// The session id the event names.
        session_id: String,
    },
    /// The decrypted plaintext's own `room_id` does not match the room
    /// the ciphertext actually arrived in -- a cross-room replay attempt.
    #[error("decrypted payload's room_id {plaintext_room_id} does not match the event's own room {event_room_id}")]
    WrongRoom {
        /// The room this event actually arrived in.
        event_room_id: RoomId,
        /// The room the decrypted plaintext itself claims.
        plaintext_room_id: RoomId,
    },
    /// The event's `sender` does not match the session's bound user --
    /// a session from one user can never authenticate an event claimed
    /// by another.
    #[error("event sender {event_sender} does not match this session's bound sender {bound_user_id}")]
    WrongSender {
        /// The user this session is bound to.
        bound_user_id: UserId,
        /// The event's own claimed sender.
        event_sender: UserId,
    },
    /// The same `(session_id, message_index)` was already seen under a
    /// DIFFERENT event id or timestamp -- a genuine replay. The exact
    /// same event redelivered is not an error (see this method's own
    /// doc).
    #[error("message index {message_index} of session {session_id} was already seen under a different event")]
    Replay {
        /// The session whose index was reused.
        session_id: String,
        /// The reused message index.
        message_index: u32,
    },
    /// The ciphertext bytes were not a well-formed [`MegolmMessage`].
    #[error("malformed Megolm ciphertext: {0}")]
    MalformedCiphertext(#[from] VodozemacDecodeError),
    /// Megolm's own signature/MAC/ratchet-index check failed.
    #[error("Megolm decryption failed: {0}")]
    Vodozemac(#[from] MegolmDecryptionError),
    /// The decrypted bytes, or a stored record, were not the expected
    /// JSON shape.
    #[error("decrypted or stored payload was not valid JSON: {0}")]
    InvalidPayload(#[from] serde_json::Error),
    /// A [`crate::store::CryptoStore`] operation failed.
    #[error("store: {0}")]
    Store(#[from] crate::store::StoreError),
}

impl GroupDecryptError {
    /// Maps this error onto the UI-facing [`UtdReason`] set. Only
    /// [`GroupDecryptError::MissingSession`] gets its own variant
    /// ([`UtdReason::MissingSession`]) -- `WrongRoom`, `WrongSender`,
    /// `Replay`, and every decode/crypto failure collapse to
    /// [`UtdReason::Unknown`] (task brief's own documented choice): none
    /// of them are actionable the way "you're missing a key, maybe a
    /// re-share will fix it" is, and surfacing them precisely to the UI
    /// would leak internal attack-detection detail (e.g. "this was a
    /// replay") without giving the user anything useful to do about it.
    pub fn utd_reason(&self) -> UtdReason {
        match self {
            GroupDecryptError::MissingSession { .. } => UtdReason::MissingSession,
            _ => UtdReason::Unknown,
        }
    }
}

/// Outbound/inbound Megolm group-session management. See the module doc
/// for the security invariants this type enforces. Carries no state of
/// its own -- every method reads/writes through the caller's
/// [`CryptoStore`].
pub struct GroupSessionManager;

impl GroupSessionManager {
    /// Ensures room `room_id` has a current outbound Megolm session
    /// reflecting `member_user_ids`/`known_devices`, rotating it first if
    /// any of the module doc's rotation triggers fire. Returns the
    /// (possibly new) session's id, whether it rotated, and the room-key
    /// share (if any) a caller must now fan out over Olm to-device before
    /// this session can be used to [`GroupSessionManager::encrypt_event`]
    /// for anyone new.
    ///
    /// `our_user_id`/`our_device_id` are excluded from the recipient set
    /// (a device never Olm-shares its own outbound session key to
    /// itself -- it already has it) even if present in `known_devices`.
    #[allow(clippy::too_many_arguments)]
    pub fn ensure_outbound_session<S: CryptoStore>(
        store: &mut S,
        room_id: &RoomId,
        our_user_id: &UserId,
        our_device_id: &DeviceId,
        encryption: &RoomEncryptionContent,
        member_user_ids: &BTreeSet<UserId>,
        known_devices: &[StoredDevice],
        now_ms: i64,
    ) -> Result<OutboundSessionUpdate, MessengerError> {
        let existing_bytes = store.outbound_group_session(room_id)?.map(<[u8]>::to_vec);

        let (group_session, created_at_ms, mut shared_with, rotated) = match existing_bytes {
            None => (GroupSession::new(SessionConfig::version_1()), now_ms, BTreeSet::new(), true),
            Some(bytes) => {
                let record: OutboundGroupSessionRecord = serde_json::from_slice(&bytes).map_err(|source| {
                    MessengerError::Crypto(format!("decode outbound Megolm session for room {room_id}: {source}"))
                })?;
                let group_session = GroupSession::from_pickle(record.session);
                let msgs_exhausted = i64::from(group_session.message_index()) >= encryption.rotation_period_msgs;
                let time_elapsed = now_ms.saturating_sub(record.created_at_ms) >= encryption.rotation_period_ms;
                let a_shared_user_left =
                    record.shared_with.iter().any(|(user_id, _)| !member_user_ids.contains(user_id));
                let a_shared_device_disappeared = record.shared_with.iter().any(|(user_id, device_id)| {
                    !known_devices.iter().any(|device| &device.user_id == user_id && &device.device_id == device_id)
                });
                if msgs_exhausted || time_elapsed || a_shared_user_left || a_shared_device_disappeared {
                    (GroupSession::new(SessionConfig::version_1()), now_ms, BTreeSet::new(), true)
                } else {
                    (group_session, record.created_at_ms, record.shared_with, false)
                }
            }
        };

        let new_recipients: Vec<StoredDevice> = known_devices
            .iter()
            .filter(|device| {
                member_user_ids.contains(&device.user_id)
                    && !device.blocked
                    && !(device.user_id == *our_user_id && device.device_id == *our_device_id)
            })
            .filter(|device| !shared_with.contains(&(device.user_id.clone(), device.device_id.clone())))
            .cloned()
            .collect();

        let session_id = group_session.session_id();
        let room_key_content = if new_recipients.is_empty() {
            None
        } else {
            for device in &new_recipients {
                shared_with.insert((device.user_id.clone(), device.device_id.clone()));
            }
            Some(RoomKeyContent {
                algorithm: MEGOLM_ALGORITHM.to_string(),
                room_id: room_id.clone(),
                session_id: session_id.clone(),
                session_key: group_session.session_key().to_base64(),
            })
        };

        if rotated || room_key_content.is_some() {
            let record = OutboundGroupSessionRecord { session: group_session.pickle(), created_at_ms, shared_with };
            let bytes = serde_json::to_vec(&record).map_err(|source| {
                MessengerError::Crypto(format!("encode outbound Megolm session for room {room_id}: {source}"))
            })?;
            store.save_outbound_group_session(room_id, bytes)?;
        }

        Ok(OutboundSessionUpdate { session_id, rotated, new_recipients, room_key_content })
    }

    /// Encrypts one room event under `room_id`'s current outbound Megolm
    /// session (must already exist -- call
    /// [`GroupSessionManager::ensure_outbound_session`] first). Persists
    /// the ratchet-advanced session before returning, so a crash right
    /// after this call never reuses the message index it just consumed
    /// (module doc; this crate's flush-before-send discipline).
    pub fn encrypt_event<S: CryptoStore>(
        store: &mut S,
        room_id: &RoomId,
        sender_curve25519_b64: &str,
        sender_device_id: &DeviceId,
        event_type: &str,
        content: serde_json::Value,
        relates_to: Option<RelatesTo>,
    ) -> Result<MegolmEncryptedContent, MessengerError> {
        let bytes = store
            .outbound_group_session(room_id)?
            .map(<[u8]>::to_vec)
            .ok_or_else(|| MessengerError::Crypto(format!("no outbound Megolm session for room {room_id} yet")))?;
        let mut record: OutboundGroupSessionRecord = serde_json::from_slice(&bytes).map_err(|source| {
            MessengerError::Crypto(format!("decode outbound Megolm session for room {room_id}: {source}"))
        })?;
        let mut group_session = GroupSession::from_pickle(record.session);

        let plaintext = RoomEventPlaintext { event_type: event_type.to_string(), content, room_id: room_id.clone() };
        let plaintext_bytes = serde_json::to_vec(&plaintext)?;
        let message = group_session.encrypt(plaintext_bytes);
        let session_id = group_session.session_id();

        record.session = group_session.pickle();
        let record_bytes = serde_json::to_vec(&record).map_err(|source| {
            MessengerError::Crypto(format!("encode outbound Megolm session for room {room_id}: {source}"))
        })?;
        store.save_outbound_group_session(room_id, record_bytes)?;

        Ok(MegolmEncryptedContent {
            ciphertext: message.to_base64(),
            session_id,
            sender_key: Some(sender_curve25519_b64.to_string()),
            device_id: Some(sender_device_id.clone()),
            relates_to,
        })
    }

    /// Splits `recipients` into chunks of at most
    /// [`MAX_DEVICES_PER_SEND_TO_DEVICE_REQUEST`] devices (module doc) --
    /// a caller builds one `sendToDevice` request per chunk via
    /// [`GroupSessionManager::build_room_key_send_to_device_body`], each
    /// with its own `RequestId`/`TxnId` (only the caller's own sequence
    /// can mint those -- `crate::ids`'s module doc explains why this
    /// crate never keeps a hidden counter of its own).
    pub fn chunk_recipients_for_send_to_device(recipients: &[StoredDevice]) -> std::slice::Chunks<'_, StoredDevice> {
        recipients.chunks(MAX_DEVICES_PER_SEND_TO_DEVICE_REQUEST)
    }

    /// Builds one `sendToDevice` request body (the client-server API's
    /// `{"messages": {"@user:x": {"DEVICEID": {...}}}}` shape) sharing
    /// `room_key_content` with every device in `chunk`, Olm-encrypting it
    /// individually per device. Every device in `chunk` must already have
    /// an established Olm session (a prior `/keys/claim` round trip via
    /// [`OlmSessionManager`]) -- this function surfaces the first missing
    /// one as an error rather than silently dropping it from the fan-out.
    pub fn build_room_key_send_to_device_body<S: CryptoStore>(
        store: &mut S,
        account: &OlmAccountState,
        our_user_id: &UserId,
        our_device_id: &DeviceId,
        chunk: &[StoredDevice],
        room_key_content: &RoomKeyContent,
    ) -> Result<serde_json::Value, MessengerError> {
        let content_value = serde_json::to_value(room_key_content)?;
        let mut by_user: BTreeMap<UserId, serde_json::Map<String, serde_json::Value>> = BTreeMap::new();
        for device in chunk {
            let encrypted = OlmSessionManager::encrypt_to_device(
                store,
                account,
                our_user_id,
                our_device_id,
                device,
                "m.room_key",
                content_value.clone(),
            )?;
            let wire_content = serde_json::to_value(RoomEncryptedContent::Olm(encrypted))?;
            by_user.entry(device.user_id.clone()).or_default().insert(device.device_id.as_str().to_string(), wire_content);
        }
        let mut top = serde_json::Map::new();
        for (user_id, devices) in by_user {
            top.insert(user_id.as_str().to_string(), serde_json::Value::Object(devices));
        }
        Ok(serde_json::json!({ "messages": serde_json::Value::Object(top) }))
    }

    /// Convenience wrapper over [`GroupSessionManager::receive_room_key`]
    /// for a caller holding a [`DecryptedToDevice`] -- the type only
    /// [`OlmSessionManager::decrypt_to_device`] can construct, which is
    /// what enforces the module doc's "only from an Olm-decrypted
    /// to-device event" rule structurally rather than by a runtime check.
    /// Returns `Ok(false)` (a no-op, not an error) for any to-device
    /// event whose inner type is not `"m.room_key"` -- a caller can call
    /// this unconditionally on every decrypted to-device event without
    /// pre-filtering.
    pub fn accept_room_key_from_to_device<S: CryptoStore>(
        store: &mut S,
        decrypted: &DecryptedToDevice,
    ) -> Result<bool, MessengerError> {
        if decrypted.event_type != "m.room_key" {
            return Ok(false);
        }
        let content: RoomKeyContent = serde_json::from_value(decrypted.content.clone())?;
        if content.algorithm != MEGOLM_ALGORITHM {
            return Ok(false);
        }
        Self::receive_room_key(store, &decrypted.sender, decrypted.sender_device_curve25519, decrypted.sender_ed25519, &content)?;
        Ok(true)
    }

    /// Accepts an `m.room_key` payload already known to have arrived over
    /// an Olm-decrypted to-device event, binding the resulting inbound
    /// session to `sender_user_id`/`sender_curve25519`/`sender_ed25519`.
    /// See the module doc's "Inbound" section for the merge-not-overwrite
    /// rule this method enforces when a session for the same id already
    /// exists.
    pub fn receive_room_key<S: CryptoStore>(
        store: &mut S,
        sender_user_id: &UserId,
        sender_curve25519: Curve25519PublicKey,
        sender_ed25519: Ed25519PublicKey,
        content: &RoomKeyContent,
    ) -> Result<(), MessengerError> {
        if content.algorithm != MEGOLM_ALGORITHM {
            return Ok(());
        }
        let session_key = SessionKey::from_base64(&content.session_key)
            .map_err(|source| MessengerError::Crypto(format!("malformed Megolm session_key: {source}")))?;
        let mut incoming_session = InboundGroupSession::new(&session_key, SessionConfig::version_1());

        let existing_bytes = store.inbound_group_session(&content.room_id, &content.session_id)?.map(<[u8]>::to_vec);

        let record = match existing_bytes {
            None => InboundGroupSessionRecord {
                session: incoming_session.pickle(),
                bound_user_id: sender_user_id.clone(),
                bound_sender_curve25519: sender_curve25519.to_base64(),
                bound_sender_ed25519: sender_ed25519.to_base64(),
                replay_index: BTreeMap::new(),
            },
            Some(bytes) => {
                let existing_record: InboundGroupSessionRecord = serde_json::from_slice(&bytes).map_err(|source| {
                    MessengerError::Crypto(format!(
                        "decode inbound Megolm session for room {} session {}: {source}",
                        content.room_id, content.session_id
                    ))
                })?;
                let mut existing_session = InboundGroupSession::from_pickle(existing_record.session);
                match existing_session.compare(&mut incoming_session) {
                    // The stored session is already at least as good, or
                    // this share does not even connect to it -- never let
                    // a later share regress (or hijack) an already-known
                    // session (module doc).
                    SessionOrdering::Equal | SessionOrdering::Better | SessionOrdering::Unconnected => return Ok(()),
                    SessionOrdering::Worse => {
                        // `compare() == Worse` only when `connected()` is
                        // true (its own doc), so `merge` always succeeds
                        // here -- `unwrap_or` is a defensive fallback,
                        // never a load-bearing branch: even if that
                        // invariant were somehow violated, adopting the
                        // incoming session outright (still strictly
                        // better by index than what we had) is still
                        // forward progress, never a panic.
                        let merged = existing_session.merge(&mut incoming_session).unwrap_or(incoming_session);
                        InboundGroupSessionRecord {
                            session: merged.pickle(),
                            bound_user_id: sender_user_id.clone(),
                            bound_sender_curve25519: sender_curve25519.to_base64(),
                            bound_sender_ed25519: sender_ed25519.to_base64(),
                            replay_index: existing_record.replay_index,
                        }
                    }
                }
            }
        };

        let bytes = serde_json::to_vec(&record).map_err(|source| {
            MessengerError::Crypto(format!(
                "encode inbound Megolm session for room {} session {}: {source}",
                content.room_id, content.session_id
            ))
        })?;
        store.save_inbound_group_session(&content.room_id, &content.session_id, bytes)?;
        Ok(())
    }

    /// Decrypts one `m.room.encrypted` (Megolm) room event. See the
    /// module doc's "Inbound" section for the exact check order.
    pub fn decrypt_event<S: CryptoStore>(
        store: &mut S,
        room_id: &RoomId,
        event_id: &EventId,
        origin_server_ts: i64,
        sender: &UserId,
        content: &MegolmEncryptedContent,
    ) -> Result<RoomEventPlaintext, GroupDecryptError> {
        let bytes = store.inbound_group_session(room_id, &content.session_id)?.map(<[u8]>::to_vec).ok_or_else(|| {
            GroupDecryptError::MissingSession { room_id: room_id.clone(), session_id: content.session_id.clone() }
        })?;
        let mut record: InboundGroupSessionRecord = serde_json::from_slice(&bytes)?;
        let mut session = InboundGroupSession::from_pickle(record.session);

        let message = MegolmMessage::from_base64(&content.ciphertext)?;
        let decrypted = session.decrypt(&message)?;
        let plaintext: RoomEventPlaintext = serde_json::from_slice(&decrypted.plaintext)?;

        if plaintext.room_id != *room_id {
            return Err(GroupDecryptError::WrongRoom {
                event_room_id: room_id.clone(),
                plaintext_room_id: plaintext.room_id,
            });
        }
        if record.bound_user_id != *sender {
            return Err(GroupDecryptError::WrongSender {
                bound_user_id: record.bound_user_id,
                event_sender: sender.clone(),
            });
        }

        match record.replay_index.get(&decrypted.message_index) {
            Some(seen) if seen.event_id != *event_id || seen.origin_server_ts != origin_server_ts => {
                return Err(GroupDecryptError::Replay {
                    session_id: content.session_id.clone(),
                    message_index: decrypted.message_index,
                });
            }
            Some(_) => {
                // The exact same event, redelivered -- fine (module doc).
            }
            None => {
                record
                    .replay_index
                    .insert(decrypted.message_index, ReplayEntry { event_id: event_id.clone(), origin_server_ts });
                record.session = session.pickle();
                let bytes = serde_json::to_vec(&record)?;
                store.save_inbound_group_session(room_id, &content.session_id, bytes)?;
            }
        }

        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::room::timeline::{ItemContent, Timeline};
    use crate::store::{InsecurePlainCodecForTests, Store};
    use crate::wire::events::RawEvent;
    use mlc_vodozemac::olm::Account;

    fn new_store() -> Store<InsecurePlainCodecForTests> {
        Store::new(DeviceId::parse("DEV").expect("valid device id"), InsecurePlainCodecForTests)
    }

    fn user(name: &str) -> UserId {
        UserId::parse(format!("@{name}:example.org")).expect("valid user id")
    }

    fn device(name: &str) -> DeviceId {
        DeviceId::parse(name).expect("valid device id")
    }

    fn room(name: &str) -> RoomId {
        RoomId::parse(format!("!{name}:example.org")).expect("valid room id")
    }

    fn stored_device(user_id: &UserId, device_id: &DeviceId) -> StoredDevice {
        let identity = Account::new().identity_keys();
        StoredDevice {
            user_id: user_id.clone(),
            device_id: device_id.clone(),
            curve25519: identity.curve25519,
            ed25519: identity.ed25519,
            algorithms: vec!["m.olm.v1.curve25519-aes-sha2".to_string(), "m.megolm.v1.aes-sha2".to_string()],
            display_name: None,
            verified: false,
            blocked: false,
        }
    }

    fn default_encryption() -> RoomEncryptionContent {
        RoomEncryptionContent {
            algorithm: MEGOLM_ALGORITHM.to_string(),
            rotation_period_ms: 604_800_000,
            rotation_period_msgs: 100,
        }
    }

    fn plaintext_bytes(room_id: &RoomId, body: &str) -> Vec<u8> {
        serde_json::to_vec(&RoomEventPlaintext {
            event_type: "m.room.message".to_string(),
            content: serde_json::json!({ "body": body }),
            room_id: room_id.clone(),
        })
        .expect("valid JSON")
    }

    #[test]
    fn outbound_session_rotates_after_rotation_period_msgs() {
        let mut store = new_store();
        let room_id = room("r");
        let alice = user("alice");
        let alice_dev = device("ALICEDEV");
        let bob = user("bob");
        let bob_device = stored_device(&bob, &device("BOBDEV"));
        let members: BTreeSet<UserId> = [alice.clone(), bob.clone()].into_iter().collect();
        let known = vec![bob_device];
        let encryption =
            RoomEncryptionContent { algorithm: MEGOLM_ALGORITHM.to_string(), rotation_period_ms: 604_800_000, rotation_period_msgs: 2 };

        let first = GroupSessionManager::ensure_outbound_session(&mut store, &room_id, &alice, &alice_dev, &encryption, &members, &known, 0)
            .expect("create session");
        assert!(first.rotated);
        assert!(first.room_key_content.is_some());

        GroupSessionManager::encrypt_event(&mut store, &room_id, "alice-curve", &alice_dev, "m.room.message", serde_json::json!({}), None)
            .expect("encrypt 1");
        GroupSessionManager::encrypt_event(&mut store, &room_id, "alice-curve", &alice_dev, "m.room.message", serde_json::json!({}), None)
            .expect("encrypt 2");

        let second = GroupSessionManager::ensure_outbound_session(&mut store, &room_id, &alice, &alice_dev, &encryption, &members, &known, 0)
            .expect("re-check");
        assert!(second.rotated, "message-count threshold reached");
        assert_ne!(second.session_id, first.session_id);
    }

    #[test]
    fn outbound_session_rotates_after_rotation_period_ms() {
        let mut store = new_store();
        let room_id = room("r");
        let alice = user("alice");
        let alice_dev = device("ALICEDEV");
        let bob = user("bob");
        let bob_device = stored_device(&bob, &device("BOBDEV"));
        let members: BTreeSet<UserId> = [alice.clone(), bob.clone()].into_iter().collect();
        let known = vec![bob_device];
        let encryption =
            RoomEncryptionContent { algorithm: MEGOLM_ALGORITHM.to_string(), rotation_period_ms: 1_000, rotation_period_msgs: 100 };

        let first = GroupSessionManager::ensure_outbound_session(&mut store, &room_id, &alice, &alice_dev, &encryption, &members, &known, 0)
            .expect("create session");

        let not_yet =
            GroupSessionManager::ensure_outbound_session(&mut store, &room_id, &alice, &alice_dev, &encryption, &members, &known, 500)
                .expect("re-check before the period elapses");
        assert!(!not_yet.rotated, "rotation_period_ms has not elapsed yet");
        assert_eq!(not_yet.session_id, first.session_id);

        let elapsed =
            GroupSessionManager::ensure_outbound_session(&mut store, &room_id, &alice, &alice_dev, &encryption, &members, &known, 1_000)
                .expect("re-check after the period elapses");
        assert!(elapsed.rotated, "rotation_period_ms has elapsed");
        assert_ne!(elapsed.session_id, first.session_id);
    }

    #[test]
    fn outbound_session_rotates_on_member_removed() {
        let mut store = new_store();
        let room_id = room("r");
        let alice = user("alice");
        let alice_dev = device("ALICEDEV");
        let bob = user("bob");
        let bob_device = stored_device(&bob, &device("BOBDEV"));
        let members: BTreeSet<UserId> = [alice.clone(), bob.clone()].into_iter().collect();
        let known = vec![bob_device];
        let encryption = default_encryption();

        let first = GroupSessionManager::ensure_outbound_session(&mut store, &room_id, &alice, &alice_dev, &encryption, &members, &known, 0)
            .expect("create+share");

        let members_after: BTreeSet<UserId> = [alice.clone()].into_iter().collect();
        let second =
            GroupSessionManager::ensure_outbound_session(&mut store, &room_id, &alice, &alice_dev, &encryption, &members_after, &known, 0)
                .expect("bob removed");
        assert!(second.rotated, "a previously-shared user no longer being a member forces rotation");
        assert_ne!(second.session_id, first.session_id);
        assert!(second.new_recipients.is_empty(), "bob is no longer eligible, nothing new to share");
    }

    #[test]
    fn outbound_session_rotates_when_a_shared_device_disappears() {
        let mut store = new_store();
        let room_id = room("r");
        let alice = user("alice");
        let alice_dev = device("ALICEDEV");
        let bob = user("bob");
        let bob_old_device = stored_device(&bob, &device("BOBOLD"));
        let bob_new_device = stored_device(&bob, &device("BOBNEW"));
        let members: BTreeSet<UserId> = [alice.clone(), bob.clone()].into_iter().collect();
        let encryption = default_encryption();

        let first = GroupSessionManager::ensure_outbound_session(
            &mut store,
            &room_id,
            &alice,
            &alice_dev,
            &encryption,
            &members,
            std::slice::from_ref(&bob_old_device),
            0,
        )
        .expect("create+share with the old device");

        // Bob's old device disappeared from the tracked device list
        // entirely (deleted/logged out) -- only the new one is known now.
        let second = GroupSessionManager::ensure_outbound_session(
            &mut store,
            &room_id,
            &alice,
            &alice_dev,
            &encryption,
            &members,
            std::slice::from_ref(&bob_new_device),
            0,
        )
        .expect("old device gone");
        assert!(second.rotated, "a previously-shared device disappearing forces rotation");
        assert_ne!(second.session_id, first.session_id);
        assert_eq!(second.new_recipients, vec![bob_new_device]);
    }

    #[test]
    fn new_device_gets_current_session_without_rotation() {
        let mut store = new_store();
        let room_id = room("r");
        let alice = user("alice");
        let alice_dev = device("ALICEDEV");
        let bob = user("bob");
        let bob_device_1 = stored_device(&bob, &device("BOBDEV1"));
        let bob_device_2 = stored_device(&bob, &device("BOBDEV2"));
        let members: BTreeSet<UserId> = [alice.clone(), bob.clone()].into_iter().collect();
        let encryption = default_encryption();

        let first = GroupSessionManager::ensure_outbound_session(
            &mut store,
            &room_id,
            &alice,
            &alice_dev,
            &encryption,
            &members,
            std::slice::from_ref(&bob_device_1),
            0,
        )
        .expect("create+share with device 1");

        GroupSessionManager::encrypt_event(&mut store, &room_id, "alice-curve", &alice_dev, "m.room.message", serde_json::json!({}), None)
            .expect("encrypt one message, advancing the ratchet to index 1");

        let known = vec![bob_device_1.clone(), bob_device_2.clone()];
        let second = GroupSessionManager::ensure_outbound_session(&mut store, &room_id, &alice, &alice_dev, &encryption, &members, &known, 0)
            .expect("device 2 appears");
        assert!(!second.rotated, "a new device of an already-shared member never forces rotation");
        assert_eq!(second.session_id, first.session_id);
        assert_eq!(second.new_recipients, vec![bob_device_2]);

        // The share reflects the CURRENT ratchet position (index 1), not
        // the initial one -- device 2 cannot read message 0.
        let shared = second.room_key_content.expect("something to share with device 2");
        let session_key = SessionKey::from_base64(&shared.session_key).expect("valid session key");
        let inbound = InboundGroupSession::new(&session_key, SessionConfig::version_1());
        assert_eq!(inbound.first_known_index(), 1);
    }

    #[test]
    fn room_key_from_a_room_event_is_ignored() {
        // A room-timeline-shaped `m.room_key` is never treated as key
        // material anywhere in this crate: `room::timeline` does not
        // special-case it (falls through to `ItemContent::Unknown`), and
        // `GroupSessionManager`'s only room-key intake function
        // (`receive_room_key`/`accept_room_key_from_to_device`) takes an
        // Olm-verified sender identity or a `DecryptedToDevice` -- neither
        // of which a bare room event can ever produce.
        let event: RawEvent = serde_json::from_value(serde_json::json!({
            "event_id": "$x:example.org",
            "type": "m.room_key",
            "sender": "@mallory:example.org",
            "origin_server_ts": 1,
            "content": {
                "algorithm": "m.megolm.v1.aes-sha2",
                "room_id": "!r:example.org",
                "session_id": "abc",
                "session_key": "irrelevant"
            }
        }))
        .expect("valid raw event shape");

        let mut timeline = Timeline::new();
        timeline.apply_timeline_batch(&[event], false, None);

        assert_eq!(timeline.items().len(), 1);
        assert_eq!(timeline.items()[0].content, ItemContent::Unknown);
    }

    #[test]
    fn decrypted_payload_for_another_room_is_rejected() {
        let mut alice_store = new_store();
        let room_a = room("a");
        let room_b = room("b");
        let alice = user("alice");
        let alice_dev = device("ALICEDEV");
        let bob = user("bob");
        let bob_device = stored_device(&bob, &device("BOBDEV"));
        let members: BTreeSet<UserId> = [alice.clone(), bob.clone()].into_iter().collect();
        let encryption = default_encryption();
        let identity = Account::new().identity_keys();

        let update = GroupSessionManager::ensure_outbound_session(
            &mut alice_store,
            &room_a,
            &alice,
            &alice_dev,
            &encryption,
            &members,
            std::slice::from_ref(&bob_device),
            0,
        )
        .expect("create+share");
        let mut room_key_content = update.room_key_content.expect("bob is a new recipient");

        let encrypted = GroupSessionManager::encrypt_event(
            &mut alice_store,
            &room_a,
            &identity.curve25519.to_base64(),
            &alice_dev,
            "m.room.message",
            serde_json::json!({ "body": "hi" }),
            None,
        )
        .expect("encrypt under room A");

        // Bob's client is fed the room key mislabelled as belonging to
        // room B -- the only way it could ever end up filing a genuine
        // room-A ciphertext under a session it believes is room B's.
        room_key_content.room_id = room_b.clone();
        let mut bob_store = new_store();
        GroupSessionManager::receive_room_key(&mut bob_store, &alice, identity.curve25519, identity.ed25519, &room_key_content)
            .expect("receive");

        let event_id = EventId::parse("$evt:example.org").expect("valid event id");
        let result = GroupSessionManager::decrypt_event(&mut bob_store, &room_b, &event_id, 1, &alice, &encrypted);
        assert!(matches!(result, Err(GroupDecryptError::WrongRoom { .. })), "got {result:?}");
    }

    #[test]
    fn session_cannot_authenticate_another_sender() {
        let mut alice_store = new_store();
        let room_id = room("r");
        let alice = user("alice");
        let alice_dev = device("ALICEDEV");
        let mallory = user("mallory");
        let bob = user("bob");
        let bob_device = stored_device(&bob, &device("BOBDEV"));
        let members: BTreeSet<UserId> = [alice.clone(), bob.clone()].into_iter().collect();
        let encryption = default_encryption();
        let identity = Account::new().identity_keys();

        let update = GroupSessionManager::ensure_outbound_session(
            &mut alice_store,
            &room_id,
            &alice,
            &alice_dev,
            &encryption,
            &members,
            std::slice::from_ref(&bob_device),
            0,
        )
        .expect("create+share");
        let room_key_content = update.room_key_content.expect("bob is a new recipient");

        let mut bob_store = new_store();
        GroupSessionManager::receive_room_key(&mut bob_store, &alice, identity.curve25519, identity.ed25519, &room_key_content)
            .expect("bob genuinely received this key from alice");

        let encrypted = GroupSessionManager::encrypt_event(
            &mut alice_store,
            &room_id,
            &identity.curve25519.to_base64(),
            &alice_dev,
            "m.room.message",
            serde_json::json!({ "body": "hi" }),
            None,
        )
        .expect("encrypt");

        // The room event itself claims mallory as the sender.
        let event_id = EventId::parse("$evt:example.org").expect("valid event id");
        let result = GroupSessionManager::decrypt_event(&mut bob_store, &room_id, &event_id, 1, &mallory, &encrypted);
        assert!(matches!(result, Err(GroupDecryptError::WrongSender { .. })), "got {result:?}");
    }

    #[test]
    fn inbound_session_rejects_replayed_message_index() {
        let mut alice_store = new_store();
        let room_id = room("r");
        let alice = user("alice");
        let alice_dev = device("ALICEDEV");
        let bob = user("bob");
        let bob_device = stored_device(&bob, &device("BOBDEV"));
        let members: BTreeSet<UserId> = [alice.clone(), bob.clone()].into_iter().collect();
        let encryption = default_encryption();
        let identity = Account::new().identity_keys();

        let update = GroupSessionManager::ensure_outbound_session(
            &mut alice_store,
            &room_id,
            &alice,
            &alice_dev,
            &encryption,
            &members,
            std::slice::from_ref(&bob_device),
            0,
        )
        .expect("create+share");
        let room_key_content = update.room_key_content.expect("bob is a new recipient");

        let mut bob_store = new_store();
        GroupSessionManager::receive_room_key(&mut bob_store, &alice, identity.curve25519, identity.ed25519, &room_key_content)
            .expect("receive");

        let encrypted = GroupSessionManager::encrypt_event(
            &mut alice_store,
            &room_id,
            &identity.curve25519.to_base64(),
            &alice_dev,
            "m.room.message",
            serde_json::json!({ "body": "hi" }),
            None,
        )
        .expect("encrypt");

        let event_id_1 = EventId::parse("$e1:example.org").expect("valid event id");
        let first = GroupSessionManager::decrypt_event(&mut bob_store, &room_id, &event_id_1, 100, &alice, &encrypted)
            .expect("first decrypt succeeds");
        assert_eq!(first.content, serde_json::json!({ "body": "hi" }));

        let redelivered = GroupSessionManager::decrypt_event(&mut bob_store, &room_id, &event_id_1, 100, &alice, &encrypted)
            .expect("redelivery of the exact same event is not a replay");
        assert_eq!(redelivered.content, serde_json::json!({ "body": "hi" }));

        let event_id_2 = EventId::parse("$e2:example.org").expect("valid event id");
        let replay = GroupSessionManager::decrypt_event(&mut bob_store, &room_id, &event_id_2, 200, &alice, &encrypted);
        assert!(matches!(replay, Err(GroupDecryptError::Replay { .. })), "got {replay:?}");
    }

    #[test]
    fn later_share_does_not_replace_earlier_index() {
        let alice = user("alice");
        let identity = Account::new().identity_keys();
        let room_id = room("r");

        let mut group_session = GroupSession::new(SessionConfig::version_1());
        let key_at_0 = group_session.session_key();
        let message_0 = group_session.encrypt(plaintext_bytes(&room_id, "msg0"));
        let key_at_1 = group_session.session_key();

        let content_at_1 = RoomKeyContent {
            algorithm: MEGOLM_ALGORITHM.to_string(),
            room_id: room_id.clone(),
            session_id: group_session.session_id(),
            session_key: key_at_1.to_base64(),
        };
        let content_at_0 = RoomKeyContent { session_key: key_at_0.to_base64(), ..content_at_1.clone() };

        let mut bob_store = new_store();
        GroupSessionManager::receive_room_key(&mut bob_store, &alice, identity.curve25519, identity.ed25519, &content_at_1)
            .expect("receive the later (higher-index) share first");
        GroupSessionManager::receive_room_key(&mut bob_store, &alice, identity.curve25519, identity.ed25519, &content_at_0)
            .expect("receive the earlier (lower-index) share second");

        let encrypted_0 = MegolmEncryptedContent {
            ciphertext: message_0.to_base64(),
            session_id: group_session.session_id(),
            sender_key: None,
            device_id: None,
            relates_to: None,
        };
        let event_id = EventId::parse("$evt0:example.org").expect("valid event id");
        let decrypted = GroupSessionManager::decrypt_event(&mut bob_store, &room_id, &event_id, 1, &alice, &encrypted_0)
            .expect("the earlier share's lower index must have won, so message 0 is decryptable");
        assert_eq!(decrypted.content, serde_json::json!({ "body": "msg0" }));
    }
}
