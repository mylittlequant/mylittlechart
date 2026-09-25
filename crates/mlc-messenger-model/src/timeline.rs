//! Timeline items — one open room's own message list. Relations (edits,
//! redactions, reactions, reply previews) arrive ALREADY AGGREGATED onto
//! the item; this crate has no raw relation-event type and the UI must
//! never re-aggregate one (coordinator ruling 2026-09-24).

use crate::ids::{EventId, UserId};
use crate::member::{MemberRole, RankBadge};
use crate::misc::Timestamp;

/// Why a message could not be decrypted — Matrix's own taxonomy of "why did
/// the room key never open this" (`m.room_key.withheld`'s own `code`
/// values, plus the plain "no session at all" case), surfaced so a bubble
/// can say something more useful than a generic "cannot decrypt".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UtdReason {
    /// No session key for this message has reached this device at all.
    MissingSession,
    /// The sender's device blacklisted this device explicitly.
    WithheldBlacklisted,
    /// The sender's device withheld the key because this device is
    /// unverified.
    WithheldUnverified,
    /// The sender's device withheld the key because this device is not
    /// authorised (not a room member, or removed since).
    WithheldUnauthorised,
    /// The sender's own identity has changed and is not (yet) re-verified
    /// — decryption is refused until it is, not merely delayed.
    SenderIdentityNotVerified,
    Unknown,
}

/// A membership change rendered inline in the timeline (join/leave/invite/
/// role change) — Matrix `m.room.member` state events, surfaced as a plain
/// system-notice row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MembershipChange {
    Joined,
    Left,
    Invited,
    Removed,
    RoleChanged(MemberRole),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MembershipEvent {
    pub user: UserId,
    pub change: MembershipChange,
}

/// One timeline item's content, at whatever stage of decryption the
/// provider has reached — see [`TimelineItem`]'s own doc for why edits/
/// reactions/replies are never re-derived from raw relation events here.
#[derive(Clone, Debug, PartialEq)]
pub enum MessageContent {
    Text(String),
    /// Sealed, but plausibly openable later (the vault is locked, or a
    /// session key has not arrived yet) — renders as raw ciphertext,
    /// dimmed, the same "proof the encryption is real" posture the DM pane
    /// already has (owner ruling 2026-09-10).
    Encrypted { ciphertext_b64: String },
    /// Structurally cannot be opened — see [`UtdReason`].
    Undecryptable(UtdReason),
    Notice(String),
    Membership(MembershipEvent),
    /// A device/session-key rotation notice inline in the timeline —
    /// distinct from a per-message [`UtdReason`]: this is a standalone
    /// system event, not a property of one message.
    KeyChanged,
    /// A message that predates the Matrix-shaped migration (today's 1:1 DM
    /// model, `dm_messages`) — carried in the SAME timeline as native
    /// Matrix events so a migrated DM room reads as one continuous
    /// history rather than a hard cut. The UI decides on its own "legacy"
    /// marker/badge from this variant alone; the provider adds no separate
    /// flag for it.
    Legacy(LegacyContent),
}

/// A pre-migration message's own content — decrypted, or still sealed
/// under the OLD (pre-Matrix) DM key scheme (`mlc-vault`'s `dm::
/// seal_message`, unrelated to Megolm). Never [`UtdReason`]: a legacy
/// message's ciphertext is expected to open once ITS OWN key (the old DM
/// conversation key, not a Matrix room key) is available, which is not a
/// Matrix decryption failure at all.
#[derive(Clone, Debug, PartialEq)]
pub enum LegacyContent {
    Text(String),
    Encrypted { ciphertext_b64: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReplyPreview {
    pub event_id: EventId,
    pub sender: UserId,
    pub preview: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReactionSummary {
    pub key: String,
    pub count: u32,
    pub by_me: bool,
}

/// A shared chart/preset attachment on one timeline item (the concept's own
/// `.card`: symbol/timeframe/venue header, a preview, and Open/Clone
/// actions) — a Matrix custom-event attachment, carried ALONGSIDE
/// [`MessageContent::Text`] on the same item rather than as its own
/// `MessageContent` variant: the concept's own message bubble shows BOTH a
/// card AND a text line under it in the same bubble, the same "relation
/// arrives already aggregated onto the item" shape [`TimelineItem::
/// reactions`]/`reply_to` already use for their own attachments.
#[derive(Clone, Debug, PartialEq)]
pub struct SharedChartCard {
    pub symbol: String,
    pub timeframe: String,
    pub venue: String,
    /// The card's own subtitle line (concept: "SMC scan · 2 order blocks ·
    /// shared chart") — free text the sender wrote, not derived from the
    /// symbol/timeframe/venue above.
    pub note: String,
}

/// Where one item sits in its own send lifecycle. `Sent` is a durable,
/// server-accepted fact; READ status is deliberately not a variant here —
/// see [`TimelineItem::read_by`]: a message can be read by some recipients
/// and not others at once, which a single enum state cannot represent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendState {
    /// Appended optimistically, not yet handed to the transport.
    LocalEcho,
    /// Handed to the transport, no server acknowledgement yet.
    Sending,
    /// The server accepted it — a durable timeline event from here on.
    Sent,
    Failed { reason: String },
}

/// One row of an open room's timeline.
#[derive(Clone, Debug, PartialEq)]
pub struct TimelineItem {
    pub event_id: EventId,
    pub sender: UserId,
    pub sender_rank: Option<RankBadge>,
    pub ts: Timestamp,
    pub content: MessageContent,
    pub reply_to: Option<ReplyPreview>,
    /// `true` once an `m.replace` relation has landed for this event — the
    /// content above is already the REPLACED text; this is only the
    /// "edited" badge.
    pub edited: bool,
    pub reactions: Vec<ReactionSummary>,
    pub redacted: bool,
    pub send_state: SendState,
    /// User ids whose read receipt has advanced to (or past) this event —
    /// aggregated per-event, same "no raw relation walk in the UI" rule as
    /// the rest of this type.
    pub read_by: Vec<UserId>,
    pub pinned: bool,
    /// A shared chart/preset attachment, if the sender attached one — see
    /// [`SharedChartCard`]'s own doc.
    pub shared_chart: Option<SharedChartCard>,
    /// Whether this event's push rules highlighted the CURRENT viewer
    /// specifically (an `@mention` of them, or an equivalent room-level
    /// rule) — aggregated per-event, the same per-message counterpart to
    /// [`crate::RoomSummary::highlight_count`]'s own room-level total.
    /// Drives the bubble's own accent outline; never re-derived from the
    /// message text (a plain "@word" token elsewhere in the text is styled
    /// on its own, independent of whether it is actually THIS viewer).
    pub mentions_me: bool,
}
