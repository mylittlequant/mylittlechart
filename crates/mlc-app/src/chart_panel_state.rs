//! `ChartPanelState`: per-canvas display options.
//!
//! Moved from `mlc-core::state::chart_panel_state` in Step 4 (state lift,
//! see `docs/mlc/plans/doctrine-cutover-2026-06-27.md`) — this is UI-window
//! state (which series render, chart type, display options for one canvas
//! leaf), not a type-dictionary value.  Has zero consumers below `mlc-app`.
//!
//! Stored in `TradingPanelsStore.chart_panel` (chart-app) keyed by
//! [`ChartPanelId`].  Replaces the 18 duplicate display-option fields (S2.7).
//!
//! The `drawing_manager` field (S2.8) was previously here; it was lifted to
//! `BackendCore::drawing` (backend-cutover), and Step 3.1 further reset it
//! into a per-`BubbleId` session manager (not per-panel) so that all
//! consumers access it through a single authoritative location keyed by
//! the data bubble rather than the chart panel.

use mlc_core::chart::{CrosshairOptions, FootprintDisplayOptions, GridOptions, Legend, Tooltip, Watermark};
use mlc_core::ids::ChartPanelId;

/// State of one chart canvas leaf.
///
/// Stored in `TradingPanelsStore.chart_panel` keyed by [`ChartPanelId`].
/// Owns the 5 display-option fields and 13 series-visibility fields that
/// were formerly duplicated on [`crate::chart_container::ChartContainer`].
#[derive(Debug)]
pub struct ChartPanelState {
    pub chart_panel_id: ChartPanelId,

    // ── Series visibility flags + chart_type ─────────────────────────────────
    /// Show candlesticks (default: `true`).
    pub show_candles: bool,
    /// Show OHLC bars with ticks (default: `false`).
    pub show_bars: bool,
    /// Show hollow candles — bullish=outline, bearish=filled (default: `false`).
    pub show_hollow_candles: bool,
    /// Show Heikin Ashi smoothed candles (default: `false`).
    pub show_heikin_ashi: bool,
    /// Show line series (default: `false`).
    pub show_line: bool,
    /// Show step line / staircase (default: `false`).
    pub show_step_line: bool,
    /// Show line with dot markers (default: `false`).
    pub show_line_markers: bool,
    /// Show area series (default: `false`).
    pub show_area: bool,
    /// Show HLC area — high/low/close band (default: `false`).
    pub show_hlc_area: bool,
    /// Show histogram (default: `false`).
    pub show_histogram: bool,
    /// Show columns — vertical bars from baseline (default: `false`).
    pub show_columns: bool,
    /// Show baseline (default: `false`).
    pub show_baseline: bool,
    /// Active chart type name (e.g. `"candles"`, `"bars"`, `"line"`, `"area"`).
    /// Default: `"candles"`.
    pub chart_type: &'static str,

    /// Numeric parameter for derived-bar chart types (range_bar / volume_bar).
    ///
    /// - For `"range_bar"`: price-move size (e.g. `10.0` USD) — converted to
    ///   ×1e8 fixed-point at the bridge boundary.
    /// - For `"volume_bar"`: cumulative volume per bar (e.g. `1000.0` units).
    /// - For `"footprint"`: unused — footprint uses `chart_type_interval` instead.
    /// - For plain render chart types: unused.
    ///
    /// Default `0.0` — code-side defaults (10.0 / 1000.0) are applied at
    /// subscribe time when this field is non-positive.
    pub chart_type_param: f64,

    /// Kline interval label for `"footprint"` chart type (e.g. `"1m"`, `"5m"`).
    /// Default: `"1m"`. Unused for non-footprint chart types.
    pub chart_type_interval: &'static str,

    // ── Display options ───────────────────────────────────────────────────────
    /// Grid display options (vertical + horizontal lines).
    pub grid_options: GridOptions,
    /// Crosshair display options (mode + line styles).
    pub crosshair_options: CrosshairOptions,
    /// Legend display state and configuration.
    pub legend: Legend,
    /// Watermark display (default: `Some(Watermark::default())`).
    pub watermark: Option<Watermark>,
    /// Tooltip display state and configuration.
    pub tooltip: Tooltip,
    /// Footprint cluster display toggles (ABS markers / volume text / heat
    /// intensity). Unused for every other chart type.
    pub footprint_display: FootprintDisplayOptions,
}

impl ChartPanelState {
    /// Construct default state for a freshly-allocated canvas leaf.
    ///
    /// Defaults match [`crate::chart_container::ChartContainer::new_with_provider`]:
    /// `show_candles = true`, all other `show_*` flags `false`,
    /// `chart_type = "candles"`. Display options use their respective
    /// `Default` impls except `watermark = Some(Watermark::default())`.
    pub fn new(chart_panel_id: ChartPanelId) -> Self {
        Self {
            chart_panel_id,
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
            chart_type_param: 0.0,
            chart_type_interval: "1m",
            grid_options: GridOptions::default(),
            crosshair_options: CrosshairOptions::default(),
            legend: Legend::default(),
            watermark: Some(Watermark::default()),
            tooltip: Tooltip::default(),
            footprint_display: FootprintDisplayOptions::default(),
        }
    }
}
