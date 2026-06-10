use std::collections::{HashMap, HashSet};

use mlc_core::state::panel_grid::ChartPanelGrid;
use mlc_core::tag_manager::BubbleStore;
use mlc_core::LeafId;
use mlc_indicators::IndicatorManager;

/// All runtime state belonging to one open preset tab.
///
/// Swapped in/out of `ChartApp` fields via `mem::replace` on tab switch.
/// No serialization needed — just pointer swaps.
pub struct LivePresetState {
    pub panel_grid: ChartPanelGrid,
    pub tag_manager: BubbleStore,
    pub indicator_manager: IndicatorManager,
    pub alert_manager: mlc_alerts::AlertManager,
    pub leaf_color_tags: HashMap<LeafId, [f32; 4]>,
    pub indicator_overlay_states: HashMap<LeafId, mlc_layout::ui::modal_settings::IndicatorOverlayState>,
    pub series_handles: HashMap<(u64, mlc_bar_service::BarSeriesKey), mlc_bar_service::TrackedSeriesHandle>,
    pub pending_sub_pane_ratios: HashMap<u64, HashMap<u64, f32>>,
    pub pending_sub_pane_above_main: HashMap<u64, HashSet<u64>>,
    pub pending_sub_pane_order: HashMap<u64, Vec<u64>>,
    pub needs_initial_viewport_fit: bool,
    pub slot_dockings: [mlc_sidebar_content::SlotDockingManager; 4],
    pub panels_store: crate::panels_store::TradingPanelsStore,
    pub focused_free_leaf: Option<(usize, uzor::panels::LeafId)>,
}
