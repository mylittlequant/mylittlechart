//! Tag UI/presentation state — split off `TagManager` per doctrine 1B
//! (`docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1B).
//!
//! `ui_container_ids` (composite docking windows showing a tag) and
//! `standalone_panel_ids` (flat outer-dock panels following a tag) are
//! presentation — which windows display a tag / focus is not something a
//! headless daemon needs to work correctly. They live here, sidecar-keyed
//! by the domain id (`TagId`, owned by `mlc_backend::tag::TagManager`).
//!
//! Hard cutover: the old `Tag` struct (pre-3.6b) declared
//! `ui_container_ids` / `standalone_panel_ids` directly but neither field
//! was ever populated beyond `Vec::new()` nor read by any renderer/persist
//! path (audited 2026-07-02) — no seed-from-legacy migration needed.
//!
//! `active_chart_tag` does NOT live here (corrected 2026-07-02, Step 6):
//! it turned out to be live, actively-written state
//! (`BubbleStore::active_chart_bubble`, set inside `reassign_synced_panels`)
//! that the original 3.6b audit missed because it lived on `BubbleStore`,
//! not the old `TagStore`. It drives which tag Synced panels follow — a
//! headless daemon UI needs this to route correctly — so it is domain
//! routing, not presentation. It now lives on
//! `mlc_backend::tag::TagManager::active_chart_tag`.

use std::collections::HashMap;

use mlc_core::tag::TagId;

use crate::ui_container::UiContainerId;
use mlc_core::state::PanelId;

mod view;
pub use view::build_group_view;

/// Presentation state for one `Tag`.
#[derive(Clone, Debug, Default)]
pub struct TagUiState {
    /// Composite docking windows (`UiContainer`) currently showing this
    /// tag's bubble.
    pub ui_container_ids: Vec<UiContainerId>,
    /// Flat outer-dock panels (no inner_docking) currently following this
    /// tag's bubble.
    pub standalone_panel_ids: Vec<PanelId>,
}

/// Top-level tag UI/presentation state — per-tag sidecars.
#[derive(Clone, Debug, Default)]
pub struct TagUiRegistry {
    /// Per-tag presentation state, keyed by `TagId`.
    tags: HashMap<TagId, TagUiState>,
}

impl TagUiRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Presentation state for a given tag, materializing a default entry on
    /// first access.
    pub fn tag_mut(&mut self, tag_id: TagId) -> &mut TagUiState {
        self.tags.entry(tag_id).or_default()
    }

    /// Read-only presentation state for a given tag, `None` if never
    /// touched.
    pub fn tag(&self, tag_id: TagId) -> Option<&TagUiState> {
        self.tags.get(&tag_id)
    }

    /// Drop the sidecar for a deleted tag (call alongside
    /// `TagsMut::remove`).
    pub fn remove_tag(&mut self, tag_id: TagId) {
        self.tags.remove(&tag_id);
    }
}
