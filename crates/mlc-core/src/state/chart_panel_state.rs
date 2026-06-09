//! Phase 4 S2.8 — `ChartPanelState`: per-canvas display options + drawing manager.
//!
//! Moved into `zengeld-chart` so that both the chart crate and `chart-app`
//! can reference the type without a circular dependency.
//!
//! Stored in `TradingPanelsStore.chart_panel` (chart-app) keyed by
//! [`ChartPanelId`].  Replaces the 18 duplicate display-option fields (S2.7)
//! and the `drawing_manager` field (S2.8) that were formerly on
//! [`super::chart_container::ChartContainer`].

use crate::chart::{CrosshairOptions, GridOptions, Legend, Tooltip, Watermark};
use crate::drawing::DrawingManager;
use crate::state::chart_container_leaf::ChartPanelId;

/// State of one chart canvas leaf.
///
/// Stored in `TradingPanelsStore.chart_panel` keyed by [`ChartPanelId`].
/// Owns the 5 display-option fields and 13 series-visibility fields that
/// were formerly duplicated on [`super::chart_container::ChartContainer`].
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

    // ── Drawing manager ───────────────────────────────────────────────────────
    /// Drawing primitives manager for this canvas leaf.
    ///
    /// Migrated here from [`super::chart_container::ChartContainer`] in Phase 4
    /// S2.8. Callers that need to draw, read primitives, or hit-test must look
    /// up the `ChartPanelState` via `TradingPanelsStore.chart_panel`.
    pub drawing_manager: DrawingManager,

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
}

impl ChartPanelState {
    /// Construct default state for a freshly-allocated canvas leaf.
    ///
    /// Defaults match [`super::chart_container::ChartContainer::new_with_provider`]:
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
            drawing_manager: DrawingManager::new(),
            grid_options: GridOptions::default(),
            crosshair_options: CrosshairOptions::default(),
            legend: Legend::default(),
            watermark: Some(Watermark::default()),
            tooltip: Tooltip::default(),
        }
    }
}
