//! Chart Window - Multi-window chart support
//!
//! Each ChartContainer represents an independent chart view with its own:
//! - Data (bars, indicators)
//! - Viewport (pan/zoom state)
//! - Price scale (Y-axis)
//! - Sub-panes (indicator panels)
//! - Symbol and timeframe
//! - Drawing primitives
//! - Indicators (via IndicatorSource trait object)
//! - All display options
//!
//! Multiple ChartContainer instances can be synced via SyncMode settings.

use std::collections::HashMap;
use std::sync::Arc;

use mlc_core::ids::{BubbleId, ChartId, ChartPanelId, generate_chart_id, generate_chart_panel_id};
use mlc_core::{Bar, Viewport, PriceScale, TimeScale};
use mlc_core::chart::types::price_scale::ScaleMode;
use mlc_core::state::Timeframe;
use mlc_core::{Crosshair, KineticState, DragMode};
use mlc_core::{Legend, Watermark, PriceLine};
use crate::chart_panel_state::ChartPanelState;
use mlc_core::state::sub_pane_y_state::SubPaneYState;
use mlc_core::chart::{CompareOverlay, CompareSeries, get_compare_color};
use mlc_core::state::CommandHistory;
use mlc_core::data_provider::{SharedDataProvider, NullDataProvider};
use mlc_core::scale_settings::ScaleSettings;
use mlc_core::state::toolbar_state::{ChartToolbarState, ToolbarConfig};
use mlc_core::state::modal_data::SubPaneOverlayState;
use mlc_core::state::sub_pane_overlay::SubPaneOverlayResult;

/// Rectangle defining a window's position and size (f32 coordinates)
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl WindowRect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    /// Check if point is inside this rect
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.width &&
        py >= self.y && py < self.y + self.height
    }

    /// Convert global coordinates to local (window-relative)
    pub fn global_to_local(&self, global_x: f32, global_y: f32) -> (f32, f32) {
        (global_x - self.x, global_y - self.y)
    }
}

/// Default gap between windows in multi-window layouts
pub const WINDOW_GAP: f32 = 4.0;


/// Individual chart window with its own state
///
/// Each window is a fully independent chart with its own data, scales,
/// drawings, indicators, and display options.
pub struct ChartContainer {
    /// Unique chart identifier (ChartId)
    pub id: ChartId,

    /// Identifier of this container's canvas leaf — used to look up
    /// `ChartPanelState` in `TradingPanelsStore.chart_panel` after the
    /// Phase 4 S2.6 canvas-state migration. Allocated fresh on every
    /// construction (including `clone_for_split`).
    pub chart_panel_id: ChartPanelId,

    /// Permanent home bubble for this chart, created at chart birth and never
    /// changed.  Holds home-state primitives and command history while the
    /// chart is in a color-tag sync group.
    ///
    /// `BubbleId(0)` is a construction-time sentinel only.  Every code path
    /// that uses a `ChartContainer` must ensure this has been set to a real
    /// bubble before the container is active.
    pub auto_tag_id: BubbleId,

    /// The bubble this chart currently displays.  Equal to `auto_tag_id`
    /// when ungrouped; flipped to the color-tag bubble on sync join; flipped
    /// back to `auto_tag_id` on desync.
    ///
    /// `BubbleId(0)` is a construction-time sentinel (same as `auto_tag_id`).
    pub current_tag_id: BubbleId,

    // === Data ===
    // Phase X0 / C5c (2026-06-19): per-Kind series storage moved to DataBubble.
    // ChartContainer no longer owns bars / footprint_bars / renko_bars /
    // three_line_break / pnf_columns / kagi_segments / cvd_line / tpo_sessions /
    // oi_delta / funding_rate / dom_heatmap / dom_trade_dots / liq_heatmap / liq_dots.
    // The renderer reads them via `to_render_state(&bubble, ...)`; window
    // methods take `bars: &[Bar]` parameters (caller passes `&bubble.bars`).

    /// Heikin Ashi memo cache. Populated by
    /// [`ChartContainer::ensure_ha_cache`] and invalidated whenever the
    /// cache stamp no longer matches `bars`. The cache stamp is
    /// `(bars.len(), last_ts_ms, last_close_bits, last_high_bits, last_low_bits)`.
    /// Not serialized — recomputed on first frame after deserialize.
    pub ha_cache: Option<Vec<mlc_core::types::HaBar>>,
    /// Stamp paired with `ha_cache` —
    /// `(len, last_ts_ms, last_close_bits, last_high_bits, last_low_bits)`.
    /// The three `_bits` fields are `f64::to_bits()` — bit-identical comparison
    /// that is `Eq`-safe and catches in-place last-bar OHLC mutations (live tick
    /// update on the still-open bar) that share the same `len` and `timestamp`.
    pub ha_cache_stamp: Option<(usize, i64, u64, u64, u64)>,

    /// Volume Profile overlay data. Computed by `ensure_vp()` before each
    /// frame while a visible `overlay_volume_profile` instance exists. Not
    /// serialized — recomputed on load.
    pub vp_overlay: Option<mlc_core::chart::volume_profile::VolumeProfileData>,
    /// Stamp paired with `vp_overlay` —
    /// `(bars.len(), last_ts_ms, view_start_idx, visible_count, mode_hash, last_close_bits, last_high_bits, last_low_bits)`.
    /// The three `_bits` fields catch in-place last-bar OHLC mutations on the
    /// still-open bar (same len + ts but different close/high/low).
    pub vp_overlay_stamp: Option<(usize, i64, usize, usize, u64, u64, u64, u64, usize, u64, usize)>,
    /// Which window of bars the VP aggregates over. Pushed here by the
    /// prepass from the instance's `mode` parameter — the prepass reads it
    /// under a `containers_mut()` borrow that cannot reach the indicators
    /// facade, so the resolved value rides on the window.
    pub vp_mode: mlc_core::chart::volume_profile::VpMode,

    /// TPO Profile overlay data (item D — block-histogram time-at-price
    /// profile). Computed by `ensure_tpo_profile_from_bars()` before each
    /// frame while a visible `overlay_tpo_profile` instance exists. Not
    /// serialized — recomputed on load. Mirrors `vp_overlay`.
    pub tpo_profile_overlay: Option<mlc_core::chart::tpo_profile::TpoProfileData>,
    /// Stamp paired with `tpo_profile_overlay` —
    /// `(visible_count, view_start_idx, last_close_bits, last_high_bits, last_low_bits)`.
    /// TPO Profile always aggregates the VISIBLE range (no mode selector),
    /// so the stamp is narrower than VP's — it catches a pan/zoom, an
    /// in-place last-bar mutation, or a change of the shape parameters.
    pub tpo_profile_overlay_stamp: Option<(usize, usize, u64, u64, u64, u64, u64, usize)>,

    // The four `*_show` display gates that used to sit here — `vp_show`,
    // `tpo_profile_show`, `oi_show`, `funding_show` — are gone (2026-08-03).
    // Every one of those layers is a real `IndicatorInstance` whose
    // visibility IS the gate, and by the end the bools had no writer at all:
    // their toggle events lost their last emitter, so `tpo_profile_show`
    // held the TPO layer dark inside the trading container while the object
    // tree showed it live. A second switch for one layer is a second answer
    // to the same question.


    // === Viewport & Scales ===
    /// Viewport state (pan/zoom). PRIVATE — single-writer: all mutation
    /// goes through the intent methods below (`pan`, `zoom_at`,
    /// `begin_fresh_load`, `resize_geometry`, ...). Reads via
    /// [`ChartContainer::viewport`].
    viewport: Viewport,
    /// Price scale (Y-axis). PRIVATE — same single-writer discipline as
    /// `viewport`. Reads via [`ChartContainer::price_scale`].
    price_scale: PriceScale,
    /// Time scale (X-axis) - shared visual config
    pub time_scale: TimeScale,

    // === Sub-panes ===
    /// Per-instance Y-axis state (price_min/max, auto_scale, drag start).
    /// Keyed by indicator `instance_id`. Entry is absent until the sub-pane
    /// is first rendered or manipulated.
    pub sub_pane_y_state: HashMap<u64, SubPaneYState>,
    /// Per-sub-pane overlay button UI state (hover visibility, hovered button).
    ///
    /// Indexed in tree order of indicator leaves in `inner_docking`.
    /// Grown on demand; missing entries are treated as default (not visible).
    pub sub_pane_overlay_states: Vec<SubPaneOverlayState>,
    /// Cached sub-pane overlay button rects from the last render frame.
    /// Used by hit tester to detect button clicks.
    pub sub_pane_overlay_results: Vec<SubPaneOverlayResult>,
    /// Main pane's own overlay button UI state (hover-triggered visibility).
    /// Mirrors a single entry of `sub_pane_overlay_states` but for the main
    /// chart slot, which has no indicator instance identity of its own.
    pub main_pane_overlay_state: SubPaneOverlayState,
    /// Cached main-pane overlay button rect from the last render frame.
    /// Used by hit tester to detect Expand/Restore clicks on the main chart.
    pub main_pane_overlay_result: Option<SubPaneOverlayResult>,

    // === Interaction State ===
    /// Crosshair state (position, visibility)
    pub crosshair: Crosshair,
    /// Viewer is following the author's viewport. Local pan and zoom wait.
    pub follow_remote_view: bool,
    /// Kinetic scrolling state
    pub kinetic: KineticState,
    /// Current drag mode
    pub drag_mode: DragMode,
    /// Last mouse position
    pub last_mouse_pos: Option<(f32, f32)>,
    /// Drag start X coordinate
    pub drag_start_x: f64,
    /// Drag start Y coordinate
    pub drag_start_y: f64,
    /// Drag start view position
    pub drag_start_view: f64,
    /// Drag start bar spacing
    pub drag_start_spacing: f64,
    /// Drag start price min
    pub drag_start_price_min: f64,
    /// Drag start price max
    pub drag_start_price_max: f64,

    // === Object Managers ===
    /// Horizontal price lines
    pub price_lines: HashMap<String, PriceLine>,

    // === Compare Overlay ===
    /// Symbol comparison overlay (per-window state)
    pub compare_overlay: CompareOverlay,

    // === Command History ===
    /// Undo/redo command history for this window
    pub command_history: CommandHistory,

    // === Data Provider ===
    /// Data provider for loading bars (demo, exchange, etc.)
    pub data_provider: SharedDataProvider,

    // === Scale Settings ===
    /// Scale display settings (positioning, dimensions, visibility)
    pub scale_settings: ScaleSettings,

    /// Local toolbar state for this chart panel (autonomous panel architecture)
    pub chart_toolbar: ChartToolbarState,

    /// Per-toolbar visibility configuration for this window.
    ///
    /// Primary windows default to `ToolbarConfig::terminal()` (top + left).
    /// Standalone chart-app overrides this to `ToolbarConfig::standalone()` (all 4).
    /// Windows created via `clone_for_split` use `ToolbarConfig::minimal()` so
    /// their full rect is available as chart content.
    pub toolbar_config: ToolbarConfig,

    /// Per-symbol drawing cache. When the user switches symbols, the current
    /// drawings are snapshotted here keyed by the old symbol, and drawings
    /// for the new symbol are restored (if any).
    pub symbol_drawings: std::collections::HashMap<String, Vec<mlc_core::drawing::PrimitiveSnapshot>>,

    /// Deferred viewport / price-scale position to apply once `BarsLoaded` fires.
    /// Set to `true` when a symbol switch is initiated (bars cleared, new request sent).
    /// Forces the `BarsLoaded` handler to take the initial-load path (set_bars + reposition
    /// to end) even if a stray `TradeUpdate` inserted a synthetic bar before bars arrived.
    /// Cleared after `set_bars()` runs in the `BarsLoaded` handler.
    pub pending_symbol_load: bool,

    /// Set to `true` when `set_bars()` is called before `chart_width` is valid (still 0.0).
    /// In that case `calc_auto_scale()` / `visible_bars()` would operate on an empty range
    /// and produce wrong Y-axis bounds. `prepare_frame()` checks this flag after
    /// `sync_viewport_from_layout()` has set the real dimensions and re-runs the scale.
    pub needs_auto_scale_after_bars: bool,

    /// Scale mode to restore after the next `set_bars()` call completes.
    ///
    /// Set by `LoadPreset` before bars arrive asynchronously. Consumed and
    /// cleared inside `set_bars()` — applied after snap-to-end and auto-scale
    /// so the user's Manual/Auto preference survives the async boundary.
    /// `None` = no restoration needed; `set_bars()` leaves the mode as-is.
    /// Runtime-only, not persisted.
    pub restore_scale_mode: Option<ScaleMode>,

    /// When `true`, the `BackfillComplete` handler must call `snap_to_end` after
    /// delivering fresh bars.  Set on cold preset-restore / new-window spawn
    /// (the stale cached bars that arrive via `BarsLoaded` / `set_bars` snap to
    /// the wrong position; the correct fresh data only lands later via
    /// `BackfillComplete`).  Cleared by the handler after snapping.
    /// Runtime-only, not persisted.
    pub needs_snap_after_backfill: bool,

    /// (head_ts_ms, last_ts_ms) of the derived active series as of the last
    /// `reconcile_derived_series` run. Lets the next run tell a prepend
    /// (lazy-deepen / scroll-left reseed merging older history in front —
    /// head_ts moves older, every index shifts right) from a tail append
    /// (live bar). `None` until the first non-empty reconcile; reset by
    /// `begin_fresh_load*`. Runtime-only, not persisted.
    pub(crate) derived_series_stamp: Option<(i64, i64)>,

    /// One-shot: viewport width materially changed (split, window resize,
    /// pane re-proportion) → in Auto mode the Y range must be refit to the
    /// NEW visible window. Set by `resize_geometry`, consumed by
    /// `consume_y_rescale` in prepare (after geometry settles, same frame).
    /// Invariant: Auto-mode Y always wraps the CURRENT visible window —
    /// panes must never inherit a Y computed for a different width
    /// (split-Y staleness bug, 2026-07-04). Runtime-only, not persisted.
    pub needs_y_rescale: bool,

    /// Pre-join scale mode: saved when this window enters a group with
    /// `sync_viewport = ON`. Restored automatically on desync so the window
    /// returns to its own A/M/F choice. F (Focus) implies a pinned viewport,
    /// so mode is tightly coupled to viewport sync state.
    pub stashed_scale_mode: Option<ScaleMode>,

    /// Whether the chart's home stash (auto_bubble.primitives +
    /// pre_tag_indicator_ids) should be rendered as a "Memory" overlay in the
    /// Object Tree while the chart is in its current color tag.  Set to
    /// `true` only on flows that legitimately carry the chart's home content
    /// forward into the new tag (first untagged -> tag, or split sibling
    /// inheriting from a parent that had it).  Re-tag (tag -> tag) sets it
    /// to `false`: the new tag is a fresh bucket and must not display the
    /// home stash as memory in its tree.  Reset to `false` on
    /// `perform_desync` so the next tag entry decides afresh.
    pub tag_inherits_home_overlay: bool,

    /// Last data-fetch error for this window, if any.
    ///
    /// Set by the `LiveUpdate::Error` handler when an error is matched to this
    /// window's instrument key.  Cleared on any successful `set_bars` call and
    /// on symbol/timeframe change (`pending_symbol_load = true` sites).
    /// Runtime-only — not persisted to disk.
    pub data_error: Option<String>,

    /// Inner uzor docking tree: [MainChart leaf, SubPane leaf, ...].
    /// Single source of truth for sub-pane identity, proportions, and
    /// separator drag. Replaces `inner_layout` (which is kept only during
    /// the dual-write transition and deleted once all reads migrate here).
    pub inner_docking: uzor::layout::DockState<crate::chart_container_leaf::ChartContainerLeaf>,

    /// Set of sub-pane instance_ids that are positioned ABOVE the main chart.
    /// Runtime state — not backed by the inner_docking tree (which always stores
    /// panes in below order for proportion/identity purposes). The layout builder
    /// reads this set to reorder panes into above-main positions in the emitted
    /// geometry. Populated from snapshots via `pending_sub_pane_above_main` in
    /// `ChartApp::sync_sub_panes_from_manager`.
    pub sub_pane_above_main: std::collections::HashSet<u64>,

}

/// Default number of empty bars shown to the right of the last candle
/// after a snap-to-end operation. Will be user-configurable in the future.
pub const DEFAULT_SNAP_MARGIN: f64 = 5.0;

/// Bars searched on EACH side of the cursor's bar for the magnet's nearest
/// OHLC extreme (owner spec 2026-08-13, window magnet — see
/// [`ChartContainer::calculate_magnet_snap`]). A swing high/low rarely
/// lands exactly under the cursor at typical zoom levels — being off by a
/// bar or two is common — so the window widens the search past the single
/// bar the mouse happens to be over. 3 bars each side (7-bar window) catches
/// a near-miss without reaching into a neighbouring swing.
pub const MAGNET_WINDOW_BARS: usize = 3;

/// Build a fresh `DockState<ChartContainerLeaf>` with a single `MainChart` leaf.
///
/// Used by every `ChartContainer` constructor. The main chart leaf's
/// `chart_panel_id` matches `ChartContainer.chart_panel_id` so that lookups
/// in `TradingPanelsStore.chart_panel` remain consistent.
pub fn make_inner_docking_with_main_chart(
    panel_id: ChartPanelId,
) -> uzor::layout::DockState<crate::chart_container_leaf::ChartContainerLeaf> {
    let mut ds = uzor::layout::DockState::new();
    ds.tree_mut().add_leaf(
        crate::chart_container_leaf::ChartContainerLeaf::MainChart { chart_panel_id: panel_id },
    );
    pin_vertical_stack_layout(&mut ds);
    ds
}

/// Force the inner-docking root branch to a vertical stack layout.
///
/// uzor's `add_leaf` / `add_leaf_near` / `remove_leaf` re-infer the branch
/// layout from child count (2 → SplitHorizontal, 3 → ThreeColumns,
/// 4 → Grid2x2, 5+ → Custom) — all HORIZONTAL layouts. The chart container
/// stack is always vertical: a horizontal label breaks `drag_separator`
/// (width basis + 80px width minimums instead of height basis + 26px) and
/// at 4 children routes the drag into the cross_ratio fast-path where
/// proportions are never written at all. Call after every structural
/// mutation of the tree. Writes the field directly — `set_layout` would
/// clear `proportions` and wipe user drag state.
pub fn pin_vertical_stack_layout(
    inner_docking: &mut uzor::layout::DockState<crate::chart_container_leaf::ChartContainerLeaf>,
) {
    let root_id = inner_docking.tree().root().id;
    if let Some(root) = inner_docking.tree_mut().find_branch_mut(root_id) {
        root.layout = uzor::panels::WindowLayout::SplitVertical;
        root.cross_ratio = None;
    }
}

/// Collect indicator `instance_id`s from an inner docking tree in tree order
/// (skipping the root MainChart leaf at index 0).
pub fn collect_inner_instance_ids(
    inner_docking: &uzor::layout::DockState<crate::chart_container_leaf::ChartContainerLeaf>,
) -> Vec<u64> {
    inner_docking.tree().root().children
        .iter()
        .skip(1)
        .filter_map(|node| {
            if let uzor::panels::PanelNode::Leaf(leaf) = node {
                if leaf.hidden {
                    return None;
                }
                leaf.panels.first().and_then(|p| p.indicator_instance_id())
            } else {
                None
            }
        })
        .collect()
}

/// Collect indicator `instance_id`s for hidden sub-pane leaves.
/// Used by `ChartWindowSnapshot::from_window` to capture the hidden set.
pub fn collect_hidden_instance_ids(
    inner_docking: &uzor::layout::DockState<crate::chart_container_leaf::ChartContainerLeaf>,
) -> std::collections::HashSet<u64> {
    inner_docking.tree().root().children
        .iter()
        .skip(1)
        .filter_map(|node| {
            if let uzor::panels::PanelNode::Leaf(leaf) = node {
                if leaf.hidden {
                    leaf.panels.first().and_then(|p| p.indicator_instance_id())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

/// Collect proportions from an inner docking tree in parallel with
/// `collect_inner_instance_ids`.  Index 0 = main chart share, [1..] = sub-panes.
pub fn collect_inner_proportions(
    inner_docking: &uzor::layout::DockState<crate::chart_container_leaf::ChartContainerLeaf>,
) -> Vec<f64> {
    inner_docking.tree().root().proportions.clone()
}

impl ChartContainer {
    /// Create a new chart window with a data provider.
    pub fn new_with_provider(_symbol: &str, _timeframe: Timeframe, data_provider: SharedDataProvider) -> Self {
        let id = generate_chart_id();
        let panel_id = generate_chart_panel_id();

        Self {
            id,
            chart_panel_id: panel_id,
            // Sentinel — caller must set auto_tag_id / current_tag_id
            // immediately after construction via post-construction mutation.
            auto_tag_id: BubbleId(0),
            current_tag_id: BubbleId(0),
            ha_cache: None,
            ha_cache_stamp: None,
            vp_overlay: None,
            vp_overlay_stamp: None,
            vp_mode: mlc_core::chart::volume_profile::VpMode::default(),
            tpo_profile_overlay: None,
            tpo_profile_overlay_stamp: None,
            viewport: Viewport::default(),
            price_scale: PriceScale::default(),
            time_scale: TimeScale::new(),
            sub_pane_y_state: HashMap::new(),
            sub_pane_overlay_states: Vec::new(),
            sub_pane_overlay_results: Vec::new(),
            main_pane_overlay_state: SubPaneOverlayState::default(),
            main_pane_overlay_result: None,
            // Interaction
            crosshair: Crosshair::default(),
            follow_remote_view: false,
            kinetic: KineticState::new(),
            drag_mode: DragMode::None,
            last_mouse_pos: None,
            drag_start_x: 0.0,
            drag_start_y: 0.0,
            drag_start_view: 0.0,
            drag_start_spacing: 0.0,
            drag_start_price_min: 0.0,
            drag_start_price_max: 0.0,
            // Object managers (DrawingManager lives in BackendCore::drawing, keyed by BubbleId)
            price_lines: HashMap::new(),
            // Compare overlay (per-window)
            compare_overlay: CompareOverlay::new(),
            // Command history (250 undo levels per window)
            command_history: CommandHistory::new(250),
            // Data provider
            data_provider,
            // Scale settings
            scale_settings: ScaleSettings::default(),
            // Toolbar state
            chart_toolbar: ChartToolbarState::default(),
            // Toolbar config (DEBUG: standalone mode with all 4 toolbars)
            toolbar_config: ToolbarConfig::standalone(),
            symbol_drawings: HashMap::new(),
            pending_symbol_load: false,
            needs_auto_scale_after_bars: false,
            restore_scale_mode: None,
            stashed_scale_mode: None,
            tag_inherits_home_overlay: false,
            needs_snap_after_backfill: false,
            derived_series_stamp: None,
            needs_y_rescale: false,
            data_error: None,
            inner_docking: make_inner_docking_with_main_chart(panel_id),
            sub_pane_above_main: std::collections::HashSet::new(),
        }
    }

    /// Create a new chart window with default settings (uses NullDataProvider).
    pub fn new(symbol: &str, timeframe: Timeframe) -> Self {
        Self::new_with_provider(symbol, timeframe, Arc::new(NullDataProvider))
    }

    /// Create with specific ID (for deserialization)
    pub fn with_id(id: ChartId, symbol: &str, timeframe: Timeframe) -> Self {
        let mut window = Self::new(symbol, timeframe);
        window.id = id;
        window
    }

    // =========================================================================
    // Inner-docking helpers.
    // =========================================================================

    /// Return the separator index in `inner_docking.separators()` for the
    /// separator that sits above the `SubPane` leaf with `instance_id`.
    ///
    /// The separators list is only valid after `inner_docking.layout(...)` has
    /// been called. The caller must ensure layout is fresh before calling this.
    fn sep_index_for_instance(&self, instance_id: u64) -> Option<usize> {
        // Walk inner_docking leaves to find the leaf_id for this instance_id.
        let target_leaf_id = self.inner_docking.tree().leaves().iter()
            .find(|leaf| {
                leaf.panels.first()
                    .and_then(|p| p.indicator_instance_id())
                    == Some(instance_id)
            })
            .map(|leaf| leaf.id)?;

        // Walk separators to find the one whose child_b is this leaf's raw id.
        self.inner_docking.separators().iter().position(|sep| {
            sep.child_b() == Some(target_leaf_id.0)
        })
    }

    /// Drag the BOTTOM separator of an ABOVE-MAIN pane.
    ///
    /// Above-main panes are a presentation-only reordering: the tree stores
    /// them as ordinary below-main siblings, so uzor's separator pairs
    /// (tree-adjacent children) don't correspond to the visually adjacent
    /// elements. The visual pair for pane P's bottom line is (P, X) where X
    /// is the NEXT above pane in tree order, or the main chart when P is the
    /// lowest above pane. Transfer share directly between those two root
    /// children: dragging down grows P by `delta_y` pixels and shrinks X.
    ///
    /// Returns `true` when proportions were updated.
    pub fn drag_above_pane_separator(
        &mut self,
        instance_id: u64,
        delta_y: f64,
        content_h: f64,
    ) -> bool {
        const MIN_PANE_PX: f64 = 26.0;
        let ids = collect_inner_instance_ids(&self.inner_docking);
        let above: Vec<u64> = ids
            .iter()
            .filter(|id| self.sub_pane_above_main.contains(id))
            .copied()
            .collect();
        let pos_in_above = match above.iter().position(|&id| id == instance_id) {
            Some(p) => p,
            None => return false,
        };
        // Proportion indices: main = 0, sub i (tree order) = i + 1.
        let idx_p = match ids.iter().position(|&id| id == instance_id) {
            Some(i) => i + 1,
            None => return false,
        };
        let idx_x = if pos_in_above + 1 < above.len() {
            match ids.iter().position(|&id| id == above[pos_in_above + 1]) {
                Some(i) => i + 1,
                None => return false,
            }
        } else {
            0 // lowest above pane borders the main chart
        };

        let root_id = self.inner_docking.tree().root().id;
        let mut props = self.inner_docking.tree().root().proportions.clone();
        let n = self.inner_docking.tree().root().children.len();
        if props.len() != n || idx_p >= n || idx_x >= n || idx_p == idx_x {
            return false;
        }
        let total: f64 = props.iter().sum();
        let budget = content_h.max(1.0);
        if total <= 0.0 {
            return false;
        }
        let min_share = MIN_PANE_PX / budget * total;
        // Dragging the bottom line down grows P, shrinks X.
        let want = delta_y / budget * total;
        let take = want.clamp(
            -(props[idx_p] - min_share).max(0.0),
            (props[idx_x] - min_share).max(0.0),
        );
        if take == 0.0 {
            return false;
        }
        props[idx_p] += take;
        props[idx_x] -= take;
        self.inner_docking.tree_mut().set_branch_proportions(root_id, props);
        true
    }

    /// Apply a sub-pane separator drag through the uzor inner docking tree.
    /// `delta_y` is per-event pixels (positive = drag down).
    /// `content_w` / `content_h` are the pixel dimensions of the container's
    /// content area (excluding toolbars and time scale).
    pub fn drag_sub_pane_separator(
        &mut self,
        instance_id: u64,
        delta_y: f64,
        content_h: f64,
        content_w: f64,
    ) -> bool {
        // Re-pin the vertical stack layout: any add/remove since the last
        // sync re-inferred a horizontal layout, which would flip the drag
        // axis to width and (at 4 children) divert into cross_ratio.
        pin_vertical_stack_layout(&mut self.inner_docking);
        // Ensure separators list is fresh before looking up sep_idx.
        self.inner_docking.layout(uzor::panels::PanelRect::new(
            0.0, 0.0, content_w as f32, content_h as f32,
        ));
        let sep_idx = match self.sep_index_for_instance(instance_id) {
            Some(i) => i,
            None => return false,
        };
        self.inner_docking.drag_separator(
            sep_idx,
            delta_y as f32,
            content_w as f32,
            content_h as f32,
        )
    }

    /// Snapshot the supplied primitives into the per-symbol cache.
    ///
    /// The cache key is `"symbol:exchange:account_type"` (e.g. `"BTCUSDT:binance:S"`).
    /// Caller extracts the live primitives from the `DrawingManager`
    /// (which lives backend-side) and hands the slice in — this keeps
    /// `ChartContainer` free of the manager type.
    pub fn snapshot_drawings_for_symbol(
        &mut self,
        symbol: &str,
        exchange: &str,
        account_type: &str,
        primitives: &[Box<dyn mlc_core::drawing::primitives_v2::Primitive>],
    ) {
        let key = format!("{}:{}:{}", symbol, exchange, account_type);
        let snapshots: Vec<mlc_core::drawing::PrimitiveSnapshot> = primitives
            .iter()
            .map(|p| mlc_core::drawing::PrimitiveSnapshot::from_primitive(p.as_ref()))
            .collect();
        if !snapshots.is_empty() {
            self.symbol_drawings.insert(key, snapshots);
        } else {
            self.symbol_drawings.remove(&key);
        }
    }

    /// Look up cached primitive snapshots for the given symbol.
    ///
    /// Returns a clone of the saved snapshots so the caller can rebuild
    /// concrete `Box<dyn Primitive>` instances through the
    /// `PrimitiveRegistry` and push them back into the live
    /// `DrawingManager`.  Returns `None` when nothing is cached.
    pub fn take_drawings_snapshot_for_symbol(
        &self,
        symbol: &str,
        exchange: &str,
        account_type: &str,
    ) -> Option<Vec<mlc_core::drawing::PrimitiveSnapshot>> {
        let key = format!("{}:{}:{}", symbol, exchange, account_type);
        self.symbol_drawings.get(&key).cloned()
    }

    // `clone_for_split` was deleted — every split path uses
    // `clone_for_split_shell` instead, which never touched DrawingManager.
    // The DM-aware clone path was dead code blocking the manager's
    // backend-side relocation.

    // =========================================================================
    // Viewport / price-scale readers (single-writer: no `_mut` counterpart —
    // all mutation goes through the intent methods in the Viewport Intents
    // section below).
    // =========================================================================

    /// Read-only access to the viewport (pan/zoom/geometry state).
    #[inline]
    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    /// Read-only access to the price scale (Y-axis).
    #[inline]
    pub fn price_scale(&self) -> &PriceScale {
        &self.price_scale
    }

    /// Follow a remote Live viewport. `price` locks the Y range to manual;
    /// `None` leaves the local scale mode alone.
    pub fn apply_shared_view(&mut self, view_start: f64, bar_spacing: f64, price: Option<(f64, f64)>) {
        if view_start.is_finite() {
            self.viewport.view_start = view_start;
        }
        if bar_spacing.is_finite() && bar_spacing > 0.0 {
            let min = self.viewport.min_bar_spacing();
            let mut max = self.viewport.max_bar_spacing();
            if max < min {
                max = min;
            }
            self.viewport.bar_spacing = bar_spacing.clamp(min, max);
        }
        if let Some((lo, hi)) = price {
            if lo.is_finite() && hi.is_finite() && hi > lo {
                self.price_scale.price_min = lo;
                self.price_scale.price_max = hi;
                self.price_scale.scale_mode = ScaleMode::Manual;
            }
        }
    }

    /// Lock or release pan/zoom to a remote Live viewport.
    pub fn set_follow_remote_view(&mut self, follow: bool) {
        self.follow_remote_view = follow;
    }

    /// Lock or release the crosshair to a remote Live cursor.
    pub fn set_follow_remote_cursor(&mut self, follow: bool) {
        self.crosshair.follow_remote = follow;
    }

    /// Crosshair state for rendering (respects render flag).
    pub fn crosshair_for_render(&self, render_crosshair: bool) -> Crosshair {
        if render_crosshair {
            self.crosshair
        } else {
            let mut disabled = self.crosshair;
            disabled.enabled = false;
            disabled.visible = false;
            disabled
        }
    }

    /// Legend state for rendering (respects render flag).
    pub fn legend_for_render(legend: &Legend, render_legend: bool) -> Legend {
        if render_legend {
            legend.clone()
        } else {
            let mut l = legend.clone();
            l.visible = false;
            l
        }
    }

    /// Calculate auto-scale for price axis based on visible data.
    ///
    /// Audit D8 (ActiveSeries seam): the price range comes from whatever
    /// array the chart_type actually renders (renko bricks, pnf columns,
    /// ...), not always `bubble.bars` — callers resolve via
    /// [`crate::active_series::resolve_active_series`] and pass the result
    /// straight through. Mirrors `PriceScale::calc_auto_scale`'s body
    /// (empty-range no-op, 4% padding) without needing a `&[Bar]`-shaped
    /// view of non-Bar series.
    ///
    /// Owner redefinition (2026-07-10, supersedes the 2026-07-06 "F is
    /// grow-only fixation" doctrine): **F behaves exactly like A** — full
    /// refit to the visible bars, shrinking and growing with them — PLUS a
    /// focus guard applied on top: [`Self::apply_focus_guard`] extends
    /// (never cuts) the just-computed A-fit range so the last tick price
    /// keeps `FOCUS_Y_GUARD_RATIO` of pane-height margin from both edges.
    /// The two modes now share the SAME fit computation; F differs only in
    /// the guard-band extension applied afterward.
    pub fn calc_auto_scale(&mut self, series: crate::active_series::ActiveSeries<'_>) {
        if !self.price_scale.scale_mode.is_auto_y() {
            return;
        }
        let (start, end) = self.viewport.visible_range();
        if start >= end || series.is_empty() {
            return;
        }
        let Some((min, max)) = series.y_range(start..end) else { return };
        let range = max - min;
        let padding = range * 0.04;
        self.price_scale.price_min = min - padding;
        self.price_scale.price_max = max + padding;

        if self.price_scale.scale_mode == ScaleMode::Focus {
            self.apply_focus_guard(series);
        }
    }

    /// Focus-mode guard band (2026-07-10 doctrine): extend — never cut —
    /// the range [`Self::calc_auto_scale`] just fit to the visible bars, so
    /// the last tick price keeps at least `FOCUS_Y_GUARD_RATIO * pane_height`
    /// of margin from both the top and bottom edges. Called ONLY right
    /// after the A-fit computation in [`Self::calc_auto_scale`] — this is
    /// the "with focus" half of "F = A with focus", not an independent
    /// entry point.
    ///
    /// Superseded 2026-07-06 doctrine: F used to be grow-only fixation
    /// (user's Y position preserved, only pressure from the last bar
    /// expanded bounds). Owner override 2026-07-10: F now refits fully like
    /// A on every call (shrink included) — this guard only ever WIDENS the
    /// already-fresh A-fit range by extending one or both edges outward,
    /// never narrows it and never runs independently of a fresh A-fit.
    fn apply_focus_guard(&mut self, series: crate::active_series::ActiveSeries<'_>) {
        if self.viewport.chart_height <= 0.0 {
            return;
        }
        // F Y guard tracks the LIVE TICK PRICE (last close), not the last
        // bar's high/low. Tick can move between bar h/l values or breach
        // either as it moves — the guard must catch it on the tick itself,
        // universally, regardless of chart type. Series-family agnostic:
        // last-element close for bar-shaped series, last-element value for
        // scalar series.
        let last_price = match series {
            crate::active_series::ActiveSeries::Bars(s) => s.last().map(|b| b.close),
            crate::active_series::ActiveSeries::Renko(s) => s.last().map(|b| if b.up { b.top } else { b.bottom }),
            crate::active_series::ActiveSeries::Pnf(s) => s.last().map(|c| if c.is_x { c.top } else { c.bottom }),
            crate::active_series::ActiveSeries::Kagi(s) => s.last().map(|k| k.end_price),
            crate::active_series::ActiveSeries::Tlb(s) => s.last().map(|l| l.high()),
            crate::active_series::ActiveSeries::Cvd(s) => s.last().map(|c| c.value),
            crate::active_series::ActiveSeries::Tpo(_) => None,
        };
        let Some(tick) = last_price else { return };
        let cur_min = self.price_scale.price_min;
        let cur_max = self.price_scale.price_max;
        let cur_range = cur_max - cur_min;
        if cur_range <= 0.0 || !cur_range.is_finite() {
            return;
        }
        // 2% of pane height, in price units. cur_range spans chart_height
        // pixels, so 2% * chart_height * price_per_px == 0.02 * cur_range.
        const FOCUS_Y_GUARD_RATIO: f64 = 0.02;
        let guard_price = FOCUS_Y_GUARD_RATIO * cur_range;
        // Extend-only: tick wants `>= guard_price` room to both bounds —
        // never pulled back in even if the fresh A-fit already has MORE
        // than the guard margin (that's not this method's job to shrink).
        let want_max = tick + guard_price;
        let want_min = tick - guard_price;
        if want_max > cur_max {
            self.price_scale.price_max = want_max;
        }
        if want_min < cur_min {
            self.price_scale.price_min = want_min;
        }
    }

    /// Consume the `needs_snap_after_backfill` one-shot flag.
    ///
    /// Snaps the viewport to the newest bar (+DEFAULT_SNAP_MARGIN) and forces
    /// a Y autoscale bypassing the Manual-mode gate (the deserialized price
    /// range belongs to a stale viewport position; keeping it renders the fresh
    /// bars as a flat line).  Then clears the flag so the snap fires exactly once
    /// regardless of which delivery path (cache-served BarsLoaded, first-frame
    /// deferred, or BackfillComplete) arrives first.
    /// Audit D8: caller resolves the active series for this window's
    /// chart_type (`bubble.bars` for candle-family, the type's own slot for
    /// renko/pnf/kagi/tlb/cvd) via
    /// [`crate::active_series::resolve_active_series`] and passes it here —
    /// the Y range must come from what's actually rendered, not always the
    /// kline array.
    pub fn consume_cold_snap(&mut self, series: crate::active_series::ActiveSeries<'_>) {
        let (vs0, bc0) = (self.viewport.view_start, self.viewport.bar_count);
        // Cold-load zoom seed (owner doctrine 2026-07-12): the viewport
        // never survives a load — every cold snap starts from the default
        // 100-bar view (`fit_default_view`), then snaps to the newest
        // data. Wide chart types re-widen right after via their per-frame
        // `ensure_min_bar_spacing` (TPO 120px / footprint 50px).
        self.viewport.reset_to_default();
        self.snap_to_end(DEFAULT_SNAP_MARGIN);
        let saved_mode = self.price_scale.scale_mode;
        self.price_scale.scale_mode = mlc_core::chart::types::price_scale::ScaleMode::Auto;
        self.calc_auto_scale(series);
        self.price_scale.scale_mode = saved_mode;
        self.needs_snap_after_backfill = false;
        self.log_view_jump("consume_cold_snap", vs0, bc0);
    }

    /// Reconcile viewport for initial load — resets viewport position and auto-scales.
    ///
    /// Phase X0 / C5c: bars storage lives on the bubble; this method only
    /// performs the viewport-side effects after `bubble.bars` has been
    /// replaced. `bars` is `&bubble.bars` (kline array — every caller of
    /// this method is a kline-exclusive event: `BarsLoaded`, undo/redo
    /// symbol/timeframe change). `series` is the ActiveSeries the window's
    /// chart_type actually renders (audit D8) — for candle-family windows
    /// this is `ActiveSeries::Bars(bars)`, same slice, zero-cost wrap.
    ///
    /// Use for the first data load. For backfill / incremental updates use
    /// [`reconcile_view_keep_anchor`] which preserves the user's viewport.
    pub fn reconcile_view_fresh(&mut self, bars: &[Bar], series: crate::active_series::ActiveSeries<'_>) {
        let (vs0, bc0) = (self.viewport.view_start, self.viewport.bar_count);
        // A successful delivery clears any prior fetch error.
        self.data_error = None;

        // Update prev close line if enabled (derives prev_close from bars[0].open)
        self.update_prev_close_line(bars);

        // Update bar count so the viewport knows how many elements the
        // active series holds.
        self.viewport.bar_count = series.len();

        if self.viewport.chart_width > 0.0 && self.viewport.bar_spacing > 0.0 {
            self.snap_to_end(DEFAULT_SNAP_MARGIN);
            self.calc_auto_scale(series);
            if let Some(mode) = self.restore_scale_mode.take() {
                self.price_scale.scale_mode = mode;
            }
            self.needs_auto_scale_after_bars = false;
        } else {
            self.needs_auto_scale_after_bars = true;
        }
        self.log_view_jump("reconcile_view_fresh", vs0, bc0);
    }

    /// Reconcile viewport after backfill / scroll-load — keep anchor by ts,
    /// keep user's pan/zoom and scale mode.
    ///
    /// Phase X0 / C5c: bars storage lives on the bubble. `bars` is
    /// `&bubble.bars` — every caller is a kline-exclusive event
    /// (`BackfillCompleted`, `ScrollBarsAppended`, `BarsLoaded` backfill
    /// branch), so the anchor lookup stays keyed to the kline timeline.
    /// `series` is the ActiveSeries the window's chart_type renders (audit
    /// D8) — for candle-family windows this is `ActiveSeries::Bars(bars)`.
    /// `prev_anchor_ts` is the timestamp of the bar that WAS at view_start
    /// before bubble.bars was replaced; caller captures it from the old
    /// bubble state right before the bridge writes the new series.
    pub fn reconcile_view_keep_anchor(
        &mut self,
        bars: &[Bar],
        series: crate::active_series::ActiveSeries<'_>,
        prev_anchor: Option<(i64, f64)>,
    ) {
        let (vs0, bc0) = (self.viewport.view_start, self.viewport.bar_count);
        self.update_prev_close_line(bars);

        self.viewport.bar_count = series.len();

        if let Some((anchor_ts, offset)) = prev_anchor {
            if let Some(pos_i) = bars.iter().position(|b| b.timestamp >= anchor_ts) {
                // F2 (viewport doctrine audit 2026-07-07): the anchor ts can
                // vanish from the array after a merge (gap-filling backfill,
                // history trim) — a bare `position()` then lands on whatever
                // bar happens to be next, silently teleporting the view. Pick
                // whichever of the candidate and its left neighbor is
                // actually closer to the anchor ts by absolute distance.
                let candidate_i = if pos_i > 0 {
                    let dist_at = bars[pos_i].timestamp - anchor_ts;
                    let dist_prev = anchor_ts - bars[pos_i - 1].timestamp;
                    if dist_prev < dist_at { pos_i - 1 } else { pos_i }
                } else {
                    pos_i
                };
                let chosen_ts = bars[candidate_i].timestamp;
                let gap = (chosen_ts - anchor_ts).abs();
                // Estimate the local bar interval from the chosen bar's
                // neighbor (prefer the following bar, fall back to the
                // preceding one) — any positive diff is good enough for a
                // tolerance check, never used to relocate the anchor itself.
                let local_interval = bars.get(candidate_i + 1)
                    .map(|b| b.timestamp - chosen_ts)
                    .filter(|d| *d > 0)
                    .or_else(|| {
                        if candidate_i > 0 {
                            Some(chosen_ts - bars[candidate_i - 1].timestamp).filter(|d| *d > 0)
                        } else {
                            None
                        }
                    });
                if let Some(interval) = local_interval {
                    if gap > interval {
                        log::warn!(
                            "[VPANCHOR gap] reconcile_view_keep_anchor: anchor_ts={anchor_ts} nearest_ts={chosen_ts} gap={gap} interval={interval} idx={candidate_i}",
                        );
                    }
                }
                // Never snap, never bail — always land on the nearest bar.
                // `offset` is SIGNED and unbounded (see capture_view_anchor)
                // so out-of-range viewport positions survive the reconcile.
                self.viewport.view_start = candidate_i as f64 + offset;
            }
        }

        // Gate parity with reconcile_view_fresh (audit D1): a placeholder/transient
        // dimension frame must not autoscale Y off whatever sliver of bars is
        // visible at that width — the real resize + tick recalc covers it later.
        if self.price_scale.scale_mode.is_auto_y()
            && self.viewport.chart_width > 0.0
            && self.viewport.bar_spacing > 0.0
        {
            self.calc_auto_scale(series);
        }
        self.log_view_jump("reconcile_view_keep_anchor", vs0, bc0);
    }

    /// Capture the (timestamp, SIGNED offset) of the viewport relative to
    /// the nearest in-range bar. Used by callers before they replace
    /// bubble.bars so they can pass it to `reconcile_view_keep_anchor`.
    ///
    /// Live-caught 2026-07-07 (owner repro, VPJUMP log): the old
    /// `(ts, frac∈[0,1))` shape CLAMPED out-of-range positions — a user
    /// scrolled into the pre-history void (`view_start = -72.4`) got
    /// re-anchored to bar 0 with the -72-bar offset silently dropped, so a
    /// backfill prepend shifted the visual window by 72 bars. The offset is
    /// now signed and unbounded: `view_start = new_index(ts) + offset`
    /// reproduces positions beyond EITHER edge (pre-history void and
    /// future-scroll overhang) exactly.
    pub fn capture_view_anchor(&self, bars: &[Bar]) -> Option<(i64, f64)> {
        if bars.is_empty() {
            return None;
        }
        let vs = self.viewport.view_start;
        let clamped_idx = (vs.floor().max(0.0) as usize).min(bars.len().saturating_sub(1));
        let offset = vs - clamped_idx as f64;
        bars.get(clamped_idx).map(|b| (b.timestamp, offset))
    }

    // `clone_data_from` was unused (mirrors viewport + scales between
    // windows) and forced ChartContainer to know about DrawingManager —
    // deleted alongside the other DM-bound methods.

    /// Recompute the Heikin Ashi memo if the supplied bars slice has changed
    /// since the last build. Phase X0 / C5b (2026-06-19): caller passes the
    /// bubble's `bars` slice; cache stays on the window because it is a
    /// per-window render-time optimization.
    pub fn ensure_ha_cache_from_bars(
        &mut self,
        bars: &[Bar],
    ) {
        let last = bars.last();
        let stamp = (
            bars.len(),
            last.map(|b| b.ts_ms()).unwrap_or(0),
            last.map(|b| b.close.to_bits()).unwrap_or(0),
            last.map(|b| b.high.to_bits()).unwrap_or(0),
            last.map(|b| b.low.to_bits()).unwrap_or(0),
        );
        if self.ha_cache_stamp == Some(stamp) && self.ha_cache.is_some() { return; }
        if bars.is_empty() {
            self.ha_cache = Some(Vec::new());
            self.ha_cache_stamp = Some(stamp);
            return;
        }
        let mut out: Vec<mlc_core::types::HaBar> = Vec::with_capacity(bars.len());
        let first = &bars[0];
        let mut prev_open = (first.open + first.close) * 0.5;
        let mut prev_close = (first.open + first.high + first.low + first.close) * 0.25;
        out.push(mlc_core::types::HaBar {
            ts_ms: first.ts_ms(),
            open: prev_open,
            high: first.high.max(prev_open).max(prev_close),
            low: first.low.min(prev_open).min(prev_close),
            close: prev_close,
        });
        for bar in &bars[1..] {
            let ha_close = (bar.open + bar.high + bar.low + bar.close) * 0.25;
            let ha_open = (prev_open + prev_close) * 0.5;
            let ha_high = bar.high.max(ha_open).max(ha_close);
            let ha_low = bar.low.min(ha_open).min(ha_close);
            out.push(mlc_core::types::HaBar {
                ts_ms: bar.ts_ms(),
                open: ha_open,
                high: ha_high,
                low: ha_low,
                close: ha_close,
            });
            prev_open = ha_open;
            prev_close = ha_close;
        }
        self.ha_cache = Some(out);
        self.ha_cache_stamp = Some(stamp);
    }

    /// Recompute the Volume Profile overlay if the bars window, viewport, or
    /// mode changed since the last build.
    ///
    /// `visible_bars` is the pre-filtered slice of bars already clipped to the
    /// viewport's visible range (caller computes it via `viewport.visible_range()`).
    /// For Session / Composite modes the filtering is done inside `filter_bars_for_mode`.
    ///
    /// `bucket_count`, `value_area` and `mode` all come from the overlay's
    /// instance — they were 24, 0.70 and "whatever the window field held"
    /// until 2026-08-03, and the mode's only switch had been deleted from
    /// the UI, so the layer was stuck on Visible Range.
    /// `foreign` carries the same symbol's bars from OTHER venues, already
    /// time-filtered by the caller. They are appended to whatever the mode
    /// selected, which is exactly the sum of the per-venue profiles: both
    /// profiles are histograms over price, so the union of the bars is the
    /// union of their contributions and the shared bucket grid falls out of
    /// the combined price range. It is deliberately NOT an averaged OHLC
    /// series — there is no such thing as the market's candle.
    pub fn ensure_vp_from_bars(
        &mut self,
        bars: &[Bar],
        visible_bars: &[Bar],
        bucket_count: usize,
        value_area: f64,
        mode: mlc_core::chart::volume_profile::VpMode,
        foreign: &[Bar],
    ) {
        use mlc_core::chart::volume_profile::{VolumeProfileData, VpMode, filter_bars_for_mode};

        self.vp_mode = mode;

        let mode_hash: u64 = match mode {
            VpMode::VisibleRange => 0,
            VpMode::Session => 1,
            VpMode::FixedRange { start_ts_ms, end_ts_ms } => {
                2u64.wrapping_add(start_ts_ms as u64).wrapping_add(end_ts_ms as u64)
            }
            VpMode::Composite => 3,
        };
        let vp_last = bars.last();
        // The shape params ride in the stamp: changing rows or the value
        // area changes the OUTPUT, so a stamp blind to them would keep
        // serving the profile built with the old ones.
        let stamp = (
            bars.len(),
            vp_last.map(|b| b.ts_ms()).unwrap_or(0),
            self.viewport.view_start_idx(),
            visible_bars.len(),
            mode_hash,
            vp_last.map(|b| b.close.to_bits()).unwrap_or(0),
            vp_last.map(|b| b.high.to_bits()).unwrap_or(0),
            vp_last.map(|b| b.low.to_bits()).unwrap_or(0),
            bucket_count,
            value_area.to_bits(),
            foreign.len(),
        );
        if self.vp_overlay_stamp == Some(stamp) && self.vp_overlay.is_some() {
            return;
        }

        let bars_to_profile: Vec<&Bar> = match mode {
            VpMode::VisibleRange => visible_bars.iter().collect(),
            _ => filter_bars_for_mode(bars, mode, 5),
        };

        if bars_to_profile.is_empty() {
            self.vp_overlay = Some(VolumeProfileData::compute(&[], mode, value_area, bucket_count));
            self.vp_overlay_stamp = Some(stamp);
            return;
        }

        let mut owned: Vec<Bar> = bars_to_profile.into_iter().copied().collect();
        if !foreign.is_empty() {
            // Only the foreign bars inside the window this mode selected —
            // the caller filtered by the VISIBLE range, which is wider than
            // a Session or Composite window.
            let t0 = owned.first().map(|b| b.ts_ms()).unwrap_or(i64::MIN);
            let t1 = owned.last().map(|b| b.ts_ms()).unwrap_or(i64::MAX);
            owned.extend(foreign.iter().filter(|b| {
                let ts = b.ts_ms();
                ts >= t0 && ts <= t1
            }));
        }
        self.vp_overlay = Some(VolumeProfileData::compute(&owned, mode, value_area, bucket_count));
        self.vp_overlay_stamp = Some(stamp);
    }

    /// Recompute the TPO Profile overlay (item D) if the visible bar window
    /// changed since the last build. Mirrors [`Self::ensure_vp_from_bars`],
    /// one layer simpler — TPO Profile has no mode selector, it always
    /// aggregates the VISIBLE range (owner spec: "computes a time-at-price
    /// profile over the VISIBLE bar range from render_state.bars").
    ///
    /// `visible_bars` is the pre-filtered slice already clipped to the
    /// viewport's visible range (caller computes it via
    /// `viewport.visible_range()`, same as `ensure_vp_from_bars`'s caller).
    /// `min_tick` floors the row step (see [`TpoProfileData::compute`]'s
    /// doc comment) — pass the instrument's price precision.
    pub fn ensure_tpo_profile_from_bars(
        &mut self,
        visible_bars: &[Bar],
        min_tick: f64,
        target_rows: f64,
        value_area: f64,
        foreign: &[Bar],
    ) {
        use mlc_core::chart::tpo_profile::TpoProfileData;

        let last = visible_bars.last();
        // Shape params in the stamp — see `ensure_vp_from_bars`.
        let stamp = (
            visible_bars.len(),
            self.viewport.view_start_idx(),
            last.map(|b| b.close.to_bits()).unwrap_or(0),
            last.map(|b| b.high.to_bits()).unwrap_or(0),
            last.map(|b| b.low.to_bits()).unwrap_or(0),
            target_rows.to_bits(),
            value_area.to_bits(),
            foreign.len(),
        );
        if self.tpo_profile_overlay_stamp == Some(stamp) && self.tpo_profile_overlay.is_some() {
            return;
        }

        let merged: Vec<Bar>;
        let profile_bars: &[Bar] = if foreign.is_empty() {
            visible_bars
        } else {
            merged = visible_bars.iter().chain(foreign.iter()).copied().collect();
            &merged
        };
        self.tpo_profile_overlay =
            Some(TpoProfileData::compute(profile_bars, value_area, min_tick, target_rows));
        self.tpo_profile_overlay_stamp = Some(stamp);
    }

    /// Create a minimal clone of this chart window for use in `panel_grid::split_active`.
    ///
    /// Unlike `clone_for_split`, this variant does **not** take a source
    /// `ChartPanelState`.  It creates a fresh `ChartContainer` shell whose
    /// `current_tag_id` is set by the caller — under the per-bubble
    /// drawing model (Step 3.1) no separate `DrawingManager` seeding is
    /// needed: the new leaf either shares the source bubble (peer window)
    /// or is bound to a freshly created bubble, and `DrawingManager`
    /// sessions in `BackendCore::drawing` are created lazily per `BubbleId`
    /// on first access. The caller (chart-app `sync_group.rs`) is only
    /// responsible for seeding the real `ChartPanelState` into `panels_store`
    /// after the split returns the new leaf ids.
    ///
    /// This overload exists so that `ChartPanelGrid::split_active` (inside the
    /// `zengeld-chart` crate) can call it without needing to know about
    /// `panels_store` — it just creates the container shell; the panel state is
    /// seeded by chart-app after the split.
    pub fn clone_for_split_shell(
        &self,
        new_chart_id: ChartId,
    ) -> Self {
        let new_panel_id = generate_chart_panel_id();

        Self {
            id: new_chart_id,
            chart_panel_id: new_panel_id,
            // Sentinel — do_split must detect BubbleId(0) and create a fresh
            // auto-bubble for every chart that clone_for_split_shell produces.
            auto_tag_id: BubbleId(0),
            current_tag_id: BubbleId(0),

            // Phase X0 / C5c: per-Kind series live on the bubble.
            ha_cache: None,
            ha_cache_stamp: None,
            vp_overlay: None,
            vp_overlay_stamp: None,
            vp_mode: self.vp_mode,
            tpo_profile_overlay: None,
            tpo_profile_overlay_stamp: None,
            viewport: self.viewport.clone(),
            price_scale: self.price_scale.clone(),
            time_scale: self.time_scale.clone(),

            sub_pane_y_state: HashMap::new(),
            sub_pane_overlay_states: Vec::new(),
            sub_pane_overlay_results: Vec::new(),
            main_pane_overlay_state: SubPaneOverlayState::default(),
            main_pane_overlay_result: None,

            crosshair: {
                let mut ch = self.crosshair;
                ch.visible = false;
                ch
            },
            follow_remote_view: false,
            kinetic: KineticState::new(),
            drag_mode: DragMode::None,
            last_mouse_pos: None,
            drag_start_x: 0.0,
            drag_start_y: 0.0,
            drag_start_view: 0.0,
            drag_start_spacing: 0.0,
            drag_start_price_min: 0.0,
            drag_start_price_max: 0.0,

            price_lines: self.price_lines.clone(),

            compare_overlay: CompareOverlay::new(),
            command_history: CommandHistory::new(250),
            data_provider: self.data_provider.clone(),
            scale_settings: self.scale_settings.clone(),
            chart_toolbar: ChartToolbarState::default(),
            toolbar_config: ToolbarConfig::minimal(),

            symbol_drawings: HashMap::new(),
            // Sibling born from a split starts with an empty bubble (until
            // `do_split` seeds it).  Flag the cold-load path so the first
            // BarsLoaded replaces the empty bars wholesale + snaps to the
            // right edge instead of running the backfill anchor logic, which
            // requires non-empty prev_bars to make sense.  Without this the
            // sibling's reconcile branch goes through `reconcile_view_keep_anchor`
            // with an empty anchor and the viewport sits at view_start=0 /
            // bar_count=0, painting a blank chart even after the bubble
            // received the REST sweep.
            pending_symbol_load: true,
            needs_auto_scale_after_bars: false,
            restore_scale_mode: None,
            stashed_scale_mode: None,
            tag_inherits_home_overlay: false,
            // The fresh-snap contract is carried ENTIRELY by
            // `pending_symbol_load: true` above — the BarsLoaded reconcile
            // path (`reconcile_view_fresh`) snaps to the right edge on data
            // arrival, same contract as `seed_default_chart`. Do NOT set
            // `needs_snap_after_backfill` here: the unified per-frame
            // deferred-snap path consumes that flag on whatever bars the
            // (not-yet-reseeded) tag currently resolves to — a premature
            // cold-snap off the mother's data that made the sibling diverge
            // from the mother pane (split viewport divergence, 2026-07-03).
            needs_snap_after_backfill: false,
            derived_series_stamp: None,
            needs_y_rescale: false,
            data_error: None,
            inner_docking: make_inner_docking_with_main_chart(new_panel_id),
            sub_pane_above_main: std::collections::HashSet::new(),
        }
    }

    /// Create ScaleCornerState from this window's data
    pub fn to_corner_state(&self) -> mlc_core::state::scale_corner::ScaleCornerState {
        mlc_core::state::scale_corner::ScaleCornerState {
            scale_mode: self.price_scale.scale_mode,
            mode_label: self.price_scale.mode.short_label().to_string(),
            am_hovered: false,
            mode_hovered: false,
        }
    }

    // =========================================================================
    // Crosshair Management (single source of truth)
    // =========================================================================

    /// Update crosshair from global coordinates.
    ///
    /// # Parameters
    /// - `drag_pane`: Controls coordinate-system locking during drag.
    ///   - `None` — hover mode: detect which pane the cursor is in via hit-testing.
    ///   - `Some(None)` — drag locked to main chart coordinate system.
    ///   - `Some(Some(idx))` — drag locked to sub-pane `idx` coordinate system.
    ///
    /// During drag the crosshair is locked to the originating pane so that
    /// moving the cursor outside that pane's rect does not cause it to jump
    /// to a different coordinate system.  On separators (cursor not inside any
    /// content rect) the crosshair is hidden.
    pub fn update_crosshair_from_global(
        &mut self,
        global_x: f64,
        global_y: f64,
        layout: &mlc_core::state::geometry::ExtendedFrameLayout,
        drag_pane: Option<Option<usize>>,
        bars: &[Bar],
    ) -> bool {
        let main_chart = &layout.main_chart;

        // During drag, lock to the originating pane's coordinate system.
        if let Some(locked_pane) = drag_pane {
            match locked_pane {
                Some(pane_idx) => {
                    // Locked to sub-pane.
                    if let Some(sub_pane_layout) = layout.sub_panes.get(pane_idx) {
                        let local_x = global_x - sub_pane_layout.content.x;
                        let local_y = global_y - sub_pane_layout.content.y;
                        let pane_height = sub_pane_layout.content.height;
                        let (price_min, price_max) = self.sub_pane_y_state
                            .get(&sub_pane_layout.instance_id)
                            .map(|s| (s.price_min, s.price_max))
                            .unwrap_or((0.0, 100.0));
                        self.update_crosshair_internal(
                            local_x, local_y, pane_height, price_min, price_max,
                            Some(pane_idx), main_chart.chart.width, true, bars,
                        );
                        return true;
                    }
                    // Sub-pane layout not found — hide crosshair.
                    self.crosshair.visible = false;
                    return false;
                }
                None => {
                    // Locked to main chart.
                    let local_x = global_x - main_chart.chart.x;
                    let local_y = global_y - main_chart.chart.y;
                    let pane_height = main_chart.chart.height;
                    self.update_crosshair_internal(
                        local_x, local_y, pane_height,
                        self.price_scale.price_min, self.price_scale.price_max,
                        None, main_chart.chart.width, true, bars,
                    );
                    return true;
                }
            }
        }

        // Hover mode (no drag) — purely hit-test based.

        // Check sub-panes first (they're below main chart).
        for (idx, sub_pane_layout) in layout.sub_panes.iter().enumerate() {
            if sub_pane_layout.content.contains(global_x, global_y) {
                let local_x = global_x - sub_pane_layout.content.x;
                let local_y = global_y - sub_pane_layout.content.y;
                let pane_height = sub_pane_layout.content.height;
                let (price_min, price_max) = self.sub_pane_y_state
                    .get(&sub_pane_layout.instance_id)
                    .map(|s| (s.price_min, s.price_max))
                    .unwrap_or((0.0, 100.0));
                self.update_crosshair_internal(
                    local_x, local_y, pane_height, price_min, price_max,
                    Some(idx), main_chart.chart.width, false, bars,
                );
                return true;
            }
        }

        // Check main chart area.
        if main_chart.chart.contains(global_x, global_y) {
            let local_x = global_x - main_chart.chart.x;
            let local_y = global_y - main_chart.chart.y;
            self.update_crosshair_internal(
                local_x, local_y, main_chart.chart.height,
                self.price_scale.price_min, self.price_scale.price_max,
                None, main_chart.chart.width, false, bars,
            );
            return true;
        }

        // Cursor is on a separator or outside all chart areas — hide crosshair.
        self.crosshair.visible = false;
        false
    }

    /// Internal crosshair update with all computed values.
    /// Phase X0 / C5c: caller supplies `bars` for magnet-snap OHLC lookup.
    fn update_crosshair_internal(
        &mut self,
        local_x: f64,
        local_y: f64,
        pane_height: f64,
        price_min: f64,
        price_max: f64,
        pane_index: Option<usize>,
        chart_width: f64,
        is_dragging: bool,
        bars: &[Bar],
    ) {
        let clamped_x = local_x.clamp(0.0, chart_width);

        let bar_idx = self.viewport.x_to_bar(clamped_x);
        let bar_f64 = self.viewport.x_to_bar_f64(clamped_x);

        let price_range = price_max - price_min;
        let price = if pane_height > 0.0 {
            price_max - (local_y / pane_height) * price_range
        } else {
            price_min
        };

        if self.crosshair.follow_remote {
            return;
        }
        self.crosshair.visible = true;
        self.crosshair.synced = false; // Local mouse update — not synced
        self.crosshair.pane_index = pane_index;
        self.crosshair.x = clamped_x;
        self.crosshair.y = local_y.clamp(0.0, pane_height);
        self.crosshair.bar_idx = bar_idx;
        self.crosshair.bar_f64 = bar_f64;
        self.crosshair.price = price;

        if pane_index.is_none() && !is_dragging {
            let (snapped_price, snapped_y, snapped_bar_idx) = self.calculate_magnet_snap(
                bars, bar_idx, price, pane_height, price_min, price_max,
            );
            self.crosshair.set_snapped(snapped_price, snapped_y);
            // The window magnet can land on a neighbouring bar — move the
            // crosshair's own bar position with it so the vertical line,
            // the virtual snap dot, and the OHLC tooltip all agree on the
            // SAME bar the price was snapped to (see
            // `calculate_magnet_snap`'s own doc: a snap that moves price
            // but leaves the bar behind puts the pivot at the wrong point).
            if let Some(idx) = snapped_bar_idx {
                self.crosshair.bar_idx = Some(idx);
                self.crosshair.bar_f64 = idx as f64;
            }
        } else {
            self.crosshair.snapped_y = self.crosshair.y;
            self.crosshair.snapped_price = price;
        }
    }

    /// Centralized magnet snap calculation. Phase X0 / C5c: caller supplies `bars`.
    ///
    /// Owner spec 2026-08-13 (window magnet): a swing pivot rarely lands
    /// exactly under the cursor at typical zoom levels, so the search looks
    /// at every bar's HIGH and LOW within [`MAGNET_WINDOW_BARS`] bars of the
    /// cursor, plus the cursor bar's own OPEN/CLOSE (the pre-window
    /// behaviour, kept reachable), and snaps to whichever candidate is
    /// nearest the cursor by PIXEL distance in BOTH axes — not by price
    /// alone, so the magnet behaves the same at every zoom level (the X
    /// distance is priced in bars via `self.viewport.bar_spacing`).
    ///
    /// Returns `(price, y, bar_idx)` — `bar_idx` is the bar the winning
    /// candidate belongs to, which may differ from the cursor's own
    /// `bar_idx` argument. Callers that derive a timestamp from the snap
    /// MUST use this returned index, not the cursor's own bar: otherwise
    /// the placed point's price jumps to the neighbouring pivot while its
    /// timestamp stays on the bar under the mouse, putting the pivot at the
    /// wrong (x, y) pair on the chart.
    pub fn calculate_magnet_snap(
        &self,
        bars: &[Bar],
        bar_idx: Option<usize>,
        raw_price: f64,
        pane_height: f64,
        price_min: f64,
        price_max: f64,
    ) -> (f64, f64, Option<usize>) {
        let price_range = price_max - price_min;
        let price_to_y = |p: f64| {
            if price_range > 0.0 {
                ((price_max - p) / price_range) * pane_height
            } else {
                0.0
            }
        };
        let raw_y = price_to_y(raw_price);

        if !self.crosshair.is_magnet() {
            return (raw_price, raw_y, bar_idx);
        }
        let Some(idx) = bar_idx else {
            return (raw_price, raw_y, bar_idx);
        };
        if idx >= bars.len() {
            return (raw_price, raw_y, bar_idx);
        }

        // Tolerance mirrors `Crosshair::find_nearest_ohlc`'s own per-mode
        // radius (Magnet = strong, MagnetOHLC = light), now applied to the
        // combined pixel distance below instead of Y-distance alone.
        let tolerance = match self.crosshair.mode {
            mlc_core::CrosshairMode::Magnet => (self.viewport.bar_spacing * 0.4).clamp(20.0, 80.0),
            _ => (self.viewport.bar_spacing * 0.25).clamp(10.0, 50.0),
        };

        let window_start = idx.saturating_sub(MAGNET_WINDOW_BARS);
        let window_end = (idx + MAGNET_WINDOW_BARS).min(bars.len() - 1);

        let mut best_price = raw_price;
        let mut best_y = raw_y;
        let mut best_idx = idx;
        let mut best_dist = f64::INFINITY;
        let mut consider = |candidate_idx: usize, price: f64| {
            let y = price_to_y(price);
            let x_dist = (candidate_idx as f64 - idx as f64) * self.viewport.bar_spacing;
            let dist = x_dist.hypot(y - raw_y);
            if dist < best_dist {
                best_dist = dist;
                best_price = price;
                best_y = y;
                best_idx = candidate_idx;
            }
        };

        for i in window_start..=window_end {
            consider(i, bars[i].high);
            consider(i, bars[i].low);
        }
        // Cursor bar's own body — the pre-window behaviour, kept reachable.
        consider(idx, bars[idx].open);
        consider(idx, bars[idx].close);

        if best_dist <= tolerance {
            (best_price, best_y.clamp(0.0, pane_height), Some(best_idx))
        } else {
            (raw_price, raw_y, bar_idx)
        }
    }

    /// Hide crosshair (call when mouse leaves chart area)
    pub fn hide_crosshair(&mut self) {
        self.crosshair.visible = false;
    }

    /// Set crosshair position from a bar index (for sync group propagation).
    ///
    /// Converts `bar_f64` to a pixel X coordinate using this window's viewport,
    /// so the crosshair lands at the correct horizontal position even when the
    /// peer window has a different bar spacing or scroll offset.
    ///
    /// Set crosshair from a synced bar position and price.
    /// Converts bar_f64 to X pixel via this window's viewport, and price to Y
    /// pixel via this window's price scale — so both lines render correctly.
    ///
    /// `pane_index` mirrors the source window's active pane: `None` means the
    /// main chart, `Some(n)` means sub-pane `n` (RSI, MACD, etc.).  When the
    /// peer window also has that sub-pane the Y coordinate is computed from the
    /// peer's own `price_min`/`price_max` for that pane; if the peer lacks the
    /// sub-pane the horizontal line is hidden (Y set to -1.0).
    pub fn set_crosshair_from_bar(&mut self, bar_f64: f64, price: f64, visible: bool, pane_index: Option<usize>) {
        let x = self.viewport.bar_to_x_f64(bar_f64);
        self.crosshair.bar_f64 = bar_f64;
        // Phase X0 / C5c: bounds checked against viewport.bar_count (mirror of bubble.bars.len()).
        self.crosshair.bar_idx = if bar_f64 >= 0.0 && (bar_f64 as usize) < self.viewport.bar_count {
            Some(bar_f64 as usize)
        } else {
            None
        };
        self.crosshair.x = x;
        self.crosshair.visible = visible;
        self.crosshair.synced = true;
        self.crosshair.pane_index = pane_index;

        match pane_index {
            None => {
                // Main chart — use price_scale
                let y = self.price_scale.price_to_y(price, self.viewport.chart_height);
                self.crosshair.y = y;
                self.crosshair.price = price;
                self.crosshair.snapped_y = y;
                self.crosshair.snapped_price = price;
            }
            Some(idx) => {
                // Sub-pane — look up via tree-order instance_id.
                let iid = collect_inner_instance_ids(&self.inner_docking)
                    .into_iter()
                    .nth(idx);
                if let Some(instance_id) = iid {
                    let (price_min, price_max) = self.sub_pane_y_state
                        .get(&instance_id)
                        .map(|s| (s.price_min, s.price_max))
                        .unwrap_or((0.0, 100.0));
                    // Use viewport.chart_height as a proxy — height is corrected
                    // on the next render frame once geometry is recomputed.
                    let pane_height = self.viewport.chart_height;
                    let range = price_max - price_min;
                    let y = if range > 0.0 && pane_height > 0.0 {
                        pane_height * (1.0 - (price - price_min) / range)
                    } else {
                        pane_height / 2.0
                    };
                    self.crosshair.y = y;
                    self.crosshair.price = price;
                    self.crosshair.snapped_y = y;
                    self.crosshair.snapped_price = price;
                } else {
                    // Peer doesn't have this sub-pane — show X line only,
                    // push horizontal line off-screen so it doesn't render.
                    self.crosshair.y = -1.0;
                    self.crosshair.price = price;
                    self.crosshair.snapped_y = -1.0;
                    self.crosshair.snapped_price = price;
                }
            }
        }
    }

    /// Set crosshair from a synced timestamp and price.
    ///
    /// Converts the timestamp to a fractional bar index in this window's local
    /// bar array, then delegates to [`set_crosshair_from_bar`].  This is the
    /// correct way to propagate crosshairs across windows that may have
    /// different instruments or different bar counts — the bar index in the
    /// source window is meaningless on peers, but the timestamp is universal.
    pub fn set_crosshair_from_timestamp(&mut self, timestamp: i64, price: f64, visible: bool, pane_index: Option<usize>, bars: &[Bar]) {
        let bar_f64 = Self::timestamp_to_bar_f64(bars, timestamp);
        self.set_crosshair_from_bar(bar_f64, price, visible, pane_index);
    }

    /// Convert a UTC timestamp (unix seconds) to a fractional bar index using
    /// binary search. Phase X0 / C5c: associated function taking `bars` slice;
    /// caller supplies `&bubble.bars`.
    pub fn timestamp_to_bar_f64(bars: &[Bar], timestamp: i64) -> f64 {
        if bars.is_empty() {
            return 0.0;
        }
        if bars.len() == 1 {
            return 0.0;
        }

        let first_ts = bars[0].timestamp;
        let last_ts = bars[bars.len() - 1].timestamp;

        if timestamp <= first_ts {
            let interval = bars[1].timestamp - first_ts;
            if interval > 0 {
                return (timestamp - first_ts) as f64 / interval as f64;
            }
            return 0.0;
        }

        if timestamp >= last_ts {
            let interval = last_ts - bars[bars.len() - 2].timestamp;
            if interval > 0 {
                return (bars.len() - 1) as f64
                    + (timestamp - last_ts) as f64 / interval as f64;
            }
            return (bars.len() - 1) as f64;
        }

        match bars.binary_search_by_key(&timestamp, |b| b.timestamp) {
            Ok(idx) => idx as f64,
            Err(idx) => {
                if idx > 0 && idx < bars.len() {
                    let lo_ts = bars[idx - 1].timestamp;
                    let hi_ts = bars[idx].timestamp;
                    let frac = if hi_ts > lo_ts {
                        (timestamp - lo_ts) as f64 / (hi_ts - lo_ts) as f64
                    } else {
                        0.0
                    };
                    (idx - 1) as f64 + frac
                } else {
                    idx as f64
                }
            }
        }
    }

    // =========================================================================
    // Compare Overlay Management
    // =========================================================================

    /// Add a comparison symbol to this chart window.
    ///
    /// `main_symbol` and `main_timeframe` are read from `branch_meta` by the
    /// caller (Phase 4 B — instrument fields removed from ChartContainer).
    pub fn add_compare_symbol(&mut self, symbol: &str, main_symbol: &str, main_timeframe: &Timeframe, main_bars: &[Bar]) -> bool {
        if self.compare_overlay.has_symbol(symbol) || symbol == main_symbol {
            log::debug!("[Compare] Symbol {} already compared or is main symbol", symbol);
            return false;
        }

        let Some(bars) = self.data_provider.get_bars(symbol, main_timeframe) else {
            log::debug!(
                "[Compare] Data not available for symbol={} tf={}",
                symbol,
                main_timeframe.name
            );
            return false;
        };
        if bars.is_empty() {
            log::debug!("[Compare] No bars loaded for {}", symbol);
            return false;
        }

        let color = get_compare_color(self.compare_overlay.series.len());
        let series = CompareSeries::new(symbol, bars, color);
        self.compare_overlay.add_series(series);

        if self.compare_overlay.series.len() == 1 && !main_bars.is_empty() {
            let (start, _) = self.viewport.visible_range();
            let base_idx = start.min(main_bars.len().saturating_sub(1));
            let base_bar = &main_bars[base_idx];
            self.compare_overlay.set_main_base(base_bar.close, base_bar.timestamp);
        }

        log::debug!(
            "[Compare] Added {} (color: {}, now {} series)",
            symbol,
            color,
            self.compare_overlay.series.len()
        );
        true
    }

    /// Remove a comparison symbol from this chart window.
    pub fn remove_compare_symbol(&mut self, symbol: &str) -> bool {
        if !self.compare_overlay.has_symbol(symbol) {
            return false;
        }

        self.compare_overlay.remove_series_by_symbol(symbol);
        log::debug!(
            "[Compare] Removed {} (now {} series)",
            symbol,
            self.compare_overlay.series.len()
        );
        true
    }

    /// Clear all comparison symbols from this chart window.
    pub fn clear_compare_symbols(&mut self) {
        let count = self.compare_overlay.series.len();
        self.compare_overlay.clear();
        log::debug!("[Compare] Cleared {} series", count);
    }

    // =========================================================================
    // Scale Settings Access
    // =========================================================================

    /// Get scale settings reference
    pub fn scale_settings(&self) -> &ScaleSettings {
        &self.scale_settings
    }

    /// Get mutable scale settings reference
    pub fn scale_settings_mut(&mut self) -> &mut ScaleSettings {
        &mut self.scale_settings
    }

    // =========================================================================
    // Previous Close Line
    // =========================================================================

    /// Update previous close price line based on settings.
    /// Phase X0 / C5c: caller supplies `bars` (typically `&bubble.bars`).
    pub fn update_prev_close_line(&mut self, bars: &[Bar]) {
        self.price_lines.remove("prev_close");

        if self.scale_settings.show_prev_close_line {
            if let Some(prev_close) = bars.first().map(|b| b.open) {
                let line = PriceLine {
                    id: "prev_close".to_string(),
                    price: prev_close,
                    color: self.scale_settings.prev_close_color.clone(),
                    line_width: 1,
                    line_style: mlc_core::drawing::LineStyle::Dashed,
                    line_visible: true,
                    axis_label_visible: true,
                    title: "Prev Close".to_string(),
                    axis_label_color: String::new(),
                    axis_label_text_color: String::new(),
                };
                self.price_lines.insert("prev_close".to_string(), line);
            }
        }
    }

    // =========================================================================
    // Toggle Overlays
    // =========================================================================

    /// Toggle the legend visibility.
    pub fn toggle_legend(&self, panel: &mut ChartPanelState) {
        panel.legend.visible = !panel.legend.visible;
    }

    /// Toggle both grid lines (vertical and horizontal) together.
    pub fn toggle_grid(&self, panel: &mut ChartPanelState) {
        let new_visible = !panel.grid_options.vert_lines.visible;
        panel.grid_options.vert_lines.visible = new_visible;
        panel.grid_options.horz_lines.visible = new_visible;
    }

    /// Toggle crosshair enabled state.
    pub fn toggle_crosshair(&mut self) {
        self.crosshair.enabled = !self.crosshair.enabled;
    }

    /// Toggle watermark visibility. Creates a default watermark if none exists.
    pub fn toggle_watermark(&self, panel: &mut ChartPanelState) {
        match &mut panel.watermark {
            Some(wm) => wm.visible = !wm.visible,
            None => {
                panel.watermark = Some(Watermark::default());
            }
        }
    }

    /// Toggle vertical grid lines visibility.
    pub fn toggle_grid_vertical(&self, panel: &mut ChartPanelState) {
        panel.grid_options.vert_lines.visible = !panel.grid_options.vert_lines.visible;
    }

    /// Toggle horizontal grid lines visibility.
    pub fn toggle_grid_horizontal(&self, panel: &mut ChartPanelState) {
        panel.grid_options.horz_lines.visible = !panel.grid_options.horz_lines.visible;
    }

    /// Toggle tooltip visibility.
    pub fn toggle_tooltip(&self, panel: &mut ChartPanelState) {
        panel.tooltip.visible = !panel.tooltip.visible;
    }

    /// Toggle tooltip follow-cursor mode.
    pub fn toggle_tooltip_follow(&self, panel: &mut ChartPanelState) {
        panel.tooltip.follow_cursor = !panel.tooltip.follow_cursor;
    }

    /// Toggle crosshair vertical line visibility.
    pub fn toggle_crosshair_vert_line(&self, panel: &mut ChartPanelState) {
        panel.crosshair_options.vert_line.visible = !panel.crosshair_options.vert_line.visible;
    }

    /// Toggle crosshair horizontal line visibility.
    pub fn toggle_crosshair_horz_line(&self, panel: &mut ChartPanelState) {
        panel.crosshair_options.horz_line.visible = !panel.crosshair_options.horz_line.visible;
    }

    /// Toggle legend OHLC display.
    pub fn toggle_legend_ohlc(&self, panel: &mut ChartPanelState) {
        panel.legend.show_ohlc = !panel.legend.show_ohlc;
    }

    /// Toggle legend change display.
    pub fn toggle_legend_change(&self, panel: &mut ChartPanelState) {
        panel.legend.show_change = !panel.legend.show_change;
    }

    /// Toggle legend percent display.
    pub fn toggle_legend_percent(&self, panel: &mut ChartPanelState) {
        panel.legend.show_percent = !panel.legend.show_percent;
    }

    // =========================================================================
    // Setters
    // =========================================================================

    /// Set grid line style for both vertical and horizontal lines.
    pub fn set_grid_style(&self, panel: &mut ChartPanelState, style: mlc_core::drawing::LineStyle) {
        panel.grid_options.vert_lines.style = style;
        panel.grid_options.horz_lines.style = style;
    }

    /// Set crosshair mode on both crosshair state and crosshair options.
    pub fn set_crosshair_mode(&mut self, panel: &mut ChartPanelState, mode: mlc_core::CrosshairMode) {
        self.crosshair.mode = mode;
        panel.crosshair_options.mode = mode;
    }

    /// Set crosshair line style for both vertical and horizontal lines.
    pub fn set_crosshair_style(&self, panel: &mut ChartPanelState, style: mlc_core::drawing::LineStyle) {
        panel.crosshair_options.vert_line.style = style;
        panel.crosshair_options.horz_line.style = style;
    }

    /// Set legend position.
    pub fn set_legend_position(&self, panel: &mut ChartPanelState, pos: mlc_core::LegendPosition) {
        panel.legend.position = pos;
    }

    /// Set watermark position. Creates a default watermark if none exists.
    pub fn set_watermark_position(&self, panel: &mut ChartPanelState, horz: mlc_core::HorzAlign, vert: mlc_core::VertAlign) {
        match &mut panel.watermark {
            Some(wm) => {
                wm.horz_align = horz;
                wm.vert_align = vert;
            }
            None => {
                panel.watermark = Some(Watermark {
                    horz_align: horz,
                    vert_align: vert,
                    ..Watermark::default()
                });
            }
        }
    }

    /// Set watermark color. Creates a default watermark if none exists.
    pub fn set_watermark_color(&self, panel: &mut ChartPanelState, color: &str) {
        match &mut panel.watermark {
            Some(wm) => wm.set_color(color),
            None => {
                let mut wm = Watermark::default();
                wm.set_color(color);
                panel.watermark = Some(wm);
            }
        }
    }

    /// Set watermark text. Creates a default watermark if none exists.
    pub fn set_watermark_text(&self, panel: &mut ChartPanelState, text: &str) {
        match &mut panel.watermark {
            Some(wm) => wm.set_text(text),
            None => {
                let mut wm = Watermark::default();
                wm.set_text(text);
                panel.watermark = Some(wm);
            }
        }
    }

    // =========================================================================
    // Zoom
    // =========================================================================

    /// Snap viewport to the most recent bar with `margin` bars of empty right space.
    ///
    /// `margin` is a count of empty bars shown to the right of the last candle.
    /// Use [`DEFAULT_SNAP_MARGIN`] for the standard value.
    ///
    /// Formula: `view_start = count + effective_margin - visible_f`.
    ///
    /// Preconditions: `self.viewport.chart_width > 0.0` and
    /// `self.viewport.bar_spacing > 0.0` must both hold, or the result is
    /// meaningless (those are the same preconditions as [`Viewport::visible_bars`]).
    /// Phase X0 / C5c: uses `viewport.bar_count` which is maintained by
    /// `reconcile_view_to_bars` whenever bubble.bars length changes — no
    /// direct reference to a bars slice needed here.
    ///
    /// No lower clamp on `view_start` (fixed 2026-07-11 — was `.max(0.0)`):
    /// when the series has FEWER bars than fit on screen (e.g. a 1-session
    /// TPO chart at its wide `min_bar_spacing`, or any series right after a
    /// fresh chart-type switch), the right-anchored formula is legitimately
    /// negative — that's what places the sparse content flush against the
    /// right edge with empty space to its left, matching every other
    /// series' "tail at right, margin beyond it" convention instead of
    /// pinning it to the left edge. `Viewport::view_start_idx()` already
    /// clamps negative `view_start` to index 0 for iteration purposes, so
    /// nothing downstream misbehaves on a negative value.
    pub fn snap_to_end(&mut self, margin: f64) {
        let (vs0, bc0) = (self.viewport.view_start, self.viewport.bar_count);
        let visible_f = self.viewport.chart_width / self.viewport.bar_spacing;
        let count = self.viewport.bar_count;
        // Progressive right-edge margin: absolute-bar `margin` values only
        // make sense on wide viewports — a fixed 5-bar margin means half an
        // empty screen when the user zooms to 10 visible bars. Restore the
        // proportional ladder from v0.5.73 (commit 0cee2596, killed by the
        // v0.5.216 unify commit c860233e): margin scales with visible bar
        // count, capped by the caller's request. Callers pass
        // `DEFAULT_SNAP_MARGIN` (5.0) as the desired MAXIMUM; the actual
        // margin is `min(DEFAULT_SNAP_MARGIN, ceil(5% * visible_f))` with a
        // floor of 0 so the last bar stays on-screen at any zoom.
        let progressive = (0.05 * visible_f).ceil().max(0.0);
        let effective_margin = margin.min(progressive);
        self.viewport.view_start = count as f64 + effective_margin - visible_f;
        self.log_view_jump("snap_to_end", vs0, bc0);
    }

    // =========================================================================
    // Split-aware scroll helpers (called when routing scroll to sub-charts)
    // =========================================================================

    /// Zoom the viewport horizontally around `center_x` (local pixel coord).
    ///
    /// `factor` > 1.0 zooms in (more bars per pixel), < 1.0 zooms out.
    /// Delegates to [`Viewport::zoom_at`] which handles clamping.
    /// `series` is the ActiveSeries the window's chart_type renders (audit D8).
    pub fn zoom_horizontal(&mut self, center_x: f64, factor: f64, series: crate::active_series::ActiveSeries<'_>) {
        if self.follow_remote_view {
            return;
        }
        self.viewport.zoom_at(center_x, factor);
        if self.price_scale.scale_mode.is_auto_y() {
            self.calc_auto_scale(series);
        }
    }

    /// [`Self::zoom_horizontal`] with a chart-type fit cap (registry
    /// `viewport_fit_cap`, owner spec 2026-07-13): `fit_cap_bars` bounds
    /// how many bars may EVER be visible, so the zoom-out floor is
    /// `chart_width / cap` (never below the global pixel clamp). `None`
    /// behaves exactly like [`Self::zoom_horizontal`].
    pub fn zoom_horizontal_capped(&mut self, center_x: f64, factor: f64, fit_cap_bars: Option<u32>, series: crate::active_series::ActiveSeries<'_>) {
        if self.follow_remote_view {
            return;
        }
        let mut min = self.viewport.min_bar_spacing();
        if let Some(cap) = fit_cap_bars {
            if cap > 0 && self.viewport.chart_width > 0.0 {
                min = min.max(self.viewport.chart_width / cap as f64);
            }
        }
        let max = self.viewport.max_bar_spacing();
        self.viewport.zoom_at_x(center_x, factor, min, max);
        if self.price_scale.scale_mode.is_auto_y() {
            self.calc_auto_scale(series);
        }
    }

    // =========================================================================
    // Viewport Intents — single-writer entry points for every remaining
    // W-site from `docs/mlc/audits/viewport-audit-2026-07-03.md`. Callers
    // outside this file never write `self.viewport` / `self.price_scale`
    // fields directly; they call one of these.
    // =========================================================================

    /// Reset viewport + pending-load bookkeeping for a symbol/timeframe/
    /// chart-type switch: `bar_count = 0`, `view_start = 0.0`,
    /// `pending_symbol_load = true`.
    ///
    /// Replaces the copy-pasted 3-line reset idiom that used to live at 7+
    /// call sites (W25-W31). Fixes the W29 inconsistency (one sibling site
    /// forgot to reset `view_start`) by construction — every caller now
    /// goes through the same body.
    pub fn begin_fresh_load(&mut self) {
        self.viewport.bar_count = 0;
        self.viewport.view_start = 0.0;
        self.pending_symbol_load = true;
        self.derived_series_stamp = None;
    }

    /// Same reset as [`begin_fresh_load`] but does NOT set
    /// `pending_symbol_load`.
    ///
    /// Used exactly once (peer timeframe propagation in
    /// `input::sync_group::propagate_timeframe_to_sync_group`, W32) where
    /// the peer's `pending_symbol_load` is deliberately left alone — the
    /// caller relies on the subsequent `BarsLoaded` matching by instrument
    /// only, not by the `pending_symbol_load` fresh-vs-backfill branch.
    pub fn begin_fresh_load_keep_pending(&mut self) {
        self.viewport.bar_count = 0;
        self.viewport.view_start = 0.0;
        self.derived_series_stamp = None;
    }

    /// Seed a fresh-load reconcile: force `ScaleMode::Auto` (the global
    /// scale_mode persistence channel was deleted in the viewport SWC arc —
    /// every fresh load starts Auto; `restore_scale_mode` from a cold
    /// preset-load path overrides this inside `reconcile_view_fresh` when
    /// present), then run the fresh-load viewport reconcile.
    pub fn seed_fresh_load(&mut self, bars: &[Bar], series: crate::active_series::ActiveSeries<'_>) {
        self.price_scale.scale_mode = ScaleMode::Auto;
        self.reconcile_view_fresh(bars, series);
    }

    /// Cycle the A/M/F scale mode to `next` and apply its immediate
    /// side-effects: follow-mode snaps to the right edge, auto-mode
    /// recalculates the visible price range, and every sub-pane's
    /// `auto_scale` flag follows the new mode. Returns `next` so the
    /// caller can propagate it to a sync group / bubble.
    pub fn cycle_scale_mode(&mut self, next: ScaleMode, series: crate::active_series::ActiveSeries<'_>) -> ScaleMode {
        self.price_scale.scale_mode = next;
        if next.is_follow() {
            self.snap_to_end(DEFAULT_SNAP_MARGIN);
        }
        if next.is_auto_y() {
            self.calc_auto_scale(series);
        }
        let is_auto = next.is_auto_y();
        for state in self.sub_pane_y_state.values_mut() {
            state.auto_scale = is_auto;
        }
        next
    }

    /// Set the scale mode directly (no cycling), without touching
    /// sub-pane auto-scale flags. Used by reset buttons / template apply
    /// / desync-restore sites that already know the target mode.
    pub fn set_scale_mode(&mut self, mode: ScaleMode) {
        self.price_scale.scale_mode = mode;
    }

    /// Set the scale mode to `mode` and mark every sub-pane's Y axis
    /// auto-scale flag to match (`is_auto_y()`), optionally recalculating
    /// the main chart's auto-scale range from `bars` when the new mode is
    /// Auto. Used by reset-to-auto flows that also need to normalize
    /// sub-panes (double-click sub-pane reset, ResetPriceScale sub-pane
    /// branch, desync restore).
    pub fn set_scale_mode_with_recalc(&mut self, mode: ScaleMode, series: crate::active_series::ActiveSeries<'_>) {
        self.price_scale.scale_mode = mode;
        let is_auto = mode.is_auto_y();
        for state in self.sub_pane_y_state.values_mut() {
            state.auto_scale = is_auto;
        }
        if is_auto {
            self.calc_auto_scale(series);
        }
    }

    /// Set the price-scale user precision override (dropdown-cycle in
    /// chart settings). Distinct from `scale_settings.user_precision`
    /// (the source of truth mirrored into `price_scale` for rendering).
    pub fn set_price_precision(&mut self, precision: Option<usize>) {
        self.price_scale.user_precision = precision;
    }

    /// Toggle the price-scale display mode (Normal / Percent / Logarithmic
    /// — the corner "Mode" button), independent of the A/M/F scale_mode.
    pub fn toggle_price_scale_mode(&mut self) {
        self.price_scale.toggle_mode();
    }

    /// Reset the X axis (time/bar scale) to the default zoom level and
    /// scroll to end — the "reset zoom" toolbar button.
    pub fn reset_time_scale(&mut self) {
        self.viewport.reset_to_default();
    }

    /// Directly set the price range (manual price-scale drag) and switch
    /// to Manual mode.
    pub fn set_price_range(&mut self, price_min: f64, price_max: f64) {
        if self.follow_remote_view {
            return;
        }
        self.price_scale.scale_mode = ScaleMode::Manual;
        self.price_scale.price_min = price_min;
        self.price_scale.price_max = price_max;
    }

    /// Pan the viewport's X axis by a raw pixel delta, with no price-scale
    /// side effect (the caller handles Y separately — sub-pane drag vs.
    /// main-chart auto/manual price adjust are two different follow-ups
    /// depending on drag mode).
    pub fn pan(&mut self, pixel_delta: f64) {
        if self.follow_remote_view {
            return;
        }
        self.viewport.pan(pixel_delta);
    }

    /// Center the viewport's X axis on `bar_idx`, keeping the current
    /// scale mode. Recalculates auto-scale if already in an auto-Y mode.
    /// Used by signal-row clicks (W11).
    pub fn center_on_bar(&mut self, bar_idx: f64, series: crate::active_series::ActiveSeries<'_>) {
        let visible_bars = self.viewport.chart_width / self.viewport.bar_spacing;
        self.viewport.view_start = (bar_idx - visible_bars / 2.0).max(0.0);
        if self.price_scale.scale_mode.is_auto_y() {
            self.calc_auto_scale(series);
        }
    }

    /// Center the viewport's X axis on `bar_idx` and force Auto-Y on both
    /// the main chart and every sub-pane. Used by drawing-row clicks in
    /// the Object Tree (W10), where centering on a primitive should also
    /// re-fit the Y axis regardless of the previous scale mode.
    pub fn center_on_bar_auto(&mut self, bar_idx: f64, series: crate::active_series::ActiveSeries<'_>) {
        let visible_bars = self.viewport.chart_width / self.viewport.bar_spacing;
        self.viewport.view_start = (bar_idx - visible_bars / 2.0).max(0.0);
        self.price_scale.scale_mode = ScaleMode::Auto;
        for state in self.sub_pane_y_state.values_mut() {
            state.auto_scale = true;
        }
        self.calc_auto_scale(series);
    }

    /// Apply a sync-group propagation write from the source window.
    ///
    /// `peer_view_start` / `peer_spacing` are already resolved into this
    /// window's own coordinate space by the caller (ts-anchored bar index,
    /// chart-width-adjusted spacing). Only writes + rescales when the
    /// viewport actually moves (matches source `>1e-9` epsilon) — an
    /// unconditional per-frame write raced the per-tick autoscale on the
    /// same peer and caused visible line/scale-chip flicker. A bare mode
    /// flip with no movement still triggers exactly one rescale.
    /// Returns `true` if the viewport moved.
    pub fn set_from_sync(
        &mut self,
        peer_view_start: f64,
        peer_spacing: f64,
        scale_mode: Option<ScaleMode>,
        series: crate::active_series::ActiveSeries<'_>,
    ) -> bool {
        if let Some(mode) = scale_mode {
            self.price_scale.scale_mode = mode;
            let is_auto = mode.is_auto_y();
            for state in self.sub_pane_y_state.values_mut() {
                state.auto_scale = is_auto;
            }
        }
        let moved = (self.viewport.view_start - peer_view_start).abs() > 1e-9
            || (self.viewport.bar_spacing - peer_spacing).abs() > 1e-9;
        if moved {
            let (vs0, bc0) = (self.viewport.view_start, self.viewport.bar_count);
            self.viewport.view_start = peer_view_start;
            self.viewport.bar_spacing = peer_spacing;
            if self.price_scale.scale_mode.is_auto_y() {
                self.calc_auto_scale(series);
            }
            self.log_view_jump("set_from_sync", vs0, bc0);
        } else if scale_mode.is_some() && self.price_scale.scale_mode.is_auto_y() {
            self.calc_auto_scale(series);
        }
        moved
    }

    /// Apply an `AgentCommand::SetViewport` command from the HTTP/agent
    /// control surface.
    ///
    /// `mode` mirrors the corner-button semantics: `"focus"` scrolls to
    /// the right edge at the current spacing, `"fit"` fits all `bar_count`
    /// bars into the current chart width. Explicit `view_start`/
    /// `bar_spacing` overrides (if present) apply after the mode.
    pub fn set_from_agent(
        &mut self,
        mode: Option<&str>,
        view_start: Option<f64>,
        bar_spacing: Option<f64>,
        bar_count: usize,
    ) {
        let (vs0, bc0) = (self.viewport.view_start, self.viewport.bar_count);
        match mode {
            Some("focus") => {
                if bar_count > 0 {
                    let visible = if self.viewport.bar_spacing > 0.0 {
                        (self.viewport.chart_width / self.viewport.bar_spacing).ceil() as usize
                    } else {
                        1
                    };
                    self.viewport.view_start = (bar_count as f64) - (visible as f64);
                }
            }
            Some("fit") => {
                if bar_count > 0 && self.viewport.chart_width > 0.0 {
                    self.viewport.bar_spacing = self.viewport.chart_width / bar_count as f64;
                    self.viewport.view_start = 0.0;
                }
            }
            _ => {}
        }
        if let Some(vs) = view_start {
            self.viewport.view_start = vs;
        }
        if let Some(bs) = bar_spacing {
            self.viewport.bar_spacing = bs;
        }
        self.log_view_jump("set_from_agent", vs0, bc0);
    }

    /// Resize the viewport's chart geometry, pinning the right edge (the
    /// bars currently visible at the right stay anchored) when the window
    /// hasn't already deferred its initial snap.
    ///
    /// Used by both the non-split (`ChartApp::sync_viewport_from_layout`)
    /// and split-per-leaf (`prepare_frame`) resize paths — same body,
    /// different callers.
    pub fn resize_geometry(&mut self, new_width: f64, new_height: f64) {
        let (vs0, bc0) = (self.viewport.view_start, self.viewport.bar_count);
        let old_width = self.viewport.chart_width;
        // A window with a pending deferred/cold snap has placeholder view
        // state — bar-shifting it applies a spurious offset the upcoming
        // snap then has to fight. Either pending flag suppresses the shift;
        // dimensions below are always adopted regardless.
        if !self.needs_auto_scale_after_bars
            && !self.needs_snap_after_backfill
            && (old_width - new_width).abs() > 0.5
            && self.viewport.bar_spacing > 0.0
            && old_width > 0.0
            && new_width > 0.0
        {
            let bar_shift = (old_width - new_width) / self.viewport.bar_spacing;
            self.viewport.view_start += bar_shift;
        }
        // Don't store negative/zero widths — they'd poison the next bar_shift calc.
        if new_width > 0.0 {
            if (old_width - new_width).abs() > 0.5 {
                // Visible window changed → Auto-mode Y must be refit to it
                // (split halves the pane, resize stretches it; inheriting a
                // Y computed for the OLD width is the split-Y staleness bug,
                // 2026-07-04). Consumed by consume_y_rescale in prepare.
                self.needs_y_rescale = true;
            }
            self.viewport.chart_width = new_width;
        }
        if new_height > 0.0 {
            self.viewport.chart_height = new_height;
        }
        // Degenerate-width heal (a1089a85): a snap that ran against a
        // transient sliver width (observed 47px during wasm tab-return)
        // could pin view_start ≈ bar_count with NOTHING on screen. The
        // original clamp healed that but over-reached — it pinned the view
        // to the data's right edge on every geometry pass, which also
        // killed LEGIT user overscroll into empty future space (owner
        // report 2026-07-05: "can't scroll the chart into the future").
        // TV semantics: scrolling right is free as long as SOME data stays
        // visible — so the invalid state is only "zero data bars on
        // screen", not "past the snap margin". Skip entirely while the
        // series is transiently empty; nothing to clamp against anyway.
        if self.viewport.bar_count > 0
            && self.viewport.bar_spacing > 0.0
            && self.viewport.chart_width > 0.0
        {
            // Owner 2026-07-06: 3-bar minimum was too aggressive — at
            // deep zoom (2 visible bars) the clamp blocked Manual-mode
            // future-scroll while ≥3 bars were still off-screen to the
            // left. The original intent of the clamp was «keep the last
            // bar visible»; one bar is sufficient for that.
            const MIN_VISIBLE_DATA_BARS: f64 = 1.0;
            let max_vs = (self.viewport.bar_count as f64 - MIN_VISIBLE_DATA_BARS).max(0.0);
            if self.viewport.view_start > max_vs {
                self.viewport.view_start = max_vs;
            }
        }
        self.log_view_jump("resize_geometry", vs0, bc0);
    }

    /// Debug trap (viewport-SWC arc): every big-jump-capable intent reports
    /// itself when it moves view_start or bar_count by more than 100 bars —
    /// the single-door design makes the culprit of any runaway/teleport
    /// identifiable by name in the log/console. Cheap (two compares) — kept
    /// in release until the wasm tab-return teleport is closed.
    fn log_view_jump(&self, tag: &str, vs0: f64, bc0: usize) {
        let vs1 = self.viewport.view_start;
        let bc1 = self.viewport.bar_count;
        if (vs1 - vs0).abs() > 100.0 || bc1.abs_diff(bc0) > 100 {
            log::warn!(
                "[VPJUMP] {tag}: panel={} view_start {vs0:.1}->{vs1:.1} bar_count {bc0}->{bc1} cw={:.0} bs={:.2}",
                self.chart_panel_id.0, self.viewport.chart_width, self.viewport.bar_spacing
            );
        }
    }

    /// Consume the deferred viewport snap set by an async bars delivery
    /// that landed before layout dimensions were known
    /// (`needs_auto_scale_after_bars` / `needs_snap_after_backfill`).
    ///
    /// Audit D8: `series` is the ActiveSeries this window's chart_type
    /// actually renders (resolved by the caller once per frame in
    /// `prepare_frame` via `resolve_active_series`) — the emptiness gate
    /// and the Y autoscale below must key off the rendered array, not
    /// always `bubble.bars`.
    ///
    /// Returns `Some((view_start, bar_spacing))` when a snap fired (so the
    /// caller can propagate the new position to sync-group peers);
    /// `None` when the gate didn't pass (chart_width still 0, series empty,
    /// or neither flag set).
    pub fn consume_deferred_snap(&mut self, series: crate::active_series::ActiveSeries<'_>) -> Option<(f64, f64)> {
        if !(self.needs_auto_scale_after_bars || self.needs_snap_after_backfill)
            || series.is_empty()
            || self.viewport.chart_width <= 0.0
        {
            return None;
        }
        self.needs_auto_scale_after_bars = false;
        if self.needs_snap_after_backfill {
            // Cold-restore snap supersedes the plain deferred snap:
            // consume_cold_snap snaps + force-autoscales + clears the flag.
            self.consume_cold_snap(series);
        } else {
            self.snap_to_end(DEFAULT_SNAP_MARGIN);
            self.calc_auto_scale(series);
        }
        // restore_scale_mode is already consumed inside set_bars() for the
        // eager path; for the deferred path (chart_width was 0 at set_bars
        // time), consume it here.
        if let Some(mode) = self.restore_scale_mode.take() {
            self.price_scale.scale_mode = mode;
        }
        Some((self.viewport.view_start, self.viewport.bar_spacing))
    }

    /// Consume the one-shot Y-refit flag set by `resize_geometry` on a
    /// material width change (split, window resize). Auto-mode Y is refit
    /// to the CURRENT visible window; Manual/Focus windows just clear the
    /// flag (the user owns their Y range). X is never touched here.
    pub fn consume_y_rescale(&mut self, series: crate::active_series::ActiveSeries<'_>) {
        if !self.needs_y_rescale {
            return;
        }
        if series.is_empty() || self.viewport.chart_width <= 0.0 {
            // Keep the flag armed until real data + dimensions exist.
            return;
        }
        self.needs_y_rescale = false;
        if self.price_scale.scale_mode.is_auto_y() {
            self.calc_auto_scale(series);
        }
    }

    /// Restore persisted price-scale DISPLAY config from a preset
    /// snapshot. The viewport is fully RAM-only (owner doctrine
    /// 2026-07-12): nothing of it — `bar_spacing` included — survives a
    /// reload; the cold snap ([`Self::consume_cold_snap`]) seeds
    /// `fit_default_view` (100 bars, right-snapped) instead.
    /// `scale_mode`/`price_min`/`price_max` were already never restored
    /// raw (viewport SWC arc) — auto-scale recomputes them.
    pub fn restore_persisted_display(
        &mut self,
        price_persist: &mlc_core::preset::snapshots::PriceScalePersist,
    ) {
        self.price_scale.mode = price_persist.mode;
        self.price_scale.base_price = price_persist.base_price;
        self.price_scale.width = price_persist.width;
        self.price_scale.user_precision = price_persist.user_precision;
    }

    /// Set the viewport's bar count to `new_count` (bookkeeping only, no
    /// rescale). Used by derived-bar-append reactors (W23) and live-tick
    /// passthrough reactors, which call [`ChartContainer::calc_auto_scale`]
    /// /[`ChartContainer::advance_view_on_tick`] themselves afterward.
    pub fn set_bar_count(&mut self, new_count: usize) {
        self.viewport.bar_count = new_count;
    }

    /// Bump `bar_count` by one when `is_new_bar`, then apply the two
    /// live-tick follow behaviors: Follow mode snaps to the right edge
    /// every tick; Auto mode advances `view_start` by one bar only when a
    /// new bar was appended AND the last bar was already at (or past) the
    /// right edge — so the view tracks new bars without visibly jumping
    /// mid-bar.
    pub fn advance_view_on_tick(&mut self, is_new_bar: bool) {
        self.viewport.bar_count = self.viewport.bar_count.saturating_add(if is_new_bar { 1 } else { 0 });
        let count = self.viewport.bar_count;
        let visible_f = self.viewport.chart_width / self.viewport.bar_spacing;
        if self.price_scale.scale_mode.is_follow() {
            self.snap_to_end(DEFAULT_SNAP_MARGIN);
        }
        if is_new_bar && self.price_scale.scale_mode == ScaleMode::Auto {
            let right_edge_bar = self.viewport.view_start + visible_f;
            let last_bar = count as f64;
            if last_bar >= right_edge_bar {
                self.viewport.view_start += 1.0;
            }
        }
        // Beyond-end anomaly trap: view positioned way past the data edge.
        if is_new_bar && self.viewport.view_start > self.viewport.bar_count as f64 + 60.0 {
            log::warn!(
                "[VPJUMP] advance_view_on_tick BEYOND-END: view_start={:.1} bar_count={} cw={:.0} bs={:.2}",
                self.viewport.view_start, self.viewport.bar_count,
                self.viewport.chart_width, self.viewport.bar_spacing
            );
        }
    }

    /// Reconcile viewport after a derived-series upsert (the six
    /// `*Appended`/`*Updated` reactor events: footprint/renko/pnf/kagi/
    /// tpo/tlb). Owner doctrine 2026-07-05: data arrivals NEVER move what
    /// the user is looking at — the viewport is positioned once per load
    /// (cold snap on the first NON-EMPTY paint), after that only the user
    /// (pan/zoom) and the scale mode may move it. Mechanically:
    ///
    /// - the one-shot `needs_snap_after_backfill` stays ARMED until the
    ///   series actually has data — burning it against the first bar of a
    ///   warm replay is how the viewport got stranded at the oldest edge
    ///   (repro 2026-07-05: snap consumed at len==1, the remaining 381
    ///   warm bars then appended to the right of a view stuck near 0);
    /// - a prepend (lazy-deepen / scroll-left reseed merging older bars in
    ///   front) shifts every existing index right by the prepend count →
    ///   `view_start` gets the same shift so the anchored bar stays put;
    /// - a tail append leaves `view_start` alone UNLESS the view was glued
    ///   to the right edge before the append (kline
    ///   [`Self::advance_view_on_tick`] Auto-follow parity) or the scale
    ///   mode is Follow.
    pub fn reconcile_derived_series(&mut self, series: crate::active_series::ActiveSeries<'_>) {
        let new_len = series.len();
        if self.needs_snap_after_backfill {
            if new_len > 0 && self.viewport.chart_width > 0.0 && self.viewport.bar_spacing > 0.0 {
                self.viewport.bar_count = new_len;
                self.consume_cold_snap(series);
                self.derived_series_stamp = Some((
                    series.ts_ms_at(0).unwrap_or(0),
                    series.ts_ms_at(new_len - 1).unwrap_or(0),
                ));
            }
            // No data yet or chart not sized — keep the flag armed for the
            // event that actually delivers bars; nothing to book-keep.
            return;
        }
        if new_len == 0 {
            self.viewport.bar_count = 0;
            self.derived_series_stamp = None;
            return;
        }
        let new_head = series.ts_ms_at(0).unwrap_or(0);
        let new_last = series.ts_ms_at(new_len - 1).unwrap_or(0);
        let (vs0, bc0) = (self.viewport.view_start, self.viewport.bar_count);
        match self.derived_series_stamp {
            Some((old_head, old_last)) => {
                // F3 (viewport doctrine audit 2026-07-07): the prepend
                // binary-search below assumes strictly monotonic replay
                // (either a clean prepend or a clean tail append). Guard the
                // two ways that assumption breaks: unsorted endpoints, or a
                // wholesale replacement (shrunk on BOTH ends) that isn't an
                // append/prepend at all. Adopt the new count as-is — no
                // shift math, no glue-snap — and log instead of trusting a
                // binary search over data that doesn't fit its contract.
                if new_last < new_head || (new_head > old_head && new_last < old_last) {
                    log::warn!(
                        "[VPDERIVED nonmonotonic] reconcile_derived_series: old=({old_head},{old_last}) new=({new_head},{new_last}) new_len={new_len} vs0={vs0:.1} bc0={bc0}",
                    );
                    self.viewport.bar_count = new_len;
                    self.derived_series_stamp = Some((new_head, new_last));
                    return;
                }
                // Prepend shift: index of the first element at/after the old
                // head == number of bars merged in front of it.
                let prepended = if new_head < old_head {
                    let mut lo = 0usize;
                    let mut hi = new_len;
                    while lo < hi {
                        let mid = lo + (hi - lo) / 2;
                        if series.ts_ms_at(mid).unwrap_or(i64::MIN) < old_head {
                            lo = mid + 1;
                        } else {
                            hi = mid;
                        }
                    }
                    lo
                } else {
                    0
                };
                let visible_f = if self.viewport.bar_spacing > 0.0 {
                    self.viewport.chart_width / self.viewport.bar_spacing
                } else {
                    0.0
                };
                let was_glued = bc0 as f64 <= vs0 + visible_f;
                if prepended > 0 {
                    self.viewport.view_start += prepended as f64;
                }
                self.viewport.bar_count = new_len;
                let tail_grew = new_last > old_last;
                if self.price_scale.scale_mode.is_follow()
                    || (tail_grew && was_glued && self.price_scale.scale_mode == ScaleMode::Auto)
                {
                    self.snap_to_end(DEFAULT_SNAP_MARGIN);
                }
                if prepended > 0 {
                    self.log_view_jump("reconcile_derived_prepend", vs0, bc0);
                }
            }
            None => {
                // First sighting with the snap flag already spent (e.g. a
                // preset restore that landed data before this reactor ran):
                // adopt the count without touching view_start.
                self.viewport.bar_count = new_len;
            }
        }
        self.derived_series_stamp = Some((new_head, new_last));
    }

    /// Test-only: force the viewport into a known geometry/position.
    ///
    /// Narrow, explicitly named escape hatch for integration tests (in the
    /// separate `tests/` crate, so `#[cfg(test)]` cannot gate it) that need
    /// to synthesize a specific viewport state (e.g. "scrolled to the far
    /// left") to exercise a reactor path. Not called from production code.
    #[doc(hidden)]
    pub fn force_viewport_for_test(&mut self, chart_width: f64, bar_spacing: f64, view_start: f64) {
        if self.viewport.chart_width <= 0.0 {
            self.viewport.chart_width = chart_width;
        }
        if self.viewport.bar_spacing <= 0.0 {
            self.viewport.bar_spacing = bar_spacing;
        }
        self.viewport.view_start = view_start;
    }


    /// Clamp `bar_spacing` up to `min_spacing` if it's currently below it,
    /// keeping the RIGHT EDGE anchored (the bar that currently sits at the
    /// visible-window's right boundary stays there after the clamp).
    ///
    /// Wave T1 commit 1 (`docs/mlc/audits/series-types-ui-audit-2026-07-04.md`):
    /// chart types with a text-legibility floor (footprint clusters) inherit
    /// whatever `bar_spacing` the previous chart type left behind — often the
    /// 8px kline default, well under footprint's 40px `MIN_BAR_WIDTH_PX`
    /// gate — which renders as a permanent zoom-in placeholder. Called once
    /// on `ChangeChartType` when switching TO such a type; switching AWAY
    /// does not restore the old spacing (the user can zoom back out).
    ///
    /// Right-edge math mirrors `Viewport::zoom_at`'s "keep the bar under a
    /// fixed pixel position fixed" identity, evaluated at `x = chart_width`
    /// (the right edge) instead of the cursor position:
    /// `bar_under_edge = view_start + chart_width / old_spacing`,
    /// then re-derive `view_start` from the same `bar_under_edge` at the
    /// new spacing.
    pub fn ensure_min_bar_spacing(&mut self, min_spacing: f64) {
        let old_spacing = self.viewport.bar_spacing;
        if old_spacing <= 0.0 || old_spacing >= min_spacing {
            return;
        }
        let max_spacing = self.viewport.max_bar_spacing();
        let new_spacing = min_spacing.min(max_spacing.max(min_spacing));
        if self.viewport.chart_width > 0.0 {
            let bar_under_right_edge = self.viewport.view_start + self.viewport.chart_width / old_spacing;
            self.viewport.bar_spacing = new_spacing;
            self.viewport.view_start = bar_under_right_edge - self.viewport.chart_width / new_spacing;
        } else {
            self.viewport.bar_spacing = new_spacing;
        }
        self.needs_y_rescale = true;
    }

    // =========================================================================
    // Action Dispatch
    // =========================================================================

    // The unused method-level `execute_chart_action` was deleted — every
    // caller goes through the free function in `state::action_executor`.
    // Keeping the method-flavoured version forced ChartContainer to take a
    // `&mut DrawingManager`, blocking the manager's backend relocation.
}

impl Default for ChartContainer {
    fn default() -> Self {
        Self::new("BTCUSD", Timeframe::h1())
    }
}

#[cfg(test)]
mod focus_scale_mode_tests {
    //! Coverage for the 2026-07-10 owner redefinition: F = A (full refit,
    //! shrink+grow) + a focus guard-band extension on top. See
    //! `ChartContainer::calc_auto_scale`'s doc comment for the contract.
    use super::*;
    use mlc_core::Bar;

    fn bar(ts_sec: i64, high: f64, low: f64, close: f64) -> Bar {
        Bar { timestamp: ts_sec, open: close, high, low, close, volume: 1.0, ..Bar::default() }
    }

    /// 20 bars, each [i*10 - 1, i*10 + 1] high/low around a close of i*10 —
    /// a steadily widening range so "visible window narrows" and "visible
    /// window widens" produce visibly different fits.
    fn bars(n: usize) -> Vec<Bar> {
        (0..n).map(|i| {
            let base = i as f64 * 10.0;
            bar((i as i64 + 1) * 60, base + 1.0, base - 1.0, base)
        }).collect()
    }

    fn container_with_viewport(scale_mode: ScaleMode, chart_height: f64, view_start: f64, bar_count: usize) -> ChartContainer {
        let mut c = ChartContainer::new("BTCUSDT", Timeframe::m1());
        c.price_scale.scale_mode = scale_mode;
        c.viewport.chart_width = 400.0;
        c.viewport.chart_height = chart_height;
        c.viewport.bar_spacing = 40.0; // 10 bars visible at a time
        c.viewport.view_start = view_start;
        c.viewport.bar_count = bar_count;
        c
    }

    /// (a) F + visible bars move to a TIGHTER range → the range tightens,
    /// exactly like A (F is no longer grow-only fixation).
    #[test]
    fn focus_mode_tightens_like_auto_when_visible_bars_narrow() {
        let data = bars(20);

        // Wide window first (fits bars 0..12ish → wide range), matching A's
        // behavior for the same starting point.
        let mut f = container_with_viewport(ScaleMode::Focus, 300.0, 0.0, data.len());
        f.calc_auto_scale(crate::active_series::ActiveSeries::Bars(&data));
        let wide_range = f.price_scale.price_max - f.price_scale.price_min;

        // Scroll the SAME container to a narrower-range window (later bars,
        // same visible bar COUNT — range across those bars is identical
        // shape but let's instead directly shrink bar_spacing's effective
        // window by moving view_start near the end where fewer bars remain
        // before bar_count, narrowing visible_range's clamped span).
        f.viewport.view_start = 17.0;
        f.calc_auto_scale(crate::active_series::ActiveSeries::Bars(&data));
        let tight_range = f.price_scale.price_max - f.price_scale.price_min;

        assert!(
            tight_range < wide_range,
            "F must tighten when the visible window narrows, like A: wide={wide_range} tight={tight_range}"
        );

        // Pin against A computed independently over the SAME final window —
        // the two must agree on the FIT portion (F may only be >= A's fit
        // after the guard extension, never derive a different fit).
        let mut a = container_with_viewport(ScaleMode::Auto, 300.0, 17.0, data.len());
        a.calc_auto_scale(crate::active_series::ActiveSeries::Bars(&data));
        assert!(f.price_scale.price_min <= a.price_scale.price_min + 1e-9);
        assert!(f.price_scale.price_max >= a.price_scale.price_max - 1e-9);
    }

    /// (b) F + last tick within 2% of the edge → the range extends BEYOND
    /// A's fit (the guard-band pushes the bound out; A itself never does
    /// this).
    #[test]
    fn focus_mode_extends_beyond_auto_fit_when_tick_is_near_the_edge() {
        let mut data = bars(20);
        // Force the last bar's close right at the top of its own high/low
        // band (already at the fit's max) so the guard band from a 2%-close
        // tick pushes price_max out past what A alone would compute.
        let last_idx = data.len() - 1;
        data[last_idx].close = data[last_idx].high;

        let mut f = container_with_viewport(ScaleMode::Focus, 300.0, 0.0, data.len());
        f.calc_auto_scale(crate::active_series::ActiveSeries::Bars(&data));

        let mut a = container_with_viewport(ScaleMode::Auto, 300.0, 0.0, data.len());
        a.calc_auto_scale(crate::active_series::ActiveSeries::Bars(&data));

        assert!(
            f.price_scale.price_max > a.price_scale.price_max,
            "F must extend past A's fit when the tick sits at the very edge: f_max={} a_max={}",
            f.price_scale.price_max, a.price_scale.price_max,
        );
        // The bottom edge is untouched by this scenario (tick is near the
        // TOP edge only) — F's price_min must equal A's fit exactly.
        assert!((f.price_scale.price_min - a.price_scale.price_min).abs() < 1e-9);
    }

    /// (c) A is unchanged by the refactor — pin one concrete A case against
    /// the exact 4%-padding formula `calc_auto_scale`'s doc comment
    /// describes.
    #[test]
    fn auto_mode_fit_is_unchanged_4_percent_padding() {
        let data = bars(20);
        let mut a = container_with_viewport(ScaleMode::Auto, 300.0, 0.0, data.len());
        a.calc_auto_scale(crate::active_series::ActiveSeries::Bars(&data));

        let (start, end) = a.viewport.visible_range();
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;
        for b in &data[start..end.min(data.len())] {
            min = min.min(b.low);
            max = max.max(b.high);
        }
        let range = max - min;
        let padding = range * 0.04;
        assert!((a.price_scale.price_min - (min - padding)).abs() < 1e-9);
        assert!((a.price_scale.price_max - (max + padding)).abs() < 1e-9);
    }
}

#[cfg(test)]
mod viewport_snap_tests {
    //! Regression coverage for the 2026-07-11 owner report (TPO live
    //! verification): (1) `ensure_min_bar_spacing` must apply on
    //! `ChangeChartType` regardless of symbol/account-type/timeframe/
    //! connector state — covered at the `chart_container` layer by pinning
    //! `snap_to_end`'s behavior once `bar_spacing` is already the wide
    //! value (the ChangeChartType wiring itself lives in
    //! `input::chart_out_events`, exercised live); (2) a series with fewer
    //! bars than the visible window must snap with the tail flush against
    //! the RIGHT edge (negative `view_start`), not pinned to the left.
    use super::*;

    fn container_with(chart_width: f64, bar_spacing: f64, bar_count: usize) -> ChartContainer {
        let mut c = ChartContainer::new("BTCUSDT", Timeframe::m1());
        c.viewport.chart_width = chart_width;
        c.viewport.chart_height = 300.0;
        c.viewport.bar_spacing = bar_spacing;
        c.viewport.bar_count = bar_count;
        c
    }

    #[test]
    fn snap_to_end_places_undersized_series_at_right_edge_not_left() {
        // 1 session at TPO's 120px min_bar_spacing, 1200px chart_width ->
        // visible_f = 10 bars. A 1-element series must NOT clamp to
        // view_start=0 (left edge) — it must go negative so the single
        // block sits near the right edge with margin, like every other
        // series.
        let mut c = container_with(1200.0, 120.0, 1);
        c.snap_to_end(DEFAULT_SNAP_MARGIN);
        assert!(
            c.viewport.view_start < 0.0,
            "expected negative view_start (tail-at-right) for a 1-bar series in a 10-bar-wide viewport, got {}",
            c.viewport.view_start
        );
        // The single bar (index 0) must fall within the visible range.
        let (start, end) = c.viewport.visible_range();
        assert!(start <= 0 && end >= 1, "bar 0 must be visible: range=({start},{end})");
    }

    #[test]
    fn snap_to_end_keeps_normal_clamp_for_oversized_series() {
        // 500 bars, 10 visible -> the classic right-anchored case; must
        // stay non-negative and unaffected by the removed .max(0.0) floor.
        let mut c = container_with(1200.0, 120.0, 500);
        c.snap_to_end(DEFAULT_SNAP_MARGIN);
        assert!(c.viewport.view_start > 0.0, "view_start should be deep into the series: {}", c.viewport.view_start);
        let (_, end) = c.viewport.visible_range();
        assert_eq!(end, 500, "the last bar must be included in the visible range");
    }

    #[test]
    fn snap_to_end_exact_fit_uses_progressive_margin_not_floored() {
        // count == visible_f (10 bars, 10 visible): the progressive margin
        // formula (`min(DEFAULT_SNAP_MARGIN, ceil(5% * visible_f))`) still
        // adds a small headroom margin — view_start = count + margin -
        // visible_f = 10 + 1 - 10 = 1, NOT artificially floored to 0.
        let mut c = container_with(1200.0, 120.0, 10);
        c.snap_to_end(DEFAULT_SNAP_MARGIN);
        assert_eq!(c.viewport.view_start, 1.0, "exact fit should reflect the progressive margin, not a floor");
    }

    #[test]
    fn ensure_min_bar_spacing_applies_from_default_kline_spacing() {
        // Simulates the ChangeChartType path: window inherits the 8px
        // kline default, then switches to a chart_type with a wide floor
        // (tpo's registry min_bar_spacing = 120.0).
        let mut c = container_with(1200.0, 8.0, 0);
        c.ensure_min_bar_spacing(120.0);
        assert_eq!(c.viewport.bar_spacing, 120.0);
    }

    #[test]
    fn ensure_min_bar_spacing_is_noop_when_already_wide_enough() {
        let mut c = container_with(1200.0, 150.0, 0);
        c.ensure_min_bar_spacing(120.0);
        assert_eq!(c.viewport.bar_spacing, 150.0, "must not shrink an already-wider spacing");
    }

    #[test]
    fn ensure_min_bar_spacing_applies_even_with_zero_chart_width() {
        // Dispatch-time edge case: layout hasn't run yet (chart_width == 0).
        // The clamp must still land unconditionally (the `else` branch).
        let mut c = container_with(0.0, 8.0, 0);
        c.ensure_min_bar_spacing(120.0);
        assert_eq!(c.viewport.bar_spacing, 120.0);
    }
}

#[cfg(test)]
mod pane_expand_defaults_tests {
    //! Coverage for the pane-expand arc's two new `ChartContainer` fields
    //! (docs/mlc/plans/pane-expand-2026-09-25.md, Step 4): a freshly
    //! constructed container has no main-pane overlay visible and no cached
    //! hit-test rect, same as a sub-pane's overlay state/result default.
    use super::*;

    #[test]
    fn new_container_has_default_main_pane_overlay_state_and_result() {
        let c = ChartContainer::new("BTCUSDT", Timeframe::m1());
        assert!(!c.main_pane_overlay_state.visible);
        assert_eq!(c.main_pane_overlay_state.hovered_button, None);
        assert!(c.main_pane_overlay_result.is_none());
    }
}
