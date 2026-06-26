//! `TagStore` — registry of `Tag`s keyed by `TagId`.  Paired 1:1 with
//! `BubbleStore` (every bubble has a paired tag with the same id).
//!
//! Phase 1 stub: empty store + minimal CRUD.  Phase 2 wires
//! `BubbleStore::create_group` / `create_group_auto` / `remove_group`
//! to mirror into here.

use std::collections::HashMap;

use mlc_core::tag_manager::BubbleId;

use super::tag::{Tag, TagId};

#[derive(Clone, Debug, Default)]
pub struct TagStore {
    tags: HashMap<TagId, Tag>,
}

impl TagStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, tag: Tag) {
        self.tags.insert(tag.id, tag);
    }

    pub fn remove(&mut self, id: TagId) -> Option<Tag> {
        self.tags.remove(&id)
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

    /// Look up the tag paired with a given bubble.  Today Tag.id ==
    /// data_bubble_id so this is identity; kept as a named accessor so
    /// Phase 8 rename doesn't churn callers.
    pub fn tag_for_bubble(&self, bid: BubbleId) -> Option<&Tag> {
        self.get(bid)
    }
}
