//! Scrollable container and scrollbar widget for chart modals.
//!
//! Moved from `zengeld-terminal-core::ui::render::scrollable` and
//! `zengeld-terminal-core::ui::render::scrollbar` so that chart-level modal
//! renderers can use them without depending on core.
//!
//! Core re-exports `ScrollableContainer`, `ScrollableConfig`, `ScrollableResult`,
//! `ScrollbarConfig`, `draw_scrollbar`, `ScrollbarState`, and `ScrollbarResult`
//! via `pub use mlc_core::ui::scroll_widget::*`.
//!
//! Harvest: uzor scroll-bottom-anchor (the bottom-anchored free functions
//! below `ScrollableContainer::end` only — the pre-existing top-anchored
//! container/scrollbar above are this crate's own pre-2026-09-24 code, not
//! part of the messenger harvest-ready pass).

use mlc_core::engine::render::RenderContext;
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
        // Single choke point (recon: `docs/mlc/research/owner-review-2026-09-23/
        // input-hotkeys-and-modal-wheel.md` item 2): every modal that uses
        // `ScrollableContainer` reads the eased, animated offset here — none
        // of the ~30 call sites need their own animation wiring.
        let offset = scroll_state.animated_offset();
        Self {
            content_y: viewport.y - offset,
            content_width: viewport.width - config.scrollbar_width,
            viewport,
            scroll_offset: offset,
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

// =============================================================================
// Bottom-anchored mode — the messenger feed's own scroll behaviour (newest
// message at the bottom, like every chat client): land at the bottom on
// open, and keep the view pinned to the bottom as new content arrives ONLY
// while the caller was already there — a message landing underneath a
// caller who scrolled up to read history must never yank them back down.
// `ScrollableContainer` itself stays top-anchored (`content_y = viewport.y
// - offset`, unchanged above) — bottom-anchoring is entirely a matter of
// which `ScrollState::offset`/`target` value the CALLER feeds it each
// frame, so these are free functions over `&mut ScrollState` rather than a
// second container type.
// =============================================================================

/// Slack (px) within which the viewport counts as "at the bottom" — tolerant
/// of [`ScrollState::animated_offset`]'s own settle epsilon so an ease that
/// has not quite finished converging still reads as bottomed out.
const AT_BOTTOM_SLACK_PX: f64 = 4.0;

/// Whether the viewport is currently at (or within [`AT_BOTTOM_SLACK_PX`]
/// of) the bottom of `content_height` — the messenger feed's own "should a
/// growth re-pin the view" test, and the jump-to-latest button's own
/// visibility test ([`jump_button_visible`]).
pub fn is_at_bottom(scroll: &ScrollState, content_height: f64, viewport_height: f64) -> bool {
    let max_scroll = (content_height - viewport_height).max(0.0);
    max_scroll - scroll.animated_offset() <= AT_BOTTOM_SLACK_PX
}

/// Land a freshly opened bottom-anchored view (a conversation just opened,
/// no prior scroll state for it) at its own bottom — immediate, no ease,
/// same "must land this frame" contract every other `ScrollState` jump
/// (`scroll_to`/`ensure_visible`/drag) already follows.
pub fn open_at_bottom(scroll: &mut ScrollState, content_height: f64, viewport_height: f64) {
    let max_scroll = (content_height - viewport_height).max(0.0);
    scroll.scroll_to(max_scroll, content_height, viewport_height);
}

/// Content grew (a new message/day-separator landed) to `new_content_height`
/// — call AFTER the caller's own content height has already been updated to
/// its new value. `was_at_bottom` is [`is_at_bottom`]'s own answer captured
/// against the OLD content height, BEFORE the growth landed: pass `true` to
/// keep the view pinned to the new bottom, `false` to leave the numeric
/// offset exactly where the caller left it (just clamped, in case the
/// content actually shrank).
pub fn keep_bottom_anchored(
    scroll: &mut ScrollState,
    was_at_bottom: bool,
    new_content_height: f64,
    viewport_height: f64,
) {
    if was_at_bottom {
        open_at_bottom(scroll, new_content_height, viewport_height);
    } else {
        scroll.clamp(new_content_height, viewport_height);
    }
}

/// Whether the concept's floating "jump to latest" button (`.jump`) should
/// draw at all — visible exactly when there is unseen content below the
/// current view, i.e. whenever the view is NOT at the bottom.
pub fn jump_button_visible(scroll: &ScrollState, content_height: f64, viewport_height: f64) -> bool {
    !is_at_bottom(scroll, content_height, viewport_height)
}

#[cfg(test)]
mod bottom_anchor_tests {
    use super::*;

    #[test]
    fn open_at_bottom_lands_exactly_on_the_bottom() {
        let mut s = ScrollState::new();
        open_at_bottom(&mut s, 1000.0, 400.0);
        assert!((s.animated_offset() - 600.0).abs() < 0.001);
        assert!(is_at_bottom(&s, 1000.0, 400.0));
        assert!(!jump_button_visible(&s, 1000.0, 400.0));
    }

    #[test]
    fn scrolled_up_away_from_the_bottom_is_not_at_bottom() {
        let mut s = ScrollState::new();
        s.scroll_to(0.0, 1000.0, 400.0);
        assert!(!is_at_bottom(&s, 1000.0, 400.0));
        assert!(jump_button_visible(&s, 1000.0, 400.0));
    }

    #[test]
    fn keep_bottom_anchored_repins_only_when_the_caller_was_already_there() {
        let mut at_bottom = ScrollState::new();
        open_at_bottom(&mut at_bottom, 1000.0, 400.0);
        keep_bottom_anchored(&mut at_bottom, true, 1100.0, 400.0);
        assert!((at_bottom.animated_offset() - 700.0).abs() < 0.001);

        let mut scrolled_up = ScrollState::new();
        scrolled_up.scroll_to(200.0, 1000.0, 400.0);
        keep_bottom_anchored(&mut scrolled_up, false, 1100.0, 400.0);
        assert!((scrolled_up.animated_offset() - 200.0).abs() < 0.001);
    }
}
