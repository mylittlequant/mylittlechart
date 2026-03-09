//! Chart Window - Multi-window chart support
//!
//! Each ChartWindow represents an independent chart view with its own:
//! - Data (bars, indicators)
//! - Viewport (pan/zoom state)
//! - Price scale (Y-axis)
//! - Sub-panes (indicator panels)
//! - Symbol and timeframe
//! - Drawing primitives
//! - Indicators (via IndicatorSource trait object)
//! - All display options
//!
//! Multiple ChartWindow instances can be synced via SyncMode settings.

use std::collections::HashMap;
use std::sync::Arc;

use crate::{Bar, Viewport, PriceScale, TimeScale};
use crate::chart::types::price_scale::ScaleMode;
use crate::state::Timeframe;
use crate::{Crosshair, CrosshairOptions, KineticState, DragMode};
use crate::{GridOptions, Legend, Watermark, Tooltip, PriceLine, MarkerManager};
use crate::drawing::{DrawingManager, SignalManager, TradeManager};
use crate::chart::{CompareOverlay, CompareSeries, get_compare_color};
use crate::state::{SubPane, PaneManager, CommandHistory, Command};
use crate::data_provider::{DataProvider, SharedDataProvider, NullDataProvider};
use crate::scale_settings::ScaleSettings;
use crate::indicator_source::{IndicatorSource, NullIndicatorSource};
use crate::panel_app::{ChartToolbarState, ToolbarConfig};
use crate::layout::LayoutRect;
use crate::chart::render::ChartRect;
use crate::scale_settings::{PriceScalePosition, TimeScalePosition};

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

/// Saved viewport and price-scale state that should be applied once bars arrive.
///
/// Created during `LoadPreset` when the data provider cannot supply bars
/// synchronously. After `BarsLoaded` calls `set_bars()` (which resets the
/// viewport), the deferred snapshot is applied to restore the preset position.
#[derive(Clone, Debug)]
pub struct ViewportSnapshot {
    /// Saved `view_start` (horizontal scroll position).
    pub view_start: f64,
    /// Saved `bar_spacing` (horizontal zoom level).
    pub bar_spacing: f64,
    /// Saved `price_min` (bottom of Y axis).
    pub price_min: f64,
    /// Saved `price_max` (top of Y axis).
    pub price_max: f64,
    /// Saved scale mode (Auto / Manual / etc.).
    pub scale_mode: ScaleMode,
}

/// Individual chart window with its own state
///
/// Each window is a fully independent chart with its own data, scales,
/// drawings, indicators, and display options.
pub struct ChartWindow {
    /// Unique chart identifier (ChartId)
    pub id: ChartId,

    /// Display title (e.g., "BTCUSD 1H")
    pub title: String,

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
    /// Sub-panes for indicators
    pub sub_panes: Vec<SubPane>,
    /// Pane manager for layout
    pub pane_manager: PaneManager,
    /// Total chart height including sub-panes
    pub total_chart_height: f64,

    // === Symbol & Timeframe ===
    /// Current symbol (ticker)
    pub symbol: String,
    /// Current timeframe
    pub timeframe: Timeframe,
    /// Exchange name (e.g. "Binance", "Demo")
    pub exchange: String,

    // === Computed Data ===
    /// Fast moving average
    pub ma_fast: Vec<f64>,
    /// Slow moving average
    pub ma_slow: Vec<f64>,

    // === Interaction State ===
    /// Whether this window is active/focused
    pub is_active: bool,
    /// Active pane index: None = main chart, Some(n) = sub-pane n
    pub active_pane_index: Option<usize>,
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

    // === Display Options ===
    /// Grid display options
    pub grid_options: GridOptions,
    /// Crosshair display options
    pub crosshair_options: CrosshairOptions,
    /// Legend display state
    pub legend: Legend,
    /// Watermark display
    pub watermark: Option<Watermark>,
    /// Tooltip display state and config
    pub tooltip: Tooltip,

    // === Object Managers ===
    /// Drawing primitives (trend lines, rectangles, etc.)
    pub drawing_manager: DrawingManager,
    /// Signal markers from strategies
    pub signal_manager: SignalManager,
    /// Trade visualization
    pub trade_manager: TradeManager,
    /// Indicator data source (trait object - no direct IndicatorManager dependency)
    pub indicator_source: Box<dyn IndicatorSource>,
    /// Chart markers
    pub marker_manager: MarkerManager,
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

    // === Series Visibility ===
    /// Show candlesticks
    pub show_candles: bool,
    /// Show OHLC bars with ticks
    pub show_bars: bool,
    /// Show hollow candles (bullish=outline, bearish=filled)
    pub show_hollow_candles: bool,
    /// Show Heikin Ashi smoothed candles
    pub show_heikin_ashi: bool,
    /// Show line series
    pub show_line: bool,
    /// Show step line (staircase)
    pub show_step_line: bool,
    /// Show line with dot markers
    pub show_line_markers: bool,
    /// Show area series
    pub show_area: bool,
    /// Show HLC area (high-low-close)
    pub show_hlc_area: bool,
    /// Show histogram
    pub show_histogram: bool,
    /// Show columns (vertical bars from baseline)
    pub show_columns: bool,
    /// Show baseline
    pub show_baseline: bool,
    /// Active chart type name (e.g. "candles", "bars", "line", "area")
    pub chart_type: &'static str,

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

    // === Previous Close ===
    /// Previous session close price (for prev close line)
    pub prev_close_price: Option<f64>,

    /// Connection/data feed status
    pub connection_status: ConnectionStatus,

    /// Sync group this window belongs to (set by TagManager during split/join/desync).
    pub group_id: Option<crate::tag_manager::SyncGroupId>,

    /// Indicator instance IDs that existed on this window BEFORE it joined a tag.
    /// On desync, only indicators NOT in this set are removed (they came from the tag).
    /// Empty for split children (they had nothing before the tag).
    pub pre_tag_indicator_ids: Vec<u64>,
    /// Stashed primitives: window's own primitives hidden when joining an existing tag.
    /// Restored on desync. Empty for new-tag creation (seed flow) and split children.
    pub stashed_primitives: Vec<Box<dyn crate::drawing::primitives_v2::Primitive>>,
    /// Stashed command history: the window's own `command_history` saved when the
    /// window joins a sync group. While in a group, shared commands go into the
    /// group's `command_history`; on desync the window-local history is restored.
    pub stashed_command_history: Option<CommandHistory>,

    /// Per-symbol drawing cache. When the user switches symbols, the current
    /// drawings are snapshotted here keyed by the old symbol, and drawings
    /// for the new symbol are restored (if any).
    pub symbol_drawings: std::collections::HashMap<String, Vec<crate::preset::snapshots::PrimitiveSnapshot>>,

    /// Deferred viewport / price-scale position to apply once `BarsLoaded` fires.
    ///
    /// Set during `LoadPreset` when bars cannot be fetched synchronously.
    /// `set_bars()` resets the viewport, so the desired position must be
    /// re-applied after bars arrive. Cleared in the `BarsLoaded` handler.
    pub pending_viewport_restore: Option<ViewportSnapshot>,
}

impl ChartWindow {
    /// Create a new chart window with a data provider.
    pub fn new_with_provider(symbol: &str, timeframe: Timeframe, data_provider: SharedDataProvider) -> Self {
        let id = generate_chart_id();
        let mut drawing_manager = DrawingManager::new();
        drawing_manager.set_current_window(Some(id.0));

        Self {
            id,
            title: format!("{} {}", symbol, timeframe.name.clone()),
            bars: Vec::new(),
            viewport: Viewport::default(),
            price_scale: PriceScale::default(),
            time_scale: TimeScale::new(),
            sub_panes: Vec::new(),
            pane_manager: PaneManager::new(),
            total_chart_height: 0.0,
            symbol: symbol.to_string(),
            timeframe,
            exchange: data_provider.exchange_name(symbol),
            ma_fast: Vec::new(),
            ma_slow: Vec::new(),
            // Interaction
            is_active: false,
            active_pane_index: None,
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
            // Display options
            grid_options: GridOptions::default(),
            crosshair_options: CrosshairOptions::default(),
            legend: Legend::default(),
            watermark: Some(Watermark::default()),
            tooltip: Tooltip::default(),
            // Object managers
            drawing_manager,
            signal_manager: SignalManager::new(),
            trade_manager: TradeManager::new(),
            indicator_source: Box::new(NullIndicatorSource),
            marker_manager: MarkerManager::new(),
            price_lines: HashMap::new(),
            // Compare overlay (per-window)
            compare_overlay: CompareOverlay::new(),
            // Command history (250 undo levels per window)
            command_history: CommandHistory::new(250),
            // Data provider
            data_provider,
            // Series visibility
            show_candles: true,
            show_bars: false,
            show_hollow_candles: false,
            show_heikin_ashi: false,
            show_line: false,
            show_step_line: false,
            show_line_markers: false,
            show_area: false,
            show_hlc_area: false,
            show_histogram: false,
            show_columns: false,
            show_baseline: false,
            chart_type: "candles",
            // Scale settings
            scale_settings: ScaleSettings::default(),
            // Toolbar state
            chart_toolbar: ChartToolbarState::default(),
            // Toolbar config (DEBUG: standalone mode with all 4 toolbars)
            toolbar_config: ToolbarConfig::standalone(),
            // Previous close
            prev_close_price: None,
            // Connection status
            connection_status: ConnectionStatus::Live,
            // Sync group (not yet assigned)
            group_id: None,
            pre_tag_indicator_ids: Vec::new(),
            stashed_primitives: Vec::new(),
            stashed_command_history: None,
            symbol_drawings: HashMap::new(),
            pending_viewport_restore: None,
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

    /// Update title based on current symbol and timeframe
    pub fn update_title(&mut self) {
        self.title = format!("{} {}", self.symbol, self.timeframe.name);
    }

    /// Snapshot current drawings into the per-symbol cache for the given symbol.
    ///
    /// If there are no drawings, any existing cache entry for the symbol is removed.
    pub fn snapshot_drawings_for_symbol(&mut self, symbol: &str) {
        let snapshots: Vec<crate::preset::snapshots::PrimitiveSnapshot> = self
            .drawing_manager
            .primitives()
            .iter()
            .map(|p| crate::preset::snapshots::PrimitiveSnapshot::from_primitive(p.as_ref()))
            .collect();
        if !snapshots.is_empty() {
            self.symbol_drawings.insert(symbol.to_string(), snapshots);
        } else {
            self.symbol_drawings.remove(symbol);
        }
    }

    /// Restore drawings from the per-symbol cache for the given symbol.
    ///
    /// Returns `true` if drawings were restored, `false` if no cache entry exists.
    pub fn restore_drawings_for_symbol(&mut self, symbol: &str) -> bool {
        if let Some(snapshots) = self.symbol_drawings.get(symbol).cloned() {
            if let Ok(reg) = crate::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                for snap in &snapshots {
                    if let Some(prim) = reg.from_json(&snap.type_id, &snap.json) {
                        self.drawing_manager.add_external_primitive(prim);
                    }
                }
            }
            true
        } else {
            false
        }
    }

    /// Store the active chart type so the chart can read it without TerminalApp.
    pub fn set_chart_type(&mut self, chart_type: &'static str) {
        self.chart_type = chart_type;
    }

    /// Create a clone of this chart window for split operation.
    ///
    /// Copies symbol, timeframe, viewport settings, scale settings, indicators.
    /// Gets a fresh ChartId.
    ///
    /// When `sync_drawings` is `true`, all existing primitives are cloned into the
    /// new window as synced copies (`origin_id` is set to the original's id).
    /// When `false`, the new window starts with an empty drawing manager.
    pub fn clone_for_split(&self, new_chart_id: ChartId, sync_drawings: bool) -> Self {
        let mut drawing_manager = DrawingManager::new();
        drawing_manager.set_current_window(Some(new_chart_id.0));
        if sync_drawings {
            let synced = self.drawing_manager.clone_primitives_for_sync(new_chart_id.0);
            drawing_manager.add_synced_primitives(synced);
        }

        Self {
            // New chart ID
            id: new_chart_id,

            // Generate new title from symbol/timeframe
            title: format!("{} {}", self.symbol, self.timeframe.name),

            // === Data (cloned) ===
            bars: self.bars.clone(),

            // === Viewport & Scales (cloned) ===
            viewport: self.viewport.clone(),
            price_scale: self.price_scale.clone(),
            time_scale: self.time_scale.clone(),

            // === Sub-panes (cloned) ===
            // Clone sub_panes so the new window has correct layout immediately.
            // sync_sub_panes_from_manager will reconcile instance IDs on the next tick.
            sub_panes: self.sub_panes.clone(),
            pane_manager: self.pane_manager.clone(),
            total_chart_height: self.total_chart_height,

            // === Symbol & Timeframe (cloned) ===
            symbol: self.symbol.clone(),
            timeframe: self.timeframe.clone(),
            exchange: self.exchange.clone(),

            // === Computed Data (cloned) ===
            ma_fast: self.ma_fast.clone(),
            ma_slow: self.ma_slow.clone(),

            // === Interaction State (reset) ===
            is_active: false,
            active_pane_index: None,
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

            // === Display Options (cloned) ===
            grid_options: self.grid_options.clone(),
            crosshair_options: self.crosshair_options.clone(),
            legend: self.legend.clone(),
            watermark: self.watermark.clone(),
            tooltip: self.tooltip.clone(),

            // === Object Managers ===
            drawing_manager,
            signal_manager: SignalManager::new(),
            trade_manager: TradeManager::new(),
            indicator_source: Box::new(NullIndicatorSource), // Fresh (no indicators copied)
            marker_manager: MarkerManager::new(),
            price_lines: self.price_lines.clone(),

            // === Compare Overlay (reset) ===
            compare_overlay: CompareOverlay::new(),

            // === Command History (reset) ===
            command_history: CommandHistory::new(250),

            // === Data Provider (shared) ===
            data_provider: self.data_provider.clone(),

            // === Series Visibility (cloned) ===
            show_candles: self.show_candles,
            show_bars: self.show_bars,
            show_hollow_candles: self.show_hollow_candles,
            show_heikin_ashi: self.show_heikin_ashi,
            show_line: self.show_line,
            show_step_line: self.show_step_line,
            show_line_markers: self.show_line_markers,
            show_area: self.show_area,
            show_hlc_area: self.show_hlc_area,
            show_histogram: self.show_histogram,
            show_columns: self.show_columns,
            show_baseline: self.show_baseline,
            chart_type: self.chart_type,

            // === Scale Settings (cloned) ===
            scale_settings: self.scale_settings.clone(),

            // === Toolbar State (reset for new panel) ===
            chart_toolbar: ChartToolbarState::default(),

            // === Toolbar Config (sub-windows have no toolbar — parent owns it) ===
            toolbar_config: ToolbarConfig::minimal(),

            // === Previous Close ===
            prev_close_price: self.prev_close_price,

            // === Connection Status ===
            connection_status: self.connection_status,

            // === Sync Group (inherited from source) ===
            group_id: self.group_id,
            // Split child has no pre-tag state — everything comes from the tag.
            pre_tag_indicator_ids: Vec::new(),
            stashed_primitives: Vec::new(),
            // Split child starts with no stashed history.
            stashed_command_history: None,
            // Split child does not inherit per-symbol drawing cache.
            symbol_drawings: HashMap::new(),
            // Split child has no pending viewport restore.
            pending_viewport_restore: None,
        }
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
    pub fn legend_for_render(&self, render_legend: bool) -> Legend {
        if render_legend {
            self.legend.clone()
        } else {
            let mut legend = self.legend.clone();
            legend.visible = false;
            legend
        }
    }

    /// Build the main chart rect for rendering within a window layout.
    pub fn chart_rect_for_render(
        &self,
        chart_panel: &LayoutRect,
        window_rect: &WindowRect,
    ) -> ChartRect {
        const SUB_PANE_HEIGHT: f64 = 100.0;
        const SEPARATOR_HEIGHT: f64 = 1.0;

        let sub_pane_count = self
            .indicator_source
            .get_instances_for_symbol(&self.symbol)
            .into_iter()
            .filter(|i| i.visible && i.pane_index > 0)
            .count();

        let sub_panes_height = if sub_pane_count > 0 {
            (SUB_PANE_HEIGHT + SEPARATOR_HEIGHT) * sub_pane_count as f64
        } else {
            0.0
        };

        // Account for scale positions
        let price_width = self.scale_settings.effective_price_scale_width();
        let time_height = self.scale_settings.effective_time_scale_height();

        let available_height = window_rect.height as f64 - time_height;
        let main_chart_height = (available_height - sub_panes_height).max(200.0);

        let chart_x = match self.scale_settings.price_scale_position {
            PriceScalePosition::Left => chart_panel.x + window_rect.x as f64 + price_width,
            _ => chart_panel.x + window_rect.x as f64,
        };

        let chart_y = match self.scale_settings.time_scale_position {
            TimeScalePosition::Top => chart_panel.y + window_rect.y as f64 + time_height,
            _ => chart_panel.y + window_rect.y as f64,
        };

        let chart_width = window_rect.width as f64 - price_width;

        ChartRect::new(
            chart_x,
            chart_y,
            chart_width,
            main_chart_height,
        )
    }

    /// Calculate auto-scale for price axis based on visible data
    pub fn calc_auto_scale(&mut self) {
        self.price_scale.calc_auto_scale(
            &self.bars,
            self.viewport.visible_range(),
            &self.ma_fast,
            &self.ma_slow,
        );
    }

    /// Calculate moving averages from bar data
    pub fn calc_moving_averages(&mut self) {
        self.ma_fast.clear();
        self.ma_slow.clear();

        for i in 0..self.bars.len() {
            // MA10 (fast)
            if i >= 9 {
                let sum: f64 = self.bars[i.saturating_sub(9)..=i]
                    .iter()
                    .map(|b| b.close)
                    .sum();
                self.ma_fast.push(sum / 10.0);
            } else {
                self.ma_fast.push(f64::NAN);
            }

            // MA20 (slow)
            if i >= 19 {
                let sum: f64 = self.bars[i.saturating_sub(19)..=i]
                    .iter()
                    .map(|b| b.close)
                    .sum();
                self.ma_slow.push(sum / 20.0);
            } else {
                self.ma_slow.push(f64::NAN);
            }
        }
    }

    /// Set bars and recalculate derived data
    pub fn set_bars(&mut self, bars: Vec<Bar>) {
        self.bars = bars;
        self.calc_moving_averages();

        // Calculate prev_close (use first bar's open as proxy for previous session close)
        if !self.bars.is_empty() {
            self.prev_close_price = Some(self.bars[0].open);
        } else {
            self.prev_close_price = None;
        }

        // Update prev close line if enabled
        self.update_prev_close_line();

        // Update viewport
        let count = self.bars.len();
        self.viewport.bar_count = count;
        // Position so last bar has ~5 bars of right margin (polling space)
        let visible = self.viewport.visible_bars();
        let right_margin: usize = 5;
        if count + right_margin > visible {
            self.viewport.view_start = (count + right_margin - visible) as f64;
        } else {
            self.viewport.view_start = 0.0;
        }
        self.price_scale.scale_mode = ScaleMode::Auto;
        self.calc_auto_scale();
    }

    /// Change symbol (requires data reload)
    pub fn set_symbol(&mut self, symbol: &str) {
        self.symbol = symbol.to_string();
        self.update_title();
    }

    /// Change symbol and load data from the configured provider.
    ///
    /// Returns `true` if symbol was changed, `false` if data not available.
    pub fn change_symbol(&mut self, symbol: &str) -> bool {
        let Some(new_bars) = self.data_provider.get_bars(symbol, &self.timeframe) else {
            eprintln!(
                "[ChartWindow] Data not available for symbol={} tf={}",
                symbol,
                self.timeframe.name
            );
            return false;
        };
        if new_bars.is_empty() {
            eprintln!("[ChartWindow] No bars loaded for {}", symbol);
            return false;
        }

        self.symbol = symbol.to_string();
        self.exchange = self.data_provider.exchange_name(symbol);
        self.update_title();
        self.set_bars(new_bars);
        self.viewport.scroll_to_end();

        eprintln!(
            "[ChartWindow] Changed to {} ({} bars)",
            symbol,
            self.bars.len()
        );
        true
    }

    /// Change timeframe and load data from the configured provider.
    ///
    /// Returns `true` if timeframe was changed, `false` if data not available.
    pub fn change_timeframe(&mut self, timeframe: Timeframe) -> bool {
        let Some(new_bars) = self.data_provider.get_bars(&self.symbol, &timeframe) else {
            eprintln!(
                "[ChartWindow] Data not available for symbol={} tf={}",
                self.symbol,
                timeframe.name
            );
            return false;
        };
        if new_bars.is_empty() {
            eprintln!("[ChartWindow] No bars loaded for {} at {:?}", self.symbol, timeframe);
            return false;
        }

        self.timeframe = timeframe;
        self.update_title();
        self.set_bars(new_bars);

        // Recalculate primitive bar caches for new timeframe
        self.drawing_manager.recalculate_all_bar_caches(&self.bars);

        self.viewport.scroll_to_end();

        eprintln!(
            "[ChartWindow] Changed timeframe to {} ({} bars)",
            self.timeframe.name,
            self.bars.len()
        );
        true
    }

    /// Change timeframe (requires data reload)
    pub fn set_timeframe(&mut self, timeframe: Timeframe) {
        self.timeframe = timeframe;
        self.update_title();
    }

    /// Synchronize sub_panes with indicator_source
    ///
    /// Creates/removes SubPane entries to match indicators with pane > 0.
    /// Preserves existing Y-axis state for panes that still exist.
    pub fn sync_sub_panes(&mut self) {
        // Get all indicator instances that need sub-panes (pane > 0)
        let sub_pane_indicators: Vec<u64> = self.indicator_source
            .get_instances_for_symbol(&self.symbol)
            .into_iter()
            .filter(|i| i.visible && i.pane_index > 0)
            .map(|i| i.id)
            .collect();

        // Get visible range for price calculation
        let (visible_start, visible_end) = self.viewport.visible_range();
        let visible_end = visible_end.min(self.bars.len());

        // Build new sub_panes list, preserving existing state where possible
        let mut new_sub_panes = Vec::with_capacity(sub_pane_indicators.len());

        for (index, &instance_id) in sub_pane_indicators.iter().enumerate() {
            // Try to find existing sub_pane with this instance_id
            if let Some(existing) = self.sub_panes.iter().find(|p| p.instance_id == instance_id) {
                let mut pane = existing.clone();
                pane.index = index;
                new_sub_panes.push(pane);
            } else {
                let mut pane = SubPane::new(instance_id);
                pane.index = index;

                // Initialize price_min/max from indicator data
                if let Some((p_min, p_max)) = self.indicator_source.calculate_pane_range(instance_id, visible_start, visible_end) {
                    pane.price_min = p_min;
                    pane.price_max = p_max;
                }

                new_sub_panes.push(pane);
            }
        }

        self.sub_panes = new_sub_panes;
    }

    /// Get sub_pane by index (if exists)
    pub fn get_sub_pane(&self, index: usize) -> Option<&SubPane> {
        self.sub_panes.get(index)
    }

    /// Get mutable sub_pane by index (if exists)
    pub fn get_sub_pane_mut(&mut self, index: usize) -> Option<&mut SubPane> {
        self.sub_panes.get_mut(index)
    }

    /// Get sub_pane by instance_id (if exists)
    pub fn get_sub_pane_by_instance(&self, instance_id: u64) -> Option<&SubPane> {
        self.sub_panes.iter().find(|p| p.instance_id == instance_id)
    }

    /// Get mutable sub_pane by instance_id (if exists)
    pub fn get_sub_pane_by_instance_mut(&mut self, instance_id: u64) -> Option<&mut SubPane> {
        self.sub_panes.iter_mut().find(|p| p.instance_id == instance_id)
    }

    /// Update sub-pane price ranges for auto-scaling panes
    pub fn update_sub_pane_ranges(&mut self) {
        let (visible_start, visible_end) = self.viewport.visible_range();
        let visible_end = visible_end.min(self.bars.len());

        for sub_pane in &mut self.sub_panes {
            if sub_pane.auto_scale {
                if let Some((p_min, p_max)) = self.indicator_source.calculate_pane_range(
                    sub_pane.instance_id,
                    visible_start,
                    visible_end,
                ) {
                    sub_pane.price_min = p_min;
                    sub_pane.price_max = p_max;
                }
            }
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

    /// Clone data from another window (bars, symbol, timeframe, viewport settings)
    pub fn clone_data_from(&mut self, source: &ChartWindow) {
        self.bars = source.bars.clone();
        self.ma_fast = source.ma_fast.clone();
        self.ma_slow = source.ma_slow.clone();

        self.symbol = source.symbol.clone();
        self.timeframe = source.timeframe.clone();
        self.exchange = source.exchange.clone();
        self.update_title();

        self.viewport = source.viewport.clone();
        self.price_scale = source.price_scale.clone();

        self.grid_options = source.grid_options.clone();
        self.show_candles = source.show_candles;
        self.show_bars = source.show_bars;
        self.show_hollow_candles = source.show_hollow_candles;
        self.show_heikin_ashi = source.show_heikin_ashi;
        self.show_line = source.show_line;
        self.show_area = source.show_area;
        self.show_baseline = source.show_baseline;

        self.scale_settings = source.scale_settings.clone();
        self.price_scale.user_precision = self.scale_settings.user_precision;

        eprintln!("[ChartWindow] Cloned data from window {}: {} bars, symbol={}, tf={}",
            source.id, self.bars.len(), self.symbol, self.timeframe.name);
    }

    /// Create ChartRenderState from this window's data
    pub fn to_render_state<'a>(
        &'a self,
        chart_rect: crate::chart::render::ChartRect,
        theme: &'a crate::chart::render::ChartTheme,
        current_timeframe: Option<&'a str>,
        time_format_settings: Option<&'a crate::TimeFormatSettings>,
    ) -> crate::chart::render::ChartRenderState<'a> {
        crate::chart::render::ChartRenderState {
            viewport: &self.viewport,
            price_scale: &self.price_scale,
            time_scale: &self.time_scale,
            bars: &self.bars,
            grid: &self.grid_options,
            crosshair: &self.crosshair,
            legend: &self.legend,
            chart_rect,
            theme,
            time_ticks: None,
            current_timeframe,
            disable_clip: false,
            time_format_settings,
            timeframe_minutes: Some(self.timeframe.minutes),
            scale_settings: Some(&self.scale_settings),
            body_enabled: true,
            border_enabled: true,
            wick_enabled: true,
            use_prev_close: false,
        }
    }

    /// Create ChartRenderConfig from this window's options and render flags.
    pub fn to_render_config(
        &self,
        scale_theme: crate::chart::render::ScaleTheme,
        render_crosshair: bool,
        chart_type: &'static str,
    ) -> crate::layout::ChartRenderConfig {
        let crosshair_opts = &self.crosshair_options;
        let crosshair_config = if render_crosshair {
            crate::chart::render::CrosshairConfig {
                vert_visible: crosshair_opts.vert_line.visible,
                vert_width: crosshair_opts.vert_line.width,
                vert_style: crosshair_opts.vert_line.style.clone(),
                horz_visible: crosshair_opts.horz_line.visible,
                horz_width: crosshair_opts.horz_line.width,
                horz_style: crosshair_opts.horz_line.style.clone(),
            }
        } else {
            crate::chart::render::CrosshairConfig {
                vert_visible: false,
                vert_width: crosshair_opts.vert_line.width,
                vert_style: crosshair_opts.vert_line.style.clone(),
                horz_visible: false,
                horz_width: crosshair_opts.horz_line.width,
                horz_style: crosshair_opts.horz_line.style.clone(),
            }
        };

        crate::layout::ChartRenderConfig {
            scale_config: crate::chart::render::ScaleConfig::default(),
            scale_theme,
            crosshair_config,
            is_dragging: self.drag_mode.is_dragging() || self.drawing_manager.is_dragging(),
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
                        let (price_min, price_max) = if let Some(sub_pane) = self.sub_panes.get(pane_idx) {
                            (sub_pane.price_min, sub_pane.price_max)
                        } else {
                            (0.0, 100.0)
                        };
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
                let (price_min, price_max) = if let Some(sub_pane) = self.sub_panes.get(idx) {
                    (sub_pane.price_min, sub_pane.price_max)
                } else {
                    (0.0, 100.0)
                };
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
                    None,
                    None,
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
                // Sub-pane — use sub-pane's price_min/price_max
                if let Some(sub_pane) = self.sub_panes.get(idx) {
                    let pane_height = sub_pane.height as f64;
                    let range = sub_pane.price_max - sub_pane.price_min;
                    let y = if range > 0.0 {
                        pane_height * (1.0 - (price - sub_pane.price_min) / range)
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

    // =========================================================================
    // Compare Overlay Management
    // =========================================================================

    /// Add a comparison symbol to this chart window.
    pub fn add_compare_symbol(&mut self, symbol: &str) -> bool {
        if self.compare_overlay.has_symbol(symbol) || symbol == self.symbol {
            eprintln!("[Compare] Symbol {} already compared or is main symbol", symbol);
            return false;
        }

        let Some(bars) = self.data_provider.get_bars(symbol, &self.timeframe) else {
            eprintln!(
                "[Compare] Data not available for symbol={} tf={}",
                symbol,
                self.timeframe.name
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
            if let Some(prev_close) = self.prev_close_price {
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

    /// Set chart type and record in undo history.
    pub fn set_chart_type_with_undo(&mut self, chart_type: &'static str) {
        let previous_type = self.chart_type.to_string();
        self.chart_type = chart_type;
        self.command_history.push(Command::ChangeChartType {
            previous_type,
            new_type: chart_type.to_string(),
        });
    }

    // =========================================================================
    // Toggle Overlays
    // =========================================================================

    /// Toggle the legend visibility.
    pub fn toggle_legend(&mut self) {
        self.legend.visible = !self.legend.visible;
    }

    /// Toggle both grid lines (vertical and horizontal) together.
    pub fn toggle_grid(&mut self) {
        let new_visible = !self.grid_options.vert_lines.visible;
        self.grid_options.vert_lines.visible = new_visible;
        self.grid_options.horz_lines.visible = new_visible;
    }

    /// Toggle crosshair enabled state.
    pub fn toggle_crosshair(&mut self) {
        self.crosshair.enabled = !self.crosshair.enabled;
    }

    /// Toggle watermark visibility. Creates a default watermark if none exists.
    pub fn toggle_watermark(&mut self) {
        match &mut self.watermark {
            Some(wm) => wm.visible = !wm.visible,
            None => {
                self.watermark = Some(Watermark::default());
            }
        }
    }

    /// Toggle vertical grid lines visibility.
    pub fn toggle_grid_vertical(&mut self) {
        self.grid_options.vert_lines.visible = !self.grid_options.vert_lines.visible;
    }

    /// Toggle horizontal grid lines visibility.
    pub fn toggle_grid_horizontal(&mut self) {
        self.grid_options.horz_lines.visible = !self.grid_options.horz_lines.visible;
    }

    /// Toggle tooltip visibility.
    pub fn toggle_tooltip(&mut self) {
        self.tooltip.visible = !self.tooltip.visible;
    }

    /// Toggle tooltip follow-cursor mode.
    pub fn toggle_tooltip_follow(&mut self) {
        self.tooltip.follow_cursor = !self.tooltip.follow_cursor;
    }

    /// Toggle crosshair vertical line visibility.
    pub fn toggle_crosshair_vert_line(&mut self) {
        self.crosshair_options.vert_line.visible = !self.crosshair_options.vert_line.visible;
    }

    /// Toggle crosshair horizontal line visibility.
    pub fn toggle_crosshair_horz_line(&mut self) {
        self.crosshair_options.horz_line.visible = !self.crosshair_options.horz_line.visible;
    }

    /// Toggle legend OHLC display.
    pub fn toggle_legend_ohlc(&mut self) {
        self.legend.show_ohlc = !self.legend.show_ohlc;
    }

    /// Toggle legend change display.
    pub fn toggle_legend_change(&mut self) {
        self.legend.show_change = !self.legend.show_change;
    }

    /// Toggle legend percent display.
    pub fn toggle_legend_percent(&mut self) {
        self.legend.show_percent = !self.legend.show_percent;
    }

    // =========================================================================
    // Setters
    // =========================================================================

    /// Set grid line style for both vertical and horizontal lines.
    pub fn set_grid_style(&mut self, style: crate::drawing::LineStyle) {
        self.grid_options.vert_lines.style = style.clone();
        self.grid_options.horz_lines.style = style;
    }

    /// Set crosshair mode on both crosshair state and crosshair options.
    pub fn set_crosshair_mode(&mut self, mode: crate::CrosshairMode) {
        self.crosshair.mode = mode;
        self.crosshair_options.mode = mode;
    }

    /// Set crosshair line style for both vertical and horizontal lines.
    pub fn set_crosshair_style(&mut self, style: crate::drawing::LineStyle) {
        self.crosshair_options.vert_line.style = style.clone();
        self.crosshair_options.horz_line.style = style;
    }

    /// Set legend position.
    pub fn set_legend_position(&mut self, pos: crate::LegendPosition) {
        self.legend.position = pos;
    }

    /// Set watermark position. Creates a default watermark if none exists.
    pub fn set_watermark_position(&mut self, horz: crate::HorzAlign, vert: crate::VertAlign) {
        match &mut self.watermark {
            Some(wm) => {
                wm.horz_align = horz;
                wm.vert_align = vert;
            }
            None => {
                let mut wm = Watermark::default();
                wm.horz_align = horz;
                wm.vert_align = vert;
                self.watermark = Some(wm);
            }
        }
    }

    /// Set watermark color. Creates a default watermark if none exists.
    pub fn set_watermark_color(&mut self, color: &str) {
        match &mut self.watermark {
            Some(wm) => wm.set_color(color),
            None => {
                let mut wm = Watermark::default();
                wm.set_color(color);
                self.watermark = Some(wm);
            }
        }
    }

    /// Set watermark text. Creates a default watermark if none exists.
    pub fn set_watermark_text(&mut self, text: &str) {
        match &mut self.watermark {
            Some(wm) => wm.set_text(text),
            None => {
                let mut wm = Watermark::default();
                wm.set_text(text);
                self.watermark = Some(wm);
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

    /// Fit all bars into the visible chart area.
    pub fn fit_content(&mut self) {
        let bar_count = self.bars.len();
        if bar_count > 0 {
            self.viewport.bar_spacing = self.viewport.chart_width / bar_count as f64;
            self.viewport.bar_spacing = self.viewport.bar_spacing.clamp(1.0, 30.0);
            self.viewport.scroll_to_end();
            self.calc_auto_scale();
        }
    }

    /// Reset zoom to default bar spacing (8px) and scroll to end.
    pub fn reset_zoom(&mut self) {
        self.viewport.bar_spacing = 8.0;
        self.viewport.scroll_to_end();
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
        self.update_sub_pane_ranges();
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

    /// Execute a chart action, mutating chart state directly.
    ///
    /// Returns `true` if the action was handled by this window, `false` if it
    /// requires app-level handling.
    pub fn execute_chart_action(&mut self, action: &crate::ChartAction) -> bool {
        use crate::ChartAction;
        match action {
            // === Series / Chart Type ===
            ChartAction::SetChartType(ct) => {
                self.set_chart_type_with_undo(ct);
                true
            }

            // === Toggles ===
            ChartAction::ToggleLegend => { self.toggle_legend(); true }
            ChartAction::ToggleGrid => { self.toggle_grid(); true }
            ChartAction::ToggleCrosshair => { self.toggle_crosshair(); true }
            ChartAction::ToggleMagnet => { self.crosshair.toggle_magnet(); true }
            ChartAction::ToggleWatermark => { self.toggle_watermark(); true }
            ChartAction::ToggleGridVertical => { self.toggle_grid_vertical(); true }
            ChartAction::ToggleGridHorizontal => { self.toggle_grid_horizontal(); true }
            ChartAction::ToggleTooltip => { self.toggle_tooltip(); true }
            ChartAction::ToggleTooltipFollow => { self.toggle_tooltip_follow(); true }
            ChartAction::ToggleCrosshairVertLine => { self.toggle_crosshair_vert_line(); true }
            ChartAction::ToggleCrosshairHorzLine => { self.toggle_crosshair_horz_line(); true }
            ChartAction::ToggleLegendOHLC => { self.toggle_legend_ohlc(); true }
            ChartAction::ToggleLegendChange => { self.toggle_legend_change(); true }
            ChartAction::ToggleLegendPercent => { self.toggle_legend_percent(); true }

            // === Setters ===
            ChartAction::SetGridStyle(s) => { self.set_grid_style(*s); true }
            ChartAction::SetCrosshairMode(m) => { self.set_crosshair_mode(*m); true }
            ChartAction::SetCrosshairStyle(s) => { self.set_crosshair_style(*s); true }
            ChartAction::SetLegendPosition(p) => { self.set_legend_position(*p); true }
            ChartAction::SetWatermarkPosition(h, v) => { self.set_watermark_position(*h, *v); true }
            ChartAction::SetWatermarkColor(c) => { self.set_watermark_color(c); true }
            ChartAction::SetWatermarkText(t) => { self.set_watermark_text(t); true }

            // === Zoom ===
            ChartAction::ZoomIn => { self.zoom_in(); true }
            ChartAction::ZoomOut => { self.zoom_out(); true }
            ChartAction::FitContent => { self.fit_content(); true }
            ChartAction::ResetZoom => { self.reset_zoom(); true }

            // === Drawing ===
            ChartAction::SelectTool(tool) => { self.drawing_manager.set_tool(Some(tool)); true }
            ChartAction::ToggleLockDrawings => { self.drawing_manager.toggle_lock(); true }
            ChartAction::ToggleDrawingsVisible => { self.drawing_manager.toggle_visible(); true }
            ChartAction::DeleteSelected => { self.drawing_manager.delete_selected(); true }
            ChartAction::DeleteAll => { self.drawing_manager.clear(); true }

            // === Undo / Redo ===
            ChartAction::Undo => self.command_history.can_undo(),
            ChartAction::Redo => self.command_history.can_redo(),

            // === Actions not handled at window level (app-level, dialogs, etc.) ===
            _ => false,
        }
    }
}

impl Default for ChartWindow {
    fn default() -> Self {
        Self::new("BTCUSD", Timeframe::h1())
    }
}
