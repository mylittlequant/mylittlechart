//! `TagStore` — registry of `Tag`s keyed by `TagId`.  Paired 1:1 with
//! `BubbleStore` (every bubble has a paired tag with the same id).
//!
//! Also owns the routing table from individual chart / panel members
//! to the tag (and therefore the bubble) they belong to.  This used
//! to live as `BubbleStore::member_to_bubble`; routing is a tag
//! concern, not a bubble concern, so it moves here in Phase 2B.

use std::collections::{HashMap, HashSet};

use mlc_core::ids::{BubbleId, SyncMemberId};

use super::tag::{Tag, TagId};

#[derive(Debug, Default)]
pub struct TagStore {
    tags: HashMap<TagId, Tag>,

    /// Reverse index: member → tag.  A member can be in at most one
    /// tag at a time.
    member_to_tag: HashMap<SyncMemberId, TagId>,

    /// The tag whose member is the currently-active chart.  Updated
    /// when the active chart changes.  (Was `BubbleStore::active_chart_bubble`.)
    pub active_chart_tag: Option<TagId>,

    /// Panel members currently in Synced state.  Their tag membership
    /// is dynamic — reassigned when the active chart changes.
    /// (Was `BubbleStore::synced_panels`.)
    synced_panels: HashSet<SyncMemberId>,
}

#[derive(Debug)]
pub enum TagStoreError {
    MemberNotInTag(SyncMemberId),
    TagNotFound(TagId),
}

impl std::fmt::Display for TagStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MemberNotInTag(id) => write!(f, "member {:?} is not in any tag", id),
            Self::TagNotFound(id) => write!(f, "tag {:?} does not exist", id),
        }
    }
}

impl std::error::Error for TagStoreError {}

impl TagStore {
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
        if let Some(tag) = self.tags.remove(&id) {
            for member_id in &tag.members {
                self.member_to_tag.remove(member_id);
            }
            Some(tag)
        } else {
            None
        }
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

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&TagId, &mut Tag)> {
        self.tags.iter_mut()
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

    /// Tag.id == data_bubble_id by invariant, so this is identity.
    /// Kept as a named accessor so the Phase 8 rename doesn't churn
    /// callers.
    pub fn tag_for_bubble(&self, bid: BubbleId) -> Option<&Tag> {
        self.get(bid)
    }

    pub fn tag_for_bubble_mut(&mut self, bid: BubbleId) -> Option<&mut Tag> {
        self.get_mut(bid)
    }

    // =========================================================================
    // Membership
    // =========================================================================

    /// Connect a member to a tag.  If already in another tag, the
    /// member is disconnected first.
    pub fn connect(
        &mut self,
        member: SyncMemberId,
        tag_id: TagId,
    ) -> Result<(), TagStoreError> {
        self.disconnect(member);
        let tag = self
            .tags
            .get_mut(&tag_id)
            .ok_or(TagStoreError::TagNotFound(tag_id))?;
        tag.members.insert(member);
        self.member_to_tag.insert(member, tag_id);
        Ok(())
    }

    /// Disconnect a member.  Returns the tag id it was in, or `None`.
    pub fn disconnect(&mut self, member: SyncMemberId) -> Option<TagId> {
        if let Some(tag_id) = self.member_to_tag.remove(&member) {
            if let Some(tag) = self.tags.get_mut(&tag_id) {
                tag.members.remove(&member);
            }
            Some(tag_id)
        } else {
            None
        }
    }

    pub fn tag_for_member(&self, member: SyncMemberId) -> Option<TagId> {
        self.member_to_tag.get(&member).copied()
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

    /// Pick the next preset palette color that is not already used by
    /// a user-tagged (non-auto-created) tag.  Falls back to the first
    /// preset color if all are taken.
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

    // =========================================================================
    // Synced-panel set
    // =========================================================================

    pub fn synced_panels(&self) -> &HashSet<SyncMemberId> {
        &self.synced_panels
    }

    pub fn synced_panels_mut(&mut self) -> &mut HashSet<SyncMemberId> {
        &mut self.synced_panels
    }
}
