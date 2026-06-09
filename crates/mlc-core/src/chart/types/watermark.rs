//! Watermark overlay for chart branding
//!
//! Provides configurable text watermarks with multi-line support,
//! alignment, and styling options.

use serde::{Deserialize, Serialize};

// =============================================================================
// Alignment Enums
// =============================================================================

/// Horizontal alignment of watermark
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum HorzAlign {
    Left,
    #[default]
    Center,
    Right,
}


/// Vertical alignment of watermark
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum VertAlign {
    Top,
    #[default]
    Center,
    Bottom,
}


/// Font style for watermark text
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Bold,
    #[serde(rename = "bold italic")]
    BoldItalic,
}


// =============================================================================
// Watermark Line
// =============================================================================

/// Single line of watermark text (for multi-line watermarks)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WatermarkLine {
    pub text: String,
    pub color: String,
    pub font_size: f64,
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default)]
    pub font_style: FontStyle,
}

fn default_font_family() -> String {
    "Arial".to_string()
}

impl WatermarkLine {
    /// Create a new watermark line with minimal parameters
    pub fn new(text: impl Into<String>, color: impl Into<String>, font_size: f64) -> Self {
        Self {
            text: text.into(),
            color: color.into(),
            font_size,
            font_family: default_font_family(),
            font_style: FontStyle::Normal,
        }
    }

    /// Get CSS font string for Canvas rendering
    pub fn css_font(&self) -> String {
        let style_str = match self.font_style {
            FontStyle::Normal => "",
            FontStyle::Italic => "italic ",
            FontStyle::Bold => "bold ",
            FontStyle::BoldItalic => "italic bold ",
        };
        format!("{}{}px {}", style_str, self.font_size, self.font_family)
    }
}

// =============================================================================
// Watermark
// =============================================================================

/// Watermark configuration (v4 compatible + v5 extensions)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Watermark {
    /// Visibility of watermark
    pub visible: bool,

    /// Horizontal alignment
    #[serde(default)]
    pub horz_align: HorzAlign,

    /// Vertical alignment
    #[serde(default)]
    pub vert_align: VertAlign,

    /// Watermark lines (v5-style multi-line support)
    /// For simple watermark use one element
    pub lines: Vec<WatermarkLine>,

    /// Padding from edges (pixels)
    #[serde(default = "default_padding")]
    pub padding: f64,
}

fn default_padding() -> f64 {
    20.0
}

impl Default for Watermark {
    fn default() -> Self {
        Self {
            visible: false, // Hidden by default
            horz_align: HorzAlign::Left,
            vert_align: VertAlign::Bottom,
            lines: vec![WatermarkLine::new(
                "SEE YOU...",
                "#2a2e39", // Theme border color (will be overridden at render time)
                24.0,
            )],
            padding: default_padding(),
        }
    }
}

impl Watermark {
    /// Create a simple single-line watermark (v4-style)
    pub fn simple(text: impl Into<String>) -> Self {
        Self {
            visible: true,
            horz_align: HorzAlign::Left,
            vert_align: VertAlign::Bottom,
            lines: vec![WatermarkLine::new(
                text,
                "#2a2e39", // Theme border color
                24.0,
            )],
            padding: 20.0,
        }
    }

    /// Set text for first line (convenience method)
    pub fn set_text(&mut self, text: impl Into<String>) {
        if self.lines.is_empty() {
            self.lines.push(WatermarkLine::new(text, "#2a2e39", 24.0));
        } else {
            self.lines[0].text = text.into();
        }
    }

    /// Set color for first line (convenience method)
    pub fn set_color(&mut self, color: impl Into<String>) {
        if !self.lines.is_empty() {
            self.lines[0].color = color.into();
        }
    }

    /// Set font size for first line (convenience method)
    pub fn set_font_size(&mut self, size: f64) {
        if !self.lines.is_empty() {
            self.lines[0].font_size = size;
        }
    }

    /// Create a multi-line watermark (v5-style)
    pub fn multi_line(lines: Vec<WatermarkLine>) -> Self {
        Self {
            visible: true,
            horz_align: HorzAlign::Center,
            vert_align: VertAlign::Center,
            lines,
            padding: 20.0,
        }
    }

    /// Set alignment
    pub fn with_alignment(mut self, horz: HorzAlign, vert: VertAlign) -> Self {
        self.horz_align = horz;
        self.vert_align = vert;
        self
    }

    /// Calculate rendering positions for all lines
    ///
    /// Returns (x, y, &WatermarkLine) for each line with alignment applied.
    /// The `measure_text` callback is used to measure text width.
    pub fn calc_positions<F>(
        &self,
        chart_width: f64,
        chart_height: f64,
        measure_text: F,
    ) -> Vec<(f64, f64, &WatermarkLine)>
    where
        F: Fn(&str, &str) -> f64, // (text, font) -> width
    {
        if !self.visible || self.lines.is_empty() {
            return vec![];
        }

        let mut positions = Vec::new();
        let line_spacing = 1.2; // CSS line-height equivalent

        // Calculate total block height
        let total_height: f64 = self
            .lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let h = line.font_size;
                if i > 0 {
                    h * line_spacing
                } else {
                    h
                }
            })
            .sum();

        // Starting Y position of block
        let block_y = match self.vert_align {
            VertAlign::Top => self.padding,
            VertAlign::Center => (chart_height - total_height) / 2.0,
            VertAlign::Bottom => chart_height - total_height - self.padding,
        };

        let mut current_y = block_y;

        for line in &self.lines {
            let text_width = measure_text(&line.text, &line.css_font());

            // X position for this line
            let x = match self.horz_align {
                HorzAlign::Left => self.padding,
                HorzAlign::Center => (chart_width - text_width) / 2.0,
                HorzAlign::Right => chart_width - text_width - self.padding,
            };

            positions.push((x, current_y, line));
            current_y += line.font_size * line_spacing;
        }

        positions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watermark_simple() {
        let wm = Watermark::simple("Test");
        assert!(wm.visible);
        assert_eq!(wm.lines.len(), 1);
        assert_eq!(wm.lines[0].text, "Test");
    }

    #[test]
    fn test_watermark_alignment() {
        let wm = Watermark::simple("Test").with_alignment(HorzAlign::Left, VertAlign::Top);
        assert_eq!(wm.horz_align, HorzAlign::Left);
        assert_eq!(wm.vert_align, VertAlign::Top);
    }

    #[test]
    fn test_css_font() {
        let line = WatermarkLine {
            text: "Test".into(),
            color: "#fff".into(),
            font_size: 24.0,
            font_family: "Arial".into(),
            font_style: FontStyle::Bold,
        };
        assert_eq!(line.css_font(), "bold 24px Arial");
    }

    #[test]
    fn test_position_calculation() {
        // simple() uses Left alignment with padding=20
        let wm = Watermark::simple("ABC");
        let measure = |_text: &str, _font: &str| 100.0; // Mock width

        let positions = wm.calc_positions(800.0, 600.0, measure);
        assert_eq!(positions.len(), 1);

        // Left alignment with padding: x = 20.0
        let (x, _y, _line) = positions[0];
        assert!((x - 20.0).abs() < 0.1);
    }

    #[test]
    fn test_position_calculation_center() {
        // Create center-aligned watermark
        let wm = Watermark::simple("ABC").with_alignment(HorzAlign::Center, VertAlign::Center);
        let measure = |_text: &str, _font: &str| 100.0; // Mock width

        let positions = wm.calc_positions(800.0, 600.0, measure);
        assert_eq!(positions.len(), 1);

        // Center alignment: x = (800 - 100) / 2 = 350
        let (x, _y, _line) = positions[0];
        assert!((x - 350.0).abs() < 0.1);
    }

    #[test]
    fn test_multi_line_positions() {
        let lines = vec![
            WatermarkLine::new("Line 1", "#fff", 20.0),
            WatermarkLine::new("Line 2", "#fff", 20.0),
        ];
        let wm = Watermark::multi_line(lines);
        let measure = |_text: &str, _font: &str| 100.0;

        let positions = wm.calc_positions(800.0, 600.0, measure);
        assert_eq!(positions.len(), 2);

        // Second line should be below first
        assert!(positions[1].1 > positions[0].1);
    }
}
