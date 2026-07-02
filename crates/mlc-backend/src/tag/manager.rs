//! `TagManager` — registry of `Tag`s keyed by `TagId`. Paired 1:1 with
//! `BubbleStore` (every bubble has a paired tag with the same id).
//!
//! Also owns the routing table from individual chart / panel members to the
//! tag (and therefore the bubble) they belong to, the set of panel members
//! currently in Synced state, and which tag the active chart shows
//! (`active_chart_tag`). Bubble grouping is domain — a headless daemon UI
//! cannot sync panels or route Synced-panel-follow without it — per
//! doctrine 1B (`docs/mlc/plans/doctrine-cutover-2026-06-27.md`).
//!
//! Presentation state (`ui_container_ids`, `standalone_panel_ids`) lives on
//! the app-side `mlc-app::tag_ui::TagUiState` sidecar, NOT here.
//!
//! Methods are `pub` (not `pub(crate)`) mirroring `WatchlistManager`: the
//! `Tags`/`TagsMut` facade governs access while the manager sits behind
//! `BackendCore`'s `RwLock`; once a caller owns an extracted subset directly
//! (preset park/unpark via `extract_tags_for_bubbles`/`merge_tags` in
//! `mlc-app::persistence` — shared-BackendCore arc §5.3), its `pub` API is
//! the normal way to query it — no facade needed for an unlocked,
//! directly-owned instance.

use std::collections::{HashMap, HashSet};

use mlc_core::ids::{BubbleId, ChartId, SyncMemberId};
use mlc_core::tag::{Tag, TagId};

#[derive(Debug, Default)]
pub struct TagManager {
    tags: HashMap<TagId, Tag>,

    /// Reverse index: member → tag. A member can be in at most one tag at a
    /// time.
    member_to_tag: HashMap<SyncMemberId, TagId>,

    /// Panel members currently in Synced state. Their tag membership is
    /// dynamic — reassigned when the active chart changes.
    synced_panels: HashSet<SyncMemberId>,

    /// The tag whose member is the currently-active chart. Updated whenever
    /// the active chart changes (inside `reassign_synced_panels`). Domain
    /// routing state — promoted from `BubbleStore::active_chart_bubble`
    /// (Step 6). Drives which tag Synced panels follow; a headless daemon UI
    /// needs this to route panel-follow correctly, so it lives here rather
    /// than on the app-side presentation sidecar.
    active_chart_tag: Option<TagId>,
}

#[derive(Debug)]
pub enum TagManagerError {
    TagNotFound(TagId),
}

impl std::fmt::Display for TagManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TagNotFound(id) => write!(f, "tag {:?} does not exist", id),
        }
    }
}

impl std::error::Error for TagManagerError {}

impl TagManager {
    pub fn new() -> Self {
        Self::default()
    }

    // =========================================================================
    // CRUD
    // =========================================================================

    pub fn insert(&mut self, tag: Tag) {
        self.tags.insert(tag.id, tag);
    }

    /// Remove a tag and unmap all its members.
    pub fn remove(&mut self, id: TagId) -> Option<Tag> {
        let tag = self.tags.remove(&id)?;
        for member_id in &tag.members {
            self.member_to_tag.remove(member_id);
        }
        Some(tag)
    }

    pub fn get(&self, id: TagId) -> Option<&Tag> {
        self.tags.get(&id)
    }

    pub fn get_mut(&mut self, id: TagId) -> Option<&mut Tag> {
        self.tags.get_mut(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&TagId, &Tag)> {
        self.tags.iter()
    }

    pub fn len(&self) -> usize {
        self.tags.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }

    pub fn contains(&self, id: TagId) -> bool {
        self.tags.contains_key(&id)
    }

    // =========================================================================
    // Membership
    // =========================================================================

    /// Connect a member to a tag. If already in another tag, the member is
    /// disconnected first.
    pub fn connect(
        &mut self,
        member: SyncMemberId,
        tag_id: TagId,
    ) -> Result<(), TagManagerError> {
        self.disconnect(member);
        let tag = self
            .tags
            .get_mut(&tag_id)
            .ok_or(TagManagerError::TagNotFound(tag_id))?;
        tag.members.insert(member);
        self.member_to_tag.insert(member, tag_id);
        Ok(())
    }

    /// Disconnect a member. Returns the tag id it was in, or `None`.
    pub fn disconnect(&mut self, member: SyncMemberId) -> Option<TagId> {
        let tag_id = self.member_to_tag.remove(&member)?;
        if let Some(tag) = self.tags.get_mut(&tag_id) {
            tag.members.remove(&member);
        }
        Some(tag_id)
    }

    pub fn tag_for_member(&self, member: SyncMemberId) -> Option<TagId> {
        self.member_to_tag.get(&member).copied()
    }

    // =========================================================================
    // Membership — chart forwarding helpers (mirrors `BubbleStore`'s chart
    // convenience wrappers; Step 6 routing parity, Audit-F items 2-8)
    // =========================================================================

    /// Attach a chart window to a tag. Wraps `connect(SyncMemberId::Chart(...))`.
    pub fn attach_chart_to_tag(
        &mut self,
        chart_id: ChartId,
        tag_id: TagId,
    ) -> Result<(), TagManagerError> {
        self.connect(SyncMemberId::Chart(chart_id.0), tag_id)
    }

    /// Detach a chart window from its current tag.
    pub fn detach_chart_from_tag(&mut self, chart_id: ChartId) -> Option<TagId> {
        self.disconnect(SyncMemberId::Chart(chart_id.0))
    }

    /// Return the tag that contains the given chart window, if any.
    pub fn tag_for_chart(&self, chart_id: ChartId) -> Option<TagId> {
        self.tag_for_member(SyncMemberId::Chart(chart_id.0))
    }

    /// Return all chart members of a tag as `ChartId` values.
    pub fn chart_member_ids(&self, tag_id: TagId) -> Vec<ChartId> {
        self.tags
            .get(&tag_id)
            .map(|t| t.members.iter().filter_map(|m| m.as_chart().map(ChartId)).collect())
            .unwrap_or_default()
    }

    /// Return all panel member IDs of a tag.
    pub fn panel_members(&self, tag_id: TagId) -> Vec<u64> {
        self.tags
            .get(&tag_id)
            .map(|t| t.members.iter().filter_map(|m| m.as_panel()).collect())
            .unwrap_or_default()
    }

    /// Return the set of all member `SyncMemberId`s in a tag, or `None` if
    /// the tag does not exist.
    pub fn members(&self, tag_id: TagId) -> Option<&HashSet<SyncMemberId>> {
        self.tags.get(&tag_id).map(|t| &t.members)
    }

    /// Return all peer chart windows of the given chart (every chart member
    /// of its tag except itself).
    pub fn peers(&self, chart_id: ChartId) -> Vec<ChartId> {
        let member = SyncMemberId::Chart(chart_id.0);
        let Some(&tag_id) = self.member_to_tag.get(&member) else { return Vec::new() };
        let Some(tag) = self.tags.get(&tag_id) else { return Vec::new() };
        tag.members
            .iter()
            .filter_map(|m| m.as_chart().map(ChartId))
            .filter(|&id| id != chart_id)
            .collect()
    }

    // =========================================================================
    // Color / auto_created helpers
    // =========================================================================

    /// Find the first user-tagged (non-auto-created) tag with the given
    /// color (within a small epsilon).
    pub fn find_tag_by_color(&self, color: [f32; 4]) -> Option<TagId> {
        const EPS: f32 = 1.0e-2;
        self.tags
            .iter()
            .find(|(_, t)| {
                !t.auto_created
                    && (t.color[0] - color[0]).abs() < EPS
                    && (t.color[1] - color[1]).abs() < EPS
                    && (t.color[2] - color[2]).abs() < EPS
            })
            .map(|(id, _)| *id)
    }

    /// Pick the next preset palette color that is not already used by a
    /// user-tagged (non-auto-created) tag. Falls back to the first preset
    /// color if all are taken.
    pub fn next_unused_color(&self) -> [f32; 4] {
        use mlc_core::state::toolbar_geometry::PRESET_COLORS;
        const EPS: f32 = 1.0e-2;
        let used: Vec<[f32; 4]> = self
            .tags
            .values()
            .filter(|t| !t.auto_created)
            .map(|t| t.color)
            .collect();
        for &preset in PRESET_COLORS.iter() {
            let is_used = used.iter().any(|uc| {
                (uc[0] - preset[0]).abs() < EPS
                    && (uc[1] - preset[1]).abs() < EPS
                    && (uc[2] - preset[2]).abs() < EPS
            });
            if !is_used {
                return preset;
            }
        }
        PRESET_COLORS[0]
    }

    /// Read color out of the tag for a given bubble id.
    pub fn color_for_bubble(&self, bid: BubbleId) -> Option<[f32; 4]> {
        self.get(bid).map(|t| t.color)
    }

    /// Read `auto_created` for a given bubble id.
    pub fn auto_created_for_bubble(&self, bid: BubbleId) -> bool {
        self.get(bid).map(|t| t.auto_created).unwrap_or(false)
    }

    /// Set the display color of a tag.
    pub fn set_color(&mut self, tag_id: TagId, color: [f32; 4]) {
        if let Some(tag) = self.tags.get_mut(&tag_id) {
            tag.color = color;
        }
    }

    // =========================================================================
    // Synced-panel set
    // =========================================================================

    pub fn synced_panels(&self) -> &HashSet<SyncMemberId> {
        &self.synced_panels
    }

    pub fn is_synced(&self, member: SyncMemberId) -> bool {
        self.synced_panels.contains(&member)
    }

    pub fn mark_synced(&mut self, member: SyncMemberId) {
        self.synced_panels.insert(member);
    }

    pub fn unmark_synced(&mut self, member: SyncMemberId) {
        self.synced_panels.remove(&member);
    }

    /// Snapshot the synced_panels set (for persistence).
    pub fn synced_panels_snapshot(&self) -> Vec<SyncMemberId> {
        self.synced_panels.iter().copied().collect()
    }

    /// Put a panel in Synced state: register in `synced_panels` and connect
    /// to the given tag.
    pub fn set_synced(&mut self, panel: SyncMemberId, tag_id: TagId) {
        self.synced_panels.insert(panel);
        let _ = self.connect(panel, tag_id);
    }

    /// Put a panel in auto-created tag state: remove from `synced_panels`
    /// and connect to its own private auto-created tag.
    pub fn set_auto_group(&mut self, panel: SyncMemberId, private_tag_id: TagId) {
        self.synced_panels.remove(&panel);
        let _ = self.connect(panel, private_tag_id);
    }

    /// Move all currently-Synced panels to a new tag (called when the active
    /// chart changes so Synced panels follow it). Also records `new_tag_id`
    /// as the active chart's tag.
    pub fn reassign_synced_panels(&mut self, new_tag_id: TagId) {
        let panels: Vec<SyncMemberId> = self.synced_panels.iter().copied().collect();
        for member in panels {
            self.disconnect(member);
            let _ = self.connect(member, new_tag_id);
        }
        self.active_chart_tag = Some(new_tag_id);
    }

    /// The tag whose member is the currently-active chart.
    pub fn active_chart_tag(&self) -> Option<TagId> {
        self.active_chart_tag
    }

    // =========================================================================
    // Preset restore helpers
    // =========================================================================

    /// Remove all tags and reset routing state. Used during preset restore
    /// to start from a clean slate before re-inserting tags reconstructed
    /// from snapshots — call alongside `BubbleStore::clear()`.
    pub fn clear(&mut self) {
        self.tags.clear();
        self.member_to_tag.clear();
        self.synced_panels.clear();
        self.active_chart_tag = None;
    }

    /// Rebuild the tag registry from the same persisted snapshot list
    /// `BubbleStore::restore_from_snapshots` consumes.
    ///
    /// For each snapshot: reconstructs a `Tag` via `Tag::from_snapshot`
    /// (color, auto_created, members, member_overrides), inserts it,
    /// rebuilds `member_to_tag`, and marks any `synced_panel_ids` from the
    /// snapshot as Synced (mirrors the old `synced_panel_restorer` callback
    /// — `BubbleStore` no longer owns that concern, Step 6 doctrine
    /// cutover). Does NOT call [`Self::clear`] — callers that want a full
    /// wipe-and-rebuild call `clear()` first.
    ///
    /// Returns the `(SyncMemberId, TagId)` pairs that were marked Synced, so
    /// the caller can drive any additional app-level Synced-panel wiring
    /// (mirrors the pre-Step-6 `synced_panel_restorer` callback shape).
    pub fn restore_from_snapshots(
        &mut self,
        snaps: &[mlc_core::preset::snapshots::DataBubbleSnapshot],
    ) -> Vec<(SyncMemberId, TagId)> {
        let mut panel_syncs = Vec::new();
        for snap in snaps {
            let tag = Tag::from_snapshot(snap);
            let tag_id = tag.id;
            for &member_id in &tag.members {
                self.member_to_tag.insert(member_id, tag_id);
            }
            self.tags.insert(tag_id, tag);
            for &pid in &snap.synced_panel_ids {
                let member = SyncMemberId::Panel(pid);
                self.synced_panels.insert(member);
                panel_syncs.push((member, tag_id));
            }
        }
        panel_syncs
    }

    // =========================================================================
    // Scoped park/unpark — bubble-id-subset extraction (shared-BackendCore
    // arc, §5.3). Replaces whole-manager `swap()`: under a single
    // process-wide `BackendCore`, `synced_panels` and `active_chart_tag`
    // are domain-wide state (§4.2 verdict) and must NEVER travel in a
    // park/unpark payload — only the tags themselves (keyed 1:1 by
    // `BubbleId`, per the `TagId == BubbleId` invariant) and their
    // `member_to_tag` routing rows move.
    // =========================================================================

    /// Remove and return only the tags whose id is in `bubble_ids`
    /// (`TagId == BubbleId` by invariant), plus their `member_to_tag`
    /// routing rows. `synced_panels` and `active_chart_tag` are left
    /// untouched on `self` — they are global, not part of any one preset's
    /// state (§4.2).
    ///
    /// Used exclusively by preset tab park (`mlc-app/src/persistence/
    /// profile.rs::park_active_preset`).
    pub fn extract_tags_for_bubbles(&mut self, bubble_ids: &[BubbleId]) -> TagManager {
        let mut out = TagManager::default();
        for &id in bubble_ids {
            let Some(tag) = self.tags.remove(&id) else { continue };
            for &member_id in &tag.members {
                if let Some(&owner) = self.member_to_tag.get(&member_id) {
                    if owner == id {
                        self.member_to_tag.remove(&member_id);
                        out.member_to_tag.insert(member_id, id);
                    }
                }
            }
            out.tags.insert(id, tag);
        }
        out
    }

    /// Re-insert every tag and routing row from `other` into `self`.
    /// `other.synced_panels`/`active_chart_tag` are discarded (always
    /// default/empty on an extracted `TagManager` — see
    /// [`Self::extract_tags_for_bubbles`]).
    ///
    /// Used exclusively by preset tab unpark (`mlc-app/src/persistence/
    /// profile.rs::unpark_preset`).
    pub fn merge_tags(&mut self, other: TagManager) {
        for (id, tag) in other.tags {
            self.tags.insert(id, tag);
        }
        for (member_id, tag_id) in other.member_to_tag {
            self.member_to_tag.insert(member_id, tag_id);
        }
    }
}

#[cfg(test)]
mod park_unpark_tests {
    //! Round-trip coverage for `extract_tags_for_bubbles`/`merge_tags` — the
    //! shared-BackendCore-arc scoped-park replacement for whole-manager
    //! `swap()` (`docs/mlc/plans/shared-backend-core-2026-07-03.md` §5.3).

    use super::*;

    #[test]
    fn extract_then_merge_round_trips_to_identical_manager() {
        let mut mgr = TagManager::new();
        let a = BubbleId(1);
        let b = BubbleId(2);
        let c = BubbleId(3);
        mgr.insert(Tag::new(a, [1.0, 0.0, 0.0, 1.0]));
        mgr.insert(Tag::new(b, [0.0, 1.0, 0.0, 1.0]));
        mgr.insert(Tag::new(c, [0.0, 0.0, 1.0, 1.0]));
        mgr.connect(SyncMemberId::Chart(10), a).expect("connect a");
        mgr.connect(SyncMemberId::Chart(20), b).expect("connect b");
        mgr.connect(SyncMemberId::Chart(30), c).expect("connect c");

        let extracted = mgr.extract_tags_for_bubbles(&[a, b]);
        assert!(!mgr.contains(a));
        assert!(!mgr.contains(b));
        assert!(mgr.contains(c), "non-extracted tag must remain in the source manager");
        assert!(extracted.contains(a));
        assert!(extracted.contains(b));
        assert_eq!(extracted.tag_for_member(SyncMemberId::Chart(10)), Some(a));
        assert_eq!(mgr.tag_for_member(SyncMemberId::Chart(30)), Some(c));
        assert_eq!(mgr.tag_for_member(SyncMemberId::Chart(10)), None, "routing row must move with the tag");

        mgr.merge_tags(extracted);
        assert!(mgr.contains(a));
        assert!(mgr.contains(b));
        assert!(mgr.contains(c));
        assert_eq!(mgr.tag_for_member(SyncMemberId::Chart(10)), Some(a));
        assert_eq!(mgr.tag_for_member(SyncMemberId::Chart(20)), Some(b));
    }

    #[test]
    fn extract_leaves_synced_panels_and_active_chart_tag_global() {
        // §4.2 verdict: synced_panels/active_chart_tag are domain-wide and
        // must NEVER travel in a park/unpark payload.
        let mut mgr = TagManager::new();
        let a = BubbleId(1);
        mgr.insert(Tag::new(a, [1.0, 0.0, 0.0, 1.0]));
        mgr.set_synced(SyncMemberId::Panel(99), a);
        mgr.reassign_synced_panels(a);
        assert_eq!(mgr.active_chart_tag(), Some(a));
        assert!(mgr.is_synced(SyncMemberId::Panel(99)));

        let extracted = mgr.extract_tags_for_bubbles(&[a]);

        // Global state stays on `mgr`, is NOT reset or moved into `extracted`.
        assert_eq!(mgr.active_chart_tag(), Some(a));
        assert!(mgr.is_synced(SyncMemberId::Panel(99)));
        assert!(extracted.synced_panels().is_empty());
        assert_eq!(extracted.active_chart_tag(), None);
    }

    #[test]
    fn extract_other_windows_tags_are_untouched() {
        let mut mgr = TagManager::new();
        let window_a_bid = BubbleId(1);
        let window_b_bid = BubbleId(2);
        mgr.insert(Tag::new(window_a_bid, [1.0, 0.0, 0.0, 1.0]));
        mgr.insert(Tag::new(window_b_bid, [0.0, 1.0, 0.0, 1.0]));

        let parked_b = mgr.extract_tags_for_bubbles(&[window_b_bid]);

        assert!(mgr.contains(window_a_bid), "window A's tag must survive parking window B's preset");
        assert!(!parked_b.contains(window_a_bid), "extraction must not leak window A's tag into window B's payload");
    }
}
