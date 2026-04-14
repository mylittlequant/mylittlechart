use std::collections::{HashMap, HashSet};

use zengeld_chart::state::panel_grid::ChartPanelGrid;
use zengeld_chart::tag_manager::TagManager;
use zengeld_chart::LeafId;
use zengeld_terminal_indicators::IndicatorManager;

/// All runtime state belonging to one open preset tab.
///
/// Swapped in/out of `ChartApp` fields via `mem::replace` on tab switch.
/// No serialization needed — just pointer swaps.
pub struct LivePresetState {
    pub panel_grid: ChartPanelGrid,
    pub tag_manager: TagManager,
    pub indicator_manager: IndicatorManager,
    pub alert_manager: alerts::AlertManager,
    pub leaf_color_tags: HashMap<LeafId, [f32; 4]>,
    pub indicator_overlay_states: HashMap<LeafId, zengeld_chart::ui::modal_settings::IndicatorOverlayState>,
    pub series_handles: HashMap<(u64, bar_service::BarSeriesKey), bar_service::TrackedSeriesHandle>,
    pub pending_sub_pane_ratios: HashMap<u64, HashMap<u64, f32>>,
    pub pending_sub_pane_above_main: HashMap<u64, HashSet<u64>>,
    pub pending_sub_pane_order: HashMap<u64, Vec<u64>>,
    pub needs_initial_viewport_fit: bool,
    pub slot_dockings: [sidebar_content::SlotDockingManager; 4],
    pub panels_store: crate::panels_store::TradingPanelsStore,
    pub focused_free_leaf: Option<(usize, uzor::panels::LeafId)>,
}
