//! `Tags` / `TagsMut` — the only doctrinally-sanctioned access path to
//! `TagManager` from outside `mlc-backend`.
//!
//! Like `WatchlistManager`, `TagManager` never touches `DataBubble` directly
//! — it owns the tag registry + member routing table + synced-panel set as
//! its own state. The facade holds exactly one guard, no `BubbleStore`
//! co-acquisition.
//!
//! See `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A / §1B.

use std::sync::{RwLockReadGuard, RwLockWriteGuard};

use mlc_core::ids::{BubbleId, SyncMemberId};
use mlc_core::tag::{Tag, TagId};

use super::manager::{TagManager, TagManagerError};

/// Read-only facade over `TagManager`.
pub struct Tags<'a> {
    mgr: RwLockReadGuard<'a, TagManager>,
}

impl<'a> Tags<'a> {
    pub(crate) fn new(mgr: RwLockReadGuard<'a, TagManager>) -> Self {
        Self { mgr }
    }

    pub fn get(&self, id: TagId) -> Option<&Tag> {
        self.mgr.get(id)
    }

    /// `Tag.id == data_bubble_id` by invariant, so this is identity. Kept as
    /// a named accessor so the Phase 8 rename doesn't churn callers.
    pub fn tag_for_bubble(&self, bid: BubbleId) -> Option<&Tag> {
        self.mgr.get(bid)
    }

    pub fn tag_for_member(&self, member: SyncMemberId) -> Option<TagId> {
        self.mgr.tag_for_member(member)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&TagId, &Tag)> {
        self.mgr.iter()
    }

    pub fn len(&self) -> usize {
        self.mgr.len()
    }

    pub fn is_empty(&self) -> bool {
        self.mgr.is_empty()
    }

    pub fn contains(&self, id: TagId) -> bool {
        self.mgr.contains(id)
    }

    pub fn find_tag_by_color(&self, color: [f32; 4]) -> Option<TagId> {
        self.mgr.find_tag_by_color(color)
    }

    pub fn next_unused_color(&self) -> [f32; 4] {
        self.mgr.next_unused_color()
    }

    pub fn color_for_bubble(&self, bid: BubbleId) -> Option<[f32; 4]> {
        self.mgr.color_for_bubble(bid)
    }

    pub fn auto_created_for_bubble(&self, bid: BubbleId) -> bool {
        self.mgr.auto_created_for_bubble(bid)
    }

    pub fn synced_panels(&self) -> &std::collections::HashSet<SyncMemberId> {
        self.mgr.synced_panels()
    }

    pub fn is_synced(&self, member: SyncMemberId) -> bool {
        self.mgr.is_synced(member)
    }
}

/// Mutable facade over `TagManager`.
pub struct TagsMut<'a> {
    mgr: RwLockWriteGuard<'a, TagManager>,
}

impl<'a> TagsMut<'a> {
    pub(crate) fn new(mgr: RwLockWriteGuard<'a, TagManager>) -> Self {
        Self { mgr }
    }

    pub fn get(&self, id: TagId) -> Option<&Tag> {
        self.mgr.get(id)
    }

    pub fn get_mut(&mut self, id: TagId) -> Option<&mut Tag> {
        self.mgr.get_mut(id)
    }

    pub fn tag_for_bubble(&self, bid: BubbleId) -> Option<&Tag> {
        self.mgr.get(bid)
    }

    pub fn tag_for_member(&self, member: SyncMemberId) -> Option<TagId> {
        self.mgr.tag_for_member(member)
    }

    /// Insert a freshly-constructed tag (paired with a just-created bubble).
    pub fn insert(&mut self, tag: Tag) {
        self.mgr.insert(tag);
    }

    /// Remove a tag and unmap all its members.
    pub fn remove(&mut self, id: TagId) -> Option<Tag> {
        self.mgr.remove(id)
    }

    /// Connect a member to a tag. If already in another tag, the member is
    /// disconnected first.
    pub fn connect(&mut self, member: SyncMemberId, tag_id: TagId) -> Result<(), TagManagerError> {
        self.mgr.connect(member, tag_id)
    }

    /// Disconnect a member. Returns the tag id it was in, or `None`.
    pub fn disconnect(&mut self, member: SyncMemberId) -> Option<TagId> {
        self.mgr.disconnect(member)
    }

    pub fn find_tag_by_color(&self, color: [f32; 4]) -> Option<TagId> {
        self.mgr.find_tag_by_color(color)
    }

    pub fn next_unused_color(&self) -> [f32; 4] {
        self.mgr.next_unused_color()
    }

    pub fn color_for_bubble(&self, bid: BubbleId) -> Option<[f32; 4]> {
        self.mgr.color_for_bubble(bid)
    }

    pub fn auto_created_for_bubble(&self, bid: BubbleId) -> bool {
        self.mgr.auto_created_for_bubble(bid)
    }

    /// Promote an auto-created tag to a user-visible (color-tagged) one.
    /// No-op (returns `false`) if the tag does not exist or is already
    /// user-tagged.
    pub fn promote_to_tagged(&mut self, bid: BubbleId, color: [f32; 4]) -> bool {
        match self.mgr.get_mut(bid) {
            Some(tag) if tag.auto_created => {
                tag.auto_created = false;
                tag.color = color;
                true
            }
            _ => false,
        }
    }

    pub fn synced_panels(&self) -> &std::collections::HashSet<SyncMemberId> {
        self.mgr.synced_panels()
    }

    pub fn is_synced(&self, member: SyncMemberId) -> bool {
        self.mgr.is_synced(member)
    }

    pub fn mark_synced(&mut self, member: SyncMemberId) {
        self.mgr.mark_synced(member);
    }

    pub fn unmark_synced(&mut self, member: SyncMemberId) {
        self.mgr.unmark_synced(member);
    }
}
