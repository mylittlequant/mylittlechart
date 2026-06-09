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
use crate::{Crosshair, CrosshairOptions, KineticState, DragMode};
use crate::{GridOptions, Legend, Watermark, PriceLine};
use crate::state::chart_panel_state::ChartPanelState;
use crate::chart::{CompareOverlay, CompareSeries, get_compare_color};
use crate::state::{CommandHistory, Command};
use crate::data_provider::{SharedDataProvider, NullDataProvider};
use crate::scale_settings::ScaleSettings;
use crate::panel_app::{ChartToolbarState, ToolbarConfig};
use crate::ui::modal_settings::SubPaneOverlayState;
use crate::layout::SubPaneOverlayResult;

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
    /// OHLC bars for this chart
    pub bars: Vec<Bar>,

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
    pub symbol_drawings: std::collections::HashMap<String, Vec<crate::preset::snapshots::PrimitiveSnapshot>>,

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
    pub scroll_fetch_started: Option<std::time::Instant>,

    /// Scale mode to restore after the next `set_bars()` call completes.
    ///
    /// Set by `LoadPreset` before bars arrive asynchronously. Consumed and
    /// cleared inside `set_bars()` — applied after snap-to-end and auto-scale
    /// so the user's Manual/Auto preference survives the async boundary.
    /// `None` = no restoration needed; `set_bars()` leaves the mode as-is.
    /// Runtime-only, not persisted.
    pub restore_scale_mode: Option<ScaleMode>,

    /// Pre-join scale mode: saved when this window enters a group with
    /// `sync_viewport = ON`. Restored automatically on desync so the window
    /// returns to its own A/M/F choice. F (Focus) implies a pinned viewport,
    /// so mode is tightly coupled to viewport sync state.
    pub stashed_scale_mode: Option<ScaleMode>,

    /// Inner uzor docking tree: [MainChart leaf, IndicatorPanel leaf, ...].
    /// Single source of truth for sub-pane identity, proportions, and
    /// separator drag. Replaces `inner_layout` (which is kept only during
    /// the dual-write transition and deleted once all reads migrate here).
    pub inner_docking: uzor::layout::DockState<crate::state::chart_container_leaf::ChartContainerLeaf>,
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
    ds
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
            bars: Vec::new(),
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
            // Object managers (drawing_manager now lives in ChartPanelState)
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
            scroll_fetch_in_flight: false,
            scroll_fetch_started: None,
            inner_docking: make_inner_docking_with_main_chart(panel_id),
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
    /// separator that sits above the `IndicatorPanel` leaf with `instance_id`.
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

    /// Snapshot current drawings into the per-symbol cache for the given symbol.
    ///
    /// The cache key is `"symbol:exchange:account_type"` (e.g. `"BTCUSDT:binance:S"`).
    /// If there are no drawings, any existing cache entry for the key is removed.
    ///
    /// `drawing_manager` is taken from the caller's `ChartPanelState` (S2.8 migration).
    pub fn snapshot_drawings_for_symbol(
        &mut self,
        symbol: &str,
        exchange: &str,
        account_type: &str,
        drawing_manager: &crate::drawing::DrawingManager,
    ) {
        let key = format!("{}:{}:{}", symbol, exchange, account_type);
        let snapshots: Vec<crate::preset::snapshots::PrimitiveSnapshot> = drawing_manager
            .primitives()
            .iter()
            .map(|p| crate::preset::snapshots::PrimitiveSnapshot::from_primitive(p.as_ref()))
            .collect();
        if !snapshots.is_empty() {
            self.symbol_drawings.insert(key, snapshots);
        } else {
            self.symbol_drawings.remove(&key);
        }
    }

    /// Restore drawings from the per-symbol cache for the given symbol.
    ///
    /// The cache key is `"symbol:exchange:account_type"` (e.g. `"BTCUSDT:binance:S"`).
    /// Returns `true` if drawings were restored, `false` if no cache entry exists.
    ///
    /// `drawing_manager` is taken from the caller's `ChartPanelState` (S2.8 migration).
    pub fn restore_drawings_for_symbol(
        &mut self,
        symbol: &str,
        exchange: &str,
        account_type: &str,
        drawing_manager: &mut crate::drawing::DrawingManager,
    ) -> bool {
        let key = format!("{}:{}:{}", symbol, exchange, account_type);
        if let Some(snapshots) = self.symbol_drawings.get(&key).cloned() {
            if let Ok(reg) = crate::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                for snap in &snapshots {
                    if let Some(prim) = reg.from_json(&snap.type_id, &snap.json) {
                        drawing_manager.add_external_primitive(prim);
                    }
                }
            }
            true
        } else {
            false
        }
    }

    /// Store the active chart type in the panel state.
    pub fn set_chart_type(&mut self, panel: &mut ChartPanelState, chart_type: &'static str) {
        panel.chart_type = chart_type;
    }

    /// Create a clone of this chart window for split operation.
    ///
    /// Copies viewport settings, scale settings, indicators.
    /// Gets a fresh ChartId.
    ///
    /// `symbol`, `exchange`, `account_type` and `timeframe` are now owned by
    /// `branch_meta` in chart-app; callers pass them in from `read_*` helpers.
    ///
    /// `src_drawing_manager` is the source window's `DrawingManager` (from
    /// `ChartPanelState` after S2.8 migration).
    ///
    /// When `sync_drawings` is `true`, all existing primitives are cloned into the
    /// new window as synced copies (`origin_id` is set to the original's id).
    /// When `false`, the new window starts with an empty drawing manager.
    ///
    /// Returns a `(ChartContainer, ChartPanelState)` tuple.  The caller must
    /// insert the `ChartPanelState` into `TradingPanelsStore.chart_panel` under
    /// the new `chart_panel_id` (available as `new_container.chart_panel_id`).
    pub fn clone_for_split(
        &self,
        new_chart_id: ChartId,
        sync_drawings: bool,
        symbol: &str,
        exchange: &str,
        account_type: &str,
        src_drawing_manager: &crate::drawing::DrawingManager,
        src_panel: &ChartPanelState,
    ) -> (Self, ChartPanelState) {
        let new_panel_id = crate::state::chart_container_leaf::generate_chart_panel_id();
        let mut drawing_manager = crate::drawing::DrawingManager::new();
        drawing_manager.set_current_window(Some(new_chart_id.0));
        drawing_manager.set_current_symbol_key(symbol, exchange, account_type);
        if sync_drawings {
            let synced = src_drawing_manager.clone_primitives_for_sync(new_chart_id.0);
            drawing_manager.add_synced_primitives(synced);
        }

        // Build ChartPanelState for the new window, inheriting display options
        // from the source panel.
        let mut new_panel = ChartPanelState::new(new_panel_id);
        new_panel.show_candles = src_panel.show_candles;
        new_panel.show_bars = src_panel.show_bars;
        new_panel.show_hollow_candles = src_panel.show_hollow_candles;
        new_panel.show_heikin_ashi = src_panel.show_heikin_ashi;
        new_panel.show_line = src_panel.show_line;
        new_panel.show_step_line = src_panel.show_step_line;
        new_panel.show_line_markers = src_panel.show_line_markers;
        new_panel.show_area = src_panel.show_area;
        new_panel.show_hlc_area = src_panel.show_hlc_area;
        new_panel.show_histogram = src_panel.show_histogram;
        new_panel.show_columns = src_panel.show_columns;
        new_panel.show_baseline = src_panel.show_baseline;
        new_panel.chart_type = src_panel.chart_type;
        new_panel.grid_options = src_panel.grid_options.clone();
        new_panel.crosshair_options = src_panel.crosshair_options.clone();
        new_panel.legend = src_panel.legend.clone();
        new_panel.watermark = src_panel.watermark.clone();
        new_panel.tooltip = src_panel.tooltip.clone();
        new_panel.drawing_manager = drawing_manager;

        let new_container = Self {
            // New chart ID
            id: new_chart_id,
            // Fresh canvas-leaf id for the split; the source's stays put.
            chart_panel_id: new_panel_id,
            // Sentinel — do_split must detect BubbleId(0) and create a fresh
            // auto-bubble for every chart that clone_for_split produces.
            auto_bubble_id: BubbleId(0),
            current_bubble_id: BubbleId(0),

            // === Data (cloned) ===
            bars: self.bars.clone(),

            // === Viewport & Scales (cloned) ===
            viewport: self.viewport.clone(),
            price_scale: self.price_scale.clone(),
            time_scale: self.time_scale.clone(),

            // === Sub-panes ===
            // Y-state not inherited — sync_sub_panes_from_manager seeds it on next tick.
            sub_pane_y_state: HashMap::new(),
            // Overlay state is not inherited — the split child starts with no hover state.
            sub_pane_overlay_states: Vec::new(),
            // Overlay results are frame-local; the split child starts empty.
            sub_pane_overlay_results: Vec::new(),

            // === Interaction State (reset) ===
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

            // === Object Managers (drawing_manager now lives in ChartPanelState) ===
            price_lines: self.price_lines.clone(),

            // === Compare Overlay (reset) ===
            compare_overlay: CompareOverlay::new(),

            // === Command History (reset) ===
            command_history: CommandHistory::new(250),

            // === Data Provider (shared) ===
            data_provider: self.data_provider.clone(),

            // === Scale Settings (cloned) ===
            scale_settings: self.scale_settings.clone(),

            // === Toolbar State (reset for new panel) ===
            chart_toolbar: ChartToolbarState::default(),

            // === Toolbar Config (sub-windows have no toolbar — parent owns it) ===
            toolbar_config: ToolbarConfig::minimal(),

            // Split child does not inherit per-symbol drawing cache.
            symbol_drawings: HashMap::new(),
            // Split child has no pending viewport restore.
            // Split child has no pending symbol load.
            pending_symbol_load: false,
            // Split child has no deferred auto-scale pending.
            needs_auto_scale_after_bars: false,
            // Split child has no scale mode to restore.
            restore_scale_mode: None,
            // Split child has no stashed scale mode — it joins the group fresh.
            stashed_scale_mode: None,
            // Split child has no in-flight scroll fetch.
            scroll_fetch_in_flight: false,
            scroll_fetch_started: None,
            // Clone starts with only MainChart leaf; sync_sub_panes_from_manager
            // re-adds indicator leaves on the next tick.
            inner_docking: make_inner_docking_with_main_chart(new_panel_id),
        };
        (new_container, new_panel)
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

    /// Calculate auto-scale for price axis based on visible data
    pub fn calc_auto_scale(&mut self) {
        self.price_scale.calc_auto_scale(
            &self.bars,
            self.viewport.visible_range(),
        );
    }

    /// Set bars for initial load — resets viewport position and auto-scales.
    ///
    /// Use this only for the first data load. For backfill / incremental
    /// updates use [`update_bars`] which preserves the user's viewport.
    ///
    /// When `chart_width > 0` the snap fires eagerly here (most calls after
    /// the first frame). When `chart_width == 0` (first-launch, brand-new
    /// window) the snap is deferred to `prepare_frame()` via
    /// `needs_auto_scale_after_bars`.
    pub fn set_bars(&mut self, bars: Vec<Bar>) {
        self.bars = bars;

        // Update prev close line if enabled (derives prev_close from bars[0].open)
        self.update_prev_close_line();

        // Update bar count so the viewport knows how many bars exist.
        self.viewport.bar_count = self.bars.len();

        if self.viewport.chart_width > 0.0 && self.viewport.bar_spacing > 0.0 {
            // Eager snap: chart_width is valid, snap immediately.
            self.snap_to_end(DEFAULT_SNAP_MARGIN);
            self.calc_auto_scale();
            // Restore user's scale mode preference if LoadPreset set one.
            if let Some(mode) = self.restore_scale_mode.take() {
                self.price_scale.scale_mode = mode;
            }
            self.needs_auto_scale_after_bars = false;
        } else {
            // Deferred: chart_width not yet set (first-launch, brand-new window).
            // prepare_frame() will run the snap once layout sets real dimensions.
            self.needs_auto_scale_after_bars = true;
        }
    }

    /// Replace bars without resetting viewport position or scale mode.
    ///
    /// Used for backfill / WebSocket reconnect — the user's current pan/zoom
    /// and scale mode are preserved.  Only `bar_count` and derived data
    /// (prev-close) are recalculated.
    pub fn update_bars(&mut self, bars: Vec<Bar>) {
        self.bars = bars;

        self.update_prev_close_line();

        self.viewport.bar_count = self.bars.len();
        // NOTE: do NOT call snap_to_end here — all three callers
        // (BarsLoaded backfill, BackfillComplete, ScrollBarsLoaded) explicitly
        // preserve the user's pan/zoom. Snapping to the end on every prepend
        // makes scroll-left appear to "jump back to newest" once the user has
        // scrolled past the snap margin.

        if self.price_scale.scale_mode.is_auto_y() {
            self.calc_auto_scale();
        }
    }

    /// Get bar count
    pub fn bar_count(&self) -> usize {
        self.bars.len()
    }

    /// Check if window has data
    pub fn has_data(&self) -> bool {
        !self.bars.is_empty()
    }

    /// Clone data from another window (bars, viewport settings).
    ///
    /// Instrument fields (symbol/timeframe/exchange/account_type) live in
    /// `branch_meta` in chart-app; the caller is responsible for mirroring
    /// the source window's instrument into branch_meta for the dest chart_id.
    ///
    /// `dst_panel` is the destination window's `ChartPanelState`; this method
    /// calls `dst_panel.drawing_manager.set_current_symbol_key` on it (S2.8).
    pub fn clone_data_from(
        &mut self,
        source: &ChartContainer,
        symbol: &str,
        exchange: &str,
        account_type: &str,
        dst_panel: &mut ChartPanelState,
    ) {
        self.bars = source.bars.clone();

        dst_panel.drawing_manager.set_current_symbol_key(symbol, exchange, account_type);

        self.viewport = source.viewport.clone();
        self.price_scale = source.price_scale.clone();

        self.scale_settings = source.scale_settings.clone();
        self.price_scale.user_precision = self.scale_settings.user_precision;

        eprintln!("[ChartContainer] Cloned data from window {}: {} bars", source.id, self.bars.len());
    }

    /// Create ChartRenderState from this window's data.
    ///
    /// `grid` and `legend` are supplied by the caller from `ChartPanelState`
    /// (they were removed from `ChartContainer` in Phase 4 C).
    ///
    /// `timeframe_minutes` is provided by the caller from `branch_meta` via
    /// `read_timeframe(...).map(|t| t.minutes)` — the field was removed from
    /// `ChartContainer` in Phase 4 B.
    pub fn to_render_state<'a>(
        &'a self,
        chart_rect: crate::chart::render::ChartRect,
        theme: &'a crate::chart::render::ChartTheme,
        current_timeframe: Option<&'a str>,
        time_format_settings: Option<&'a crate::TimeFormatSettings>,
        timeframe_minutes: Option<u32>,
        grid: &'a GridOptions,
        legend: &'a Legend,
    ) -> crate::chart::render::ChartRenderState<'a> {
        crate::chart::render::ChartRenderState {
            viewport: &self.viewport,
            price_scale: &self.price_scale,
            time_scale: &self.time_scale,
            bars: &self.bars,
            grid,
            crosshair: &self.crosshair,
            legend,
            chart_rect,
            theme,
            time_ticks: None,
            current_timeframe,
            disable_clip: false,
            time_format_settings,
            timeframe_minutes,
            scale_settings: Some(&self.scale_settings),
            body_enabled: true,
            border_enabled: true,
            wick_enabled: true,
            use_prev_close: false,
        }
    }

    /// Create ChartRenderConfig from this window's options and render flags.
    ///
    /// `crosshair_options` and `chart_type` are supplied by the caller from
    /// `ChartPanelState` (removed from `ChartContainer` in Phase 4 C).
    /// Create a minimal clone of this chart window for use in `panel_grid::split_active`.
    ///
    /// Unlike `clone_for_split`, this variant does **not** take a source
    /// `DrawingManager` or `ChartPanelState`.  It creates a fresh `ChartContainer`
    /// with an empty drawing_manager hint (just the window-id set).  The caller
    /// (chart-app `sync_group.rs`) is responsible for seeding the real
    /// `ChartPanelState` (including the `drawing_manager`) into `panels_store`
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
        let new_panel_id = crate::state::chart_container_leaf::generate_chart_panel_id();

        Self {
            id: new_chart_id,
            chart_panel_id: new_panel_id,
            // Sentinel — do_split must detect BubbleId(0) and create a fresh
            // auto-bubble for every chart that clone_for_split_shell produces.
            auto_bubble_id: BubbleId(0),
            current_bubble_id: BubbleId(0),

            bars: self.bars.clone(),
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
            pending_symbol_load: false,
            needs_auto_scale_after_bars: false,
            restore_scale_mode: None,
            stashed_scale_mode: None,
            scroll_fetch_in_flight: false,
            scroll_fetch_started: None,
            inner_docking: make_inner_docking_with_main_chart(new_panel_id),
        }
    }

    /// `is_drawing_dragging` should be `panel.drawing_manager.is_dragging()` where
    /// `panel` is the `ChartPanelState` for this window (S2.8 migration).
    pub fn to_render_config(
        &self,
        scale_theme: crate::chart::render::ScaleTheme,
        render_crosshair: bool,
        crosshair_options: &CrosshairOptions,
        chart_type: &'static str,
        is_drawing_dragging: bool,
    ) -> crate::layout::ChartRenderConfig {
        let crosshair_config = if render_crosshair {
            crate::chart::render::CrosshairConfig {
                vert_visible: crosshair_options.vert_line.visible,
                vert_width: crosshair_options.vert_line.width,
                vert_style: crosshair_options.vert_line.style,
                horz_visible: crosshair_options.horz_line.visible,
                horz_width: crosshair_options.horz_line.width,
                horz_style: crosshair_options.horz_line.style,
            }
        } else {
            crate::chart::render::CrosshairConfig {
                vert_visible: false,
                vert_width: crosshair_options.vert_line.width,
                vert_style: crosshair_options.vert_line.style,
                horz_visible: false,
                horz_width: crosshair_options.horz_line.width,
                horz_style: crosshair_options.horz_line.style,
            }
        };

        crate::layout::ChartRenderConfig {
            scale_config: crate::chart::render::ScaleConfig::default(),
            scale_theme,
            crosshair_config,
            is_dragging: self.drag_mode.is_dragging() || is_drawing_dragging,
            chart_type,
        }
    }

    /// Create ScaleCornerState from this window's data
    pub fn to_corner_state(&self) -> crate::layout::ScaleCornerState {
        crate::layout::ScaleCornerState {
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
        layout: &crate::layout::ExtendedFrameLayout,
        drag_pane: Option<Option<usize>>,
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
                            Some(pane_idx), main_chart.chart.width, true,
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
                        None, main_chart.chart.width, true,
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
                    Some(idx), main_chart.chart.width, false,
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
                None, main_chart.chart.width, false,
            );
            return true;
        }

        // Cursor is on a separator or outside all chart areas — hide crosshair.
        self.crosshair.visible = false;
        false
    }

    /// Internal crosshair update with all computed values
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
                bar_idx, price, pane_height, price_min, price_max,
            );
            self.crosshair.set_snapped(snapped_price, snapped_y);
        } else {
            self.crosshair.snapped_y = self.crosshair.y;
            self.crosshair.snapped_price = price;
        }
    }

    /// Centralized magnet snap calculation.
    pub fn calculate_magnet_snap(
        &self,
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
            if idx < self.bars.len() {
                let bar = &self.bars[idx];
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
        self.crosshair.bar_idx = if bar_f64 >= 0.0 && (bar_f64 as usize) < self.bars.len() {
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
    pub fn set_crosshair_from_timestamp(&mut self, timestamp: i64, price: f64, visible: bool, pane_index: Option<usize>) {
        let bar_f64 = self.timestamp_to_bar_f64(timestamp);
        self.set_crosshair_from_bar(bar_f64, price, visible, pane_index);
    }

    /// Convert a UTC timestamp (unix seconds) to a fractional bar index using
    /// binary search.  Interpolates between bars and extrapolates beyond the
    /// edges of the loaded bar array.
    fn timestamp_to_bar_f64(&self, timestamp: i64) -> f64 {
        if self.bars.is_empty() {
            return 0.0;
        }
        if self.bars.len() == 1 {
            return 0.0;
        }

        let first_ts = self.bars[0].timestamp;
        let last_ts = self.bars[self.bars.len() - 1].timestamp;

        // Before first bar — extrapolate backwards.
        if timestamp <= first_ts {
            let interval = self.bars[1].timestamp - first_ts;
            if interval > 0 {
                return (timestamp - first_ts) as f64 / interval as f64;
            }
            return 0.0;
        }

        // After last bar — extrapolate forwards.
        if timestamp >= last_ts {
            let interval = last_ts - self.bars[self.bars.len() - 2].timestamp;
            if interval > 0 {
                return (self.bars.len() - 1) as f64
                    + (timestamp - last_ts) as f64 / interval as f64;
            }
            return (self.bars.len() - 1) as f64;
        }

        // Binary search for an exact match or interpolation point.
        match self.bars.binary_search_by_key(&timestamp, |b| b.timestamp) {
            Ok(idx) => idx as f64,
            Err(idx) => {
                // Between bars[idx-1] and bars[idx] — interpolate.
                if idx > 0 && idx < self.bars.len() {
                    let lo_ts = self.bars[idx - 1].timestamp;
                    let hi_ts = self.bars[idx].timestamp;
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
    pub fn add_compare_symbol(&mut self, symbol: &str, main_symbol: &str, main_timeframe: &Timeframe) -> bool {
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

        if self.compare_overlay.series.len() == 1 && !self.bars.is_empty() {
            let (start, _) = self.viewport.visible_range();
            let base_idx = start.min(self.bars.len().saturating_sub(1));
            let base_bar = &self.bars[base_idx];
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

    /// Update previous close price line based on settings
    pub fn update_prev_close_line(&mut self) {
        self.price_lines.remove("prev_close");

        if self.scale_settings.show_prev_close_line {
            // Phase 4 A — derive from bars[0].open instead of cached field.
            if let Some(prev_close) = self.bars.first().map(|b| b.open) {
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
    pub fn zoom_in(&mut self) {
        let center_x = self.viewport.chart_width / 2.0;
        self.viewport.zoom_at(center_x, 1.1);
        self.calc_auto_scale();
    }

    /// Zoom out at chart center (0.9x bar spacing).
    pub fn zoom_out(&mut self) {
        let center_x = self.viewport.chart_width / 2.0;
        self.viewport.zoom_at(center_x, 0.9);
        self.calc_auto_scale();
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
    pub fn snap_to_end(&mut self, margin: f64) {
        let visible_f = self.viewport.chart_width / self.viewport.bar_spacing;
        let count = self.bars.len();
        self.viewport.view_start = (count as f64 + margin - visible_f).max(0.0);
    }

    /// Fit all bars into the visible chart area.
    pub fn fit_content(&mut self) {
        let bar_count = self.bars.len();
        if bar_count > 0 {
            self.viewport.bar_spacing = self.viewport.chart_width / bar_count as f64;
            self.viewport.bar_spacing = self.viewport.bar_spacing.clamp(1.0, 30.0);
            self.snap_to_end(DEFAULT_SNAP_MARGIN);
            self.calc_auto_scale();
        }
    }

    /// Reset zoom to default bar spacing (8px) and scroll to end.
    pub fn reset_zoom(&mut self) {
        self.viewport.bar_spacing = 8.0;
        self.snap_to_end(DEFAULT_SNAP_MARGIN);
        self.calc_auto_scale();
    }

    // =========================================================================
    // Split-aware scroll helpers (called when routing scroll to sub-charts)
    // =========================================================================

    /// Pan the viewport horizontally by `bar_delta` pixels.
    ///
    /// Converts pixel delta to bars using the current bar spacing, then
    /// delegates to [`Viewport::pan`] which handles clamping.
    pub fn pan_horizontal(&mut self, pixel_delta: f64) {
        if self.viewport.bar_spacing > 0.0 {
            let bar_delta = pixel_delta / self.viewport.bar_spacing;
            self.viewport.pan(bar_delta);
            if self.price_scale.scale_mode.is_auto_y() {
                self.calc_auto_scale();
            }
        }
    }

    /// Zoom the viewport horizontally around `center_x` (local pixel coord).
    ///
    /// `factor` > 1.0 zooms in (more bars per pixel), < 1.0 zooms out.
    /// Delegates to [`Viewport::zoom_at`] which handles clamping.
    pub fn zoom_horizontal(&mut self, center_x: f64, factor: f64) {
        self.viewport.zoom_at(center_x, factor);
        if self.price_scale.scale_mode.is_auto_y() {
            self.calc_auto_scale();
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

    /// Execute a chart action, mutating chart state and panel display state.
    ///
    /// Returns `true` if the action was handled by this window, `false` if it
    /// requires app-level handling.
    pub fn execute_chart_action(&mut self, panel: &mut ChartPanelState, action: &crate::ChartAction) -> bool {
        use crate::ChartAction;
        match action {
            // === Series / Chart Type ===
            ChartAction::SetChartType(ct) => {
                self.set_chart_type_with_undo(panel, ct);
                true
            }

            // === Toggles ===
            ChartAction::ToggleLegend => { self.toggle_legend(panel); true }
            ChartAction::ToggleGrid => { self.toggle_grid(panel); true }
            ChartAction::ToggleCrosshair => { self.toggle_crosshair(); true }
            ChartAction::ToggleMagnet => { self.crosshair.toggle_magnet(); true }
            ChartAction::ToggleWatermark => { self.toggle_watermark(panel); true }
            ChartAction::ToggleGridVertical => { self.toggle_grid_vertical(panel); true }
            ChartAction::ToggleGridHorizontal => { self.toggle_grid_horizontal(panel); true }
            ChartAction::ToggleTooltip => { self.toggle_tooltip(panel); true }
            ChartAction::ToggleTooltipFollow => { self.toggle_tooltip_follow(panel); true }
            ChartAction::ToggleCrosshairVertLine => { self.toggle_crosshair_vert_line(panel); true }
            ChartAction::ToggleCrosshairHorzLine => { self.toggle_crosshair_horz_line(panel); true }
            ChartAction::ToggleLegendOHLC => { self.toggle_legend_ohlc(panel); true }
            ChartAction::ToggleLegendChange => { self.toggle_legend_change(panel); true }
            ChartAction::ToggleLegendPercent => { self.toggle_legend_percent(panel); true }

            // === Setters ===
            ChartAction::SetGridStyle(s) => { self.set_grid_style(panel, *s); true }
            ChartAction::SetCrosshairMode(m) => { self.set_crosshair_mode(panel, *m); true }
            ChartAction::SetCrosshairStyle(s) => { self.set_crosshair_style(panel, *s); true }
            ChartAction::SetLegendPosition(p) => { self.set_legend_position(panel, *p); true }
            ChartAction::SetWatermarkPosition(h, v) => { self.set_watermark_position(panel, *h, *v); true }
            ChartAction::SetWatermarkColor(c) => { self.set_watermark_color(panel, c); true }
            ChartAction::SetWatermarkText(t) => { self.set_watermark_text(panel, t); true }

            // === Zoom ===
            ChartAction::ZoomIn => { self.zoom_in(); true }
            ChartAction::ZoomOut => { self.zoom_out(); true }
            ChartAction::FitContent => { self.fit_content(); true }
            ChartAction::ResetZoom => { self.reset_zoom(); true }

            // === Drawing (drawing_manager now lives in ChartPanelState — S2.8) ===
            ChartAction::SelectTool(tool) => { panel.drawing_manager.set_tool(Some(tool)); true }
            ChartAction::ToggleLockDrawings => { panel.drawing_manager.toggle_lock(); true }
            ChartAction::ToggleDrawingsVisible => { panel.drawing_manager.toggle_visible(); true }
            ChartAction::DeleteSelected => { panel.drawing_manager.delete_selected(); true }
            ChartAction::DeleteAll => { panel.drawing_manager.clear(); true }

            // === Undo / Redo ===
            ChartAction::Undo => self.command_history.can_undo(),
            ChartAction::Redo => self.command_history.can_redo(),

            // === Actions not handled at window level (app-level, dialogs, etc.) ===
            _ => false,
        }
    }
}

impl Default for ChartContainer {
    fn default() -> Self {
        Self::new("BTCUSD", Timeframe::h1())
    }
}
