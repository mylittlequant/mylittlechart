//! Tooltip overlay for detailed bar information
//!
//! Displays a floating box with time and OHLC data when hovering
//! over the chart. Automatically flips position to stay within bounds.

use serde::{Deserialize, Serialize};

// =============================================================================
// Tooltip Content
// =============================================================================

/// Content to display in tooltip
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TooltipContent {
    pub time: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: Option<f64>,
}

impl TooltipContent {
    /// Create tooltip content from bar
    pub fn from_bar(bar: &crate::Bar, volume: Option<f64>) -> Self {
        use crate::chart::types::format_time_full;

        Self {
            time: format_time_full(bar.timestamp),
            open: bar.open,
            high: bar.high,
            low: bar.low,
            close: bar.close,
            volume,
        }
    }

    /// Format tooltip lines
    pub fn format_lines(&self, price_step: f64) -> Vec<String> {
        use crate::format_price;

        let mut lines = vec![
            format!("Time: {}", self.time),
            format!("Open: {}", format_price(self.open, price_step)),
            format!("High: {}", format_price(self.high, price_step)),
            format!("Low: {}", format_price(self.low, price_step)),
            format!("Close: {}", format_price(self.close, price_step)),
        ];

        if let Some(vol) = self.volume {
            lines.push(format!("Volume: {:.0}", vol));
        }

        lines
    }
}

// =============================================================================
// Tooltip Configuration
// =============================================================================

/// Tooltip configuration and state
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tooltip {
    /// Visibility of tooltip
    pub visible: bool,

    /// Whether tooltip follows cursor (vs fixed position)
    #[serde(default = "default_follow_cursor")]
    pub follow_cursor: bool,

    /// Bar index (runtime state, not serialized)
    #[serde(skip)]
    pub bar_idx: Option<usize>,

    /// X position in pixels (runtime state)
    #[serde(skip)]
    pub x: f64,

    /// Y position in pixels (runtime state)
    #[serde(skip)]
    pub y: f64,

    /// Tooltip content (runtime state)
    #[serde(skip)]
    pub content: Option<TooltipContent>,

    /// Offset from cursor X
    #[serde(default = "default_tooltip_offset")]
    pub offset_x: f64,

    /// Offset from cursor Y
    #[serde(default = "default_tooltip_offset")]
    pub offset_y: f64,

    /// Background color
    pub background_color: String,

    /// Text color
    pub text_color: String,

    /// Border color
    pub border_color: String,

    /// Font size
    #[serde(default = "default_tooltip_font_size")]
    pub font_size: f64,

    /// Internal padding
    #[serde(default = "default_tooltip_padding")]
    pub padding: f64,
}

fn default_tooltip_offset() -> f64 {
    10.0
}

fn default_follow_cursor() -> bool {
    true
}

fn default_tooltip_font_size() -> f64 {
    11.0
}

fn default_tooltip_padding() -> f64 {
    8.0
}

impl Default for Tooltip {
    fn default() -> Self {
        Self {
            visible: false,
            follow_cursor: true,
            bar_idx: None,
            x: 0.0,
            y: 0.0,
            content: None,
            offset_x: 10.0,
            offset_y: 10.0,
            background_color: "rgba(30, 34, 45, 0.95)".to_string(),
            text_color: "#b2b5be".to_string(),
            border_color: "#2a2e39".to_string(),
            font_size: 11.0,
            padding: 8.0,
        }
    }
}

impl Tooltip {
    /// Update tooltip position and content
    pub fn update(
        &mut self,
        x: f64,
        y: f64,
        bar_idx: Option<usize>,
        content: Option<TooltipContent>,
    ) {
        self.x = x;
        self.y = y;
        self.bar_idx = bar_idx;
        let is_some = content.is_some();
        self.content = content;
        self.visible = is_some;
    }

    /// Hide tooltip
    pub fn hide(&mut self) {
        self.visible = false;
        self.content = None;
    }

    /// Get CSS font string
    pub fn css_font(&self) -> String {
        format!(
            "{}px 'Trebuchet MS', Arial, sans-serif",
            self.font_size
        )
    }

    /// Calculate tooltip size
    pub fn calc_size<F>(&self, price_step: f64, measure_text: F) -> (f64, f64)
    where
        F: Fn(&str) -> f64,
    {
        if let Some(content) = &self.content {
            let lines = content.format_lines(price_step);

            let max_width = lines
                .iter()
                .map(|line| measure_text(line))
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap_or(100.0);

            let line_height = self.font_size * 1.4;
            let total_height = lines.len() as f64 * line_height;

            (
                max_width + self.padding * 2.0,
                total_height + self.padding * 2.0,
            )
        } else {
            (0.0, 0.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bar;

    #[test]
    fn test_tooltip_content_from_bar() {
        let bar = Bar::new(1699920000, 100.0, 105.0, 98.0, 103.0);
        let content = TooltipContent::from_bar(&bar, Some(10000.0));

        assert_eq!(content.open, 100.0);
        assert_eq!(content.close, 103.0);
        assert_eq!(content.volume, Some(10000.0));
    }

    #[test]
    fn test_tooltip_format_lines() {
        let content = TooltipContent {
            time: "2023-11-14 00:00".to_string(),
            open: 100.0,
            high: 105.0,
            low: 98.0,
            close: 103.0,
            volume: Some(10000.0),
        };

        let lines = content.format_lines(0.01);
        assert_eq!(lines.len(), 6); // Time + OHLC + Volume
        assert!(lines[0].contains("Time:"));
        assert!(lines[5].contains("Volume:"));
    }

    #[test]
    fn test_tooltip_hide() {
        let mut tooltip = Tooltip::default();
        tooltip.visible = true;
        tooltip.content = Some(TooltipContent {
            time: "test".to_string(),
            open: 100.0,
            high: 105.0,
            low: 98.0,
            close: 103.0,
            volume: None,
        });

        tooltip.hide();
        assert!(!tooltip.visible);
        assert!(tooltip.content.is_none());
    }
}
