//! Cold-path preset hydration.
//!
//! Pure transform `&ChartPreset -> HydratedPresetState`. Designed to run on
//! a worker thread; never touches `DataBridge`. The resulting struct holds
//! exactly the set of fields that `ChartApp` swaps in on a preset switch —
//! field-for-field compatible with [`super::live_cache::LivePresetState`]
//! plus a `request_bars_seeds` list (consumed by [`ignite_subscriptions`]).
//!
//! After swap-in, the main thread calls [`ignite_subscriptions`] to kick off
//! `request_bars` + the data-feed subscriptions (DOM / footprint / l2_tape /
//! etc.) — bridge calls and runtime side-effects live there, not in
//! [`hydrate_preset`].
//!
//! NOTE — implementation is staged:
//! - Skeleton (this file) ships first so `LoadState` + autosave guard can
//!   land without disturbing the existing cold body.
//! - Real `hydrate_preset` body is lifted out of `chart_out_events.rs` in a
//!   later commit and wired through a `UserEvent::PresetHydrated` event.
//! - Until then, cold-path stays synchronous (existing behaviour) and the
//!   new worker pipeline runs only for tabs where the caller opts in.

use std::collections::{HashMap, HashSet};

use mlc_core::state::panel_grid::ChartPanelGrid;
use mlc_backend::bubble::BubbleStore;
use mlc_core::LeafId;
use mlc_backend_indicator_engine::IndicatorManager;

/// Build-result of a pure preset hydration pass.
///
/// Every field here gets `mem::replace`'d into the matching field on
/// `ChartApp` once the host receives `UserEvent::PresetHydrated`. The set
/// mirrors [`super::live_cache::LivePresetState`] plus the `request_bars_seeds`
/// list that drives [`ignite_subscriptions`].
pub struct HydratedPresetState {
    pub panel_grid: ChartPanelGrid,
    pub tag_manager: BubbleStore,
    pub indicator_manager: IndicatorManager,
    pub alert_manager: mlc_backend::alert::AlertManager,
    /// Per-bubble drawing sessions (Step 3.1) — see `LivePresetState::drawing_manager`.
    pub drawing_manager: mlc_backend::drawing::DrawingManager,
    pub leaf_color_tags: HashMap<LeafId, [f32; 4]>,
    pub indicator_overlay_states: HashMap<LeafId, mlc_app_layout::ui::modal_settings::IndicatorOverlayState>,
    pub pending_sub_pane_ratios: HashMap<u64, HashMap<u64, f32>>,
    pub pending_sub_pane_above_main: HashMap<u64, HashSet<u64>>,
    pub pending_sub_pane_order: HashMap<u64, Vec<u64>>,
    pub pending_sub_pane_hidden: HashMap<u64, HashSet<u64>>,
    pub slot_dockings: [mlc_app_sidebar_content::SlotDockingManager; 4],
    pub panels_store: crate::panels_store::TradingPanelsStore,
    /// `(exchange_id_str, symbol, timeframe, account_type_label,
    /// needs_force)`. After swap-in `ignite_subscriptions` walks this list
    /// and calls `bridge.ensure_connector` + `bridge.request_bars`.
    pub request_bars_seeds: Vec<RequestBarsSeed>,
}

#[derive(Debug, Clone)]
pub struct RequestBarsSeed {
    pub exchange: String,
    pub symbol: String,
    pub timeframe: mlc_core::state::Timeframe,
    pub account_type: String,
    /// `true` only when the window will arrive without any cached bars —
    /// mirrors the cure-defer "empty window" rule. Default `false`.
    pub force: bool,
}
