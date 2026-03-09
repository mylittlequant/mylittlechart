//! Universal Styling System for zengeld-chart
//!
//! Provides a unified styling interface for all chart objects including:
//! - Series (candlesticks, lines, areas, etc.)
//! - Overlays (legend, tooltip, watermark)
//! - Primitives (trend lines, rectangles, etc.)
//! - Markers and price lines
//!
//! Three-level inheritance pattern:
//! 1. Library defaults (built-in)
//! 2. Type-specific defaults (per series/overlay type)
//! 3. Instance configuration (user-provided, runtime updates)

use serde::{Deserialize, Serialize};

// =============================================================================
// Z-Order
// =============================================================================

/// Z-order for layering primitives
///
/// Determines the drawing order of primitives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub enum ZOrder {
    /// Draw behind other elements (e.g., background patterns)
    Bottom,
    /// Normal drawing order
    #[default]
    Normal,
    /// Draw on top of other elements (e.g., tooltips)
    Top,
}

// =============================================================================
// Line Style (Unified)
// =============================================================================

/// Universal line style for all chart elements
///
/// Combines patterns from price_line::LineStyle and primitives::LineStyle
/// for a unified styling system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnifiedLineStyle {
    /// Solid continuous line
    #[default]
    Solid,
    /// Dotted line: [lineWidth, lineWidth]
    Dotted,
    /// Dashed line: [2×lineWidth, 2×lineWidth]
    Dashed,
    /// Large dashed line: [6×lineWidth, 6×lineWidth]
    LargeDashed,
    /// Sparse dotted line: [lineWidth, 4×lineWidth]
    SparseDotted,
}

impl UnifiedLineStyle {
    /// Get Canvas2D dash pattern for this style
    ///
    /// Returns pattern values for ctx.setLineDash()
    pub fn dash_pattern(&self, line_width: f64) -> Vec<f64> {
        match self {
            UnifiedLineStyle::Solid => vec![],
            UnifiedLineStyle::Dotted => vec![line_width, line_width],
            UnifiedLineStyle::Dashed => vec![2.0 * line_width, 2.0 * line_width],
            UnifiedLineStyle::LargeDashed => vec![6.0 * line_width, 6.0 * line_width],
            UnifiedLineStyle::SparseDotted => vec![line_width, 4.0 * line_width],
        }
    }
}

// =============================================================================
// Font Weight
// =============================================================================

/// Font weight for text rendering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FontWeight {
    /// Thin (100)
    Thin,
    /// Light (300)
    Light,
    /// Normal/Regular (400)
    #[default]
    Normal,
    /// Medium (500)
    Medium,
    /// Semi-bold (600)
    SemiBold,
    /// Bold (700)
    Bold,
}

impl FontWeight {
    /// Get CSS font-weight value
    pub fn css_value(&self) -> &'static str {
        match self {
            FontWeight::Thin => "100",
            FontWeight::Light => "300",
            FontWeight::Normal => "400",
            FontWeight::Medium => "500",
            FontWeight::SemiBold => "600",
            FontWeight::Bold => "700",
        }
    }

    /// Get numeric weight value
    pub fn numeric(&self) -> u16 {
        match self {
            FontWeight::Thin => 100,
            FontWeight::Light => 300,
            FontWeight::Normal => 400,
            FontWeight::Medium => 500,
            FontWeight::SemiBold => 600,
            FontWeight::Bold => 700,
        }
    }
}

// =============================================================================
// Font Style
// =============================================================================

/// Font style (normal, italic, oblique)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FontStyleType {
    /// Normal/upright text
    #[default]
    Normal,
    /// Italic text
    Italic,
    /// Oblique text
    Oblique,
}

impl FontStyleType {
    /// Get CSS font-style value
    pub fn css_value(&self) -> &'static str {
        match self {
            FontStyleType::Normal => "normal",
            FontStyleType::Italic => "italic",
            FontStyleType::Oblique => "oblique",
        }
    }
}

// =============================================================================
// StyleSet - Universal Style Container
// =============================================================================

/// Universal style set applicable to any chart object
///
/// Uses Option<T> for all fields to support partial updates
/// (DeepPartial pattern for applyOptions()).
///
/// # Example
///
/// ```
/// use zengeld_chart::StyleSet;
///
/// // Create a partial style update
/// let update = StyleSet {
///     fill_color: Some("#ff0000".to_string()),
///     stroke_width: Some(2.0),
///     ..Default::default()
/// };
///
/// // Merge with existing style
/// let mut existing = StyleSet::default();
/// existing.merge(&update);
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StyleSet {
    // === Colors ===
    /// Fill/background color (CSS format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<String>,

    /// Stroke/border color (CSS format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_color: Option<String>,

    /// Text color (CSS format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_color: Option<String>,

    /// Secondary fill color (for gradients)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill_color_secondary: Option<String>,

    // === Stroke/Line ===
    /// Stroke/line width in pixels
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,

    /// Line style (solid, dashed, dotted)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_style: Option<UnifiedLineStyle>,

    // === Font ===
    /// Font family (CSS format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,

    /// Font size in pixels
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,

    /// Font weight
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_weight: Option<FontWeight>,

    /// Font style (normal, italic)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_style: Option<FontStyleType>,

    // === Visibility & Layering ===
    /// Visibility flag
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,

    /// Z-order for layering
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z_order: Option<ZOrder>,

    // === Opacity ===
    /// Overall opacity (0.0 - 1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,

    // === Spacing ===
    /// Padding in pixels
    #[serde(skip_serializing_if = "Option::is_none")]
    pub padding: Option<f64>,

    /// Border radius for rounded corners
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_radius: Option<f64>,
}

impl StyleSet {
    /// Create empty style set
    pub fn new() -> Self {
        Self::default()
    }

    /// Create style with fill color
    pub fn with_fill(color: impl Into<String>) -> Self {
        Self {
            fill_color: Some(color.into()),
            ..Default::default()
        }
    }

    /// Create style with stroke
    pub fn with_stroke(color: impl Into<String>, width: f64) -> Self {
        Self {
            stroke_color: Some(color.into()),
            stroke_width: Some(width),
            ..Default::default()
        }
    }

    /// Create style with text properties
    pub fn with_text(color: impl Into<String>, size: f64) -> Self {
        Self {
            text_color: Some(color.into()),
            font_size: Some(size),
            ..Default::default()
        }
    }

    /// Merge another StyleSet into this one
    ///
    /// Other takes precedence for non-None values.
    pub fn merge(&mut self, other: &StyleSet) {
        if other.fill_color.is_some() {
            self.fill_color = other.fill_color.clone();
        }
        if other.stroke_color.is_some() {
            self.stroke_color = other.stroke_color.clone();
        }
        if other.text_color.is_some() {
            self.text_color = other.text_color.clone();
        }
        if other.fill_color_secondary.is_some() {
            self.fill_color_secondary = other.fill_color_secondary.clone();
        }
        if other.stroke_width.is_some() {
            self.stroke_width = other.stroke_width;
        }
        if other.line_style.is_some() {
            self.line_style = other.line_style;
        }
        if other.font_family.is_some() {
            self.font_family = other.font_family.clone();
        }
        if other.font_size.is_some() {
            self.font_size = other.font_size;
        }
        if other.font_weight.is_some() {
            self.font_weight = other.font_weight;
        }
        if other.font_style.is_some() {
            self.font_style = other.font_style;
        }
        if other.visible.is_some() {
            self.visible = other.visible;
        }
        if other.z_order.is_some() {
            self.z_order = other.z_order;
        }
        if other.opacity.is_some() {
            self.opacity = other.opacity;
        }
        if other.padding.is_some() {
            self.padding = other.padding;
        }
        if other.border_radius.is_some() {
            self.border_radius = other.border_radius;
        }
    }

    /// Create a new StyleSet with defaults applied for unset values
    pub fn with_defaults(&self, defaults: &StyleSet) -> StyleSet {
        let mut result = defaults.clone();
        result.merge(self);
        result
    }

    /// Check if all style fields are None (empty style)
    pub fn is_empty(&self) -> bool {
        self.fill_color.is_none()
            && self.stroke_color.is_none()
            && self.text_color.is_none()
            && self.fill_color_secondary.is_none()
            && self.stroke_width.is_none()
            && self.line_style.is_none()
            && self.font_family.is_none()
            && self.font_size.is_none()
            && self.font_weight.is_none()
            && self.font_style.is_none()
            && self.visible.is_none()
            && self.z_order.is_none()
            && self.opacity.is_none()
            && self.padding.is_none()
            && self.border_radius.is_none()
    }

    /// Get effective fill color with fallback
    pub fn effective_fill<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.fill_color.as_deref().unwrap_or(fallback)
    }

    /// Get effective stroke color with fallback
    pub fn effective_stroke<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.stroke_color.as_deref().unwrap_or(fallback)
    }

    /// Get effective text color with fallback
    pub fn effective_text<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.text_color.as_deref().unwrap_or(fallback)
    }

    /// Get effective stroke width with fallback
    pub fn effective_stroke_width(&self, fallback: f64) -> f64 {
        self.stroke_width.unwrap_or(fallback)
    }

    /// Get effective font size with fallback
    pub fn effective_font_size(&self, fallback: f64) -> f64 {
        self.font_size.unwrap_or(fallback)
    }

    /// Get effective visibility with fallback
    pub fn effective_visible(&self, fallback: bool) -> bool {
        self.visible.unwrap_or(fallback)
    }

    /// Build CSS font string
    pub fn build_font_string(&self) -> Option<String> {
        let size = self.font_size?;
        let family = self.font_family.as_deref().unwrap_or("sans-serif");
        let weight = self.font_weight.map(|w| w.css_value()).unwrap_or("400");
        let style = self.font_style.map(|s| s.css_value()).unwrap_or("normal");

        Some(format!("{} {} {}px {}", style, weight, size, family))
    }
}

// =============================================================================
// Styleable Trait
// =============================================================================

/// Trait for objects that can be styled
///
/// Provides a uniform interface for getting and setting styles
/// on any chart object (series, overlays, primitives, markers).
pub trait Styleable {
    /// Get current style
    fn style(&self) -> StyleSet;

    /// Set complete style (replaces existing)
    fn set_style(&mut self, style: StyleSet);

    /// Apply partial style update (merge with existing)
    fn apply_style(&mut self, partial: StyleSet) {
        let mut current = self.style();
        current.merge(&partial);
        self.set_style(current);
    }

    /// Reset to default style
    fn reset_style(&mut self) {
        self.set_style(StyleSet::default());
    }
}

// =============================================================================
// Default Styles for Common Types
// =============================================================================

/// Default styles for various chart element types
pub struct DefaultStyles;

impl DefaultStyles {
    /// Default style for candlestick series (bullish)
    pub fn candlestick_up() -> StyleSet {
        StyleSet {
            fill_color: Some("#26a69a".to_string()),
            stroke_color: Some("#26a69a".to_string()),
            ..Default::default()
        }
    }

    /// Default style for candlestick series (bearish)
    pub fn candlestick_down() -> StyleSet {
        StyleSet {
            fill_color: Some("#ef5350".to_string()),
            stroke_color: Some("#ef5350".to_string()),
            ..Default::default()
        }
    }

    /// Default style for line series
    pub fn line_series() -> StyleSet {
        StyleSet {
            stroke_color: Some("#2196f3".to_string()),
            stroke_width: Some(2.0),
            line_style: Some(UnifiedLineStyle::Solid),
            ..Default::default()
        }
    }

    /// Default style for area series
    pub fn area_series() -> StyleSet {
        StyleSet {
            fill_color: Some("rgba(46, 220, 135, 0.4)".to_string()),
            fill_color_secondary: Some("rgba(40, 221, 100, 0)".to_string()),
            stroke_color: Some("#33D778".to_string()),
            stroke_width: Some(2.0),
            ..Default::default()
        }
    }

    /// Default style for histogram series
    pub fn histogram_series() -> StyleSet {
        StyleSet {
            fill_color: Some("#26a69a".to_string()),
            ..Default::default()
        }
    }

    /// Default style for price lines
    pub fn price_line() -> StyleSet {
        StyleSet {
            stroke_color: Some("#2962ff".to_string()),
            stroke_width: Some(1.0),
            line_style: Some(UnifiedLineStyle::Solid),
            text_color: Some("#ffffff".to_string()),
            visible: Some(true),
            ..Default::default()
        }
    }

    /// Default style for markers
    pub fn marker() -> StyleSet {
        StyleSet {
            fill_color: Some("#4caf50".to_string()),
            text_color: Some("#b2b5be".to_string()),
            font_size: Some(11.0),
            ..Default::default()
        }
    }

    /// Default style for trend lines
    pub fn trend_line() -> StyleSet {
        StyleSet {
            stroke_color: Some("#2962ff".to_string()),
            stroke_width: Some(1.0),
            line_style: Some(UnifiedLineStyle::Solid),
            ..Default::default()
        }
    }

    /// Default style for horizontal lines
    pub fn horizontal_line() -> StyleSet {
        StyleSet {
            stroke_color: Some("#758696".to_string()),
            stroke_width: Some(1.0),
            line_style: Some(UnifiedLineStyle::Dashed),
            ..Default::default()
        }
    }

    /// Default style for vertical lines
    pub fn vertical_line() -> StyleSet {
        StyleSet {
            stroke_color: Some("#758696".to_string()),
            stroke_width: Some(1.0),
            line_style: Some(UnifiedLineStyle::Dashed),
            ..Default::default()
        }
    }

    /// Default style for rectangles
    pub fn rectangle() -> StyleSet {
        StyleSet {
            fill_color: Some("rgba(33, 150, 243, 0.2)".to_string()),
            stroke_color: Some("#2196f3".to_string()),
            stroke_width: Some(1.0),
            ..Default::default()
        }
    }

    /// Default style for legend overlay
    pub fn legend() -> StyleSet {
        StyleSet {
            fill_color: Some("rgba(0, 0, 0, 0.5)".to_string()),
            text_color: Some("#b2b5be".to_string()),
            font_size: Some(12.0),
            font_family: Some("-apple-system, BlinkMacSystemFont, 'Trebuchet MS', Roboto, Ubuntu, sans-serif".to_string()),
            padding: Some(8.0),
            visible: Some(true),
            ..Default::default()
        }
    }

    /// Default style for tooltip overlay
    pub fn tooltip() -> StyleSet {
        StyleSet {
            fill_color: Some("#1e222d".to_string()),
            stroke_color: Some("#2a2e39".to_string()),
            text_color: Some("#b2b5be".to_string()),
            font_size: Some(12.0),
            font_family: Some("-apple-system, BlinkMacSystemFont, 'Trebuchet MS', Roboto, Ubuntu, sans-serif".to_string()),
            padding: Some(8.0),
            border_radius: Some(4.0),
            visible: Some(false),
            ..Default::default()
        }
    }

    /// Default style for watermark overlay
    pub fn watermark() -> StyleSet {
        StyleSet {
            text_color: Some("rgba(255, 255, 255, 0.1)".to_string()),
            font_size: Some(48.0),
            font_family: Some("-apple-system, BlinkMacSystemFont, 'Trebuchet MS', Roboto, Ubuntu, sans-serif".to_string()),
            font_weight: Some(FontWeight::Bold),
            visible: Some(true),
            ..Default::default()
        }
    }

    /// Default style for crosshair
    pub fn crosshair() -> StyleSet {
        StyleSet {
            stroke_color: Some("#758696".to_string()),
            stroke_width: Some(1.0),
            line_style: Some(UnifiedLineStyle::Dashed),
            ..Default::default()
        }
    }

    /// Default style for grid
    pub fn grid() -> StyleSet {
        StyleSet {
            stroke_color: Some("rgba(42, 46, 57, 0.6)".to_string()),
            stroke_width: Some(1.0),
            line_style: Some(UnifiedLineStyle::Solid),
            visible: Some(true),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_style_set_merge() {
        let mut base = StyleSet {
            fill_color: Some("#ff0000".to_string()),
            stroke_width: Some(1.0),
            ..Default::default()
        };

        let update = StyleSet {
            fill_color: Some("#00ff00".to_string()),
            font_size: Some(14.0),
            ..Default::default()
        };

        base.merge(&update);

        assert_eq!(base.fill_color, Some("#00ff00".to_string()));
        assert_eq!(base.stroke_width, Some(1.0)); // Preserved
        assert_eq!(base.font_size, Some(14.0)); // Added
    }

    #[test]
    fn test_style_set_with_defaults() {
        let partial = StyleSet {
            fill_color: Some("#ff0000".to_string()),
            ..Default::default()
        };

        let defaults = StyleSet {
            fill_color: Some("#000000".to_string()),
            stroke_width: Some(2.0),
            ..Default::default()
        };

        let result = partial.with_defaults(&defaults);

        assert_eq!(result.fill_color, Some("#ff0000".to_string())); // Partial wins
        assert_eq!(result.stroke_width, Some(2.0)); // Default applied
    }

    #[test]
    fn test_effective_values() {
        let style = StyleSet {
            fill_color: Some("#ff0000".to_string()),
            ..Default::default()
        };

        assert_eq!(style.effective_fill("#000000"), "#ff0000");
        assert_eq!(style.effective_stroke("#000000"), "#000000"); // Fallback used
        assert_eq!(style.effective_stroke_width(1.0), 1.0); // Fallback used
    }

    #[test]
    fn test_font_string_builder() {
        let style = StyleSet {
            font_size: Some(14.0),
            font_family: Some("Arial".to_string()),
            font_weight: Some(FontWeight::Bold),
            font_style: Some(FontStyleType::Italic),
            ..Default::default()
        };

        let font_string = style.build_font_string().unwrap();
        assert_eq!(font_string, "italic 700 14px Arial");
    }

    #[test]
    fn test_unified_line_style_patterns() {
        assert!(UnifiedLineStyle::Solid.dash_pattern(2.0).is_empty());
        assert_eq!(UnifiedLineStyle::Dotted.dash_pattern(2.0), vec![2.0, 2.0]);
        assert_eq!(UnifiedLineStyle::Dashed.dash_pattern(2.0), vec![4.0, 4.0]);
    }

    #[test]
    fn test_default_styles() {
        let line_style = DefaultStyles::line_series();
        assert_eq!(line_style.stroke_color, Some("#2196f3".to_string()));
        assert_eq!(line_style.stroke_width, Some(2.0));

        let tooltip_style = DefaultStyles::tooltip();
        assert_eq!(tooltip_style.visible, Some(false));
    }
}
