//! Legend overlay for OHLC value display
//!
//! Shows OHLC values, change, and percentage change for the bar
//! under the cursor or the last bar.

use serde::{Deserialize, Serialize};

// =============================================================================
// Legend Position
// =============================================================================

/// Position of legend on chart
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum LegendPosition {
    #[serde(rename = "topleft")]
    #[default]
    TopLeft,
    #[serde(rename = "topright")]
    TopRight,
    #[serde(rename = "bottomleft")]
    BottomLeft,
    #[serde(rename = "bottomright")]
    BottomRight,
}


// =============================================================================
// Legend Configuration
// =============================================================================

/// Legend configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Legend {
    /// Visibility of legend
    pub visible: bool,

    /// Position on chart
    #[serde(default)]
    pub position: LegendPosition,

    /// Show OHLC values
    #[serde(default = "default_true")]
    pub show_ohlc: bool,

    /// Show absolute change
    #[serde(default = "default_true")]
    pub show_change: bool,

    /// Show percentage change
    #[serde(default = "default_true")]
    pub show_percent: bool,

    /// Padding from edge (pixels)
    #[serde(default = "default_legend_padding")]
    pub padding: f64,

    /// Font size
    #[serde(default = "default_legend_font_size")]
    pub font_size: f64,

    /// Text color (None = use theme)
    pub text_color: Option<String>,

    /// Background color (None = transparent)
    pub background_color: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_legend_padding() -> f64 {
    10.0
}

fn default_legend_font_size() -> f64 {
    12.0
}

impl Default for Legend {
    fn default() -> Self {
        Self {
            visible: true,
            position: LegendPosition::TopLeft,
            show_ohlc: true,
            show_change: true,
            show_percent: true,
            padding: default_legend_padding(),
            font_size: default_legend_font_size(),
            text_color: None,
            background_color: None,
        }
    }
}

impl Legend {
    /// Calculate legend position
    pub fn calc_position(
        &self,
        chart_width: f64,
        chart_height: f64,
        text_width: f64,
    ) -> (f64, f64) {
        let text_height = self.font_size * 1.5; // Account for line-height

        match self.position {
            LegendPosition::TopLeft => (self.padding, self.padding),
            LegendPosition::TopRight => (chart_width - text_width - self.padding, self.padding),
            LegendPosition::BottomLeft => {
                (self.padding, chart_height - text_height - self.padding)
            }
            LegendPosition::BottomRight => (
                chart_width - text_width - self.padding,
                chart_height - text_height - self.padding,
            ),
        }
    }

    /// Get CSS font string
    pub fn css_font(&self) -> String {
        format!(
            "{}px 'Trebuchet MS', Arial, sans-serif",
            self.font_size
        )
    }
}

// =============================================================================
// Legend Data
// =============================================================================

/// Data to display in legend
#[derive(Clone, Debug)]
pub struct LegendData {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub prev_close: Option<f64>,
}

impl LegendData {
    /// Create legend data from bar
    pub fn from_bar(bar: &crate::Bar, prev_close: Option<f64>) -> Self {
        Self {
            open: bar.open,
            high: bar.high,
            low: bar.low,
            close: bar.close,
            prev_close,
        }
    }

    /// Calculate absolute change
    pub fn change(&self) -> Option<f64> {
        self.prev_close.map(|prev| self.close - prev)
    }

    /// Calculate percentage change
    pub fn change_percent(&self) -> Option<f64> {
        self.prev_close.map(|prev| {
            if prev != 0.0 {
                (self.close - prev) / prev * 100.0
            } else {
                0.0
            }
        })
    }

    /// Format legend text
    ///
    /// Example output: "O: 100.00  H: 105.00  L: 98.00  C: 103.00  +3.00 (+3.00%)"
    pub fn format(&self, legend: &Legend, price_step: f64) -> String {
        use crate::format_price;

        let mut parts = Vec::new();

        if legend.show_ohlc {
            parts.push(format!("O: {}", format_price(self.open, price_step)));
            parts.push(format!("H: {}", format_price(self.high, price_step)));
            parts.push(format!("L: {}", format_price(self.low, price_step)));
            parts.push(format!("C: {}", format_price(self.close, price_step)));
        }

        if legend.show_change || legend.show_percent {
            if let Some(change) = self.change() {
                let sign = if change >= 0.0 { "+" } else { "" };

                if legend.show_change {
                    parts.push(format!("{}{}", sign, format_price(change, price_step)));
                }

                if legend.show_percent {
                    if let Some(pct) = self.change_percent() {
                        parts.push(format!("({}{:.2}%)", sign, pct));
                    }
                }
            }
        }

        parts.join("  ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bar;

    #[test]
    fn test_legend_data_change() {
        let data = LegendData {
            open: 100.0,
            high: 105.0,
            low: 98.0,
            close: 103.0,
            prev_close: Some(100.0),
        };

        assert_eq!(data.change(), Some(3.0));
        assert_eq!(data.change_percent(), Some(3.0));
    }

    #[test]
    fn test_legend_format() {
        let data = LegendData {
            open: 100.0,
            high: 105.0,
            low: 98.0,
            close: 103.0,
            prev_close: Some(100.0),
        };

        let legend = Legend::default();
        let text = data.format(&legend, 0.01);

        assert!(text.contains("O:"));
        assert!(text.contains("H:"));
        assert!(text.contains("L:"));
        assert!(text.contains("C:"));
        assert!(text.contains("+")); // Positive change
    }

    #[test]
    fn test_legend_format_negative_change() {
        let data = LegendData {
            open: 100.0,
            high: 105.0,
            low: 95.0,
            close: 97.0,
            prev_close: Some(100.0),
        };

        let legend = Legend::default();
        let text = data.format(&legend, 0.01);

        assert!(text.contains("-")); // Negative change
    }

    #[test]
    fn test_legend_position_calculation() {
        let legend = Legend {
            position: LegendPosition::TopRight,
            padding: 10.0,
            ..Default::default()
        };

        let (x, y) = legend.calc_position(800.0, 600.0, 200.0);
        assert_eq!(x, 800.0 - 200.0 - 10.0); // Right edge
        assert_eq!(y, 10.0); // Top edge
    }

    #[test]
    fn test_legend_bottom_left() {
        let legend = Legend {
            position: LegendPosition::BottomLeft,
            padding: 10.0,
            font_size: 12.0,
            ..Default::default()
        };

        let (x, y) = legend.calc_position(800.0, 600.0, 200.0);
        assert_eq!(x, 10.0); // Left edge
        let expected_y = 600.0 - 12.0 * 1.5 - 10.0;
        assert!((y - expected_y).abs() < 0.1);
    }
}
