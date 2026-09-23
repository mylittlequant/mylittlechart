//! Timeline items — one open room's own message list. Relations (edits,
//! redactions, reactions, reply previews) arrive ALREADY AGGREGATED onto
//! the item; this crate has no raw relation-event type and the UI must
//! never re-aggregate one (coordinator ruling 2026-09-24).

use crate::ids::{EventId, UserId};
use crate::member::{MemberRole, RankBadge};
use crate::misc::Timestamp;

/// Why a message could not be decrypted — Matrix's own taxonomy of "why did
/// the room key never open this", surfaced so a bubble can say something
/// more useful than a generic "cannot decrypt".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UtdReason {
    /// No session key for this message has reached this device at all.
    MissingSessionKey,
    /// A session key was explicitly withheld (`m.room_key.withheld`) — the
    /// sender's device refused to share it with this one.
    Withheld,
    /// The sending device is unknown/unverified and the account's own
    /// policy refuses to decrypt from it.
    UnknownDevice,
    /// The session key existed once but has since been rotated out and
    /// this device holds no backup of it.
    LegacyKeyUnavailable,
    Other(String),
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
}
