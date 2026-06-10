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
//! use mlc_core::chart::render::{render_chart_frame, ChartRenderState};
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
mod utils;

// Re-export rendering functions
pub use grid::{draw_grid, draw_grid_extended, draw_styled_line, draw_dashed_line};
pub use candles::{draw_candles, draw_bars, draw_hollow_candles, draw_heikin_ashi};
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

// Shim: re-export all DATA types from render_data so that existing
// `mlc_core::chart::render::ChartRect` etc. paths continue to resolve.
pub use crate::chart::render_data::{
    ChartRect, ChartBounds, ChartTheme, ChartRenderState,
    ScaleConfig, ScaleTheme,
    CrosshairConfig,
    LegendData, TooltipLines,
    RenderPriceLine, RenderMarker, RenderMarkerShape,
    PaneGeom, PaneTheme, HistogramStyle,
    GridRenderOptions, LineRenderStyle,
};

// Legacy path shims: the old names PriceLine/Marker/MarkerShape are no longer
// exported from this module (they were renamed to Render* to resolve collisions).
// Consumers that were using these names within chart/render/ have been updated in-place.

use crate::render::{RenderContext, InputState, FrameResult};

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
