//! Tag UI/presentation state — split off `TagManager` per doctrine 1B
//! (`docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1B).
//!
//! `ui_container_ids` (composite docking windows showing a tag) and
//! `standalone_panel_ids` (flat outer-dock panels following a tag) are
//! presentation — which windows display a tag / focus is not something a
//! headless daemon needs to work correctly. They live here, sidecar-keyed
//! by the domain id (`TagId`, owned by `mlc_backend::tag::TagManager`).
//!
//! `active_chart_tag` (which tag the currently-active chart shows) is also
//! presentation and lives here as a plain runtime field — NOT persisted.
//!
//! Hard cutover: the old `Tag` struct (pre-3.6b) declared
//! `ui_container_ids` / `standalone_panel_ids` directly but neither field
//! was ever populated beyond `Vec::new()` nor read by any renderer/persist
//! path (audited 2026-07-02) — no seed-from-legacy migration needed.

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

/// Top-level tag UI/presentation state — per-tag sidecars plus which tag
/// the active chart currently shows.
#[derive(Clone, Debug, Default)]
pub struct TagUiRegistry {
    /// Per-tag presentation state, keyed by `TagId`.
    tags: HashMap<TagId, TagUiState>,

    /// The tag whose member is the currently-active chart. Updated when the
    /// active chart changes. Runtime-only, never persisted.
    pub active_chart_tag: Option<TagId>,
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
        if self.active_chart_tag == Some(tag_id) {
            self.active_chart_tag = None;
        }
    }
}
