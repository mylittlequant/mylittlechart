//! Scrollable container and scrollbar widget for chart modals.
//!
//! Moved from `zengeld-terminal-core::ui::render::scrollable` and
//! `zengeld-terminal-core::ui::render::scrollbar` so that chart-level modal
//! renderers can use them without depending on core.
//!
//! Core re-exports `ScrollableContainer`, `ScrollableConfig`, `ScrollableResult`,
//! `ScrollbarConfig`, `draw_scrollbar`, `ScrollbarState`, and `ScrollbarResult`
//! via `pub use zengeld_chart::ui::scroll_widget::*`.

use crate::engine::render::RenderContext;
use crate::ui::widgets::types::{WidgetTheme};
use uzor::types::Rect as WidgetRect;
pub use crate::ui::scroll_state::ScrollState;

// =============================================================================
// Scrollbar
// =============================================================================

/// Scrollbar configuration
#[derive(Clone, Debug)]
pub struct ScrollbarConfig {
    pub content_height: f64,
    pub viewport_height: f64,
    pub scroll_offset: f64,
    pub width: f64,
    pub min_handle_height: f64,
    pub radius: f64,
    pub padding: f64,
    pub opacity_dormant: f64,
    pub opacity_active: f64,
    pub opacity_hover: f64,
    pub horizontal: bool,
}

impl Default for ScrollbarConfig {
    fn default() -> Self {
        Self {
            content_height: 0.0,
            viewport_height: 0.0,
            scroll_offset: 0.0,
            width: 8.0,
            min_handle_height: 30.0,
            radius: 4.0,
            padding: 2.0,
            opacity_dormant: 0.0,
            opacity_active: 0.5,
            opacity_hover: 0.8,
            horizontal: false,
        }
    }
}

impl ScrollbarConfig {
    pub fn new(content_height: f64, viewport_height: f64, scroll_offset: f64) -> Self {
        Self {
            content_height,
            viewport_height,
            scroll_offset,
            ..Default::default()
        }
    }

    pub fn needs_scrollbar(&self) -> bool {
        self.content_height > self.viewport_height
    }

    pub fn visible_ratio(&self) -> f64 {
        if self.content_height <= 0.0 {
            return 1.0;
        }
        (self.viewport_height / self.content_height).clamp(0.0, 1.0)
    }

    pub fn scroll_ratio(&self) -> f64 {
        let max_scroll = (self.content_height - self.viewport_height).max(0.0);
        if max_scroll <= 0.0 {
            return 0.0;
        }
        (self.scroll_offset / max_scroll).clamp(0.0, 1.0)
    }

    pub fn max_scroll(&self) -> f64 {
        (self.content_height - self.viewport_height).max(0.0)
    }
}

/// Scrollbar rendering result
#[derive(Clone, Debug, Default)]
pub struct ScrollbarResult {
    pub scroll_offset: f64,
    pub dragged: bool,
    pub handle_hovered: bool,
    pub handle_rect: WidgetRect,
    pub track_rect: WidgetRect,
}

/// Scrollbar visual state
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollbarState {
    #[default]
    Hidden,
    Dormant,
    Active,
    HandleHovered,
    Dragging,
}

/// Draw a vertical scrollbar
pub fn draw_scrollbar(
    ctx: &mut dyn RenderContext,
    config: &ScrollbarConfig,
    state: ScrollbarState,
    rect: WidgetRect,
    theme: &WidgetTheme,
    drag_pos: Option<f64>,
) -> ScrollbarResult {
    if !config.needs_scrollbar() || matches!(state, ScrollbarState::Hidden) {
        return ScrollbarResult {
            scroll_offset: config.scroll_offset,
            handle_rect: WidgetRect::default(),
            track_rect: rect,
            ..Default::default()
        };
    }

    let opacity = match state {
        ScrollbarState::Hidden => 0.0,
        ScrollbarState::Dormant => config.opacity_dormant,
        ScrollbarState::Active => config.opacity_active,
        ScrollbarState::HandleHovered | ScrollbarState::Dragging => config.opacity_hover,
    };

    if opacity <= 0.0 {
        return ScrollbarResult {
            scroll_offset: config.scroll_offset,
            handle_rect: WidgetRect::default(),
            track_rect: rect,
            ..Default::default()
        };
    }

    let track_rect = WidgetRect::new(
        rect.x + config.padding,
        rect.y + config.padding,
        rect.width - config.padding * 2.0,
        rect.height - config.padding * 2.0,
    );

    let visible_ratio = config.visible_ratio();
    let scroll_ratio = config.scroll_ratio();

    let handle_height = (visible_ratio * track_rect.height).max(config.min_handle_height);
    let available_height = track_rect.height - handle_height;
    let mut handle_y = track_rect.y + scroll_ratio * available_height;
    let mut offset = config.scroll_offset;

    if let Some(y) = drag_pos {
        let new_ratio = ((y - track_rect.y - handle_height / 2.0) / available_height).clamp(0.0, 1.0);
        offset = new_ratio * config.max_scroll();
        handle_y = track_rect.y + new_ratio * available_height;
    }

    let handle_rect = WidgetRect::new(track_rect.x, handle_y, track_rect.width, handle_height);

    let handle_color = match state {
        ScrollbarState::HandleHovered | ScrollbarState::Dragging => &theme.text_normal,
        _ => &theme.text_disabled,
    };

    let alpha_hex = format!("{:02x}", (opacity * 255.0) as u8);
    let color_with_alpha = if handle_color.starts_with('#') && handle_color.len() == 7 {
        format!("{}{}", handle_color, alpha_hex)
    } else {
        handle_color.clone()
    };

    ctx.set_fill_color(&color_with_alpha);
    ctx.fill_rounded_rect(handle_rect.x, handle_rect.y, handle_rect.width, handle_rect.height, config.radius);

    ScrollbarResult {
        scroll_offset: offset,
        dragged: drag_pos.is_some(),
        handle_hovered: matches!(state, ScrollbarState::HandleHovered),
        handle_rect,
        track_rect,
    }
}

// =============================================================================
// Scrollable container
// =============================================================================

/// Result from scrollable container rendering
#[derive(Clone, Debug, Default)]
pub struct ScrollableResult {
    pub handle_rect: Option<WidgetRect>,
    pub track_rect: Option<WidgetRect>,
    pub content_height: f64,
    pub viewport_height: f64,
    pub has_scrollbar: bool,
}

/// Configuration for scrollable container
#[derive(Clone, Debug)]
pub struct ScrollableConfig {
    pub scrollbar_width: f64,
    pub scrollbar_padding: f64,
    pub always_show_scrollbar: bool,
}

impl Default for ScrollableConfig {
    fn default() -> Self {
        Self {
            scrollbar_width: 8.0,
            scrollbar_padding: 4.0,
            always_show_scrollbar: false,
        }
    }
}

/// Scrollable container for rendering scrollable content areas with automatic scrollbar
pub struct ScrollableContainer {
    viewport: WidgetRect,
    scroll_offset: f64,
    is_dragging: bool,
    config: ScrollableConfig,
    content_y: f64,
    content_width: f64,
}

impl ScrollableContainer {
    pub fn new(viewport: WidgetRect, scroll_state: &ScrollState, config: Option<ScrollableConfig>) -> Self {
        let config = config.unwrap_or_default();
        Self {
            content_y: viewport.y - scroll_state.offset,
            content_width: viewport.width - config.scrollbar_width,
            viewport,
            scroll_offset: scroll_state.offset,
            is_dragging: scroll_state.is_dragging,
            config,
        }
    }

    pub fn begin(&self, ctx: &mut dyn RenderContext) {
        ctx.save();
        ctx.begin_path();
        ctx.rect(self.viewport.x, self.viewport.y, self.content_width, self.viewport.height);
        ctx.clip();
    }

    pub fn content_y(&self) -> f64 {
        self.content_y
    }

    pub fn content_width(&self) -> f64 {
        self.content_width
    }

    pub fn viewport(&self) -> &WidgetRect {
        &self.viewport
    }

    pub fn scroll_offset(&self) -> f64 {
        self.scroll_offset
    }

    pub fn end(self, ctx: &mut dyn RenderContext, content_height: f64, theme: &WidgetTheme) -> ScrollableResult {
        ctx.restore();

        // Effective height: clamp to content so scrollbar/track never exceeds content area
        let effective_h = self.viewport.height.min(content_height);

        let mut result = ScrollableResult {
            content_height,
            viewport_height: effective_h,
            has_scrollbar: false,
            handle_rect: None,
            track_rect: None,
        };

        let needs_scrollbar = content_height > self.viewport.height || self.config.always_show_scrollbar;
        if !needs_scrollbar {
            return result;
        }

        result.has_scrollbar = true;

        let track_rect = WidgetRect::new(
            self.viewport.x + self.viewport.width - self.config.scrollbar_width,
            self.viewport.y + self.config.scrollbar_padding,
            self.config.scrollbar_width,
            effective_h - self.config.scrollbar_padding * 2.0,
        );

        let state = if self.is_dragging {
            ScrollbarState::Dragging
        } else {
            ScrollbarState::Active
        };

        let scrollbar_config = ScrollbarConfig::new(content_height, self.viewport.height, self.scroll_offset);
        let scrollbar_result = draw_scrollbar(ctx, &scrollbar_config, state, track_rect, theme, None);

        result.handle_rect = Some(scrollbar_result.handle_rect);
        result.track_rect = Some(scrollbar_result.track_rect);

        result
    }
}
