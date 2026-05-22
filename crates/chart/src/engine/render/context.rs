//! Platform-agnostic rendering context trait for charts
//!
//! This module extends uzor-render's RenderContext with chart-specific
//! coordinate conversion methods for bar/price data.

use crate::{Bar, timestamp_ms_to_bar_f64};

/// Chart-specific rendering context
///
/// Extends the base RenderContext from uzor-render with coordinate conversion
/// methods needed for rendering OHLCV charts and technical indicators.
pub trait RenderContext: uzor::render::RenderContext {
    // =========================================================================
    // Dimensions (moved from uzor-render)
    // =========================================================================

    /// Get chart area width
    fn chart_width(&self) -> f64;

    /// Get chart area height
    fn chart_height(&self) -> f64;

    /// Canvas dimensions (full canvas, not just chart area)
    fn canvas_width(&self) -> f64 {
        self.chart_width()
    }

    fn canvas_height(&self) -> f64 {
        self.chart_height()
    }

    // =========================================================================
    // Coordinate Conversion (for chart primitives)
    // =========================================================================

    /// Convert bar index to X coordinate
    fn bar_to_x(&self, bar: f64) -> f64;

    /// Convert price to Y coordinate
    fn price_to_y(&self, price: f64) -> f64;

    /// Provide the bar slice used for timestamp → X conversion.
    ///
    /// Implementations that carry a bars reference override this to return it.
    /// The default returns an empty slice; callers fall back to `bar_to_x(0.0)`.
    fn bars(&self) -> &[Bar] {
        &[]
    }

    /// Convert a Unix timestamp in **milliseconds** to an X screen coordinate.
    ///
    /// Internally converts ms → fractional bar index using `timestamp_ms_to_bar_f64`,
    /// then delegates to `bar_to_x`.  When `bars()` is empty the position is
    /// approximated as `bar_to_x(0.0)`.
    fn ts_to_x_ms(&self, ts_ms: i64) -> f64 {
        let bar = timestamp_ms_to_bar_f64(self.bars(), ts_ms);
        self.bar_to_x(bar)
    }

    /// Update coordinate conversion parameters for a specific window
    /// This is called before rendering primitives for each window in multi-window layouts
    /// Parameters are: chart_width, chart_height, view_start, bar_spacing, price_min, price_max
    fn set_coordinate_space(
        &mut self,
        chart_width: f64,
        chart_height: f64,
        view_start: f64,
        bar_spacing: f64,
        price_min: f64,
        price_max: f64,
    );

    // =========================================================================
    // Line Style Helper (chart-specific)
    // =========================================================================

    /// Set line style from LineStyle enum
    fn set_line_style(&mut self, style: crate::chart::annotations::LineStyle) {
        match style {
            crate::chart::annotations::LineStyle::Solid => self.set_line_dash(&[]),
            crate::chart::annotations::LineStyle::Dashed => self.set_line_dash(&[8.0, 4.0]),
            crate::chart::annotations::LineStyle::Dotted => self.set_line_dash(&[2.0, 2.0]),
            crate::chart::annotations::LineStyle::LargeDashed => self.set_line_dash(&[12.0, 6.0]),
            crate::chart::annotations::LineStyle::SparseDotted => self.set_line_dash(&[2.0, 8.0]),
        }
    }
}

// =============================================================================
// Text Rendering Helpers
// =============================================================================

// Re-export types from uzor-render for use in this module
use uzor::render::{TextAlign, TextBaseline};

use crate::drawing::primitives_v2::{PrimitiveText, TextAlign as PrimitiveTextAlign};

/// Render text from PrimitiveText configuration
pub fn render_primitive_text(
    ctx: &mut dyn RenderContext,
    text: &PrimitiveText,
    x: f64,
    y: f64,
    fallback_color: &str,
) {
    render_primitive_text_rotated(ctx, text, x, y, fallback_color, 0.0);
}

/// Render text from PrimitiveText configuration with rotation
pub fn render_primitive_text_rotated(
    ctx: &mut dyn RenderContext,
    text: &PrimitiveText,
    x: f64,
    y: f64,
    fallback_color: &str,
    rotation: f64,
) {
    if text.content.is_empty() {
        return;
    }

    // Build font string
    let mut font_parts = Vec::new();
    if text.italic {
        font_parts.push("italic".to_string());
    }
    if text.bold {
        font_parts.push("bold".to_string());
    }
    font_parts.push(format!("{}px", text.font_size as i32));
    font_parts.push("sans-serif".to_string());
    let font = font_parts.join(" ");

    ctx.set_font(&font);

    // Set alignment
    let h_align = match text.h_align {
        PrimitiveTextAlign::Start => TextAlign::Left,
        PrimitiveTextAlign::Center => TextAlign::Center,
        PrimitiveTextAlign::End => TextAlign::Right,
    };
    ctx.set_text_align(h_align);

    // Set vertical alignment (baseline)
    let baseline = match text.v_align {
        PrimitiveTextAlign::Start => TextBaseline::Top,
        PrimitiveTextAlign::Center => TextBaseline::Middle,
        PrimitiveTextAlign::End => TextBaseline::Bottom,
    };
    ctx.set_text_baseline(baseline);

    // Set color
    let color = text.color.as_deref().unwrap_or(fallback_color);
    ctx.set_fill_color(color);

    // Render text lines with optional rotation
    let line_height = text.font_size * 1.2;
    let lines: Vec<&str> = text.content.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        let line_y = y + (i as f64 * line_height);
        ctx.fill_text_rotated(line, x, line_y, rotation);
    }
}

/// Measure text dimensions from PrimitiveText configuration
/// Returns (width, height)
pub fn measure_primitive_text(ctx: &dyn RenderContext, text: &PrimitiveText) -> (f64, f64) {
    if text.content.is_empty() {
        return (0.0, 0.0);
    }

    let lines: Vec<&str> = text.content.lines().collect();
    let line_height = text.font_size * 1.2;
    let height = lines.len() as f64 * line_height;

    let mut max_width = 0.0f64;
    for line in &lines {
        let w = ctx.measure_text(line);
        if w > max_width {
            max_width = w;
        }
    }

    (max_width, height)
}

/// Render text with optional background
pub fn render_text_with_background(
    ctx: &mut dyn RenderContext,
    text: &PrimitiveText,
    x: f64,
    y: f64,
    fallback_color: &str,
    bg_color: Option<&str>,
    padding: f64,
) {
    if text.content.is_empty() {
        return;
    }

    // Setup font first for measurement
    let mut font_parts = Vec::new();
    if text.italic {
        font_parts.push("italic".to_string());
    }
    if text.bold {
        font_parts.push("bold".to_string());
    }
    font_parts.push(format!("{}px", text.font_size as i32));
    font_parts.push("sans-serif".to_string());
    let font = font_parts.join(" ");
    ctx.set_font(&font);

    // Measure text
    let (text_width, text_height) = measure_primitive_text(ctx, text);

    // Calculate background rect position based on alignment
    let bg_x = match text.h_align {
        PrimitiveTextAlign::Start => x - padding,
        PrimitiveTextAlign::Center => x - text_width / 2.0 - padding,
        PrimitiveTextAlign::End => x - text_width - padding,
    };
    let bg_y = match text.v_align {
        PrimitiveTextAlign::Start => y - padding,
        PrimitiveTextAlign::Center => y - text_height / 2.0 - padding,
        PrimitiveTextAlign::End => y - text_height - padding,
    };

    // Draw background if specified
    if let Some(bg) = bg_color {
        ctx.set_fill_color(bg);
        ctx.fill_rect(
            bg_x,
            bg_y,
            text_width + padding * 2.0,
            text_height + padding * 2.0,
        );
    }

    // Draw text
    render_primitive_text(ctx, text, x, y, fallback_color);
}
