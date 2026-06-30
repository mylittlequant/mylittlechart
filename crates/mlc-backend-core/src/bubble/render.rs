use mlc_core::chart::render_data::{ChartRect, ChartRenderState, ChartTheme};
use mlc_core::chart::types::{GridOptions, Legend};
use mlc_core::state::chart_container::ChartContainer;
use mlc_core::TimeFormatSettings;

use super::data_bubble::DataBubble;

/// Build a `ChartRenderState` from a window and its associated bubble.
///
/// Lifted out of `ChartContainer::to_render_state` so that `mlc-core` no
/// longer needs to reference `DataBubble` (which lives in `mlc-backend-core`)
/// — breaking the dependency cycle.
pub fn build_chart_render_state<'a>(
    window: &'a ChartContainer,
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
        viewport: &window.viewport,
        price_scale: &window.price_scale,
        time_scale: &window.time_scale,
        bars: &bubble.bars,
        grid,
        crosshair: &window.crosshair,
        legend,
        chart_rect,
        theme,
        time_ticks: None,
        current_timeframe,
        disable_clip: false,
        time_format_settings,
        timeframe_interval_ms,
        scale_settings: Some(&window.scale_settings),
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
        ha_cache: window.ha_cache.as_deref(),
        vp_overlay: if window.vp_show { window.vp_overlay.as_ref() } else { None },
        // Display gates (oi_show / funding_show) are per-window view state;
        // storage is unconditional on bubble.
        oi_delta_ref: if window.oi_show { bubble.oi_delta.as_deref() } else { None },
        funding_rate_ref: if window.funding_show { bubble.funding_rate.as_deref() } else { None },
        dom_heatmap_ref: bubble.dom_heatmap.as_ref(),
        dom_trade_dots_ref: bubble.dom_trade_dots.as_deref(),
        liq_heatmap_ref: bubble.liq_heatmap.as_ref(),
        liq_dots_ref: bubble.liq_dots.as_deref(),
    }
}
