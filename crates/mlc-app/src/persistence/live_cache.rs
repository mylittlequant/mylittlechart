//! In-memory cache of parked preset live-state.
//!
//! `LivePresetState` owns every field that `ChartApp` swaps in/out on a warm
//! preset tab switch. Storage is a plain `HashMap<id, LivePresetState>` on
//! `ChartApp::live_preset_cache`. Swap is `mem::replace` against
//! `ChartPanelGrid::placeholder()` + per-manager `::new()` defaults.
//!
//! Cold-path hydration (first-ever click of a preset id, or after the cache
//! entry was dropped) lives in [`super::hydration`]. It builds the same set
//! of fields from a `ChartPreset` and the result is swap-compatible with
//! anything `park_active_preset` produces here.

use std::collections::{HashMap, HashSet};

use crate::panel_grid::ChartPanelGrid;
use mlc_backend::bubble::BubbleStore;
use mlc_backend::tag::TagManager;
use mlc_core::LeafId;
use mlc_backend::indicator::IndicatorManager;

/// All runtime state belonging to one open preset tab.
///
/// Swapped in/out of `ChartApp` fields via `mem::replace` on tab switch.
/// No serialization needed — just pointer swaps.
pub struct LivePresetState {
    pub panel_grid: ChartPanelGrid,
    pub bubble_store: BubbleStore,
    /// Tag registry + member routing paired 1:1 with `bubble_store` (Step 6
    /// doctrine cutover). Must travel with `bubble_store` — both are keyed by
    /// the same `BubbleId`s; parking one without the other orphans routing
    /// for this preset's bubbles.
    pub tag_manager: TagManager,
    pub indicator_manager: IndicatorManager,
    pub alert_manager: mlc_backend::alert::AlertManager,
    /// Per-bubble drawing sessions (Step 3.1). Must travel with
    /// `bubble_store` — `DrawingManager::sessions` is keyed by the same
    /// `BubbleId`s that live in this preset's `BubbleStore`; parking one
    /// without the other orphans sessions in the (fresh, empty) active
    /// `DrawingManager`.
    pub drawing_manager: mlc_backend::drawing::DrawingManager,
    pub leaf_color_tags: HashMap<LeafId, [f32; 4]>,
    pub indicator_overlay_states: HashMap<LeafId, mlc_app_layout::ui::modal_settings::IndicatorOverlayState>,
    pub pending_sub_pane_ratios: HashMap<u64, HashMap<u64, f32>>,
    pub pending_sub_pane_above_main: HashMap<u64, HashSet<u64>>,
    pub pending_sub_pane_order: HashMap<u64, Vec<u64>>,
    pub needs_initial_viewport_fit: bool,
    pub slot_dockings: [mlc_app_sidebar_content::SlotDockingManager; mlc_app_sidebar_content::free_slot::SLOT_COUNT],
    pub panels_store: crate::panels_store::TradingPanelsStore,
    pub focused_free_leaf: Option<(usize, uzor::panels::LeafId)>,
    /// Wall-clock instant this preset was parked into the cache. The
    /// idle-tab release sweep (`ChartApp::release_idle_parked_presets`)
    /// measures elapsed time off this to answer "how long has nobody
    /// looked at this tab" — never touched again while the entry sits
    /// parked, so it stays exactly "park time" for the whole time this
    /// preset is out of view.
    pub parked_at: web_time::Instant,
    /// Set once the release sweep has already emptied this entry's kline
    /// rings. Lets the sweep skip an already-released preset on every
    /// later pass instead of re-walking (and re-touching) its bubble
    /// store every minute for as long as the tab stays parked.
    pub bars_released: bool,
}
