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
//! use zengeld_chart::chart::render::{render_chart_frame, ChartRenderState};
//! use zengeld_chart::render::{RenderContext, InputState};
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
pub use scales::{draw_price_scale, draw_time_scale, ScaleConfig, ScaleTheme};
pub use crosshair::{draw_crosshair, draw_pane_crosshair, CrosshairConfig};
pub use overlays::{
    draw_watermark, draw_legend, draw_tooltip, draw_price_lines, draw_markers,
    draw_last_price_line,
    LegendData, TooltipLines, PriceLine, Marker, MarkerShape,
};
pub use panes::{
    draw_pane_separator, draw_pane_background, draw_pane_grid,
    draw_pane_line, draw_pane_histogram, draw_pane_price_scale,
    PaneGeom, PaneTheme, HistogramStyle,
};
pub use utils::{GridRenderOptions, LineRenderStyle};

use crate::render::{RenderContext, InputState, FrameResult};
use crate::chart::types::{
    Viewport, PriceScale, TimeScale, GridOptions, Crosshair, Legend, TimeTick,
};
use crate::Bar;

/// State needed for chart rendering
///
/// This struct aggregates all the data needed to render a chart frame.
/// Platforms create this from their own state and pass it to render functions.
#[derive(Debug)]
pub struct ChartRenderState<'a> {
    /// Viewport for coordinate conversion
    pub viewport: &'a Viewport,
    /// Price scale for Y-axis
    pub price_scale: &'a PriceScale,
    /// Time scale for X-axis
    pub time_scale: &'a TimeScale,
    /// Bar data
    pub bars: &'a [Bar],
    /// Grid options
    pub grid: &'a GridOptions,
    /// Crosshair state
    pub crosshair: &'a Crosshair,
    /// Legend state
    pub legend: &'a Legend,
    /// Chart area dimensions
    pub chart_rect: ChartRect,
    /// Theme colors
    pub theme: &'a ChartTheme,
    /// Pre-computed time ticks (shared between grid and time scale)
    /// If None, will be computed on-demand (for backwards compatibility)
    pub time_ticks: Option<&'a [TimeTick]>,
    /// Current timeframe label (e.g., "1H", "15m", "1D") for primitive visibility filtering
    /// If None, all primitives are visible regardless of timeframe settings
    pub current_timeframe: Option<&'a str>,
    /// Disable clipping to chart_rect bounds (for Glass styles where chart renders under toolbars)
    /// When true, chart content can render beyond chart_rect boundaries
    pub disable_clip: bool,
    /// Time format settings for rendering time labels
    /// If None, defaults will be used
    pub time_format_settings: Option<&'a crate::TimeFormatSettings>,
    /// Timeframe minutes for countdown calculation
    pub timeframe_minutes: Option<u32>,
    /// Scale settings for countdown and other scale features
    pub scale_settings: Option<&'a crate::ScaleSettings>,
    /// Whether candle bodies are visible
    pub body_enabled: bool,
    /// Whether candle borders are visible
    pub border_enabled: bool,
    /// Whether candle wicks are visible
    pub wick_enabled: bool,
    /// Use previous bar's close to determine candle color (instead of open vs close)
    pub use_prev_close: bool,
}

/// Chart area rectangle
#[derive(Clone, Copy, Debug, Default)]
pub struct ChartRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ChartRect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    /// Get bounds for clamping operations
    #[inline]
    pub fn bounds(&self) -> ChartBounds {
        ChartBounds {
            left: self.x,
            right: self.x + self.width,
            top: self.y,
            bottom: self.y + self.height,
        }
    }
}

/// Bounds for clamping render coordinates to chart area
///
/// Used to prevent rendering outside the chart boundaries
/// (e.g., over price scale or time scale areas).
#[derive(Clone, Copy, Debug, Default)]
pub struct ChartBounds {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

impl ChartBounds {
    /// Create bounds from a ChartRect
    #[inline]
    pub fn from_rect(rect: &ChartRect) -> Self {
        rect.bounds()
    }

    /// Check if X coordinate is within bounds
    #[inline]
    pub fn x_in_bounds(&self, x: f64) -> bool {
        x >= self.left && x <= self.right
    }

    /// Check if Y coordinate is within bounds
    #[inline]
    pub fn y_in_bounds(&self, y: f64) -> bool {
        y >= self.top && y <= self.bottom
    }

    /// Clamp X coordinate to bounds
    #[inline]
    pub fn clamp_x(&self, x: f64) -> f64 {
        x.clamp(self.left, self.right.max(self.left))
    }

    /// Clamp Y coordinate to bounds
    #[inline]
    pub fn clamp_y(&self, y: f64) -> f64 {
        y.clamp(self.top, self.bottom.max(self.top))
    }

    /// Check if a vertical line segment (high_y to low_y) is completely outside bounds
    /// Note: In screen coordinates, high_y < low_y (top of screen is 0)
    #[inline]
    pub fn is_y_range_outside(&self, high_y: f64, low_y: f64) -> bool {
        high_y >= self.bottom || low_y <= self.top
    }

    /// Clamp a rectangle to bounds, returning (x, width) tuple
    /// Returns None if rectangle is completely outside bounds
    #[inline]
    pub fn clamp_rect_x(&self, x: f64, width: f64) -> Option<(f64, f64)> {
        let left = x.max(self.left);
        let right = (x + width).min(self.right);
        let w = right - left;
        if w > 0.0 { Some((left, w)) } else { None }
    }

    /// Clamp a rectangle to bounds, returning (y, height) tuple
    /// Returns None if rectangle is completely outside bounds
    #[inline]
    pub fn clamp_rect_y(&self, y: f64, height: f64) -> Option<(f64, f64)> {
        let top = y.max(self.top);
        let bottom = (y + height).min(self.bottom);
        let h = bottom - top;
        if h > 0.0 { Some((top, h)) } else { None }
    }
}

/// Theme colors for chart rendering
#[derive(Clone, Debug)]
pub struct ChartTheme {
    /// Background color
    pub background: String,
    /// Grid line color
    pub grid_line: String,
    /// Text color
    pub text: String,
    /// Up candle body
    pub candle_up: String,
    /// Down candle body
    pub candle_down: String,
    /// Up candle wick
    pub wick_up: String,
    /// Down candle wick
    pub wick_down: String,
    /// Up candle border (optional, for hollow/outlined candles)
    pub candle_up_border: Option<String>,
    /// Down candle border (optional, for hollow/outlined candles)
    pub candle_down_border: Option<String>,
    /// Legend value color for bullish
    pub legend_value_up: String,
    /// Legend value color for bearish
    pub legend_value_down: String,
    /// Crosshair color
    pub crosshair: String,
    /// Scale background
    pub scale_bg: String,
    /// Scale border
    pub scale_border: String,
    /// Sub-pane background (with style opacity applied)
    pub sub_pane_bg: String,
}

impl Default for ChartTheme {
    fn default() -> Self {
        Self {
            background: "#1e222d".to_string(),
            grid_line: "#2a2e39".to_string(),
            text: "#d1d4dc".to_string(),
            candle_up: "#26a69a".to_string(),
            candle_down: "#ef5350".to_string(),
            wick_up: "#26a69a".to_string(),
            wick_down: "#ef5350".to_string(),
            candle_up_border: None,
            candle_down_border: None,
            legend_value_up: "#26a69a".to_string(),
            legend_value_down: "#ef5350".to_string(),
            crosshair: "#9598a1".to_string(),
            scale_bg: "#1e222d".to_string(),
            scale_border: "#2a2e39".to_string(),
            sub_pane_bg: "#1e222d".to_string(),
        }
    }
}

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
