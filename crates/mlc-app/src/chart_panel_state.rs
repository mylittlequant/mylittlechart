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

    // ── Series visibility flags ────────────────────────────────────────────────
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
    /// Tick-bar body-opacity intensity toggle (modulate by trade-count
    /// volume, `draw_tick_bars`'s `(vol/max_vol).sqrt().clamp(0.25,1.0)`
    /// formula). Default `false` — plain solid bodies exactly like
    /// `draw_candles`. Unused for every other chart type.
    pub tick_intensity: bool,
    /// Wick-suppression override for the bar family (range_bar / volume_bar
    /// / dollar_bar) — those three render WITHOUT wicks by default (their
    /// open/high/low pins are chained-continuity artifacts, not genuine
    /// intrabar excursions the way candle wicks are); this toggle re-enables
    /// them per-panel. Default `false`. The effective wick flag for those
    /// three chart types is `state.wick_enabled && bar_family_wicks`
    /// (`ChartRenderState::wicks_effective`, precomputed at render-state
    /// build time — `chart_type` itself is not threaded into the renderer).
    /// Unused (has no visual effect) for every other chart type — tick_bar /
    /// imbalance_bar / run_bar keep wicks unconditionally (canon).
    pub bar_family_wicks: bool,
}

impl ChartPanelState {
    /// Construct default state for a freshly-allocated canvas leaf.
    ///
    /// Defaults match [`crate::chart_container::ChartContainer::new_with_provider`]:
    /// `show_candles = true`, all other `show_*` flags `false`. Display
    /// options use their respective `Default` impls except
    /// `watermark = Some(Watermark::default())`.
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
            grid_options: GridOptions::default(),
            crosshair_options: CrosshairOptions::default(),
            legend: Legend::default(),
            watermark: Some(Watermark::default()),
            tooltip: Tooltip::default(),
            footprint_display: FootprintDisplayOptions::default(),
            // Registry-derived (chart-type-engine-consolidation B3, S5) —
            // fresh panels start on "candles" (`show_candles: true` above);
            // `RenderPolicy::bar_family_wicks_default` resolves to `false`
            // for candles' `WickPolicy::Always` (no toggle concept), same
            // value the independent literal used to hardcode.
            tick_intensity: false,
            bar_family_wicks: mlc_core::chart_type::by_id("candles")
                .map(|d| d.render_policy.bar_family_wicks_default())
                .unwrap_or(false),
        }
    }
}
