//! Chart rendering module
//!
//! Platform-agnostic chart rendering using RenderContext trait.
//!
//! # Functions
//!
//! - `render_chart_frame()` - Main entry point, renders entire chart
//! - `draw_grid()` - Grid lines (horizontal price, vertical time)
//! - `draw_candles()` / `draw_bars()` - OHLC visualization
//! - `draw_line_series()` / `draw_area_series()` - Line charts
//! - `draw_price_scale()` / `draw_time_scale()` - Axis labels
//! - `draw_crosshair()` - Cursor tracking
//! - `draw_legend()` - OHLC values display
//!
//! # Usage
//!
//! ```ignore
//! use mlc_app_layout::chart_render::{render_chart_frame, ChartRenderState};
//! use mlc_core::render::{RenderContext, InputState};
//!
//! fn render(ctx: &mut dyn RenderContext, input: &InputState, state: &ChartRenderState) {
//!     render_chart_frame(ctx, input, state);
//! }
//! ```

mod grid;
mod candles;
mod series;
mod scales;
mod crosshair;
mod overlays;
mod panes;
mod footprint;
mod renko;
mod pnf;
mod kagi;
mod cvd;
mod tpo;
mod three_line_break;
mod volume_profile;
mod oi_delta;
mod funding_rate;
mod dom_heatmap;
mod liquidation_heatmap;

// Re-export rendering functions
pub use grid::{draw_grid, draw_grid_extended, draw_styled_line, draw_dashed_line};
pub use candles::{draw_candles, draw_bars, draw_hollow_candles, draw_heikin_ashi, draw_range_bars, draw_volume_bars, draw_tick_bars, draw_footprint};
pub use footprint::draw_footprint_cluster;
pub use renko::draw_renko;
pub use pnf::draw_pnf;
pub use kagi::draw_kagi;
pub use cvd::{draw_cvd_chart, draw_cvd_overlay};
pub use tpo::draw_tpo;
pub use three_line_break::draw_three_line_break;
pub use volume_profile::draw_volume_profile_overlay;
pub use oi_delta::draw_oi_delta;
pub use funding_rate::draw_funding_rate_strip;
pub use dom_heatmap::draw_dom_heatmap;
pub use liquidation_heatmap::draw_liquidation_heatmap;
pub use series::{
    draw_line_series, draw_area_series, draw_histogram,
    draw_baseline_series, draw_step_line, draw_line_with_markers,
    draw_line_from_data, draw_hlc_area, draw_columns, draw_compare_overlay,
};
pub use scales::{
    draw_price_scale, draw_price_scale_cursor_label,
    draw_time_scale, draw_time_scale_cursor_label,
};
pub use crosshair::{draw_crosshair, draw_pane_crosshair};
pub use overlays::{
    draw_watermark, draw_legend, draw_tooltip, draw_price_lines, draw_markers,
    draw_last_price_line,
};
pub use panes::{
    draw_pane_separator, draw_pane_background, draw_pane_grid,
    draw_pane_line, draw_pane_histogram, draw_pane_price_scale,
};

// Re-export all DATA types from mlc_core::chart::render_data so that
// intra-layout code can use `crate::chart_render::ChartRect` etc.
pub use mlc_core::chart::render_data::{
    ChartRect, ChartBounds, ChartTheme, ChartRenderState,
    ScaleConfig, ScaleTheme,
    CrosshairConfig,
    LegendData, TooltipLines,
    RenderPriceLine, RenderMarker, RenderMarkerShape,
    PaneGeom, PaneTheme, HistogramStyle,
    GridRenderOptions, LineRenderStyle,
};

use mlc_core::render::{RenderContext, InputState, FrameResult, TextAlign, TextBaseline};

/// Main chart rendering entry point
///
/// Renders a complete chart frame in the correct order:
/// 1. Background
/// 2. Grid
/// 3. Series (candles/bars/line)
/// 4. Overlays (crosshair, legend)
/// 5. Scales
pub fn render_chart_frame(
    ctx: &mut dyn RenderContext,
    _input: &InputState,
    state: &ChartRenderState,
) -> FrameResult {
    let rect = &state.chart_rect;
    let theme = state.theme;

    // 1. Background
    ctx.set_fill_color(&theme.background);
    ctx.fill_rect(rect.x, rect.y, rect.width, rect.height);

    // 2. Grid
    draw_grid(ctx, state);

    // Note: For full chart rendering use layout::render_chart_window()
    // which renders candles, scales, crosshair, etc.

    FrameResult::default()
}

/// Honest empty-series placeholder shared by every derived-chart renderer
/// that silently no-ops when its data source is empty (Wave T1 commit 2,
/// `docs/mlc/audits/series-types-ui-audit-2026-07-04.md`).
///
/// A blank pane reads as "broken"; this draws centered dim text instead —
/// same visual language as footprint's zoom-out placeholder
/// (`footprint::draw_no_data_placeholder`), factored out here so the other
/// 8 empty-series renderers (cvd, tpo, renko, pnf, kagi, three_line_break,
/// dom_heatmap, liquidation_heatmap) share ONE implementation instead of
/// 8 copies.
pub(crate) fn draw_waiting_placeholder(ctx: &mut dyn RenderContext, state: &ChartRenderState, msg: &str) {
    let rect = &state.chart_rect;
    ctx.set_fill_color("#787878");
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(msg, rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
}
