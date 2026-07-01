//! `TagManager` — registry of `Tag`s keyed by `TagId`. Paired 1:1 with
//! `BubbleStore` (every bubble has a paired tag with the same id).
//!
//! Also owns the routing table from individual chart / panel members to the
//! tag (and therefore the bubble) they belong to, and the set of panel
//! members currently in Synced state. Bubble grouping is domain — a headless
//! daemon UI cannot sync panels without it — per doctrine 1B.
//!
//! Presentation state (`ui_container_ids`, `standalone_panel_ids`,
//! `active_chart_tag`) lives on the app-side `mlc-app::tag_ui::TagUiState`
//! sidecar, NOT here — see `docs/mlc/plans/doctrine-cutover-2026-06-27.md`
//! §1B.

use std::collections::{HashMap, HashSet};

use mlc_core::ids::{BubbleId, SyncMemberId};
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

    pub(crate) fn insert(&mut self, tag: Tag) {
        self.tags.insert(tag.id, tag);
    }

    /// Remove a tag and unmap all its members.
    pub(crate) fn remove(&mut self, id: TagId) -> Option<Tag> {
        let tag = self.tags.remove(&id)?;
        for member_id in &tag.members {
            self.member_to_tag.remove(member_id);
        }
        Some(tag)
    }

    pub(crate) fn get(&self, id: TagId) -> Option<&Tag> {
        self.tags.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: TagId) -> Option<&mut Tag> {
        self.tags.get_mut(&id)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&TagId, &Tag)> {
        self.tags.iter()
    }

    pub(crate) fn len(&self) -> usize {
        self.tags.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }

    pub(crate) fn contains(&self, id: TagId) -> bool {
        self.tags.contains_key(&id)
    }

    // =========================================================================
    // Membership
    // =========================================================================

    /// Connect a member to a tag. If already in another tag, the member is
    /// disconnected first.
    pub(crate) fn connect(
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
    pub(crate) fn disconnect(&mut self, member: SyncMemberId) -> Option<TagId> {
        let tag_id = self.member_to_tag.remove(&member)?;
        if let Some(tag) = self.tags.get_mut(&tag_id) {
            tag.members.remove(&member);
        }
        Some(tag_id)
    }

    pub(crate) fn tag_for_member(&self, member: SyncMemberId) -> Option<TagId> {
        self.member_to_tag.get(&member).copied()
    }

    // =========================================================================
    // Color / auto_created helpers
    // =========================================================================

    /// Find the first user-tagged (non-auto-created) tag with the given
    /// color (within a small epsilon).
    pub(crate) fn find_tag_by_color(&self, color: [f32; 4]) -> Option<TagId> {
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
    pub(crate) fn next_unused_color(&self) -> [f32; 4] {
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
    pub(crate) fn color_for_bubble(&self, bid: BubbleId) -> Option<[f32; 4]> {
        self.get(bid).map(|t| t.color)
    }

    /// Read `auto_created` for a given bubble id.
    pub(crate) fn auto_created_for_bubble(&self, bid: BubbleId) -> bool {
        self.get(bid).map(|t| t.auto_created).unwrap_or(false)
    }

    // =========================================================================
    // Synced-panel set
    // =========================================================================

    pub(crate) fn synced_panels(&self) -> &HashSet<SyncMemberId> {
        &self.synced_panels
    }

    pub(crate) fn is_synced(&self, member: SyncMemberId) -> bool {
        self.synced_panels.contains(&member)
    }

    pub(crate) fn mark_synced(&mut self, member: SyncMemberId) {
        self.synced_panels.insert(member);
    }

    pub(crate) fn unmark_synced(&mut self, member: SyncMemberId) {
        self.synced_panels.remove(&member);
    }
}
