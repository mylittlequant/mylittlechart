//! Rooms — the chat-list row / open-conversation-header contract.

use crate::ids::{RoomId, TagName, UserId};
use crate::member::MemberRole;
use crate::misc::{PowerLevel, Timestamp};

/// What kind of room this is — derived from room state (membership shape,
/// join rule, power levels), never a separately stored flag the UI could
/// drift out of sync with the state it was computed from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomKind {
    Direct,
    Group,
    Channel,
}

/// One room-tag membership (Matrix `m.tag`: `{tag_name: {order: f64}}`) — a
/// custom folder's identity IS a tag name (see [`crate::Folder`]'s own
/// doc); a room can carry several at once, each with its own drag-sort
/// order.
#[derive(Clone, Debug, PartialEq)]
pub struct Tag {
    pub name: TagName,
    pub order: f64,
}

/// The shape of a room's last event, for the chat-list row's own preview
/// line — never the raw event: a sealed room shows [`PreviewContent::
/// Encrypted`] regardless of what the event actually held, the same
/// "ciphertext proves the encryption is real" posture the DM pane already
/// has.
#[derive(Clone, Debug, PartialEq)]
pub enum PreviewContent {
    Text(String),
    Encrypted,
    Redacted,
    Notice(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct LastEventPreview {
    pub sender: UserId,
    pub content: PreviewContent,
    pub ts: Timestamp,
}

/// One row of the chat list, and the header of that room once open —
/// everything the concept's row (`.row`) and chat header (`.ch`) need in
/// one read, no second fetch.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomSummary {
    pub id: RoomId,
    pub kind: RoomKind,
    pub name: String,
    pub initials: String,
    pub is_encrypted: bool,
    pub is_public: bool,
    pub notification_count: u64,
    pub highlight_count: u64,
    pub last_event: Option<LastEventPreview>,
    pub muted: bool,
    pub pinned: bool,
    pub archived: bool,
    pub tags: Vec<Tag>,
    pub typing_user_ids: Vec<UserId>,
    /// The peer's own last-active moment (DM only — a group/channel has no
    /// single "peer", so this stays `None` there). Presence is never a
    /// pushed stream (owner ruling, Matrix amendment §7.2 — no XMPP-style
    /// event storms): the row's online dot comes from comparing this
    /// against "now", not a live socket.
    pub peer_last_active: Option<Timestamp>,
    pub my_role: MemberRole,
    pub my_power_level: PowerLevel,
    pub can_post: bool,
}

impl RoomSummary {
    /// Whether this room currently has anything unread — pure, shared by
    /// every caller (folder rules, the rail's own badge) that needs the
    /// `> 0` check rather than duplicating it inline.
    pub fn has_unread(&self) -> bool {
        self.notification_count > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(notification_count: u64) -> RoomSummary {
        RoomSummary {
            id: RoomId::from("!r:x"),
            kind: RoomKind::Direct,
            name: "peer".to_string(),
            initials: "P".to_string(),
            is_encrypted: true,
            is_public: false,
            notification_count,
            highlight_count: 0,
            last_event: None,
            muted: false,
            pinned: false,
            archived: false,
            tags: Vec::new(),
            typing_user_ids: Vec::new(),
            peer_last_active: None,
            my_role: MemberRole::Member,
            my_power_level: PowerLevel(0),
            can_post: true,
        }
    }

    #[test]
    fn has_unread_is_the_notification_count_check() {
        assert!(!summary(0).has_unread());
        assert!(summary(1).has_unread());
    }
}
