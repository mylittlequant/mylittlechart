//! Price Lines - Horizontal price markers
//!
//! Price lines are horizontal markers with full support
//! for custom styling, axis labels, and dash patterns.

use serde::{Deserialize, Serialize};

/// Line style for price lines
///
/// Each style has a specific dash pattern that scales with line width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "lowercase")]
pub enum LineStyle {
    /// Solid continuous line
    #[default]
    Solid = 0,
    /// Dotted line: [lineWidth, lineWidth]
    Dotted = 1,
    /// Dashed line: [2×lineWidth, 2×lineWidth]
    Dashed = 2,
    /// Large dashed line: [6×lineWidth, 6×lineWidth]
    LargeDashed = 3,
    /// Sparse dotted line: [lineWidth, 4×lineWidth]
    SparseDotted = 4,
}

impl LineStyle {
    /// Get the Canvas2D dash pattern for this style
    ///
    /// Returns an array of [dash_length, gap_length] values.
    /// Empty array means solid line.
    ///
    /// # Arguments
    /// * `line_width` - The width of the line in pixels
    ///
    /// # Returns
    /// Vector of dash pattern values for ctx.setLineDash()
    ///
    /// # Example
    /// ```
    /// use zengeld_chart::LineStyle;
    /// let pattern = LineStyle::Dashed.dash_pattern(2.0);
    /// assert_eq!(pattern, vec![4.0, 4.0]);
    /// ```
    pub fn dash_pattern(&self, line_width: f64) -> Vec<f64> {
        match self {
            LineStyle::Solid => vec![],
            LineStyle::Dotted => vec![line_width, line_width],
            LineStyle::Dashed => vec![2.0 * line_width, 2.0 * line_width],
            LineStyle::LargeDashed => vec![6.0 * line_width, 6.0 * line_width],
            LineStyle::SparseDotted => vec![line_width, 4.0 * line_width],
        }
    }

    /// Get a human-readable name for debugging
    pub fn name(&self) -> &'static str {
        match self {
            LineStyle::Solid => "Solid",
            LineStyle::Dotted => "Dotted",
            LineStyle::Dashed => "Dashed",
            LineStyle::LargeDashed => "LargeDashed",
            LineStyle::SparseDotted => "SparseDotted",
        }
    }

    /// Get string representation for serialization
    pub fn as_str(&self) -> &'static str {
        match self {
            LineStyle::Solid => "solid",
            LineStyle::Dotted => "dotted",
            LineStyle::Dashed => "dashed",
            LineStyle::LargeDashed => "large_dashed",
            LineStyle::SparseDotted => "sparse_dotted",
        }
    }

    /// Parse from string
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "solid" => LineStyle::Solid,
            "dotted" => LineStyle::Dotted,
            "dashed" => LineStyle::Dashed,
            "large_dashed" | "largedashed" => LineStyle::LargeDashed,
            "sparse_dotted" | "sparsedotted" => LineStyle::SparseDotted,
            _ => LineStyle::Solid,
        }
    }
}

/// Parse LineStyle from integer value (for JS interop)
impl From<u8> for LineStyle {
    fn from(value: u8) -> Self {
        match value {
            0 => LineStyle::Solid,
            1 => LineStyle::Dotted,
            2 => LineStyle::Dashed,
            3 => LineStyle::LargeDashed,
            4 => LineStyle::SparseDotted,
            _ => LineStyle::Solid, // fallback
        }
    }
}

impl From<LineStyle> for u8 {
    fn from(style: LineStyle) -> u8 {
        style as u8
    }
}

/// A horizontal price line on the chart
///
/// Price lines are horizontal markers at a specific price level.
/// They can be styled with different colors, line styles, and can
/// display labels on the price axis and titles on the chart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceLine {
    /// Unique identifier for this price line
    pub id: String,

    /// Price level (Y coordinate in price space)
    pub price: f64,

    /// Line color in CSS format (hex, rgb, rgba)
    #[serde(default = "default_color")]
    pub color: String,

    /// Line width in pixels (1-4)
    #[serde(default = "default_line_width")]
    pub line_width: u8,

    /// Line style (solid, dotted, dashed, etc.)
    #[serde(default)]
    pub line_style: LineStyle,

    /// Whether the line itself is visible
    #[serde(default = "default_true")]
    pub line_visible: bool,

    /// Whether to show a label on the price axis
    #[serde(default = "default_true")]
    pub axis_label_visible: bool,

    /// Optional title text displayed on the chart
    #[serde(default)]
    pub title: String,

    /// Custom background color for axis label (defaults to line color)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub axis_label_color: String,

    /// Custom text color for axis label (defaults to auto-contrast)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub axis_label_text_color: String,
}

// Default value functions for serde
fn default_color() -> String {
    "#2962ff".to_string()
}

fn default_line_width() -> u8 {
    1
}

fn default_true() -> bool {
    true
}

impl PriceLine {
    /// Create a new price line with required fields
    pub fn new(id: impl Into<String>, price: f64) -> Self {
        Self {
            id: id.into(),
            price,
            color: default_color(),
            line_width: 1,
            line_style: LineStyle::Solid,
            line_visible: true,
            axis_label_visible: true,
            title: String::new(),
            axis_label_color: String::new(),
            axis_label_text_color: String::new(),
        }
    }

    /// Builder: set line color
    pub fn with_color(mut self, color: impl Into<String>) -> Self {
        self.color = color.into();
        self
    }

    /// Builder: set line width (clamped to 1-4)
    pub fn with_line_width(mut self, width: u8) -> Self {
        self.line_width = width.clamp(1, 4);
        self
    }

    /// Builder: set line style
    pub fn with_line_style(mut self, style: LineStyle) -> Self {
        self.line_style = style;
        self
    }

    /// Builder: set visibility
    pub fn with_line_visible(mut self, visible: bool) -> Self {
        self.line_visible = visible;
        self
    }

    /// Builder: set axis label visibility
    pub fn with_axis_label_visible(mut self, visible: bool) -> Self {
        self.axis_label_visible = visible;
        self
    }

    /// Builder: set title
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Get effective axis label color (defaults to line color)
    pub fn effective_axis_label_color(&self) -> &str {
        if self.axis_label_color.is_empty() {
            &self.color
        } else {
            &self.axis_label_color
        }
    }

    /// Validate line width (must be 1-4)
    pub fn validate(&self) -> Result<(), String> {
        if self.line_width < 1 || self.line_width > 4 {
            return Err(format!(
                "Invalid line_width: {}. Must be 1-4.",
                self.line_width
            ));
        }
        if self.color.is_empty() {
            return Err("Color cannot be empty".to_string());
        }
        Ok(())
    }
}

/// Options for creating or updating a price line
///
/// All fields are optional except price (required for creation).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PriceLineOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_width: Option<u8>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_style: Option<LineStyle>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_visible: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub axis_label_visible: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub axis_label_color: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub axis_label_text_color: Option<String>,
}

impl PriceLineOptions {
    /// Create options with only price (minimum for creation)
    pub fn with_price(price: f64) -> Self {
        Self {
            price: Some(price),
            ..Default::default()
        }
    }

    /// Apply these options to an existing PriceLine
    pub fn apply_to(&self, line: &mut PriceLine) {
        if let Some(price) = self.price {
            line.price = price;
        }
        if let Some(ref color) = self.color {
            line.color = color.clone();
        }
        if let Some(width) = self.line_width {
            line.line_width = width.clamp(1, 4);
        }
        if let Some(style) = self.line_style {
            line.line_style = style;
        }
        if let Some(visible) = self.line_visible {
            line.line_visible = visible;
        }
        if let Some(visible) = self.axis_label_visible {
            line.axis_label_visible = visible;
        }
        if let Some(ref title) = self.title {
            line.title = title.clone();
        }
        if let Some(ref color) = self.axis_label_color {
            line.axis_label_color = color.clone();
        }
        if let Some(ref color) = self.axis_label_text_color {
            line.axis_label_text_color = color.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dash_patterns() {
        assert_eq!(LineStyle::Solid.dash_pattern(2.0), Vec::<f64>::new());
        assert_eq!(LineStyle::Dotted.dash_pattern(2.0), vec![2.0, 2.0]);
        assert_eq!(LineStyle::Dashed.dash_pattern(2.0), vec![4.0, 4.0]);
        assert_eq!(LineStyle::LargeDashed.dash_pattern(2.0), vec![12.0, 12.0]);
        assert_eq!(LineStyle::SparseDotted.dash_pattern(2.0), vec![2.0, 8.0]);
    }

    #[test]
    fn test_line_style_from_u8() {
        assert_eq!(LineStyle::from(0), LineStyle::Solid);
        assert_eq!(LineStyle::from(1), LineStyle::Dotted);
        assert_eq!(LineStyle::from(2), LineStyle::Dashed);
        assert_eq!(LineStyle::from(3), LineStyle::LargeDashed);
        assert_eq!(LineStyle::from(4), LineStyle::SparseDotted);
        assert_eq!(LineStyle::from(99), LineStyle::Solid); // fallback
    }

    #[test]
    fn test_price_line_builder() {
        let line = PriceLine::new("test", 100.0)
            .with_color("#ff0000")
            .with_line_width(2)
            .with_line_style(LineStyle::Dashed)
            .with_title("Test Line");

        assert_eq!(line.id, "test");
        assert_eq!(line.price, 100.0);
        assert_eq!(line.color, "#ff0000");
        assert_eq!(line.line_width, 2);
        assert_eq!(line.line_style, LineStyle::Dashed);
        assert_eq!(line.title, "Test Line");
    }

    #[test]
    fn test_line_width_validation() {
        let line = PriceLine::new("test", 100.0).with_line_width(10);
        assert_eq!(line.line_width, 4); // clamped to max

        let line = PriceLine::new("test", 100.0).with_line_width(0);
        assert_eq!(line.line_width, 1); // clamped to min
    }

    #[test]
    fn test_effective_axis_label_color() {
        let line = PriceLine::new("test", 100.0).with_color("#ff0000");
        assert_eq!(line.effective_axis_label_color(), "#ff0000");

        let mut line_custom = line;
        line_custom.axis_label_color = "#00ff00".to_string();
        assert_eq!(line_custom.effective_axis_label_color(), "#00ff00");
    }

    #[test]
    fn test_options_apply() {
        let mut line = PriceLine::new("test", 100.0);

        let options = PriceLineOptions {
            price: Some(150.0),
            color: Some("#00ff00".to_string()),
            line_width: Some(3),
            ..Default::default()
        };

        options.apply_to(&mut line);

        assert_eq!(line.price, 150.0);
        assert_eq!(line.color, "#00ff00");
        assert_eq!(line.line_width, 3);
    }

    #[test]
    fn test_validation() {
        let valid = PriceLine::new("test", 100.0)
            .with_line_width(2)
            .with_color("#ff0000");
        assert!(valid.validate().is_ok());

        let mut invalid_color = PriceLine::new("test", 100.0);
        invalid_color.color = String::new();
        assert!(invalid_color.validate().is_err());
    }
}
