//! Phase 4 — `ChartPanelState` with series-visibility fields (S2.6.A).
//!
//! Per the S2 handoff
//! (`docs/plans/phase4-s2-handoff-2026-06-04.md`), subsequent S2.X commits
//! migrate canvas-leaf fields out of `ChartContainer` into this struct, stored
//! in `TradingPanelsStore.chart_panel` keyed by `ChartPanelId`.

use zengeld_chart::state::ChartPanelId;

/// State of one chart canvas leaf.
///
/// Stored in `TradingPanelsStore.chart_panel`, keyed by [`ChartPanelId`].
/// Phase 4 / S2.6.A adds 13 series-visibility fields mirroring their
/// counterparts in [`zengeld_chart::state::ChartContainer`]. The
/// `ChartContainer` fields remain read-authoritative through S2.7; these
/// fields are kept in sync via dual-write helpers in `chart_panel_bridge`.
#[derive(Debug)]
pub struct ChartPanelState {
    pub chart_panel_id: ChartPanelId,

    // Phase 4 S2.6.A — series visibility flags + chart_type
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
}

impl ChartPanelState {
    /// Construct default state for a freshly-allocated canvas leaf.
    ///
    /// Defaults match [`zengeld_chart::state::ChartContainer::new_with_provider`]:
    /// `show_candles = true`, all other `show_*` flags `false`,
    /// `chart_type = "candles"`.
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
        }
    }
}
