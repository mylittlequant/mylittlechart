//! Compare Overlay - overlay another symbol as a line chart for comparison
//!
//! This module provides functionality to overlay additional symbols on the main chart
//! for comparison purposes. Values are displayed as relative percentages from a base point.

use serde::{Serialize, Deserialize};
use crate::Bar;
use crate::chart::types::PriceScaleMode;
use crate::drawing::primitives_v2::config::TimeframeVisibilityConfig;

/// A single compare series (one symbol overlay)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompareSeries {
    /// Symbol ticker (e.g., "AAPL", "BTCUSD")
    pub symbol: String,
    /// Display name for legend
    pub name: String,
    /// Original bar data for this symbol
    pub bars: Vec<Bar>,
    /// Line color (hex string like "#2196F3")
    pub color: String,
    /// Line width in pixels
    pub line_width: f32,
    /// Line style: "solid", "dashed", "dotted"
    #[serde(default = "default_line_style")]
    pub line_style: String,
    /// Whether this series is visible
    pub visible: bool,
    /// Base price for percentage calculation (first visible bar's close)
    pub base_price: f64,
    /// Base timestamp (when comparison starts)
    pub base_timestamp: i64,
    /// Optional per-timeframe visibility configuration.
    /// When `None`, the series is visible on all timeframes.
    #[serde(default)]
    pub timeframe_visibility: Option<TimeframeVisibilityConfig>,
}

fn default_line_style() -> String {
    "solid".to_string()
}

impl CompareSeries {
    /// Create a new compare series
    pub fn new(symbol: impl Into<String>, bars: Vec<Bar>, color: impl Into<String>) -> Self {
        let symbol = symbol.into();
        let base_price = bars.first().map(|b| b.close).unwrap_or(100.0);
        let base_timestamp = bars.first().map(|b| b.timestamp).unwrap_or(0);

        Self {
            name: symbol.clone(),
            symbol,
            bars,
            color: color.into(),
            line_width: 2.0,
            line_style: "solid".to_string(),
            visible: true,
            base_price,
            base_timestamp,
            timeframe_visibility: None,
        }
    }

    /// Get percentage value at a given timestamp
    /// Returns (timestamp, percent_change) or None if not found
    pub fn get_percent_at_timestamp(&self, timestamp: i64) -> Option<f64> {
        // Find the bar with matching or closest timestamp
        self.bars
            .iter()
            .find(|b| b.timestamp == timestamp)
            .map(|bar| self.price_to_percent(bar.close))
    }

    /// Convert absolute price to percentage change from base
    #[inline]
    pub fn price_to_percent(&self, price: f64) -> f64 {
        if self.base_price == 0.0 {
            return 0.0;
        }
        ((price - self.base_price) / self.base_price) * 100.0
    }

    /// Get all data points as (timestamp, percent) pairs
    pub fn get_percent_data(&self) -> Vec<(i64, f64)> {
        self.bars
            .iter()
            .map(|bar| (bar.timestamp, self.price_to_percent(bar.close)))
            .collect()
    }

    /// Update base price (recalculates all percentages)
    pub fn set_base_price(&mut self, price: f64) {
        self.base_price = price;
    }

    /// Reset base price to first bar
    pub fn reset_base(&mut self) {
        if let Some(first) = self.bars.first() {
            self.base_price = first.close;
            self.base_timestamp = first.timestamp;
        }
    }

    /// Set base to a specific timestamp
    pub fn set_base_timestamp(&mut self, timestamp: i64) {
        if let Some(bar) = self.bars.iter().find(|b| b.timestamp == timestamp) {
            self.base_price = bar.close;
            self.base_timestamp = timestamp;
        }
    }
}

/// Compare overlay state - manages multiple comparison series
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CompareOverlay {
    /// List of comparison series
    pub series: Vec<CompareSeries>,
    /// Whether compare mode is active (changes price scale to %)
    pub active: bool,
    /// Price scale mode when compare is active
    pub scale_mode: PriceScaleMode,
    /// Main symbol's base price (for percentage calculation)
    pub main_base_price: f64,
    /// Main symbol's base timestamp
    pub main_base_timestamp: i64,
}

impl CompareOverlay {
    /// Create a new empty compare overlay
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a comparison series
    pub fn add_series(&mut self, series: CompareSeries) {
        self.series.push(series);
        self.active = true;
        // Auto-switch to percent mode when adding comparison
        if self.scale_mode == PriceScaleMode::Normal {
            self.scale_mode = PriceScaleMode::Percent;
        }
    }

    /// Remove a comparison series by symbol
    pub fn remove_series_by_symbol(&mut self, symbol: &str) {
        self.series.retain(|s| s.symbol != symbol);
        if self.series.is_empty() {
            self.active = false;
            self.scale_mode = PriceScaleMode::Normal;
        }
    }

    /// Remove a comparison series by index
    /// Returns the removed symbol if successful
    pub fn remove_series(&mut self, index: usize) -> Option<String> {
        if index < self.series.len() {
            let removed = self.series.remove(index);
            if self.series.is_empty() {
                self.active = false;
                self.scale_mode = PriceScaleMode::Normal;
            }
            Some(removed.symbol)
        } else {
            None
        }
    }

    /// Toggle visibility of a series by index
    /// Returns the new visibility state
    pub fn toggle_visibility(&mut self, index: usize) -> bool {
        if let Some(series) = self.series.get_mut(index) {
            series.visible = !series.visible;
            series.visible
        } else {
            false
        }
    }

    /// Clear all comparison series
    pub fn clear(&mut self) {
        self.series.clear();
        self.active = false;
        self.scale_mode = PriceScaleMode::Normal;
    }

    /// Check if a symbol is already being compared
    pub fn has_symbol(&self, symbol: &str) -> bool {
        self.series.iter().any(|s| s.symbol == symbol)
    }

    /// Get reference to a series by symbol
    pub fn get_series(&self, symbol: &str) -> Option<&CompareSeries> {
        self.series.iter().find(|s| s.symbol == symbol)
    }

    /// Get mutable reference to a series by symbol
    pub fn get_series_mut(&mut self, symbol: &str) -> Option<&mut CompareSeries> {
        self.series.iter_mut().find(|s| s.symbol == symbol)
    }

    /// Set the main symbol's base price (for its percentage calculation)
    pub fn set_main_base(&mut self, price: f64, timestamp: i64) {
        self.main_base_price = price;
        self.main_base_timestamp = timestamp;
    }

    /// Calculate main symbol percentage from base
    #[inline]
    pub fn main_price_to_percent(&self, price: f64) -> f64 {
        if self.main_base_price == 0.0 {
            return 0.0;
        }
        ((price - self.main_base_price) / self.main_base_price) * 100.0
    }

    /// Synchronize all series base timestamps to a common point
    pub fn sync_bases_to_timestamp(&mut self, timestamp: i64) {
        self.main_base_timestamp = timestamp;
        for series in &mut self.series {
            series.set_base_timestamp(timestamp);
        }
    }

    /// Get the min/max percentage values across all visible series in a time range
    pub fn get_percent_range(
        &self,
        main_bars: &[Bar],
        visible_range: (usize, usize),
    ) -> Option<(f64, f64)> {
        if !self.active || self.scale_mode != PriceScaleMode::Percent {
            return None;
        }

        let (start, end) = visible_range;
        if start >= end || main_bars.is_empty() {
            return None;
        }

        let mut min_pct = f64::INFINITY;
        let mut max_pct = f64::NEG_INFINITY;

        // Get timestamp range from main bars
        let timestamps: Vec<i64> = main_bars[start..end.min(main_bars.len())]
            .iter()
            .map(|b| b.timestamp)
            .collect();

        // Calculate main symbol percentages
        for i in start..end.min(main_bars.len()) {
            let pct = self.main_price_to_percent(main_bars[i].close);
            min_pct = min_pct.min(pct);
            max_pct = max_pct.max(pct);
            // Also consider high/low for main
            let pct_high = self.main_price_to_percent(main_bars[i].high);
            let pct_low = self.main_price_to_percent(main_bars[i].low);
            min_pct = min_pct.min(pct_low);
            max_pct = max_pct.max(pct_high);
        }

        // Calculate compare series percentages
        for series in &self.series {
            if !series.visible {
                continue;
            }
            for &ts in &timestamps {
                if let Some(pct) = series.get_percent_at_timestamp(ts) {
                    min_pct = min_pct.min(pct);
                    max_pct = max_pct.max(pct);
                }
            }
        }

        if min_pct.is_finite() && max_pct.is_finite() {
            Some((min_pct, max_pct))
        } else {
            None
        }
    }

    /// Get number of visible series
    pub fn visible_count(&self) -> usize {
        self.series.iter().filter(|s| s.visible).count()
    }

    /// Toggle visibility of a series
    pub fn toggle_series_visibility(&mut self, symbol: &str) {
        if let Some(series) = self.get_series_mut(symbol) {
            series.visible = !series.visible;
        }
    }

    /// Set visibility of a series
    pub fn set_series_visibility(&mut self, symbol: &str, visible: bool) {
        if let Some(series) = self.get_series_mut(symbol) {
            series.visible = visible;
        }
    }

    /// Set color of a series by symbol
    pub fn set_series_color(&mut self, symbol: &str, color: impl Into<String>) {
        if let Some(series) = self.get_series_mut(symbol) {
            series.color = color.into();
        }
    }

    /// Set color of a series by index
    pub fn set_series_color_by_index(&mut self, index: usize, color: impl Into<String>) {
        if let Some(series) = self.series.get_mut(index) {
            series.color = color.into();
        }
    }

    /// Get color of a series
    pub fn get_series_color(&self, symbol: &str) -> Option<&str> {
        self.series
            .iter()
            .find(|s| s.symbol == symbol)
            .map(|s| s.color.as_str())
    }

    /// Set line width of a series by index
    pub fn set_series_line_width_by_index(&mut self, index: usize, line_width: f32) {
        if let Some(series) = self.series.get_mut(index) {
            series.line_width = line_width;
        }
    }

    /// Set line style of a series by index ("solid", "dashed", "dotted")
    pub fn set_series_line_style_by_index(&mut self, index: usize, line_style: impl Into<String>) {
        if let Some(series) = self.series.get_mut(index) {
            series.line_style = line_style.into();
        }
    }

    /// Set the timeframe visibility config for a series by index.
    pub fn set_series_timeframe_visibility(&mut self, index: usize, config: TimeframeVisibilityConfig) {
        if let Some(series) = self.series.get_mut(index) {
            series.timeframe_visibility = Some(config);
        }
    }

    /// Get the timeframe visibility config for a series by index.
    pub fn get_series_timeframe_visibility(&self, index: usize) -> Option<&TimeframeVisibilityConfig> {
        self.series.get(index).and_then(|s| s.timeframe_visibility.as_ref())
    }
}

/// Default colors for compare series (cycling palette)
pub const COMPARE_COLORS: [&str; 8] = [
    "#2196F3", // Blue
    "#FF9800", // Orange
    "#9C27B0", // Purple
    "#4CAF50", // Green
    "#F44336", // Red
    "#00BCD4", // Cyan
    "#FFEB3B", // Yellow
    "#E91E63", // Pink
];

/// Get a color for a new compare series based on index
pub fn get_compare_color(index: usize) -> &'static str {
    COMPARE_COLORS[index % COMPARE_COLORS.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_bars() -> Vec<Bar> {
        vec![
            Bar {
                timestamp: 1000,
                open: 100.0,
                high: 105.0,
                low: 95.0,
                close: 100.0,
                volume: 1000.0,
            },
            Bar {
                timestamp: 2000,
                open: 100.0,
                high: 110.0,
                low: 98.0,
                close: 105.0,
                volume: 1000.0,
            },
            Bar {
                timestamp: 3000,
                open: 105.0,
                high: 115.0,
                low: 100.0,
                close: 110.0,
                volume: 1000.0,
            },
        ]
    }

    #[test]
    fn test_compare_series_percent() {
        let bars = create_test_bars();
        let series = CompareSeries::new("TEST", bars, "#2196F3");

        // Base is 100, so 105 should be +5%
        assert_eq!(series.price_to_percent(105.0), 5.0);
        assert_eq!(series.price_to_percent(110.0), 10.0);
        assert_eq!(series.price_to_percent(90.0), -10.0);
    }

    #[test]
    fn test_compare_overlay_add_remove() {
        let mut overlay = CompareOverlay::new();
        assert!(!overlay.active);

        let bars = create_test_bars();
        overlay.add_series(CompareSeries::new("AAPL", bars.clone(), "#2196F3"));

        assert!(overlay.active);
        assert_eq!(overlay.scale_mode, PriceScaleMode::Percent);
        assert!(overlay.has_symbol("AAPL"));

        overlay.remove_series_by_symbol("AAPL");
        assert!(!overlay.active);
        assert!(!overlay.has_symbol("AAPL"));
    }

    #[test]
    fn test_price_scale_mode_cycle() {
        // Cycle: lin -> log -> % -> lin
        let mode = PriceScaleMode::Normal;
        assert_eq!(mode.next(), PriceScaleMode::Logarithmic);
        assert_eq!(mode.next().next(), PriceScaleMode::Percent);
        assert_eq!(mode.next().next().next(), PriceScaleMode::Normal);
    }
}
