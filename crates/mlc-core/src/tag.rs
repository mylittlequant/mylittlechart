//! `Tag` — domain value type binding one `DataBubble` to its members and
//! its color identity. Pure domain data, no UI/presentation fields.
//!
//! Which windows display a tag (`ui_container_ids`, `standalone_panel_ids`)
//! and which tag the active chart currently shows (`active_chart_tag`) are
//! presentation — they live in `mlc-app::tag_ui` — see
//! `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1B.
//!
//! `color` stays on `Tag` in mlc-core: it's domain identity, participates in
//! routing (`TagManager::find_tag_by_color`) — a tag's color is its name,
//! per the §1B verdict table.
//!
//! `Tag.id == data_bubble_id` by 1:1 pairing invariant. This is why the
//! `TagId` type alias is just `BubbleId` — the same raw u64 identifies both.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::ids::{BubbleId, MemberSyncOverride, SyncMemberId};

/// `Tag` and its paired `DataBubble` share the same raw id. In Phase 8 the
/// rename sweep makes `TagId` a distinct newtype; until then this alias
/// keeps new code calling `TagId` without churning the storage layer.
pub type TagId = BubbleId;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tag {
    pub id: TagId,
    pub data_bubble_id: BubbleId,

    /// Display color shown on the tag chip in the UI. RGBA in 0..=1. Domain
    /// identity — a tag's color is its name (`TagManager::find_tag_by_color`
    /// routes by it).
    pub color: [f32; 4],

    /// Invisible default tag — auto-created so every window has a bubble to
    /// feed from. No color chip is shown in the UI; the first explicit
    /// user-tag flips this to `false`.
    pub auto_created: bool,

    /// All chart + panel members subscribed to this tag's bubble.
    pub members: HashSet<SyncMemberId>,

    /// Per-member sync-flag overrides. Absent = use the bubble's
    /// `sync_flags` default.
    pub member_overrides: HashMap<SyncMemberId, MemberSyncOverride>,
}

impl Tag {
    /// Construct a `Tag` paired 1:1 with a `BubbleId`.
    pub fn new(bubble_id: BubbleId, color: [f32; 4]) -> Self {
        Self {
            id: bubble_id,
            data_bubble_id: bubble_id,
            color,
            auto_created: false,
            members: HashSet::new(),
            member_overrides: HashMap::new(),
        }
    }

    /// Construct an auto-created (invisible) tag.
    pub fn new_auto(bubble_id: BubbleId, color: [f32; 4]) -> Self {
        Self {
            id: bubble_id,
            data_bubble_id: bubble_id,
            color,
            auto_created: true,
            members: HashSet::new(),
            member_overrides: HashMap::new(),
        }
    }

    /// Effective `sync_symbol` for a member: per-member override wins over
    /// the bubble's default. Caller passes the bubble's flag.
    pub fn effective_sync_symbol(&self, member: SyncMemberId, bubble_default: bool) -> bool {
        self.member_overrides
            .get(&member)
            .and_then(|o| o.sync_symbol)
            .unwrap_or(bubble_default)
    }

    /// Effective `sync_crosshair` for a member.
    pub fn effective_sync_crosshair(&self, member: SyncMemberId, bubble_default: bool) -> bool {
        self.member_overrides
            .get(&member)
            .and_then(|o| o.sync_crosshair)
            .unwrap_or(bubble_default)
    }
}

#[cfg(test)]
mod json_compat_tests {
    //! Gate test: literal fixture of the OLD (pre-3.6b) `Tag` JSON layout
    //! minus the two presentation fields (`ui_container_ids`,
    //! `standalone_panel_ids`) that moved to `mlc-app::tag_ui::TagUiState`.
    //! Domain fields must keep deserializing byte-for-byte off the old shape
    //! (with the moved fields simply ignored by serde — no `#[serde(deny_unknown_fields)]`
    //! anywhere on `Tag`).

    use super::*;

    #[test]
    fn deserializes_legacy_layout_ignoring_moved_ui_fields() {
        let json = r#"{
            "id": 7,
            "data_bubble_id": 7,
            "ui_container_ids": [1, 2, 3],
            "standalone_panel_ids": [9],
            "color": [0.2, 0.4, 0.6, 1.0],
            "auto_created": false,
            "members": [{"Chart": 1}],
            "member_overrides": {}
        }"#;
        let tag: Tag = serde_json::from_str(json).expect("legacy Tag JSON must still deserialize");
        assert_eq!(tag.id.0, 7);
        assert_eq!(tag.data_bubble_id.0, 7);
        assert_eq!(tag.color, [0.2, 0.4, 0.6, 1.0]);
        assert!(!tag.auto_created);
        assert!(tag.members.contains(&SyncMemberId::Chart(1)));
    }

    #[test]
    fn round_trips_current_layout() {
        let mut tag = Tag::new(BubbleId(3), [1.0, 0.0, 0.0, 1.0]);
        tag.members.insert(SyncMemberId::Panel(5));
        let json = serde_json::to_string(&tag).expect("serialize");
        let back: Tag = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, tag.id);
        assert_eq!(back.color, tag.color);
        assert!(back.members.contains(&SyncMemberId::Panel(5)));
    }
}
