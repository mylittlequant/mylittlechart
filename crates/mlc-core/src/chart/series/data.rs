//! Data structures for all series types

use crate::Bar;

// =============================================================================
// Single Value Data (for Line, Area, Baseline, Histogram)
// =============================================================================

/// Single value with timestamp
#[derive(Clone, Copy, Debug)]
pub struct SingleValue {
    /// Unix timestamp in seconds
    pub timestamp: i64,
    /// Value (price, indicator)
    pub value: f64,
}

impl SingleValue {
    pub fn new(timestamp: i64, value: f64) -> Self {
        Self { timestamp, value }
    }
}

// =============================================================================
// OHLC Data (for Candlestick and Bar)
// =============================================================================

/// Data for candlestick series
#[derive(Clone, Debug)]
pub struct CandlestickData {
    /// Base OHLC data
    pub bar: Bar,
    /// Color override (optional)
    pub color: Option<String>,
    /// Border color override (optional)
    pub border_color: Option<String>,
    /// Wick color override (optional)
    pub wick_color: Option<String>,
}

/// Data for Bar series (OHLC)
#[derive(Clone, Debug)]
pub struct BarData {
    /// Base OHLC data
    pub bar: Bar,
    /// Color override (optional)
    pub color: Option<String>,
}

// =============================================================================
// Line Series Data
// =============================================================================

/// Data for line series
#[derive(Clone, Debug)]
pub struct LineData {
    /// Base value
    pub point: SingleValue,
    /// Color override (optional)
    pub color: Option<String>,
}

// =============================================================================
// Area Series Data
// =============================================================================

/// Data for area series
#[derive(Clone, Debug)]
pub struct AreaData {
    /// Base value
    pub point: SingleValue,
    /// Color override (optional)
    pub color: Option<String>,
}

// =============================================================================
// Baseline Series Data
// =============================================================================

/// Data for baseline series
#[derive(Clone, Debug)]
pub struct BaselineData {
    /// Base value
    pub point: SingleValue,
    /// Top zone color overrides
    pub top_fill_color1: Option<String>,
    pub top_fill_color2: Option<String>,
    pub top_line_color: Option<String>,
    /// Bottom zone color overrides
    pub bottom_fill_color1: Option<String>,
    pub bottom_fill_color2: Option<String>,
    pub bottom_line_color: Option<String>,
}

impl Default for BaselineData {
    fn default() -> Self {
        Self {
            point: SingleValue::new(0, 0.0),
            top_fill_color1: None,
            top_fill_color2: None,
            top_line_color: None,
            bottom_fill_color1: None,
            bottom_fill_color2: None,
            bottom_line_color: None,
        }
    }
}

// =============================================================================
// Histogram Series Data
// =============================================================================

/// Data for histogram series
#[derive(Clone, Debug)]
pub struct HistogramData {
    /// Base value
    pub point: SingleValue,
    /// Color override (optional)
    pub color: Option<String>,
}

// =============================================================================
// SeriesData Enum (Union Type)
// =============================================================================

/// Union type for all series data
#[derive(Clone, Debug)]
pub enum SeriesData {
    Candlestick(Vec<CandlestickData>),
    Bar(Vec<BarData>),
    Line(Vec<LineData>),
    Area(Vec<AreaData>),
    Baseline(Vec<BaselineData>),
    Histogram(Vec<HistogramData>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_value_creation() {
        let sv = SingleValue::new(1699920000, 100.0);
        assert_eq!(sv.timestamp, 1699920000);
        assert_eq!(sv.value, 100.0);
    }

    #[test]
    fn test_line_data_with_color_override() {
        let data = LineData {
            point: SingleValue::new(1699920000, 100.0),
            color: Some("#ff0000".to_string()),
        };
        assert_eq!(data.color, Some("#ff0000".to_string()));
    }

    #[test]
    fn test_candlestick_data_from_bar() {
        let bar = Bar::new(1699920000, 100.0, 105.0, 95.0, 102.0);
        let candlestick = CandlestickData {
            bar,
            color: None,
            border_color: None,
            wick_color: None,
        };
        assert_eq!(candlestick.bar.close, 102.0);
    }

    #[test]
    fn test_baseline_data_default() {
        let data = BaselineData::default();
        assert_eq!(data.point.timestamp, 0);
        assert_eq!(data.point.value, 0.0);
        assert!(data.top_fill_color1.is_none());
    }
}
