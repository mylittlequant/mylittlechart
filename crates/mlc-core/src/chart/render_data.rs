//! Chart render DATA types (themes, configs, render-state bags). Pure data consumed by backend + painters; the painters themselves live in mlc-layout (S1).

use crate::chart::types::{Viewport, PriceScale, TimeScale, GridOptions, Crosshair, Legend, TimeTick};
use crate::chart::annotations::LineStyle;
use crate::Bar;

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
    /// Timeframe interval in milliseconds for countdown calculation
    pub timeframe_interval_ms: Option<i64>,
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
    /// Footprint bar data for cluster rendering. When `Some(_)` and the chart
    /// type is `footprint`, the cluster renderer paints Bid × Ask per price
    /// level, POC highlight, and value-area background.
    /// When `None` (default), footprint dispatch falls back to draw_candles.
    pub footprint_bars: Option<&'a [crate::types::FootprintBar]>,
    /// Renko brick data. When `Some(_)` and chart_type == "renko" the
    /// renko renderer draws filled rect bricks; otherwise renko dispatch
    /// renders nothing.
    pub renko_bars: Option<&'a [crate::types::RenkoBrick]>,
    /// Three Line Break data. When `Some(_)` and chart_type ==
    /// "three_line_break" the TLB renderer draws filled rect lines.
    pub three_line_break: Option<&'a [crate::types::ThreeLineBreakLine]>,
    /// Point-and-Figure column data.
    pub pnf_columns: Option<&'a [crate::types::PnfColumn]>,
    /// Kagi segment data.
    pub kagi_segments: Option<&'a [crate::types::KagiSegment]>,
    /// Cumulative Volume Delta line samples (used by both the standalone
    /// CVD chart type and the CVD overlay on other chart types when
    /// enabled).
    pub cvd_line: Option<&'a [crate::types::CvdSample]>,
    /// TPO Market Profile session snapshots — typically a small ring
    /// (current session + N closed sessions).
    pub tpo_sessions: Option<&'a [crate::types::TpoSession]>,
    /// Pre-computed Heikin Ashi cache. When `Some(_)` and chart_type
    /// == "heikin_ashi" the HA renderer reads directly from this slice
    /// instead of running the recursive HA formula from index 0 each
    /// frame. Window maintains the cache on `ChartContainer.ha_cache`.
    pub ha_cache: Option<&'a [crate::types::HaBar]>,
    /// Pre-computed Volume Profile overlay. When `Some(_)` and
    /// `vp_show == true` the VP renderer draws the right-side histogram.
    /// Maintained by `ChartContainer::ensure_vp` in the prepare prepass.
    pub vp_overlay: Option<&'a crate::chart::volume_profile::VolumeProfileData>,
}

/// Configuration for scale rendering
#[derive(Clone, Debug)]
pub struct ScaleConfig {
    /// Price scale width in pixels
    pub price_scale_width: f64,
    /// Time scale height in pixels
    pub time_scale_height: f64,
    /// Font size for scale labels
    pub font_size: f64,
    /// Crosshair label font size
    pub crosshair_font_size: f64,
}

impl Default for ScaleConfig {
    fn default() -> Self {
        Self {
            price_scale_width: crate::PRICE_SCALE_WIDTH,
            time_scale_height: crate::TIME_SCALE_HEIGHT,
            font_size: 11.0,
            crosshair_font_size: 11.0,
        }
    }
}

/// Extended theme for scales (includes scale-specific colors)
#[derive(Clone, Debug)]
pub struct ScaleTheme {
    /// Scale background color
    pub scale_bg: String,
    /// Scale border color
    pub scale_border: String,
    /// Scale text color
    pub scale_text: String,
    /// Scale text color (medium weight)
    pub scale_text_medium: String,
    /// Scale text color (muted)
    pub scale_text_muted: String,
    /// Crosshair label background
    pub crosshair_label_bg: String,
    /// Crosshair label background (styled with opacity)
    pub crosshair_label_bg_styled: String,
    /// Crosshair label text
    pub crosshair_label_text: String,
}

impl Default for ScaleTheme {
    fn default() -> Self {
        Self {
            scale_bg: "#1e222d".to_string(),
            scale_border: "#2a2e39".to_string(),
            scale_text: "#d1d4dc".to_string(),
            scale_text_medium: "#9598a1".to_string(),
            scale_text_muted: "#6a6d78".to_string(),
            crosshair_label_bg: "#363a45".to_string(),
            crosshair_label_bg_styled: "#363a45".to_string(),
            crosshair_label_text: "#d1d4dc".to_string(),
        }
    }
}

/// Configuration for crosshair rendering
#[derive(Clone, Debug)]
pub struct CrosshairConfig {
    /// Whether vertical line is visible
    pub vert_visible: bool,
    /// Vertical line width
    pub vert_width: f64,
    /// Vertical line style
    pub vert_style: LineStyle,
    /// Whether horizontal line is visible
    pub horz_visible: bool,
    /// Horizontal line width
    pub horz_width: f64,
    /// Horizontal line style
    pub horz_style: LineStyle,
}

impl Default for CrosshairConfig {
    fn default() -> Self {
        Self {
            vert_visible: true,
            vert_width: 1.0,
            vert_style: LineStyle::Dashed,
            horz_visible: true,
            horz_width: 1.0,
            horz_style: LineStyle::Dashed,
        }
    }
}

/// Legend data for rendering
#[derive(Clone, Debug)]
pub struct LegendData {
    /// Symbol name
    pub symbol: String,
    /// Timeframe label
    pub timeframe: String,
    /// Current bar data
    pub bar: Bar,
    /// Change from open
    pub change: f64,
    /// Percent change
    pub percent: f64,
    /// Is bullish
    pub is_bullish: bool,
}

/// Tooltip content lines
#[derive(Clone, Debug)]
pub struct TooltipLines {
    pub lines: Vec<String>,
}

impl TooltipLines {
    pub fn from_bar(bar: &Bar) -> Self {
        Self {
            lines: vec![
                format!("O: {}", Self::fmt_price(bar.open)),
                format!("H: {}", Self::fmt_price(bar.high)),
                format!("L: {}", Self::fmt_price(bar.low)),
                format!("C: {}", Self::fmt_price(bar.close)),
                format!("V: {:.0}", bar.volume),
            ],
        }
    }

    /// Format price with appropriate precision for tooltip display
    pub fn fmt_price(price: f64) -> String {
        if price == 0.0 {
            "0".to_string()
        } else if price.abs() < 1e-8 {
            // Scientific notation for extremely small prices
            let exp = price.abs().log10().floor() as i32;
            let mantissa = price / 10f64.powi(exp);
            format!("{:.2}e{}", mantissa, exp)
        } else if price.abs() < 0.01 {
            format!("{:.8}", price)
        } else if price.abs() < 1.0 {
            format!("{:.6}", price)
        } else if price.abs() < 100.0 {
            format!("{:.4}", price)
        } else if price.abs() < 10000.0 {
            format!("{:.2}", price)
        } else {
            format!("{:.2}", price)
        }
    }
}

/// Render-side price line configuration (distinct from `chart::annotations::price_line::PriceLine`)
#[derive(Clone, Debug)]
pub struct RenderPriceLine {
    pub price: f64,
    pub color: String,
    pub line_width: f64,
    pub line_style: LineStyle,
    pub title: String,
    pub visible: bool,
}

/// Render-side marker shape (distinct from `chart::annotations::markers::MarkerShape`)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMarkerShape {
    Circle,
    Square,
    ArrowUp,
    ArrowDown,
}

/// Render-side chart marker (distinct from `chart::annotations::markers::Marker`)
#[derive(Clone, Debug)]
pub struct RenderMarker {
    pub bar_idx: usize,
    pub price: f64,
    pub shape: RenderMarkerShape,
    pub color: String,
    pub size: f64,
    pub text: Option<String>,
}

/// Sub-pane geometry and state
#[derive(Clone, Debug)]
pub struct PaneGeom {
    /// Pane rectangle (in screen coordinates)
    pub rect: ChartRect,
    /// Minimum value for Y-axis
    pub value_min: f64,
    /// Maximum value for Y-axis
    pub value_max: f64,
    /// Pane title
    pub title: String,
}

/// Histogram rendering style
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HistogramStyle {
    /// Draw bars from bottom of pane
    #[default]
    FromBottom,
    /// Draw bars from zero line (for MACD-style)
    Centered,
}

/// Theme for pane rendering
#[derive(Clone, Debug)]
pub struct PaneTheme {
    pub background: String,
    pub separator: String,
    pub grid_line: String,
    pub text: String,
    pub scale_bg: String,
    pub scale_border: String,
    pub up_color: String,
    pub down_color: String,
}

impl Default for PaneTheme {
    fn default() -> Self {
        Self {
            background: "#131722".to_string(),
            separator: "#2a2e39".to_string(),
            grid_line: "#2a2e3980".to_string(),
            text: "#d1d4dc".to_string(),
            scale_bg: "#1e222d".to_string(),
            scale_border: "#2a2e39".to_string(),
            up_color: "#26a69a".to_string(),
            down_color: "#ef5350".to_string(),
        }
    }
}

/// Options for grid rendering
#[derive(Clone, Debug)]
pub struct GridRenderOptions {
    /// Line color
    pub color: String,
    /// Line width
    pub width: f64,
    /// Line style
    pub style: LineStyle,
    /// Whether grid is visible
    pub visible: bool,
}

impl Default for GridRenderOptions {
    fn default() -> Self {
        Self {
            color: "#2a2e39".to_string(),
            width: 1.0,
            style: LineStyle::Solid,
            visible: true,
        }
    }
}

/// Line rendering style
#[derive(Clone, Copy, Debug, Default)]
pub struct LineRenderStyle {
    /// Dash length (0 for solid)
    pub dash: f64,
    /// Gap length
    pub gap: f64,
}

impl LineRenderStyle {
    pub fn solid() -> Self {
        Self { dash: 0.0, gap: 0.0 }
    }

    pub fn dashed() -> Self {
        Self { dash: 8.0, gap: 4.0 }
    }

    pub fn dotted() -> Self {
        Self { dash: 2.0, gap: 2.0 }
    }

    pub fn large_dashed() -> Self {
        Self { dash: 12.0, gap: 6.0 }
    }

    pub fn sparse_dotted() -> Self {
        Self { dash: 2.0, gap: 8.0 }
    }

    pub fn from_line_style(style: LineStyle) -> Self {
        match style {
            LineStyle::Solid => Self::solid(),
            LineStyle::Dashed => Self::dashed(),
            LineStyle::Dotted => Self::dotted(),
            LineStyle::LargeDashed => Self::large_dashed(),
            LineStyle::SparseDotted => Self::sparse_dotted(),
        }
    }

    pub fn is_solid(&self) -> bool {
        self.dash <= 0.0
    }
}
