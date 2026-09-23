//! Folders — the chat-list rail's grouping. Built-ins (All chats /
//! Personal / Groups / Channels / Archive) are computed client-side from
//! `RoomKind`/`archived`, never round-tripped. A custom folder's identity
//! is a room tag ([`crate::TagName`]) — folders are tags, not a separate
//! store (coordinator ruling 2026-09-24).

use crate::ids::{RoomId, TagName};
use crate::room::{RoomKind, RoomSummary};

/// Icon glyph for a folder row — the fixed set the approved concept draws
/// (`i-candles`/`i-folder`/`i-group`/`i-channel`/`i-bookmark`/`i-person`),
/// typed rather than a free string so a harvested widget can match on it
/// exhaustively instead of falling back to a default glyph on typos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderIcon {
    Candles,
    Folder,
    Group,
    Channel,
    Bookmark,
    Person,
}

/// Which room kinds/rooms a folder pulls in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FolderInclude {
    pub all_dms: bool,
    pub all_groups: bool,
    pub all_channels: bool,
    pub rooms: Vec<RoomId>,
}

/// Which rooms a folder drops even if [`FolderInclude`] would otherwise
/// pull them in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FolderExclude {
    pub muted: bool,
    pub read: bool,
    pub archived: bool,
}

/// A chat-list folder. Built-in folders carry `id: None`. A CUSTOM
/// folder's identity is a [`TagName`] (`id: Some(..)`) — creating one is
/// tagging the rooms it should hold (Matrix `m.tag`), and `order` is that
/// tag's own drag-sort order: a plain `f64` (Matrix convention), not an
/// integer index, so two folders can be reordered by picking a value
/// between their neighbours without rewriting every other one.
#[derive(Clone, Debug, PartialEq)]
pub struct Folder {
    pub id: Option<TagName>,
    pub name: String,
    pub icon: FolderIcon,
    pub include: FolderInclude,
    pub exclude: FolderExclude,
    pub order: f64,
}

impl Folder {
    /// Whether `room` belongs in this folder — pure: an include rule
    /// (kind-wide, a named room, or `room` directly carrying this folder's
    /// own tag) that survives every exclude rule. A room dragged directly
    /// onto a folder icon in the concept always counts, regardless of the
    /// include rules, the same way it would if it were separately listed.
    pub fn matches(&self, room: &RoomSummary) -> bool {
        if self.exclude.muted && room.muted {
            return false;
        }
        if self.exclude.read && !room.has_unread() {
            return false;
        }
        if self.exclude.archived && room.archived {
            return false;
        }
        if let Some(tag) = &self.id {
            if room.tags.iter().any(|t| &t.name == tag) {
                return true;
            }
        }
        let kind_included = match room.kind {
            RoomKind::Direct => self.include.all_dms,
            RoomKind::Group => self.include.all_groups,
            RoomKind::Channel => self.include.all_channels,
        };
        kind_included || self.include.rooms.contains(&room.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::member::MemberRole;
    use crate::misc::PowerLevel;
    use crate::room::Tag;

    fn room(kind: RoomKind, muted: bool, archived: bool, notification_count: u64) -> RoomSummary {
        RoomSummary {
            id: RoomId::from("!r:x"),
            kind,
            name: "x".to_string(),
            initials: "X".to_string(),
            is_encrypted: true,
            is_public: false,
            notification_count,
            highlight_count: 0,
            last_event: None,
            muted,
            pinned: false,
            archived,
            tags: Vec::new(),
            typing_user_ids: Vec::new(),
            peer_last_active: None,
            my_role: MemberRole::Member,
            my_power_level: PowerLevel(0),
            can_post: true,
        }
    }

    #[test]
    fn all_groups_folder_matches_only_groups() {
        let f = Folder {
            id: None,
            name: "Groups".to_string(),
            icon: FolderIcon::Group,
            include: FolderInclude { all_groups: true, ..Default::default() },
            exclude: FolderExclude::default(),
            order: 0.0,
        };
        assert!(f.matches(&room(RoomKind::Group, false, false, 0)));
        assert!(!f.matches(&room(RoomKind::Direct, false, false, 0)));
        assert!(!f.matches(&room(RoomKind::Channel, false, false, 0)));
    }

    #[test]
    fn exclude_muted_vetoes_an_otherwise_included_room() {
        let f = Folder {
            id: None,
            name: "Groups".to_string(),
            icon: FolderIcon::Group,
            include: FolderInclude { all_groups: true, ..Default::default() },
            exclude: FolderExclude { muted: true, ..Default::default() },
            order: 0.0,
        };
        assert!(!f.matches(&room(RoomKind::Group, true, false, 0)));
        assert!(f.matches(&room(RoomKind::Group, false, false, 0)));
    }

    #[test]
    fn exclude_read_keeps_only_unread_rooms() {
        let f = Folder {
            id: None,
            name: "Unread groups".to_string(),
            icon: FolderIcon::Group,
            include: FolderInclude { all_groups: true, ..Default::default() },
            exclude: FolderExclude { read: true, ..Default::default() },
            order: 0.0,
        };
        assert!(!f.matches(&room(RoomKind::Group, false, false, 0)));
        assert!(f.matches(&room(RoomKind::Group, false, false, 3)));
    }

    #[test]
    fn a_custom_folders_own_tag_matches_regardless_of_include_rules() {
        let f = Folder {
            id: Some(TagName::from("u.trading-desk")),
            name: "Trading desk".to_string(),
            icon: FolderIcon::Candles,
            include: FolderInclude::default(),
            exclude: FolderExclude::default(),
            order: 1.0,
        };
        let mut r = room(RoomKind::Direct, false, false, 0);
        assert!(!f.matches(&r));
        r.tags.push(Tag { name: TagName::from("u.trading-desk"), order: 1.0 });
        assert!(f.matches(&r));
    }

    #[test]
    fn exclude_archived_vetoes_even_a_directly_tagged_room() {
        let f = Folder {
            id: Some(TagName::from("u.trading-desk")),
            name: "Trading desk".to_string(),
            icon: FolderIcon::Candles,
            include: FolderInclude::default(),
            exclude: FolderExclude { archived: true, ..Default::default() },
            order: 1.0,
        };
        let mut r = room(RoomKind::Direct, false, true, 0);
        r.tags.push(Tag { name: TagName::from("u.trading-desk"), order: 1.0 });
        assert!(!f.matches(&r));
    }
}
