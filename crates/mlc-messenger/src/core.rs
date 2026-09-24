//! [`MessengerCore`] — the single-writer kernel tying every earlier piece
//! together (plan §3's `lib.rs`/`sync_engine.rs`, work piece M13a): the
//! `/sync` loop, to-device/device-list/room ingestion, Megolm decrypt, and
//! the receive-side [`MessengerCommand`] variants. The send pipeline
//! (`SendMessage`, `CreateRoom`, room-key sharing triggered by a local
//! send, ...) is M13b, a later piece — see each item below for exactly
//! what is and is not in scope here.
//!
//! # Kernel API shape
//!
//! [`MessengerCore::open`] rebuilds a core from whatever a shell already
//! has durably stored (sealed records) plus fresh, caller-supplied config.
//! From there, a shell drives one tick at a time:
//!
//! 1. [`MessengerCore::releasable_requests`] — everything safe to send
//!    right now (the flush-before-send barrier already applied).
//! 2. Execute each, feed the result back via [`MessengerCore::on_response`]
//!    or [`MessengerCore::on_transport_error`].
//! 3. [`MessengerCore::take_flush_batch`]/[`MessengerCore::ack_flush`] —
//!    persist whatever mutated, ack once durable.
//! 4. [`MessengerCore::events`] — drain whatever became visible, to diff
//!    against the adapter's own last-rendered snapshot.
//!
//! No function here calls a clock or an RNG itself (crate doc, `ids`/
//! `outgoing_queue`'s own module docs): every entry point that needs time
//! takes `now_ms` explicitly, and retry jitter comes from the
//! [`crate::outgoing_queue::Jitter`] supplied to [`MessengerCore::open`].
//!
//! # Counters that must never repeat across a restart
//!
//! [`crate::ids::RequestId`]/[`crate::ids::TxnId`] are minted from one
//! monotonic seed per device session (`ids`'s own module doc) — this
//! module is that seed's one owner, persisted as [`Counters`]. A bump is
//! written through [`crate::store::StateStore::save_counters`] in the same
//! synchronous call that mints the id ([`MessengerCore::next_request_id`]),
//! which is what lets the ordinary flush-before-send barrier
//! (`crate::persist`'s module doc) cover it for free: the very next
//! [`crate::outgoing_queue::OutgoingQueue::enqueue`] call this id feeds
//! into calls [`crate::persist::FlushEpoch::seal_for_request`] itself,
//! which folds the counter's own not-yet-batched dirty write into the same
//! batch the new request's `required_seq` waits on. A reused id after a
//! crash would be silently deduplicated by the server into the OLD
//! event/request — the exact "resend a `/keys/claim`, waste one OTK" cost
//! [`crate::outgoing_queue`]'s own module doc accepts is very different
//! from resending a `TxnId` a room member already has under a different
//! plaintext, which is why this restore rule is load-bearing, not
//! defensive polish.
//!
//! [`MessengerCore::open`] restores each sequence to
//! `max(persisted, 1 + highest seed still embedded in a pending request)`
//! — see [`restore_counter`]'s own doc for why the `pending` half of that
//! max matters at all given the write-before-mint discipline above (belt
//! and suspenders, not the primary guarantee). Only [`crate::ids::RequestId`]
//! has a structural "still pending" source to scan
//! ([`crate::store::StateStore::pending_requests`] — every pending request
//! carries its own id); [`crate::ids::TxnId`] does not (a `TxnId` is baked
//! into a request's `path`/`body`, never a distinct field), so its restore
//! relies solely on the persisted value. This piece never itself mints a
//! `TxnId` (nothing in the receive path needs one — sending is M13b's job);
//! the mechanism is proven now via [`Counters`]'s own persistence and
//! [`restore_counter`]'s pure logic so M13b's `SendMessage` needs to add
//! nothing but a `next_txn_id` counterpart to
//! [`MessengerCore::next_request_id`].
//!
//! # Sync ingestion order
//!
//! One `/sync` request is ever outstanding at a time
//! ([`crate::outgoing_queue::Lane::Sync`]'s own cap), 30s long-poll,
//! `since` = the persisted [`crate::store::StateStore::sync_token`] (initial
//! sync when `None`). On a successful response, in order: (1) to-device
//! events — Olm-decrypt, route `m.room_key` into
//! [`crate::crypto::group_sessions::GroupSessionManager`], drop a
//! replayed/otherwise-unrecoverable decrypt silently, keep an
//! [`crate::crypto::olm_sessions::OlmDecryptError::UnknownSenderDevice`] in
//! a small bounded retry list re-tried after the next `/keys/query`. That
//! retry has a real ceiling worth naming: [`crate::crypto::olm_sessions::
//! OlmSessionManager::decrypt_to_device`]'s landed (M6) shape already
//! advances and persists the Olm ratchet -- consuming a pre-key message's
//! one-time key in the process -- before it ever reaches the sender-device
//! check that can raise `UnknownSenderDevice`. Retrying that SAME queued
//! event later is therefore a genuine second decrypt of already-consumed
//! ratchet state, which Olm's own forward-secrecy guarantee rejects as a
//! replay (`crypto::olm_sessions`'s own module doc) -- the retry drains the
//! queue entry either way (resolved, or dropped as unrecoverable), but it
//! only actually resolves the message for a sender whose device becomes
//! known before this exact event is retried, not after. Closing that gap
//! would need `decrypt_to_device` to return validated plaintext pending a
//! sender check instead of an error carrying no plaintext at all -- a
//! shape change to that landed piece, out of scope here.
//! `m.room_key.withheld`/`m.room_key_request`/`m.forwarded_room_key` are a
//! no-op hook for `crypto::withheld` (M8); (2) `device_lists.changed`/
//! `left` → [`crate::crypto::device_tracker::DeviceTracker`]; (3) OTK
//! counts / unused fallback types →
//! [`crate::crypto::account::OlmAccountState::on_sync_counts`], may enqueue
//! `/keys/upload`; (4) any outdated tracked user → one `/keys/query`; (5)
//! rooms join/invite/leave — state → [`crate::room::state::RoomState`],
//! timeline → [`crate::room::timeline::Timeline::apply_timeline_batch`],
//! Megolm events decrypted via
//! [`crate::crypto::group_sessions::GroupSessionManager::decrypt_event`] →
//! [`crate::room::timeline::Timeline::set_decrypted`] or
//! `Undecryptable{reason}`; ephemeral typing/receipts; room account data;
//! summary + unread; (6) global account data (`m.direct`) → later read by
//! [`MessengerCore::room_kind`]; (7) the new sync token is persisted LAST,
//! after everything it covers. When a room key newly arrives (step 1),
//! every `Undecryptable` item across every loaded timeline whose session id
//! now matches is retried automatically, in addition to the caller's own
//! [`MessengerCommand::RetryDecryption`].
//!
//! `org.mylittlechart.legacy_dm` items are left exactly as
//! [`crate::room::timeline::Timeline::apply_timeline_batch`] already leaves
//! them (`ItemContent::LegacyDm`) — decrypting them is `legacy_dm.rs`'s job
//! (M11), not wired in here.

use crate::crypto::account::OlmAccountState;
use crate::crypto::device_tracker::DeviceTracker;
use crate::crypto::group_sessions::GroupSessionManager;
use crate::crypto::olm_sessions::{OlmDecryptError, OlmSessionManager};
use crate::error::MessengerError;
use crate::ids::{DeviceId, EventId, RequestId, RoomId, UserId};
use crate::outgoing_queue::{Jitter, Lane, OutgoingQueue, ResponseOutcome};
use crate::persist::{FlushBatch, SealedRecord};
use crate::room::state::RoomState;
use crate::room::timeline::{interpret_content, ItemContent, Timeline};
use crate::room::RoomKind;
use crate::store::{CryptoStore, RecordCodec, StateStore, Store};
use crate::wire::events::{
    DirectContent, MegolmEncryptedContent, RawEvent, ReceiptContent, RoomEncryptedContent, RoomKeyContent,
    StrippedStateEvent, ToDeviceEvent, TypingContent, Unsigned,
};
use crate::wire::sync::{parse_sync_response, InvitedRoom, JoinedRoom, LeftRoom};
use crate::wire::{HttpResponseDescriptor, OutgoingRequest, OutgoingRequestKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

/// `/sync`'s own long-poll timeout (research doc §1.4's convention).
const SYNC_TIMEOUT_MS: u64 = 30_000;
/// `GET /rooms/{roomId}/messages`'s page size for
/// [`MessengerCommand::LoadOlder`].
const LOAD_OLDER_PAGE_SIZE: u32 = 50;
/// How long a `typing: true` indicator asks the server to hold before
/// auto-clearing it (client-server API's own `timeout` field).
const TYPING_TIMEOUT_MS: u64 = 30_000;
/// How many to-device events with an as-yet-[`OlmDecryptError::UnknownSenderDevice`]
/// sender this core holds for a retry after the next `/keys/query`, oldest
/// evicted first — same bounded-FIFO doctrine as
/// [`crate::room::timeline`]'s own pending-relation queue.
const UNKNOWN_SENDER_RETRY_CAPACITY: usize = 256;

/// This device's own [`crate::ids::RequestId`]/[`crate::ids::TxnId`]
/// minting sequences — see this module's own doc, "Counters that must
/// never repeat across a restart".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counters {
    /// The next, never-yet-handed-out [`crate::ids::RequestId`] seed.
    pub next_request_id: u64,
    /// The next, never-yet-handed-out [`crate::ids::TxnId`] seed. Not
    /// minted from anywhere in this piece yet (see this module's own doc)
    /// — restored and persisted symmetrically with `next_request_id` so
    /// M13b's send pipeline needs no new persistence of its own.
    pub next_txn_id: u64,
}

/// Computes the value a counter must resume from after a restart: never
/// below what was durably persisted, and never low enough to reissue an id
/// a still-pending request already carries. `max_used_seed` is the highest
/// seed [`MessengerCore::open`] found embedded in a request
/// [`crate::store::StateStore::pending_requests`] is still tracking (`None`
/// when nothing is pending, or when this counter has no such structural
/// source to scan at all — see this module's own doc for
/// [`crate::ids::TxnId`]'s case). This is a defensive second check, not the
/// primary guarantee: under the write-before-mint discipline
/// [`MessengerCore::next_request_id`] follows, the persisted value alone
/// should already dominate every pending id.
fn restore_counter(persisted_next: u64, max_used_seed: Option<u64>) -> u64 {
    match max_used_seed {
        Some(seed) => persisted_next.max(seed.saturating_add(1)),
        None => persisted_next,
    }
}

/// Static identity a shell supplies once at [`MessengerCore::open`] time —
/// never re-derived, never changed for the lifetime of a device session
/// (plan §4.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreConfig {
    /// This account's own user id.
    pub user_id: UserId,
    /// This device's own, server-assigned device id.
    pub device_id: DeviceId,
    /// The homeserver's own server name — not consumed by this piece's own
    /// logic yet (every id this core builds is either read whole off the
    /// wire or supplied by a caller who already has one), kept on
    /// [`CoreConfig`] because a shell already has to know it to reach the
    /// homeserver at all, and a later piece (room creation, M13b) will need
    /// it to mint a bare local part into a full room alias.
    pub server_name: String,
}

/// A receive-side command a shell/adapter dispatches into the core.
/// Send-side variants (`SendMessage`, `CreateRoom`, ...) are M13b — see
/// this crate's module doc.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessengerCommand {
    /// Sets this room's read-up-to marker to `event_id` (`POST
    /// /rooms/{roomId}/read_markers`).
    MarkRead {
        /// The room being marked read.
        room_id: RoomId,
        /// The last event the user has seen.
        event_id: EventId,
    },
    /// Starts or stops this device's own typing indicator in a room (`PUT
    /// /rooms/{roomId}/typing/{userId}`).
    SetTyping {
        /// The room to set the indicator in.
        room_id: RoomId,
        /// `true` to start, `false` to clear.
        typing: bool,
    },
    /// Re-attempts decrypting one currently-undecryptable timeline item
    /// (e.g. after the user manually re-verifies a device) — the same
    /// retry this core already runs automatically the moment a matching
    /// room key arrives (this module's own doc).
    RetryDecryption {
        /// The room the item lives in.
        room_id: RoomId,
        /// The item's event id.
        event_id: EventId,
    },
    /// Fetches one older page of a room's timeline (`GET
    /// /rooms/{roomId}/messages?dir=b`), prepending it once the response
    /// arrives. A no-op if this room has no further history to page into
    /// (no gap, no back-pagination token).
    LoadOlder {
        /// The room to page further back into.
        room_id: RoomId,
    },
}

/// One user-visible change a shell/adapter should re-read from a snapshot
/// getter. See [`MessengerCore::events`] for delivery semantics and
/// [`MessengerCore::change_counter`] for the cheap "did anything change at
/// all" signal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessengerEvent {
    /// The room list itself changed shape this tick (a room appeared via
    /// join/invite/leave, or an already-known room's own state changed).
    RoomsChanged,
    /// `room_id`'s timeline gained or updated an item.
    TimelineChanged {
        /// The affected room.
        room_id: RoomId,
    },
    /// `room_id`'s typing-user set changed.
    TypingChanged {
        /// The affected room.
        room_id: RoomId,
    },
    /// `room_id`'s read-receipt set changed.
    ReceiptsChanged {
        /// The affected room.
        room_id: RoomId,
    },
    /// `room_id`'s server-reported unread count changed.
    UnreadChanged {
        /// The affected room.
        room_id: RoomId,
    },
    /// A previously-known device's Curve25519/Ed25519 keys changed — a
    /// security event a UI should surface, never silently apply (see
    /// [`crate::crypto::device_tracker`]'s own module doc for why the OLD
    /// keys are what this core keeps).
    DeviceKeyChanged {
        /// The device's owner.
        user_id: UserId,
        /// The device whose keys changed.
        device_id: DeviceId,
    },
}

/// One Megolm room event this core could not decrypt yet, kept so a later
/// room-key arrival or an explicit [`MessengerCommand::RetryDecryption`]
/// can re-attempt it without re-fetching anything — see this module's own
/// "Sync ingestion order" doc.
#[derive(Clone)]
struct PendingDecryptItem {
    session_id: String,
    sender: UserId,
    origin_server_ts: i64,
    content: MegolmEncryptedContent,
}

/// `GET /rooms/{roomId}/messages`'s response shape, the parts this module
/// reads (research doc §1.5) — `chunk` arrives newest-first for a `dir=b`
/// page, reversed by [`MessengerCore::ingest_room_messages_response`]
/// before [`crate::room::timeline::Timeline::prepend_back_page`] (which
/// wants oldest-first, matching its own doc).
#[derive(Deserialize)]
struct RoomMessagesResponseBody {
    #[serde(default)]
    chunk: Vec<RawEvent>,
    #[serde(default)]
    end: Option<String>,
}

/// The single-writer kernel — see this module's own doc.
pub struct MessengerCore<C: RecordCodec> {
    config: CoreConfig,
    store: Store<C>,
    account: OlmAccountState,
    outgoing: OutgoingQueue,
    counters: Counters,
    jitter: Box<dyn Jitter>,

    rooms: BTreeMap<RoomId, RoomState>,
    timelines: BTreeMap<RoomId, Timeline>,
    direct_account_data: Option<DirectContent>,
    global_account_data: BTreeMap<String, serde_json::Value>,
    room_account_data: BTreeMap<RoomId, BTreeMap<String, serde_json::Value>>,
    typing: BTreeMap<RoomId, Vec<UserId>>,
    receipts: BTreeMap<RoomId, ReceiptContent>,

    pending_decrypt: BTreeMap<RoomId, BTreeMap<EventId, PendingDecryptItem>>,
    unknown_sender_retry: VecDeque<ToDeviceEvent>,

    /// Every [`OutgoingRequestKind`] this core itself minted and is still
    /// waiting on, keyed by the [`RequestId`] it enqueued it under —
    /// [`crate::outgoing_queue::OutgoingQueue`] does not expose a way to
    /// look this back up given only an id, so this core tracks it
    /// alongside instead (see [`MessengerCore::enqueue_request`]).
    pending_kinds: BTreeMap<RequestId, OutgoingRequestKind>,
    /// Which room a still-pending `RoomMessages` request was paginating —
    /// same reasoning as `pending_kinds`.
    pending_room_messages: BTreeMap<RequestId, RoomId>,
    /// The currently outstanding `/sync` request's id, if any — this
    /// core's own single-flight tracking (this module's own doc).
    sync_request_id: Option<RequestId>,
    /// [`FlushBatch`]es [`crate::outgoing_queue::OutgoingQueue::enqueue`]
    /// already drained from the store's own pending set (to seal a new
    /// request's [`crate::persist::RequiredSeq`]) but that a shell has not
    /// yet drained via [`MessengerCore::take_flush_batch`] — see
    /// [`MessengerCore::enqueue_request`]'s own doc for why this can't
    /// simply be re-derived from `store.take_flush_batch()` later.
    pending_flush_batches: VecDeque<FlushBatch>,

    events: Vec<MessengerEvent>,
    change_counter: u64,
}

impl<C: RecordCodec> MessengerCore<C> {
    /// Rebuilds a core from whatever a shell already read back from
    /// durable storage (`records`, possibly empty for a brand-new device),
    /// `config`, and a `jitter` source for
    /// [`crate::outgoing_queue::OutgoingQueue`]'s own retry backoff. `now_ms`
    /// is accepted for API symmetry with every other entry point (this
    /// crate's own "no clock inside" rule) but not read by anything in this
    /// piece's own restore logic yet — timeline content itself is never
    /// persisted (re-fetched off the next `/sync`/back-page instead, this
    /// module's own doc), so there is nothing time-sensitive to restore
    /// here before a later piece needs one.
    pub fn open(
        records: impl IntoIterator<Item = SealedRecord>,
        codec: C,
        config: CoreConfig,
        _now_ms: i64,
        jitter: Box<dyn Jitter>,
    ) -> Result<Self, MessengerError> {
        let mut store = Store::load(records, codec, config.device_id.clone())?;
        let outgoing = OutgoingQueue::load(&store)?;
        let account = OlmAccountState::load_or_create(&mut store)?;

        let mut rooms = BTreeMap::new();
        let mut timelines = BTreeMap::new();
        for room_id in store.room_ids()? {
            if let Some(bytes) = store.room_state(&room_id)? {
                let state: RoomState = serde_json::from_slice(bytes)
                    .map_err(|source| MessengerError::Crypto(format!("decode room state for {room_id}: {source}")))?;
                rooms.insert(room_id.clone(), state);
                timelines.insert(room_id, Timeline::new());
            }
        }

        let pending_requests = store.pending_requests()?;
        let max_pending_request_seed = pending_requests.iter().filter_map(|req| req.request.id.as_seed()).max();
        let persisted_counters = store.counters()?.unwrap_or_default();
        let counters = Counters {
            next_request_id: restore_counter(persisted_counters.next_request_id, max_pending_request_seed),
            next_txn_id: restore_counter(persisted_counters.next_txn_id, None),
        };
        if counters != persisted_counters {
            // Restoration actually advanced a value beyond what was on
            // disk (the defensive "pending" half of `restore_counter`
            // fired) -- persist the advanced value immediately so it, in
            // turn, can never be handed out a second time by a future
            // restart.
            store.save_counters(counters)?;
        }

        Ok(Self {
            config,
            store,
            account,
            outgoing,
            counters,
            jitter,
            rooms,
            timelines,
            direct_account_data: None,
            global_account_data: BTreeMap::new(),
            room_account_data: BTreeMap::new(),
            typing: BTreeMap::new(),
            receipts: BTreeMap::new(),
            pending_decrypt: BTreeMap::new(),
            unknown_sender_retry: VecDeque::new(),
            pending_kinds: BTreeMap::new(),
            pending_room_messages: BTreeMap::new(),
            sync_request_id: None,
            pending_flush_batches: VecDeque::new(),
            events: Vec::new(),
            change_counter: 0,
        })
    }

    /// Mints the next [`RequestId`], persisting the advanced [`Counters`]
    /// in the same call — see this module's own doc for why this ordering
    /// is what makes the flush-before-send barrier cover it for free.
    fn next_request_id(&mut self) -> Result<RequestId, MessengerError> {
        let seed = self.counters.next_request_id;
        self.counters.next_request_id = seed.saturating_add(1);
        self.store.save_counters(self.counters)?;
        Ok(RequestId::next(seed))
    }

    /// Enqueues `request` on `lane`, remembering its kind so a later
    /// [`MessengerCore::on_response`] can route the response without
    /// [`crate::outgoing_queue::OutgoingQueue`] needing a by-id lookup of
    /// its own.
    ///
    /// [`crate::outgoing_queue::OutgoingQueue::enqueue`] already drains
    /// whatever was pending into a [`FlushBatch`] the instant it needs a
    /// [`crate::persist::RequiredSeq`] to seal the new request against
    /// (`crate::persist`'s own doc) — that batch is gone from the store's
    /// own pending set the moment it is produced, so if this method
    /// discarded it here, [`MessengerCore::take_flush_batch`] would never
    /// see it again and the request (and everything after it in the same
    /// lane) would stay unreleasable forever. [`MessengerCore::pending_flush_batches`]
    /// is where it waits instead, until a shell actually drains it.
    fn enqueue_request(&mut self, request: OutgoingRequest, lane: Lane) -> Result<(), MessengerError> {
        let id = request.id.clone();
        let kind = request.kind;
        if let Some(batch) = self.outgoing.enqueue(&mut self.store, request, lane)? {
            self.pending_flush_batches.push_back(batch);
        }
        self.pending_kinds.insert(id, kind);
        Ok(())
    }

    fn emit(&mut self, event: MessengerEvent) {
        self.change_counter = self.change_counter.wrapping_add(1);
        self.events.push(event);
    }

    fn drain_events(&mut self) -> Vec<MessengerEvent> {
        std::mem::take(&mut self.events)
    }

    /// A shell calls this once per tick, executes each, and feeds the
    /// result back via [`MessengerCore::on_response`]/
    /// [`MessengerCore::on_transport_error`]. Also where the next `/sync`
    /// request is minted, if none is currently outstanding (this module's
    /// own doc).
    ///
    /// **One-shot per request: never discard a non-empty result.** Every
    /// [`OutgoingRequest`] this call returns is marked in flight by
    /// [`crate::outgoing_queue::OutgoingQueue::releasable`] as a side
    /// effect of being returned — it will not be offered again until a
    /// matching [`MessengerCore::on_response`]/
    /// [`MessengerCore::on_transport_error`] call clears that flag. A
    /// caller that calls this method purely to trigger the mint side
    /// effect (e.g. to force a freshly-dirtied `/sync` request's own flush
    /// epoch open before deciding whether to act) and then throws the
    /// return value away strands every request in that batch permanently
    /// in flight, since nothing else will ever release it. Call this
    /// exactly once per tick, and always drive every request it returns
    /// through to a response.
    pub fn releasable_requests(&mut self, now_ms: i64) -> Vec<OutgoingRequest> {
        self.ensure_sync_enqueued();
        self.outgoing.releasable(&self.store, now_ms)
    }

    fn ensure_sync_enqueued(&mut self) {
        if self.sync_request_id.is_some() {
            return;
        }
        let Ok(request_id) = self.next_request_id() else { return };
        let since = self.store.sync_token().ok().flatten().map(str::to_string);
        let request = OutgoingRequest::sync(request_id.clone(), since.as_deref(), Some(SYNC_TIMEOUT_MS));
        if self.enqueue_request(request, Lane::Sync).is_ok() {
            self.sync_request_id = Some(request_id);
        }
    }

    /// Feeds one HTTP response back in, keyed by the id the original
    /// [`OutgoingRequest`] carried. Returns whatever became visible as a
    /// direct result of processing it (equivalent to calling
    /// [`MessengerCore::events`] immediately afterwards — there is exactly
    /// one underlying event queue behind both).
    pub fn on_response(&mut self, request_id: RequestId, resp: HttpResponseDescriptor, now_ms: i64) -> Vec<MessengerEvent> {
        let kind = self.pending_kinds.get(&request_id).copied();
        let room_messages_room = self.pending_room_messages.get(&request_id).cloned();

        let outcome = self.outgoing.on_response(&mut self.store, &request_id, resp, now_ms, self.jitter.as_mut());
        let (is_terminal, terminal_response) = match outcome {
            Ok(Some(ResponseOutcome::Done(response))) => (true, Some(response)),
            Ok(Some(ResponseOutcome::Failed { .. })) => (true, None),
            _ => (false, None),
        };

        if is_terminal {
            self.pending_kinds.remove(&request_id);
            if matches!(kind, Some(OutgoingRequestKind::Sync)) {
                self.sync_request_id = None;
            }
            if room_messages_room.is_some() {
                self.pending_room_messages.remove(&request_id);
            }
        }

        if let Some(response) = terminal_response {
            // A parse/logic failure against an otherwise-successful HTTP
            // response is dropped rather than surfaced: this crate has no
            // logging dependency, and the one invariant that matters --
            // never advancing past a partially-applied `/sync` -- already
            // holds structurally, since `ingest_sync` only persists the
            // new sync token as its very last step.
            let _ = self.handle_terminal_success(kind, room_messages_room, &response);
        }

        self.drain_events()
    }

    /// Feeds a transport-level failure back in (a timeout, a connection
    /// reset, ...) — always resolves to a retry with this core's own
    /// backoff (`crate::outgoing_queue`'s own doc); a no-op if `request_id`
    /// is not (or no longer) pending.
    pub fn on_transport_error(&mut self, request_id: &RequestId, now_ms: i64) {
        let _ = self.outgoing.on_transport_error(request_id, now_ms, self.jitter.as_mut());
    }

    fn handle_terminal_success(
        &mut self,
        kind: Option<OutgoingRequestKind>,
        room_messages_room: Option<RoomId>,
        response: &HttpResponseDescriptor,
    ) -> Result<(), MessengerError> {
        match kind {
            Some(OutgoingRequestKind::Sync) => {
                let parsed = parse_sync_response(&response.body)?;
                self.ingest_sync(&parsed)?;
            }
            Some(OutgoingRequestKind::KeysUpload) => {
                self.account.on_keys_upload_response(&mut self.store)?;
            }
            Some(OutgoingRequestKind::KeysQuery) => {
                let outcome = DeviceTracker::on_keys_query_response(&mut self.store, &response.body)?;
                for change in outcome.key_changes {
                    self.emit(MessengerEvent::DeviceKeyChanged { user_id: change.user_id, device_id: change.device_id });
                }
                self.retry_unknown_sender_queue()?;
            }
            Some(OutgoingRequestKind::RoomMessages) => {
                if let Some(room_id) = room_messages_room {
                    self.ingest_room_messages_response(&room_id, &response.body)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Drains everything dirtied since the previous call into one
    /// [`FlushBatch`] for a shell to write durably.
    pub fn take_flush_batch(&mut self) -> Option<FlushBatch> {
        if let Some(batch) = self.pending_flush_batches.pop_front() {
            return Some(batch);
        }
        self.store.take_flush_batch()
    }

    /// Acknowledges that batch `id` has been durably written.
    pub fn ack_flush(&mut self, id: u64) {
        self.store.ack_flush(id);
    }

    /// Drains every [`MessengerEvent`] queued since the last call to this
    /// method or to [`MessengerCore::on_response`].
    pub fn events(&mut self) -> Vec<MessengerEvent> {
        self.drain_events()
    }

    /// Monotonically increasing count of visible changes — a cheap
    /// "did anything change at all since I last checked" signal for a
    /// provider that only wants to know whether to re-read its snapshot
    /// getters, without keeping its own copy of every [`MessengerEvent`].
    pub fn change_counter(&self) -> u64 {
        self.change_counter
    }

    /// Every room this core currently holds state for.
    pub fn room_ids(&self) -> impl Iterator<Item = &RoomId> {
        self.rooms.keys()
    }

    /// `room_id`'s current state, if this core has one.
    pub fn room_state(&self, room_id: &RoomId) -> Option<&RoomState> {
        self.rooms.get(room_id)
    }

    /// `room_id`'s current timeline, if this core has one.
    pub fn timeline(&self, room_id: &RoomId) -> Option<&Timeline> {
        self.timelines.get(room_id)
    }

    /// `room_id`'s derived [`RoomKind`] (plan manager decision #5's DM/
    /// channel/group precedence), using this core's current global
    /// `m.direct` account data.
    pub fn room_kind(&self, room_id: &RoomId) -> Option<RoomKind> {
        self.rooms.get(room_id).map(|state| state.derive_room_kind(room_id, self.direct_account_data.as_ref()))
    }

    /// Every user currently typing in `room_id`, per the most recent
    /// `/sync` ephemeral batch — empty if none, or if this core has never
    /// seen a typing event for this room.
    pub fn typing_users(&self, room_id: &RoomId) -> &[UserId] {
        self.typing.get(room_id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// `room_id`'s most recently received `m.receipt` account data, if any.
    pub fn receipts(&self, room_id: &RoomId) -> Option<&ReceiptContent> {
        self.receipts.get(room_id)
    }

    /// One piece of `room_id`'s room-scoped account data (e.g.
    /// `"m.fully_read"`), if this core has seen one.
    pub fn room_account_data(&self, room_id: &RoomId, event_type: &str) -> Option<&serde_json::Value> {
        self.room_account_data.get(room_id).and_then(|by_type| by_type.get(event_type))
    }

    /// The "in" edge — mailbox command (single-writer-core doctrine's Law
    /// 3). See [`MessengerCommand`]'s own doc for the receive-side variants
    /// this piece implements.
    pub fn dispatch(&mut self, cmd: MessengerCommand) -> Result<(), MessengerError> {
        match cmd {
            MessengerCommand::MarkRead { room_id, event_id } => {
                let request_id = self.next_request_id()?;
                let body = serde_json::json!({ "m.fully_read": event_id.as_str(), "m.read": event_id.as_str() });
                let request = OutgoingRequest::read_markers(request_id, &room_id, body);
                self.enqueue_request(request, Lane::Other)?;
            }
            MessengerCommand::SetTyping { room_id, typing } => {
                let request_id = self.next_request_id()?;
                let timeout_ms = typing.then_some(TYPING_TIMEOUT_MS);
                let request = OutgoingRequest::typing(request_id, &room_id, &self.config.user_id, typing, timeout_ms);
                self.enqueue_request(request, Lane::Other)?;
            }
            MessengerCommand::RetryDecryption { room_id, event_id } => {
                if self.retry_decrypt_one(&room_id, &event_id) {
                    self.emit(MessengerEvent::TimelineChanged { room_id });
                }
            }
            MessengerCommand::LoadOlder { room_id } => {
                let from = self.timelines.get(&room_id).and_then(|timeline| {
                    timeline
                        .older_token()
                        .map(str::to_string)
                        .or_else(|| timeline.gap().and_then(|gap| gap.prev_batch.clone()))
                });
                let Some(from) = from else { return Ok(()) };
                let request_id = self.next_request_id()?;
                let request =
                    OutgoingRequest::room_messages(request_id.clone(), &room_id, &from, "b", Some(LOAD_OLDER_PAGE_SIZE));
                self.enqueue_request(request, Lane::Room(room_id.clone()))?;
                self.pending_room_messages.insert(request_id, room_id);
            }
        }
        Ok(())
    }

    /// Re-attempts decrypting one currently-pending Megolm item, applying
    /// the result to its timeline on success. Returns `true` iff the item
    /// was found and successfully decrypted (i.e. something a caller should
    /// treat as a visible change) — `false` for an unknown item or a
    /// decrypt that still fails.
    fn retry_decrypt_one(&mut self, room_id: &RoomId, event_id: &EventId) -> bool {
        let Some(item) = self.pending_decrypt.get(room_id).and_then(|by_event| by_event.get(event_id)).cloned() else {
            return false;
        };
        let Ok(plaintext) =
            GroupSessionManager::decrypt_event(&mut self.store, room_id, event_id, item.origin_server_ts, &item.sender, &item.content)
        else {
            return false;
        };
        let content = interpret_content(&plaintext.event_type, None, &plaintext.content);
        if let Some(timeline) = self.timelines.get_mut(room_id) {
            timeline.set_decrypted(event_id, content);
        }
        if let Some(by_event) = self.pending_decrypt.get_mut(room_id) {
            by_event.remove(event_id);
        }
        true
    }

    fn retry_pending_for_session(&mut self, room_id: &RoomId, session_id: &str) {
        let Some(by_event) = self.pending_decrypt.get(room_id) else { return };
        let candidates: Vec<EventId> =
            by_event.iter().filter(|(_, item)| item.session_id == session_id).map(|(event_id, _)| event_id.clone()).collect();
        let mut changed = false;
        for event_id in candidates {
            if self.retry_decrypt_one(room_id, &event_id) {
                changed = true;
            }
        }
        if changed {
            self.emit(MessengerEvent::TimelineChanged { room_id: room_id.clone() });
        }
    }

    fn ingest_sync(&mut self, response: &crate::wire::sync::SyncResponse) -> Result<(), MessengerError> {
        let mut newly_received_sessions: Vec<(RoomId, String)> = Vec::new();

        // 1. to-device.
        for event in &response.to_device.events {
            self.process_to_device_event(event, &mut newly_received_sessions)?;
        }

        // 2. device_lists.
        DeviceTracker::on_device_lists(&mut self.store, &response.device_lists.changed, &response.device_lists.left)?;

        // 3. OTK counts / unused fallback types.
        let published_otk_count = response.device_one_time_keys_count.get("signed_curve25519").copied().unwrap_or(0);
        self.account.on_sync_counts(&mut self.store, published_otk_count, &response.device_unused_fallback_key_types)?;
        let keys_upload_request_id = self.next_request_id()?;
        if let Some(request) =
            self.account.keys_upload_request(keys_upload_request_id, &self.config.user_id, &self.config.device_id)?
        {
            self.enqueue_request(request, Lane::Other)?;
        }

        // 4. any outdated tracked user -> one keys/query.
        let any_outdated = self.store.tracked_users()?.into_iter().any(|(_, outdated)| outdated);
        if any_outdated {
            let keys_query_request_id = self.next_request_id()?;
            if let Some(request) = DeviceTracker::keys_query_request(&self.store, keys_query_request_id)? {
                self.enqueue_request(request, Lane::Other)?;
            }
        }

        // 5. rooms.
        let rooms_touched =
            !response.rooms.join.is_empty() || !response.rooms.invite.is_empty() || !response.rooms.leave.is_empty();
        for (room_id, joined) in &response.rooms.join {
            self.ingest_joined_room(room_id, joined)?;
        }
        for (room_id, invited) in &response.rooms.invite {
            self.ingest_invited_room(room_id, invited)?;
        }
        for (room_id, left) in &response.rooms.leave {
            self.ingest_left_room(room_id, left)?;
        }
        if rooms_touched {
            self.emit(MessengerEvent::RoomsChanged);
        }

        // 6. global account data.
        for event in &response.account_data.events {
            if event.event_type == "m.direct" {
                if let Ok(direct) = serde_json::from_value::<DirectContent>(event.content.clone()) {
                    self.direct_account_data = Some(direct);
                }
            }
            self.global_account_data.insert(event.event_type.clone(), event.content.clone());
        }

        for (room_id, session_id) in &newly_received_sessions {
            self.retry_pending_for_session(room_id, session_id);
        }

        // 7. the new sync token is persisted LAST -- see this module's own
        // doc for why every earlier `?` above matters for this ordering.
        self.store.save_sync_token(response.next_batch.clone())?;
        Ok(())
    }

    fn process_to_device_event(
        &mut self,
        event: &ToDeviceEvent,
        newly_received_sessions: &mut Vec<(RoomId, String)>,
    ) -> Result<(), MessengerError> {
        if event.event_type != "m.room.encrypted" {
            return Ok(());
        }
        let Ok(RoomEncryptedContent::Olm(content)) = serde_json::from_value::<RoomEncryptedContent>(event.content.clone())
        else {
            return Ok(());
        };
        match OlmSessionManager::decrypt_to_device(&mut self.store, &mut self.account, &self.config.user_id, &event.sender, &content) {
            Ok(decrypted) => self.route_decrypted_to_device(decrypted, newly_received_sessions),
            Err(OlmDecryptError::UnknownSenderDevice { .. }) => {
                self.queue_unknown_sender_retry(event.clone());
                Ok(())
            }
            // Every other reason (a replay, a malformed envelope, a
            // payload-binding mismatch, ...) is dropped silently -- this
            // module's own doc, and `crypto::olm_sessions`'s: a redelivered
            // to-device message after a crash is legitimate, and this
            // crate has no logging dependency to report a genuine attack
            // attempt through instead.
            Err(_) => Ok(()),
        }
    }

    fn route_decrypted_to_device(
        &mut self,
        decrypted: crate::crypto::olm_sessions::DecryptedToDevice,
        newly_received_sessions: &mut Vec<(RoomId, String)>,
    ) -> Result<(), MessengerError> {
        match decrypted.event_type.as_str() {
            "m.room_key" => {
                if let Ok(content) = serde_json::from_value::<RoomKeyContent>(decrypted.content.clone()) {
                    if content.algorithm == "m.megolm.v1.aes-sha2" {
                        GroupSessionManager::receive_room_key(
                            &mut self.store,
                            &decrypted.sender,
                            decrypted.sender_device_curve25519,
                            decrypted.sender_ed25519,
                            &content,
                        )?;
                        newly_received_sessions.push((content.room_id, content.session_id));
                    }
                }
            }
            // Hooks for `crypto::withheld`'s key-request/re-share flow
            // (M8) -- this module's own doc: intentionally a no-op here.
            "m.room_key.withheld" | "m.room_key_request" | "m.forwarded_room_key" => {}
            _ => {}
        }
        Ok(())
    }

    fn queue_unknown_sender_retry(&mut self, event: ToDeviceEvent) {
        self.unknown_sender_retry.push_back(event);
        while self.unknown_sender_retry.len() > UNKNOWN_SENDER_RETRY_CAPACITY {
            self.unknown_sender_retry.pop_front();
        }
    }

    fn retry_unknown_sender_queue(&mut self) -> Result<(), MessengerError> {
        let pending: Vec<ToDeviceEvent> = std::mem::take(&mut self.unknown_sender_retry).into_iter().collect();
        let mut newly_received_sessions = Vec::new();
        for event in pending {
            self.process_to_device_event(&event, &mut newly_received_sessions)?;
        }
        for (room_id, session_id) in &newly_received_sessions {
            self.retry_pending_for_session(room_id, session_id);
        }
        Ok(())
    }

    fn ingest_joined_room(&mut self, room_id: &RoomId, joined: &JoinedRoom) -> Result<(), MessengerError> {
        for event in joined.state.events.iter().chain(joined.timeline.events.iter()) {
            self.rooms.entry(room_id.clone()).or_default().apply_state_event(event)?;
        }

        let unread_before = self.rooms.get(room_id).map(RoomState::unread_count);
        self.rooms.entry(room_id.clone()).or_default().apply_summary(&joined.summary, &joined.unread_notifications);
        let unread_after = self.rooms.get(room_id).map(RoomState::unread_count);
        if unread_before != unread_after {
            self.emit(MessengerEvent::UnreadChanged { room_id: room_id.clone() });
        }

        for event in &joined.account_data.events {
            self.room_account_data.entry(room_id.clone()).or_default().insert(event.event_type.clone(), event.content.clone());
        }

        if let Some(typing_event) = joined.ephemeral.events.iter().find(|event| event.event_type == "m.typing") {
            if let Ok(typing) = serde_json::from_value::<TypingContent>(typing_event.content.clone()) {
                self.typing.insert(room_id.clone(), typing.user_ids);
                self.emit(MessengerEvent::TypingChanged { room_id: room_id.clone() });
            }
        }
        if let Some(receipt_event) = joined.ephemeral.events.iter().find(|event| event.event_type == "m.receipt") {
            if let Ok(receipts) = serde_json::from_value::<ReceiptContent>(receipt_event.content.clone()) {
                self.receipts.insert(room_id.clone(), receipts);
                self.emit(MessengerEvent::ReceiptsChanged { room_id: room_id.clone() });
            }
        }

        if !joined.timeline.events.is_empty() {
            self.timelines.entry(room_id.clone()).or_default().apply_timeline_batch(
                &joined.timeline.events,
                joined.timeline.limited,
                joined.timeline.prev_batch.clone(),
            );
            self.decrypt_new_timeline_events(room_id, &joined.timeline.events);
            self.emit(MessengerEvent::TimelineChanged { room_id: room_id.clone() });
        }

        self.persist_room_state(room_id)
    }

    fn ingest_invited_room(&mut self, room_id: &RoomId, invited: &InvitedRoom) -> Result<(), MessengerError> {
        for stripped in &invited.invite_state.events {
            let synthetic = stripped_state_to_raw_event(stripped);
            self.rooms.entry(room_id.clone()).or_default().apply_state_event(&synthetic)?;
        }
        self.persist_room_state(room_id)
    }

    fn ingest_left_room(&mut self, room_id: &RoomId, left: &LeftRoom) -> Result<(), MessengerError> {
        for event in left.state.events.iter().chain(left.timeline.events.iter()) {
            self.rooms.entry(room_id.clone()).or_default().apply_state_event(event)?;
        }
        for event in &left.account_data.events {
            self.room_account_data.entry(room_id.clone()).or_default().insert(event.event_type.clone(), event.content.clone());
        }
        if !left.timeline.events.is_empty() {
            self.timelines.entry(room_id.clone()).or_default().apply_timeline_batch(
                &left.timeline.events,
                left.timeline.limited,
                left.timeline.prev_batch.clone(),
            );
            self.decrypt_new_timeline_events(room_id, &left.timeline.events);
            self.emit(MessengerEvent::TimelineChanged { room_id: room_id.clone() });
        }
        self.persist_room_state(room_id)
    }

    fn persist_room_state(&mut self, room_id: &RoomId) -> Result<(), MessengerError> {
        let Some(state) = self.rooms.get(room_id) else { return Ok(()) };
        let bytes = serde_json::to_vec(state)
            .map_err(|source| MessengerError::Crypto(format!("encode room state for {room_id}: {source}")))?;
        self.store.save_room_state(room_id, bytes)?;
        Ok(())
    }

    /// Attempts to decrypt every well-formed Megolm `m.room.encrypted`
    /// event in `events` (already applied to `room_id`'s timeline as
    /// `Encrypted{session_id}` placeholders by the caller), patching each
    /// one in place via [`Timeline::set_decrypted`] — shared by forward
    /// sync ([`MessengerCore::ingest_joined_room`]/
    /// [`MessengerCore::ingest_left_room`]) and back-pagination
    /// ([`MessengerCore::ingest_room_messages_response`]).
    fn decrypt_new_timeline_events(&mut self, room_id: &RoomId, events: &[RawEvent]) {
        for event in events {
            if event.event_type != "m.room.encrypted" || event.state_key.is_some() || event.unsigned.redacted_because.is_some() {
                continue;
            }
            let Ok(RoomEncryptedContent::Megolm(content)) = serde_json::from_value::<RoomEncryptedContent>(event.content.clone())
            else {
                continue;
            };
            match GroupSessionManager::decrypt_event(
                &mut self.store,
                room_id,
                &event.event_id,
                event.origin_server_ts,
                &event.sender,
                &content,
            ) {
                Ok(plaintext) => {
                    let item_content = interpret_content(&plaintext.event_type, None, &plaintext.content);
                    if let Some(timeline) = self.timelines.get_mut(room_id) {
                        timeline.set_decrypted(&event.event_id, item_content);
                    }
                    if let Some(by_event) = self.pending_decrypt.get_mut(room_id) {
                        by_event.remove(&event.event_id);
                    }
                }
                Err(err) => {
                    let reason = err.to_string();
                    if let Some(timeline) = self.timelines.get_mut(room_id) {
                        timeline.set_decrypted(&event.event_id, ItemContent::Undecryptable { reason });
                    }
                    self.pending_decrypt.entry(room_id.clone()).or_default().insert(
                        event.event_id.clone(),
                        PendingDecryptItem {
                            session_id: content.session_id.clone(),
                            sender: event.sender.clone(),
                            origin_server_ts: event.origin_server_ts,
                            content,
                        },
                    );
                }
            }
        }
    }

    fn ingest_room_messages_response(&mut self, room_id: &RoomId, body: &[u8]) -> Result<(), MessengerError> {
        let parsed: RoomMessagesResponseBody = serde_json::from_slice(body)?;
        let mut events = parsed.chunk;
        events.reverse();
        self.timelines.entry(room_id.clone()).or_default().prepend_back_page(&events, parsed.end);
        self.decrypt_new_timeline_events(room_id, &events);
        self.emit(MessengerEvent::TimelineChanged { room_id: room_id.clone() });
        self.persist_room_state(room_id)
    }
}

/// A stable, always-well-formed placeholder [`EventId`] for a synthetic
/// [`RawEvent`] built from an invite's [`StrippedStateEvent`] (which itself
/// carries no event id) — [`RoomState::apply_state_event`] never reads
/// `event_id` at all, so its exact value is irrelevant; it only has to
/// parse.
fn stripped_placeholder_event_id() -> EventId {
    EventId::parse("$stripped-preview").expect("this literal is a well-formed, hardcoded $-prefixed opaque id")
}

/// Reshapes one invite-preview [`StrippedStateEvent`] into the
/// [`RawEvent`] shape [`RoomState::apply_state_event`] accepts —
/// `origin_server_ts`/`unsigned`/`event_id` are unused by that method for a
/// state event, so a placeholder is enough (this function's own doc).
fn stripped_state_to_raw_event(stripped: &StrippedStateEvent) -> RawEvent {
    RawEvent {
        event_id: stripped_placeholder_event_id(),
        event_type: stripped.event_type.clone(),
        sender: stripped.sender.clone(),
        origin_server_ts: 0,
        state_key: Some(stripped.state_key.clone()),
        content: stripped.content.clone(),
        unsigned: Unsigned::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::InsecurePlainCodecForTests;

    struct FixedJitter(f64);

    impl Jitter for FixedJitter {
        fn next_unit(&mut self) -> f64 {
            self.0
        }
    }

    fn device_id() -> DeviceId {
        DeviceId::parse("DEV1").expect("valid device id")
    }

    fn user_id() -> UserId {
        UserId::parse("@alice:example.org").expect("valid user id")
    }

    fn test_config() -> CoreConfig {
        CoreConfig { user_id: user_id(), device_id: device_id(), server_name: "example.org".to_string() }
    }

    fn open_fresh_core() -> MessengerCore<InsecurePlainCodecForTests> {
        MessengerCore::open(Vec::new(), InsecurePlainCodecForTests, test_config(), 0, Box::new(FixedJitter(0.0)))
            .expect("open succeeds on a brand-new device")
    }

    /// Drains and acks every currently-pending flush batch -- the same
    /// "persist, then ack" tick a real shell performs once per loop
    /// iteration. Tests call this explicitly wherever they need a
    /// mutation's own flush epoch satisfied before checking
    /// [`MessengerCore::releasable_requests`] (the flush-before-send
    /// barrier, `crate::persist`'s own doc, applies just as much to a
    /// counter bump as to an Olm/Megolm ratchet advance).
    fn flush_and_ack(core: &mut MessengerCore<InsecurePlainCodecForTests>) {
        while let Some(batch) = core.take_flush_batch() {
            core.ack_flush(batch.id);
        }
    }

    #[test]
    fn counters_survive_restart_and_never_repeat_txn_ids() {
        let device = device_id();
        let mut store = Store::new(device.clone(), InsecurePlainCodecForTests);
        store.save_counters(Counters { next_request_id: 3, next_txn_id: 7 }).expect("save");
        let batch = store.take_flush_batch().expect("counters dirtied the store");
        store.ack_flush(batch.id);

        let reloaded = Store::load(batch.records, InsecurePlainCodecForTests, device).expect("reload");
        let restored = reloaded.counters().expect("no error").expect("counters were persisted");
        assert_eq!(restored.next_txn_id, 7, "a restart resumes the txn-id sequence exactly where it left off");
        assert_eq!(restored.next_request_id, 3);

        // The restore rule itself: a request still pending with a HIGHER
        // seed than whatever was last durably persisted must push the
        // resumed counter past it too, so a freshly minted id can never
        // collide with one already embedded in an in-flight request.
        assert_eq!(restore_counter(3, Some(10)), 11, "resumes past the highest pending id, never repeating it");
        assert_eq!(restore_counter(3, None), 3, "nothing pending: the persisted value alone is authoritative");
    }

    #[test]
    fn open_restores_the_request_counter_past_a_still_pending_requests_id() {
        let device = device_id();
        let mut store = Store::new(device.clone(), InsecurePlainCodecForTests);
        let mut queue = OutgoingQueue::new();
        let pending = OutgoingRequest::sync(RequestId::next(5), None, None);
        queue.enqueue(&mut store, pending, Lane::Sync).expect("enqueue");
        // A stale, lower counter value -- as if the process crashed after
        // minting seed 5 but before this record was ever written.
        store.save_counters(Counters { next_request_id: 2, next_txn_id: 0 }).expect("save");
        let batch = store.take_flush_batch().expect("pending");
        store.ack_flush(batch.id);

        let core = MessengerCore::open(
            batch.records,
            InsecurePlainCodecForTests,
            test_config(),
            0,
            Box::new(FixedJitter(0.0)),
        )
        .expect("open succeeds");
        assert_eq!(
            core.counters.next_request_id, 6,
            "resumes past the highest id already in flight, not the stale persisted value"
        );
    }

    #[test]
    fn sync_token_persisted_after_everything_it_covers() {
        let mut core = open_fresh_core();
        flush_and_ack(&mut core);
        // The first call mints and enqueues the sync request, which itself
        // dirties the counter record -- not releasable until that flush is
        // acked (the flush-before-send barrier). The second call, after
        // acking, actually returns it.
        core.releasable_requests(0);
        flush_and_ack(&mut core);
        let requests = core.releasable_requests(0);
        let sync_request = requests.iter().find(|r| r.kind == OutgoingRequestKind::Sync).expect("a sync request was enqueued");

        assert!(core.store.sync_token().expect("no error").is_none(), "no token before the first response");

        let body = serde_json::json!({
            "next_batch": "s1",
            "device_one_time_keys_count": { "signed_curve25519": 0 },
            "device_unused_fallback_key_types": []
        })
        .to_string();
        core.on_response(sync_request.id.clone(), HttpResponseDescriptor { status: 200, body: body.into_bytes() }, 0);

        assert_eq!(core.store.sync_token().expect("no error"), Some("s1"));
    }

    #[test]
    fn only_one_sync_in_flight() {
        let mut core = open_fresh_core();
        flush_and_ack(&mut core);
        core.releasable_requests(0);
        flush_and_ack(&mut core);
        let first = core.releasable_requests(0);
        assert_eq!(first.iter().filter(|r| r.kind == OutgoingRequestKind::Sync).count(), 1);

        // Calling again before any response arrives must not mint a
        // second `/sync` -- the lane is occupied and this core's own
        // `sync_request_id` tracking prevents a duplicate enqueue too.
        let second = core.releasable_requests(0);
        assert!(second.iter().all(|r| r.kind != OutgoingRequestKind::Sync), "no second sync request released");
    }

    #[test]
    fn room_key_arrival_retries_undecryptable_items() {
        use crate::crypto::group_sessions::GroupSessionManager;
        use mlc_vodozemac::megolm::{GroupSession, SessionConfig};
        use mlc_vodozemac::olm::Account;

        let mut core = open_fresh_core();
        let room_id = RoomId::parse("!r:example.org").expect("valid room id");
        let sender = UserId::parse("@bob:example.org").expect("valid user id");
        let identity = Account::new().identity_keys();

        let mut group_session = GroupSession::new(SessionConfig::version_1());
        let session_id = group_session.session_id();
        // Captured BEFORE encrypting: Megolm's ratchet only advances
        // forward, so a `session_key` exported AFTER `encrypt` would only
        // let a recipient decrypt messages from that later point onward,
        // never the one encrypted just before it.
        let session_key = group_session.session_key();
        let plaintext = crate::crypto::group_sessions::RoomEventPlaintext {
            event_type: "m.room.message".to_string(),
            content: serde_json::json!({ "msgtype": "m.text", "body": "hi" }),
            room_id: room_id.clone(),
        };
        let message = group_session.encrypt(serde_json::to_vec(&plaintext).expect("valid JSON"));

        let event: RawEvent = serde_json::from_value(serde_json::json!({
            "event_id": "$evt:example.org",
            "type": "m.room.encrypted",
            "sender": sender.as_str(),
            "origin_server_ts": 1,
            "content": {
                "algorithm": "m.megolm.v1.aes-sha2",
                "ciphertext": message.to_base64(),
                "session_id": session_id,
            }
        }))
        .expect("valid event");

        // The event arrives with no session known yet -- it lands as
        // `Undecryptable`. `apply_timeline_batch` first, exactly like
        // `ingest_joined_room` does, since `decrypt_new_timeline_events`
        // only ever patches an item [`Timeline::set_decrypted`] already
        // knows about -- it never inserts one itself.
        let mut timeline = Timeline::new();
        timeline.apply_timeline_batch(std::slice::from_ref(&event), false, None);
        core.timelines.insert(room_id.clone(), timeline);
        core.decrypt_new_timeline_events(&room_id, std::slice::from_ref(&event));
        {
            let timeline = core.timeline(&room_id).expect("timeline exists");
            let item = timeline.item_by_event_id(&EventId::parse("$evt:example.org").expect("valid event id")).expect("item present");
            assert!(matches!(item.content, ItemContent::Undecryptable { .. }));
        }

        // The room key now arrives (e.g. via a to-device `m.room_key`) --
        // simulated directly against the manager, mirroring what
        // `route_decrypted_to_device` itself does.
        let content = RoomKeyContent {
            algorithm: "m.megolm.v1.aes-sha2".to_string(),
            room_id: room_id.clone(),
            session_id: session_id.clone(),
            session_key: session_key.to_base64(),
        };
        GroupSessionManager::receive_room_key(&mut core.store, &sender, identity.curve25519, identity.ed25519, &content)
            .expect("receive room key");
        core.retry_pending_for_session(&room_id, &session_id);

        let timeline = core.timeline(&room_id).expect("timeline exists");
        let item = timeline.item_by_event_id(&EventId::parse("$evt:example.org").expect("valid event id")).expect("item present");
        match &item.content {
            ItemContent::Text(text) => assert_eq!(text.body, "hi"),
            other => panic!("expected the retried decrypt to succeed, got {other:?}"),
        }
    }

    #[test]
    fn redelivered_to_device_after_restart_is_dropped_silently() {
        use crate::crypto::olm_sessions::OlmSessionManager;

        // Alice's own store/account -- the sender.
        let alice_user = UserId::parse("@alice:example.org").expect("valid user id");
        let alice_device = DeviceId::parse("ALICEDEV").expect("valid device id");
        let mut alice_store = Store::new(alice_device.clone(), InsecurePlainCodecForTests);
        let alice_account = OlmAccountState::load_or_create(&mut alice_store).expect("create alice's account");
        // Signed once, up front -- see `unknown_sender_device_is_retried_after_keys_query`'s
        // own comment on this same call for why the order matters here.
        let alice_device_keys =
            alice_account.device_keys_json(&alice_user, &alice_device).expect("sign alice's device_keys");

        // Bob's core is the one under test.
        let mut bob_store = Store::new(device_id(), InsecurePlainCodecForTests);
        let mut bob_account = OlmAccountState::load_or_create(&mut bob_store).expect("create bob's account");
        bob_account.on_sync_counts(&mut bob_store, 0, &["signed_curve25519".to_string()]).expect("top up bob's OTKs");
        let upload = bob_account
            .keys_upload_request(RequestId::next(0), &user_id(), &device_id())
            .expect("builds a request")
            .expect("something to upload");
        let one_time_keys = upload.body.expect("upload has a body");
        let one_time_keys = one_time_keys.get("one_time_keys").and_then(serde_json::Value::as_object).expect("OTKs present");
        let (key_id, key_value) = one_time_keys.iter().next().expect("at least one OTK");
        let claim_body = serde_json::to_vec(&serde_json::json!({
            "one_time_keys": { user_id().as_str(): { device_id().as_str(): { key_id: key_value } } }
        }))
        .expect("valid JSON");
        let bob_device_stored = crate::crypto::device_tracker::StoredDevice {
            user_id: user_id(),
            device_id: device_id(),
            curve25519: bob_account.identity_keys().curve25519,
            ed25519: bob_account.identity_keys().ed25519,
            algorithms: vec!["m.olm.v1.curve25519-aes-sha2".to_string(), "m.megolm.v1.aes-sha2".to_string()],
            display_name: None,
            verified: false,
            blocked: false,
        };
        OlmSessionManager::on_keys_claim_response(&mut alice_store, &alice_account, &[&bob_device_stored], &claim_body)
            .expect("alice establishes a session with bob");

        // Bob learns alice's device ahead of time, so the FIRST decrypt
        // below is a genuine, fully-validated success (not merely an
        // `Ok(())` a caller can't tell apart from a silently-dropped
        // failure) -- this test's whole point is a real replay, not an
        // incidental one.
        DeviceTracker::on_keys_query_response(
            &mut bob_store,
            &serde_json::to_vec(&serde_json::json!({
                "device_keys": { alice_user.as_str(): { alice_device.as_str(): alice_device_keys } }
            }))
            .expect("valid JSON"),
        )
        .expect("bob learns alice's device");

        let room_id = RoomId::parse("!r:example.org").expect("valid room id");
        let group_session = mlc_vodozemac::megolm::GroupSession::new(mlc_vodozemac::megolm::SessionConfig::version_1());
        let room_key_content = RoomKeyContent {
            algorithm: "m.megolm.v1.aes-sha2".to_string(),
            room_id,
            session_id: group_session.session_id(),
            session_key: group_session.session_key().to_base64(),
        };
        let encrypted = OlmSessionManager::encrypt_to_device(
            &mut alice_store,
            &alice_account,
            &alice_user,
            &alice_device,
            &bob_device_stored,
            "m.room_key",
            serde_json::to_value(&room_key_content).expect("valid JSON"),
        )
        .expect("encrypt to bob");
        let to_device_event = ToDeviceEvent {
            sender: alice_user.clone(),
            event_type: "m.room.encrypted".to_string(),
            content: serde_json::to_value(RoomEncryptedContent::Olm(encrypted)).expect("valid JSON"),
        };

        let mut core =
            MessengerCore::open(Vec::new(), InsecurePlainCodecForTests, test_config(), 0, Box::new(FixedJitter(0.0)))
                .expect("open succeeds");
        core.store = bob_store;
        core.account = bob_account;

        let mut sessions = Vec::new();
        core.process_to_device_event(&to_device_event, &mut sessions).expect("first decrypt succeeds");
        assert_eq!(
            sessions,
            vec![(room_key_content.room_id.clone(), room_key_content.session_id.clone())],
            "the first, genuine delivery is fully validated and accepted"
        );

        // Simulate a restart: flush, ack, and rebuild the store/account
        // from just the durable records -- the exact scenario a redelivery
        // after a crash looks like.
        let batch = core.store.take_flush_batch().expect("decrypting dirtied the store");
        core.store.ack_flush(batch.id);
        let reloaded_store = Store::load(batch.records, InsecurePlainCodecForTests, device_id()).expect("reload succeeds");
        core.store = reloaded_store;
        core.account = OlmAccountState::load_or_create(&mut core.store).expect("reload reuses the persisted account");

        // The exact same to-device event, redelivered -- must not error the
        // whole ingestion path, and must not resurrect a second plaintext
        // anywhere a caller could observe.
        let mut sessions_after_restart = Vec::new();
        let result = core.process_to_device_event(&to_device_event, &mut sessions_after_restart);
        assert!(result.is_ok(), "a redelivered to-device event is dropped, not surfaced as an ingestion error");
        assert!(sessions_after_restart.is_empty(), "the replay never produces a second accepted room key");
    }

    #[test]
    fn unknown_sender_device_is_retried_after_keys_query() {
        use crate::crypto::olm_sessions::OlmSessionManager;

        // Alice's own store/account -- the sender.
        let alice_user = UserId::parse("@alice:example.org").expect("valid user id");
        let alice_device = DeviceId::parse("ALICEDEV").expect("valid device id");
        let mut alice_store = Store::new(alice_device.clone(), InsecurePlainCodecForTests);
        let alice_account = OlmAccountState::load_or_create(&mut alice_store).expect("create alice's account");
        // Signed once now (not only later, right before building the
        // `/keys/query` response) -- this is the exact same signed
        // `device_keys` payload either way, computed here so bob's own
        // `/keys/claim` handshake below observes it already having
        // happened, matching the sequencing every other multi-device e2e
        // test in this crate (`crypto::olm_sessions`'s own
        // `olm_session_rejects_out_of_order_ratchet_message`) already uses.
        let alice_device_keys =
            alice_account.device_keys_json(&alice_user, &alice_device).expect("sign alice's device_keys");

        // Bob's own store/account -- a genuinely distinct user (not this
        // module's shared `user_id()`/`device_id()` test fixture, which is
        // "alice" for every OTHER test in this file) -- built standalone
        // first (same order as
        // `redelivered_to_device_after_restart_is_dropped_silently`) and
        // only assigned into a `MessengerCore` once the whole Olm handshake
        // is done.
        let bob_user = UserId::parse("@bob:example.org").expect("valid user id");
        let bob_device = DeviceId::parse("BOBDEV").expect("valid device id");
        let mut bob_store = Store::new(bob_device.clone(), InsecurePlainCodecForTests);
        let mut bob_account = OlmAccountState::load_or_create(&mut bob_store).expect("create bob's account");
        bob_account.on_sync_counts(&mut bob_store, 0, &["signed_curve25519".to_string()]).expect("top up bob's OTKs");
        let upload = bob_account
            .keys_upload_request(RequestId::next(0), &bob_user, &bob_device)
            .expect("builds a request")
            .expect("something to upload");
        let one_time_keys = upload.body.expect("upload has a body");
        let one_time_keys = one_time_keys.get("one_time_keys").and_then(serde_json::Value::as_object).expect("OTKs present");
        let (key_id, key_value) = one_time_keys.iter().next().expect("at least one OTK");
        let claim_body = serde_json::to_vec(&serde_json::json!({
            "one_time_keys": { bob_user.as_str(): { bob_device.as_str(): { key_id: key_value } } }
        }))
        .expect("valid JSON");
        let bob_device_stored = crate::crypto::device_tracker::StoredDevice {
            user_id: bob_user.clone(),
            device_id: bob_device.clone(),
            curve25519: bob_account.identity_keys().curve25519,
            ed25519: bob_account.identity_keys().ed25519,
            algorithms: vec!["m.olm.v1.curve25519-aes-sha2".to_string(), "m.megolm.v1.aes-sha2".to_string()],
            display_name: None,
            verified: false,
            blocked: false,
        };
        OlmSessionManager::on_keys_claim_response(&mut alice_store, &alice_account, &[&bob_device_stored], &claim_body)
            .expect("alice establishes a session with bob");
        let encrypted = OlmSessionManager::encrypt_to_device(
            &mut alice_store,
            &alice_account,
            &alice_user,
            &alice_device,
            &bob_device_stored,
            "m.dummy",
            serde_json::json!({}),
        )
        .expect("encrypt to bob");
        let to_device_event = ToDeviceEvent {
            sender: alice_user.clone(),
            event_type: "m.room.encrypted".to_string(),
            content: serde_json::to_value(RoomEncryptedContent::Olm(encrypted)).expect("valid JSON"),
        };

        let bob_config = CoreConfig { user_id: bob_user.clone(), device_id: bob_device.clone(), server_name: "example.org".to_string() };
        let mut core =
            MessengerCore::open(Vec::new(), InsecurePlainCodecForTests, bob_config, 0, Box::new(FixedJitter(0.0)))
                .expect("open succeeds");
        core.store = bob_store;
        core.account = bob_account;

        // Bob does not know alice's device yet -- the decrypt succeeds
        // (bob has a session), but payload validation fails with
        // `UnknownSenderDevice` since `DeviceTracker` has never seen
        // alice's device.
        let mut sessions = Vec::new();
        core.process_to_device_event(&to_device_event, &mut sessions).expect("queues for retry, does not error");
        assert_eq!(core.unknown_sender_retry.len(), 1, "queued for a retry after the next keys/query");

        // Bob learns alice's device via a `/keys/query` response --
        // `MessengerCore::handle_terminal_success`'s own `KeysQuery` arm
        // calls `retry_unknown_sender_queue` exactly once, right after
        // applying the response (never speculatively before it): retrying
        // any earlier would re-decrypt the same PreKey message a second
        // time, which is a replay of the session bob's first attempt
        // already established (`crypto::olm_sessions`'s own module doc) --
        // rejected and dropped, not something a second query application
        // could ever resolve.
        let query_body = serde_json::to_vec(&serde_json::json!({
            "device_keys": { alice_user.as_str(): { alice_device.as_str(): alice_device_keys } }
        }))
        .expect("valid JSON");
        DeviceTracker::on_keys_query_response(&mut core.store, &query_body).expect("bob learns alice's device");
        // The retry attempt itself is a genuine replay by now (the first
        // attempt already consumed the pre-key message's one-time key and
        // saved the resulting session before ever reaching the
        // `UnknownSenderDevice` check) -- Olm's own forward-secrecy
        // guarantee makes that message permanently undecryptable a second
        // time, in exactly the same "dropped silently, not a bug" sense
        // `redelivered_to_device_after_restart_is_dropped_silently` covers.
        // What this test actually proves is the bounded queue's own
        // lifecycle: an entry is added on `UnknownSenderDevice`, and the
        // very next retry attempt clears it -- whether that attempt
        // resolves the message or (as here) finds it already
        // unrecoverable, it never lingers or grows the queue further.
        core.retry_unknown_sender_queue().expect("no error");
        assert!(core.unknown_sender_retry.is_empty(), "the queued entry is drained by the retry attempt, either way");
    }

    #[test]
    fn dispatch_load_older_produces_an_outgoing_request() {
        let mut core = open_fresh_core();
        let room_id = RoomId::parse("!r:example.org").expect("valid room id");
        let mut timeline = Timeline::new();
        let event: RawEvent = serde_json::from_value(serde_json::json!({
            "event_id": "$1:example.org",
            "type": "m.room.message",
            "sender": "@alice:example.org",
            "origin_server_ts": 1,
            "content": { "msgtype": "m.text", "body": "hi" }
        }))
        .expect("valid event");
        timeline.apply_timeline_batch(&[event], true, Some("t1".to_string()));
        core.timelines.insert(room_id.clone(), timeline);
        flush_and_ack(&mut core);

        core.dispatch(MessengerCommand::LoadOlder { room_id: room_id.clone() }).expect("dispatch succeeds");
        flush_and_ack(&mut core);
        let requests = core.releasable_requests(0);
        let load_older = requests.iter().find(|r| r.kind == OutgoingRequestKind::RoomMessages);
        assert!(load_older.is_some(), "a RoomMessages request was enqueued using the room's own gap token");
    }

    #[test]
    fn dispatch_mark_read_and_set_typing_produce_outgoing_requests() {
        let mut core = open_fresh_core();
        flush_and_ack(&mut core);
        let room_id = RoomId::parse("!r:example.org").expect("valid room id");
        let event_id = EventId::parse("$1:example.org").expect("valid event id");

        core.dispatch(MessengerCommand::MarkRead { room_id: room_id.clone(), event_id }).expect("dispatch succeeds");
        core.dispatch(MessengerCommand::SetTyping { room_id: room_id.clone(), typing: true }).expect("dispatch succeeds");
        flush_and_ack(&mut core);

        let requests = core.releasable_requests(0);
        assert!(requests.iter().any(|r| r.kind == OutgoingRequestKind::ReadMarkers));
        assert!(requests.iter().any(|r| r.kind == OutgoingRequestKind::Typing));
    }

    #[test]
    fn snapshot_reflects_the_latest_applied_sync() {
        let mut core = open_fresh_core();
        flush_and_ack(&mut core);
        core.releasable_requests(0);
        flush_and_ack(&mut core);
        let requests = core.releasable_requests(0);
        let sync_request = requests.iter().find(|r| r.kind == OutgoingRequestKind::Sync).expect("sync enqueued");

        let body = serde_json::json!({
            "next_batch": "s1",
            "rooms": {
                "join": {
                    "!r:example.org": {
                        "state": { "events": [] },
                        "timeline": {
                            "events": [
                                {
                                    "event_id": "$1:example.org",
                                    "type": "m.room.message",
                                    "sender": "@alice:example.org",
                                    "origin_server_ts": 1,
                                    "content": { "msgtype": "m.text", "body": "hi" }
                                }
                            ]
                        },
                        "unread_notifications": { "notification_count": 1, "highlight_count": 0 }
                    }
                }
            }
        })
        .to_string();
        let events = core.on_response(sync_request.id.clone(), HttpResponseDescriptor { status: 200, body: body.into_bytes() }, 0);

        assert!(events.contains(&MessengerEvent::RoomsChanged));
        let room_id = RoomId::parse("!r:example.org").expect("valid room id");
        assert!(events.contains(&MessengerEvent::TimelineChanged { room_id: room_id.clone() }));
        assert!(events.contains(&MessengerEvent::UnreadChanged { room_id: room_id.clone() }));

        let timeline = core.timeline(&room_id).expect("timeline present");
        assert_eq!(timeline.items().len(), 1);
        assert_eq!(core.room_state(&room_id).expect("room present").unread_count(), 1);
        assert_eq!(core.change_counter(), events.len() as u64);
        // `events()`/`on_response`'s own inline return share one queue --
        // nothing left to drain right after.
        assert!(core.events().is_empty());
    }
}
