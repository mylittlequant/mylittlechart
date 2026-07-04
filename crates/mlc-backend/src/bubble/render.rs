use mlc_core::chart::render_data::{ChartRect, ChartRenderState, ChartTheme};
use mlc_core::chart::types::price_scale::PriceScale;
use mlc_core::chart::types::{Crosshair, GridOptions, Legend, TimeScale, Viewport};
use mlc_core::scale_settings::ScaleSettings;
use mlc_core::types::HaBar;
use mlc_core::chart::volume_profile::VolumeProfileData;
use mlc_core::TimeFormatSettings;

use super::data_bubble::DataBubble;

/// Window-side render inputs needed to build a `ChartRenderState`.
///
/// A narrow struct-of-refs instead of `&ChartContainer` — `mlc-backend`
/// cannot depend on `mlc-app` (which owns `ChartContainer`), so this
/// function takes only the handful of fields the render pass actually
/// reads.  The caller (`mlc-app`) builds this from its own `&ChartContainer`
/// at the call site (see `mlc-app::lib.rs` `render_state_for` callers).
pub struct ChartWindowRenderInputs<'a> {
    pub viewport: &'a Viewport,
    pub price_scale: &'a PriceScale,
    pub time_scale: &'a TimeScale,
    pub crosshair: &'a Crosshair,
    pub scale_settings: &'a ScaleSettings,
    /// Heikin Ashi memo cache (window-local, viewport-coupled).
    pub ha_cache: Option<&'a [HaBar]>,
    /// Volume Profile overlay, already gated by the window's `vp_show` flag.
    pub vp_overlay: Option<&'a VolumeProfileData>,
    /// OI Delta display gate (window-local view state).
    pub oi_show: bool,
    /// Funding Rate display gate (window-local view state).
    pub funding_show: bool,
}

/// Build a `ChartRenderState` from window render inputs and the associated bubble.
///
/// Lifted out of `ChartContainer::to_render_state` so that `mlc-core` no
/// longer needs to reference `DataBubble` (which lives in `mlc-backend`)
/// — breaking the dependency cycle.  Step 4 (state lift) further narrowed
/// the `window` parameter from `&ChartContainer` (which now lives in
/// `mlc-app`) to [`ChartWindowRenderInputs`].
pub fn build_chart_render_state<'a>(
    window: ChartWindowRenderInputs<'a>,
    bubble: &'a DataBubble,
    chart_rect: ChartRect,
    theme: &'a ChartTheme,
    current_timeframe: Option<&'a str>,
    time_format_settings: Option<&'a TimeFormatSettings>,
    timeframe_interval_ms: Option<i64>,
    grid: &'a GridOptions,
    legend: &'a Legend,
) -> ChartRenderState<'a> {
    ChartRenderState {
        viewport: window.viewport,
        price_scale: window.price_scale,
        time_scale: window.time_scale,
        bars: &bubble.bars,
        grid,
        crosshair: window.crosshair,
        legend,
        chart_rect,
        theme,
        time_ticks: None,
        current_timeframe,
        disable_clip: false,
        time_format_settings,
        timeframe_interval_ms,
        scale_settings: Some(window.scale_settings),
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
        ha_cache: window.ha_cache,
        vp_overlay: window.vp_overlay,
        // Display gates (oi_show / funding_show) are per-window view state;
        // storage is unconditional on bubble.
        oi_delta_ref: if window.oi_show { bubble.oi_delta.as_deref() } else { None },
        funding_rate_ref: if window.funding_show { bubble.funding_rate.as_deref() } else { None },
        dom_heatmap_ref: bubble.dom_heatmap.as_ref(),
        dom_trade_dots_ref: bubble.dom_trade_dots.as_deref(),
        liq_heatmap_ref: bubble.liq_heatmap.as_ref(),
        liq_dots_ref: bubble.liq_dots.as_deref(),
        // Default false — flipped to true by the caller (`mlc-app::lib.rs`)
        // only when a non-temporal ts-shadow (renko/pnf/kagi/three_line_break)
        // is swapped into `bars` after this state is built.
        non_temporal_axis: false,
        // Default false — flipped to true by the caller (`mlc-app::lib.rs`)
        // for the counted-bar family (range/tick/volume/dollar/tick-imbalance/
        // volume-imbalance/run bars).
        hide_time_grid: false,
    }
}
