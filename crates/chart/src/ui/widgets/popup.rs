//! Popup container widget rendering
//!
//! Platform-agnostic popup rendering using RenderContext.
//! A popup is a floating container that can hold any content.
//! Used as base for color pickers, tooltips, context menus, etc.

use crate::render::RenderContext;
use uzor::types::Rect as WidgetRect;

/// Popup configuration
#[derive(Clone, Debug)]
pub struct PopupConfig {
    /// Popup width
    pub width: f64,
    /// Popup height
    pub height: f64,
    /// Corner radius
    pub radius: f64,
    /// Padding inside popup
    pub padding: f64,
    /// Shadow offset X
    pub shadow_offset_x: f64,
    /// Shadow offset Y
    pub shadow_offset_y: f64,
    /// Shadow blur (not implemented in basic renderer, but used for color alpha)
    pub shadow_blur: f64,
}

impl Default for PopupConfig {
    fn default() -> Self {
        Self {
            width: 200.0,
            height: 150.0,
            radius: 4.0,
            padding: 8.0,
            shadow_offset_x: 2.0,
            shadow_offset_y: 4.0,
            shadow_blur: 8.0,
        }
    }
}

impl PopupConfig {
    /// Create popup with specific size
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            width,
            height,
            ..Default::default()
        }
    }

    /// Set corner radius
    pub fn with_radius(mut self, radius: f64) -> Self {
        self.radius = radius;
        self
    }

    /// Set padding
    pub fn with_padding(mut self, padding: f64) -> Self {
        self.padding = padding;
        self
    }

    /// Calculate content rect inside popup
    pub fn content_rect(&self, origin: (f64, f64)) -> WidgetRect {
        WidgetRect::new(
            origin.0 + self.padding,
            origin.1 + self.padding,
            self.width - self.padding * 2.0,
            self.height - self.padding * 2.0,
        )
    }
}

/// Popup theme colors
#[derive(Clone, Debug)]
pub struct PopupTheme {
    /// Background color
    pub background: String,
    /// Border color
    pub border: String,
    /// Shadow color (with alpha)
    pub shadow: String,
    /// Active/accent color (for sliders, etc.)
    pub active: String,
}

impl Default for PopupTheme {
    fn default() -> Self {
        Self {
            background: "#1e222d".to_string(),
            border: "#363a45".to_string(),
            shadow: "rgba(0,0,0,0.4)".to_string(),
            active: "#2962ff".to_string(),
        }
    }
}

impl PopupTheme {
    /// Create theme from hex colors
    pub fn new(background: &str, border: &str) -> Self {
        Self {
            background: background.to_string(),
            border: border.to_string(),
            shadow: "rgba(0,0,0,0.4)".to_string(),
            active: "#2962ff".to_string(),
        }
    }

    /// Set active/accent color
    pub fn with_active(mut self, active: &str) -> Self {
        self.active = active.to_string();
        self
    }
}

/// Popup rendering result
#[derive(Clone, Debug, Default)]
pub struct PopupResult {
    /// Popup bounding rect (including shadow)
    pub popup_rect: WidgetRect,
    /// Content area rect (inside padding)
    pub content_rect: WidgetRect,
}

/// Draw a popup container
///
/// This draws just the container (background, border, shadow).
/// Content should be drawn separately inside the content_rect.
///
/// # Parameters
/// - `ctx` - Render context
/// - `config` - Popup configuration
/// - `origin` - Top-left position of the popup
/// - `theme` - Popup theme colors
///
/// # Returns
/// PopupResult with popup and content rectangles
pub fn draw_popup(
    ctx: &mut dyn RenderContext,
    config: &PopupConfig,
    origin: (f64, f64),
    theme: &PopupTheme,
) -> PopupResult {
    let popup_rect = WidgetRect::new(origin.0, origin.1, config.width, config.height);
    let content_rect = config.content_rect(origin);

    // Draw shadow
    ctx.set_fill_color(&theme.shadow);
    ctx.fill_rounded_rect(
        popup_rect.x + config.shadow_offset_x,
        popup_rect.y + config.shadow_offset_y,
        popup_rect.width,
        popup_rect.height,
        config.radius,
    );

    // Blur background (FrostedGlass/LiquidGlass)
    ctx.draw_blur_background(popup_rect.x, popup_rect.y, popup_rect.width, popup_rect.height);

    // Draw background
    ctx.set_fill_color(&theme.background);
    ctx.fill_rounded_rect(
        popup_rect.x,
        popup_rect.y,
        popup_rect.width,
        popup_rect.height,
        config.radius,
    );

    // Draw border
    ctx.set_stroke_color(&theme.border);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(
        popup_rect.x,
        popup_rect.y,
        popup_rect.width,
        popup_rect.height,
        config.radius,
    );

    PopupResult {
        popup_rect,
        content_rect,
    }
}

/// Check if point is inside popup rect
pub fn popup_hit_test(popup_rect: &WidgetRect, x: f64, y: f64) -> bool {
    popup_rect.contains(x, y)
}
