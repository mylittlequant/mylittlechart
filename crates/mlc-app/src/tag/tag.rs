//! `Tag` — app-level orchestrator.  Owns nothing data-side; points at
//! one `DataBubble` and N `UiContainer`s by id.
//!
//! Phase 1 stub: only id + the two ref vectors.  Phase 2 will move
//! `color`, `auto_created`, `members`, `member_overrides` here off
//! `DataBubble`.
//!
//! All fields are shell-agnostic and serde-clean — no winit / web_sys
//! / wgpu / vello types may be added.

use mlc_core::tag_manager::BubbleId;
use serde::{Deserialize, Serialize};

use crate::ui_container::UiContainerId;

/// In Phase 8 this gets renamed via `BubbleId → TagId` sweep.  Until
/// then we re-export `BubbleId` under the canonical name so new code
/// can use `TagId` from day one and the Phase 8 rename becomes a
/// no-op for sites that already migrated.
pub type TagId = BubbleId;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tag {
    pub id: TagId,
    pub data_bubble_id: BubbleId,
    pub ui_container_ids: Vec<UiContainerId>,
}

impl Tag {
    /// Construct a `Tag` paired 1:1 with a `BubbleId` (Tag.id ==
    /// data_bubble_id by invariant).
    pub fn new(bubble_id: BubbleId) -> Self {
        Self {
            id: bubble_id,
            data_bubble_id: bubble_id,
            ui_container_ids: Vec::new(),
        }
    }
}
