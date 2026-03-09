//! Button widget rendering for chart crate.
//!
//! Ported from `zengeld-terminal-core::ui::render::button` so that chart modal
//! renderers can use buttons without depending on core.

use crate::render::{TextAlign, TextBaseline};
use crate::engine::render::RenderContext;
use crate::ui::widgets::types::{WidgetState, WidgetTheme};
use crate::ui::toolbar_core::IconId;
use uzor::types::Rect as WidgetRect;

/// Button configuration
#[derive(Clone, Debug)]
pub struct ButtonConfig {
    /// Button text (optional)
    pub text: Option<String>,
    /// Icon (optional)
    pub icon: Option<IconId>,
    /// Whether button is in active/toggled state
    pub active: bool,
    /// Whether button is disabled
    pub disabled: bool,
    /// Corner radius
    pub radius: f64,
    /// Padding (horizontal)
    pub padding_x: f64,
    /// Padding (vertical)
    pub padding_y: f64,
    /// Icon size
    pub icon_size: f64,
    /// Font size
    pub font_size: f64,
    /// Gap between icon and text
    pub gap: f64,
    /// Show border when active
    pub active_border: bool,
}

impl Default for ButtonConfig {
    fn default() -> Self {
        Self {
            text: None,
            icon: None,
            active: false,
            disabled: false,
            radius: 4.0,
            padding_x: 8.0,
            padding_y: 4.0,
            icon_size: 16.0,
            font_size: 13.0,
            gap: 6.0,
            active_border: false,
        }
    }
}

impl ButtonConfig {
    /// Create text-only button
    pub fn text(text: &str) -> Self {
        Self {
            text: Some(text.to_string()),
            ..Default::default()
        }
    }

    /// Create icon-only button
    pub fn icon(icon: impl Into<IconId>) -> Self {
        Self {
            icon: Some(icon.into()),
            ..Default::default()
        }
    }

    /// Create button with icon and text
    pub fn icon_text(icon: impl Into<IconId>, text: &str) -> Self {
        Self {
            icon: Some(icon.into()),
            text: Some(text.to_string()),
            ..Default::default()
        }
    }

    /// Set active state
    pub fn with_active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Set disabled state
    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set corner radius
    pub fn with_radius(mut self, radius: f64) -> Self {
        self.radius = radius;
        self
    }

    /// Set active border
    pub fn with_active_border(mut self, border: bool) -> Self {
        self.active_border = border;
        self
    }
}

/// Button rendering result
#[derive(Clone, Debug, Default)]
pub struct ButtonResult {
    /// Whether button was clicked this frame
    pub clicked: bool,
    /// Whether button is currently hovered
    pub hovered: bool,
    /// Whether button is currently pressed
    pub pressed: bool,
}

/// Draw a button with text and/or icon.
///
/// # Parameters
/// - `ctx` - Render context
/// - `config` - Button configuration
/// - `state` - Current widget state (hovered, pressed, etc.)
/// - `rect` - Button rectangle
/// - `theme` - Widget theme colors
/// - `draw_icon` - Callback to draw icon (platform-specific, pass `|_, _, _, _| {}` if unused)
pub fn draw_button<F>(
    ctx: &mut dyn RenderContext,
    config: &ButtonConfig,
    state: WidgetState,
    rect: WidgetRect,
    theme: &WidgetTheme,
    draw_icon: F,
) -> ButtonResult
where
    F: FnOnce(&mut dyn RenderContext, &IconId, WidgetRect, &str),
{
    let effective_state = if config.disabled {
        WidgetState::Disabled
    } else {
        state
    };

    // Determine colors based on state
    let (bg_color, text_color) = match effective_state {
        WidgetState::Disabled => (&theme.bg_disabled, &theme.text_disabled),
        WidgetState::Pressed => (&theme.bg_pressed, &theme.text_hover),
        WidgetState::Hovered => (&theme.bg_hover, &theme.text_hover),
        WidgetState::Normal => {
            if config.active {
                (&theme.accent, &theme.text_hover)
            } else {
                (&theme.bg_normal, &theme.text_normal)
            }
        }
    };

    // Draw background
    ctx.set_fill_color(bg_color);
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, config.radius);

    // Draw active border if enabled
    if config.active && config.active_border {
        ctx.set_stroke_color(&theme.accent);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, config.radius);
    }

    // Calculate content position
    let padding = config.padding_x.min(config.padding_y);
    let content_rect = WidgetRect::new(
        rect.x + padding,
        rect.y + padding,
        rect.width - padding * 2.0,
        rect.height - padding * 2.0,
    );

    // Draw icon if present
    let mut text_x = content_rect.x;

    if let Some(ref icon) = config.icon {
        let icon_rect = WidgetRect::new(
            content_rect.x,
            content_rect.y + content_rect.height / 2.0 - config.icon_size / 2.0,
            config.icon_size,
            config.icon_size,
        );
        draw_icon(ctx, icon, icon_rect, text_color);
        text_x = icon_rect.x + icon_rect.width + config.gap;
    }

    // Draw text if present
    if let Some(ref text) = config.text {
        ctx.set_font(&format!("{}px sans-serif", config.font_size));
        ctx.set_fill_color(text_color);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(text, text_x, rect.y + rect.height / 2.0);
    }

    ButtonResult {
        clicked: matches!(effective_state, WidgetState::Pressed),
        hovered: effective_state.is_hovered(),
        pressed: effective_state.is_pressed(),
    }
}
