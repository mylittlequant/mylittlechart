//! `MessengerProvider` adapter over TODAY's 1:1 DM state — deliverable B
//! of the messenger UI-1 piece (`docs/mlc/plans/community-messenger-arc-
//! 2026-09-24.md` §7.3). Lets the new messenger UI (built against
//! `mlc_messenger_model::MessengerProvider`) run against the existing DM
//! server/crypto path unchanged, before `messenger-proto`'s Matrix-shaped
//! provider exists.
//!
//! SWC placement: `DmState` is owned by `CommunityState` in this crate
//! already (`ui::modal_settings`) — this adapter BORROWS it, owns nothing
//! of its own, and both shells keep reading/writing `DmState`/
//! `DmState::request` exactly as they do today (`main.rs`'s DM effect
//! block, `cloud_session.rs`'s own drain). `DmMessengerProvider` is a
//! throwaway view constructed fresh wherever the messenger UI needs one
//! this frame — it is not stored anywhere.
//!
//! Every method here is a straight, honest mapping — no field is invented.
//! Where today's DM model simply carries less than a Matrix room would
//! (no peer user id, no per-message rank, no room-level "last message"
//! text, no pagination token, no folders/reactions/typing/presence), the
//! mapping says so in that method's own doc rather than synthesizing a
//! plausible-looking placeholder.

use mlc_messenger_model::{
    initials_from_name, BackPagination, Capabilities, EventId, Folder, Member, MemberRole,
    MessageContent, MessengerIntent, MessengerProvider, PowerLevel, RoomId, RoomKind, RoomSummary,
    SendState, Timestamp, TimelineItem, UserId, UtdReason, VaultState,
};

use super::modal_settings::{DmMessageBody, DmMessagePage, DmRequest, DmState, DmUnavailableReason, DmView};

/// The `UserId` this adapter uses for the signed-in caller's own messages
/// — DM has no numeric "my user id" surfaced to this crate (the server
/// resolves the caller from the session), so a fixed opaque marker stands
/// in; stable within one conversation, which is all a single-provider UI
/// ever compares it against.
const ME: &str = "me";

/// Adapter over `&mut DmState` — reads borrow it immutably per call,
/// [`MessengerProvider::enqueue`] borrows it mutably to stamp
/// [`DmState::request`], the SAME one-shot mailbox the shells already
/// drain (no new field, no new shell wiring).
pub struct DmMessengerProvider<'a> {
    state: &'a mut DmState,
}

impl<'a> DmMessengerProvider<'a> {
    pub fn new(state: &'a mut DmState) -> Self {
        Self { state }
    }

    fn room_id(id: i64) -> RoomId {
        RoomId::from(id.to_string())
    }

    fn parse_room_id(room: &RoomId) -> Option<i64> {
        room.as_str().parse().ok()
    }

    fn event_id(id: i64) -> EventId {
        EventId::from(id.to_string())
    }

    fn parse_event_id(event_id: &EventId) -> Option<i64> {
        event_id.as_str().parse().ok()
    }

    /// Peer `UserId` for a conversation — DM never surfaces the peer's own
    /// numeric account id to this crate, only their display label
    /// (`DmConversationUi::peer_label`), so that label doubles as the
    /// opaque id. Two DIFFERENT peers who happen to share a label would
    /// collide here — an accepted, narrow gap of a bridge adapter over a
    /// model that never needed a real user-id concept before.
    fn peer_user_id(peer_label: &str) -> UserId {
        UserId::from(peer_label)
    }

    fn timestamp(rfc3339: &str) -> Timestamp {
        let secs = crate::layout::modals::cloud_overlay::parse_rfc3339_unix_secs(rfc3339).unwrap_or(0);
        Timestamp(secs.saturating_mul(1000))
    }
}

impl MessengerProvider for DmMessengerProvider<'_> {
    /// Derived, not stored — `DmState` carries no counter field of its own
    /// (adding one would mean touching every shell's fetch-landing code,
    /// which this adapter must not require: "shells keep feeding it
    /// exactly as now"). Combines the two freshness stamps the shells
    /// ALREADY set on every successful fetch (`conversations_fetched_at`/
    /// `messages_fetched_at`) with the open conversation's own message
    /// count. Not a perfect edge-to-edge signal (an in-place ciphertext
    /// redecrypt touches neither stamp) — good enough for "is it worth
    /// re-reading a snapshot this frame", which is all a change counter is
    /// for.
    fn change_counter(&self) -> u64 {
        let convo_stamp = self.state.conversations_fetched_at.unwrap_or(0);
        let msg_stamp = self.state.messages_fetched_at.unwrap_or(0);
        let msg_count = match &self.state.view {
            DmView::Conversation { messages, .. } => messages.len() as u64,
            DmView::List => 0,
        };
        convo_stamp.wrapping_add(msg_stamp.wrapping_mul(1_000_003)).wrapping_add(msg_count)
    }

    /// Today's DM server: 1:1 only, no groups/channels/folders/reactions/
    /// edits/typing. `receipts: true` — `DmMessageUi::read_at` already
    /// carries a read timestamp per message (unused by `dm_overlay.rs`'s
    /// own drawing today, but present on the wire).
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            groups: false,
            channels: false,
            folders: false,
            reactions: false,
            edits: false,
            replies: false,
            typing: false,
            receipts: true,
        }
    }

    /// `DmUnavailableReason::NotPaid` is an ACCESS-LADDER fact, not a vault
    /// state — it does not change what this method reports (a not-paid
    /// account's vault can still be genuinely unlocked); the ladder gate
    /// itself surfaces through [`Self::last_error`] and each room's own
    /// `can_post`, same as it does in `dm_overlay.rs` today.
    fn vault_state(&self) -> VaultState {
        if matches!(self.state.unavailable_reason, DmUnavailableReason::VaultLocked) {
            return VaultState::Locked;
        }
        if self.state.identity_fingerprint.is_none() {
            return VaultState::NeedsSetup;
        }
        VaultState::Ready
    }

    fn last_error(&self) -> Option<String> {
        self.state.last_error.clone()
    }

    fn rooms(&self) -> Vec<RoomSummary> {
        self.state.conversations.iter().map(|c| RoomSummary {
            id: Self::room_id(c.id),
            kind: RoomKind::Direct,
            name: c.peer_label.clone(),
            initials: initials_from_name(&c.peer_label),
            is_encrypted: true,
            is_public: false,
            notification_count: c.unread_count.max(0) as u64,
            highlight_count: 0,
            // `DmConversationUi` carries a last-message TIMESTAMP
            // (`last_message_at`) but no last-message TEXT/ciphertext at
            // all — the row preview is drawn from a fetched message list
            // in today's UI, not a summary field. Honestly `None` rather
            // than a synthesized "Encrypted" marker with a fabricated
            // timestamp/sender.
            last_event: None,
            muted: false,
            pinned: false,
            archived: false,
            tags: Vec::new(),
            typing_user_ids: Vec::new(),
            peer_last_active: None,
            my_role: MemberRole::Member,
            my_power_level: PowerLevel(0),
            can_post: self.state.available,
        }).collect()
    }

    fn room(&self, room: &RoomId) -> Option<RoomSummary> {
        let id = Self::parse_room_id(room)?;
        self.rooms().into_iter().find(|r| r.id == Self::room_id(id))
    }

    fn timeline(&self, room: &RoomId) -> Vec<TimelineItem> {
        let Some(id) = Self::parse_room_id(room) else { return Vec::new() };
        let DmView::Conversation { id: open_id, messages, .. } = &self.state.view else { return Vec::new() };
        if *open_id != id {
            return Vec::new();
        }
        let peer_label = self.state.conversations.iter().find(|c| c.id == id).map(|c| c.peer_label.as_str()).unwrap_or("peer");
        let last_idx = messages.len().saturating_sub(1);
        messages.iter().enumerate().map(|(i, m)| {
            let content = match &m.body {
                DmMessageBody::Plain(text) => MessageContent::Text(text.clone()),
                DmMessageBody::Ciphertext { ciphertext_b64, .. } => {
                    MessageContent::Encrypted { ciphertext_b64: ciphertext_b64.clone() }
                }
                // Today's DM model carries no MORE specific reason than
                // "the seal failed" — see `DmMessageBody::Undecryptable`'s
                // own doc; `Unknown` is the honest catch-all, not a guess
                // at which Matrix-shaped reason it would have been.
                DmMessageBody::Undecryptable => MessageContent::Undecryptable(UtdReason::Unknown),
            };
            let sending = m.mine && self.state.send_pending && i == last_idx;
            TimelineItem {
                event_id: Self::event_id(m.id),
                sender: if m.mine { UserId::from(ME) } else { Self::peer_user_id(peer_label) },
                // DM carries no per-message ladder rank.
                sender_rank: None,
                ts: Self::timestamp(&m.created_at),
                content,
                reply_to: None,
                edited: false,
                reactions: Vec::new(),
                redacted: false,
                send_state: if sending { SendState::Sending } else { SendState::Sent },
                read_by: if m.read_at.is_some() { vec![Self::peer_user_id(peer_label)] } else { Vec::new() },
                pinned: false,
            }
        }).collect()
    }

    /// DM pages by message id, not an opaque server cursor (`DmMessagePage`'s
    /// own doc) — `token` stays `None`; [`Self::enqueue`]'s own
    /// `LoadOlder` handling derives the `Before(lowest_id_held)` request
    /// straight from the currently-held timeline instead of round-tripping
    /// a token through this method.
    fn back_pagination(&self, room: &RoomId) -> BackPagination {
        let Some(id) = Self::parse_room_id(room) else { return BackPagination::default() };
        match &self.state.view {
            DmView::Conversation { id: open_id, has_older, .. } if *open_id == id => {
                BackPagination { token: None, has_more: *has_older }
            }
            _ => BackPagination::default(),
        }
    }

    /// DM is always exactly two participants — "me" and the conversation's
    /// own peer. `has_messaging_key` reads the peer's published DM
    /// identity fingerprint (`None` there means the same "no key
    /// published" state `dm_overlay.rs`'s header already shows).
    fn members(&self, room: &RoomId) -> Vec<Member> {
        let Some(id) = Self::parse_room_id(room) else { return Vec::new() };
        let Some(c) = self.state.conversations.iter().find(|c| c.id == id) else { return Vec::new() };
        vec![
            Member {
                user_id: UserId::from(ME),
                display_name: "You".to_string(),
                initials: "Y".to_string(),
                rank: None,
                role: MemberRole::Member,
                power_level: PowerLevel(0),
                online: false,
                last_active: None,
                has_messaging_key: self.state.identity_fingerprint.is_some(),
            },
            Member {
                user_id: Self::peer_user_id(&c.peer_label),
                display_name: c.peer_label.clone(),
                initials: initials_from_name(&c.peer_label),
                rank: None,
                role: MemberRole::Member,
                power_level: PowerLevel(0),
                online: false,
                last_active: None,
                has_messaging_key: c.peer_fingerprint.is_some(),
            },
        ]
    }

    /// No folders in today's DM model.
    fn folders(&self) -> Vec<Folder> {
        Vec::new()
    }

    /// Only `SendText`/`MarkRead`/`LoadOlder` have a real DM equivalent
    /// (matches [`Self::capabilities`]'s own flags) — everything else is
    /// refused with a `last_error`, never a panic: the UI is expected to
    /// gate these behind `capabilities()`, but a stale render (a frame
    /// drawn against a DIFFERENT, richer provider's affordances, then
    /// clicked a frame after the provider swapped back to this one) must
    /// still land somewhere safe. DM's `request` is a single-slot mailbox
    /// (`DmState::request: Option<DmRequest>`, same shape this method
    /// mirrors) — a second `enqueue` before the shell drains the first
    /// overwrites it, exactly like today's one `DmRequest` at a time.
    fn enqueue(&mut self, intent: MessengerIntent) {
        match intent {
            MessengerIntent::SendText { room, text, .. } => {
                if let Some(id) = Self::parse_room_id(&room) {
                    self.state.request = Some(DmRequest::Send { id, plaintext: text });
                }
            }
            MessengerIntent::MarkRead { room, event_id } => {
                if let (Some(id), Some(up_to)) = (Self::parse_room_id(&room), Self::parse_event_id(&event_id)) {
                    self.state.request = Some(DmRequest::MarkRead { id, up_to });
                }
            }
            MessengerIntent::LoadOlder { room } => {
                let Some(id) = Self::parse_room_id(&room) else { return };
                let lowest = match &self.state.view {
                    DmView::Conversation { id: open_id, messages, .. } if *open_id == id => {
                        messages.first().map(|m| m.id)
                    }
                    _ => None,
                };
                if let Some(lowest_id) = lowest {
                    self.state.request = Some(DmRequest::LoadMessages { id, page: DmMessagePage::Before(lowest_id) });
                }
            }
            _ => {
                self.state.last_error = Some("not supported by the direct-message provider".to_string());
            }
        }
    }
}
