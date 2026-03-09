//! Modal dialog widget rendering
//!
//! Platform-agnostic modal dialog rendering using RenderContext.

use crate::engine::render::{RenderContext, TextAlign, TextBaseline};
use crate::ui::widgets::types::WidgetTheme;
use uzor::types::Rect as WidgetRect;
use uzor::types::IconId;

/// Modal size preset
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModalSize {
    Small,
    #[default]
    Medium,
    Large,
    FullScreen,
    Custom { width: u32, height: u32 },
}

impl ModalSize {
    pub fn dimensions(&self) -> (f64, f64) {
        match self {
            Self::Small => (400.0, 300.0),
            Self::Medium => (600.0, 450.0),
            Self::Large => (800.0, 600.0),
            Self::FullScreen => (0.0, 0.0), // Will use screen size
            Self::Custom { width, height } => (*width as f64, *height as f64),
        }
    }
}

/// Modal configuration
#[derive(Clone, Debug)]
pub struct ModalConfig {
    /// Modal title
    pub title: String,
    /// Modal size
    pub size: ModalSize,
    /// Show close button
    pub show_close: bool,
    /// Show overlay backdrop
    pub show_backdrop: bool,
    /// Backdrop opacity (0.0 - 1.0)
    pub backdrop_opacity: f64,
    /// Close on backdrop click
    pub close_on_backdrop: bool,
    /// Corner radius
    pub radius: f64,
    /// Header height
    pub header_height: f64,
    /// Footer height (0 if no footer)
    pub footer_height: f64,
    /// Padding
    pub padding: f64,
}

impl Default for ModalConfig {
    fn default() -> Self {
        Self {
            title: String::new(),
            size: ModalSize::Medium,
            show_close: true,
            show_backdrop: true,
            backdrop_opacity: 0.5,
            close_on_backdrop: true,
            radius: 8.0,
            header_height: 48.0,
            footer_height: 0.0,
            padding: 16.0,
        }
    }
}

impl ModalConfig {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            ..Default::default()
        }
    }

    /// Create a confirmation dialog preset (small, simple)
    pub fn confirmation(title: &str) -> Self {
        Self {
            title: title.to_string(),
            size: ModalSize::Small,
            radius: 0.0, // No rounded corners for minimal style
            header_height: 36.0,
            padding: 12.0,
            ..Default::default()
        }
    }

    /// Create a settings modal preset (medium-large, with tabs)
    pub fn settings(title: &str) -> Self {
        Self {
            title: title.to_string(),
            size: ModalSize::Custom { width: 620, height: 580 },
            radius: 0.0, // No rounded corners for minimal style
            header_height: 44.0,
            padding: 16.0,
            footer_height: 52.0,
            ..Default::default()
        }
    }

    /// Create a search overlay preset (centered, medium size)
    pub fn search(title: &str) -> Self {
        Self {
            title: title.to_string(),
            size: ModalSize::Medium,
            radius: 0.0, // No rounded corners for minimal style
            header_height: 36.0,
            padding: 12.0,
            show_backdrop: false, // Search overlays don't block everything
            ..Default::default()
        }
    }

    pub fn with_size(mut self, size: ModalSize) -> Self {
        self.size = size;
        self
    }

    pub fn with_footer(mut self, height: f64) -> Self {
        self.footer_height = height;
        self
    }

    pub fn without_close(mut self) -> Self {
        self.show_close = false;
        self
    }

    /// Set custom content height (adjusts modal height dynamically)
    pub fn with_content_height(mut self, content_height: f64) -> Self {
        let total_height = self.header_height + content_height + self.footer_height + self.padding * 2.0;
        self.size = match self.size {
            ModalSize::Custom { width, .. } => ModalSize::Custom { width, height: total_height as u32 },
            _ => {
                let (width, _) = self.size.dimensions();
                ModalSize::Custom { width: width as u32, height: total_height as u32 }
            }
        };
        self
    }
}

/// Modal theme
#[derive(Clone, Debug)]
pub struct ModalTheme {
    pub backdrop: String,
    pub background: String,
    pub border: String,
    pub shadow: String,
    pub header_bg: String,
    pub header_text: String,
    pub header_border: String,
    pub footer_bg: String,
    pub footer_border: String,
    pub close_button: String,
    pub close_button_hover: String,
}

impl Default for ModalTheme {
    fn default() -> Self {
        Self {
            backdrop: "rgba(0,0,0,0.5)".to_string(),
            background: "#1e222d".to_string(),
            border: "#363a45".to_string(),
            shadow: "rgba(0,0,0,0.5)".to_string(),
            header_bg: "#1e222d".to_string(),
            header_text: "#ffffff".to_string(),
            header_border: "#363a45".to_string(),
            footer_bg: "#1e222d".to_string(),
            footer_border: "#363a45".to_string(),
            close_button: "#9598a1".to_string(),
            close_button_hover: "#ffffff".to_string(),
        }
    }
}

impl ModalTheme {
    /// Create ModalTheme from FrameTheme and ToolbarTheme
    ///
    /// This is a convenience function for migrating existing modals that use
    /// FrameTheme/ToolbarTheme to the new unified modal system.
    ///
    /// # Note
    /// FrameTheme is from zengeld_chart::layout::FrameTheme
    /// ToolbarTheme is from crate::ui::render::ToolbarTheme
    pub fn from_frame_theme(
        toolbar_bg: &str,
        toolbar_border: &str,
        item_text: &str,
        item_text_hover: &str,
        separator: &str,
    ) -> Self {
        Self {
            backdrop: "rgba(0,0,0,0.6)".to_string(),
            background: toolbar_bg.to_string(),
            border: toolbar_border.to_string(),
            shadow: "rgba(0,0,0,0.4)".to_string(),
            header_bg: toolbar_bg.to_string(),
            header_text: item_text.to_string(),
            header_border: separator.to_string(),
            footer_bg: toolbar_bg.to_string(),
            footer_border: separator.to_string(),
            close_button: item_text.to_string(),
            close_button_hover: item_text_hover.to_string(),
        }
    }
}

/// Modal rendering result
#[derive(Clone, Debug, Default)]
pub struct ModalResult {
    /// Whether close button was clicked
    pub close_clicked: bool,
    /// Whether backdrop was clicked
    pub backdrop_clicked: bool,
    /// Modal frame rectangle
    pub frame_rect: WidgetRect,
    /// Content area rectangle
    pub content_rect: WidgetRect,
    /// Header rectangle
    pub header_rect: WidgetRect,
    /// Footer rectangle (if any)
    pub footer_rect: Option<WidgetRect>,
    /// Close button rectangle
    pub close_rect: Option<WidgetRect>,
}

/// Draw a modal backdrop overlay
///
/// Should be called before draw_modal_frame
pub fn draw_modal_backdrop(
    ctx: &mut dyn RenderContext,
    screen_rect: WidgetRect,
    theme: &ModalTheme,
    opacity: f64,
) {
    // Parse backdrop color and apply opacity
    let backdrop_color = if opacity < 1.0 {
        format!("rgba(0,0,0,{})", opacity)
    } else {
        theme.backdrop.clone()
    };

    ctx.set_fill_color(&backdrop_color);
    ctx.fill_rect(screen_rect.x, screen_rect.y, screen_rect.width, screen_rect.height);
}

/// Draw a modal dialog frame
///
/// # Parameters
/// - `ctx` - Render context
/// - `config` - Modal configuration
/// - `screen_rect` - Full screen rectangle (for centering)
/// - `theme` - Modal theme
/// - `close_hovered` - Whether close button is hovered
/// - `draw_close_icon` - Callback to draw close icon
///
/// # Returns
/// Modal result with content rectangle for placing inner content
pub fn draw_modal_frame<F>(
    ctx: &mut dyn RenderContext,
    config: &ModalConfig,
    screen_rect: WidgetRect,
    theme: &ModalTheme,
    close_hovered: bool,
    draw_close_icon: F,
) -> ModalResult
where
    F: FnOnce(&mut dyn RenderContext, WidgetRect, &str),
{
    let mut result = ModalResult::default();

    // Calculate modal dimensions
    let (modal_width, modal_height) = if matches!(config.size, ModalSize::FullScreen) {
        (screen_rect.width * 0.9, screen_rect.height * 0.9)
    } else {
        config.size.dimensions()
    };

    // Center modal
    let modal_x = screen_rect.x + (screen_rect.width - modal_width) / 2.0;
    let modal_y = screen_rect.y + (screen_rect.height - modal_height) / 2.0;

    let frame_rect = WidgetRect::new(modal_x, modal_y, modal_width, modal_height);
    result.frame_rect = frame_rect;

    // Draw shadow
    ctx.set_fill_color("rgba(0,0,0,0.3)");
    ctx.fill_rounded_rect(
        frame_rect.x + 4.0,
        frame_rect.y + 8.0,
        frame_rect.width,
        frame_rect.height,
        config.radius,
    );

    // Draw background
    ctx.set_fill_color(&theme.background);
    ctx.fill_rounded_rect(frame_rect.x, frame_rect.y, frame_rect.width, frame_rect.height, config.radius);

    // Draw border
    ctx.set_stroke_color(&theme.border);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(frame_rect.x, frame_rect.y, frame_rect.width, frame_rect.height, config.radius);

    // Draw header
    let header_rect = WidgetRect::new(
        frame_rect.x,
        frame_rect.y,
        frame_rect.width,
        config.header_height,
    );
    result.header_rect = header_rect;

    // Header background (with rounded top corners)
    ctx.set_fill_color(&theme.header_bg);
    // Simplified: draw full rounded rect then cover bottom with regular rect
    ctx.fill_rounded_rect(header_rect.x, header_rect.y, header_rect.width, header_rect.height, config.radius);
    ctx.fill_rect(header_rect.x, header_rect.y + config.radius, header_rect.width, header_rect.height - config.radius);

    // Header bottom border
    ctx.set_stroke_color(&theme.header_border);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(header_rect.x, header_rect.bottom());
    ctx.line_to(header_rect.right(), header_rect.bottom());
    ctx.stroke();

    // Draw title
    ctx.set_font("bold 16px sans-serif");
    ctx.set_fill_color(&theme.header_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&config.title, header_rect.x + config.padding, header_rect.center_y());

    // Draw close button
    if config.show_close {
        let close_size = 24.0;
        let close_rect = WidgetRect::new(
            header_rect.right() - config.padding - close_size,
            header_rect.center_y() - close_size / 2.0,
            close_size,
            close_size,
        );
        result.close_rect = Some(close_rect);

        let close_color = if close_hovered {
            &theme.close_button_hover
        } else {
            &theme.close_button
        };

        // Draw close button background on hover
        if close_hovered {
            ctx.set_fill_color("rgba(255,255,255,0.1)");
            ctx.fill_rounded_rect(close_rect.x, close_rect.y, close_rect.width, close_rect.height, 4.0);
        }

        draw_close_icon(ctx, close_rect, close_color);
    }

    // Draw footer if present
    if config.footer_height > 0.0 {
        let footer_rect = WidgetRect::new(
            frame_rect.x,
            frame_rect.bottom() - config.footer_height,
            frame_rect.width,
            config.footer_height,
        );
        result.footer_rect = Some(footer_rect);

        // Footer top border
        ctx.set_stroke_color(&theme.footer_border);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(footer_rect.x, footer_rect.y);
        ctx.line_to(footer_rect.right(), footer_rect.y);
        ctx.stroke();
    }

    // Calculate content rect
    let content_y = header_rect.bottom();
    let content_height = if config.footer_height > 0.0 {
        frame_rect.height - config.header_height - config.footer_height
    } else {
        frame_rect.height - config.header_height
    };

    result.content_rect = WidgetRect::new(
        frame_rect.x + config.padding,
        content_y + config.padding,
        frame_rect.width - config.padding * 2.0,
        content_height - config.padding * 2.0,
    );

    result
}

/// Draw a simple close icon (X)
pub fn draw_close_icon(
    ctx: &mut dyn RenderContext,
    rect: WidgetRect,
    color: &str,
) {
    let padding = 6.0;
    let x1 = rect.x + padding;
    let y1 = rect.y + padding;
    let x2 = rect.right() - padding;
    let y2 = rect.bottom() - padding;

    ctx.set_stroke_color(color);
    ctx.set_stroke_width(2.0);
    ctx.set_line_dash(&[]);

    ctx.begin_path();
    ctx.move_to(x1, y1);
    ctx.line_to(x2, y2);
    ctx.stroke();

    ctx.begin_path();
    ctx.move_to(x2, y1);
    ctx.line_to(x1, y2);
    ctx.stroke();
}

/// Tab configuration for tabbed modals
#[derive(Clone, Debug)]
pub struct ModalTab {
    pub id: String,
    pub label: String,
    pub icon: Option<IconId>,
}

/// Draw modal tabs
///
/// # Parameters
/// - `ctx` - Render context
/// - `tabs` - Tab configurations
/// - `rect` - Tab bar rectangle
/// - `active_id` - Currently active tab ID
/// - `hovered_id` - Currently hovered tab ID
/// - `theme` - Widget theme
///
/// # Returns
/// Vector of (tab_id, tab_rect) for hit testing
pub fn draw_modal_tabs(
    ctx: &mut dyn RenderContext,
    tabs: &[ModalTab],
    rect: WidgetRect,
    active_id: &str,
    hovered_id: Option<&str>,
    theme: &WidgetTheme,
) -> Vec<(String, WidgetRect)> {
    let mut result = Vec::new();

    if tabs.is_empty() {
        return result;
    }

    let tab_width = rect.width / tabs.len() as f64;

    for (i, tab) in tabs.iter().enumerate() {
        let tab_rect = WidgetRect::new(
            rect.x + i as f64 * tab_width,
            rect.y,
            tab_width,
            rect.height,
        );

        let is_active = tab.id == active_id;
        let is_hovered = hovered_id == Some(tab.id.as_str());

        // Draw tab background
        if is_active {
            ctx.set_fill_color(&theme.accent);
        } else if is_hovered {
            ctx.set_fill_color(&theme.bg_hover);
        }

        if is_active || is_hovered {
            ctx.fill_rect(tab_rect.x, tab_rect.y, tab_rect.width, tab_rect.height);
        }

        // Draw tab label
        let text_color = if is_active {
            &theme.text_hover
        } else {
            &theme.text_normal
        };

        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(text_color);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&tab.label, tab_rect.center_x(), tab_rect.center_y());

        // Draw active indicator
        if is_active {
            ctx.set_fill_color(&theme.accent);
            ctx.fill_rect(tab_rect.x, tab_rect.bottom() - 2.0, tab_rect.width, 2.0);
        }

        result.push((tab.id.clone(), tab_rect));
    }

    result
}

/// Render modal frame (shadow, blur, background, border) without header/footer
///
/// This is a low-level helper for complex modals that need custom layouts.
/// Use `render_modal()` for standard modals with header/footer.
///
/// # Parameters
/// - `ctx` - Render context
/// - `frame_rect` - Modal frame rectangle (position and size)
/// - `theme` - Modal theme
/// - `with_radius` - Whether to use rounded corners (0.0 for minimal style)
pub fn render_modal_frame_only(
    ctx: &mut dyn RenderContext,
    frame_rect: WidgetRect,
    theme: &ModalTheme,
    with_radius: f64,
) {
    let shadow_offset = if with_radius > 0.0 { 4.0 } else { 3.0 };

    // Shadow (subtle)
    ctx.set_fill_color("rgba(0, 0, 0, 0.4)");
    if with_radius > 0.0 {
        ctx.fill_rounded_rect(
            frame_rect.x + shadow_offset,
            frame_rect.y + shadow_offset,
            frame_rect.width,
            frame_rect.height,
            with_radius,
        );
    } else {
        ctx.fill_rect(
            frame_rect.x + shadow_offset,
            frame_rect.y + shadow_offset,
            frame_rect.width,
            frame_rect.height,
        );
    }

    // Blur background (FrostedGlass/LiquidGlass effect)
    ctx.draw_blur_background(frame_rect.x, frame_rect.y, frame_rect.width, frame_rect.height);

    // Background
    ctx.set_fill_color(&theme.background);
    if with_radius > 0.0 {
        ctx.fill_rounded_rect(frame_rect.x, frame_rect.y, frame_rect.width, frame_rect.height, with_radius);
    } else {
        ctx.fill_rect(frame_rect.x, frame_rect.y, frame_rect.width, frame_rect.height);
    }

    // Border
    ctx.set_stroke_color(&theme.border);
    ctx.set_stroke_width(1.0);
    if with_radius > 0.0 {
        ctx.stroke_rounded_rect(frame_rect.x, frame_rect.y, frame_rect.width, frame_rect.height, with_radius);
    } else {
        ctx.stroke_rect(frame_rect.x, frame_rect.y, frame_rect.width, frame_rect.height);
    }
}

/// Unified modal rendering function with content callback
///
/// Renders a complete modal dialog with:
/// - Optional backdrop overlay
/// - Shadow and blur background (FrostedGlass/LiquidGlass effect)
/// - Modal frame with border
/// - Header with title and close button
/// - Content area (rendered by callback)
/// - Optional footer
///
/// # Parameters
/// - `ctx` - Render context
/// - `config` - Modal configuration
/// - `screen_rect` - Full screen rectangle (for centering)
/// - `theme` - Modal theme
/// - `close_hovered` - Whether close button is hovered
/// - `content_fn` - Callback to render content area, receives content_rect
///
/// # Returns
/// Modal result with rectangles for hit testing
///
/// # Example
/// ```ignore
/// let config = ModalConfig::confirmation("Confirm Action")
///     .with_content_height(150.0);
/// let result = render_modal(ctx, &config, screen_rect, theme, false, |content_rect| {
///     // Render your content here
///     ctx.set_fill_color("#ffffff");
///     ctx.fill_text("Are you sure?", content_rect.center_x(), content_rect.center_y());
/// });
/// ```
pub fn render_modal<F>(
    ctx: &mut dyn RenderContext,
    config: &ModalConfig,
    screen_rect: WidgetRect,
    theme: &ModalTheme,
    close_hovered: bool,
    content_fn: F,
) -> ModalResult
where
    F: FnOnce(&mut dyn RenderContext, WidgetRect),
{
    let mut result = ModalResult::default();

    // Draw backdrop if configured
    if config.show_backdrop {
        draw_modal_backdrop(ctx, screen_rect, theme, config.backdrop_opacity);
    }

    // Calculate modal dimensions
    let (modal_width, modal_height) = if matches!(config.size, ModalSize::FullScreen) {
        (screen_rect.width * 0.9, screen_rect.height * 0.9)
    } else {
        config.size.dimensions()
    };

    // Center modal
    let modal_x = screen_rect.x + (screen_rect.width - modal_width) / 2.0;
    let modal_y = screen_rect.y + (screen_rect.height - modal_height) / 2.0;

    let frame_rect = WidgetRect::new(modal_x, modal_y, modal_width, modal_height);
    result.frame_rect = frame_rect;

    // Shadow (subtle, matching render_ui.rs pattern)
    let shadow_offset = if config.radius > 0.0 { 4.0 } else { 3.0 };
    ctx.set_fill_color("rgba(0, 0, 0, 0.4)");
    if config.radius > 0.0 {
        ctx.fill_rounded_rect(
            frame_rect.x + shadow_offset,
            frame_rect.y + shadow_offset,
            frame_rect.width,
            frame_rect.height,
            config.radius,
        );
    } else {
        ctx.fill_rect(
            frame_rect.x + shadow_offset,
            frame_rect.y + shadow_offset,
            frame_rect.width,
            frame_rect.height,
        );
    }

    // Blur background (FrostedGlass/LiquidGlass effect)
    ctx.draw_blur_background(frame_rect.x, frame_rect.y, frame_rect.width, frame_rect.height);

    // Background
    ctx.set_fill_color(&theme.background);
    if config.radius > 0.0 {
        ctx.fill_rounded_rect(frame_rect.x, frame_rect.y, frame_rect.width, frame_rect.height, config.radius);
    } else {
        ctx.fill_rect(frame_rect.x, frame_rect.y, frame_rect.width, frame_rect.height);
    }

    // Border
    ctx.set_stroke_color(&theme.border);
    ctx.set_stroke_width(1.0);
    if config.radius > 0.0 {
        ctx.stroke_rounded_rect(frame_rect.x, frame_rect.y, frame_rect.width, frame_rect.height, config.radius);
    } else {
        ctx.stroke_rect(frame_rect.x, frame_rect.y, frame_rect.width, frame_rect.height);
    }

    // Header rectangle
    let header_rect = WidgetRect::new(
        frame_rect.x,
        frame_rect.y,
        frame_rect.width,
        config.header_height,
    );
    result.header_rect = header_rect;

    // Title
    ctx.set_font("14px sans-serif");
    ctx.set_fill_color(&theme.header_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&config.title, header_rect.x + config.padding, header_rect.center_y());

    // Close button
    if config.show_close {
        let close_size = 20.0;
        let close_rect = WidgetRect::new(
            header_rect.right() - close_size - config.padding,
            header_rect.center_y() - close_size / 2.0,
            close_size,
            close_size,
        );
        result.close_rect = Some(close_rect);

        let close_color = if close_hovered {
            &theme.close_button_hover
        } else {
            &theme.close_button
        };

        draw_close_icon(ctx, close_rect, close_color);
    }

    // Header bottom border
    ctx.set_stroke_color(&theme.header_border);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(header_rect.x, header_rect.bottom());
    ctx.line_to(header_rect.right(), header_rect.bottom());
    ctx.stroke();

    // Footer (if present)
    if config.footer_height > 0.0 {
        let footer_rect = WidgetRect::new(
            frame_rect.x,
            frame_rect.bottom() - config.footer_height,
            frame_rect.width,
            config.footer_height,
        );
        result.footer_rect = Some(footer_rect);

        // Footer top border
        ctx.set_stroke_color(&theme.footer_border);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(footer_rect.x, footer_rect.y);
        ctx.line_to(footer_rect.right(), footer_rect.y);
        ctx.stroke();
    }

    // Calculate content rect
    let content_y = header_rect.bottom();
    let content_height = frame_rect.height - config.header_height - config.footer_height;

    let content_rect = WidgetRect::new(
        frame_rect.x + config.padding,
        content_y + config.padding,
        frame_rect.width - config.padding * 2.0,
        content_height - config.padding * 2.0,
    );
    result.content_rect = content_rect;

    // Render content via callback
    content_fn(ctx, content_rect);

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modal_size() {
        let (w, h) = ModalSize::Medium.dimensions();
        assert_eq!(w, 600.0);
        assert_eq!(h, 450.0);

        let (w, h) = ModalSize::Custom { width: 800, height: 600 }.dimensions();
        assert_eq!(w, 800.0);
        assert_eq!(h, 600.0);
    }

    #[test]
    fn test_modal_config() {
        let config = ModalConfig::new("Test Modal")
            .with_size(ModalSize::Large)
            .with_footer(56.0);

        assert_eq!(config.title, "Test Modal");
        assert_eq!(config.footer_height, 56.0);
    }

    #[test]
    fn test_modal_presets() {
        let confirmation = ModalConfig::confirmation("Confirm");
        assert_eq!(confirmation.size, ModalSize::Small);
        assert_eq!(confirmation.radius, 0.0);

        let settings = ModalConfig::settings("Settings");
        assert_eq!(settings.footer_height, 52.0);

        let search = ModalConfig::search("Search");
        assert_eq!(search.show_backdrop, false);
    }
}
