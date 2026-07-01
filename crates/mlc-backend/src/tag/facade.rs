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
use mlc_core::state::chart_container::ChartId;
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

    pub fn synced_panels_snapshot(&self) -> Vec<SyncMemberId> {
        self.mgr.synced_panels_snapshot()
    }

    /// Return the tag that contains the given chart window, if any.
    pub fn tag_for_chart(&self, chart_id: ChartId) -> Option<TagId> {
        self.mgr.tag_for_chart(chart_id)
    }

    /// Return all chart members of a tag as `ChartId` values.
    pub fn chart_member_ids(&self, tag_id: TagId) -> Vec<ChartId> {
        self.mgr.chart_member_ids(tag_id)
    }

    /// Return all panel member IDs of a tag.
    pub fn panel_members(&self, tag_id: TagId) -> Vec<u64> {
        self.mgr.panel_members(tag_id)
    }

    /// Return the set of all member `SyncMemberId`s in a tag.
    pub fn members(&self, tag_id: TagId) -> Option<&std::collections::HashSet<SyncMemberId>> {
        self.mgr.members(tag_id)
    }

    /// Return all peer chart windows of the given chart (every chart member
    /// of its tag except itself).
    pub fn peers(&self, chart_id: ChartId) -> Vec<ChartId> {
        self.mgr.peers(chart_id)
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

    pub fn synced_panels_snapshot(&self) -> Vec<SyncMemberId> {
        self.mgr.synced_panels_snapshot()
    }

    /// Attach a chart window to a tag. Wraps `connect(SyncMemberId::Chart(...))`.
    pub fn attach_chart_to_tag(&mut self, chart_id: ChartId, tag_id: TagId) -> Result<(), TagManagerError> {
        self.mgr.attach_chart_to_tag(chart_id, tag_id)
    }

    /// Detach a chart window from its current tag.
    pub fn detach_chart_from_tag(&mut self, chart_id: ChartId) -> Option<TagId> {
        self.mgr.detach_chart_from_tag(chart_id)
    }

    /// Return the tag that contains the given chart window, if any.
    pub fn tag_for_chart(&self, chart_id: ChartId) -> Option<TagId> {
        self.mgr.tag_for_chart(chart_id)
    }

    /// Return all chart members of a tag as `ChartId` values.
    pub fn chart_member_ids(&self, tag_id: TagId) -> Vec<ChartId> {
        self.mgr.chart_member_ids(tag_id)
    }

    /// Return all panel member IDs of a tag.
    pub fn panel_members(&self, tag_id: TagId) -> Vec<u64> {
        self.mgr.panel_members(tag_id)
    }

    /// Return the set of all member `SyncMemberId`s in a tag.
    pub fn members(&self, tag_id: TagId) -> Option<&std::collections::HashSet<SyncMemberId>> {
        self.mgr.members(tag_id)
    }

    /// Return all peer chart windows of the given chart (every chart member
    /// of its tag except itself).
    pub fn peers(&self, chart_id: ChartId) -> Vec<ChartId> {
        self.mgr.peers(chart_id)
    }

    /// Put a panel in Synced state: register in `synced_panels` and connect
    /// to the given tag.
    pub fn set_synced(&mut self, panel: SyncMemberId, tag_id: TagId) {
        self.mgr.set_synced(panel, tag_id);
    }

    /// Put a panel in auto-created tag state: remove from `synced_panels`
    /// and connect to its own private auto-created tag.
    pub fn set_auto_group(&mut self, panel: SyncMemberId, private_tag_id: TagId) {
        self.mgr.set_auto_group(panel, private_tag_id);
    }

    /// Move all currently-Synced panels to a new tag.
    pub fn reassign_synced_panels(&mut self, new_tag_id: TagId) {
        self.mgr.reassign_synced_panels(new_tag_id);
    }

    /// Set the display color of a tag.
    pub fn set_color(&mut self, tag_id: TagId, color: [f32; 4]) {
        self.mgr.set_color(tag_id, color);
    }
}
