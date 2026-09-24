//! The working-set store: [`Store`], the in-memory `CryptoStore`/
//! `StateStore` implementation every later piece (crypto, room, sync engine)
//! codes against, plus the sealed-record export/import protocol that ties it
//! to [`crate::persist::FlushGate`].
//!
//! # Shape (plan decision #3, as refined for this piece)
//!
//! The plan's own §3.2 sketches `CryptoStore`/`StateStore` as `Send + Sync`
//! traits called through `Arc<dyn Trait>` with `&self` mutation (interior
//! mutability hidden inside the impl). That shape is **not** what this crate
//! builds: decision #3 also settles that the core owns its store **by
//! value**, single-threaded, so both traits here take `&mut self` for every
//! mutation, add no `Send`/`Sync` bound, and [`Store`] is a plain
//! (non-`Arc`, non-`dyn`) struct a `MessengerCore` (a later piece) holds
//! directly. Every other piece of the plan's method *set* — which
//! operations exist, what each is keyed by — survives unchanged; only the
//! calling convention and the value types do not (the pickled Olm/Megolm/
//! cross-signing types the plan names, e.g. `PickledAccount`, are later
//! pieces' work — at this layer every value is an opaque blob, because this
//! store only has to guarantee *persistence* semantics, never interpret
//! what it persists).
//!
//! # Sealing is a codec, not a concrete algorithm
//!
//! [`RecordCodec`] is the seam between "how a value is turned into bytes
//! safe to hand a shell" and everything else in this module. [`Store`] is
//! generic over it (`Store<C: RecordCodec>`) rather than owning a concrete
//! encryption implementation, so this piece never needs the real AES-256-GCM
//! codec the plan's §3.3 describes (`store/sealed.rs`'s
//! `mlc_vault::crypto`-backed codec — a later piece, M14, binding the record
//! key as AAD). [`InsecurePlainCodecForTests`] is the only codec this piece
//! ships: an identity function, good for nothing but letting this crate's
//! own tests build and round-trip a [`Store`] without a real vault key. It
//! is gated behind the `test-support` Cargo feature (off by default, turned
//! on for this crate's own test builds only — see its own doc comment) so no
//! shell can compile against it by accident.
//!
//! # Key layout
//!
//! Every record [`Store`] persists lives under `messenger/{device_id}/...`,
//! extending the plan's §3.3 table with the record kinds §3.2's trait list
//! names but §3.3's own table did not enumerate (tracked users, withheld
//! codes, backup key material, cross-signing key material, legacy DM peer
//! keys) — same prefix convention, same "one flat `Storage` namespace"
//! shape:
//!
//! | Key | Contents |
//! |---|---|
//! | `messenger/{device_id}/account` | pickled Olm account |
//! | `messenger/{device_id}/olm_session/{curve25519}/{n}` | one pickled Olm session (`n`-th for that peer curve key) |
//! | `messenger/{device_id}/megolm_in/{room_id}/{session_id}` | one pickled inbound Megolm session |
//! | `messenger/{device_id}/megolm_out/{room_id}` | the room's pickled outbound Megolm session |
//! | `messenger/{device_id}/devices/{user_id}` | that user's tracked device list |
//! | `messenger/{device_id}/tracked_user/{user_id}` | a single `0`/`1` byte: the user's outdated flag |
//! | `messenger/{device_id}/withheld/{room_id}/{session_id}` | a withheld-reason code |
//! | `messenger/{device_id}/backup_key` | pickled key-backup material |
//! | `messenger/{device_id}/cross_signing_keys` | pickled cross-signing key material |
//! | `messenger/{device_id}/legacy_dm_peer_key/{user_id}` | that peer's 32-byte legacy DM X25519 public key |
//! | `messenger/{device_id}/sync_token` | the `/sync` `next_batch` token, UTF-8 |
//! | `messenger/{device_id}/room_state/{room_id}` | that room's serialized state blob |
//! | `messenger/{device_id}/pending/{request_id}` | one not-yet-acked [`OutgoingRequest`], JSON |
//!
//! Every dynamic path component (`{curve25519}`, `{room_id}`, `{user_id}`,
//! `{session_id}`, `{request_id}`) is percent-encoded with the exact scheme
//! [`crate::wire::requests`] already uses for HTTP paths (reused, not
//! reinvented) — required because a Megolm `session_id` or a legacy device
//! curve key is base64 and may itself contain `/`, which would otherwise be
//! indistinguishable from this layout's own path separators.
//!
//! `messenger/local_device_id` (plaintext, needed before a seal key can even
//! be derived — see the plan's §3.3 note) is deliberately **not** handled by
//! this module: [`Store::load`]/[`Store::new`] both already take `device_id`
//! as an explicit parameter, so deciding which device's namespace to open is
//! a shell/M14 concern, not this store's.
//!
//! # Forward compatibility
//!
//! A key whose kind segment (`account`, `olm_session`, ...) this version of
//! the store does not recognize at all is **not** an error: [`Store::load`]
//! keeps its sealed bytes verbatim and immediately re-adopts them into the
//! very next [`FlushGate::take_batch`] call, unchanged, so a field a newer
//! writer introduced is never silently dropped by an older reader. A key
//! whose kind segment *is* recognized but whose structure doesn't match that
//! kind's shape (a missing id component, a non-numeric session index, ...)
//! is the opposite: a hard [`StoreError::UnknownRecordLayout`], because at
//! that point this store knows enough to know something is wrong, not
//! enough to know what to do about it.

use crate::ids::{DeviceId, RequestId, RoomId, UserId};
use crate::persist::{FlushBatch, FlushGate, RecordKey, SealedRecord};
use crate::wire::{percent_decode_segment, percent_encode_segment, OutgoingRequest};
use std::collections::BTreeMap;

/// Every error [`Store::load`] or a mutation can return. All three variants
/// name the offending [`RecordKey`] — nothing here fails silently or points
/// only at "a record", ever.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The record's own bytes (after a successful [`RecordCodec::open`])
    /// don't match what this key's kind expects — non-UTF-8 bytes where a
    /// sync token string was expected, a byte count other than 32 where a
    /// legacy DM peer key was expected, JSON that doesn't parse as an
    /// [`OutgoingRequest`], and so on.
    #[error("record {key}: {reason}")]
    Decode {
        /// The record whose plaintext failed to decode.
        key: RecordKey,
        /// What about it was wrong.
        reason: String,
    },

    /// [`RecordCodec::open`] rejected a record's sealed bytes outright —
    /// wrong key, corrupted bytes, or (for the real AES-GCM codec a later
    /// piece adds) an authentication failure.
    #[error("record {key}: codec rejected the sealed bytes")]
    CodecOpen {
        /// The record the codec refused to open.
        key: RecordKey,
    },

    /// The key's kind segment (e.g. `"olm_session"`) is recognized, but the
    /// rest of the key does not match that kind's expected shape. Distinct
    /// from a genuinely unrecognized kind, which [`Store::load`] keeps
    /// verbatim rather than treating as an error — see the module doc's
    /// "Forward compatibility" section.
    #[error("record {key}: recognized kind but malformed key layout")]
    UnknownRecordLayout {
        /// The malformed record's key.
        key: RecordKey,
    },
}

impl StoreError {
    fn decode(key: &RecordKey, reason: impl Into<String>) -> Self {
        StoreError::Decode { key: key.clone(), reason: reason.into() }
    }

    fn unknown_layout(key: &RecordKey) -> Self {
        StoreError::UnknownRecordLayout { key: key.clone() }
    }
}

/// Turns a record's plaintext into bytes safe to hand a shell, and back.
/// [`Store`] calls [`seal`](RecordCodec::seal) on every mutation before
/// handing the result to [`FlushGate::mark_dirty`], and calls
/// [`open`](RecordCodec::open) once per record during [`Store::load`]. The
/// real codec (AES-256-GCM, binding `key` as AAD) is a later piece (plan
/// §3.3, M14); this piece only ships [`InsecurePlainCodecForTests`].
pub trait RecordCodec {
    /// Seals `plaintext` for storage at `key`. Infallible by construction:
    /// sealing (unlike opening) has no "wrong key" or "corrupted input" case
    /// to reject.
    fn seal(&self, key: &RecordKey, plaintext: &[u8]) -> Vec<u8>;

    /// Recovers the plaintext `seal` produced for `key`, or a
    /// [`StoreError::CodecOpen`] naming `key` if `sealed` was not produced
    /// by this codec for this key (wrong key, corrupted bytes, or a failed
    /// AEAD authentication check for a real codec).
    fn open(&self, key: &RecordKey, sealed: &[u8]) -> Result<Vec<u8>, StoreError>;
}

/// An identity [`RecordCodec`] that provides **no confidentiality
/// whatsoever** — `seal` and `open` are both the identity function. Exists
/// only so this crate's own tests (and, later, its `tests/e2e_*.rs`
/// integration tests) can build and round-trip a [`Store`] without a real
/// vault key. Never wire this into anything a shell actually persists.
///
/// Gated behind the `test-support` Cargo feature (off by default) so no
/// shell, or any other crate depending on this one normally, can link
/// against a codec that provides zero confidentiality by accident. This
/// crate's own test builds turn the feature on via the self-referential
/// `[dev-dependencies]` entry in `Cargo.toml`.
#[cfg(feature = "test-support")]
#[derive(Debug, Default, Clone, Copy)]
pub struct InsecurePlainCodecForTests;

#[cfg(feature = "test-support")]
impl RecordCodec for InsecurePlainCodecForTests {
    fn seal(&self, _key: &RecordKey, plaintext: &[u8]) -> Vec<u8> {
        plaintext.to_vec()
    }

    fn open(&self, _key: &RecordKey, sealed: &[u8]) -> Result<Vec<u8>, StoreError> {
        Ok(sealed.to_vec())
    }
}

/// The Olm/Megolm/cross-signing/backup half of the working set — every
/// method the plan's §3.2 `CryptoStore` names, kept as this trait's method
/// set (see the module doc for what changed and why). Every value is an
/// opaque blob: the typed pickled objects these blobs will eventually hold
/// are later pieces' work (crypto/account.rs and friends), not this one's.
pub trait CryptoStore {
    /// The pickled Olm account, if one has been created yet.
    fn account(&self) -> Result<Option<&[u8]>, StoreError>;
    /// Persists the pickled Olm account, replacing any earlier one.
    fn save_account(&mut self, account: Vec<u8>) -> Result<(), StoreError>;

    /// Every pickled Olm session established with `their_curve25519`, in
    /// the order they were saved. Empty (never an error) if none exist yet.
    fn olm_sessions_for_device(&self, their_curve25519: &str) -> Result<&[Vec<u8>], StoreError>;
    /// Appends one more pickled Olm session for `their_curve25519`. This
    /// store has no way to tell "the same session, ratcheted forward" apart
    /// from "a brand new session" from an opaque blob alone — it always
    /// appends; a later piece (crypto's session manager) owns picking which
    /// of a peer's sessions to use and, if it wants in-place replacement
    /// rather than accumulation, must track that itself.
    fn save_olm_session(&mut self, their_curve25519: &str, session: Vec<u8>) -> Result<(), StoreError>;

    /// The pickled inbound Megolm session for `(room_id, session_id)`, if
    /// this device has ever received one.
    fn inbound_group_session(
        &self,
        room_id: &RoomId,
        session_id: &str,
    ) -> Result<Option<&[u8]>, StoreError>;
    /// Persists a pickled inbound Megolm session, replacing any earlier one
    /// for the same `(room_id, session_id)`.
    fn save_inbound_group_session(
        &mut self,
        room_id: &RoomId,
        session_id: &str,
        session: Vec<u8>,
    ) -> Result<(), StoreError>;

    /// The room's current pickled outbound Megolm session, if one exists.
    fn outbound_group_session(&self, room_id: &RoomId) -> Result<Option<&[u8]>, StoreError>;
    /// Persists the room's pickled outbound Megolm session, replacing any
    /// earlier one.
    fn save_outbound_group_session(&mut self, room_id: &RoomId, session: Vec<u8>) -> Result<(), StoreError>;

    /// The serialized device list this store has cached for `user_id`, if
    /// any.
    fn devices_for_user(&self, user_id: &UserId) -> Result<Option<&[u8]>, StoreError>;
    /// Replaces the cached device list for `user_id`.
    fn save_devices(&mut self, user_id: &UserId, devices: Vec<u8>) -> Result<(), StoreError>;

    /// Every user this device tracks device lists for, with its outdated
    /// flag.
    fn tracked_users(&self) -> Result<Vec<(UserId, bool)>, StoreError>;
    /// Starts (or updates) tracking `user_id`, recording whether its device
    /// list is known to be stale.
    fn mark_user_tracked(&mut self, user_id: &UserId, outdated: bool) -> Result<(), StoreError>;

    /// The withheld-reason code recorded for `(room_id, session_id)`, if
    /// any.
    fn withheld_reason(&self, room_id: &RoomId, session_id: &str) -> Result<Option<&[u8]>, StoreError>;
    /// Records a withheld-reason code for `(room_id, session_id)`.
    fn save_withheld(&mut self, room_id: &RoomId, session_id: &str, code: Vec<u8>) -> Result<(), StoreError>;

    /// The pickled key-backup material, if one has been set up.
    fn backup_key(&self) -> Result<Option<&[u8]>, StoreError>;
    /// Persists pickled key-backup material, replacing any earlier one.
    fn save_backup_key(&mut self, key: Vec<u8>) -> Result<(), StoreError>;

    /// The pickled cross-signing key material, if one has been set up.
    fn cross_signing_keys(&self) -> Result<Option<&[u8]>, StoreError>;
    /// Persists pickled cross-signing key material, replacing any earlier
    /// one.
    fn save_cross_signing_keys(&mut self, keys: Vec<u8>) -> Result<(), StoreError>;

    /// The peer's legacy DM X25519 public key, if this device has cached
    /// one.
    fn legacy_dm_peer_key(&self, user_id: &UserId) -> Result<Option<[u8; 32]>, StoreError>;
    /// Caches a peer's legacy DM X25519 public key.
    fn save_legacy_dm_peer_key(&mut self, user_id: &UserId, key: [u8; 32]) -> Result<(), StoreError>;
}

/// The sync/room half of the working set — every method the plan's §3.2
/// `StateStore` names, kept as this trait's method set. `pending_requests`
/// carries the already-concrete [`OutgoingRequest`] type this crate defines
/// (plan §5's `outgoing_queue::PendingRequest` is a later piece, M12; there
/// is nothing that type would add over the request itself at this layer).
pub trait StateStore {
    /// The `/sync` `next_batch` token from the most recent successful sync,
    /// if any.
    fn sync_token(&self) -> Result<Option<&str>, StoreError>;
    /// Persists the `/sync` `next_batch` token, replacing any earlier one.
    fn save_sync_token(&mut self, token: String) -> Result<(), StoreError>;

    /// The serialized state blob for `room_id`, if this store has one.
    fn room_state(&self, room_id: &RoomId) -> Result<Option<&[u8]>, StoreError>;
    /// Replaces the serialized state blob for `room_id`.
    fn save_room_state(&mut self, room_id: &RoomId, state: Vec<u8>) -> Result<(), StoreError>;
    /// Every room this store currently has state for.
    fn room_ids(&self) -> Result<Vec<RoomId>, StoreError>;

    /// Every outgoing request still waiting to be sent or acked.
    fn pending_requests(&self) -> Result<Vec<OutgoingRequest>, StoreError>;
    /// Records an outgoing request as pending (survives a crash/restart
    /// until [`StateStore::delete_pending_request`] removes it).
    fn save_pending_request(&mut self, req: OutgoingRequest) -> Result<(), StoreError>;
    /// Removes a pending request, e.g. once its response has arrived. A
    /// no-op if `id` is not currently pending.
    fn delete_pending_request(&mut self, id: &RequestId) -> Result<(), StoreError>;
}

const KIND_ACCOUNT: &str = "account";
const KIND_OLM_SESSION: &str = "olm_session";
const KIND_MEGOLM_IN: &str = "megolm_in";
const KIND_MEGOLM_OUT: &str = "megolm_out";
const KIND_DEVICES: &str = "devices";
const KIND_TRACKED_USER: &str = "tracked_user";
const KIND_WITHHELD: &str = "withheld";
const KIND_BACKUP_KEY: &str = "backup_key";
const KIND_CROSS_SIGNING_KEYS: &str = "cross_signing_keys";
const KIND_LEGACY_DM_PEER_KEY: &str = "legacy_dm_peer_key";
const KIND_SYNC_TOKEN: &str = "sync_token";
const KIND_ROOM_STATE: &str = "room_state";
const KIND_PENDING: &str = "pending";

fn account_record_key(device_id: &DeviceId) -> RecordKey {
    RecordKey::new(format!("messenger/{}/{KIND_ACCOUNT}", device_id.as_str()))
}

fn olm_session_record_key(device_id: &DeviceId, their_curve25519: &str, index: usize) -> RecordKey {
    RecordKey::new(format!(
        "messenger/{}/{KIND_OLM_SESSION}/{}/{index}",
        device_id.as_str(),
        percent_encode_segment(their_curve25519),
    ))
}

fn inbound_group_session_record_key(device_id: &DeviceId, room_id: &RoomId, session_id: &str) -> RecordKey {
    RecordKey::new(format!(
        "messenger/{}/{KIND_MEGOLM_IN}/{}/{}",
        device_id.as_str(),
        percent_encode_segment(room_id.as_str()),
        percent_encode_segment(session_id),
    ))
}

fn outbound_group_session_record_key(device_id: &DeviceId, room_id: &RoomId) -> RecordKey {
    RecordKey::new(format!(
        "messenger/{}/{KIND_MEGOLM_OUT}/{}",
        device_id.as_str(),
        percent_encode_segment(room_id.as_str()),
    ))
}

fn devices_record_key(device_id: &DeviceId, user_id: &UserId) -> RecordKey {
    RecordKey::new(format!(
        "messenger/{}/{KIND_DEVICES}/{}",
        device_id.as_str(),
        percent_encode_segment(user_id.as_str()),
    ))
}

fn tracked_user_record_key(device_id: &DeviceId, user_id: &UserId) -> RecordKey {
    RecordKey::new(format!(
        "messenger/{}/{KIND_TRACKED_USER}/{}",
        device_id.as_str(),
        percent_encode_segment(user_id.as_str()),
    ))
}

fn withheld_record_key(device_id: &DeviceId, room_id: &RoomId, session_id: &str) -> RecordKey {
    RecordKey::new(format!(
        "messenger/{}/{KIND_WITHHELD}/{}/{}",
        device_id.as_str(),
        percent_encode_segment(room_id.as_str()),
        percent_encode_segment(session_id),
    ))
}

fn backup_key_record_key(device_id: &DeviceId) -> RecordKey {
    RecordKey::new(format!("messenger/{}/{KIND_BACKUP_KEY}", device_id.as_str()))
}

fn cross_signing_keys_record_key(device_id: &DeviceId) -> RecordKey {
    RecordKey::new(format!("messenger/{}/{KIND_CROSS_SIGNING_KEYS}", device_id.as_str()))
}

fn legacy_dm_peer_key_record_key(device_id: &DeviceId, user_id: &UserId) -> RecordKey {
    RecordKey::new(format!(
        "messenger/{}/{KIND_LEGACY_DM_PEER_KEY}/{}",
        device_id.as_str(),
        percent_encode_segment(user_id.as_str()),
    ))
}

fn sync_token_record_key(device_id: &DeviceId) -> RecordKey {
    RecordKey::new(format!("messenger/{}/{KIND_SYNC_TOKEN}", device_id.as_str()))
}

fn room_state_record_key(device_id: &DeviceId, room_id: &RoomId) -> RecordKey {
    RecordKey::new(format!(
        "messenger/{}/{KIND_ROOM_STATE}/{}",
        device_id.as_str(),
        percent_encode_segment(room_id.as_str()),
    ))
}

fn pending_request_record_key(device_id: &DeviceId, request_id: &RequestId) -> RecordKey {
    RecordKey::new(format!(
        "messenger/{}/{KIND_PENDING}/{}",
        device_id.as_str(),
        percent_encode_segment(request_id.as_str()),
    ))
}

/// A sealed record's key, parsed into the record kind it names plus that
/// kind's decoded dynamic components. Produced by [`parse_record_key`].
enum RecognizedRecord {
    Account,
    OlmSession { their_curve25519: String, index: usize },
    InboundGroupSession { room_id: RoomId, session_id: String },
    OutboundGroupSession { room_id: RoomId },
    Devices { user_id: UserId },
    TrackedUser { user_id: UserId },
    Withheld { room_id: RoomId, session_id: String },
    BackupKey,
    CrossSigningKeys,
    LegacyDmPeerKey { user_id: UserId },
    SyncToken,
    RoomState { room_id: RoomId },
    PendingRequest { request_id: RequestId },
}

fn decode_room_id(encoded: &str, key: &RecordKey) -> Result<RoomId, StoreError> {
    let decoded = percent_decode_segment(encoded).ok_or_else(|| StoreError::unknown_layout(key))?;
    RoomId::parse(decoded).map_err(|_| StoreError::unknown_layout(key))
}

fn decode_user_id(encoded: &str, key: &RecordKey) -> Result<UserId, StoreError> {
    let decoded = percent_decode_segment(encoded).ok_or_else(|| StoreError::unknown_layout(key))?;
    UserId::parse(decoded).map_err(|_| StoreError::unknown_layout(key))
}

fn decode_plain_segment(encoded: &str, key: &RecordKey) -> Result<String, StoreError> {
    percent_decode_segment(encoded).ok_or_else(|| StoreError::unknown_layout(key))
}

/// Parses `key` into a [`RecognizedRecord`], scoped to `device_id`'s own
/// namespace. `Ok(None)` means `key`'s kind segment is not one this version
/// of the store recognizes at all — see the module doc's "Forward
/// compatibility" section for why that is not an error. `Err` means the
/// kind segment *is* recognized but the rest of `key` does not match that
/// kind's expected shape.
fn parse_record_key(device_id: &DeviceId, key: &RecordKey) -> Result<Option<RecognizedRecord>, StoreError> {
    let prefix = format!("messenger/{}/", device_id.as_str());
    let Some(rest) = key.as_str().strip_prefix(prefix.as_str()) else {
        return Ok(None);
    };
    let mut parts = rest.splitn(2, '/');
    let kind = parts.next().unwrap_or_default();
    let remainder = parts.next();
    let malformed = || StoreError::unknown_layout(key);

    match kind {
        KIND_ACCOUNT if remainder.is_none() => Ok(Some(RecognizedRecord::Account)),
        KIND_SYNC_TOKEN if remainder.is_none() => Ok(Some(RecognizedRecord::SyncToken)),
        KIND_BACKUP_KEY if remainder.is_none() => Ok(Some(RecognizedRecord::BackupKey)),
        KIND_CROSS_SIGNING_KEYS if remainder.is_none() => Ok(Some(RecognizedRecord::CrossSigningKeys)),
        KIND_OLM_SESSION => {
            let remainder = remainder.ok_or_else(malformed)?;
            let (curve_enc, index_str) = remainder.rsplit_once('/').ok_or_else(malformed)?;
            let index: usize = index_str.parse().map_err(|_| malformed())?;
            let their_curve25519 = decode_plain_segment(curve_enc, key)?;
            Ok(Some(RecognizedRecord::OlmSession { their_curve25519, index }))
        }
        KIND_MEGOLM_IN => {
            let remainder = remainder.ok_or_else(malformed)?;
            let (room_enc, session_enc) = remainder.split_once('/').ok_or_else(malformed)?;
            let room_id = decode_room_id(room_enc, key)?;
            let session_id = decode_plain_segment(session_enc, key)?;
            Ok(Some(RecognizedRecord::InboundGroupSession { room_id, session_id }))
        }
        KIND_MEGOLM_OUT => {
            let room_enc = remainder.ok_or_else(malformed)?;
            Ok(Some(RecognizedRecord::OutboundGroupSession { room_id: decode_room_id(room_enc, key)? }))
        }
        KIND_DEVICES => {
            let user_enc = remainder.ok_or_else(malformed)?;
            Ok(Some(RecognizedRecord::Devices { user_id: decode_user_id(user_enc, key)? }))
        }
        KIND_TRACKED_USER => {
            let user_enc = remainder.ok_or_else(malformed)?;
            Ok(Some(RecognizedRecord::TrackedUser { user_id: decode_user_id(user_enc, key)? }))
        }
        KIND_WITHHELD => {
            let remainder = remainder.ok_or_else(malformed)?;
            let (room_enc, session_enc) = remainder.split_once('/').ok_or_else(malformed)?;
            let room_id = decode_room_id(room_enc, key)?;
            let session_id = decode_plain_segment(session_enc, key)?;
            Ok(Some(RecognizedRecord::Withheld { room_id, session_id }))
        }
        KIND_LEGACY_DM_PEER_KEY => {
            let user_enc = remainder.ok_or_else(malformed)?;
            Ok(Some(RecognizedRecord::LegacyDmPeerKey { user_id: decode_user_id(user_enc, key)? }))
        }
        KIND_ROOM_STATE => {
            let room_enc = remainder.ok_or_else(malformed)?;
            Ok(Some(RecognizedRecord::RoomState { room_id: decode_room_id(room_enc, key)? }))
        }
        KIND_PENDING => {
            let request_enc = remainder.ok_or_else(malformed)?;
            let request_id = RequestId::from(decode_plain_segment(request_enc, key)?);
            Ok(Some(RecognizedRecord::PendingRequest { request_id }))
        }
        _ => Ok(None),
    }
}

fn decode_bool(bytes: &[u8], key: &RecordKey) -> Result<bool, StoreError> {
    match bytes {
        [0] => Ok(false),
        [1] => Ok(true),
        other => Err(StoreError::decode(key, format!("expected a single 0/1 byte, got {} bytes", other.len()))),
    }
}

fn decode_32_bytes(bytes: &[u8], key: &RecordKey) -> Result<[u8; 32], StoreError> {
    <[u8; 32]>::try_from(bytes).map_err(|_| StoreError::decode(key, format!("expected 32 bytes, got {}", bytes.len())))
}

/// The in-memory working set: every mutable piece of state
/// `mlc-messenger`'s core needs across a process lifetime, plus the
/// [`FlushGate`] that turns mutations into sealed records a shell persists.
/// Generic over the [`RecordCodec`] used to seal/open every record — see
/// the module doc's "Sealing is a codec, not a concrete algorithm" section.
#[derive(Debug)]
pub struct Store<C: RecordCodec> {
    device_id: DeviceId,
    codec: C,
    gate: FlushGate,
    account: Option<Vec<u8>>,
    olm_sessions: BTreeMap<String, Vec<Vec<u8>>>,
    inbound_group_sessions: BTreeMap<RoomId, BTreeMap<String, Vec<u8>>>,
    outbound_group_sessions: BTreeMap<RoomId, Vec<u8>>,
    devices: BTreeMap<UserId, Vec<u8>>,
    tracked_users: BTreeMap<UserId, bool>,
    withheld: BTreeMap<RoomId, BTreeMap<String, Vec<u8>>>,
    backup_key: Option<Vec<u8>>,
    cross_signing_keys: Option<Vec<u8>>,
    legacy_dm_peer_keys: BTreeMap<UserId, [u8; 32]>,
    sync_token: Option<String>,
    room_state: BTreeMap<RoomId, Vec<u8>>,
    pending_requests: BTreeMap<RequestId, OutgoingRequest>,
    /// Sealed bytes for keys no kind in this version recognizes, kept
    /// verbatim across a load — see the module doc's "Forward
    /// compatibility" section.
    unknown: BTreeMap<RecordKey, Vec<u8>>,
}

impl<C: RecordCodec> Store<C> {
    /// Builds an empty working set for `device_id`, backed by `codec` for
    /// every future flush export. Nothing is dirty yet — this is the
    /// brand-new-device case (plan §4.1's first unlock); a shell that
    /// already has records from an earlier session calls [`Store::load`]
    /// instead.
    pub fn new(device_id: DeviceId, codec: C) -> Self {
        Self {
            device_id,
            codec,
            gate: FlushGate::new(),
            account: None,
            olm_sessions: BTreeMap::new(),
            inbound_group_sessions: BTreeMap::new(),
            outbound_group_sessions: BTreeMap::new(),
            devices: BTreeMap::new(),
            tracked_users: BTreeMap::new(),
            withheld: BTreeMap::new(),
            backup_key: None,
            cross_signing_keys: None,
            legacy_dm_peer_keys: BTreeMap::new(),
            sync_token: None,
            room_state: BTreeMap::new(),
            pending_requests: BTreeMap::new(),
            unknown: BTreeMap::new(),
        }
    }

    /// Rebuilds a working set from sealed records a shell read back from
    /// durable storage (plan decision #3's `MessengerCore::open`). See the
    /// module doc's "Forward compatibility" section for what happens to a
    /// key this version of the store does not recognize, and this type's
    /// own doc for what a recognized-but-malformed or a
    /// recognized-but-uninterpretable key does.
    pub fn load(
        records: impl IntoIterator<Item = SealedRecord>,
        codec: C,
        device_id: DeviceId,
    ) -> Result<Self, StoreError> {
        let mut store = Self::new(device_id, codec);
        for record in records {
            match parse_record_key(&store.device_id, &record.key)? {
                Some(recognized) => {
                    let plaintext = store.codec.open(&record.key, &record.bytes)?;
                    store.apply_recognized(recognized, plaintext, &record.key)?;
                }
                None => {
                    store.gate.mark_dirty(record.key.clone(), record.bytes.clone());
                    store.unknown.insert(record.key, record.bytes);
                }
            }
        }
        Ok(store)
    }

    fn apply_recognized(
        &mut self,
        recognized: RecognizedRecord,
        plaintext: Vec<u8>,
        key: &RecordKey,
    ) -> Result<(), StoreError> {
        match recognized {
            RecognizedRecord::Account => self.account = Some(plaintext),
            RecognizedRecord::OlmSession { their_curve25519, index } => {
                let sessions = self.olm_sessions.entry(their_curve25519).or_default();
                if sessions.len() <= index {
                    sessions.resize(index + 1, Vec::new());
                }
                sessions[index] = plaintext;
            }
            RecognizedRecord::InboundGroupSession { room_id, session_id } => {
                self.inbound_group_sessions.entry(room_id).or_default().insert(session_id, plaintext);
            }
            RecognizedRecord::OutboundGroupSession { room_id } => {
                self.outbound_group_sessions.insert(room_id, plaintext);
            }
            RecognizedRecord::Devices { user_id } => {
                self.devices.insert(user_id, plaintext);
            }
            RecognizedRecord::TrackedUser { user_id } => {
                let outdated = decode_bool(&plaintext, key)?;
                self.tracked_users.insert(user_id, outdated);
            }
            RecognizedRecord::Withheld { room_id, session_id } => {
                self.withheld.entry(room_id).or_default().insert(session_id, plaintext);
            }
            RecognizedRecord::BackupKey => self.backup_key = Some(plaintext),
            RecognizedRecord::CrossSigningKeys => self.cross_signing_keys = Some(plaintext),
            RecognizedRecord::LegacyDmPeerKey { user_id } => {
                let key_bytes = decode_32_bytes(&plaintext, key)?;
                self.legacy_dm_peer_keys.insert(user_id, key_bytes);
            }
            RecognizedRecord::SyncToken => {
                let token = String::from_utf8(plaintext)
                    .map_err(|_| StoreError::decode(key, "sync token is not valid UTF-8"))?;
                self.sync_token = Some(token);
            }
            RecognizedRecord::RoomState { room_id } => {
                self.room_state.insert(room_id, plaintext);
            }
            RecognizedRecord::PendingRequest { request_id } => {
                let req: OutgoingRequest = serde_json::from_slice(&plaintext).map_err(|source| {
                    StoreError::decode(key, format!("pending request is not valid JSON: {source}"))
                })?;
                self.pending_requests.insert(request_id, req);
            }
        }
        Ok(())
    }

    fn mark_dirty(&mut self, key: RecordKey, plaintext: Vec<u8>) {
        let sealed = self.codec.seal(&key, &plaintext);
        self.gate.mark_dirty(key, sealed);
    }

    fn mark_deleted(&mut self, key: RecordKey) {
        self.gate.mark_deleted(key);
    }

    /// Drains everything dirtied since the previous call into one
    /// [`FlushBatch`] for a shell to write durably — see
    /// [`FlushGate::take_batch`].
    pub fn take_flush_batch(&mut self) -> Option<FlushBatch> {
        self.gate.take_batch()
    }

    /// Acknowledges that batch `id` has been durably written — see
    /// [`FlushGate::ack`].
    pub fn ack_flush(&mut self, id: u64) {
        self.gate.ack(id);
    }

    /// `true` iff it is safe to release outgoing requests right now — see
    /// [`FlushGate::can_release_requests`] for the exact guarantee this
    /// makes.
    pub fn can_release_requests(&self) -> bool {
        self.gate.can_release_requests()
    }
}

impl<C: RecordCodec> CryptoStore for Store<C> {
    fn account(&self) -> Result<Option<&[u8]>, StoreError> {
        Ok(self.account.as_deref())
    }

    fn save_account(&mut self, account: Vec<u8>) -> Result<(), StoreError> {
        let key = account_record_key(&self.device_id);
        self.mark_dirty(key, account.clone());
        self.account = Some(account);
        Ok(())
    }

    fn olm_sessions_for_device(&self, their_curve25519: &str) -> Result<&[Vec<u8>], StoreError> {
        Ok(self.olm_sessions.get(their_curve25519).map(Vec::as_slice).unwrap_or(&[]))
    }

    fn save_olm_session(&mut self, their_curve25519: &str, session: Vec<u8>) -> Result<(), StoreError> {
        let index = self.olm_sessions.get(their_curve25519).map_or(0, Vec::len);
        let key = olm_session_record_key(&self.device_id, their_curve25519, index);
        self.mark_dirty(key, session.clone());
        self.olm_sessions.entry(their_curve25519.to_string()).or_default().push(session);
        Ok(())
    }

    fn inbound_group_session(
        &self,
        room_id: &RoomId,
        session_id: &str,
    ) -> Result<Option<&[u8]>, StoreError> {
        Ok(self
            .inbound_group_sessions
            .get(room_id)
            .and_then(|sessions| sessions.get(session_id))
            .map(Vec::as_slice))
    }

    fn save_inbound_group_session(
        &mut self,
        room_id: &RoomId,
        session_id: &str,
        session: Vec<u8>,
    ) -> Result<(), StoreError> {
        let key = inbound_group_session_record_key(&self.device_id, room_id, session_id);
        self.mark_dirty(key, session.clone());
        self.inbound_group_sessions
            .entry(room_id.clone())
            .or_default()
            .insert(session_id.to_string(), session);
        Ok(())
    }

    fn outbound_group_session(&self, room_id: &RoomId) -> Result<Option<&[u8]>, StoreError> {
        Ok(self.outbound_group_sessions.get(room_id).map(Vec::as_slice))
    }

    fn save_outbound_group_session(&mut self, room_id: &RoomId, session: Vec<u8>) -> Result<(), StoreError> {
        let key = outbound_group_session_record_key(&self.device_id, room_id);
        self.mark_dirty(key, session.clone());
        self.outbound_group_sessions.insert(room_id.clone(), session);
        Ok(())
    }

    fn devices_for_user(&self, user_id: &UserId) -> Result<Option<&[u8]>, StoreError> {
        Ok(self.devices.get(user_id).map(Vec::as_slice))
    }

    fn save_devices(&mut self, user_id: &UserId, devices: Vec<u8>) -> Result<(), StoreError> {
        let key = devices_record_key(&self.device_id, user_id);
        self.mark_dirty(key, devices.clone());
        self.devices.insert(user_id.clone(), devices);
        Ok(())
    }

    fn tracked_users(&self) -> Result<Vec<(UserId, bool)>, StoreError> {
        Ok(self.tracked_users.iter().map(|(user_id, outdated)| (user_id.clone(), *outdated)).collect())
    }

    fn mark_user_tracked(&mut self, user_id: &UserId, outdated: bool) -> Result<(), StoreError> {
        let key = tracked_user_record_key(&self.device_id, user_id);
        self.mark_dirty(key, vec![u8::from(outdated)]);
        self.tracked_users.insert(user_id.clone(), outdated);
        Ok(())
    }

    fn withheld_reason(&self, room_id: &RoomId, session_id: &str) -> Result<Option<&[u8]>, StoreError> {
        Ok(self.withheld.get(room_id).and_then(|sessions| sessions.get(session_id)).map(Vec::as_slice))
    }

    fn save_withheld(&mut self, room_id: &RoomId, session_id: &str, code: Vec<u8>) -> Result<(), StoreError> {
        let key = withheld_record_key(&self.device_id, room_id, session_id);
        self.mark_dirty(key, code.clone());
        self.withheld.entry(room_id.clone()).or_default().insert(session_id.to_string(), code);
        Ok(())
    }

    fn backup_key(&self) -> Result<Option<&[u8]>, StoreError> {
        Ok(self.backup_key.as_deref())
    }

    fn save_backup_key(&mut self, key: Vec<u8>) -> Result<(), StoreError> {
        let record_key = backup_key_record_key(&self.device_id);
        self.mark_dirty(record_key, key.clone());
        self.backup_key = Some(key);
        Ok(())
    }

    fn cross_signing_keys(&self) -> Result<Option<&[u8]>, StoreError> {
        Ok(self.cross_signing_keys.as_deref())
    }

    fn save_cross_signing_keys(&mut self, keys: Vec<u8>) -> Result<(), StoreError> {
        let key = cross_signing_keys_record_key(&self.device_id);
        self.mark_dirty(key, keys.clone());
        self.cross_signing_keys = Some(keys);
        Ok(())
    }

    fn legacy_dm_peer_key(&self, user_id: &UserId) -> Result<Option<[u8; 32]>, StoreError> {
        Ok(self.legacy_dm_peer_keys.get(user_id).copied())
    }

    fn save_legacy_dm_peer_key(&mut self, user_id: &UserId, key: [u8; 32]) -> Result<(), StoreError> {
        let record_key = legacy_dm_peer_key_record_key(&self.device_id, user_id);
        self.mark_dirty(record_key, key.to_vec());
        self.legacy_dm_peer_keys.insert(user_id.clone(), key);
        Ok(())
    }
}

impl<C: RecordCodec> StateStore for Store<C> {
    fn sync_token(&self) -> Result<Option<&str>, StoreError> {
        Ok(self.sync_token.as_deref())
    }

    fn save_sync_token(&mut self, token: String) -> Result<(), StoreError> {
        let key = sync_token_record_key(&self.device_id);
        self.mark_dirty(key, token.clone().into_bytes());
        self.sync_token = Some(token);
        Ok(())
    }

    fn room_state(&self, room_id: &RoomId) -> Result<Option<&[u8]>, StoreError> {
        Ok(self.room_state.get(room_id).map(Vec::as_slice))
    }

    fn save_room_state(&mut self, room_id: &RoomId, state: Vec<u8>) -> Result<(), StoreError> {
        let key = room_state_record_key(&self.device_id, room_id);
        self.mark_dirty(key, state.clone());
        self.room_state.insert(room_id.clone(), state);
        Ok(())
    }

    fn room_ids(&self) -> Result<Vec<RoomId>, StoreError> {
        Ok(self.room_state.keys().cloned().collect())
    }

    fn pending_requests(&self) -> Result<Vec<OutgoingRequest>, StoreError> {
        Ok(self.pending_requests.values().cloned().collect())
    }

    fn save_pending_request(&mut self, req: OutgoingRequest) -> Result<(), StoreError> {
        let key = pending_request_record_key(&self.device_id, &req.id);
        let plaintext = serde_json::to_vec(&req)
            .map_err(|source| StoreError::decode(&key, format!("failed to serialize pending request: {source}")))?;
        self.mark_dirty(key, plaintext);
        self.pending_requests.insert(req.id.clone(), req);
        Ok(())
    }

    fn delete_pending_request(&mut self, id: &RequestId) -> Result<(), StoreError> {
        if self.pending_requests.remove(id).is_some() {
            let key = pending_request_record_key(&self.device_id, id);
            self.mark_deleted(key);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{DeviceId, RequestId, RoomId, UserId};

    fn device_id() -> DeviceId {
        DeviceId::parse("DEV1").expect("valid device id")
    }

    fn room_id() -> RoomId {
        RoomId::parse("!room:example.org").expect("valid room id")
    }

    fn user_id() -> UserId {
        UserId::parse("@alice:example.org").expect("valid user id")
    }

    #[test]
    fn in_memory_store_round_trips_account() {
        let mut store = Store::new(device_id(), InsecurePlainCodecForTests);
        assert_eq!(store.account().expect("no error"), None);

        store.save_account(b"pickled-account".to_vec()).expect("save succeeds");
        assert_eq!(store.account().expect("no error"), Some(b"pickled-account".as_slice()));
    }

    #[test]
    fn in_memory_store_round_trips_pending_requests() {
        let mut store = Store::new(device_id(), InsecurePlainCodecForTests);
        assert!(store.pending_requests().expect("no error").is_empty());

        let req = OutgoingRequest::room_members(RequestId::next(0), &room_id());
        store.save_pending_request(req.clone()).expect("save succeeds");
        assert_eq!(store.pending_requests().expect("no error"), vec![req]);
    }

    #[test]
    fn round_trip_every_record_kind_through_load() {
        let device = device_id();
        let room = room_id();
        let user = user_id();
        let mut store = Store::new(device.clone(), InsecurePlainCodecForTests);

        store.save_account(b"account-bytes".to_vec()).expect("save account");
        store.save_olm_session("curve25519-peer", b"olm-session-bytes".to_vec()).expect("save olm session");
        store
            .save_inbound_group_session(&room, "session-1", b"megolm-in-bytes".to_vec())
            .expect("save inbound group session");
        store
            .save_outbound_group_session(&room, b"megolm-out-bytes".to_vec())
            .expect("save outbound group session");
        store.save_devices(&user, b"devices-bytes".to_vec()).expect("save devices");
        store.mark_user_tracked(&user, true).expect("mark tracked");
        store.save_withheld(&room, "session-1", b"withheld-bytes".to_vec()).expect("save withheld");
        store.save_backup_key(b"backup-bytes".to_vec()).expect("save backup key");
        store.save_cross_signing_keys(b"xsign-bytes".to_vec()).expect("save cross signing keys");
        store.save_legacy_dm_peer_key(&user, [7u8; 32]).expect("save legacy dm peer key");
        store.save_sync_token("s123".to_string()).expect("save sync token");
        store.save_room_state(&room, b"room-state-bytes".to_vec()).expect("save room state");
        let req = OutgoingRequest::room_members(RequestId::next(0), &room);
        store.save_pending_request(req.clone()).expect("save pending request");

        let batch = store.take_flush_batch().expect("everything above dirtied something");
        assert!(batch.deletes.is_empty());

        let loaded =
            Store::load(batch.records, InsecurePlainCodecForTests, device).expect("load succeeds");

        assert_eq!(loaded.account().expect("no error"), Some(b"account-bytes".as_slice()));
        assert_eq!(
            loaded.olm_sessions_for_device("curve25519-peer").expect("no error"),
            &[b"olm-session-bytes".to_vec()]
        );
        assert_eq!(
            loaded.inbound_group_session(&room, "session-1").expect("no error"),
            Some(b"megolm-in-bytes".as_slice())
        );
        assert_eq!(
            loaded.outbound_group_session(&room).expect("no error"),
            Some(b"megolm-out-bytes".as_slice())
        );
        assert_eq!(loaded.devices_for_user(&user).expect("no error"), Some(b"devices-bytes".as_slice()));
        assert_eq!(loaded.tracked_users().expect("no error"), vec![(user.clone(), true)]);
        assert_eq!(
            loaded.withheld_reason(&room, "session-1").expect("no error"),
            Some(b"withheld-bytes".as_slice())
        );
        assert_eq!(loaded.backup_key().expect("no error"), Some(b"backup-bytes".as_slice()));
        assert_eq!(loaded.cross_signing_keys().expect("no error"), Some(b"xsign-bytes".as_slice()));
        assert_eq!(loaded.legacy_dm_peer_key(&user).expect("no error"), Some([7u8; 32]));
        assert_eq!(loaded.sync_token().expect("no error"), Some("s123"));
        assert_eq!(loaded.room_state(&room).expect("no error"), Some(b"room-state-bytes".as_slice()));
        assert_eq!(loaded.pending_requests().expect("no error"), vec![req]);
    }

    #[test]
    fn delete_is_exported_and_applied_on_load() {
        let device = device_id();
        let room = room_id();
        let mut store = Store::new(device.clone(), InsecurePlainCodecForTests);

        let req = OutgoingRequest::room_members(RequestId::next(0), &room);
        store.save_pending_request(req.clone()).expect("save pending request");
        let write_batch = store.take_flush_batch().expect("pending request save is pending");
        store.ack_flush(write_batch.id);

        store.delete_pending_request(&req.id).expect("delete succeeds");
        let delete_batch = store.take_flush_batch().expect("deletion is pending");
        assert_eq!(delete_batch.deletes, vec![pending_request_record_key(&device, &req.id)]);
        store.ack_flush(delete_batch.id);

        // Simulate a shell applying the delta to its own durable key-value
        // set: writes land, then tombstoned keys are removed.
        let mut durable: BTreeMap<RecordKey, Vec<u8>> =
            write_batch.records.into_iter().map(|record| (record.key, record.bytes)).collect();
        for key in delete_batch.deletes {
            durable.remove(&key);
        }
        let records = durable.into_iter().map(|(key, bytes)| SealedRecord { key, bytes });

        let loaded = Store::load(records, InsecurePlainCodecForTests, device).expect("load succeeds");
        assert!(loaded.pending_requests().expect("no error").is_empty());
    }

    #[test]
    fn unknown_keys_survive_a_load_and_reflush_untouched() {
        let device = device_id();
        let unknown_key = RecordKey::new(format!("messenger/{}/future_field/whatever", device.as_str()));
        let unknown_bytes = b"opaque-future-bytes".to_vec();
        let records = vec![SealedRecord { key: unknown_key.clone(), bytes: unknown_bytes.clone() }];

        let mut store =
            Store::load(records, InsecurePlainCodecForTests, device).expect("unknown keys don't fail a load");

        let batch = store.take_flush_batch().expect("the unknown record is re-adopted into the next flush");
        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.records[0].key, unknown_key);
        assert_eq!(batch.records[0].bytes, unknown_bytes, "re-emitted byte-for-byte, not re-sealed");

        store.ack_flush(batch.id);
        assert!(store.can_release_requests());
    }

    #[test]
    fn corrupt_record_fails_load_naming_the_key() {
        let device = device_id();
        let key = sync_token_record_key(&device);
        let corrupt = SealedRecord { key: key.clone(), bytes: vec![0xFF, 0xFE] };

        let err = Store::load(vec![corrupt], InsecurePlainCodecForTests, device).unwrap_err();
        assert!(matches!(err, StoreError::Decode { .. }));
        assert!(err.to_string().contains(key.as_str()), "error names the offending key: {err}");
    }

    #[test]
    fn malformed_known_prefix_key_is_a_hard_error() {
        let device = device_id();
        // "olm_session" is a recognized kind, but this key is missing both
        // the curve-key and index components.
        let key = RecordKey::new(format!("messenger/{}/olm_session", device.as_str()));
        let record = SealedRecord { key: key.clone(), bytes: b"whatever".to_vec() };

        let err = Store::load(vec![record], InsecurePlainCodecForTests, device).unwrap_err();
        assert!(matches!(err, StoreError::UnknownRecordLayout { .. }));
        assert!(err.to_string().contains(key.as_str()));
    }

    #[test]
    fn mutations_block_request_release_until_acked() {
        let mut store = Store::new(device_id(), InsecurePlainCodecForTests);
        assert!(store.can_release_requests(), "nothing dirtied yet");

        store.save_account(b"account-bytes".to_vec()).expect("save succeeds");
        assert!(!store.can_release_requests(), "dirty and not yet flushed");

        let batch = store.take_flush_batch().expect("account save is pending");
        assert!(!store.can_release_requests(), "flushed but not yet acked");

        store.ack_flush(batch.id);
        assert!(store.can_release_requests(), "flushed and acked: safe to release");
    }
}
