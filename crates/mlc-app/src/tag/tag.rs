//! `Tag` — app-level aggregator over **1 DataBubble** + **N UI consumers**.
//!
//! Bubble is unaware of how many consumers it has or who they are.
//! Tag owns that knowledge — the routing table from chart/panel ids to
//! the bubble they feed, the user-facing color identity, and the
//! per-member sync-flag overrides.
//!
//! Fields here are shell-agnostic and serde-clean.  No winit / web_sys
//! / wgpu / vello types may be added.
//!
//! `Tag.id == data_bubble_id` by 1:1 pairing invariant.  This is why
//! the `TagId` type alias is just `BubbleId` — the same raw u64
//! identifies both.

use std::collections::{HashMap, HashSet};

use mlc_core::ids::{BubbleId, MemberSyncOverride, SyncMemberId};
use serde::{Deserialize, Serialize};

use crate::ui_container::UiContainerId;

/// `Tag` and its paired `DataBubble` share the same raw id.  In Phase 8
/// the rename sweep makes `TagId` a distinct newtype; until then this
/// alias keeps new code calling `TagId` without churning the storage
/// layer.
pub type TagId = BubbleId;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tag {
    pub id: TagId,
    pub data_bubble_id: BubbleId,
    pub ui_container_ids: Vec<UiContainerId>,

    /// Display color shown on the tag chip in the UI.  RGBA in 0..=1.
    pub color: [f32; 4],

    /// Invisible default tag — auto-created so every window has a
    /// bubble to feed from.  No color chip is shown in the UI; the
    /// first explicit user-tag flips this to `false`.
    pub auto_created: bool,

    /// All chart + panel members subscribed to this tag's bubble.
    pub members: HashSet<SyncMemberId>,

    /// Per-member sync-flag overrides.  Absent = use the bubble's
    /// `sync_flags` default.
    pub member_overrides: HashMap<SyncMemberId, MemberSyncOverride>,
}

impl Tag {
    /// Construct a `Tag` paired 1:1 with a `BubbleId`.  Phase 2B-1
    /// scaffold only — the lifecycle wiring that mirrors
    /// `BubbleStore::create_bubble` into a paired `Tag` lands in
    /// Phase 2B-3.
    pub fn new(bubble_id: BubbleId, color: [f32; 4]) -> Self {
        Self {
            id: bubble_id,
            data_bubble_id: bubble_id,
            ui_container_ids: Vec::new(),
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
            ui_container_ids: Vec::new(),
            color,
            auto_created: true,
            members: HashSet::new(),
            member_overrides: HashMap::new(),
        }
    }

    /// Effective `sync_symbol` for a member: per-member override wins
    /// over the bubble's default.  Caller passes the bubble's flag.
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
