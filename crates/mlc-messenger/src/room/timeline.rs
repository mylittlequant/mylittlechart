//! [`Timeline`] -- one room's own message list, assembled from `/sync`
//! timeline windows (research doc §1.3-§1.4) and `/messages` back-pages
//! (research doc §1.5).
//!
//! # What gets its own row, and what doesn't
//!
//! A pure relation event -- an `m.reaction` (`m.annotation`) or an edit
//! (`m.replace`) -- never becomes its own [`TimelineItem`]; it folds onto
//! its target's [`RelationsBundle`] instead (`crate::room::relations`). An
//! `m.room.redaction` event never becomes its own row either; it applies
//! [`Timeline::apply_redaction`] to its `redacts` target. Every other event
//! (including one that also carries a reply/thread pointer -- that pointer
//! is the event's OWN outgoing relation, not something contributed onto a
//! different target) gets a row.
//!
//! # Two documented scope cuts
//!
//! This piece (M4) does not queue anything for later:
//!
//! - A relation event whose target this timeline has not loaded (out of
//!   the currently fetched window) is silently dropped, not retried once a
//!   later back-page brings the target into view.
//! - Redacting an event this timeline knows nothing about at all (neither
//!   a loaded item nor a tracked relation contribution) is a no-op.
//!
//! Both match real client behavior at the edges (you cannot react to, or
//! usefully redact the visible effect of, an event you have never loaded)
//! and keep this piece's bookkeeping to "what is currently in view," which
//! is what `sync_engine`/`outgoing_queue` (M12-M13) actually need from it.

use crate::ids::{EventId, TxnId, UserId};
use crate::room::relations::{aggregate_relation, recompute_bundle, RelatingEvent, RelationsBundle};
use crate::wire::events::{
    LegacyDmContent, Membership, RawEvent, RedactionContent, RelatesTo, RoomEncryptedContent, RoomMessageContent,
    TextLikeMessageContent,
};
use std::collections::BTreeMap;

const EVENT_ROOM_MESSAGE: &str = "m.room.message";
const EVENT_ROOM_ENCRYPTED: &str = "m.room.encrypted";
const EVENT_ROOM_REDACTION: &str = "m.room.redaction";
const EVENT_LEGACY_DM: &str = "org.mylittlechart.legacy_dm";
const EVENT_ROOM_MEMBER: &str = "m.room.member";
const EVENT_ROOM_NAME: &str = "m.room.name";
const EVENT_ROOM_TOPIC: &str = "m.room.topic";
const EVENT_ROOM_POWER_LEVELS: &str = "m.room.power_levels";
const EVENT_ROOM_HISTORY_VISIBILITY: &str = "m.room.history_visibility";
const EVENT_ROOM_CREATE: &str = "m.room.create";

/// One item's send lifecycle (plan §3: `LocalEcho|Sending|Sent|Failed`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendState {
    /// Appended optimistically, not yet handed to the transport (plan
    /// §4.2 step 6).
    LocalEcho,
    /// Handed to the transport, no server acknowledgement yet.
    Sending,
    /// The server accepted it -- every item [`Timeline::apply_timeline_batch`]
    /// builds from a `/sync` response starts here.
    Sent,
    /// The send failed outright (a network error, a rejected request,
    /// ...).
    Failed {
        /// Why the send failed.
        reason: String,
    },
}

/// A state-event-shaped timeline row's own summary -- only the three state
/// types this crate renders inline (member/name/topic; plan §3's
/// `ItemContent` set). Any other state event type appearing in a
/// `timeline.events` window becomes [`ItemContent::Unknown`] instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StateChangeSummary {
    /// An `m.room.member` change.
    Member {
        /// The subject's user id (the event's own `state_key`, kept as a
        /// plain `String` rather than [`UserId`] since a malformed
        /// `state_key` must still render something rather than vanish).
        user_id: String,
        /// The subject's new membership.
        membership: Membership,
        /// The subject's display name, if set and not redacted away (the
        /// v11 allow-list drops this field on redaction -- see
        /// [`Timeline::apply_redaction`]).
        displayname: Option<String>,
    },
    /// An `m.room.name` change: the room's new name.
    Name(String),
    /// An `m.room.topic` change: the room's new topic.
    Topic(String),
}

/// One timeline item's content, at whatever stage of decryption/redaction
/// it has reached (plan §3's `ItemContent` set).
#[derive(Clone, Debug, PartialEq)]
pub enum ItemContent {
    /// `m.room.message`, `msgtype: "m.text"`.
    Text(TextLikeMessageContent),
    /// `m.room.message`, `msgtype: "m.notice"`.
    Notice(TextLikeMessageContent),
    /// `m.room.message`, `msgtype: "m.emote"`.
    Emote(TextLikeMessageContent),
    /// `m.room.encrypted` (Megolm), not yet decrypted -- the crypto layer
    /// (a later piece) resolves this via [`Timeline::set_decrypted`] once
    /// it can.
    Encrypted {
        /// The Megolm session this item was encrypted with.
        session_id: String,
    },
    /// Decryption was attempted and structurally cannot succeed (no
    /// session, withheld, ...) -- a real UTD, not a transient state.
    Undecryptable {
        /// Why. A plain `String` in this piece; becomes
        /// `crypto::group_sessions::UtdReason` once that piece (M7/M8)
        /// exists.
        reason: String,
    },
    /// `org.mylittlechart.legacy_dm` (server plan manager decision #5):
    /// still encrypted under the pre-Matrix X25519+HKDF+AES-GCM scheme --
    /// `legacy_dm.rs` (M11) resolves this via [`Timeline::set_decrypted`].
    LegacyDm {
        /// The message's id in the pre-migration legacy DM store.
        legacy_message_id: String,
        /// The AES-GCM nonce, base64-encoded.
        nonce_b64: String,
        /// The AES-GCM ciphertext, base64-encoded.
        ciphertext_b64: String,
    },
    /// This event was redacted and had nothing worth keeping under the
    /// v11 allow-list (research doc §1.1) -- see
    /// [`Timeline::apply_redaction`] for the one event type this crate
    /// renders that keeps something instead.
    Redacted,
    /// A state event rendered inline in the timeline.
    StateChange(StateChangeSummary),
    /// An event type (or an unparseable `msgtype`/`algorithm`) this crate
    /// does not render inline -- kept forward-compatible rather than
    /// failing (same doctrine as `wire::events`).
    Unknown,
}

/// Records that a `limited: true` `/sync` window (research doc §1.4) left
/// a gap between what this timeline holds and the room's full history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GapMarker {
    /// The pagination token (`GET /rooms/{roomId}/messages?from=<token>&
    /// dir=b`) to fetch further back from this gap's boundary. `None`
    /// only when the server did not supply one (the room's entire history
    /// fit in this one window).
    pub prev_batch: Option<String>,
}

/// One row of a room's timeline, or a not-yet-confirmed local echo of one
/// (plan §3: keyed by `event_id | txn_id`).
#[derive(Clone, Debug, PartialEq)]
pub struct TimelineItem {
    /// This item's confirmed event id, once the server has assigned one.
    /// `None` only for a [`SendState::LocalEcho`]/[`SendState::Sending`]
    /// item that has not been reconciled yet.
    pub event_id: Option<EventId>,
    /// The `txn_id` this item was (or will be) sent with, if it
    /// originated locally -- kept even after reconciliation, as
    /// provenance.
    pub txn_id: Option<TxnId>,
    /// The item's sender.
    pub sender: UserId,
    /// Milliseconds since the Unix epoch. For a local echo, this is
    /// whatever client-side clock reading [`Timeline::push_local_echo`]'s
    /// caller supplied, since the server has not assigned a real
    /// `origin_server_ts` yet.
    pub origin_server_ts: i64,
    /// The wire event type this item was built from.
    pub event_type: String,
    /// The wire event's own `state_key`, if it was a state event.
    pub state_key: Option<String>,
    /// This item's interpreted content.
    pub content: ItemContent,
    /// This item's own reply/thread pointer, plus every reaction/edit
    /// relation other events currently contribute onto it
    /// (`crate::room::relations`).
    pub relations: RelationsBundle,
    /// Where this item sits in its own send lifecycle.
    pub send_state: SendState,
    /// `true` once [`Timeline::apply_redaction`] (or an already-redacted
    /// event straight off the wire) has stripped this item's content.
    pub redacted: bool,
    /// The original wire `content` this item was built from, kept so
    /// [`Timeline::apply_redaction`] can rebuild [`TimelineItem::content`]
    /// from the v11 allow-list without needing the original
    /// [`RawEvent`] again. `Null` for a local echo (nothing has been sent
    /// yet).
    raw_content: serde_json::Value,
}

fn interpret_content(event_type: &str, state_key: Option<&str>, content: &serde_json::Value) -> ItemContent {
    if let Some(state_key) = state_key {
        return interpret_state_change(event_type, state_key, content);
    }
    match event_type {
        EVENT_ROOM_MESSAGE => match serde_json::from_value::<RoomMessageContent>(content.clone()) {
            Ok(RoomMessageContent::Text(inner)) => ItemContent::Text(inner),
            Ok(RoomMessageContent::Notice(inner)) => ItemContent::Notice(inner),
            Ok(RoomMessageContent::Emote(inner)) => ItemContent::Emote(inner),
            _ => ItemContent::Unknown,
        },
        EVENT_ROOM_ENCRYPTED => match serde_json::from_value::<RoomEncryptedContent>(content.clone()) {
            Ok(RoomEncryptedContent::Megolm(inner)) => ItemContent::Encrypted { session_id: inner.session_id },
            _ => ItemContent::Undecryptable {
                reason: "m.room.encrypted event in a room timeline was not the megolm algorithm".to_string(),
            },
        },
        EVENT_LEGACY_DM => match serde_json::from_value::<LegacyDmContent>(content.clone()) {
            Ok(inner) => ItemContent::LegacyDm {
                legacy_message_id: inner.legacy_message_id,
                nonce_b64: inner.nonce_b64,
                ciphertext_b64: inner.ciphertext_b64,
            },
            Err(_) => ItemContent::Unknown,
        },
        _ => ItemContent::Unknown,
    }
}

fn interpret_state_change(event_type: &str, state_key: &str, content: &serde_json::Value) -> ItemContent {
    match event_type {
        EVENT_ROOM_MEMBER => {
            let membership = content
                .get("membership")
                .and_then(|value| serde_json::from_value::<Membership>(value.clone()).ok())
                .unwrap_or(Membership::Unknown);
            let displayname = content.get("displayname").and_then(serde_json::Value::as_str).map(str::to_string);
            ItemContent::StateChange(StateChangeSummary::Member {
                user_id: state_key.to_string(),
                membership,
                displayname,
            })
        }
        EVENT_ROOM_NAME => {
            let name = content.get("name").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
            ItemContent::StateChange(StateChangeSummary::Name(name))
        }
        EVENT_ROOM_TOPIC => {
            let topic = content.get("topic").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
            ItemContent::StateChange(StateChangeSummary::Topic(topic))
        }
        _ => ItemContent::Unknown,
    }
}

/// Rebuilds an item's content per the v11 redaction allow-list (research
/// doc §1.1), from whatever `content` it currently has (the server's own
/// already-stripped content for an event that arrives pre-redacted, or
/// this crate's own full `raw_content` for a live redaction --
/// [`Timeline::apply_redaction`]). Only `m.room.member` keeps anything
/// this crate renders as a distinct [`ItemContent`] variant --
/// `m.room.power_levels`/`m.room.history_visibility`/`m.room.create` do
/// have their own v11 allow-list entries too, but none of the three has a
/// dedicated [`ItemContent`] shape in this piece's scope
/// ([`StateChangeSummary`] only covers member/name/topic) and both resolve
/// to [`ItemContent::Unknown`] before and after redaction either way.
/// Every other type (every message type, and every state type with no
/// allow-list entry of its own) has nothing worth keeping.
fn content_after_redaction(event_type: &str, state_key: Option<&str>, content: &serde_json::Value) -> ItemContent {
    if event_type == EVENT_ROOM_MEMBER {
        let membership = content
            .get("membership")
            .and_then(|value| serde_json::from_value::<Membership>(value.clone()).ok())
            .unwrap_or(Membership::Unknown);
        return ItemContent::StateChange(StateChangeSummary::Member {
            user_id: state_key.unwrap_or_default().to_string(),
            membership,
            // `displayname` is not on the v11 allow-list.
            displayname: None,
        });
    }
    match event_type {
        EVENT_ROOM_POWER_LEVELS | EVENT_ROOM_HISTORY_VISIBILITY | EVENT_ROOM_CREATE => ItemContent::Unknown,
        _ => ItemContent::Redacted,
    }
}

fn build_own_relation_pointers(event: &RawEvent) -> RelationsBundle {
    let mut bundle = RelationsBundle::default();
    match event.relates_to() {
        Some(RelatesTo::InReplyTo(reply)) => bundle.reply_to = Some(reply.event_id),
        Some(RelatesTo::Thread { event_id, in_reply_to, .. }) => {
            bundle.thread_root = Some(event_id);
            if let Some(reply) = in_reply_to {
                bundle.reply_to = Some(reply.event_id);
            }
        }
        _ => {}
    }
    bundle
}

fn build_item(event: &RawEvent) -> TimelineItem {
    let already_redacted = event.unsigned.redacted_because.is_some();
    let content = if already_redacted {
        content_after_redaction(&event.event_type, event.state_key.as_deref(), &event.content)
    } else {
        interpret_content(&event.event_type, event.state_key.as_deref(), &event.content)
    };
    TimelineItem {
        event_id: Some(event.event_id.clone()),
        txn_id: event.unsigned.transaction_id.clone().map(TxnId::from),
        sender: event.sender.clone(),
        origin_server_ts: event.origin_server_ts,
        event_type: event.event_type.clone(),
        state_key: event.state_key.clone(),
        content,
        relations: build_own_relation_pointers(event),
        send_state: SendState::Sent,
        redacted: already_redacted,
        raw_content: event.content.clone(),
    }
}

/// A room's timeline: ordered items in server order, assembled from
/// `/sync` windows and `/messages` back-pages (module doc).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Timeline {
    items: Vec<TimelineItem>,
    by_event_id: BTreeMap<EventId, usize>,
    by_txn_id: BTreeMap<TxnId, usize>,
    /// For a relating event that never gets its own row (a reaction or an
    /// accepted edit): which target event id it contributed to -- so a
    /// later redaction of the RELATING event can find and retract exactly
    /// that contribution (module doc's redaction section).
    relation_contributions: BTreeMap<EventId, EventId>,
    /// Every relating event currently folded into a target's bundle,
    /// keyed by the target -- the "what remains" [`recompute_bundle`]
    /// needs after one contributor above is retracted.
    relating_events_by_target: BTreeMap<EventId, Vec<RelatingEvent>>,
    gap: Option<GapMarker>,
    older_token: Option<String>,
}

impl Timeline {
    /// An empty timeline: no items, no known gap.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every item currently held, oldest first.
    pub fn items(&self) -> &[TimelineItem] {
        &self.items
    }

    /// The item with this confirmed event id, if this timeline has one.
    pub fn item_by_event_id(&self, event_id: &EventId) -> Option<&TimelineItem> {
        self.by_event_id.get(event_id).map(|&idx| &self.items[idx])
    }

    /// The gap left by the most recent `limited: true` `/sync` window, if
    /// one is still outstanding. This piece never clears it automatically
    /// -- deciding a gap is closed (enough back-pages have filled it) is a
    /// later piece's policy call (`sync_engine.rs`, M13).
    pub fn gap(&self) -> Option<&GapMarker> {
        self.gap.as_ref()
    }

    /// The pagination token to keep fetching further back with, after the
    /// most recent [`Timeline::prepend_back_page`] call.
    pub fn older_token(&self) -> Option<&str> {
        self.older_token.as_deref()
    }

    /// Applies one `/sync` timeline window (research doc §1.4's
    /// `timeline.events`/`timeline.limited`/`timeline.prev_batch`) to this
    /// timeline. `events` must already be in the wire's own order (oldest
    /// first).
    ///
    /// - A pure relation event never gets its own row -- see the module
    ///   doc -- and instead folds onto its already-loaded target's bundle.
    /// - `m.room.redaction` never gets its own row either; it applies
    ///   [`Timeline::apply_redaction`] to its `redacts` target.
    /// - Every other event gets a [`TimelineItem`]. A duplicate
    ///   `event_id` (already present from an earlier batch) updates that
    ///   item in place rather than appending a second row.
    /// - A `txn_id` present in `unsigned.transaction_id` reconciles a
    ///   matching not-yet-confirmed local echo in place (same position,
    ///   `event_id` now known, `send_state` becomes [`SendState::Sent`])
    ///   instead of appending a duplicate.
    /// - `limited: true` records [`Timeline::gap`] (research doc §1.4).
    pub fn apply_timeline_batch(&mut self, events: &[RawEvent], limited: bool, prev_batch: Option<String>) {
        if limited {
            self.gap = Some(GapMarker { prev_batch });
        }
        for event in events {
            self.apply_one_event(event);
        }
    }

    fn apply_one_event(&mut self, event: &RawEvent) {
        if event.event_type == EVENT_ROOM_REDACTION {
            if let Ok(redaction) = serde_json::from_value::<RedactionContent>(event.content.clone()) {
                self.apply_redaction(&redaction.redacts);
            }
            return;
        }

        if event.state_key.is_none() {
            if let Some(relation) = event.relates_to() {
                if matches!(relation, RelatesTo::Annotation { .. } | RelatesTo::Replace { .. }) {
                    self.apply_relation_contribution(event, relation);
                    return;
                }
            }
        }

        let item = build_item(event);

        if let Some(txn_id) = item.txn_id.clone() {
            if let Some(&idx) = self.by_txn_id.get(&txn_id) {
                self.reconcile_local_echo(idx, item);
                return;
            }
        }

        if let Some(event_id) = item.event_id.clone() {
            if let Some(&idx) = self.by_event_id.get(&event_id) {
                self.items[idx] = item;
                return;
            }
        }

        self.insert_new_item(item);
    }

    fn apply_relation_contribution(&mut self, event: &RawEvent, relation: RelatesTo) {
        let target = match &relation {
            RelatesTo::Annotation { event_id, .. } | RelatesTo::Replace { event_id } => event_id.clone(),
            _ => return,
        };
        let Some(&idx) = self.by_event_id.get(&target) else {
            // Scope cut (module doc): a relation whose target this
            // timeline has not loaded is dropped, not queued.
            return;
        };
        let original_sender = self.items[idx].sender.clone();
        let new_content =
            if matches!(relation, RelatesTo::Replace { .. }) { event.content.get("m.new_content").cloned() } else { None };
        let relating = RelatingEvent {
            event_id: event.event_id.clone(),
            sender: event.sender.clone(),
            origin_server_ts: event.origin_server_ts,
            relation,
            new_content,
        };
        if aggregate_relation(&mut self.items[idx].relations, &original_sender, &relating) {
            self.relation_contributions.insert(relating.event_id.clone(), target.clone());
            self.relating_events_by_target.entry(target).or_default().push(relating);
        }
    }

    /// Applies a redaction to `target`, called both from
    /// [`Timeline::apply_timeline_batch`] (a live `m.room.redaction`
    /// event) and available for a caller that separately learns of one
    /// (e.g. re-processing `unsigned.redacted_because` off an event this
    /// timeline already holds un-redacted, though [`build_item`] already
    /// handles that case at insertion time).
    ///
    /// - If `target` is a normal timeline item: its content is rebuilt
    ///   from [`content_after_redaction`]'s v11 allow-list, and
    ///   [`TimelineItem::redacted`] is set.
    /// - If `target` is a relation contribution (a reaction or an
    ///   accepted edit) this timeline is still tracking: that one
    ///   contribution is dropped from its own target's history and the
    ///   bundle is [`recompute_bundle`]d from what remains.
    /// - Otherwise (an event this timeline knows nothing about) this is a
    ///   documented no-op scope cut (module doc).
    pub fn apply_redaction(&mut self, target: &EventId) {
        if let Some(&idx) = self.by_event_id.get(target) {
            let item = &mut self.items[idx];
            let stripped = content_after_redaction(&item.event_type, item.state_key.as_deref(), &item.raw_content);
            item.content = stripped;
            item.redacted = true;
            return;
        }

        let Some(bundle_target) = self.relation_contributions.remove(target) else {
            return;
        };
        if let Some(contributions) = self.relating_events_by_target.get_mut(&bundle_target) {
            contributions.retain(|relating| &relating.event_id != target);
        }
        if let Some(&target_idx) = self.by_event_id.get(&bundle_target) {
            let original_sender = self.items[target_idx].sender.clone();
            let remaining = self.relating_events_by_target.get(&bundle_target).cloned().unwrap_or_default();
            self.items[target_idx].relations = recompute_bundle(&original_sender, &remaining);
        }
    }

    fn insert_new_item(&mut self, item: TimelineItem) {
        let idx = self.items.len();
        if let Some(event_id) = &item.event_id {
            self.by_event_id.insert(event_id.clone(), idx);
        }
        if let Some(txn_id) = &item.txn_id {
            self.by_txn_id.insert(txn_id.clone(), idx);
        }
        self.items.push(item);
    }

    fn reconcile_local_echo(&mut self, idx: usize, confirmed: TimelineItem) {
        if let Some(event_id) = &confirmed.event_id {
            self.by_event_id.insert(event_id.clone(), idx);
        }
        self.items[idx] = confirmed;
    }

    /// Appends a locally-originated item before it has even been handed to
    /// the transport (plan §4.2 step 6: "gets a `LocalEcho` entry ...
    /// immediately, so the adapter can render it before any network round
    /// trip completes"). `txn_id` must be the same id the eventual `PUT
    /// /rooms/{roomId}/send/...` call uses, so a later
    /// [`Timeline::apply_timeline_batch`] call reconciles this row via
    /// `unsigned.transaction_id` instead of appending a duplicate.
    pub fn push_local_echo(&mut self, txn_id: TxnId, sender: UserId, origin_server_ts: i64, content: ItemContent) {
        let item = TimelineItem {
            event_id: None,
            txn_id: Some(txn_id),
            sender,
            origin_server_ts,
            event_type: EVENT_ROOM_MESSAGE.to_string(),
            state_key: None,
            content,
            relations: RelationsBundle::default(),
            send_state: SendState::LocalEcho,
            redacted: false,
            raw_content: serde_json::Value::Null,
        };
        self.insert_new_item(item);
    }

    /// Updates a not-yet-confirmed local echo's send state in place (e.g.
    /// `LocalEcho` -> `Sending` once handed to the transport, or ->
    /// `Failed` if the send errors out). A no-op if `txn_id` names no
    /// current item (already reconciled into a confirmed item, or never
    /// existed).
    pub fn mark_send_state(&mut self, txn_id: &TxnId, send_state: SendState) {
        if let Some(&idx) = self.by_txn_id.get(txn_id) {
            self.items[idx].send_state = send_state;
        }
    }

    /// Prepends an older page of history (`GET /rooms/{roomId}/messages`,
    /// research doc §1.5), `events` already in this timeline's own order
    /// (oldest first) -- a caller reading a `dir=b` response, which comes
    /// back newest-first, must reverse it before calling this method.
    /// `end_token` is that response's own `end` field: the token to keep
    /// paginating further back from, recorded as [`Timeline::older_token`].
    /// An event already known (by `event_id`), or a pure relation/
    /// redaction event, is skipped -- the same scope cut as
    /// [`Timeline::apply_timeline_batch`] (module doc): this page's own
    /// relations only attach to targets already loaded before this call.
    pub fn prepend_back_page(&mut self, events: &[RawEvent], end_token: Option<String>) {
        let mut new_items = Vec::with_capacity(events.len());
        for event in events {
            if event.event_type == EVENT_ROOM_REDACTION || self.by_event_id.contains_key(&event.event_id) {
                continue;
            }
            if event.state_key.is_none() {
                if let Some(relation) = event.relates_to() {
                    if matches!(relation, RelatesTo::Annotation { .. } | RelatesTo::Replace { .. }) {
                        continue;
                    }
                }
            }
            new_items.push(build_item(event));
        }
        self.items.splice(0..0, new_items);
        self.older_token = end_token;
        self.reindex();
    }

    fn reindex(&mut self) {
        self.by_event_id.clear();
        self.by_txn_id.clear();
        for (idx, item) in self.items.iter().enumerate() {
            if let Some(event_id) = &item.event_id {
                self.by_event_id.insert(event_id.clone(), idx);
            }
            if let Some(txn_id) = &item.txn_id {
                self.by_txn_id.insert(txn_id.clone(), idx);
            }
        }
    }

    /// Replaces `event_id`'s content once the crypto layer (a later
    /// piece, M7+) has decrypted it -- the hook [`ItemContent::Encrypted`]/
    /// [`ItemContent::Undecryptable`] items wait for. A no-op if this
    /// timeline holds no item for `event_id`.
    pub fn set_decrypted(&mut self, event_id: &EventId, content: ItemContent) {
        if let Some(&idx) = self.by_event_id.get(event_id) {
            self.items[idx].content = content;
        }
    }
}

/// `true` when `item`'s already-decrypted text mentions `user_id` or
/// `displayname` at a word boundary, case-insensitively (plan manager
/// decision #5: "highlight in encrypted rooms is computed client-side
/// after decryption"). Only [`ItemContent::Text`]/[`ItemContent::Notice`]/
/// [`ItemContent::Emote`] items can match -- anything else (still
/// encrypted, undecryptable, redacted, a state change, ...) never
/// highlights.
pub fn highlight(user_id: &UserId, displayname: Option<&str>, item: &TimelineItem) -> bool {
    let body = match &item.content {
        ItemContent::Text(inner) | ItemContent::Notice(inner) | ItemContent::Emote(inner) => inner.body.as_str(),
        _ => return false,
    };
    mentions_word(body, user_id.as_str()) || displayname.is_some_and(|name| !name.trim().is_empty() && mentions_word(body, name))
}

/// Word-boundary, case-insensitive substring search -- `needle` must not
/// be immediately preceded or followed by another alphanumeric character
/// in `haystack`, so `"alice"` does not match inside `"notalice123"`.
fn mentions_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let haystack_lower = haystack.to_lowercase();
    let needle_lower = needle.to_lowercase();
    let mut search_from = 0;
    while let Some(found_at) = haystack_lower[search_from..].find(&needle_lower) {
        let start = search_from + found_at;
        let end = start + needle_lower.len();
        let before_is_boundary = haystack_lower[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after_is_boundary = haystack_lower[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        if before_is_boundary && after_is_boundary {
            return true;
        }
        search_from = start + needle_lower.len().max(1);
    }
    false
}

/// The latest timeline item eligible for a chat-list preview line: not
/// still encrypted, not undecryptable, not redacted, and not a state
/// change (plan manager decision #5: "last-message preview = latest
/// decryptable timeline item"). `None` for an empty timeline, or one with
/// no eligible item at all (e.g. every message is still awaiting
/// decryption).
pub fn last_preview(timeline: &Timeline) -> Option<&TimelineItem> {
    timeline.items.iter().rev().find(|item| {
        !item.redacted
            && !matches!(
                item.content,
                ItemContent::Encrypted { .. } | ItemContent::Undecryptable { .. } | ItemContent::StateChange(_)
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_event(event_id: &str, event_type: &str, sender: &str, ts: i64, content: serde_json::Value) -> RawEvent {
        serde_json::from_value(serde_json::json!({
            "event_id": event_id,
            "type": event_type,
            "sender": sender,
            "origin_server_ts": ts,
            "content": content,
        }))
        .expect("valid raw event")
    }

    fn raw_event_with_txn(
        event_id: &str,
        event_type: &str,
        sender: &str,
        ts: i64,
        content: serde_json::Value,
        txn_id: &str,
    ) -> RawEvent {
        serde_json::from_value(serde_json::json!({
            "event_id": event_id,
            "type": event_type,
            "sender": sender,
            "origin_server_ts": ts,
            "content": content,
            "unsigned": { "transaction_id": txn_id },
        }))
        .expect("valid raw event")
    }

    fn text_content(body: &str) -> serde_json::Value {
        serde_json::json!({ "msgtype": "m.text", "body": body })
    }

    fn text_item(body: &str) -> TimelineItem {
        TimelineItem {
            event_id: Some(EventId::parse("$x:example.org").expect("valid event id")),
            txn_id: None,
            sender: UserId::parse("@someone:example.org").expect("valid user id"),
            origin_server_ts: 1,
            event_type: EVENT_ROOM_MESSAGE.to_string(),
            state_key: None,
            content: ItemContent::Text(TextLikeMessageContent { body: body.to_string(), format: None, formatted_body: None }),
            relations: RelationsBundle::default(),
            send_state: SendState::Sent,
            redacted: false,
            raw_content: serde_json::Value::Null,
        }
    }

    #[test]
    fn apply_timeline_batch_reconciles_local_echo_by_txn_id() {
        let mut timeline = Timeline::new();
        let alice = UserId::parse("@alice:example.org").expect("valid user id");
        let txn_id = TxnId::new(0);
        timeline.push_local_echo(
            txn_id.clone(),
            alice,
            100,
            ItemContent::Text(TextLikeMessageContent { body: "hi".to_string(), format: None, formatted_body: None }),
        );
        assert_eq!(timeline.items().len(), 1);
        assert_eq!(timeline.items()[0].send_state, SendState::LocalEcho);
        assert!(timeline.items()[0].event_id.is_none());

        let confirmed =
            raw_event_with_txn("$confirmed:example.org", "m.room.message", "@alice:example.org", 105, text_content("hi"), txn_id.as_str());
        timeline.apply_timeline_batch(&[confirmed], false, None);

        assert_eq!(timeline.items().len(), 1, "the echo is replaced in place, not duplicated");
        let item = &timeline.items()[0];
        assert_eq!(item.event_id, Some(EventId::parse("$confirmed:example.org").expect("valid event id")));
        assert_eq!(item.send_state, SendState::Sent);
        assert_eq!(item.txn_id, Some(txn_id));
    }

    #[test]
    fn apply_redaction_keeps_v11_allow_listed_fields() {
        let mut timeline = Timeline::new();
        let member_event: RawEvent = serde_json::from_value(serde_json::json!({
            "event_id": "$member:example.org",
            "type": "m.room.member",
            "sender": "@alice:example.org",
            "origin_server_ts": 1,
            "state_key": "@bob:example.org",
            "content": { "membership": "join", "displayname": "Bob" }
        }))
        .expect("valid member event");
        let message_event = raw_event("$msg:example.org", "m.room.message", "@bob:example.org", 2, text_content("hello"));
        timeline.apply_timeline_batch(&[member_event, message_event], false, None);

        let member_id = EventId::parse("$member:example.org").expect("valid event id");
        timeline.apply_redaction(&member_id);
        let member_item = timeline.item_by_event_id(&member_id).expect("present");
        assert!(member_item.redacted);
        match &member_item.content {
            ItemContent::StateChange(StateChangeSummary::Member { membership, displayname, .. }) => {
                assert_eq!(*membership, Membership::Join, "membership survives the v11 allow-list");
                assert_eq!(*displayname, None, "displayname is not on the v11 allow-list");
            }
            other => panic!("expected a Member state change, got {other:?}"),
        }

        let msg_id = EventId::parse("$msg:example.org").expect("valid event id");
        timeline.apply_redaction(&msg_id);
        let msg_item = timeline.item_by_event_id(&msg_id).expect("present");
        assert!(msg_item.redacted);
        assert_eq!(msg_item.content, ItemContent::Redacted, "m.room.message has nothing on the v11 allow-list");
    }

    #[test]
    fn redacting_a_reaction_removes_it_from_the_bundle() {
        let mut timeline = Timeline::new();
        let target = raw_event("$target:example.org", "m.room.message", "@alice:example.org", 1, text_content("hi"));
        let reaction = raw_event(
            "$reaction:example.org",
            "m.reaction",
            "@bob:example.org",
            2,
            serde_json::json!({
                "m.relates_to": { "rel_type": "m.annotation", "event_id": "$target:example.org", "key": "\u{1F44D}" }
            }),
        );
        timeline.apply_timeline_batch(&[target, reaction], false, None);

        let target_id = EventId::parse("$target:example.org").expect("valid event id");
        let bundle_before = timeline.item_by_event_id(&target_id).expect("present").relations.clone();
        assert_eq!(bundle_before.reactions.get("\u{1F44D}").expect("reaction present").len(), 1);
        assert_eq!(timeline.items().len(), 1, "the reaction event never gets its own row");

        timeline.apply_redaction(&EventId::parse("$reaction:example.org").expect("valid event id"));
        let bundle_after = &timeline.item_by_event_id(&target_id).expect("present").relations;
        assert!(
            !bundle_after.reactions.contains_key("\u{1F44D}"),
            "the retracted reaction is gone entirely, not just emptied"
        );
    }

    #[test]
    fn limited_batch_records_a_gap() {
        let mut timeline = Timeline::new();
        assert!(timeline.gap().is_none());

        let event = raw_event("$1:example.org", "m.room.message", "@alice:example.org", 1, text_content("hi"));
        timeline.apply_timeline_batch(&[event], true, Some("t123".to_string()));

        let gap = timeline.gap().expect("limited batch records a gap");
        assert_eq!(gap.prev_batch.as_deref(), Some("t123"));
        assert_eq!(timeline.items().len(), 1, "the window's own events are still applied");
    }

    #[test]
    fn last_preview_skips_redacted_and_state() {
        let mut timeline = Timeline::new();
        let text = raw_event("$text:example.org", "m.room.message", "@alice:example.org", 1, text_content("first"));
        let name_change: RawEvent = serde_json::from_value(serde_json::json!({
            "event_id": "$name:example.org", "type": "m.room.name", "sender": "@alice:example.org",
            "origin_server_ts": 2, "state_key": "", "content": { "name": "New name" }
        }))
        .expect("valid name event");
        let redacted: RawEvent = serde_json::from_value(serde_json::json!({
            "event_id": "$redacted:example.org", "type": "m.room.message", "sender": "@alice:example.org",
            "origin_server_ts": 3, "content": {},
            "unsigned": { "redacted_because": {
                "event_id": "$r:example.org", "type": "m.room.redaction", "sender": "@mod:example.org",
                "origin_server_ts": 4, "content": { "redacts": "$redacted:example.org" }
            }}
        }))
        .expect("valid redacted event");

        timeline.apply_timeline_batch(&[text, name_change, redacted], false, None);

        let preview = last_preview(&timeline).expect("an eligible item exists");
        assert_eq!(preview.event_id, Some(EventId::parse("$text:example.org").expect("valid event id")));
    }

    #[test]
    fn highlight_matches_mention_not_substring() {
        let alice = UserId::parse("@alice:example.org").expect("valid user id");

        let mentioned = text_item("hey @alice:example.org, are you around?");
        assert!(highlight(&alice, Some("Alice"), &mentioned));

        let by_displayname = text_item("Alice, can you review this?");
        assert!(highlight(&alice, Some("Alice"), &by_displayname));

        let substring_only = text_item("notalice123 says hi to nobody");
        assert!(!highlight(&alice, Some("Alice"), &substring_only));
    }
}
