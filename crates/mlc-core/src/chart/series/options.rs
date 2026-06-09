//! Style options for all series types

use super::enums::{LineStyle, LineType, PriceLineSource};

// =============================================================================
// Common Options for All Series
// =============================================================================

/// Base structure with options common to all series
#[derive(Clone, Debug)]
pub struct SeriesOptionsCommon {
    /// Last value label visibility
    pub last_value_visible: bool,
    /// Series title
    pub title: String,
    /// Price scale ID ("left", "right", or custom)
    pub price_scale_id: Option<String>,
    /// Series visibility
    pub visible: bool,

    // Price Line (horizontal line at last price)
    pub price_line_visible: bool,
    pub price_line_source: PriceLineSource,
    pub price_line_width: u32,
    pub price_line_color: String,
    pub price_line_style: LineStyle,
}

impl Default for SeriesOptionsCommon {
    fn default() -> Self {
        Self {
            last_value_visible: true,
            title: String::new(),
            price_scale_id: Some("right".to_string()),
            visible: true,
            price_line_visible: true,
            price_line_source: PriceLineSource::LastBar,
            price_line_width: 1,
            price_line_color: String::new(), // Empty = use series color
            price_line_style: LineStyle::Dashed,
        }
    }
}

// =============================================================================
// Candlestick Options
// =============================================================================

#[derive(Clone, Debug)]
pub struct CandlestickStyleOptions {
    // Up candle colors
    pub up_color: String,
    pub border_up_color: String,
    pub wick_up_color: String,

    // Down candle colors
    pub down_color: String,
    pub border_down_color: String,
    pub wick_down_color: String,

    // Common colors (higher priority)
    pub border_color: String,
    pub wick_color: String,

    // Element visibility
    pub wick_visible: bool,
    pub border_visible: bool,
}

impl Default for CandlestickStyleOptions {
    fn default() -> Self {
        Self {
            up_color: "#26a69a".to_string(),
            down_color: "#ef5350".to_string(),
            wick_up_color: "#26a69a".to_string(),
            wick_down_color: "#ef5350".to_string(),
            border_up_color: "#26a69a".to_string(),
            border_down_color: "#ef5350".to_string(),
            border_color: "#378658".to_string(),
            wick_color: "#737375".to_string(),
            wick_visible: true,
            border_visible: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CandlestickSeriesOptions {
    pub common: SeriesOptionsCommon,
    pub style: CandlestickStyleOptions,
}

// =============================================================================
// Bar Options
// =============================================================================

#[derive(Clone, Debug)]
pub struct BarStyleOptions {
    pub up_color: String,
    pub down_color: String,
    pub open_visible: bool,
    pub thin_bars: bool,
}

impl Default for BarStyleOptions {
    fn default() -> Self {
        Self {
            up_color: "#26a69a".to_string(),
            down_color: "#ef5350".to_string(),
            open_visible: true,
            thin_bars: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BarSeriesOptions {
    pub common: SeriesOptionsCommon,
    pub style: BarStyleOptions,
}

// =============================================================================
// Line Options
// =============================================================================

#[derive(Clone, Debug)]
pub struct LineStyleOptions {
    // Main line
    pub color: String,
    pub line_style: LineStyle,
    pub line_width: u32,
    pub line_type: LineType,
    pub line_visible: bool,

    // Point markers
    pub point_markers_visible: bool,
    pub point_markers_radius: Option<f64>,

    // Crosshair marker
    pub crosshair_marker_visible: bool,
    pub crosshair_marker_radius: f64,
    pub crosshair_marker_border_color: String,
    pub crosshair_marker_background_color: String,
    pub crosshair_marker_border_width: f64,
}

impl Default for LineStyleOptions {
    fn default() -> Self {
        Self {
            color: "#2196f3".to_string(),
            line_style: LineStyle::Solid,
            line_width: 3,
            line_type: LineType::Simple,
            line_visible: true,
            point_markers_visible: false,
            point_markers_radius: None,
            crosshair_marker_visible: true,
            crosshair_marker_radius: 4.0,
            crosshair_marker_border_color: String::new(),
            crosshair_marker_background_color: String::new(),
            crosshair_marker_border_width: 2.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LineSeriesOptions {
    pub common: SeriesOptionsCommon,
    pub style: LineStyleOptions,
}

// =============================================================================
// Area Options
// =============================================================================

#[derive(Clone, Debug)]
pub struct AreaStyleOptions {
    // Gradient fill
    pub top_color: String,
    pub bottom_color: String,
    pub relative_gradient: bool,
    pub invert_filled_area: bool,

    // Line (inherits from LineStyleOptions)
    pub line_color: String,
    pub line_style: LineStyle,
    pub line_width: u32,
    pub line_type: LineType,
    pub line_visible: bool,

    // Markers (like Line)
    pub point_markers_visible: bool,
    pub point_markers_radius: Option<f64>,
    pub crosshair_marker_visible: bool,
    pub crosshair_marker_radius: f64,
    pub crosshair_marker_border_color: String,
    pub crosshair_marker_background_color: String,
    pub crosshair_marker_border_width: f64,
}

impl Default for AreaStyleOptions {
    fn default() -> Self {
        Self {
            top_color: "rgba(46, 220, 135, 0.4)".to_string(),
            bottom_color: "rgba(40, 221, 100, 0)".to_string(),
            relative_gradient: false,
            invert_filled_area: false,
            line_color: "#33D778".to_string(),
            line_style: LineStyle::Solid,
            line_width: 3,
            line_type: LineType::Simple,
            line_visible: true,
            point_markers_visible: false,
            point_markers_radius: None,
            crosshair_marker_visible: true,
            crosshair_marker_radius: 4.0,
            crosshair_marker_border_color: String::new(),
            crosshair_marker_background_color: String::new(),
            crosshair_marker_border_width: 2.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AreaSeriesOptions {
    pub common: SeriesOptionsCommon,
    pub style: AreaStyleOptions,
}

// =============================================================================
// Baseline Options
// =============================================================================

#[derive(Clone, Debug)]
pub struct BaselineStyleOptions {
    // Base value (dividing line)
    pub base_value: f64,
    pub relative_gradient: bool,

    // Top zone (above baseline)
    pub top_fill_color1: String,
    pub top_fill_color2: String,
    pub top_line_color: String,

    // Bottom zone (below baseline)
    pub bottom_fill_color1: String,
    pub bottom_fill_color2: String,
    pub bottom_line_color: String,

    // Line
    pub line_width: u32,
    pub line_style: LineStyle,
    pub line_type: LineType,
    pub line_visible: bool,

    // Markers
    pub point_markers_visible: bool,
    pub crosshair_marker_visible: bool,
    pub crosshair_marker_radius: f64,
}

impl Default for BaselineStyleOptions {
    fn default() -> Self {
        Self {
            base_value: 0.0,
            relative_gradient: false,
            top_fill_color1: "rgba(38, 166, 154, 0.28)".to_string(),
            top_fill_color2: "rgba(38, 166, 154, 0.05)".to_string(),
            top_line_color: "rgba(38, 166, 154, 1)".to_string(),
            bottom_fill_color1: "rgba(239, 83, 80, 0.05)".to_string(),
            bottom_fill_color2: "rgba(239, 83, 80, 0.28)".to_string(),
            bottom_line_color: "rgba(239, 83, 80, 1)".to_string(),
            line_width: 3,
            line_style: LineStyle::Solid,
            line_type: LineType::Simple,
            line_visible: true,
            point_markers_visible: false,
            crosshair_marker_visible: true,
            crosshair_marker_radius: 4.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BaselineSeriesOptions {
    pub common: SeriesOptionsCommon,
    pub style: BaselineStyleOptions,
}

// =============================================================================
// Histogram Options
// =============================================================================

#[derive(Clone, Debug)]
pub struct HistogramStyleOptions {
    pub color: String,
    pub base: f64, // Base line (where columns grow from)
}

impl Default for HistogramStyleOptions {
    fn default() -> Self {
        Self {
            color: "#26a69a".to_string(),
            base: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct HistogramSeriesOptions {
    pub common: SeriesOptionsCommon,
    pub style: HistogramStyleOptions,
}

// =============================================================================
// SeriesOptions Enum (Union Type)
// =============================================================================

#[derive(Clone, Debug)]
pub enum SeriesOptions {
    Candlestick(CandlestickSeriesOptions),
    Bar(BarSeriesOptions),
    Line(LineSeriesOptions),
    Area(AreaSeriesOptions),
    Baseline(BaselineSeriesOptions),
    Histogram(HistogramSeriesOptions),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_options_defaults() {
        let common = SeriesOptionsCommon::default();
        assert!(common.last_value_visible);
        assert!(common.visible);
        assert_eq!(common.price_scale_id, Some("right".to_string()));
    }

    #[test]
    fn test_line_style_options_defaults() {
        let opts = LineStyleOptions::default();
        assert_eq!(opts.color, "#2196f3");
        assert_eq!(opts.line_width, 3);
        assert_eq!(opts.line_type, LineType::Simple);
    }

    #[test]
    fn test_baseline_style_options_defaults() {
        let opts = BaselineStyleOptions::default();
        assert_eq!(opts.base_value, 0.0);
        assert!(opts.line_visible);
        assert_eq!(opts.line_width, 3);
    }
}
