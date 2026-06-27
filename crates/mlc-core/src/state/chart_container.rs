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

use crate::tag_manager::BubbleId;
use crate::{Bar, Viewport, PriceScale, TimeScale};
use crate::chart::types::price_scale::ScaleMode;
use crate::state::Timeframe;
use crate::{Crosshair, KineticState, DragMode};
use crate::{GridOptions, Legend, Watermark, PriceLine};
use crate::state::chart_panel_state::ChartPanelState;
use crate::chart::{CompareOverlay, CompareSeries, get_compare_color};
use crate::state::{CommandHistory, Command};
use crate::data_provider::{SharedDataProvider, NullDataProvider};
use crate::scale_settings::ScaleSettings;
use crate::state::toolbar_state::{ChartToolbarState, ToolbarConfig};
use crate::state::modal_data::SubPaneOverlayState;
use crate::state::sub_pane_overlay::SubPaneOverlayResult;

/// Per-instance Y-axis state for indicator sub-panes.
///
/// Replaces the Y-axis fields that used to live on `SubPane`
/// (`price_min`, `price_max`, `auto_scale`, drag start state).
/// Keyed by `instance_id` in `ChartContainer.sub_pane_y_state`.
#[derive(Clone, Debug)]
pub struct SubPaneYState {
    /// Current Y-axis bottom price.
    pub price_min: f64,
    /// Current Y-axis top price.
    pub price_max: f64,
    /// Whether auto-scale is enabled for this sub-pane.
    pub auto_scale: bool,
    /// Saved price_min at drag start (for y-axis drag operations).
    pub drag_start_price_min: f64,
    /// Saved price_max at drag start (for y-axis drag operations).
    pub drag_start_price_max: f64,
}

impl Default for SubPaneYState {
    fn default() -> Self {
        Self {
            price_min: 0.0,
            price_max: 100.0,
            auto_scale: true,
            drag_start_price_min: 0.0,
            drag_start_price_max: 0.0,
        }
    }
}

/// Unique identifier for a chart window
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ChartId(pub u64);

impl std::fmt::Display for ChartId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Counter for generating unique chart IDs
static NEXT_CHART_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Generate a globally unique chart ID.
pub fn generate_chart_id() -> ChartId {
    ChartId(NEXT_CHART_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
}

pub fn bump_chart_id_past(min_id: u64) {
    NEXT_CHART_ID.fetch_max(min_id + 1, std::sync::atomic::Ordering::SeqCst);
}

/// Connection/data feed status for a chart window
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ConnectionStatus {
    /// Live real-time data feed
    #[default]
    Live,
    /// Delayed data feed
    Delayed,
    /// Disconnected / no data
    Disconnected,
}

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
    pub chart_panel_id: crate::state::chart_container_leaf::ChartPanelId,

    /// Permanent home bubble for this chart, created at chart birth and never
    /// changed.  Holds home-state primitives and command history while the
    /// chart is in a color-tag sync group.
    ///
    /// `BubbleId(0)` is a construction-time sentinel only.  Every code path
    /// that uses a `ChartContainer` must ensure this has been set to a real
    /// bubble before the container is active.
    pub auto_bubble_id: BubbleId,

    /// The bubble this chart currently displays.  Equal to `auto_bubble_id`
    /// when ungrouped; flipped to the color-tag bubble on sync join; flipped
    /// back to `auto_bubble_id` on desync.
    ///
    /// `BubbleId(0)` is a construction-time sentinel (same as `auto_bubble_id`).
    pub current_bubble_id: BubbleId,

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
    pub ha_cache: Option<Vec<crate::types::HaBar>>,
    /// Stamp paired with `ha_cache` —
    /// `(len, last_ts_ms, last_close_bits, last_high_bits, last_low_bits)`.
    /// The three `_bits` fields are `f64::to_bits()` — bit-identical comparison
    /// that is `Eq`-safe and catches in-place last-bar OHLC mutations (live tick
    /// update on the still-open bar) that share the same `len` and `timestamp`.
    pub ha_cache_stamp: Option<(usize, i64, u64, u64, u64)>,

    /// Volume Profile overlay data. Computed by `ensure_vp()` before each
    /// frame when `vp_show == true`. Not serialized — recomputed on load.
    pub vp_overlay: Option<crate::chart::volume_profile::VolumeProfileData>,
    /// Stamp paired with `vp_overlay` —
    /// `(bars.len(), last_ts_ms, view_start_idx, visible_count, mode_hash, last_close_bits, last_high_bits, last_low_bits)`.
    /// The three `_bits` fields catch in-place last-bar OHLC mutations on the
    /// still-open bar (same len + ts but different close/high/low).
    pub vp_overlay_stamp: Option<(usize, i64, usize, usize, u64, u64, u64, u64)>,
    /// Which window of bars the VP aggregates over.
    pub vp_mode: crate::chart::volume_profile::VpMode,
    /// When `true` the VP histogram overlay is rendered on top of the chart.
    /// Opt-in: defaults to `false` so existing presets are unaffected.
    pub vp_show: bool,

    /// When `true` the OI Delta sub-pane is rendered below the main chart.
    /// (Per-window display gate; storage lives on the bubble — Phase X0.)
    pub oi_show: bool,

    /// When `true` the Funding Rate strip is rendered at the bottom of the chart.
    /// (Per-window display gate; storage lives on the bubble — Phase X0.)
    pub funding_show: bool,

    // === Viewport & Scales ===
    /// Viewport state (pan/zoom)
    pub viewport: Viewport,
    /// Price scale (Y-axis)
    pub price_scale: PriceScale,
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

    // === Interaction State ===
    /// Crosshair state (position, visibility)
    pub crosshair: Crosshair,
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
    pub symbol_drawings: std::collections::HashMap<String, Vec<crate::drawing::PrimitiveSnapshot>>,

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

    /// `true` while an async scroll-left (historical bar extension) fetch is in flight.
    ///
    /// Guards against issuing multiple concurrent scroll-fetch requests for the
    /// same window. Reset to `false` when `ScrollBarsLoaded` arrives (or on error).
    /// Runtime-only state — not persisted to disk.
    pub scroll_fetch_in_flight: bool,

    /// When the current scroll fetch was started (for timeout reset).
    ///
    /// If `ScrollBarsLoaded` is dropped (channel lag), the 10-second timeout
    /// resets `scroll_fetch_in_flight` so the window can fetch again.
    /// Runtime-only state — not persisted to disk.
    pub scroll_fetch_started: Option<web_time::Instant>,

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
    pub inner_docking: uzor::layout::DockState<crate::state::chart_container_leaf::ChartContainerLeaf>,

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

/// Build a fresh `DockState<ChartContainerLeaf>` with a single `MainChart` leaf.
///
/// Used by every `ChartContainer` constructor. The main chart leaf's
/// `chart_panel_id` matches `ChartContainer.chart_panel_id` so that lookups
/// in `TradingPanelsStore.chart_panel` remain consistent.
pub fn make_inner_docking_with_main_chart(
    panel_id: crate::state::chart_container_leaf::ChartPanelId,
) -> uzor::layout::DockState<crate::state::chart_container_leaf::ChartContainerLeaf> {
    let mut ds = uzor::layout::DockState::new();
    ds.tree_mut().add_leaf(
        crate::state::chart_container_leaf::ChartContainerLeaf::MainChart { chart_panel_id: panel_id },
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
    inner_docking: &mut uzor::layout::DockState<crate::state::chart_container_leaf::ChartContainerLeaf>,
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
    inner_docking: &uzor::layout::DockState<crate::state::chart_container_leaf::ChartContainerLeaf>,
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
    inner_docking: &uzor::layout::DockState<crate::state::chart_container_leaf::ChartContainerLeaf>,
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
    inner_docking: &uzor::layout::DockState<crate::state::chart_container_leaf::ChartContainerLeaf>,
) -> Vec<f64> {
    inner_docking.tree().root().proportions.clone()
}

impl ChartContainer {
    /// Create a new chart window with a data provider.
    pub fn new_with_provider(_symbol: &str, _timeframe: Timeframe, data_provider: SharedDataProvider) -> Self {
        let id = generate_chart_id();
        let panel_id = crate::state::chart_container_leaf::generate_chart_panel_id();

        Self {
            id,
            chart_panel_id: panel_id,
            // Sentinel — caller must set auto_bubble_id / current_bubble_id
            // immediately after construction via post-construction mutation.
            auto_bubble_id: BubbleId(0),
            current_bubble_id: BubbleId(0),
            ha_cache: None,
            ha_cache_stamp: None,
            vp_overlay: None,
            vp_overlay_stamp: None,
            vp_mode: crate::chart::volume_profile::VpMode::default(),
            vp_show: false,
            oi_show: false,
            funding_show: false,
            viewport: Viewport::default(),
            price_scale: PriceScale::default(),
            time_scale: TimeScale::new(),
            sub_pane_y_state: HashMap::new(),
            sub_pane_overlay_states: Vec::new(),
            sub_pane_overlay_results: Vec::new(),
            // Interaction
            crosshair: Crosshair::default(),
            kinetic: KineticState::new(),
            drag_mode: DragMode::None,
            last_mouse_pos: None,
            drag_start_x: 0.0,
            drag_start_y: 0.0,
            drag_start_view: 0.0,
            drag_start_spacing: 0.0,
            drag_start_price_min: 0.0,
            drag_start_price_max: 0.0,
            // Object managers (DrawingManager lives in BackendCore::DrawingRegistry)
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
            scroll_fetch_in_flight: false,
            scroll_fetch_started: None,
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
        primitives: &[Box<dyn crate::drawing::primitives_v2::Primitive>],
    ) {
        let key = format!("{}:{}:{}", symbol, exchange, account_type);
        let snapshots: Vec<crate::drawing::PrimitiveSnapshot> = primitives
            .iter()
            .map(|p| crate::drawing::PrimitiveSnapshot::from_primitive(p.as_ref()))
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
    ) -> Option<Vec<crate::drawing::PrimitiveSnapshot>> {
        let key = format!("{}:{}:{}", symbol, exchange, account_type);
        self.symbol_drawings.get(&key).cloned()
    }

    /// Store the active chart type in the panel state.
    pub fn set_chart_type(&mut self, panel: &mut ChartPanelState, chart_type: &'static str) {
        panel.chart_type = chart_type;
    }

    // `clone_for_split` was deleted — every split path uses
    // `clone_for_split_shell` instead, which never touched DrawingManager.
    // The DM-aware clone path was dead code blocking the manager's
    // backend-side relocation.

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
    /// Phase X0 / C5c: caller supplies `bars` (typically `&bubble.bars`).
    pub fn calc_auto_scale(&mut self, bars: &[Bar]) {
        self.price_scale.calc_auto_scale(
            bars,
            self.viewport.visible_range(),
        );
    }

    /// Consume the `needs_snap_after_backfill` one-shot flag.
    ///
    /// Snaps the viewport to the newest bar (+DEFAULT_SNAP_MARGIN) and forces
    /// a Y autoscale bypassing the Manual-mode gate (the deserialized price
    /// range belongs to a stale viewport position; keeping it renders the fresh
    /// bars as a flat line).  Then clears the flag so the snap fires exactly once
    /// regardless of which delivery path (cache-served BarsLoaded, first-frame
    /// deferred, or BackfillComplete) arrives first.
    /// Phase X0 / C5c: caller supplies `bars` (typically `&bubble.bars`).
    pub fn consume_cold_snap(&mut self, bars: &[Bar]) {
        self.snap_to_end(DEFAULT_SNAP_MARGIN);
        let saved_mode = self.price_scale.scale_mode;
        self.price_scale.scale_mode = crate::chart::types::price_scale::ScaleMode::Auto;
        self.calc_auto_scale(bars);
        self.price_scale.scale_mode = saved_mode;
        self.needs_snap_after_backfill = false;
    }

    /// Reconcile viewport for initial load — resets viewport position and auto-scales.
    ///
    /// Phase X0 / C5c: bars storage lives on the bubble; this method only
    /// performs the viewport-side effects after `bubble.bars` has been
    /// replaced. The `bars` parameter is `&bubble.bars`.
    ///
    /// Use for the first data load. For backfill / incremental updates use
    /// [`reconcile_view_keep_anchor`] which preserves the user's viewport.
    pub fn reconcile_view_fresh(&mut self, bars: &[Bar]) {
        // A successful delivery clears any prior fetch error.
        self.data_error = None;

        // Update prev close line if enabled (derives prev_close from bars[0].open)
        self.update_prev_close_line(bars);

        // Update bar count so the viewport knows how many bars exist.
        self.viewport.bar_count = bars.len();

        if self.viewport.chart_width > 0.0 && self.viewport.bar_spacing > 0.0 {
            self.snap_to_end(DEFAULT_SNAP_MARGIN);
            self.calc_auto_scale(bars);
            if let Some(mode) = self.restore_scale_mode.take() {
                self.price_scale.scale_mode = mode;
            }
            self.needs_auto_scale_after_bars = false;
        } else {
            self.needs_auto_scale_after_bars = true;
        }
    }

    /// Reconcile viewport after backfill / scroll-load — keep anchor by ts,
    /// keep user's pan/zoom and scale mode.
    ///
    /// Phase X0 / C5c: bars storage lives on the bubble.
    /// `prev_anchor_ts` is the timestamp of the bar that WAS at view_start
    /// before bubble.bars was replaced; caller captures it from the old
    /// bubble state right before the bridge writes the new series.
    pub fn reconcile_view_keep_anchor(
        &mut self,
        bars: &[Bar],
        prev_anchor: Option<(i64, f64)>,
    ) {
        self.update_prev_close_line(bars);

        self.viewport.bar_count = bars.len();

        if let Some((anchor_ts, frac)) = prev_anchor {
            if let Some(new_i) = bars.iter().position(|b| b.timestamp >= anchor_ts) {
                self.viewport.view_start = new_i as f64 + frac;
            }
        }

        if self.price_scale.scale_mode.is_auto_y() {
            self.calc_auto_scale(bars);
        }
    }

    /// Capture the (timestamp, fractional offset) of the bar currently at
    /// `view_start`. Used by callers before they replace bubble.bars so they
    /// can pass it to `reconcile_view_keep_anchor` afterwards.
    pub fn capture_view_anchor(&self, bars: &[Bar]) -> Option<(i64, f64)> {
        if bars.is_empty() {
            return None;
        }
        let vs = self.viewport.view_start;
        let clamped_idx = (vs.floor() as usize).min(bars.len().saturating_sub(1));
        let frac = vs - vs.floor();
        bars.get(clamped_idx).map(|b| (b.timestamp, frac))
    }

    // `clone_data_from` was unused (mirrors viewport + scales between
    // windows) and forced ChartContainer to know about DrawingManager —
    // deleted alongside the other DM-bound methods.

    /// Create ChartRenderState from this window's view state + bubble's series storage.
    ///
    /// Phase X0 / C3 (2026-06-19): per-Kind series fields (bars + 13 derived)
    /// are sourced from `bubble` when supplied. Bubble is the storage hub;
    /// ChartContainer owns viewport / scales / crosshair / per-window
    /// derived caches (ha_cache, vp_overlay).
    ///
    /// Phase X0 / C5a (2026-06-19): `bubble` is now required. The renderer
    /// reads per-Kind series exclusively from bubble storage. Window's
    /// (legacy) `self.bars`/`self.footprint_bars`/... fields are no longer
    /// referenced here.
    ///
    /// `grid` and `legend` are supplied by the caller from `ChartPanelState`.
    /// `timeframe_interval_ms` is provided by the caller from `branch_meta`.
    pub fn to_render_state<'a>(
        &'a self,
        bubble: &'a crate::tag_manager::DataBubble,
        chart_rect: crate::chart::render_data::ChartRect,
        theme: &'a crate::chart::render_data::ChartTheme,
        current_timeframe: Option<&'a str>,
        time_format_settings: Option<&'a crate::TimeFormatSettings>,
        timeframe_interval_ms: Option<i64>,
        grid: &'a GridOptions,
        legend: &'a Legend,
    ) -> crate::chart::render_data::ChartRenderState<'a> {
        crate::chart::render_data::ChartRenderState {
            viewport: &self.viewport,
            price_scale: &self.price_scale,
            time_scale: &self.time_scale,
            bars: &bubble.bars,
            grid,
            crosshair: &self.crosshair,
            legend,
            chart_rect,
            theme,
            time_ticks: None,
            current_timeframe,
            disable_clip: false,
            time_format_settings,
            timeframe_interval_ms,
            scale_settings: Some(&self.scale_settings),
            body_enabled: true,
            border_enabled: true,
            wick_enabled: true,
            use_prev_close: false,
            footprint_bars: bubble.footprint_bars.as_deref(),
            renko_bars: bubble.renko_bars.as_deref(),
            three_line_break: bubble.three_line_break.as_deref(),
            pnf_columns: bubble.pnf_columns.as_deref(),
            kagi_segments: bubble.kagi_segments.as_deref(),
            cvd_line: bubble.cvd_line.as_deref(),
            tpo_sessions: bubble.tpo_sessions.as_deref(),
            // ha_cache + vp_overlay are window-local derived caches (viewport-coupled).
            ha_cache: self.ha_cache.as_deref(),
            vp_overlay: if self.vp_show { self.vp_overlay.as_ref() } else { None },
            // Display gates (oi_show / funding_show) are per-window view state;
            // storage is unconditional on bubble.
            oi_delta_ref: if self.oi_show { bubble.oi_delta.as_deref() } else { None },
            funding_rate_ref: if self.funding_show { bubble.funding_rate.as_deref() } else { None },
            dom_heatmap_ref: bubble.dom_heatmap.as_ref(),
            dom_trade_dots_ref: bubble.dom_trade_dots.as_deref(),
            liq_heatmap_ref: bubble.liq_heatmap.as_ref(),
            liq_dots_ref: bubble.liq_dots.as_deref(),
        }
    }

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
        let mut out: Vec<crate::types::HaBar> = Vec::with_capacity(bars.len());
        let first = &bars[0];
        let mut prev_open = (first.open + first.close) * 0.5;
        let mut prev_close = (first.open + first.high + first.low + first.close) * 0.25;
        out.push(crate::types::HaBar {
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
            out.push(crate::types::HaBar {
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
    /// `bucket_count` controls bucket granularity (24 is a reasonable default).
    pub fn ensure_vp_from_bars(
        &mut self,
        bars: &[Bar],
        visible_bars: &[Bar],
        bucket_count: usize,
    ) {
        use crate::chart::volume_profile::{VolumeProfileData, VpMode, filter_bars_for_mode};

        let mode = self.vp_mode;

        let mode_hash: u64 = match mode {
            VpMode::VisibleRange => 0,
            VpMode::Session => 1,
            VpMode::FixedRange { start_ts_ms, end_ts_ms } => {
                2u64.wrapping_add(start_ts_ms as u64).wrapping_add(end_ts_ms as u64)
            }
            VpMode::Composite => 3,
        };
        let vp_last = bars.last();
        let stamp = (
            bars.len(),
            vp_last.map(|b| b.ts_ms()).unwrap_or(0),
            self.viewport.view_start_idx(),
            visible_bars.len(),
            mode_hash,
            vp_last.map(|b| b.close.to_bits()).unwrap_or(0),
            vp_last.map(|b| b.high.to_bits()).unwrap_or(0),
            vp_last.map(|b| b.low.to_bits()).unwrap_or(0),
        );
        if self.vp_overlay_stamp == Some(stamp) && self.vp_overlay.is_some() {
            return;
        }

        let bars_to_profile: Vec<&Bar> = match mode {
            VpMode::VisibleRange => visible_bars.iter().collect(),
            _ => filter_bars_for_mode(bars, mode, 5),
        };

        if bars_to_profile.is_empty() {
            self.vp_overlay = Some(VolumeProfileData::compute(&[], mode, 0.70, bucket_count));
            self.vp_overlay_stamp = Some(stamp);
            return;
        }

        let owned: Vec<Bar> = bars_to_profile.into_iter().copied().collect();
        self.vp_overlay = Some(VolumeProfileData::compute(&owned, mode, 0.70, bucket_count));
        self.vp_overlay_stamp = Some(stamp);
    }

    /// Create a minimal clone of this chart window for use in `panel_grid::split_active`.
    ///
    /// Unlike `clone_for_split`, this variant does **not** take a source
    /// `DrawingManager` or `ChartPanelState`.  It creates a fresh `ChartContainer`
    /// shell with no drawing state.  The caller
    /// (chart-app `sync_group.rs`) is responsible for seeding the real
    /// `ChartPanelState` and a fresh `DrawingManager` into `panels_store` and
    /// `BackendCore::DrawingRegistry` after the split returns the new leaf ids.
    ///
    /// This overload exists so that `ChartPanelGrid::split_active` (inside the
    /// `zengeld-chart` crate) can call it without needing to know about
    /// `panels_store` — it just creates the container shell; the panel state is
    /// seeded by chart-app after the split.
    pub fn clone_for_split_shell(
        &self,
        new_chart_id: ChartId,
    ) -> Self {
        let new_panel_id = crate::state::chart_container_leaf::generate_chart_panel_id();

        Self {
            id: new_chart_id,
            chart_panel_id: new_panel_id,
            // Sentinel — do_split must detect BubbleId(0) and create a fresh
            // auto-bubble for every chart that clone_for_split_shell produces.
            auto_bubble_id: BubbleId(0),
            current_bubble_id: BubbleId(0),

            // Phase X0 / C5c: per-Kind series live on the bubble.
            ha_cache: None,
            ha_cache_stamp: None,
            vp_overlay: None,
            vp_overlay_stamp: None,
            vp_mode: self.vp_mode,
            vp_show: self.vp_show,
            oi_show: self.oi_show,
            funding_show: self.funding_show,
            viewport: self.viewport.clone(),
            price_scale: self.price_scale.clone(),
            time_scale: self.time_scale.clone(),

            sub_pane_y_state: HashMap::new(),
            sub_pane_overlay_states: Vec::new(),
            sub_pane_overlay_results: Vec::new(),

            crosshair: {
                let mut ch = self.crosshair;
                ch.visible = false;
                ch
            },
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
            // Pair with `pending_symbol_load: true` above — when the deferred
            // REST sweep lands as BarsLoaded the reconcile path will fresh-snap
            // the viewport to the right edge with the standard padding, same
            // contract as `seed_default_chart`.
            needs_snap_after_backfill: true,
            scroll_fetch_in_flight: false,
            scroll_fetch_started: None,
            data_error: None,
            inner_docking: make_inner_docking_with_main_chart(new_panel_id),
            sub_pane_above_main: std::collections::HashSet::new(),
        }
    }

    /// Create ScaleCornerState from this window's data
    pub fn to_corner_state(&self) -> crate::state::scale_corner::ScaleCornerState {
        crate::state::scale_corner::ScaleCornerState {
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
        layout: &crate::state::geometry::ExtendedFrameLayout,
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

        self.crosshair.visible = true;
        self.crosshair.synced = false; // Local mouse update — not synced
        self.crosshair.pane_index = pane_index;
        self.crosshair.x = clamped_x;
        self.crosshair.y = local_y.clamp(0.0, pane_height);
        self.crosshair.bar_idx = bar_idx;
        self.crosshair.bar_f64 = bar_f64;
        self.crosshair.price = price;

        if pane_index.is_none() && !is_dragging {
            let (snapped_price, snapped_y) = self.calculate_magnet_snap(
                bars, bar_idx, price, pane_height, price_min, price_max,
            );
            self.crosshair.set_snapped(snapped_price, snapped_y);
        } else {
            self.crosshair.snapped_y = self.crosshair.y;
            self.crosshair.snapped_price = price;
        }
    }

    /// Centralized magnet snap calculation. Phase X0 / C5c: caller supplies `bars`.
    pub fn calculate_magnet_snap(
        &self,
        bars: &[Bar],
        bar_idx: Option<usize>,
        raw_price: f64,
        pane_height: f64,
        price_min: f64,
        price_max: f64,
    ) -> (f64, f64) {
        let price_range = price_max - price_min;

        if !self.crosshair.is_magnet() {
            let raw_y = if price_range > 0.0 {
                ((price_max - raw_price) / price_range) * pane_height
            } else {
                0.0
            };
            return (raw_price, raw_y);
        }

        if let Some(idx) = bar_idx {
            if idx < bars.len() {
                let bar = &bars[idx];
                let (snapped_price, snapped_y) = self.crosshair.find_nearest_ohlc(
                    bar,
                    raw_price,
                    |p| {
                        let ratio = (price_max - p) / price_range;
                        ratio * pane_height
                    },
                    self.viewport.bar_spacing,
                );
                return (snapped_price, snapped_y.clamp(0.0, pane_height));
            }
        }

        let raw_y = if price_range > 0.0 {
            ((price_max - raw_price) / price_range) * pane_height
        } else {
            0.0
        };
        (raw_price, raw_y)
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
            eprintln!("[Compare] Symbol {} already compared or is main symbol", symbol);
            return false;
        }

        let Some(bars) = self.data_provider.get_bars(symbol, main_timeframe) else {
            eprintln!(
                "[Compare] Data not available for symbol={} tf={}",
                symbol,
                main_timeframe.name
            );
            return false;
        };
        if bars.is_empty() {
            eprintln!("[Compare] No bars loaded for {}", symbol);
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

        eprintln!(
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
        eprintln!(
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
        eprintln!("[Compare] Cleared {} series", count);
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
                    line_style: crate::drawing::LineStyle::Dashed,
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
    // Chart Type
    // =========================================================================

    /// Set chart type in panel state and record in undo history.
    pub fn set_chart_type_with_undo(&mut self, panel: &mut ChartPanelState, chart_type: &'static str) {
        let previous_type = panel.chart_type.to_string();
        panel.chart_type = chart_type;
        self.command_history.push(Command::ChangeChartType {
            previous_type,
            new_type: chart_type.to_string(),
        });
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
    pub fn set_grid_style(&self, panel: &mut ChartPanelState, style: crate::drawing::LineStyle) {
        panel.grid_options.vert_lines.style = style;
        panel.grid_options.horz_lines.style = style;
    }

    /// Set crosshair mode on both crosshair state and crosshair options.
    pub fn set_crosshair_mode(&mut self, panel: &mut ChartPanelState, mode: crate::CrosshairMode) {
        self.crosshair.mode = mode;
        panel.crosshair_options.mode = mode;
    }

    /// Set crosshair line style for both vertical and horizontal lines.
    pub fn set_crosshair_style(&self, panel: &mut ChartPanelState, style: crate::drawing::LineStyle) {
        panel.crosshair_options.vert_line.style = style;
        panel.crosshair_options.horz_line.style = style;
    }

    /// Set legend position.
    pub fn set_legend_position(&self, panel: &mut ChartPanelState, pos: crate::LegendPosition) {
        panel.legend.position = pos;
    }

    /// Set watermark position. Creates a default watermark if none exists.
    pub fn set_watermark_position(&self, panel: &mut ChartPanelState, horz: crate::HorzAlign, vert: crate::VertAlign) {
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

    /// Zoom in at chart center (1.1x bar spacing).
    pub fn zoom_in(&mut self, bars: &[Bar]) {
        let center_x = self.viewport.chart_width / 2.0;
        self.viewport.zoom_at(center_x, 1.1);
        self.calc_auto_scale(bars);
    }

    /// Zoom out at chart center (0.9x bar spacing).
    pub fn zoom_out(&mut self, bars: &[Bar]) {
        let center_x = self.viewport.chart_width / 2.0;
        self.viewport.zoom_at(center_x, 0.9);
        self.calc_auto_scale(bars);
    }

    /// Snap viewport to the most recent bar with `margin` bars of empty right space.
    ///
    /// `margin` is a count of empty bars shown to the right of the last candle.
    /// Use [`DEFAULT_SNAP_MARGIN`] for the standard value.
    ///
    /// Formula: `view_start = (bar_count + margin - visible_f).max(0.0)`
    ///
    /// Preconditions: `self.viewport.chart_width > 0.0` and
    /// `self.viewport.bar_spacing > 0.0` must both hold, or the result is
    /// meaningless (those are the same preconditions as [`Viewport::visible_bars`]).
    /// Phase X0 / C5c: uses `viewport.bar_count` which is maintained by
    /// `reconcile_view_to_bars` whenever bubble.bars length changes — no
    /// direct reference to a bars slice needed here.
    pub fn snap_to_end(&mut self, margin: f64) {
        let visible_f = self.viewport.chart_width / self.viewport.bar_spacing;
        let count = self.viewport.bar_count;
        self.viewport.view_start = (count as f64 + margin - visible_f).max(0.0);
    }

    /// Fit all bars into the visible chart area. Phase X0 / C5c: bars supplied by caller.
    pub fn fit_content(&mut self, bars: &[Bar]) {
        let bar_count = bars.len();
        if bar_count > 0 {
            self.viewport.bar_spacing = self.viewport.chart_width / bar_count as f64;
            self.viewport.bar_spacing = self.viewport.bar_spacing.clamp(1.0, 30.0);
            self.snap_to_end(DEFAULT_SNAP_MARGIN);
            self.calc_auto_scale(bars);
        }
    }

    /// Reset zoom to default bar spacing (8px) and scroll to end.
    pub fn reset_zoom(&mut self, bars: &[Bar]) {
        self.viewport.bar_spacing = 8.0;
        self.snap_to_end(DEFAULT_SNAP_MARGIN);
        self.calc_auto_scale(bars);
    }

    // =========================================================================
    // Split-aware scroll helpers (called when routing scroll to sub-charts)
    // =========================================================================

    /// Pan the viewport horizontally by `pixel_delta` pixels.
    ///
    /// Delegates to [`Viewport::pan`] which dispatches axis-aware
    /// (time mode shifts ts anchor, index mode converts via bar_spacing).
    pub fn pan_horizontal(&mut self, pixel_delta: f64, bars: &[Bar]) {
        self.viewport.pan(pixel_delta);
        if self.price_scale.scale_mode.is_auto_y() {
            self.calc_auto_scale(bars);
        }
    }

    /// Zoom the viewport horizontally around `center_x` (local pixel coord).
    ///
    /// `factor` > 1.0 zooms in (more bars per pixel), < 1.0 zooms out.
    /// Delegates to [`Viewport::zoom_at`] which handles clamping.
    pub fn zoom_horizontal(&mut self, center_x: f64, factor: f64, bars: &[Bar]) {
        self.viewport.zoom_at(center_x, factor);
        if self.price_scale.scale_mode.is_auto_y() {
            self.calc_auto_scale(bars);
        }
    }

    /// Pan the price scale vertically by `pixel_delta` pixels.
    ///
    /// Converts pixels to price units using the current visible price range and
    /// chart height.  Has no effect when auto-scale is enabled.
    pub fn pan_vertical(&mut self, pixel_delta: f64) {
        if !self.price_scale.scale_mode.is_auto_y() && self.viewport.chart_height > 0.0 {
            let price_range = self.price_scale.price_max - self.price_scale.price_min;
            let price_delta = pixel_delta * price_range / self.viewport.chart_height;
            self.price_scale.price_min += price_delta;
            self.price_scale.price_max += price_delta;
        }
    }

    /// Zoom the price scale vertically around the center price.
    ///
    /// `factor` > 1.0 expands the range (zoom out), < 1.0 contracts it
    /// (zoom in).  Disables auto-scale so the manual range is preserved.
    pub fn zoom_vertical(&mut self, factor: f64) {
        self.price_scale.scale_mode = ScaleMode::Manual;
        let center = (self.price_scale.price_min + self.price_scale.price_max) / 2.0;
        let half_range = (self.price_scale.price_max - self.price_scale.price_min) / 2.0 * factor;
        self.price_scale.price_min = center - half_range;
        self.price_scale.price_max = center + half_range;
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
