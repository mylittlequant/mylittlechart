//! Series types and options
//!
//! Supported series types:
//! - Line: Simple, stepped, or curved lines
//! - Area: Gradient-filled area charts
//! - Baseline: Split fill above/below a baseline
//! - Histogram: Vertical bars from a base value
//! - Candlestick: OHLC candles
//! - Bar: OHLC bars with ticks

pub mod data;
pub mod enums;
pub mod options;

// Re-export main types
pub use data::{
    AreaData, BarData, BaselineData, CandlestickData, HistogramData, LineData, SeriesData,
    SingleValue,
};
pub use enums::{LineStyle, LineType, PriceLineSource};
pub use options::{
    AreaSeriesOptions, AreaStyleOptions, BarSeriesOptions, BarStyleOptions,
    BaselineSeriesOptions, BaselineStyleOptions, CandlestickSeriesOptions,
    CandlestickStyleOptions, HistogramSeriesOptions, HistogramStyleOptions, LineSeriesOptions,
    LineStyleOptions, SeriesOptions, SeriesOptionsCommon,
};

/// Series type enum
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeriesType {
    /// Candlestick chart (OHLC with body and wicks)
    Candlestick,
    /// Bar chart (OHLC vertical line with ticks)
    Bar,
    /// Line chart (connects points)
    Line,
    /// Area chart with gradient fill
    Area,
    /// Baseline chart with split fill
    Baseline,
    /// Histogram (vertical bars from baseline)
    Histogram,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_series_type_enum() {
        assert_eq!(SeriesType::Line, SeriesType::Line);
        assert_ne!(SeriesType::Line, SeriesType::Area);
    }

    #[test]
    fn test_single_value() {
        let val = SingleValue::new(1699920000, 100.0);
        assert_eq!(val.timestamp, 1699920000);
        assert_eq!(val.value, 100.0);
    }

    #[test]
    fn test_line_data_creation() {
        let data = LineData {
            point: SingleValue::new(0, 50.0),
            color: None,
        };
        assert_eq!(data.point.value, 50.0);
    }

    #[test]
    fn test_line_style_options() {
        let opts = LineStyleOptions::default();
        assert_eq!(opts.line_type, LineType::Simple);
        assert_eq!(opts.line_style, LineStyle::Solid);
    }
}
