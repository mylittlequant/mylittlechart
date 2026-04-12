//! Right sidebar rendering — faithful clone of core's `render_right_sidebar`.
//!
//! All sub-panel rendering is in this single file (no subdirectory modules).
//! The function signature mirrors core exactly except:
//! - uses `zengeld_chart::render::RenderContext` (not uzor_render)
//! - does not depend on `FrameTheme` (uses `ToolbarTheme` for all colours)
//! - no `ThemeManager` argument (theme-settings panel is not present here)

use zengeld_chart::render::{RenderContext, TextAlign, TextBaseline, draw_svg_icon, draw_svg_multicolor};
use zengeld_chart::LayoutRect;
use zengeld_chart::ui::{Icon, scroll_widget::{ScrollableContainer, ScrollableConfig, ScrollbarConfig, ScrollbarState as SbState, draw_scrollbar}};
use zengeld_chart::ui::widgets::types::{WidgetState, WidgetTheme};
use zengeld_chart::ui::widgets::input::{InputConfig, draw_input, draw_input_cursor};
use zengeld_chart::ToolbarTheme;
use zengeld_chart::state::command::ObjectCategory;
use uzor::input::InputCoordinator;
use uzor::panels::DockPanel;
use uzor::types::Rect as WidgetRect;

use crate::state::{SidebarState, RightSidebarPanel};
use crate::types::ObjectItemState;

const MINI_MASCOT_LEFT_SVG: &str = include_str!("../../../assets/mascot/mini_mascot_left.svg");

// =============================================================================
// Result type — mirrors zengeld_chart::layout::render_frame::RightSidebarResult
// =============================================================================

/// Output of [`render_right_sidebar`].
///
/// Hit zones returned here are used by `chart-app` for click and scroll
/// dispatch, matching the data contract of core's `RightSidebarResult`.
#[derive(Clone, Debug, Default)]
pub struct RightSidebarResult {
    /// Full sidebar bounding rect.
    pub sidebar_rect: WidgetRect,
    /// Item rows: `(item_id, rect)`.
    pub item_rects: Vec<(String, WidgetRect)>,
    /// Delete-button rects for object-tree rows: `(item_id, rect)`.
    pub delete_button_rects: Vec<(String, WidgetRect)>,
    /// Settings-button rects for object-tree rows: `(item_id, rect)`.
    pub settings_button_rects: Vec<(String, WidgetRect)>,
    /// Currently hovered item id (populated externally, passed back for convenience).
    pub hovered_item_id: Option<String>,
    /// Scrollable content area rect.
    pub content_rect: WidgetRect,
    /// Total content height (used for scrollbar calculation).
    pub content_height: f64,
    /// Scrollbar handle rect (for drag detection).
    pub scrollbar_handle_rect: Option<WidgetRect>,
    /// Scrollbar track rect (for drag calculations).
    pub scrollbar_track_rect: Option<WidgetRect>,
    /// Whether the alert-create button was clicked.
    pub alert_create_clicked: bool,
    /// Per-group signal content viewport rects for scroll routing.
    ///
    /// Each entry is `(instance_id, viewport_rect)` where `viewport_rect` is the
    /// clipped area that shows the scrollable signal rows for that group.
    /// Used by `chart-app` to route wheel events to the right group scroll offset.
    pub signal_group_content_rects: Vec<(u64, WidgetRect)>,

    /// Bounding rect of the watchlist column-config dropdown panel (when rendered).
    ///
    /// `None` when the dropdown is closed.  Used by `chart-app` to detect
    /// clicks outside the dropdown so it can be auto-closed.
    pub watchlist_config_dropdown_rect: Option<WidgetRect>,

    /// Per-row drag registration rects for watchlist rows `(row_index, rect)`.
    ///
    /// The same rects as `item_rects` for `"watchlist_{i}"` entries, but
    /// stored separately so `chart-app` can route drag-start events to the
    /// watchlist reorder handler without iterating `item_rects`.
    pub watchlist_row_rects: Vec<(usize, WidgetRect)>,

    /// Separator hit zones for watchlist column header dividers.
    ///
    /// Each entry is `(separator_index, rect)` where `separator_index` is
    /// 1-based (separator 1 is between column 0 and column 1, etc.).  The
    /// rect is 8 px wide, centred on the visual separator line, spanning the
    /// full header row height.  Used by `chart-app` to start column-resize
    /// drags.
    pub watchlist_separator_rects: Vec<(usize, WidgetRect)>,

    /// Per-group scrollbar geometry for drag and track-click support.
    ///
    /// Each entry is `(instance_id, handle_rect, track_rect, content_height, viewport_height)`.
    /// Populated during signal group rendering when a scrollbar is drawn.
    /// Used by `chart-app` to detect scrollbar handle drags and track clicks.
    pub signal_group_scrollbar_rects: Vec<(u64, WidgetRect, WidgetRect, f64, f64)>,

    /// Bounding rect of the agent terminal / content area in the Agents panel.
    ///
    /// `None` when the Agents panel is not rendered.  Used by `chart-app`'s
    /// `CursorMoved` handler to auto-focus the PTY terminal on hover.
    pub agent_terminal_rect: Option<WidgetRect>,

    /// PTY terminal size in columns and rows, computed from the content area
    /// pixel dimensions.  `None` when the Agents panel is not rendered.
    ///
    /// `char_w = 7.0`, `char_h = 19.0` matches the PTY renderer character grid.
    pub agent_terminal_size: Option<(u16, u16)>,

    /// Agent input field rect (for TIM `update_field`).
    ///
    /// `None` when the Agents panel is not rendered or Chat mode is not active.
    pub agent_input_rect: Option<WidgetRect>,

    /// Agent input char boundary X positions (for TIM click-to-cursor).
    ///
    /// Contains `char_count + 1` entries — left edge of each char plus the
    /// right edge of the last character.  `None` when not rendered.
    pub agent_input_char_positions: Option<Vec<f64>>,

    /// Bounding rect of the agent chat / PTY content area (the scrollable region).
    ///
    /// Used by `chart-app` to route wheel events to `chat_scroll_offset` or
    /// `pty_scroll_offset`.  `None` when the Agents panel is not rendered.
    pub agent_content_rect: Option<WidgetRect>,

    /// Total content height of the agent chat area (pixels).
    ///
    /// Combined with `agent_content_rect.height` to compute the scroll max.
    pub agent_chat_content_height: f64,

    /// Total content height of the agent PTY area (pixels = rows * 19.0).
    pub agent_pty_content_height: f64,

    /// Scrollbar handle rect for the agent chat area.
    pub agent_chat_scrollbar_handle_rect: Option<WidgetRect>,
    /// Scrollbar track rect for the agent chat area.
    pub agent_chat_scrollbar_track_rect: Option<WidgetRect>,
    /// Scrollbar handle rect for the agent PTY area.
    pub agent_pty_scrollbar_handle_rect: Option<WidgetRect>,
    /// Scrollbar track rect for the agent PTY area.
    pub agent_pty_scrollbar_track_rect: Option<WidgetRect>,

    /// Viewport height of the agent chat area (for drag math).
    pub agent_chat_viewport_h: f64,
    /// Viewport height of the agent PTY area (for drag math).
    pub agent_pty_viewport_h: f64,

    /// Per-leaf content and viewport heights.
    ///
    /// Key: `LeafId`. Value: `(content_height, viewport_height)`.
    /// Populated for ALL rendered leaves (not just focused) so that wheel
    /// events can be routed to whichever leaf the cursor is hovering over.
    pub agent_leaf_content_heights: std::collections::HashMap<uzor::panels::LeafId, (f64, f64)>,

    /// Per-leaf scrollbar handle and track rects.
    ///
    /// Key: `LeafId`. Value: `(handle_rect, track_rect)`.
    /// Populated for ALL rendered leaves so that scrollbar drag can be
    /// initiated on any visible leaf, not only the focused one.
    pub agent_leaf_scrollbar_rects: std::collections::HashMap<uzor::panels::LeafId, (Option<WidgetRect>, Option<WidgetRect>)>,

    /// Bounding rect of the currently-rendered free-slot body (inner padded area).
    ///
    /// Set only when a `Slot1..Slot4` panel is rendered.  Used by `chart-app`
    /// for cross-container drag hit testing — checking whether the cursor is
    /// inside this slot's DockingManager area.
    pub active_slot_body_rect: Option<WidgetRect>,

    /// Index of the currently-rendered free slot (0-based, matching `slot_dockings[]`).
    ///
    /// Set only when `active_slot_body_rect` is `Some`.
    pub active_slot_index: Option<usize>,

    /// Chat line rects for selection hit-testing.
    ///
    /// Each entry is `(msg_idx, line_idx, y_top, y_bottom, leaf_id, line_text, text_x, font_tag)`.
    /// - `text_x`: pixel x where rendered text starts on this line.
    /// - `font_tag`: 0=normal 13px, 1=mono 11px, 2=italic 12px, 3=error 12px.
    /// Populated during `render_agents_chat_bubbles` for the focused leaf.
    /// Used by `chart-app` to map mouse Y → (msg, line, char) during drag-select,
    /// and to extract the exact wrapped-line text when copying a selection.
    pub agent_chat_line_rects: Vec<(u16, u16, f64, f64, uzor::panels::LeafId, String, f64, u8)>,
}

// =============================================================================
// Text helpers
// =============================================================================

/// Truncate `text` so that it fits within `max_width` pixels (as measured by
/// `ctx.measure_text`).  When truncation is needed the Unicode ellipsis `…`
/// is appended.  Returns the original string unchanged when it already fits.
fn truncate_to_width(ctx: &dyn RenderContext, text: &str, max_width: f64) -> String {
    if ctx.measure_text(text) <= max_width {
        return text.to_string();
    }
    let ellipsis = "\u{2026}"; // …
    let ellipsis_w = ctx.measure_text(ellipsis);
    let available = max_width - ellipsis_w;
    if available <= 0.0 {
        return String::new();
    }
    let mut truncated = text.to_string();
    while !truncated.is_empty() && ctx.measure_text(&truncated) > available {
        truncated.pop();
    }
    truncated.push_str(ellipsis);
    truncated
}

// =============================================================================
// Main render function
// =============================================================================

/// Render the right sidebar panel.
///
/// Mirrors `render_right_sidebar` from `zengeld-terminal-core::layout::render_ui`
/// but adapted for use from `chart-app` (no `FrameTheme`, no `ThemeManager`).
///
/// # Arguments
/// - `ctx` — mutable render context (the same `dyn RenderContext` used for the chart)
/// - `rect` — sidebar bounding rect in window coordinates
/// - `sidebar_state` — current sidebar state (panel type, scroll, items)
/// - `toolbar_theme` — colour scheme pulled from `panel_app.toolbar_theme_for_render()`
/// - `input_coordinator` — UZOR input coordinator for widget registration
pub fn render_right_sidebar(
    ctx: &mut dyn RenderContext,
    rect: &LayoutRect,
    sidebar_state: &mut SidebarState,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut InputCoordinator,
    free_item_renderer: &mut dyn FnMut(&crate::free_slot::FreeItem, (f32, f32, f32, f32), &mut dyn RenderContext),
) -> RightSidebarResult {
    let header_height = 40.0;
    // Agents panel manages its own scroll inside chat/PTY content area —
    // no sidebar-level scrollbar needed and the panel takes the full width.
    let is_agents_panel = sidebar_state.right_panel == RightSidebarPanel::Agents;
    let scrollbar_width: f64 = if is_agents_panel { 0.0 } else { 8.0 };
    let _content_padding = 12.0;

    // Content area (below header, minus scrollbar column).
    let content_rect = WidgetRect::new(
        rect.x,
        rect.y + header_height,
        rect.width - scrollbar_width,
        rect.height - header_height,
    );

    let mut result = RightSidebarResult {
        sidebar_rect: WidgetRect::new(rect.x, rect.y, rect.width, rect.height),
        content_rect,
        ..Default::default()
    };

    let panel = sidebar_state.right_panel;

    // Early return when no panel is open (should not happen — caller guards this).
    if panel == RightSidebarPanel::None {
        return result;
    }

    // Panel title and icon.
    let (title, icon) = match panel {
        RightSidebarPanel::Watchlist   => ("Watchlist",    Icon::Watchlist),
        RightSidebarPanel::Alerts      => ("Alerts",       Icon::Alert),
        RightSidebarPanel::ObjectTree  => ("Object Tree",  Icon::Layers),
        RightSidebarPanel::Signals     => ("Signals",      Icon::Signal),
        RightSidebarPanel::Connectors  => ("Connectors",   Icon::CircuitBoard),
        RightSidebarPanel::Performance => ("Performance",  Icon::Signal),
        RightSidebarPanel::Agents      => ("Agents",       Icon::Bot),
        RightSidebarPanel::Slot1       => ("Slot 1",       Icon::Bot),
        RightSidebarPanel::Slot2       => ("Slot 2",       Icon::Bot),
        RightSidebarPanel::Slot3       => ("Slot 3",       Icon::Bot),
        RightSidebarPanel::Slot4       => ("Slot 4",       Icon::Bot),
        RightSidebarPanel::None        => return result, // unreachable
    };

    // -------------------------------------------------------------------------
    // Background
    // -------------------------------------------------------------------------
    // Blur background (FrostedGlass effect — no-op on platforms that don't
    // support it, but the call is always safe).
    ctx.draw_blur_background(rect.x, rect.y, rect.width, rect.height);

    // Opaque sidebar body — same color as toolbars.
    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rect(rect.x, rect.y, rect.width, rect.height);

    // Left border (1 px separator between chart and sidebar).
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(rect.x, rect.y);
    ctx.line_to(rect.x, rect.y + rect.height);
    ctx.stroke();

    // Draggable separator hit zone — 8 px wide, centred on the left border.
    // The visual line is 1 px; the hit zone is wider for easy grabbing.
    let sep_hit_w = 8.0;
    let sep_hit_x = rect.x - sep_hit_w / 2.0;
    input_coordinator.register(
        "right_sidebar_separator",
        WidgetRect::new(sep_hit_x, rect.y, sep_hit_w, rect.height),
        uzor::input::Sense::CLICK,
    );

    // -------------------------------------------------------------------------
    // Header (40 px)
    // -------------------------------------------------------------------------
    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rect(rect.x, rect.y, rect.width, header_height);

    // Header icon (left side, 18 × 18, centred vertically).
    let icon_size = 18.0;
    let icon_x = rect.x + 12.0;
    let icon_y = rect.y + (header_height - icon_size) / 2.0;
    if panel == RightSidebarPanel::Agents {
        draw_svg_multicolor(ctx, MINI_MASCOT_LEFT_SVG, icon_x, icon_y, icon_size, icon_size);
    } else {
        draw_svg_icon(ctx, icon.svg(), icon_x, icon_y, icon_size, icon_size, &toolbar_theme.item_text_muted);
    }

    // Header title.
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(title, rect.x + 36.0, rect.y + header_height / 2.0);

    // Close button (X) on right side of header — with hover highlight.
    let close_size = 16.0;
    let close_pad = 4.0; // padding around icon for hover bg
    let close_x = rect.x + rect.width - close_size - 12.0;
    let close_y = rect.y + (header_height - close_size) / 2.0;
    let close_hovered = input_coordinator
        .is_hovered(&uzor::types::WidgetId::new("right_sidebar_close"));
    if close_hovered {
        // Draw a subtle rounded hover background behind the icon.
        ctx.set_fill_color(&toolbar_theme.item_bg_hover);
        let bg_x = close_x - close_pad;
        let bg_y = close_y - close_pad;
        let bg_s = close_size + close_pad * 2.0;
        ctx.fill_rounded_rect(bg_x, bg_y, bg_s, bg_s, 4.0);
    }
    let close_color = if close_hovered { &toolbar_theme.item_text } else { &toolbar_theme.item_text_muted };
    draw_svg_icon(ctx, Icon::Close.svg(), close_x, close_y, close_size, close_size, close_color);

    // Register close button with InputCoordinator so clicks are detected.
    input_coordinator.register(
        "right_sidebar_close",
        WidgetRect::new(close_x, close_y, close_size, close_size),
        uzor::input::Sense::CLICK,
    );
    result.item_rects.push((
        "right_sidebar_close".to_string(),
        WidgetRect::new(close_x, close_y, close_size, close_size),
    ));

    // Alerts panel: add (+) button to the left of the close button.
    if panel == RightSidebarPanel::Alerts {
        let add_size = 16.0;
        let add_x = close_x - add_size - 8.0;
        let add_y = rect.y + (header_height - add_size) / 2.0;
        draw_svg_icon(ctx, Icon::Plus.svg(), add_x, add_y, add_size, add_size, &toolbar_theme.item_text_muted);
        input_coordinator.register(
            "alert_add_button",
            WidgetRect::new(add_x, add_y, add_size, add_size),
            uzor::input::Sense::CLICK,
        );
        result.item_rects.push((
            "alert_add_button".to_string(),
            WidgetRect::new(add_x, add_y, add_size, add_size),
        ));
    }

    // Watchlist panel: "expand" button and "settings/columns" button.
    if panel == RightSidebarPanel::Watchlist {
        let btn_size = 16.0;
        // Settings / column-config button — leftmost of the two, to the left of close.
        let col_x = close_x - btn_size - 8.0;
        let col_y = rect.y + (header_height - btn_size) / 2.0;
        let col_hovered = input_coordinator
            .is_hovered(&uzor::types::WidgetId::new("watchlist_column_config"));
        if col_hovered {
            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rounded_rect(col_x - close_pad, col_y - close_pad, btn_size + close_pad * 2.0, btn_size + close_pad * 2.0, 4.0);
        }
        let col_color = if col_hovered { &toolbar_theme.item_text } else { &toolbar_theme.item_text_muted };
        draw_svg_icon(ctx, Icon::MoreHorizontal.svg(), col_x, col_y, btn_size, btn_size, col_color);
        input_coordinator.register(
            "watchlist_column_config",
            WidgetRect::new(col_x, col_y, btn_size, btn_size),
            uzor::input::Sense::CLICK,
        );
        result.item_rects.push((
            "watchlist_column_config".to_string(),
            WidgetRect::new(col_x, col_y, btn_size, btn_size),
        ));

        // Expand / open-modal button — to the left of the column-config button.
        let expand_x = col_x - btn_size - 8.0;
        let expand_y = col_y;
        let expand_hovered = input_coordinator
            .is_hovered(&uzor::types::WidgetId::new("watchlist_open_modal"));
        if expand_hovered {
            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rounded_rect(expand_x - close_pad, expand_y - close_pad, btn_size + close_pad * 2.0, btn_size + close_pad * 2.0, 4.0);
        }
        let expand_color = if expand_hovered { &toolbar_theme.item_text } else { &toolbar_theme.item_text_muted };
        draw_svg_icon(ctx, Icon::Grid.svg(), expand_x, expand_y, btn_size, btn_size, expand_color);
        input_coordinator.register(
            "watchlist_open_modal",
            WidgetRect::new(expand_x, expand_y, btn_size, btn_size),
            uzor::input::Sense::CLICK,
        );
        result.item_rects.push((
            "watchlist_open_modal".to_string(),
            WidgetRect::new(expand_x, expand_y, btn_size, btn_size),
        ));

    }

    // Slot panels: add [+] button in the header to spawn a new trading panel.
    if let Some(slot_idx) = panel.slot_index() {
        let slot_idx = slot_idx as usize;
        let add_size = 16.0;
        let add_x = close_x - add_size - 8.0;
        let add_y = rect.y + (header_height - add_size) / 2.0;
        let spawn_id = format!("slot:{}:new", slot_idx);
        let spawn_hov = input_coordinator.is_hovered(&uzor::types::WidgetId::new(&spawn_id));
        if spawn_hov {
            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rounded_rect(add_x - close_pad, add_y - close_pad, add_size + close_pad * 2.0, add_size + close_pad * 2.0, 4.0);
        }
        let spawn_color = if spawn_hov { "#58a6ff" } else { &toolbar_theme.item_text_muted };
        draw_svg_icon(ctx, Icon::Plus.svg(), add_x, add_y, add_size, add_size, spawn_color);
        input_coordinator.register(
            spawn_id.as_str(),
            WidgetRect::new(add_x, add_y, add_size, add_size),
            uzor::input::Sense::CLICK,
        );
        result.item_rects.push((spawn_id, WidgetRect::new(add_x, add_y, add_size, add_size)));
    }

    // Header bottom border.
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(rect.x, rect.y + header_height);
    ctx.line_to(rect.x + rect.width, rect.y + header_height);
    ctx.stroke();

    // -------------------------------------------------------------------------
    // Scrollable content area
    // -------------------------------------------------------------------------

    // For the watchlist panel, the column header row is rendered OUTSIDE the
    // scrollable clip so it stays fixed at the top.  The scrollable viewport is
    // shrunk by the header height so that only data rows scroll.
    let watchlist_header_h = if panel == RightSidebarPanel::Watchlist { 23.0 } else { 0.0 }; // 22 header + 1 separator

    let viewport_rect = WidgetRect::new(
        content_rect.x,
        content_rect.y + watchlist_header_h,
        content_rect.width + scrollbar_width,
        (content_rect.height - watchlist_header_h).max(0.0),
    );
    let scroll_config = ScrollableConfig {
        scrollbar_width,
        scrollbar_padding: 4.0,
        always_show_scrollbar: false,
    };
    let scrollable = ScrollableContainer::new(
        viewport_rect,
        sidebar_state.current_right_scroll(),
        Some(scroll_config),
    );

    // Draw the watchlist column header at a fixed position before clipping.
    if panel == RightSidebarPanel::Watchlist {
        render_watchlist_column_header(
            ctx,
            rect,
            content_rect.y,
            rect.width - scrollbar_width,
            sidebar_state,
            toolbar_theme,
            &mut result,
            input_coordinator,
        );
    }

    scrollable.begin(ctx);

    let content_y = scrollable.content_y();
    let content_width = rect.width - scrollbar_width;

    let mut content_height = 0.0;

    match panel {
        RightSidebarPanel::Watchlist => {
            content_height = render_watchlist_items(
                ctx,
                rect,
                content_y,
                content_width,
                scrollbar_width,
                sidebar_state,
                toolbar_theme,
                &mut result,
                input_coordinator,
            );
        }

        RightSidebarPanel::Alerts => {
            content_height = render_alert_items(
                ctx,
                rect,
                content_y,
                content_width,
                scrollbar_width,
                sidebar_state,
                toolbar_theme,
                &mut result,
                input_coordinator,
            );
        }

        RightSidebarPanel::ObjectTree => {
            content_height = render_object_tree_items(
                ctx,
                rect,
                content_y,
                content_width,
                scrollbar_width,
                sidebar_state,
                toolbar_theme,
                &mut result,
                input_coordinator,
            );
        }

        RightSidebarPanel::Signals => {
            content_height = render_indicator_signals(
                ctx,
                rect,
                content_y,
                content_width,
                scrollbar_width,
                sidebar_state,
                toolbar_theme,
                &mut result,
                input_coordinator,
            );
        }

        RightSidebarPanel::Connectors => {
            content_height = render_connectors_panel(
                ctx,
                rect,
                content_y,
                content_width,
                sidebar_state,
                toolbar_theme,
                &mut result,
                input_coordinator,
            );
        }

        RightSidebarPanel::Performance => {
            content_height = render_performance_panel(
                ctx,
                rect,
                content_y,
                content_width,
                sidebar_state,
                toolbar_theme,
                &mut result,
                input_coordinator,
            );
        }

        RightSidebarPanel::Agents => {
            content_height = render_agents_panel(
                ctx,
                rect,
                content_y,
                content_width,
                sidebar_state,
                toolbar_theme,
                &mut result,
                input_coordinator,
            );
        }

        RightSidebarPanel::Slot1
        | RightSidebarPanel::Slot2
        | RightSidebarPanel::Slot3
        | RightSidebarPanel::Slot4 => {
            let slot_idx = panel.slot_index().unwrap_or(0) as usize;
            content_height = render_slot_panel(
                ctx,
                rect,
                content_y,
                content_width,
                slot_idx,
                sidebar_state,
                toolbar_theme,
                &mut result,
                input_coordinator,
                free_item_renderer,
            );
        }

        RightSidebarPanel::None => {}
    }

    // -------------------------------------------------------------------------
    // End scroll container — draws scrollbar if needed.
    // -------------------------------------------------------------------------
    // Agents panel: clamp content_height to viewport so the sidebar-level
    // scrollbar never appears. Internal chat/PTY scrolling is handled inside
    // the panel itself.
    if is_agents_panel {
        content_height = viewport_rect.height;
    }
    let widget_theme = zengeld_chart::ui::widgets::types::WidgetTheme::default();
    let scroll_result = scrollable.end(ctx, content_height, &widget_theme);

    result.content_height = scroll_result.content_height;
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;

    // -------------------------------------------------------------------------
    // Watchlist column-config dropdown overlay (rendered over scrollable area).
    // -------------------------------------------------------------------------
    if panel == RightSidebarPanel::Watchlist && sidebar_state.watchlist_config_dropdown_open {
        render_watchlist_config_dropdown(
            ctx,
            rect,
            header_height,
            sidebar_state,
            toolbar_theme,
            &mut result,
            input_coordinator,
        );
    }

    // -------------------------------------------------------------------------
    // Slot spawn dropdown overlay (rendered on top of slot content).
    // -------------------------------------------------------------------------
    if let Some(slot_idx) = panel.slot_index() {
        let slot_idx = slot_idx as usize;
        if sidebar_state.slot_spawn_dropdown == Some(slot_idx) {
            render_slot_spawn_dropdown(
                ctx,
                rect,
                header_height,
                slot_idx,
                toolbar_theme,
                input_coordinator,
            );
        }
    }

    result
}

// =============================================================================
// Watchlist column-config dropdown
// =============================================================================

/// Render the watchlist column-config dropdown overlay.
///
/// The panel drops below the sidebar header and appears as a filled overlay
/// over the watchlist content.  It contains a checkbox row for each toggleable
/// column.
fn render_watchlist_config_dropdown(
    ctx: &mut dyn RenderContext,
    rect: &LayoutRect,
    header_height: f64,
    state: &SidebarState,
    theme: &ToolbarTheme,
    result: &mut RightSidebarResult,
    input_coordinator: &mut InputCoordinator,
) {
    let row_h = 28.0;
    let pad = 10.0;
    let checkbox_size = 10.0;
    let gap = 8.0; // gap between checkbox and label

    let col_cfg = state
        .watchlist_manager
        .active_list()
        .map(|l| l.column_config.clone())
        .unwrap_or_default();

    // Column options: (field_name, label, current_value)
    let options: &[(&str, &str, bool)] = &[
        ("show_exchange",     "Exchange",      col_cfg.show_exchange),
        ("show_account_type", "Type",          col_cfg.show_account_type),
        ("show_last_price",   "Last Price",    col_cfg.show_last_price),
        ("show_change_pct",   "Change %",      col_cfg.show_change_pct),
        ("show_change_abs",   "Change",        col_cfg.show_change_abs),
        ("show_volume",       "Volume",        col_cfg.show_volume),
        ("show_high_low",     "High / Low",    col_cfg.show_high_low),
        ("show_align_columns","Align columns", col_cfg.align_columns),
    ];

    let dropdown_w = 180.0;
    // Extra height for the separator line before the last option (Align columns).
    let sep_line_h = 8.0;
    let dropdown_h = row_h * options.len() as f64 + pad * 2.0 + sep_line_h;

    // Position: drop below the header, right-aligned inside the sidebar.
    let dropdown_x = rect.x + rect.width - dropdown_w - 4.0;
    let dropdown_y = rect.y + header_height;

    let dropdown_rect = WidgetRect::new(dropdown_x, dropdown_y, dropdown_w, dropdown_h);

    // Background.
    ctx.set_fill_color(&theme.background);
    ctx.fill_rect(dropdown_rect.x, dropdown_rect.y, dropdown_rect.width, dropdown_rect.height);

    // Border.
    ctx.set_stroke_color(&theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(dropdown_rect.x, dropdown_rect.y);
    ctx.line_to(dropdown_rect.x + dropdown_rect.width, dropdown_rect.y);
    ctx.line_to(dropdown_rect.x + dropdown_rect.width, dropdown_rect.y + dropdown_rect.height);
    ctx.line_to(dropdown_rect.x, dropdown_rect.y + dropdown_rect.height);
    ctx.line_to(dropdown_rect.x, dropdown_rect.y);
    ctx.stroke();

    // Register a transparent backdrop so clicks inside the dropdown
    // are consumed (not forwarded to the watchlist rows underneath).
    input_coordinator.register(
        "watchlist_cfg_backdrop",
        dropdown_rect,
        uzor::input::Sense::CLICK,
    );

    // Index of the separator (before "Align columns", the last option).
    let sep_before_idx = options.len() - 1;

    // Option rows.
    for (row_idx, (field, label, enabled)) in options.iter().enumerate() {
        // Offset rows after the separator line.
        let extra = if row_idx >= sep_before_idx { sep_line_h } else { 0.0 };
        let row_y = dropdown_rect.y + pad + row_idx as f64 * row_h + extra;

        // Draw separator line before this row if needed.
        if row_idx == sep_before_idx {
            let line_y = row_y - sep_line_h / 2.0;
            ctx.set_stroke_color(&theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.begin_path();
            ctx.move_to(dropdown_rect.x + pad, line_y);
            ctx.line_to(dropdown_rect.x + dropdown_w - pad, line_y);
            ctx.stroke();
        }
        let widget_id = format!("watchlist_cfg:{}", field);

        let row_rect = WidgetRect::new(dropdown_rect.x, row_y, dropdown_w, row_h);

        let is_hovered = input_coordinator
            .is_hovered(&uzor::types::WidgetId::new(&widget_id));

        if is_hovered {
            ctx.set_fill_color(&theme.item_bg_hover);
            ctx.fill_rect(row_rect.x, row_rect.y, row_rect.width, row_rect.height);
        }

        // Checkbox rect (filled = enabled, empty = disabled).
        let cb_x = dropdown_rect.x + pad;
        let cb_y = row_y + (row_h - checkbox_size) / 2.0;

        ctx.set_stroke_color(&theme.item_text_muted);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(cb_x, cb_y);
        ctx.line_to(cb_x + checkbox_size, cb_y);
        ctx.line_to(cb_x + checkbox_size, cb_y + checkbox_size);
        ctx.line_to(cb_x, cb_y + checkbox_size);
        ctx.line_to(cb_x, cb_y);
        ctx.stroke();

        if *enabled {
            // Fill checkbox interior with accent color.
            ctx.set_fill_color("#4a9eff");
            ctx.fill_rect(cb_x + 1.0, cb_y + 1.0, checkbox_size - 2.0, checkbox_size - 2.0);
        }

        // Label text.
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&theme.item_text);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, cb_x + checkbox_size + gap, row_y + row_h / 2.0);

        // Register the row for click detection.
        input_coordinator.register(
            widget_id.as_str(),
            row_rect,
            uzor::input::Sense::CLICK,
        );
        result.item_rects.push((widget_id, row_rect));
    }

    result.watchlist_config_dropdown_rect = Some(dropdown_rect);
}

// =============================================================================
// Slot spawn dropdown
// =============================================================================

/// Render the slot `[+]` spawn dropdown overlay.
///
/// Appears directly below the sidebar header, right-aligned.  Lists all 11
/// `FreeItem` variants.  Drawn last so it sits on top of all slot content.
fn render_slot_spawn_dropdown(
    ctx: &mut dyn RenderContext,
    rect: &LayoutRect,
    header_height: f64,
    slot_idx: usize,
    theme: &ToolbarTheme,
    input_coordinator: &mut InputCoordinator,
) {
    const ITEMS: &[(&str, &str)] = &[
        ("dom",                "DOM"),
        ("footprint",          "Footprint"),
        ("volume_profile",     "Volume Profile"),
        ("liquidity_heatmap",  "Liquidity Heatmap"),
        ("big_trades",         "Big Trades"),
        ("l2_tape",            "L2 Tape"),
        ("order_entry",        "Order Entry"),
        ("position_manager",   "Positions"),
        ("trade_log",          "Trade Log"),
        ("risk_calculator",    "Risk Calculator"),
        ("trading_container",  "Trading"),
    ];

    let row_h = 24.0;
    let pad_v = 4.0;
    let pad_h = 12.0;
    let dropdown_w = 180.0;
    let dropdown_h = row_h * ITEMS.len() as f64 + pad_v * 2.0;

    // Anchor: right-aligned inside the sidebar, just below the header.
    let dropdown_x = rect.x + rect.width - dropdown_w - 4.0;
    let dropdown_y = rect.y + header_height;

    // Background.
    ctx.set_fill_color("#1a1f2aee");
    ctx.fill_rect(dropdown_x, dropdown_y, dropdown_w, dropdown_h);

    // Border.
    ctx.set_stroke_color(theme.separator.as_str());
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(dropdown_x, dropdown_y);
    ctx.line_to(dropdown_x + dropdown_w, dropdown_y);
    ctx.line_to(dropdown_x + dropdown_w, dropdown_y + dropdown_h);
    ctx.line_to(dropdown_x, dropdown_y + dropdown_h);
    ctx.line_to(dropdown_x, dropdown_y);
    ctx.stroke();

    for (row_idx, (kind_str, label)) in ITEMS.iter().enumerate() {
        let row_y = dropdown_y + pad_v + row_idx as f64 * row_h;
        let row_rect = WidgetRect::new(dropdown_x, row_y, dropdown_w, row_h);

        let widget_id = format!("slot:{}:spawn:{}", slot_idx, kind_str);
        let is_hovered = input_coordinator.is_hovered(&uzor::types::WidgetId::new(&widget_id));

        if is_hovered {
            ctx.set_fill_color("#2d3748ff");
            ctx.fill_rect(row_rect.x, row_rect.y, row_rect.width, row_rect.height);
        }

        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(if is_hovered { "#e2e8f0" } else { "#c9d1d9" });
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, dropdown_x + pad_h, row_y + row_h / 2.0);

        input_coordinator.register(
            widget_id.as_str(),
            row_rect,
            uzor::input::Sense::CLICK,
        );
    }
}

// =============================================================================
// Watchlist panel
// =============================================================================

/// Minimum gap in pixels between adjacent separators (and between a separator
/// and the area edge).  Approximately 2 characters at 11px font.
const WATCHLIST_SEP_MIN_GAP: f64 = 16.0;

/// Render the watchlist column header row at a fixed (non-scrolling) position.
///
/// This is called BEFORE the scrollable clip is established so the header
/// stays pinned at the top of the content area regardless of scroll position.
fn render_watchlist_column_header(
    ctx: &mut dyn RenderContext,
    rect: &LayoutRect,
    header_y: f64,
    content_width: f64,
    state: &SidebarState,
    theme: &ToolbarTheme,
    result: &mut RightSidebarResult,
    input_coordinator: &mut InputCoordinator,
) {
    let item_padding = 8.0;
    let header_row_h = 22.0;

    // Read column config from WatchlistManager; fall back to defaults when no list is active.
    let col_cfg = state
        .watchlist_manager
        .active_list()
        .map(|l| l.column_config.clone())
        .unwrap_or_default();

    // Build dynamic column list based on config.
    let mut col_labels: Vec<&str> = Vec::with_capacity(9);
    col_labels.push("Symbol");
    if col_cfg.show_exchange      { col_labels.push("Exchange"); }
    if col_cfg.show_account_type  { col_labels.push("Type"); }
    if col_cfg.show_last_price    { col_labels.push("Last"); }
    if col_cfg.show_change_pct    { col_labels.push("Chg %"); }
    if col_cfg.show_change_abs    { col_labels.push("Chg"); }
    if col_cfg.show_high_low      { col_labels.push("High"); col_labels.push("Low"); }
    if col_cfg.show_volume        { col_labels.push("Vol"); }

    let n_cols = col_labels.len();
    let usable_w = content_width - item_padding * 2.0;
    let area_left = rect.x + item_padding;
    let area_right = area_left + usable_w;
    let n_seps = n_cols.saturating_sub(1);
    let equal_col_w = if n_cols > 0 { usable_w / n_cols as f64 } else { 0.0 };

    let mut col_x: Vec<f64> = Vec::with_capacity(n_cols);
    {
        let mut x = area_left;
        for _ in 0..n_cols {
            col_x.push(x);
            x += equal_col_w;
        }
    }
    let default_sep_x: Vec<f64> = (1..n_cols).map(|i| col_x[i]).collect();

    let sep_positions: Vec<f64> = {
        let use_custom = col_cfg.separator_offsets
            .as_ref()
            .map(|o| o.len() == n_seps)
            .unwrap_or(false);
        if use_custom && n_seps > 0 {
            let offsets = col_cfg.separator_offsets.as_ref().unwrap();
            let mut positions: Vec<f64> = offsets.iter().map(|&o| area_left + o).collect();
            let mut prev = area_left;
            for p in positions.iter_mut() {
                let min_pos = prev + WATCHLIST_SEP_MIN_GAP;
                if *p < min_pos { *p = min_pos; }
                prev = *p;
            }
            let mut next = area_right;
            for p in positions.iter_mut().rev() {
                let max_pos = next - WATCHLIST_SEP_MIN_GAP;
                if *p > max_pos { *p = max_pos; }
                next = *p;
            }
            positions
        } else {
            default_sep_x
        }
    };

    let col_clip = |col_i: usize| -> (f64, f64) {
        let left = if col_i == 0 { area_left } else { sep_positions[col_i - 1] };
        let right = if col_i + 1 < n_seps + 1 && col_i < sep_positions.len() {
            sep_positions[col_i]
        } else {
            area_right
        };
        (left, right)
    };

    let col_text_x = |col_i: usize, clip_l: f64, clip_r: f64| -> (TextAlign, f64) {
        if col_i == 0 {
            (TextAlign::Left, clip_l + 2.0)
        } else if col_i == n_cols - 1 {
            (TextAlign::Right, clip_r - 2.0)
        } else {
            (TextAlign::Center, (clip_l + clip_r) / 2.0)
        }
    };

    // Background fill for the header row so it visually covers scrolled data.
    ctx.set_fill_color(&theme.background);
    ctx.fill_rect(rect.x, header_y, content_width, header_row_h);

    // Sort-by-color button in column header — 10px wide rect.
    // Always filled with hover color so it's visible against the header bg.
    {
        let flag_w = 10.0;
        let flag_x = rect.x;
        let flag_y = header_y;
        let flag_h = header_row_h;
        let flag_hovered = input_coordinator.is_hovered(&uzor::types::WidgetId::new("watchlist_sort_color"));
        let sort_active = state.watchlist_sort_mode != 0;

        if sort_active {
            let flag_fill = if flag_hovered {
                "#ffffff"
            } else {
                match state.watchlist_sort_mode {
                    1 => "#ef5350",
                    2 => "#6b7280",
                    _ => "#555566",
                }
            };
            ctx.set_fill_color(flag_fill);
            ctx.fill_rect(flag_x, flag_y, flag_w, flag_h);
        } else {
            // No active sort — always show hover-colored rect so it's discoverable.
            let fill = if flag_hovered { &theme.item_bg_active } else { &theme.item_bg_hover };
            ctx.set_fill_color(fill);
            ctx.fill_rect(flag_x, flag_y, flag_w, flag_h);
        }

        let flag_btn_rect = WidgetRect::new(flag_x, flag_y, flag_w, flag_h);
        input_coordinator.register("watchlist_sort_color", flag_btn_rect, uzor::input::Sense::CLICK);
        result.item_rects.push(("watchlist_sort_color".to_string(), flag_btn_rect));
    }

    ctx.set_font("10px sans-serif");
    ctx.set_fill_color(&theme.item_text_muted);
    ctx.set_text_baseline(TextBaseline::Middle);
    for (i, label) in col_labels.iter().enumerate() {
        let (clip_l, clip_r) = col_clip(i);
        let clip_w = (clip_r - clip_l).max(0.0);
        if clip_w < 1.0 { continue; }
        let (align, tx) = col_text_x(i, clip_l, clip_r);
        ctx.set_text_align(align);
        let display_label = truncate_to_width(ctx, label, clip_w);
        ctx.fill_text(&display_label, tx, header_y + header_row_h / 2.0);
    }

    // Separator lines and draggable hit zones.
    let sep_hit_w = 8.0;
    ctx.set_stroke_width(1.0);
    for (sep_i, &sep_pos) in sep_positions[..n_seps].iter().enumerate() {
        let sep_x = sep_pos - 0.5;
        let sep_id = format!("watchlist_sep_{}", sep_i + 1);
        let sep_is_hovered = input_coordinator
            .is_hovered(&uzor::types::WidgetId::new(&sep_id));
        if sep_is_hovered {
            ctx.set_stroke_color("#4a9eff");
        } else {
            ctx.set_stroke_color(&theme.separator);
        }
        ctx.begin_path();
        ctx.move_to(sep_x, header_y + 3.0);
        ctx.line_to(sep_x, header_y + header_row_h - 3.0);
        ctx.stroke();
        let hit_rect = WidgetRect::new(sep_x - sep_hit_w / 2.0, header_y, sep_hit_w, header_row_h);
        input_coordinator.register(sep_id.as_str(), hit_rect, uzor::input::Sense::CLICK_AND_DRAG);
        result.watchlist_separator_rects.push((sep_i + 1, hit_rect));
    }

    // Separator line below the header row.
    ctx.set_fill_color(&theme.separator);
    ctx.fill_rect(area_left, header_y + header_row_h, usable_w, 1.0);
}

fn render_watchlist_items(
    ctx: &mut dyn RenderContext,
    rect: &LayoutRect,
    content_y: f64,
    content_width: f64,
    scrollbar_width: f64,
    state: &SidebarState,
    theme: &ToolbarTheme,
    result: &mut RightSidebarResult,
    input_coordinator: &mut InputCoordinator,
) -> f64 {
    let _ = scrollbar_width;

    if state.watchlist_items.is_empty() {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&theme.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(
            "No symbols in watchlist",
            rect.x + content_width / 2.0,
            content_y,
        );
        ctx.fill_text(
            "Click + to add symbols",
            rect.x + content_width / 2.0,
            content_y + 20.0,
        );
        return 60.0;
    }

    let item_padding = 8.0;
    let data_row_h = 36.0;
    let delete_icon_size = 12.0;
    let delete_icon_right_pad = 6.0;
    let mut current_y = content_y;

    // -------------------------------------------------------------------------
    // Virtual-scrolling bounds.
    //
    // `content_y = viewport.y - scroll_offset`, so:
    //   viewport_top    = content_y + scroll_offset
    //   viewport_bottom = viewport_top + viewport_height
    //
    // Constants that match the layout computed in render_right_sidebar():
    //   sidebar header : 40 px
    //   watchlist col header: 23 px
    //   → content area height = rect.height - 63 px
    // -------------------------------------------------------------------------
    let scroll_offset = state.current_right_scroll().offset;
    let viewport_top = content_y + scroll_offset;
    let watchlist_col_header_h = 23.0;
    let sidebar_header_h = 40.0;
    let viewport_height = (rect.height - sidebar_header_h - watchlist_col_header_h).max(0.0);
    let viewport_bottom = viewport_top + viewport_height;

    // Read column config from WatchlistManager; fall back to defaults when no list is active.
    let col_cfg = state
        .watchlist_manager
        .active_list()
        .map(|l| l.column_config.clone())
        .unwrap_or_default();

    // Build dynamic column list based on config.
    // All columns are left-aligned so that clipping from the right preserves text.
    let mut col_labels: Vec<&str> = Vec::with_capacity(9);
    col_labels.push("Symbol");

    if col_cfg.show_exchange {
        col_labels.push("Exchange");
    }
    if col_cfg.show_account_type {
        col_labels.push("Type");
    }
    if col_cfg.show_last_price {
        col_labels.push("Last");
    }
    if col_cfg.show_change_pct {
        col_labels.push("Chg %");
    }
    if col_cfg.show_change_abs {
        col_labels.push("Chg");
    }
    if col_cfg.show_high_low {
        col_labels.push("High");
        col_labels.push("Low");
    }
    if col_cfg.show_volume {
        col_labels.push("Vol");
    }

    let n_cols = col_labels.len();

    // Columns use the full width — delete button renders as an overlay on hover.
    let usable_w = content_width - item_padding * 2.0;

    // The area_left is the absolute X where the usable column area begins.
    let area_left = rect.x + item_padding;
    let area_right = area_left + usable_w;

    let data_cols = n_cols.saturating_sub(1);

    // -------------------------------------------------------------------------
    // Default column layout — all columns equal width.  Text alignment per
    // column creates the visual spread: first col left-aligned, last col
    // right-aligned, middle columns centered.
    // -------------------------------------------------------------------------
    let equal_col_w = if n_cols > 0 { usable_w / n_cols as f64 } else { 0.0 };

    // Absolute X position of each column's content (fixed — never changes).
    let mut col_x: Vec<f64> = Vec::with_capacity(n_cols);
    {
        let mut x = area_left;
        for _i in 0..n_cols {
            col_x.push(x);
            x += equal_col_w;
        }
    }

    // -------------------------------------------------------------------------
    // Default separator positions: left edge of each data column (absolute X).
    // sep_positions[i] is the absolute X of the separator between column i and i+1.
    // -------------------------------------------------------------------------
    let n_seps = n_cols.saturating_sub(1);
    let default_sep_x: Vec<f64> = (1..n_cols).map(|i| col_x[i]).collect();

    // -------------------------------------------------------------------------
    // Resolve actual separator positions.
    // Use custom offsets when present and the count matches; otherwise use defaults.
    // Clamp to maintain minimum gap between neighbours.
    // -------------------------------------------------------------------------
    let sep_positions: Vec<f64> = {
        let use_custom = col_cfg.separator_offsets
            .as_ref()
            .map(|o| o.len() == n_seps)
            .unwrap_or(false);

        if use_custom && n_seps > 0 {
            let offsets = col_cfg.separator_offsets.as_ref().unwrap();
            // Convert offsets (relative to area_left) to absolute X, then clamp.
            let mut positions: Vec<f64> = offsets.iter().map(|&o| area_left + o).collect();
            // Forward pass: ensure each separator is at least MIN_GAP after the previous boundary.
            let mut prev = area_left;
            for p in positions.iter_mut() {
                let min_pos = prev + WATCHLIST_SEP_MIN_GAP;
                if *p < min_pos {
                    *p = min_pos;
                }
                prev = *p;
            }
            // Backward pass: ensure each separator is at least MIN_GAP before the next boundary.
            let mut next = area_right;
            for p in positions.iter_mut().rev() {
                let max_pos = next - WATCHLIST_SEP_MIN_GAP;
                if *p > max_pos {
                    *p = max_pos;
                }
                next = *p;
            }
            positions
        } else {
            default_sep_x.clone()
        }
    };

    // Use a smaller font when many data columns are visible (>4) to avoid overflow.
    let data_font = if data_cols > 4 { "10px sans-serif" } else { "11px sans-serif" };

    // -------------------------------------------------------------------------
    // Helper: compute (clip_left, clip_right) for a given column index.
    // col 0:       [area_left       .. sep_positions[0]]
    // col i (mid): [sep_positions[i-1] .. sep_positions[i]]
    // last col:    [sep_positions[n-1] .. area_right]
    // -------------------------------------------------------------------------
    let col_clip = |col_i: usize| -> (f64, f64) {
        let left = if col_i == 0 {
            area_left
        } else {
            sep_positions[col_i - 1]
        };
        let right = if col_i + 1 < n_seps + 1 && col_i < sep_positions.len() {
            sep_positions[col_i]
        } else {
            area_right
        };
        (left, right)
    };

    // -------------------------------------------------------------------------
    // Helper: text position within a clipped column.
    // First column → left-aligned, last column → right-aligned, middle → centered.
    // -------------------------------------------------------------------------
    let col_text_x = |col_i: usize, clip_l: f64, clip_r: f64| -> (TextAlign, f64) {
        if col_i == 0 {
            (TextAlign::Left, clip_l + 2.0)
        } else if col_i == n_cols - 1 {
            (TextAlign::Right, clip_r - 2.0)
        } else {
            (TextAlign::Center, (clip_l + clip_r) / 2.0)
        }
    };

    // -------------------------------------------------------------------------
    // Data rows.
    // (Column header is rendered separately before the scrollable clip —
    //  see render_watchlist_column_header.)
    // -------------------------------------------------------------------------
    for (i, item) in state.watchlist_items.iter().enumerate() {
        let row_id = format!("watchlist_{}", i);
        let del_id = format!("watchlist_delete_{}", i);

        let row_rect = WidgetRect::new(
            rect.x,
            current_y,
            content_width,
            data_row_h,
        );

        // Register row FIRST so delete button (registered after) wins hit-test.
        // Use CLICK_AND_DRAG so both row selection and drag-to-reorder are supported.
        input_coordinator.register(
            row_id.as_str(),
            row_rect,
            uzor::input::Sense::CLICK_AND_DRAG,
        );
        result.item_rects.push((row_id.clone(), row_rect));
        result.watchlist_row_rects.push((i, row_rect));

        // Hover detection: row OR delete button hovered.
        let is_hovered = input_coordinator
            .is_hovered(&uzor::types::WidgetId::new(&row_id))
            || input_coordinator
                .is_hovered(&uzor::types::WidgetId::new(&del_id));
        let del_hovered = input_coordinator
            .is_hovered(&uzor::types::WidgetId::new(&del_id));

        // Virtual scrolling: skip draw calls for rows entirely outside the
        // visible viewport.  Hit-zone registrations above are kept so that
        // keyboard/click events still resolve correctly for all rows.
        let row_visible = current_y + data_row_h > viewport_top
            && current_y < viewport_bottom;

        if !row_visible {
            // Still register flag + delete zones for off-screen rows so that
            // click events that somehow reach them resolve correctly.
            let flag_id = format!("watchlist_flag_{}", i);
            let flag_rect = WidgetRect::new(rect.x, current_y, 10.0, data_row_h);
            input_coordinator.register(flag_id.as_str(), flag_rect, uzor::input::Sense::CLICK);
            let del_icon_x = rect.x + content_width - delete_icon_right_pad - delete_icon_size;
            let del_icon_y = current_y + (data_row_h - delete_icon_size) / 2.0;
            let del_rect = WidgetRect::new(del_icon_x, del_icon_y, delete_icon_size, delete_icon_size);
            input_coordinator.register(del_id.as_str(), del_rect, uzor::input::Sense::CLICK);
            result.delete_button_rects.push((del_id, del_rect));
            current_y += data_row_h;
            continue;
        }

        if is_hovered {
            ctx.set_fill_color(&theme.item_bg_hover);
            ctx.fill_rect(row_rect.x, row_rect.y, row_rect.width, row_rect.height);
        }

        // Color flag — 10px wide rect at the left edge, full row height.
        let color_flag = state.watchlist_manager.active_list()
            .and_then(|l| l.get_color_flag(&item.symbol, &item.exchange, &item.account_type))
            .unwrap_or("");
        // Flag click zone (first 10 px of row) — register BEFORE drawing so
        // is_hovered works on the same frame.
        let flag_id = format!("watchlist_flag_{}", i);
        let flag_rect = WidgetRect::new(rect.x, current_y, 10.0, data_row_h);
        input_coordinator.register(flag_id.as_str(), flag_rect, uzor::input::Sense::CLICK);
        let flag_hovered = input_coordinator.is_hovered(&uzor::types::WidgetId::new(&flag_id));

        {
            let flag_w = 10.0;
            let flag_x = rect.x;
            let flag_y = current_y;
            let flag_h = data_row_h;
            let has_color = !color_flag.is_empty();
            if has_color {
                ctx.set_fill_color(color_flag);
                ctx.fill_rect(flag_x, flag_y, flag_w, flag_h);
            } else if flag_hovered {
                ctx.set_fill_color(&theme.item_bg_hover);
                ctx.fill_rect(flag_x, flag_y, flag_w, flag_h);
            } else if is_hovered {
                ctx.set_fill_color(&theme.item_bg_active);
                ctx.fill_rect(flag_x, flag_y, flag_w, flag_h);
            }
        }

        // Delete (×) button — rightmost, visible only on hover.
        let del_icon_x = rect.x + content_width - delete_icon_right_pad - delete_icon_size;
        let del_icon_y = current_y + (data_row_h - delete_icon_size) / 2.0;
        let del_rect = WidgetRect::new(del_icon_x, del_icon_y, delete_icon_size, delete_icon_size);
        // Register delete button after row so it wins the hit-test.
        input_coordinator.register(del_id.as_str(), del_rect, uzor::input::Sense::CLICK);
        result.delete_button_rects.push((del_id.clone(), del_rect));

        // --- Column data, each cell clipped to its separator region ---

        // Symbol column (col 0) — always left-aligned.
        {
            let (clip_l, clip_r) = col_clip(0);
            let clip_w = (clip_r - clip_l).max(0.0);
            if clip_w >= 1.0 {
                let row_mid_y = current_y + data_row_h / 2.0;
                let symbol_x = clip_l + 2.0;
                let symbol_clip_w = (clip_r - symbol_x).max(0.0);

                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(&theme.item_text);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                let display_symbol = truncate_to_width(ctx, &item.symbol, symbol_clip_w);
                ctx.fill_text(&display_symbol, symbol_x, row_mid_y);
            }
        }

        // Exchange column — if enabled, comes right after Symbol.
        if col_cfg.show_exchange {
            let ex_col = 1; // exchange is col index 1 when enabled
            let (clip_l, clip_r) = col_clip(ex_col);
            let clip_w = (clip_r - clip_l).max(0.0);
            if clip_w >= 1.0 {
                let (align, tx) = col_text_x(ex_col, clip_l, clip_r);
                ctx.set_font("10px sans-serif");
                ctx.set_fill_color(&theme.item_text_muted);
                ctx.set_text_align(align);
                ctx.set_text_baseline(TextBaseline::Middle);
                let display_exchange = truncate_to_width(ctx, &item.exchange, clip_w);
                ctx.fill_text(&display_exchange, tx, current_y + data_row_h / 2.0);
            }
        }

        // Data columns — alignment determined by col_text_x (left/center/right).
        let mut col_idx = if col_cfg.show_exchange { 2 } else { 1 };
        ctx.set_font(data_font);
        ctx.set_text_baseline(TextBaseline::Middle);

        if col_cfg.show_account_type {
            let (clip_l, clip_r) = col_clip(col_idx);
            let clip_w = (clip_r - clip_l).max(0.0);
            if clip_w >= 1.0 {
                let type_label = if item.account_type.is_empty() { "S" } else { item.account_type.as_str() };
                let (align, tx) = col_text_x(col_idx, clip_l, clip_r);
                ctx.set_font("10px sans-serif");
                ctx.set_fill_color(&theme.item_text_muted);
                ctx.set_text_align(align);
                ctx.set_text_baseline(TextBaseline::Middle);
                let display_type = truncate_to_width(ctx, type_label, clip_w);
                ctx.fill_text(&display_type, tx, current_y + data_row_h / 2.0);
                ctx.set_font(data_font);
            }
            col_idx += 1;
        }

        if col_cfg.show_last_price {
            let (clip_l, clip_r) = col_clip(col_idx);
            let clip_w = (clip_r - clip_l).max(0.0);
            if clip_w >= 1.0 {
                let (align, tx) = col_text_x(col_idx, clip_l, clip_r);
                ctx.set_fill_color(&theme.item_text);
                ctx.set_text_align(align);
                let price_str = format_price(item.last_price);
                let display_price = truncate_to_width(ctx, &price_str, clip_w);
                ctx.fill_text(
                    &display_price,
                    tx,
                    current_y + data_row_h / 2.0,
                );
            }
            col_idx += 1;
        }

        if col_cfg.show_change_pct {
            let (clip_l, clip_r) = col_clip(col_idx);
            let clip_w = (clip_r - clip_l).max(0.0);
            if clip_w >= 1.0 {
                let change_color = if item.change_percent > 0.0 {
                    "#26a69a"
                } else if item.change_percent < 0.0 {
                    "#ef5350"
                } else {
                    theme.item_text_muted.as_str()
                };
                let (align, tx) = col_text_x(col_idx, clip_l, clip_r);
                ctx.set_fill_color(change_color);
                ctx.set_text_align(align);
                let chg_pct_str = format!("{:+.1}%", item.change_percent);
                let display_chg_pct = truncate_to_width(ctx, &chg_pct_str, clip_w);
                ctx.fill_text(
                    &display_chg_pct,
                    tx,
                    current_y + data_row_h / 2.0,
                );
            }
            col_idx += 1;
        }

        if col_cfg.show_change_abs {
            let (clip_l, clip_r) = col_clip(col_idx);
            let clip_w = (clip_r - clip_l).max(0.0);
            if clip_w >= 1.0 {
                let abs_change = item.last_price - (item.last_price / (1.0 + item.change_percent / 100.0));
                let change_color = if item.change_percent > 0.0 {
                    "#26a69a"
                } else if item.change_percent < 0.0 {
                    "#ef5350"
                } else {
                    theme.item_text_muted.as_str()
                };
                let (align, tx) = col_text_x(col_idx, clip_l, clip_r);
                ctx.set_fill_color(change_color);
                ctx.set_text_align(align);
                let abs_str = format_price(abs_change);
                let display_abs = truncate_to_width(ctx, &abs_str, clip_w);
                ctx.fill_text(
                    &display_abs,
                    tx,
                    current_y + data_row_h / 2.0,
                );
            }
            col_idx += 1;
        }

        if col_cfg.show_high_low {
            // High column.
            {
                let (clip_l, clip_r) = col_clip(col_idx);
                let clip_w = (clip_r - clip_l).max(0.0);
                if clip_w >= 1.0 {
                    let (align, tx) = col_text_x(col_idx, clip_l, clip_r);
                    ctx.set_fill_color(&theme.item_text_muted);
                    ctx.set_text_align(align);
                    let high_str = format_price_compact(item.high_24h);
                    let display_high = truncate_to_width(ctx, &high_str, clip_w);
                    ctx.fill_text(
                        &display_high,
                        tx,
                        current_y + data_row_h / 2.0,
                    );
                }
            }
            col_idx += 1;
            // Low column.
            {
                let (clip_l, clip_r) = col_clip(col_idx);
                let clip_w = (clip_r - clip_l).max(0.0);
                if clip_w >= 1.0 {
                    let (align, tx) = col_text_x(col_idx, clip_l, clip_r);
                    ctx.set_fill_color(&theme.item_text_muted);
                    ctx.set_text_align(align);
                    let low_str = format_price_compact(item.low_24h);
                    let display_low = truncate_to_width(ctx, &low_str, clip_w);
                    ctx.fill_text(
                        &display_low,
                        tx,
                        current_y + data_row_h / 2.0,
                    );
                }
            }
            col_idx += 1;
        }

        if col_cfg.show_volume {
            let (clip_l, clip_r) = col_clip(col_idx);
            let clip_w = (clip_r - clip_l).max(0.0);
            if clip_w >= 1.0 {
                let vol_str = format_volume(item.volume_24h);
                let (align, tx) = col_text_x(col_idx, clip_l, clip_r);
                ctx.set_fill_color(&theme.item_text_muted);
                ctx.set_text_align(align);
                let display_vol = truncate_to_width(ctx, &vol_str, clip_w);
                ctx.fill_text(
                    &display_vol,
                    tx,
                    current_y + data_row_h / 2.0,
                );
            }
        }

        // Delete (×) overlay — drawn AFTER column text so it's on top.
        if is_hovered {
            let bg_pad = 4.0;
            let bg_x = del_icon_x - bg_pad;
            let bg_y = del_icon_y - bg_pad;
            let bg_size = delete_icon_size + bg_pad * 2.0;
            // Solid bg matching row hover: body + hover tint.
            ctx.set_fill_color(&theme.dropdown_bg);
            ctx.fill_rect(bg_x, bg_y, bg_size, bg_size);
            ctx.set_fill_color(&theme.item_bg_hover);
            ctx.fill_rect(bg_x, bg_y, bg_size, bg_size);

            let del_color = if del_hovered { "#ff5252" } else { theme.item_text_muted.as_str() };
            draw_svg_icon(
                ctx,
                Icon::Close.svg(),
                del_icon_x,
                del_icon_y,
                delete_icon_size,
                delete_icon_size,
                del_color,
            );
        }

        current_y += data_row_h;
    }

    // -------------------------------------------------------------------------
    // Drag-reorder visual: drop indicator line and dragged-row highlight.
    // -------------------------------------------------------------------------
    if let Some(drag_idx) = state.watchlist_drag_index {
        // Highlight the dragged row with a semi-transparent accent overlay.
        // Data rows now start directly at content_y (header is outside the scroll clip).
        let drag_row_y = content_y + drag_idx as f64 * data_row_h;
        let drag_row_rect = WidgetRect::new(rect.x, drag_row_y, content_width, data_row_h);
        ctx.set_fill_color("#4a9eff33"); // accent blue, ~20% opacity
        ctx.fill_rect(drag_row_rect.x, drag_row_rect.y, drag_row_rect.width, drag_row_rect.height);

        // Drop indicator: a 2 px horizontal line at the drop position.
        if let Some(drop_idx) = state.watchlist_drop_index {
            let drop_line_y = content_y + drop_idx as f64 * data_row_h;
            ctx.set_stroke_color("#4a9eff");
            ctx.set_stroke_width(2.0);
            ctx.begin_path();
            ctx.move_to(area_left, drop_line_y);
            ctx.line_to(area_left + usable_w, drop_line_y);
            ctx.stroke();
        }
    }

    // -------------------------------------------------------------------------
    // Color flag picker popup overlay.
    // -------------------------------------------------------------------------
    if let Some((row_idx, popup_x, popup_y)) = state.watchlist_color_picker_open {
        let colors: &[&str] = &["#ef5350", "#f59e0b", "#22c55e", "#3b82f6", "#a855f7", "#ec4899", "#6b7280", ""];
        let swatch_size = 20.0;
        let swatch_gap = 4.0;
        let popup_pad = 6.0;
        let popup_w = colors.len() as f64 * (swatch_size + swatch_gap) - swatch_gap + popup_pad * 2.0;
        let popup_h = swatch_size + popup_pad * 2.0;

        // Background.
        ctx.set_fill_color(&theme.dropdown_bg);
        ctx.fill_rect(popup_x, popup_y, popup_w, popup_h);
        ctx.set_stroke_color(&theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(popup_x, popup_y);
        ctx.line_to(popup_x + popup_w, popup_y);
        ctx.line_to(popup_x + popup_w, popup_y + popup_h);
        ctx.line_to(popup_x, popup_y + popup_h);
        ctx.line_to(popup_x, popup_y);
        ctx.stroke();

        // Color swatches.
        for (ci, color) in colors.iter().enumerate() {
            let sx = popup_x + popup_pad + ci as f64 * (swatch_size + swatch_gap);
            let sy = popup_y + popup_pad;

            if color.is_empty() {
                // "None" — draw muted rect with an × in it.
                ctx.set_fill_color(&theme.item_text_muted);
                ctx.fill_rect(sx, sy, swatch_size, swatch_size);
                ctx.set_stroke_color(&theme.item_text);
                ctx.set_stroke_width(1.5);
                ctx.begin_path();
                ctx.move_to(sx + 4.0, sy + 4.0);
                ctx.line_to(sx + swatch_size - 4.0, sy + swatch_size - 4.0);
                ctx.move_to(sx + swatch_size - 4.0, sy + 4.0);
                ctx.line_to(sx + 4.0, sy + swatch_size - 4.0);
                ctx.stroke();
            } else {
                ctx.set_fill_color(color);
                ctx.fill_rect(sx, sy, swatch_size, swatch_size);
            }

            // Register click zone for each swatch.
            let swatch_id = format!("watchlist_color_{}_{}", row_idx, ci);
            let swatch_rect = WidgetRect::new(sx, sy, swatch_size, swatch_size);
            input_coordinator.register(swatch_id.as_str(), swatch_rect, uzor::input::Sense::CLICK);
        }
    }

    // Add bottom padding so the last row is fully visible when scrolled to the end.
    current_y += data_row_h;

    current_y - content_y
}

/// Format a volume number as a compact string (e.g. 1234567 → "1.23M").
fn format_volume(v: f64) -> String {
    if v >= 1_000_000_000.0 {
        format!("{:.2}B", v / 1_000_000_000.0)
    } else if v >= 1_000_000.0 {
        format!("{:.2}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.1}K", v / 1_000.0)
    } else {
        format!("{:.0}", v)
    }
}

/// Format a price compactly: integers for large values, fewer decimals for mid-range.
///
/// Examples: 42257.47 → "42257", 1.2345 → "1.235", 0.000123 → "0.00012"
fn format_price(v: f64) -> String {
    let abs = v.abs();
    if abs >= 10_000.0 {
        format!("{:.2}", v)
    } else if abs >= 1_000.0 {
        format!("{:.2}", v)
    } else if abs >= 100.0 {
        format!("{:.2}", v)
    } else if abs >= 1.0 {
        format!("{:.3}", v)
    } else if abs >= 0.01 {
        format!("{:.4}", v)
    } else {
        format!("{:.6}", v)
    }
}

/// Format a price for compact High/Low columns: integers for large values.
///
/// Examples: 95000.0 → "95000", 123.4 → "123.4", 1.2345 → "1.23", 0.000123 → "0.000123"
fn format_price_compact(v: f64) -> String {
    let abs = v.abs();
    if abs >= 1_000.0 {
        format!("{:.0}", v)
    } else if abs >= 100.0 {
        format!("{:.1}", v)
    } else if abs >= 1.0 {
        format!("{:.2}", v)
    } else if abs >= 0.01 {
        format!("{:.4}", v)
    } else {
        format!("{:.6}", v)
    }
}

// =============================================================================
// Alert items panel (clone of core's render_alert_items)
// =============================================================================

fn render_alert_items(
    ctx: &mut dyn RenderContext,
    rect: &LayoutRect,
    content_y: f64,
    content_width: f64,
    _scrollbar_width: f64,
    state: &SidebarState,
    theme: &ToolbarTheme,
    result: &mut RightSidebarResult,
    input_coordinator: &mut InputCoordinator,
) -> f64 {
    use alerts::AlertSource;
    use alerts::AlertStatus;

    if state.alert_items.is_empty() {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&theme.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(
            "No alerts configured",
            rect.x + content_width / 2.0,
            content_y,
        );
        ctx.fill_text(
            "Click + to add an alert",
            rect.x + content_width / 2.0,
            content_y + 20.0,
        );
        return 60.0;
    }

    // -------------------------------------------------------------------------
    // Group alerts by source category.
    // -------------------------------------------------------------------------
    let mut price_alerts: Vec<&alerts::AlertItem> = Vec::new();
    let mut drawing_alerts: Vec<&alerts::AlertItem> = Vec::new();
    let mut indicator_alerts: Vec<&alerts::AlertItem> = Vec::new();

    for item in &state.alert_items {
        match &item.source {
            AlertSource::Price { .. }        => price_alerts.push(item),
            AlertSource::Drawing { .. }      => drawing_alerts.push(item),
            AlertSource::Indicator { .. }
            | AlertSource::CrossingPair { .. }
            | AlertSource::Signal { .. }     => indicator_alerts.push(item),
        }
    }

    // Groups to render in display order: (widget_id_suffix, header_label, items).
    let groups: &[(&str, &str, &[&alerts::AlertItem])] = &[
        ("price",     "Price Alerts",     &price_alerts),
        ("drawing",   "Drawing Alerts",   &drawing_alerts),
        ("indicator", "Indicator Alerts", &indicator_alerts),
    ];

    let item_height   = 54.0;
    let section_h     = 24.0;
    let item_padding  = 8.0;
    let icon_size     = 14.0;
    let dot_r         = 3.0; // status-dot radius (drawn as small square for simplicity)
    let dot_size      = dot_r * 2.0;
    let mut current_y = content_y;

    for (grp_suffix, grp_label, items) in groups {
        if items.is_empty() {
            continue;
        }

        // -----------------------------------------------------------------
        // Section header (always expanded — ▼ triangle, visual only).
        // -----------------------------------------------------------------
        let hdr_id = format!("alert_grp:{}", grp_suffix);
        let hdr_rect = WidgetRect::new(rect.x, current_y, content_width, section_h);
        input_coordinator.register(hdr_id.as_str(), hdr_rect, uzor::input::Sense::CLICK);
        result.item_rects.push((hdr_id, hdr_rect));

        // Triangle indicator.
        ctx.set_font("bold 11px sans-serif");
        ctx.set_fill_color(&theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("\u{25BC}", rect.x + item_padding, current_y + section_h / 2.0);

        // Group label + count.
        let header_text = format!("{} ({})", grp_label, items.len());
        ctx.fill_text(&header_text, rect.x + item_padding + 14.0, current_y + section_h / 2.0);

        current_y += section_h;

        // -----------------------------------------------------------------
        // Alert rows within this group.
        // -----------------------------------------------------------------
        for item in items.iter() {
            let row_id = format!("alert_{}", item.id);
            let del_id = format!("alert_delete_{}", item.id);

            let item_rect = WidgetRect::new(
                rect.x + 4.0,
                current_y,
                content_width - 8.0,
                item_height,
            );

            // Register row FIRST so delete button (registered after) wins hit-test.
            result.item_rects.push((row_id.clone(), item_rect));
            input_coordinator.register(row_id.as_str(), item_rect, uzor::input::Sense::CLICK);

            let is_hovered = input_coordinator
                .is_hovered(&uzor::types::WidgetId::new(&row_id))
                || input_coordinator
                    .is_hovered(&uzor::types::WidgetId::new(&del_id));

            if is_hovered {
                ctx.set_fill_color(&theme.item_bg_hover);
                ctx.fill_rect(item_rect.x, item_rect.y, item_rect.width, item_rect.height);
            }

            // --- Left accent bar (4 px wide) ---
            let bar_color = match item.status {
                AlertStatus::Active    => theme.accent.as_str(),
                AlertStatus::Triggered => "#ff9800",
                AlertStatus::Paused    => theme.item_text_muted.as_str(),
                AlertStatus::Expired   => theme.item_text_muted.as_str(),
            };
            ctx.set_fill_color(bar_color);
            ctx.fill_rect(rect.x + item_padding, current_y + 4.0, 4.0, item_height - 8.0);

            // --- Line 1: source display name ---
            let display_name = if item.name.is_empty() {
                item.source_display()
            } else {
                item.name.clone()
            };
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(&theme.item_text);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.fill_text(&display_name, rect.x + item_padding + 12.0, current_y + 6.0);

            // --- Line 2: condition + price ---
            let condition_text = if item.condition.requires_second_price() {
                format!(
                    "{} {} - {}",
                    item.condition.display_name(),
                    format_price_smart(item.price),
                    format_price_smart(item.price2),
                )
            } else {
                format!(
                    "{} @ {}",
                    item.condition.display_name(),
                    format_price_smart(item.price),
                )
            };
            ctx.set_font("10px sans-serif");
            ctx.set_fill_color(&theme.item_text_muted);
            ctx.fill_text(&condition_text, rect.x + item_padding + 12.0, current_y + 23.0);

            // --- Line 3: symbol:exchange:timeframe ---
            let sym = item.symbol();
            let symbol_exchange_text = match (item.exchange.is_empty(), item.timeframe.is_empty()) {
                (true, true) => sym.to_string(),
                (false, true) => format!("{}:{}", sym, item.exchange),
                (true, false) => format!("{}:{}", sym, item.timeframe),
                (false, false) => format!("{}:{}:{}", sym, item.exchange, item.timeframe),
            };
            ctx.set_font("9px sans-serif");
            ctx.fill_text(&symbol_exchange_text, rect.x + item_padding + 12.0, current_y + 37.0);

            // --- Status dot (before delete button) ---
            let dot_color = match item.status {
                AlertStatus::Active    => "#4caf50",
                AlertStatus::Triggered => "#ff9800",
                AlertStatus::Paused    => "#9e9e9e",
                AlertStatus::Expired   => "#f44336",
            };
            let dot_x = rect.x + content_width - item_padding - icon_size - 8.0 - dot_size;
            let dot_y = current_y + (item_height - dot_size) / 2.0;
            ctx.set_fill_color(dot_color);
            ctx.fill_rect(dot_x, dot_y, dot_size, dot_size);

            // --- Delete X icon (rightmost) — registered AFTER row so it wins hit-test ---
            let icon_x = rect.x + content_width - item_padding - icon_size - 4.0;
            let icon_y = current_y + (item_height - icon_size) / 2.0;
            let delete_rect = WidgetRect::new(icon_x, icon_y, icon_size, icon_size);
            result.delete_button_rects.push((del_id.clone(), delete_rect));
            input_coordinator.register(del_id.as_str(), delete_rect, uzor::input::Sense::CLICK);

            let delete_color = if is_hovered {
                &theme.item_text_active
            } else {
                &theme.item_text_muted
            };
            draw_svg_icon(ctx, Icon::Close.svg(), icon_x, icon_y, icon_size, icon_size, delete_color);

            current_y += item_height;
        }

        // Small gap between groups.
        current_y += 8.0;
    }

    current_y - content_y
}

// =============================================================================
// Object tree panel (clone of core's render_object_tree_items)
// =============================================================================

fn render_object_tree_items(
    ctx: &mut dyn RenderContext,
    rect: &LayoutRect,
    content_y: f64,
    content_width: f64,
    _scrollbar_width: f64,
    state: &SidebarState,
    theme: &ToolbarTheme,
    result: &mut RightSidebarResult,
    input_coordinator: &mut InputCoordinator,
) -> f64 {
    if state.object_tree_items.is_empty() {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&theme.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(
            "No objects on chart",
            rect.x + content_width / 2.0,
            content_y,
        );
        ctx.fill_text(
            "Draw something to see it here",
            rect.x + content_width / 2.0,
            content_y + 20.0,
        );
        return 60.0;
    }

    let item_height = 32.0;
    let item_padding = 8.0;
    let key_header_height = 18.0;
    let icon_size = 14.0;
    let mut current_y = content_y;

    // Determine which sections are present, in display order:
    // "Group" first, then "Window", then items with no section.
    // We preserve insertion order within each section so the caller controls ordering.
    let section_order: &[Option<&str>] = &[
        Some("Group"),
        Some("Window"),
        None,
    ];

    for section_key in section_order {
        // Collect indices of items belonging to this section.
        let section_indices: Vec<usize> = state.object_tree_items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.section.as_deref() == *section_key)
            .map(|(idx, _)| idx)
            .collect();

        if section_indices.is_empty() {
            continue;
        }

        // Draw section header only when a section label is present.
        if let Some(label) = section_key {
            // Thin divider line above the section header (skip for the very first one at top).
            if current_y > content_y {
                ctx.set_fill_color(&theme.item_text_muted);
                // Draw as a thin rect (1px tall) spanning the full width.
                ctx.fill_rect(rect.x + item_padding, current_y + 3.0, content_width - item_padding * 2.0, 1.0);
                current_y += 8.0;
            }

            // Section header text — slightly larger than category headers, bold-ish.
            ctx.set_font("bold 11px sans-serif");
            ctx.set_fill_color(&theme.item_text);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            let text_x = rect.x + item_padding;
            ctx.fill_text(label, text_x, current_y + 10.0);
            current_y += 22.0;
        }

        // Determine distinct key labels in this section (preserving first-seen order).
        // Items are already sorted Active-first, so the first-seen key order reflects
        // which keys have active items first.
        let mut seen_keys: Vec<String> = Vec::new();
        for &idx in &section_indices {
            let key = state.object_tree_items[idx].key_label();
            if !seen_keys.contains(&key) {
                seen_keys.push(key);
            }
        }

        // Iterate key groups in first-seen order.
        for key_label in &seen_keys {
            // Indices belonging to this key within the section, preserving sorted order.
            let key_indices: Vec<usize> = section_indices
                .iter()
                .copied()
                .filter(|&idx| &state.object_tree_items[idx].key_label() == key_label)
                .collect();

            // Key subsection header — always shown so the user knows which
            // symbol/exchange/account context each group of objects belongs to.
            ctx.set_font("10px sans-serif");
            ctx.set_fill_color(&theme.item_text_muted);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            let key_indent = item_padding + 4.0;
            ctx.fill_text(key_label, rect.x + key_indent, current_y + key_header_height / 2.0);
            current_y += key_header_height;

            // Render items in sorted order (Active first, then Memory).
            // Track state transitions so we can insert a separator before the first
            // non-Active item, keeping the visual grouping clear without re-bucketing
            // by category (which would destroy the state ordering).
            let mut memory_separator_shown = false;

            for &idx in &key_indices {
                let item = &state.object_tree_items[idx];

                // Insert "— Memory —" separator on first non-Active item.
                if !item.is_interactive() && !memory_separator_shown {
                    memory_separator_shown = true;
                    // Thin rule + label.
                    let sep_indent = item_padding + 4.0;
                    let sep_y = current_y + 8.0;
                    ctx.set_fill_color(&theme.item_text_muted);
                    ctx.fill_rect(rect.x + sep_indent, sep_y, content_width - sep_indent * 2.0, 1.0);
                    ctx.set_font("10px sans-serif");
                    ctx.set_fill_color(&theme.item_text_muted);
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text("\u{2014} Memory \u{2014}", rect.x + sep_indent, current_y + 16.0);
                    current_y += 26.0;
                }

                // Use category-specific prefix so drawing id=1, indicator id=1, and
                // compare index=1 get distinct widget IDs ("drw_1", "ind_1", "cmp_1").
                let prefix = match item.category {
                    ObjectCategory::Indicator => "ind",
                    ObjectCategory::Compare => "cmp",
                    _ => "drw",
                };
                // Namespace by section so "Group" section drw_5 and "Window" section
                // drw_5 never produce the same widget ID string (fixes InputCoordinator
                // collision when a stash primitive and its group counterpart share the
                // same numeric ID from legacy autosave data).
                let section_tag = match (&item.item_state, item.section.as_deref()) {
                    (ObjectItemState::Memory, Some("Group")) => "grpmem",
                    (ObjectItemState::Memory, Some("Window")) => "winmem",
                    (ObjectItemState::Memory, _) => "mem",
                    (_, Some("Group")) => "grp",
                    (_, Some("Window")) => "win",
                    _ => "flt",
                };
                // Include key_label in prefix for Memory items so different keys
                // (e.g. SOLUSDT vs BTCUSDT) never collide on the same numeric ID.
                let key_slug = if item.item_state == ObjectItemState::Memory {
                    let kl = item.key_label().replace(':', "_");
                    format!("{}_{}", kl, prefix)
                } else {
                    prefix.to_string()
                };
                // widget_prefix replaces bare `prefix` for all widget ID construction.
                let widget_prefix = format!("{}_{}", section_tag, key_slug);
                let item_id = format!("{}_{}", widget_prefix, item.id);
                let is_drawing = item.category != ObjectCategory::Indicator
                    && item.category != ObjectCategory::Compare;

                let item_rect = WidgetRect::new(
                    rect.x + 4.0,
                    current_y,
                    content_width - 8.0,
                    item_height,
                );

                // Icons layout (right → left): Delete, Settings, Alert, Eye, Lock (drawings only).
                let icon_step = icon_size + 4.0;
                let icon_y = current_y + (item_height - icon_size) / 2.0;
                let del_x = rect.x + content_width - item_padding - icon_size;
                let set_x = del_x - icon_step;
                let alert_x = set_x - icon_step;
                let vis_x = alert_x - icon_step;
                let lock_x = vis_x - icon_step;

                if item.is_interactive() {
                    // --- Active item: full interactivity with all buttons ---

                    let del_id = format!("{}_delete_{}", widget_prefix, item.id);
                    let set_id = format!("{}_settings_{}", widget_prefix, item.id);
                    let alert_id = format!("{}_alert_{}", widget_prefix, item.id);
                    let vis_id = format!("{}_vis_{}", widget_prefix, item.id);

                    // Register row FIRST, then buttons (buttons win hit-test for clicks).
                    input_coordinator.register(item_id.as_str(), item_rect, uzor::input::Sense::CLICK);
                    let delete_rect = WidgetRect::new(del_x, icon_y, icon_size, icon_size);
                    input_coordinator.register(del_id.as_str(), delete_rect, uzor::input::Sense::CLICK);
                    let settings_rect = WidgetRect::new(set_x, icon_y, icon_size, icon_size);
                    input_coordinator.register(set_id.as_str(), settings_rect, uzor::input::Sense::CLICK);
                    let alert_rect = WidgetRect::new(alert_x, icon_y, icon_size, icon_size);
                    input_coordinator.register(alert_id.as_str(), alert_rect, uzor::input::Sense::CLICK);
                    let vis_rect = WidgetRect::new(vis_x, icon_y, icon_size, icon_size);
                    input_coordinator.register(vis_id.as_str(), vis_rect, uzor::input::Sense::CLICK);
                    // Lock button — drawings only.
                    let lock_id = if is_drawing {
                        Some(format!("{}_lock_{}", widget_prefix, item.id))
                    } else {
                        None
                    };
                    if let Some(ref lid) = lock_id {
                        let lock_rect = WidgetRect::new(lock_x, icon_y, icon_size, icon_size);
                        input_coordinator.register(lid.as_str(), lock_rect, uzor::input::Sense::CLICK);
                    }

                    // Row hover = row OR any of its buttons hovered.
                    let is_hovered = input_coordinator.is_hovered(&uzor::types::WidgetId::new(&item_id))
                        || input_coordinator.is_hovered(&uzor::types::WidgetId::new(&del_id))
                        || input_coordinator.is_hovered(&uzor::types::WidgetId::new(&set_id))
                        || input_coordinator.is_hovered(&uzor::types::WidgetId::new(&alert_id))
                        || input_coordinator.is_hovered(&uzor::types::WidgetId::new(&vis_id))
                        || lock_id.as_ref().is_some_and(|lid|
                            input_coordinator.is_hovered(&uzor::types::WidgetId::new(lid)));
                    let del_hovered = input_coordinator.is_hovered(&uzor::types::WidgetId::new(&del_id));

                    // Selection / hover background.
                    if item.selected {
                        ctx.set_fill_color(&format!("{}40", theme.accent));
                        ctx.fill_rect(item_rect.x, item_rect.y, item_rect.width, item_rect.height);
                    } else if is_hovered {
                        ctx.set_fill_color(&theme.item_bg_hover);
                        ctx.fill_rect(item_rect.x, item_rect.y, item_rect.width, item_rect.height);
                    }

                    // Colour swatch (16 × 16, left side).
                    if let Some(ref color) = item.color {
                        ctx.set_fill_color(color);
                        ctx.fill_rect(rect.x + item_padding, current_y + 8.0, 16.0, 16.0);
                    }

                    // Name label.
                    let name_x = if item.color.is_some() {
                        rect.x + item_padding + 24.0
                    } else {
                        rect.x + item_padding
                    };

                    // Row base color: primary for visible, half-opacity for hidden.
                    let row_color = if item.visible { &theme.item_text } else { &theme.item_text_hidden };

                    ctx.set_font("12px sans-serif");
                    ctx.set_fill_color(row_color);
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(&item.name, name_x, current_y + item_height / 2.0);

                    result.item_rects.push((item_id, item_rect));

                    // Delete (X) — red only when hovering the delete button itself.
                    result.delete_button_rects.push((del_id, delete_rect));
                    let delete_color = if del_hovered { "#ff5252" } else { row_color.as_str() };
                    draw_svg_icon(ctx, Icon::Close.svg(), del_x, icon_y, icon_size, icon_size, delete_color);

                    // Settings gear.
                    result.settings_button_rects.push((set_id, settings_rect));
                    draw_svg_icon(ctx, Icon::Settings.svg(), set_x, icon_y, icon_size, icon_size, row_color);

                    // Alert bell — accent colour when an alert is bound, row color otherwise.
                    let alert_bell_color = if item.has_alert {
                        theme.accent.as_str()
                    } else {
                        row_color.as_str()
                    };
                    draw_svg_icon(ctx, Icon::Alert.svg(), alert_x, icon_y, icon_size, icon_size, alert_bell_color);

                    // Eye / EyeOff (visibility toggle).
                    let vis_icon = if item.visible { Icon::Eye } else { Icon::EyeOff };
                    draw_svg_icon(ctx, vis_icon.svg(), vis_x, icon_y, icon_size, icon_size, row_color);

                    // Lock icon — drawings only.
                    if is_drawing {
                        let lock_icon = if item.locked { Icon::Lock } else { Icon::Unlock };
                        let lock_color = if item.locked { &theme.item_text } else { row_color.as_str() };
                        draw_svg_icon(ctx, lock_icon.svg(), lock_x, icon_y, icon_size, icon_size, lock_color);
                    }
                } else {
                    // --- Memory item: colour swatch + muted name + delete button only ---
                    // The row itself is NOT registered with input_coordinator — only the
                    // delete button is interactive, and only when the item is deletable.

                    // Colour swatch — shown so the user can still identify the object.
                    if let Some(ref color) = item.color {
                        ctx.set_fill_color(color);
                        ctx.fill_rect(rect.x + item_padding, current_y + 8.0, 16.0, 16.0);
                    }

                    // Name label — no suffix needed, Memory concept is shown by separator.
                    let name_x = if item.color.is_some() {
                        rect.x + item_padding + 24.0
                    } else {
                        rect.x + item_padding
                    };

                    ctx.set_font("12px sans-serif");
                    ctx.set_fill_color(&theme.item_text_muted);
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(&item.name, name_x, current_y + item_height / 2.0);

                    // Delete button — only for deletable items.
                    if item.is_deletable() {
                        let del_id = format!("{}_delete_{}", widget_prefix, item.id);
                        let del_hovered = input_coordinator
                            .is_hovered(&uzor::types::WidgetId::new(&del_id));
                        let delete_color = if del_hovered {
                            "#ff5252"
                        } else {
                            theme.item_text_muted.as_str()
                        };
                        draw_svg_icon(ctx, Icon::Close.svg(), del_x, icon_y, icon_size, icon_size, delete_color);
                        let delete_rect = WidgetRect::new(del_x, icon_y, icon_size, icon_size);
                        input_coordinator.register(del_id.as_str(), delete_rect, uzor::input::Sense::CLICK);
                        result.delete_button_rects.push((del_id, delete_rect));
                    }

                    // Non-interactive rows are still tracked in item_rects so the
                    // agent API can enumerate all objects in the tree.
                    result.item_rects.push((item_id, item_rect));
                }

                current_y += item_height;
            }

            current_y += 8.0; // gap between key groups
        }
    }

    current_y - content_y
}

// =============================================================================
// Indicator signals panel (clone of core's render_indicator_signals)
// =============================================================================

fn render_indicator_signals(
    ctx: &mut dyn RenderContext,
    rect: &LayoutRect,
    content_y: f64,
    content_width: f64,
    _scrollbar_width: f64,
    state: &SidebarState,
    theme: &ToolbarTheme,
    result: &mut RightSidebarResult,
    input_coordinator: &mut InputCoordinator,
) -> f64 {
    if state.indicator_signals.groups.is_empty() {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&theme.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(
            "No indicator signals",
            rect.x + content_width / 2.0,
            content_y,
        );
        ctx.fill_text(
            "Add indicators with signals enabled",
            rect.x + content_width / 2.0,
            content_y + 20.0,
        );
        return 60.0;
    }

    let group_header_height = 28.0;
    let signal_row_height = 24.0;
    let padding = 8.0;
    let mut current_y = content_y;

    // Total count header.
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(
        &format!("Total: {} signals", state.indicator_signals.total_count),
        rect.x + padding,
        current_y + 10.0,
    );
    current_y += 24.0;

    for group in &state.indicator_signals.groups {
        let is_collapsed = state.collapsed_signal_groups.contains(&group.instance_id);

        // Group header row.
        let header_rect = WidgetRect::new(
            rect.x + 4.0,
            current_y,
            content_width - 8.0,
            group_header_height,
        );

        let group_id = format!("signal_group_{}", group.instance_id);
        input_coordinator.register(
            group_id.as_str(),
            header_rect,
            uzor::input::Sense::CLICK,
        );
        let is_hovered = input_coordinator
            .is_hovered(&uzor::types::WidgetId::new(&group_id));

        if is_hovered {
            ctx.set_fill_color(&theme.item_bg_hover);
            ctx.fill_rect(header_rect.x, header_rect.y, header_rect.width, header_rect.height);
        }

        // Collapse arrow — SVG icon (ChevronRight = collapsed, ChevronDown = expanded).
        let arrow_icon = if is_collapsed { Icon::ChevronRight } else { Icon::ChevronDown };
        let icon_size = 10.0_f64;
        let icon_x = rect.x + padding;
        let icon_y = current_y + group_header_height / 2.0 - icon_size / 2.0;
        draw_svg_icon(ctx, arrow_icon.svg(), icon_x, icon_y, icon_size, icon_size, &theme.item_text);

        // Indicator name.
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&theme.item_text);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(
            &group.indicator_name,
            rect.x + padding + 16.0,
            current_y + group_header_height / 2.0,
        );

        // Signal count badge.
        let badge_text = format!("{}", group.signals.len());
        let badge_width = 24.0;
        let badge_height = 20.0;
        let badge_x = rect.x + content_width - badge_width - 4.0;
        let badge_y = current_y + (group_header_height - badge_height) / 2.0;

        ctx.set_fill_color(&format!("{}30", theme.accent));
        ctx.fill_rect(badge_x, badge_y, badge_width, badge_height);

        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(&theme.accent);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&badge_text, badge_x + badge_width / 2.0, badge_y + badge_height / 2.0);
        ctx.set_text_align(TextAlign::Left);

        result.item_rects.push((group_id, header_rect));
        current_y += group_header_height;

        // Signal rows (hidden when collapsed).
        if !is_collapsed {
            // Per-group scrollable viewport: cap display height at 8 rows but
            // allow scrolling through all signals via the group's scroll offset.
            let max_visible = 8usize;
            let viewport_rows = group.signals.len().min(max_visible);
            let viewport_height = viewport_rows as f64 * signal_row_height;
            let total_content_height = group.signals.len() as f64 * signal_row_height;

            // Retrieve and clamp the current scroll offset for this group.
            let raw_offset = state
                .signal_group_scroll
                .get(&group.instance_id)
                .map(|s| s.offset)
                .unwrap_or(0.0);
            let scroll_offset = raw_offset
                .clamp(0.0, (total_content_height - viewport_height).max(0.0));

            // Viewport rect for the group's signal area (used for clip and scroll routing).
            let group_viewport = WidgetRect::new(
                rect.x + 4.0,
                current_y,
                content_width - 8.0,
                viewport_height,
            );

            // Record the content rect so input.rs can route wheel events here.
            result.signal_group_content_rects.push((group.instance_id, group_viewport));

            // Subtle container background (drawn before clip, covers full viewport).
            ctx.set_fill_color(&format!("{}20", theme.item_bg_hover));
            ctx.fill_rect(group_viewport.x, group_viewport.y, group_viewport.width, group_viewport.height);

            // Clip to the viewport so rows scrolled out of view are not visible.
            ctx.save();
            ctx.begin_path();
            ctx.rect(group_viewport.x, group_viewport.y, group_viewport.width, group_viewport.height);
            ctx.clip();

            for (i, signal) in group.signals.iter().enumerate() {
                // Virtual Y position relative to unclipped content, shifted by scroll offset.
                let virtual_y = current_y + (i as f64 * signal_row_height) - scroll_offset;

                // Skip rows that are fully outside the viewport (optimisation, not correctness).
                if virtual_y + signal_row_height <= current_y {
                    continue;
                }
                if virtual_y >= current_y + viewport_height {
                    break;
                }

                let row_y = virtual_y;
                let signal_rect = WidgetRect::new(
                    rect.x + 4.0,
                    row_y,
                    content_width - 8.0,
                    signal_row_height,
                );
                let sig_id = format!("signal_{}_{}", group.instance_id, signal.bar_index);

                // Register for click detection.
                input_coordinator.register(sig_id.as_str(), signal_rect, uzor::input::Sense::CLICK);

                // Hover highlight (drawn before text so text renders on top).
                let is_row_hovered = input_coordinator
                    .is_hovered(&uzor::types::WidgetId::new(&sig_id));
                if is_row_hovered {
                    ctx.set_fill_color(&theme.item_bg_hover);
                    ctx.fill_rect(signal_rect.x, signal_rect.y, signal_rect.width, signal_rect.height);
                }

                result.item_rects.push((sig_id, signal_rect));

                // Direction icon with colour (ArrowUp = bullish, ArrowDown = bearish, Circle = neutral).
                let (dir_icon, dir_color): (Icon, &str) = match signal.direction {
                    1  => (Icon::ArrowUp,   "#26a69a"),
                    -1 => (Icon::ArrowDown, "#ef5350"),
                    _  => (Icon::Circle,    theme.item_text_muted.as_str()),
                };
                let dir_icon_size = 11.0_f64;
                let dir_icon_x = rect.x + padding + 4.0;
                let dir_icon_y = row_y + signal_row_height / 2.0 - dir_icon_size / 2.0;
                draw_svg_icon(ctx, dir_icon.svg(), dir_icon_x, dir_icon_y, dir_icon_size, dir_icon_size, dir_color);

                // Signal type.
                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(&theme.item_text);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(
                    &signal.signal_type,
                    rect.x + padding + 20.0,
                    row_y + signal_row_height / 2.0,
                );

                // Bar index (right-aligned, muted).
                ctx.set_text_align(TextAlign::Right);
                ctx.set_fill_color(&theme.item_text_muted);
                ctx.fill_text(
                    &format!("#{}", signal.bar_index),
                    rect.x + content_width - 4.0,
                    row_y + signal_row_height / 2.0,
                );

                // Price (right-aligned, before bar index).
                ctx.set_fill_color(&theme.item_text);
                ctx.fill_text(
                    &format!("{:.2}", signal.price),
                    rect.x + content_width - 50.0,
                    row_y + signal_row_height / 2.0,
                );
                ctx.set_text_align(TextAlign::Left);
            }

            // End clip region — rows beyond the viewport are now masked.
            ctx.restore();

            // Draw a thin scrollbar on the right edge when there are more rows than visible.
            if group.signals.len() > max_visible {
                let sb_width = 6.0;
                let sb_x = rect.x + content_width - sb_width;
                let sb_track_h = viewport_height;
                let handle_ratio = viewport_height / total_content_height;
                let handle_h = (sb_track_h * handle_ratio).max(16.0);
                let max_travel = sb_track_h - handle_h;
                let handle_y = current_y
                    + if total_content_height > viewport_height {
                        (scroll_offset / (total_content_height - viewport_height)) * max_travel
                    } else {
                        0.0
                    };

                // Track.
                ctx.set_fill_color(&format!("{}20", theme.separator));
                ctx.fill_rect(sb_x, current_y, sb_width, viewport_height);
                // Handle.
                ctx.set_fill_color(&format!("{}80", theme.separator));
                ctx.fill_rect(sb_x, handle_y, sb_width, handle_h);

                // Store scrollbar geometry for drag + track-click input routing.
                let track_rect = WidgetRect::new(sb_x, current_y, sb_width, sb_track_h);
                let handle_rect = WidgetRect::new(sb_x, handle_y, sb_width, handle_h);
                result.signal_group_scrollbar_rects.push((
                    group.instance_id,
                    handle_rect,
                    track_rect,
                    total_content_height,
                    viewport_height,
                ));
            }

            current_y += viewport_height;
            current_y += 4.0; // spacing after expanded signals
        }

        // Separator line between groups.
        ctx.set_fill_color(&format!("{}40", theme.separator));
        ctx.fill_rect(rect.x + padding, current_y, content_width - padding * 2.0, 1.0);
        current_y += 8.0;
    }

    current_y - content_y
}

// =============================================================================
// Price formatting helper (local — format_price_smart is not pub in zengeld_chart)
// =============================================================================

// =============================================================================
// Connectors panel
// =============================================================================

// SVG paths for capability indicators — no unicode characters.
const ICON_CHECK_SVG: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12l5 5L20 7"/></svg>"#;
const ICON_X_SVG: &str = r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6L6 18M6 6l12 12"/></svg>"#;

/// Render the Connectors sidebar panel.
///
/// Shows a list of exchange connectors as expandable cards.  Each card has a
/// toggle button (enabled/disabled), exchange name, chevron for expand/collapse,
/// and — when expanded — REST/WS health dots, capability flags, batch size, and
/// supported timeframes.
fn render_connectors_panel(
    ctx: &mut dyn RenderContext,
    rect: &LayoutRect,
    content_y: f64,
    content_width: f64,
    state: &SidebarState,
    theme: &ToolbarTheme,
    result: &mut RightSidebarResult,
    input_coordinator: &mut InputCoordinator,
) -> f64 {
    if state.connector_items.is_empty() {
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(&theme.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(
            "No connectors configured",
            rect.x + content_width / 2.0,
            content_y + 14.0,
        );
        return 60.0;
    }

    let pad          = 10.0;
    let row_h        = 44.0;   // taller collapsed card height
    let detail_row_h = 24.0;   // taller detail lines
    let toggle_r     = 7.0;    // bigger toggle dot radius
    let chev_size    = 14.0;   // bigger chevron
    let icon_size    = 12.0;   // check/x SVG icon size in detail rows
    let mut current_y = content_y;

    // Width of the toggle zone on the left.  Everything up to this x offset is
    // the clickable toggle area.  Everything to the right is expand/collapse.
    let toggle_zone_w = pad + toggle_r * 2.0 + 10.0 + 4.0; // ~38 px

    // ------------------------------------------------------------------
    // Helper: draw a filled circle using arc + fill.
    // ------------------------------------------------------------------
    let draw_circle = |ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64| {
        ctx.begin_path();
        ctx.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
        ctx.fill();
    };

    // -----------------------------------------------------------------
    // Group connectors into three collapsible sections.
    // -----------------------------------------------------------------
    use crate::types::ConnectorGroup;

    let groups = [
        (ConnectorGroup::NoApiKey,       "NO API KEY"),
        (ConnectorGroup::RequiresApiKey, "REQUIRES API KEY"),
        (ConnectorGroup::NonChartData,   "NON-CHART DATA"),
    ];

    // We need a flat index for alternating row backgrounds that is local
    // to each group (reset per group), so we track it separately.
    let mut _group_item_idx: usize = 0;

    for (group_variant, group_label) in &groups {
        // Collect items belonging to this group.
        let items_in_group: Vec<&crate::types::ConnectorStatusItem> = state
            .connector_items
            .iter()
            .filter(|item| item.group == *group_variant)
            .collect();

        if items_in_group.is_empty() {
            continue;
        }

        let is_collapsed = state
            .connector_group_collapsed
            .get(*group_label)
            .copied()
            .unwrap_or(false);

        // -----------------------------------------------------------------
        // Group header row.
        // -----------------------------------------------------------------
        let header_h = 28.0;
        let group_header_id = format!("connector_group:{}", group_label);
        let is_header_hovered = input_coordinator
            .is_hovered(&uzor::types::WidgetId::new(&group_header_id));

        // Header background — slightly darker than the panel background.
        if is_header_hovered {
            ctx.set_fill_color(&theme.item_bg_hover);
        } else {
            ctx.set_fill_color(&format!("{}18", theme.separator));
        }
        ctx.fill_rect(rect.x, current_y, content_width, header_h);

        // Bottom border line under the header.
        ctx.set_fill_color(&format!("{}40", theme.separator));
        ctx.fill_rect(rect.x, current_y + header_h - 1.0, content_width, 1.0);

        // Chevron (hand-drawn, same style as the metrics toggle chevron).
        let chev_cx = rect.x + pad + 5.0;
        let chev_cy = current_y + header_h / 2.0;
        ctx.set_stroke_color(&theme.item_text_muted);
        ctx.set_stroke_width(1.5);
        ctx.begin_path();
        if is_collapsed {
            // Right-pointing chevron.
            ctx.move_to(chev_cx, chev_cy - 4.0);
            ctx.line_to(chev_cx + 5.0, chev_cy);
            ctx.line_to(chev_cx, chev_cy + 4.0);
        } else {
            // Down-pointing chevron.
            ctx.move_to(chev_cx, chev_cy - 3.0);
            ctx.line_to(chev_cx + 5.0, chev_cy + 3.0);
            ctx.line_to(chev_cx + 10.0, chev_cy - 3.0);
        }
        ctx.stroke();

        // Group label text.
        let label_x = rect.x + pad + 18.0;
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(group_label, label_x, current_y + header_h / 2.0);

        // Item count (right-aligned, dimmer).
        let count_text = format!("({})", items_in_group.len());
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(&format!("{}80", theme.item_text_muted));
        ctx.set_text_align(TextAlign::Right);
        ctx.fill_text(&count_text, rect.x + content_width - pad, current_y + header_h / 2.0);
        ctx.set_text_align(TextAlign::Left);

        // Register header click widget.
        let header_rect = WidgetRect::new(rect.x, current_y, content_width, header_h);
        input_coordinator.register(
            group_header_id.as_str(),
            header_rect,
            uzor::input::Sense::CLICK,
        );
        result.item_rects.push((group_header_id, header_rect));

        current_y += header_h;

        // If collapsed, skip all items in this group.
        if is_collapsed {
            _group_item_idx = 0; // reset per group
            continue;
        }

        // -----------------------------------------------------------------
        // Render each item in this group (same logic as before).
        // -----------------------------------------------------------------
        _group_item_idx = 0;
        for connector in &items_in_group {
        let idx = _group_item_idx;
        _group_item_idx += 1;

        // -----------------------------------------------------------------
        // Collapsed row background (full-width).
        // NOTE: the row_id covers ONLY the expand zone (right of toggle).
        // -----------------------------------------------------------------
        let row_id = format!("connector_row:{}", connector.exchange_id);

        // Background uses the full row rect for visual hover state, but the
        // hit zones are split (toggle vs expand).
        let full_row_rect = WidgetRect::new(rect.x, current_y, content_width, row_h);

        let is_row_hovered = input_coordinator
            .is_hovered(&uzor::types::WidgetId::new(&row_id));
        let toggle_id = format!("connector_toggle:{}", connector.exchange_id);
        let is_toggle_hovered = input_coordinator
            .is_hovered(&uzor::types::WidgetId::new(&toggle_id));
        let is_any_hovered = is_row_hovered || is_toggle_hovered;

        if is_any_hovered {
            ctx.set_fill_color(&theme.item_bg_hover);
        } else if idx.is_multiple_of(2) {
            ctx.set_fill_color(&format!("{}08", theme.separator));
        } else {
            ctx.set_fill_color("transparent");
        }
        ctx.fill_rect(full_row_rect.x, full_row_rect.y, full_row_rect.width, full_row_rect.height);

        // -----------------------------------------------------------------
        // Toggle dot (left zone): bright green circle if enabled, dim gray if not.
        // -----------------------------------------------------------------
        let toggle_cx = rect.x + pad + toggle_r;
        let toggle_cy = current_y + row_h / 2.0;

        // Determine health-aware color for the toggle dot.
        let (ring_normal, ring_hover, fill_color) = if !connector.enabled {
            ("#6b7280", "#9ca3af", "#374151") // gray — disabled
        } else if connector.rest_healthy && connector.ws_connected {
            ("#22c55e", "#4ade80", "#22c55e") // green — fully healthy
        } else if connector.rest_healthy || connector.ws_connected {
            ("#f97316", "#fb923c", "#f97316") // orange — partial
        } else {
            ("#ef4444", "#f87171", "#ef4444") // red — neither healthy
        };

        // Outer ring for affordance — slightly brighter on hover.
        let ring_color = if is_toggle_hovered { ring_hover } else { ring_normal };
        ctx.set_stroke_color(ring_color);
        ctx.set_stroke_width(1.5);
        ctx.begin_path();
        ctx.arc(toggle_cx, toggle_cy, toggle_r + 2.0, 0.0, std::f64::consts::TAU);
        ctx.stroke();

        // Filled inner circle.
        ctx.set_fill_color(fill_color);
        draw_circle(ctx, toggle_cx, toggle_cy, toggle_r);

        // Subtle vertical divider between toggle zone and name zone.
        let divider_x = rect.x + toggle_zone_w;
        ctx.set_stroke_color(&format!("{}30", theme.separator));
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(divider_x, current_y + 6.0);
        ctx.line_to(divider_x, current_y + row_h - 6.0);
        ctx.stroke();

        // Register toggle hit zone (left zone only).
        let toggle_rect = WidgetRect::new(
            rect.x,
            current_y,
            toggle_zone_w,
            row_h,
        );
        input_coordinator.register(toggle_id.as_str(), toggle_rect, uzor::input::Sense::CLICK);
        result.item_rects.push((toggle_id, toggle_rect));

        // -----------------------------------------------------------------
        // Exchange display name (14px) — in the expand zone.
        // -----------------------------------------------------------------
        let name_x = rect.x + toggle_zone_w + 8.0;
        ctx.set_font("14px sans-serif");
        ctx.set_fill_color(if connector.enabled { &theme.item_text } else { &theme.item_text_muted });
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&connector.display_name, name_x, current_y + row_h / 2.0);

        // -----------------------------------------------------------------
        // Chevron (right side) — SVG icon, expand/collapse indicator.
        // -----------------------------------------------------------------
        let chev_x = rect.x + content_width - pad - chev_size;
        let chev_y = current_y + (row_h - chev_size) / 2.0;
        let chev_icon = if connector.expanded { Icon::ChevronDown } else { Icon::ChevronRight };
        draw_svg_icon(ctx, chev_icon.svg(), chev_x, chev_y, chev_size, chev_size, &theme.item_text_muted);

        // Register the expand zone (everything right of toggle zone).
        let expand_zone_rect = WidgetRect::new(
            rect.x + toggle_zone_w,
            current_y,
            content_width - toggle_zone_w,
            row_h,
        );
        input_coordinator.register(row_id.as_str(), expand_zone_rect, uzor::input::Sense::CLICK);
        result.item_rects.push((row_id, expand_zone_rect));

        current_y += row_h;

        // -----------------------------------------------------------------
        // Expanded detail section.
        // -----------------------------------------------------------------
        if connector.expanded {
            let indent = rect.x + pad * 2.0;
            let content_right = rect.x + content_width - pad;

            // Helper: draw a label/value pair on one line.
            let draw_detail = |ctx: &mut dyn RenderContext,
                               y: &mut f64,
                               label: &str,
                               value: &str,
                               value_color: &str| {
                ctx.set_font("12.5px sans-serif");
                ctx.set_fill_color("#6b7280"); // muted gray labels
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(label, indent, *y + detail_row_h / 2.0);
                ctx.set_fill_color(value_color);
                ctx.fill_text(value, indent + 100.0, *y + detail_row_h / 2.0);
                *y += detail_row_h;
            };

            // Helper: draw a section divider using native lines + text label.
            // Replaces the old unicode box-drawing character approach.
            let draw_section = |ctx: &mut dyn RenderContext, y: &mut f64, label: &str| {
                let line_y = *y + detail_row_h * 0.5;
                let label_pad = 5.0;

                // Set font first so measure_text uses the right metrics.
                ctx.set_font("11px sans-serif");
                let label_w = ctx.measure_text(label);
                let center_x = rect.x + content_width / 2.0;
                let text_x = center_x - label_w / 2.0;
                let text_end = center_x + label_w / 2.0;

                // Left line segment.
                ctx.set_stroke_color(&format!("{}50", theme.separator));
                ctx.set_stroke_width(0.5);
                ctx.begin_path();
                ctx.move_to(indent, line_y);
                ctx.line_to(text_x - label_pad, line_y);
                ctx.stroke();

                // Section label text.
                ctx.set_fill_color("#6b7280");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(label, text_x, line_y);

                // Right line segment.
                ctx.begin_path();
                ctx.move_to(text_end + label_pad, line_y);
                ctx.line_to(content_right, line_y);
                ctx.stroke();

                *y += detail_row_h;
            };

            // ---- Status line ----
            let rest_str = if connector.rest_status.is_empty() {
                if connector.rest_healthy { "active" } else { "offline" }
            } else {
                &connector.rest_status
            };
            let rest_color = match rest_str {
                "active" => "#22c55e",
                "error" | "inactive" => "#ef4444",
                "unknown" => "#f59e0b",
                _ => if connector.rest_healthy { "#22c55e" } else { "#ef4444" },
            };
            let ws_str = if connector.ws_status.is_empty() {
                if connector.ws_connected { "connected" } else { "disconnected" }
            } else {
                &connector.ws_status
            };
            let ws_color = match ws_str {
                "available" | "connected" => "#22c55e",
                "inactive" | "n/a" | "disconnected" => "#ef4444",
                "unknown" => "#f59e0b",
                _ => if connector.ws_connected { "#22c55e" } else { "#ef4444" },
            };

            // Inline status: "REST: active  WS: connected"
            ctx.set_font("12.5px sans-serif");
            ctx.set_fill_color("#6b7280");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("REST:", indent, current_y + detail_row_h / 2.0);
            ctx.set_fill_color(rest_color);
            let rest_val_x = indent + 40.0;
            ctx.fill_text(rest_str, rest_val_x, current_y + detail_row_h / 2.0);
            ctx.set_fill_color("#6b7280");
            let ws_label_x = rest_val_x + ctx.measure_text(rest_str) + 12.0;
            ctx.fill_text("WS:", ws_label_x, current_y + detail_row_h / 2.0);
            ctx.set_fill_color(ws_color);
            ctx.fill_text(ws_str, ws_label_x + 28.0, current_y + detail_row_h / 2.0);
            current_y += detail_row_h;

            // ---- Auth info ----
            let auth_val = if connector.auth_type.is_empty() {
                "unknown"
            } else {
                &connector.auth_type
            };
            let auth_color = if connector.requires_api_key { "#e2e8f0" } else { "#6b7280" };
            draw_detail(ctx, &mut current_y, "Auth:", auth_val, auth_color);

            let free_val = if connector.free_tier { "yes" } else { "no" };
            let free_color = if connector.free_tier { "#22c55e" } else { "#6b7280" };
            draw_detail(ctx, &mut current_y, "Free tier:", free_val, free_color);

            // ---- Rate limits ----
            let rate_str = if connector.rate_max > 0 {
                let win = connector.rate_window_seconds;
                let win_label = if win >= 60 {
                    format!("{}m", win / 60)
                } else if win > 0 {
                    format!("{}s", win)
                } else {
                    "?".to_string()
                };
                if let Some(w) = connector.weight_per_minute {
                    format!("{}/{} ({}w)", connector.rate_max, win_label, w)
                } else {
                    format!("{}/{}", connector.rate_max, win_label)
                }
            } else {
                "n/a".to_string()
            };
            let rate_color = if connector.rate_max > 0 {
                "#e2e8f0"
            } else {
                "#6b7280"
            };
            draw_detail(ctx, &mut current_y, "Rate limits:", &rate_str, rate_color);

            // ---- Data Capabilities section ----
            // Native line divider — no unicode box-drawing characters.
            draw_section(ctx, &mut current_y, "Data Capabilities");

            // Helper: draw capability item (label + SVG check/x icon).
            let draw_cap = |ctx: &mut dyn RenderContext, x: &mut f64, y: f64, label: &str, has: bool| {
                ctx.set_font("12.5px sans-serif");
                ctx.set_fill_color(if has { "#9ca3af" } else { "#4b5563" });
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(label, *x, y);
                let lw = ctx.measure_text(label);
                let icon_svg = if has { ICON_CHECK_SVG } else { ICON_X_SVG };
                let icon_color = if has { "#22c55e" } else { "#ef4444" };
                // Centre the icon vertically with the text.
                draw_svg_icon(ctx, icon_svg, *x + lw + 2.0, y - icon_size / 2.0, icon_size, icon_size, icon_color);
                *x += lw + icon_size + 6.0;
            };

            // REST capabilities row: klines, trades, orderbook.
            let row_mid_y = current_y + detail_row_h / 2.0;
            ctx.set_font("12.5px sans-serif");
            ctx.set_fill_color("#6b7280");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("REST:", indent, row_mid_y);
            let mut cap_x = indent + 44.0;
            draw_cap(ctx, &mut cap_x, row_mid_y, "klines",    connector.has_klines);
            draw_cap(ctx, &mut cap_x, row_mid_y, "trades",    connector.has_trades);
            draw_cap(ctx, &mut cap_x, row_mid_y, "orderbook", connector.has_orderbook);
            current_y += detail_row_h;

            // WS capabilities row.
            let row_mid_y = current_y + detail_row_h / 2.0;
            ctx.set_font("12.5px sans-serif");
            ctx.set_fill_color("#6b7280");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("WS:", indent, row_mid_y);
            let mut cap_x = indent + 44.0;
            draw_cap(ctx, &mut cap_x, row_mid_y, "klines",    connector.has_ws_klines);
            draw_cap(ctx, &mut cap_x, row_mid_y, "trades",    connector.has_ws_trades);
            draw_cap(ctx, &mut cap_x, row_mid_y, "orderbook", connector.has_ws_orderbook);
            current_y += detail_row_h;

            // Trading / Account / Positions row — SVG check/x icons.
            let row_mid_y = current_y + detail_row_h / 2.0;
            ctx.set_font("12.5px sans-serif");
            ctx.set_fill_color("#6b7280");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("Trading:", indent, row_mid_y);

            // Trading icon.
            {
                let icon_svg = if connector.has_trading { ICON_CHECK_SVG } else { ICON_X_SVG };
                let icon_color = if connector.has_trading { "#22c55e" } else { "#ef4444" };
                draw_svg_icon(ctx, icon_svg, indent + 58.0, row_mid_y - icon_size / 2.0, icon_size, icon_size, icon_color);
            }
            ctx.set_fill_color("#6b7280");
            ctx.fill_text("Acct:", indent + 78.0, row_mid_y);
            {
                let icon_svg = if connector.has_account { ICON_CHECK_SVG } else { ICON_X_SVG };
                let icon_color = if connector.has_account { "#22c55e" } else { "#ef4444" };
                draw_svg_icon(ctx, icon_svg, indent + 110.0, row_mid_y - icon_size / 2.0, icon_size, icon_size, icon_color);
            }
            ctx.set_fill_color("#6b7280");
            ctx.fill_text("Pos:", indent + 130.0, row_mid_y);
            {
                let icon_svg = if connector.has_positions { ICON_CHECK_SVG } else { ICON_X_SVG };
                let icon_color = if connector.has_positions { "#22c55e" } else { "#ef4444" };
                draw_svg_icon(ctx, icon_svg, indent + 158.0, row_mid_y - icon_size / 2.0, icon_size, icon_size, icon_color);
            }
            current_y += detail_row_h;

            // ---- Kline Config section ----
            // Native line divider — no unicode.
            draw_section(ctx, &mut current_y, "Kline Config");

            let batch_str = format!("{}", connector.kline_batch_size);
            let agg_str = if connector.has_aggregated_bars { "yes" } else { "no" };
            let agg_color = if connector.has_aggregated_bars { "#22c55e" } else { "#6b7280" };

            // Batch and aggregated on one line.
            ctx.set_font("12.5px sans-serif");
            ctx.set_fill_color("#6b7280");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("Batch:", indent, current_y + detail_row_h / 2.0);
            ctx.set_fill_color("#e2e8f0");
            ctx.fill_text(&batch_str, indent + 48.0, current_y + detail_row_h / 2.0);
            ctx.set_fill_color("#6b7280");
            ctx.fill_text("Aggregated:", indent + 90.0, current_y + detail_row_h / 2.0);
            ctx.set_fill_color(agg_color);
            ctx.fill_text(agg_str, indent + 172.0, current_y + detail_row_h / 2.0);
            current_y += detail_row_h;

            // Timeframes row(s).
            if !connector.supported_timeframes.is_empty() {
                let tf_str = connector.supported_timeframes.join(" ");
                ctx.set_font("12.5px sans-serif");
                ctx.set_fill_color("#6b7280");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("Timeframes:", indent, current_y + detail_row_h / 2.0);
                ctx.set_fill_color("#e2e8f0");
                ctx.fill_text(&tf_str, indent, current_y + detail_row_h / 2.0 + detail_row_h);
                current_y += detail_row_h * 2.0;
            }

            // ---- Metrics section (toggleable) ----
            {
                let metrics_row_y = current_y;
                let metrics_id = format!("connector_metrics:{}", connector.exchange_id);
                input_coordinator.register(
                    metrics_id.as_str(),
                    WidgetRect::new(rect.x + pad, metrics_row_y, content_width - pad * 2.0, detail_row_h),
                    uzor::input::Sense::CLICK,
                );

                // Chevron (right=collapsed, down=expanded).
                let chev_x = indent;
                let chev_cy = metrics_row_y + detail_row_h / 2.0;
                ctx.set_stroke_color("#6b7280");
                ctx.set_stroke_width(1.5);
                ctx.begin_path();
                if connector.show_metrics {
                    // Down chevron.
                    ctx.move_to(chev_x, chev_cy - 3.0);
                    ctx.line_to(chev_x + 5.0, chev_cy + 3.0);
                    ctx.line_to(chev_x + 10.0, chev_cy - 3.0);
                } else {
                    // Right chevron.
                    ctx.move_to(chev_x, chev_cy - 4.0);
                    ctx.line_to(chev_x + 5.0, chev_cy);
                    ctx.line_to(chev_x, chev_cy + 4.0);
                }
                ctx.stroke();

                ctx.set_font("11px sans-serif");
                ctx.set_fill_color("#6b7280");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("Metrics", chev_x + 14.0, chev_cy);
                current_y += detail_row_h;
            }

            if connector.show_metrics {
                let indent   = rect.x + pad + 8.0;
                let spark_w  = (content_width - pad * 2.0 - 16.0).max(60.0);
                // Slightly taller sparklines for better readability.
                let spark_h  = 34.0;
                let label_h  = 16.0;
                let gap      = 4.0;
                let hist     = &connector.metrics_history;

                ctx.set_font("11px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);

                // ---- WS connections (text only, purple) ----
                ctx.set_fill_color("#8b5cf6");
                ctx.fill_text(
                    &format!("WS active: {}", connector.ws_active_count),
                    indent,
                    current_y + detail_row_h / 2.0,
                );
                current_y += detail_row_h;

                // ---- HTTP req/s sparkline ----
                // Compute per-second deltas from consecutive snapshots.
                let rps_data: Vec<f64> = if hist.len() >= 2 {
                    hist.windows(2)
                        .map(|w| (w[1].http_requests.saturating_sub(w[0].http_requests)) as f64)
                        .collect()
                } else {
                    Vec::new()
                };
                let rps_max = rps_data.iter().cloned().fold(1.0_f64, f64::max);
                // Show the latest delta (current req/s), not the cumulative total.
                let current_rps = rps_data.last().copied().unwrap_or(0.0);
                // Sparkline FIRST, then label+value below.
                draw_sparkline(ctx, indent, current_y, spark_w, spark_h,
                    &rps_data, rps_max, "#3b82f640", "#3b82f6");
                current_y += spark_h;
                ctx.set_fill_color("#3b82f6");
                ctx.set_text_align(TextAlign::Left);
                ctx.fill_text("HTTP req/s", indent, current_y + label_h / 2.0);
                ctx.set_text_align(TextAlign::Right);
                ctx.fill_text(
                    &format!("{:.0}", current_rps),
                    indent + spark_w,
                    current_y + label_h / 2.0,
                );
                ctx.set_text_align(TextAlign::Left);
                current_y += label_h + gap;

                // ---- Latency sparkline ----
                let lat_data: Vec<f64> = hist.iter().map(|s| s.latency_ms as f64).collect();
                // Use window max as the sparkline ceiling for relative scale.
                let lat_max = lat_data.iter().cloned().fold(1.0_f64, f64::max);
                let current_lat = connector.last_latency_ms;
                // Sparkline FIRST, then label+value below.
                draw_sparkline(ctx, indent, current_y, spark_w, spark_h,
                    &lat_data, lat_max, "#06b6d440", "#06b6d4");
                current_y += spark_h;
                ctx.set_fill_color("#06b6d4");
                ctx.set_text_align(TextAlign::Left);
                ctx.fill_text("REST lat.", indent, current_y + label_h / 2.0);
                ctx.set_text_align(TextAlign::Right);
                ctx.fill_text(
                    &format!("{}ms", current_lat),
                    indent + spark_w,
                    current_y + label_h / 2.0,
                );
                // Show window max as a muted hint for relative scale context.
                ctx.set_text_align(TextAlign::Left);
                ctx.set_fill_color("#4b5563");
                ctx.fill_text(
                    &format!("max: {}ms", lat_max as u64),
                    indent + 52.0,
                    current_y + label_h / 2.0,
                );
                ctx.set_text_align(TextAlign::Left);
                current_y += label_h + gap;

                // ---- WS ping RTT ----
                {
                    let ws_rtt_data: Vec<f64> = hist.iter().map(|s| s.ws_ping_rtt_ms as f64).collect();
                    let ws_rtt_max = ws_rtt_data.iter().cloned().fold(1.0_f64, f64::max);
                    let current_ws_rtt = connector.ws_ping_rtt_ms;
                    draw_sparkline(ctx, indent, current_y, spark_w, spark_h,
                        &ws_rtt_data, ws_rtt_max, "#a855f740", "#a855f7");
                    current_y += spark_h;
                    ctx.set_fill_color("#a855f7");
                    ctx.set_text_align(TextAlign::Left);
                    ctx.fill_text("WS ping", indent, current_y + label_h / 2.0);
                    ctx.set_text_align(TextAlign::Right);
                    ctx.fill_text(
                        &format!("{}ms", current_ws_rtt),
                        indent + spark_w,
                        current_y + label_h / 2.0,
                    );
                    ctx.set_text_align(TextAlign::Left);
                    current_y += label_h + gap;
                }

                // ---- Rate usage: per-group bars or single sparkline ----
                if !connector.rate_groups.is_empty() {
                    // GroupRateLimiter connector — show one row per group.
                    // Colors rotate: yellow, cyan, green, orange, purple, …
                    const GROUP_COLORS: &[(&str, &str)] = &[
                        ("#eab308", "#eab30840"),  // yellow
                        ("#06b6d4", "#06b6d440"),  // cyan
                        ("#22c55e", "#22c55e40"),  // green
                        ("#f97316", "#f9731640"),  // orange
                        ("#a855f7", "#a855f740"),  // purple
                    ];
                    for (gi, (gname, gused, gmax)) in connector.rate_groups.iter().enumerate() {
                        if *gmax == 0 {
                            continue;
                        }
                        let (stroke, fill) = GROUP_COLORS[gi % GROUP_COLORS.len()];
                        let ratio = (*gused as f64 / *gmax as f64).min(1.0);
                        let bar_w = (spark_w * ratio).max(0.0);

                        // Background track.
                        ctx.set_fill_color("#0f172a");
                        ctx.fill_rect(indent, current_y, spark_w, spark_h / 2.0);

                        // Filled portion.
                        ctx.set_fill_color(fill);
                        ctx.fill_rect(indent, current_y, bar_w, spark_h / 2.0);

                        // Stroke border on top of fill.
                        ctx.set_stroke_color(stroke);
                        ctx.set_stroke_width(1.0);
                        ctx.begin_path();
                        ctx.move_to(indent, current_y);
                        ctx.line_to(indent + spark_w, current_y);
                        ctx.line_to(indent + spark_w, current_y + spark_h / 2.0);
                        ctx.line_to(indent, current_y + spark_h / 2.0);
                        ctx.close_path();
                        ctx.stroke();

                        current_y += spark_h / 2.0;

                        // Label row: "GROUP_NAME  used/max" left + right aligned.
                        ctx.set_font("11px sans-serif");
                        ctx.set_fill_color(stroke);
                        ctx.set_text_align(TextAlign::Left);
                        ctx.set_text_baseline(TextBaseline::Middle);
                        ctx.fill_text(gname, indent, current_y + label_h / 2.0);
                        ctx.set_text_align(TextAlign::Right);
                        ctx.fill_text(
                            &format!("{}/{}", gused, gmax),
                            indent + spark_w,
                            current_y + label_h / 2.0,
                        );
                        ctx.set_text_align(TextAlign::Left);
                        current_y += label_h + gap;
                    }
                } else if connector.rate_max > 0 {
                    // Single-limiter connector — sparkline only.
                    let rate_data: Vec<f64> = hist.iter()
                        .map(|s| s.rate_used as f64)
                        .collect();
                    let rate_max_val = connector.rate_max as f64;
                    let current_rate_used = connector.rate_used;
                    let ratio = current_rate_used as f64 / rate_max_val;
                    let (rate_fill, rate_stroke) = if ratio < 0.5 {
                        ("#22c55e40", "#22c55e")
                    } else if ratio < 0.8 {
                        ("#eab30840", "#eab308")
                    } else {
                        ("#ef444440", "#ef4444")
                    };

                    // Sparkline: 60s of rate_used history.
                    draw_sparkline(ctx, indent, current_y, spark_w, spark_h,
                        &rate_data, rate_max_val, rate_fill, rate_stroke);
                    current_y += spark_h;

                    // Label: "Rate/Ws  used/max"
                    let win_label = if connector.rate_window_seconds >= 60 {
                        format!("Rate/{}m", connector.rate_window_seconds / 60)
                    } else {
                        format!("Rate/{}s", connector.rate_window_seconds)
                    };
                    ctx.set_fill_color(rate_stroke);
                    ctx.set_text_align(TextAlign::Left);
                    ctx.fill_text(&win_label, indent, current_y + label_h / 2.0);
                    ctx.set_text_align(TextAlign::Right);
                    ctx.fill_text(
                        &format!("{}/{}", current_rate_used, connector.rate_max),
                        indent + spark_w,
                        current_y + label_h / 2.0,
                    );
                    ctx.set_text_align(TextAlign::Left);
                    current_y += label_h + gap;
                }

                // Bottom padding inside metrics section.
                current_y += 4.0;
            }

            // Small bottom margin between expanded cards.
            current_y += 8.0;
        }

        // Separator line below each card.
        ctx.set_fill_color(&format!("{}30", theme.separator));
        ctx.fill_rect(rect.x + pad, current_y, content_width - pad * 2.0, 1.0);
        current_y += 1.0;
        } // end for connector in &items_in_group

        // Small gap between groups.
        current_y += 4.0;
    } // end for (group_variant, group_label)

    // Bottom padding.
    current_y += row_h;
    current_y - content_y
}

// =============================================================================
// Sparkline helper
// =============================================================================

/// Draw a mini area/line chart (sparkline) inside a fixed bounding box.
///
/// # Arguments
/// - `ctx`          — render context
/// - `x`, `y`       — top-left corner of the graph area
/// - `w`, `h`       — width and height of the graph area
/// - `data`         — data points (left → right, newest on the right)
/// - `max_val`      — maximum expected value (used for Y-axis scaling)
/// - `fill_color`   — RGBA/hex fill colour for the area below the line
/// - `stroke_color` — colour for the top line
///
/// If `data` has fewer than 2 points, or `max_val <= 0`, the function draws
/// only the background rectangle and returns.
fn draw_sparkline(
    ctx: &mut dyn RenderContext,
    x: f64, y: f64, w: f64, h: f64,
    data: &[f64],
    max_val: f64,
    fill_color: &str,
    stroke_color: &str,
) {
    // Background.
    ctx.set_fill_color("#0f172a");
    ctx.fill_rect(x, y, w, h);

    if data.len() < 2 || max_val <= 0.0 {
        return;
    }

    let n = data.len();
    // Fixed step: full width = 60 slots (1 minute). Points fill from right.
    const MAX_SLOTS: f64 = 59.0;
    let step = w / MAX_SLOTS;
    // Offset so the latest point is always at the right edge.
    let x_start = x + w - (n - 1) as f64 * step;

    // Area fill: bottom-left of data → data points → bottom-right → close.
    ctx.begin_path();
    let first_x = x_start.max(x);
    ctx.move_to(first_x, y + h);
    for (i, val) in data.iter().enumerate() {
        let px = x_start + i as f64 * step;
        if px < x { continue; }
        let ratio = (val / max_val).clamp(0.0, 1.0);
        let py = y + h - ratio * h;
        ctx.line_to(px, py);
    }
    ctx.line_to(x_start + (n - 1) as f64 * step, y + h);
    ctx.close_path();
    ctx.set_fill_color(fill_color);
    ctx.fill();

    // Top stroke line: traces only the data points.
    ctx.begin_path();
    let mut started = false;
    for (i, val) in data.iter().enumerate() {
        let px = x_start + i as f64 * step;
        if px < x { continue; }
        let ratio = (val / max_val).clamp(0.0, 1.0);
        let py = y + h - ratio * h;
        if !started {
            ctx.move_to(px, py);
            started = true;
        } else {
            ctx.line_to(px, py);
        }
    }
    ctx.set_stroke_color(stroke_color);
    ctx.set_stroke_width(1.0);
    ctx.stroke();
}

// =============================================================================
// Price formatting
// =============================================================================

/// Format a price, trimming trailing zeros.
///
/// Examples: `180.10 → "180.1"`, `21323.00 → "21323"`, `0.001230 → "0.00123"`.
fn format_price_smart(price: f64) -> String {
    let precision = if price >= 10_000.0 {
        2
    } else if price >= 1_000.0 {
        2
    } else if price >= 100.0 {
        3
    } else if price >= 1.0 {
        4
    } else if price >= 0.01 {
        6
    } else {
        8
    };

    let formatted = format!("{:.prec$}", price, prec = precision);
    if formatted.contains('.') {
        let trimmed = formatted.trim_end_matches('0');
        let dot_pos = trimmed.find('.').unwrap();
        let decimals_len = trimmed.len() - dot_pos - 1;
        if decimals_len < 2 {
            format!("{:.2}", price)
        } else {
            trimmed.to_string()
        }
    } else {
        format!("{:.2}", price)
    }
}

// =============================================================================
// Performance panel
// =============================================================================

/// Renders the performance monitoring panel content.
/// Phase 3-new: render a free-slot sidebar body.
///
/// Lays out the slot's `DockingManager<FreeItem>` inside the content rect and
/// paints each leaf with a placeholder background + title. If the slot is
/// empty, draws a "Drag panels here" hint instead. Cross-container drag
/// between Main + Slot1..Slot4 and real leaf rendering land in Phase 4-new
/// together with the first real `FreeItem` variants.
fn render_slot_panel(
    ctx: &mut dyn RenderContext,
    rect: &LayoutRect,
    content_y: f64,
    content_width: f64,
    slot_idx: usize,
    state: &mut SidebarState,
    theme: &ToolbarTheme,
    result: &mut RightSidebarResult,
    input_coordinator: &mut InputCoordinator,
    free_item_renderer: &mut dyn FnMut(&crate::free_slot::FreeItem, (f32, f32, f32, f32), &mut dyn RenderContext),
) -> f64 {
    use uzor::panels::PanelRect as UzorPanelRect;

    const LEAF_HEADER_H: f32 = 22.0;

    let pad = 8.0;
    let inner_x = rect.x + pad;
    let inner_y = content_y + pad;
    let inner_w = (content_width - pad * 2.0).max(0.0);
    let inner_h = (rect.height - (inner_y - rect.y) - pad).max(0.0);

    // Record the slot body rect for cross-container drag hit testing.
    result.active_slot_body_rect = Some(WidgetRect::new(inner_x, inner_y, inner_w, inner_h));
    result.active_slot_index = Some(slot_idx);

    let mgr = state.slot_dockings[slot_idx].inner_mut();
    mgr.layout(UzorPanelRect {
        x: inner_x as f32,
        y: inner_y as f32,
        width: inner_w as f32,
        height: inner_h as f32,
    });

    // Collect (leaf_id, active_item, rect) so we can render after the mgr borrow.
    // `DockingTree::leaves()` gives each `Leaf<FreeItem>`; the active panel
    // for this leaf is `leaf.panels[leaf.active_tab]`.
    let leaves: Vec<(uzor::panels::LeafId, crate::free_slot::FreeItem, UzorPanelRect)> = {
        let rects = mgr.panel_rects().clone();
        let tree_leaves: Vec<(uzor::panels::LeafId, crate::free_slot::FreeItem)> = mgr
            .tree()
            .leaves()
            .into_iter()
            .filter_map(|leaf| {
                let active_panel = leaf.panels.get(leaf.active_tab).cloned()?;
                Some((leaf.id, active_panel))
            })
            .collect();
        tree_leaves
            .into_iter()
            .filter_map(|(id, item)| rects.get(&id).map(|r| (id, item, *r)))
            .collect()
    };

    if leaves.is_empty() {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&theme.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(
            "Drag panels here",
            inner_x + inner_w / 2.0,
            inner_y + inner_h / 2.0,
        );
        return inner_h;
    }

    let focused_free_leaf = state.focused_free_leaf;
    let hovered_free_leaf = state.hovered_free_leaf;

    for (leaf_id, item, r) in leaves {
        // Draw border/background frame for the leaf.
        ctx.set_fill_color(&theme.background);
        ctx.fill_rect(r.x as f64, r.y as f64, r.width as f64, r.height as f64);
        ctx.set_stroke_color(&theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rect(r.x as f64, r.y as f64, r.width as f64, r.height as f64);

        // ── Per-leaf header ──────────────────────────────────────────────────
        let header_x = r.x;
        let header_y = r.y;
        let header_w = r.width;
        let header_h = LEAF_HEADER_H;

        // Header background:
        //   focused   → bright (keyboard focus)
        //   hovered   → subtle highlight (mouse hover, not focused)
        //   otherwise → dark baseline
        let is_focused = focused_free_leaf == Some((slot_idx, leaf_id));
        let is_hovered = hovered_free_leaf == Some((slot_idx, leaf_id));
        let header_color = if is_focused {
            "#2d3748"
        } else if is_hovered {
            "#222837"
        } else {
            "#1a1f2a"
        };
        ctx.set_fill_color(header_color);
        ctx.fill_rect(header_x as f64, header_y as f64, header_w as f64, header_h as f64);

        // Header bottom separator.
        ctx.set_stroke_color(&theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(header_x as f64, (header_y + header_h) as f64);
        ctx.line_to((header_x + header_w) as f64, (header_y + header_h) as f64);
        ctx.stroke();

        // Panel title text.
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color("#c9d1d9");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(
            item.title(),
            (header_x + 6.0) as f64,
            (header_y + header_h / 2.0) as f64,
        );

        // Button layout (right-to-left from right edge):
        //   [×] close  |  [H] split-h  |  [V] split-v
        let btn_gap = 2.0_f32;
        let btn_w = 14.0_f32;
        let btn_h = 14.0_f32;
        let btn_y = header_y + (header_h - btn_h) / 2.0;

        // Close [×] button.
        let close_x = header_x + header_w - btn_w - 3.0;
        let close_y = btn_y;
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color("#8b949e");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(
            "\u{00d7}", // ×
            (close_x + btn_w / 2.0) as f64,
            (close_y + btn_h / 2.0) as f64,
        );

        // Split-H [H] button — to the left of close.
        let split_h_x = close_x - btn_gap - btn_w;
        let split_h_id = format!("slot:{}:leaf:{}:split_h", slot_idx, leaf_id.0);
        let is_split_h_hov = input_coordinator.is_hovered(
            &uzor::types::WidgetId::new(&split_h_id),
        );
        ctx.set_font("9px sans-serif");
        ctx.set_fill_color(if is_split_h_hov { "#58a6ff" } else { "#8b949e" });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(
            "H",
            (split_h_x + btn_w / 2.0) as f64,
            (btn_y + btn_h / 2.0) as f64,
        );

        // Split-V [V] button — to the left of split-h.
        let split_v_x = split_h_x - btn_gap - btn_w;
        let split_v_id = format!("slot:{}:leaf:{}:split_v", slot_idx, leaf_id.0);
        let is_split_v_hov = input_coordinator.is_hovered(
            &uzor::types::WidgetId::new(&split_v_id),
        );
        ctx.set_fill_color(if is_split_v_hov { "#58a6ff" } else { "#8b949e" });
        ctx.fill_text(
            "V",
            (split_v_x + btn_w / 2.0) as f64,
            (btn_y + btn_h / 2.0) as f64,
        );

        // Register split-v widget.
        let split_v_rect = WidgetRect::new(split_v_x as f64, btn_y as f64, btn_w as f64, btn_h as f64);
        input_coordinator.register(split_v_id.as_str(), split_v_rect, uzor::input::Sense::CLICK);
        result.item_rects.push((split_v_id, split_v_rect));

        // Register split-h widget.
        let split_h_rect = WidgetRect::new(split_h_x as f64, btn_y as f64, btn_w as f64, btn_h as f64);
        input_coordinator.register(split_h_id.as_str(), split_h_rect, uzor::input::Sense::CLICK);
        result.item_rects.push((split_h_id, split_h_rect));

        // Register header focus widget (full header minus all right-side buttons).
        let buttons_total_w = (btn_w + btn_gap) * 2.0 + btn_w + 3.0;
        let focus_id = format!("slot:{}:leaf:{}:focus", slot_idx, leaf_id.0);
        let focus_rect = WidgetRect::new(
            header_x as f64,
            header_y as f64,
            (header_w - buttons_total_w).max(0.0) as f64,
            header_h as f64,
        );
        input_coordinator.register(focus_id.as_str(), focus_rect, uzor::input::Sense::CLICK);
        result.item_rects.push((focus_id, focus_rect));

        // Register close button widget.
        let close_id = format!("slot:{}:leaf:{}:close", slot_idx, leaf_id.0);
        let close_rect = WidgetRect::new(
            close_x as f64,
            close_y as f64,
            btn_w as f64,
            btn_h as f64,
        );
        input_coordinator.register(close_id.as_str(), close_rect, uzor::input::Sense::CLICK);
        result.item_rects.push((close_id, close_rect));

        // Body rect: everything below the header.
        let body_y = r.y + LEAF_HEADER_H;
        let body_h = (r.height - LEAF_HEADER_H).max(0.0);

        // Delegate actual panel content to the caller-supplied renderer.
        free_item_renderer(&item, (r.x, body_y, r.width, body_h), ctx);
    }

    // Draw slot separators and register their hit zones.
    {
        use uzor::panels::SeparatorOrientation;
        let mgr = state.slot_dockings[slot_idx].inner();
        let sep_hit_w = 8.0_f64;
        for (sep_idx, sep) in mgr.separators().iter().enumerate() {
            let visual_thickness = sep.thickness_for_state() as f64;
            let sep_wid = format!("slot:{}:sep:{}", slot_idx, sep_idx);
            match sep.orientation {
                SeparatorOrientation::Vertical => {
                    let sep_x = sep.position as f64 - visual_thickness / 2.0;
                    let sep_y = sep.start as f64;
                    let sep_h = sep.length as f64;
                    ctx.set_fill_color(&theme.separator);
                    ctx.fill_rect(sep_x, sep_y, visual_thickness, sep_h);
                    let hit_rect = WidgetRect::new(
                        sep.position as f64 - sep_hit_w / 2.0,
                        sep_y,
                        sep_hit_w,
                        sep_h,
                    );
                    input_coordinator.register(sep_wid.as_str(), hit_rect, uzor::input::Sense::CLICK);
                    result.item_rects.push((sep_wid, hit_rect));
                }
                SeparatorOrientation::Horizontal => {
                    let sep_y = sep.position as f64 - visual_thickness / 2.0;
                    let sep_x = sep.start as f64;
                    let sep_w = sep.length as f64;
                    ctx.set_fill_color(&theme.separator);
                    ctx.fill_rect(sep_x, sep_y, sep_w, visual_thickness);
                    let hit_rect = WidgetRect::new(
                        sep_x,
                        sep.position as f64 - sep_hit_w / 2.0,
                        sep_w,
                        sep_hit_w,
                    );
                    input_coordinator.register(sep_wid.as_str(), hit_rect, uzor::input::Sense::CLICK);
                    result.item_rects.push((sep_wid, hit_rect));
                }
            }
        }
    }

    inner_h
}

fn render_performance_panel(
    ctx: &mut dyn RenderContext,
    rect: &LayoutRect,
    content_y: f64,
    content_width: f64,
    state: &SidebarState,
    theme: &ToolbarTheme,
    result: &mut RightSidebarResult,
    input_coordinator: &mut InputCoordinator,
) -> f64 {
    let perf = &state.performance_data;
    let pad = 12.0;
    let row_h = 22.0;
    let section_gap = 16.0;
    let mut y = content_y + pad;
    let x = rect.x + pad;
    let label_x = x;
    let value_x = rect.x + content_width - pad;
    let bar_max_w = content_width - pad * 2.0;

    // Helper: draw a section header with separator
    let draw_section = |ctx: &mut dyn RenderContext, y: &mut f64, title: &str| {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(title, label_x, *y + row_h / 2.0);
        *y += row_h;
        ctx.set_stroke_color(&theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(x, *y);
        ctx.line_to(rect.x + content_width - pad, *y);
        ctx.stroke();
        *y += 6.0;
    };

    // Helper: draw a metric row (label left, value right)
    let draw_row = |ctx: &mut dyn RenderContext, y: &mut f64, label: &str, value: &str, value_color: &str| {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, label_x, *y + row_h / 2.0);
        ctx.set_fill_color(value_color);
        ctx.set_text_align(TextAlign::Right);
        ctx.fill_text(value, value_x, *y + row_h / 2.0);
        *y += row_h;
    };

    // Helper: draw a clickable control row (label left, accented value right, hover highlight)
    let draw_control_row = |ctx: &mut dyn RenderContext,
                             input_coordinator: &mut InputCoordinator,
                             result: &mut RightSidebarResult,
                             y: &mut f64,
                             label: &str,
                             value: &str,
                             wid: &str,
                             accent: &str,
                             theme: &ToolbarTheme| {
        let row_rect = WidgetRect::new(rect.x, *y, content_width, row_h);
        let is_hovered = input_coordinator.is_hovered(&uzor::types::WidgetId::new(wid));
        if is_hovered {
            ctx.set_fill_color(&theme.item_bg_hover);
            ctx.fill_rect(row_rect.x, row_rect.y, row_rect.width, row_rect.height);
        }
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, label_x, *y + row_h / 2.0);
        ctx.set_fill_color(accent);
        ctx.set_text_align(TextAlign::Right);
        ctx.fill_text(value, value_x, *y + row_h / 2.0);
        input_coordinator.register(wid, row_rect, uzor::input::Sense::CLICK);
        result.item_rects.push((wid.to_string(), row_rect));
        *y += row_h;
    };

    let text_color = theme.item_text.clone();
    let accent = "#4a9eff";

    // =========================================================================
    // RENDERER section
    // =========================================================================
    draw_section(ctx, &mut y, "RENDERER");
    draw_control_row(
        ctx,
        input_coordinator,
        result,
        &mut y,
        "Backend",
        perf.render_backend.label(),
        "perf:backend",
        accent,
        theme,
    );

    // =========================================================================
    // FRAME TIMING section
    // =========================================================================
    y += section_gap;
    draw_section(ctx, &mut y, "FRAME TIMING");

    // FPS with color coding
    let fps_color = if perf.fps >= 55.0 { "#4ade80" } else if perf.fps >= 30.0 { "#fbbf24" } else { "#f87171" };
    draw_row(ctx, &mut y, "FPS", &format!("{:.0}", perf.fps), fps_color);
    draw_row(ctx, &mut y, "Frame Time", &format!("{:.1} ms", perf.frame_time_ms), &text_color);

    // Scene build / GPU render / GPU present
    if perf.scene_build_us > 0 || perf.gpu_render_us > 0 || perf.gpu_present_us > 0 {
        draw_row(ctx, &mut y, "Scene Build", &format!("{}μs", perf.scene_build_us), &text_color);
        draw_row(ctx, &mut y, "GPU Render", &format!("{}μs", perf.gpu_render_us), &text_color);
        draw_row(ctx, &mut y, "GPU Present", &format!("{}μs", perf.gpu_present_us), &text_color);
        let total_us = perf.scene_build_us + perf.gpu_render_us + perf.gpu_present_us;
        draw_row(ctx, &mut y, "Total", &format!("{}μs", total_us), accent);
    }

    // =========================================================================
    // SYSTEM section
    // =========================================================================
    y += section_gap;
    draw_section(ctx, &mut y, "SYSTEM");

    // System-wide CPU: average of per-core values (0-100%, reliable on Windows)
    let sys_cpu_color = if perf.cpu_usage < 30.0 { "#4ade80" } else if perf.cpu_usage < 70.0 { "#fbbf24" } else { "#f87171" };
    draw_row(ctx, &mut y, "System CPU", &format!("{:.1}%", perf.cpu_usage), sys_cpu_color);

    // App CPU: normalized to total machine capacity (comparable with System CPU).
    // Raw value (sum-of-threads) shown in parentheses for reference.
    let proc_norm_color = if perf.process_cpu_normalized < 15.0 { "#4ade80" } else if perf.process_cpu_normalized < 50.0 { "#fbbf24" } else { "#f87171" };
    draw_row(ctx, &mut y, "App CPU", &format!("{:.1}% ({:.0}% raw)", perf.process_cpu_normalized, perf.process_cpu), proc_norm_color);

    // Per-core bars (up to 16 cores)
    if !perf.per_core_cpu.is_empty() {
        let cores_to_show = perf.per_core_cpu.len().min(16);
        let bar_h = 5.0;
        let bar_gap = 2.0;
        let bar_row_h = bar_h + bar_gap;
        let cores_per_row = 4usize;
        let num_rows = cores_to_show.div_ceil(cores_per_row);
        let cell_w = bar_max_w / cores_per_row as f64;

        y += 2.0;
        for row in 0..num_rows {
            for col in 0..cores_per_row {
                let idx = row * cores_per_row + col;
                if idx >= cores_to_show { break; }
                let usage = perf.per_core_cpu[idx].clamp(0.0, 100.0) as f64;
                let bx = label_x + col as f64 * cell_w;
                let fill_w = (cell_w - bar_gap) * usage / 100.0;
                // Background track
                ctx.set_fill_color(&theme.separator);
                ctx.fill_rect(bx, y, cell_w - bar_gap, bar_h);
                // Filled portion
                if fill_w > 0.0 {
                    let bar_color = if usage < 30.0 { "#4ade80" } else if usage < 70.0 { "#fbbf24" } else { "#f87171" };
                    ctx.set_fill_color(bar_color);
                    ctx.fill_rect(bx, y, fill_w, bar_h);
                }
            }
            y += bar_row_h;
        }
        y += 4.0;
    }

    // RAM
    let ram_str = if perf.ram_total_mb > 0.0 {
        format!("{:.0} / {:.0} MB", perf.ram_mb, perf.ram_total_mb)
    } else {
        format!("{:.0} MB", perf.ram_mb)
    };
    draw_row(ctx, &mut y, "RAM", &ram_str, &text_color);

    // GPU name + VRAM
    if !perf.gpu_name.is_empty() {
        let name = if perf.gpu_name.len() > 28 { &perf.gpu_name[..28] } else { &perf.gpu_name };
        draw_row(ctx, &mut y, "GPU", name, &text_color);
        if !perf.gpu_driver.is_empty() {
            ctx.set_font("10px sans-serif");
            ctx.set_fill_color(&theme.item_text_muted);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(&perf.gpu_driver, label_x, y + row_h / 2.0);
            y += row_h;
        }
        if perf.gpu_mem_mb > 0.0 {
            draw_row(ctx, &mut y, "GPU VRAM", &format!("{:.0} MB", perf.gpu_mem_mb), &text_color);
        }
    }

    draw_row(ctx, &mut y, "Windows", &format!("{}", perf.window_count), &text_color);
    draw_row(ctx, &mut y, "Total Bars", &format!("{}", perf.total_bars), &text_color);
    draw_row(ctx, &mut y, "WS Connections", &format!("{}", perf.ws_connections), &text_color);
    draw_row(ctx, &mut y, "Connectors", &format!("{}", perf.active_connectors), &text_color);
    if perf.lag_events > 0 {
        draw_row(ctx, &mut y, "Lag Events", &format!("{}", perf.lag_events), "#f87171");
    }

    // =========================================================================
    // PROFILING section — internal CPU timing breakdown
    // =========================================================================
    y += section_gap;
    draw_section(ctx, &mut y, "PROFILING");

    // Tick total
    draw_row(ctx, &mut y, "Tick", &format!("{}μs", perf.tick_us), &text_color);

    // Events sub-row (indented label)
    draw_row(ctx, &mut y, "  Events", &format!("{}μs", perf.event_process_us), &text_color);

    // Auto Scale sub-row
    draw_row(ctx, &mut y, "  Auto Scale", &format!("{}μs", perf.auto_scale_us), &text_color);

    // Moving Avg sub-row
    draw_row(ctx, &mut y, "  Moving Avg", &format!("{}μs", perf.moving_avg_us), &text_color);

    // Indicator recalc row — color by incremental/full split
    let indicator_color = if perf.indicator_recalc_count == 0 {
        &text_color as &str
    } else if perf.indicator_full_count == 0 {
        "#4ade80" // all incremental — green
    } else if perf.indicator_incremental_count > 0 {
        "#fbbf24" // mixed — yellow
    } else {
        "#f87171" // all full recalc — red
    };
    draw_row(ctx, &mut y, "  Indicators", &format!("{}μs", perf.indicator_recalc_us), indicator_color);

    // Incremental / full counts
    if perf.indicator_recalc_count > 0 {
        draw_row(ctx, &mut y, "    Instances", &format!("{}", perf.indicator_recalc_count), &text_color);
        draw_row(ctx, &mut y, "    Incremental", &format!("{}", perf.indicator_incremental_count), "#4ade80");
        if perf.indicator_full_count > 0 {
            draw_row(ctx, &mut y, "    Full Recalc", &format!("{}", perf.indicator_full_count), "#f87171");
        }
    }

    // =========================================================================
    // SETTINGS section (clickable controls)
    // =========================================================================
    y += section_gap;
    draw_section(ctx, &mut y, "SETTINGS");

    // FPS Limit — cycles 0 / 30 / 60 / 120 / 240
    draw_control_row(
        ctx,
        input_coordinator,
        result,
        &mut y,
        "FPS Limit",
        &(if perf.fps_limit == 0 { "Unlimited".to_string() } else { format!("{}", perf.fps_limit) }),
        "perf:fps_limit",
        accent,
        theme,
    );

    // MSAA — cycles 0 / 8 / 16
    draw_control_row(
        ctx,
        input_coordinator,
        result,
        &mut y,
        "MSAA",
        &(if perf.msaa_samples == 0 { "Off".to_string() } else { format!("{}x", perf.msaa_samples) }),
        "perf:msaa",
        accent,
        theme,
    );

    // Recalc Mode
    draw_control_row(
        ctx,
        input_coordinator,
        result,
        &mut y,
        "Recalc Mode",
        &perf.recalc_mode.clone(),
        "perf:recalc_mode",
        accent,
        theme,
    );

    // Frame Log toggle (ON = accent green, OFF = muted)
    {
        let log_value_color = if perf.perf_log_enabled { "#4ade80" } else { &theme.item_text_muted };
        let log_value_text = if perf.perf_log_enabled { "ON" } else { "OFF" };
        draw_control_row(
            ctx,
            input_coordinator,
            result,
            &mut y,
            "Frame Log",
            log_value_text,
            "perf:log_toggle",
            log_value_color,
            theme,
        );
    }

    y + pad - content_y
}

// =============================================================================
// Agents panel — split-grid layout
// =============================================================================

/// Renders the AI agents panel with a split-grid docking layout.
///
/// Layout:
/// ```
/// ┌─────────────────────────────────────────────────────────────────┐
/// │ [PTY][Chat]  [Claude] [Codex] [Gemini] [OpenCode]  ← row 1 (28px)
/// ├─────────────────────────────────────────────────────────────────┤
/// │ [Layout ▾]  [×]                                    ← row 2 (28px)
/// ├─────────────────────────────────────────────────────────────────┤
/// │                                                                 │
/// │  docking grid — one pane per leaf                               │
/// │                                                                 │
/// └─────────────────────────────────────────────────────────────────┘
/// ```
fn render_agents_panel(
    ctx: &mut dyn RenderContext,
    rect: &LayoutRect,
    content_y: f64,
    content_width: f64,
    state: &mut SidebarState,
    theme: &ToolbarTheme,
    result: &mut RightSidebarResult,
    input_coordinator: &mut InputCoordinator,
) -> f64 {
    let pad = 8.0;
    let ctrl_h = 28.0;
    let btn_h = 22.0;
    let gap = 4.0;
    let x = rect.x + pad;
    let inner_w = content_width - pad * 2.0;
    let mut y = content_y + pad;

    // ── Row 1: Mode toggle [PTY][Chat] + CLI spawn buttons ────────────────────
    {
        let is_pty  = state.agent_spawn_mode == gate4agent::InstanceMode::Pty;

        // [PTY] segment
        let toggle_w = 36.0;
        let pty_rect = WidgetRect::new(x, y + (ctrl_h - btn_h) / 2.0, toggle_w, btn_h);
        let pty_hov  = !is_pty && input_coordinator.is_hovered(&uzor::types::WidgetId::new("agent:mode:pty"));
        ctx.set_fill_color(if is_pty { &theme.accent } else if pty_hov { &theme.item_bg_hover } else { &theme.button_bg });
        ctx.fill_rounded_rect(pty_rect.x, pty_rect.y, pty_rect.width, pty_rect.height, 3.0);
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(if is_pty { &theme.item_text_active } else { &theme.item_text_muted });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("PTY", pty_rect.x + toggle_w / 2.0, pty_rect.y + btn_h / 2.0);
        if !is_pty {
            input_coordinator.register("agent:mode:pty", pty_rect, uzor::input::Sense::CLICK);
        }
        result.item_rects.push(("agent:mode:pty".to_string(), pty_rect));

        // [Chat] segment
        let chat_seg_x = x + toggle_w + 2.0;
        let chat_seg_rect = WidgetRect::new(chat_seg_x, y + (ctrl_h - btn_h) / 2.0, toggle_w, btn_h);
        let chat_hov = is_pty && input_coordinator.is_hovered(&uzor::types::WidgetId::new("agent:mode:chat"));
        ctx.set_fill_color(if !is_pty { &theme.accent } else if chat_hov { &theme.item_bg_hover } else { &theme.button_bg });
        ctx.fill_rounded_rect(chat_seg_rect.x, chat_seg_rect.y, chat_seg_rect.width, chat_seg_rect.height, 3.0);
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(if !is_pty { &theme.item_text_active } else { &theme.item_text_muted });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Chat", chat_seg_rect.x + toggle_w / 2.0, chat_seg_rect.y + btn_h / 2.0);
        if is_pty {
            input_coordinator.register("agent:mode:chat", chat_seg_rect, uzor::input::Sense::CLICK);
        }
        result.item_rects.push(("agent:mode:chat".to_string(), chat_seg_rect));

        // CLI spawn buttons: [Claude] [Codex] [Gemini] [OpenCode]
        struct CliBtn { id: &'static str, label: &'static str }
        let cli_btns = [
            CliBtn { id: "agent:spawn:claude",   label: "Claude"   },
            CliBtn { id: "agent:spawn:codex",     label: "Codex"    },
            CliBtn { id: "agent:spawn:gemini",    label: "Gemini"   },
            CliBtn { id: "agent:spawn:opencode",  label: "OpenCode" },
        ];
        let spawn_area_x = chat_seg_x + toggle_w + gap * 2.0;
        let spawn_area_w = inner_w - (spawn_area_x - x);
        let spawn_btn_w  = ((spawn_area_w - gap * 3.0) / 4.0).max(1.0);

        for (i, btn) in cli_btns.iter().enumerate() {
            let bx = spawn_area_x + i as f64 * (spawn_btn_w + gap);
            let btn_rect = WidgetRect::new(bx, y + (ctrl_h - btn_h) / 2.0, spawn_btn_w, btn_h);
            let hov = input_coordinator.is_hovered(&uzor::types::WidgetId::new(btn.id));
            ctx.set_fill_color(if hov { &theme.button_bg_hover } else { &theme.button_bg });
            ctx.fill_rounded_rect(btn_rect.x, btn_rect.y, btn_rect.width, btn_rect.height, 3.0);
            ctx.set_font("10px sans-serif");
            ctx.set_fill_color(&theme.item_text);
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(btn.label, btn_rect.x + spawn_btn_w / 2.0, btn_rect.y + btn_h / 2.0);
            input_coordinator.register(btn.id, btn_rect, uzor::input::Sense::CLICK);
            result.item_rects.push((btn.id.to_string(), btn_rect));
        }

        y += ctrl_h + gap;
    }

    // ── Row 2: [H][V][R] spawn layout  [expand/collapse] [reset] [×] close ──
    {
        use crate::state::AgentSpawnLayout;
        let has_focused = state.focused_agent_leaf.is_some();
        let multi_leaf  = state.agent_leaves.len() > 1;
        let toggle_w    = 28.0;
        let btn_w       = 22.0;
        let icon_pad    = 4.0;
        let mut cur_x   = x;

        // [H] split-direction toggle
        let is_h = state.agent_spawn_layout == AgentSpawnLayout::SplitH;
        let h_rect = WidgetRect::new(cur_x, y + (ctrl_h - btn_h) / 2.0, toggle_w, btn_h);
        let h_hov  = !is_h && input_coordinator.is_hovered(&uzor::types::WidgetId::new("agent:split:h"));
        ctx.set_fill_color(if is_h { &theme.accent } else if h_hov { &theme.item_bg_hover } else { &theme.button_bg });
        ctx.fill_rounded_rect(h_rect.x, h_rect.y, h_rect.width, h_rect.height, 3.0);
        draw_svg_icon(ctx, uzor::render::icons::ui::ICON_LAYOUT_SPLIT_H,
            h_rect.x + icon_pad, h_rect.y + icon_pad,
            h_rect.width - icon_pad * 2.0, h_rect.height - icon_pad * 2.0,
            if is_h { &theme.item_text_active } else { &theme.item_text_muted });
        if !is_h {
            input_coordinator.register("agent:split:h", h_rect, uzor::input::Sense::CLICK);
        }
        result.item_rects.push(("agent:split:h".to_string(), h_rect));
        cur_x += toggle_w + 2.0;

        // [V] split-direction toggle
        let is_v = state.agent_spawn_layout == AgentSpawnLayout::SplitV;
        let v_rect = WidgetRect::new(cur_x, y + (ctrl_h - btn_h) / 2.0, toggle_w, btn_h);
        let v_hov  = !is_v && input_coordinator.is_hovered(&uzor::types::WidgetId::new("agent:split:v"));
        ctx.set_fill_color(if is_v { &theme.accent } else if v_hov { &theme.item_bg_hover } else { &theme.button_bg });
        ctx.fill_rounded_rect(v_rect.x, v_rect.y, v_rect.width, v_rect.height, 3.0);
        draw_svg_icon(ctx, uzor::render::icons::ui::ICON_LAYOUT_SPLIT_V,
            v_rect.x + icon_pad, v_rect.y + icon_pad,
            v_rect.width - icon_pad * 2.0, v_rect.height - icon_pad * 2.0,
            if is_v { &theme.item_text_active } else { &theme.item_text_muted });
        if !is_v {
            input_coordinator.register("agent:split:v", v_rect, uzor::input::Sense::CLICK);
        }
        result.item_rects.push(("agent:split:v".to_string(), v_rect));
        cur_x += toggle_w + 2.0;

        // [R] replace toggle
        let is_r = state.agent_spawn_layout == AgentSpawnLayout::Replace;
        let r_rect = WidgetRect::new(cur_x, y + (ctrl_h - btn_h) / 2.0, toggle_w, btn_h);
        let r_hov  = !is_r && input_coordinator.is_hovered(&uzor::types::WidgetId::new("agent:split:replace"));
        ctx.set_fill_color(if is_r { &theme.accent } else if r_hov { &theme.item_bg_hover } else { &theme.button_bg });
        ctx.fill_rounded_rect(r_rect.x, r_rect.y, r_rect.width, r_rect.height, 3.0);
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(if is_r { &theme.item_text_active } else { &theme.item_text_muted });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("R", r_rect.x + toggle_w / 2.0, r_rect.y + btn_h / 2.0);
        if !is_r {
            input_coordinator.register("agent:split:replace", r_rect, uzor::input::Sense::CLICK);
        }
        result.item_rects.push(("agent:split:replace".to_string(), r_rect));
        cur_x += toggle_w + gap * 2.0;

        // [expand/collapse] toggle
        let any_hidden = has_focused && multi_leaf && state.agent_leaves.keys().any(|&lid| {
            state.agent_docking.inner().tree().leaf(lid).map_or(false, |l| l.hidden)
        });
        let expand_en = has_focused && multi_leaf;
        let exp_rect  = WidgetRect::new(cur_x, y + (ctrl_h - btn_h) / 2.0, btn_w, btn_h);
        let exp_hov   = expand_en && input_coordinator.is_hovered(&uzor::types::WidgetId::new("agent:expand_toggle"));
        ctx.set_fill_color(if !expand_en { &theme.button_bg } else if exp_hov { &theme.button_bg_hover } else { &theme.button_bg });
        ctx.fill_rounded_rect(exp_rect.x, exp_rect.y, exp_rect.width, exp_rect.height, 3.0);
        {
            let exp_icon = if any_hidden { uzor::render::icons::ui::ICON_COLLAPSE } else { uzor::render::icons::ui::ICON_EXPAND };
            draw_svg_icon(ctx, exp_icon,
                exp_rect.x + icon_pad, exp_rect.y + icon_pad,
                exp_rect.width - icon_pad * 2.0, exp_rect.height - icon_pad * 2.0,
                if expand_en { &theme.item_text } else { &theme.item_text_muted });
        }
        if expand_en {
            input_coordinator.register("agent:expand_toggle", exp_rect, uzor::input::Sense::CLICK);
        }
        result.item_rects.push(("agent:expand_toggle".to_string(), exp_rect));
        cur_x += btn_w + gap;

        // [reset] reset sizes
        let reset_en   = has_focused && multi_leaf;
        let reset_rect = WidgetRect::new(cur_x, y + (ctrl_h - btn_h) / 2.0, btn_w, btn_h);
        let reset_hov  = reset_en && input_coordinator.is_hovered(&uzor::types::WidgetId::new("agent:reset_sizes"));
        ctx.set_fill_color(if !reset_en { &theme.button_bg } else if reset_hov { &theme.button_bg_hover } else { &theme.button_bg });
        ctx.fill_rounded_rect(reset_rect.x, reset_rect.y, reset_rect.width, reset_rect.height, 3.0);
        draw_svg_icon(ctx, uzor::render::icons::ui::ICON_REFRESH,
            reset_rect.x + icon_pad, reset_rect.y + icon_pad,
            reset_rect.width - icon_pad * 2.0, reset_rect.height - icon_pad * 2.0,
            if reset_en { &theme.item_text } else { &theme.item_text_muted });
        if reset_en {
            input_coordinator.register("agent:reset_sizes", reset_rect, uzor::input::Sense::CLICK);
        }
        result.item_rects.push(("agent:reset_sizes".to_string(), reset_rect));
        cur_x += btn_w + gap;

        // [×] close pane
        let close_rect = WidgetRect::new(cur_x, y + (ctrl_h - btn_h) / 2.0, btn_w, btn_h);
        let cl_hov     = has_focused && input_coordinator.is_hovered(&uzor::types::WidgetId::new("agent:close_pane"));
        ctx.set_fill_color(if !has_focused { &theme.button_bg } else if cl_hov { &theme.danger_hover_bg } else { &theme.background });
        ctx.fill_rounded_rect(close_rect.x, close_rect.y, close_rect.width, close_rect.height, 3.0);
        ctx.set_stroke_color(&theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(close_rect.x, close_rect.y);
        ctx.line_to(close_rect.x + btn_w, close_rect.y);
        ctx.line_to(close_rect.x + btn_w, close_rect.y + btn_h);
        ctx.line_to(close_rect.x, close_rect.y + btn_h);
        ctx.close_path();
        ctx.stroke();
        draw_svg_icon(ctx, uzor::render::icons::ui::ICON_CLOSE,
            close_rect.x + icon_pad, close_rect.y + icon_pad,
            close_rect.width - icon_pad * 2.0, close_rect.height - icon_pad * 2.0,
            if has_focused { &theme.danger } else { &theme.item_text_muted });
        if has_focused {
            input_coordinator.register("agent:close_pane", close_rect, uzor::input::Sense::CLICK);
        }
        result.item_rects.push(("agent:close_pane".to_string(), close_rect));

        y += ctrl_h + gap;
    }

    // ── Grid area ─────────────────────────────────────────────────────────────
    let grid_h = (rect.height - 40.0 - (y - content_y)).max(60.0);
    let grid_rect = uzor::panels::PanelRect::new(x as f32, y as f32, inner_w as f32, grid_h as f32);

    if state.agent_leaves.is_empty() {
        // Empty state placeholder.
        ctx.set_fill_color(&theme.terminal_bg);
        ctx.fill_rounded_rect(x, y, inner_w, grid_h, 4.0);
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&theme.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Pick a CLI above to open a pane", x + inner_w / 2.0, y + grid_h / 2.0);
    } else {
        // Run layout every frame so panel_rects() always reflects the current grid_rect.
        state.agent_docking.inner_mut().layout(grid_rect);
        let docking = state.agent_docking.inner();
        let panel_rects = docking.panel_rects();

        for (&leaf_id, &prect) in panel_rects {
            let desc = match state.agent_leaves.get(&leaf_id) {
                Some(d) => d,
                None => continue,
            };
            let is_focused = state.focused_agent_leaf == Some(leaf_id);
            let is_hovered = !is_focused && state.hovered_agent_leaf == Some(leaf_id);

            render_agents_pane(
                ctx,
                leaf_id,
                prect,
                desc,
                state,
                theme,
                result,
                input_coordinator,
                is_focused,
                is_hovered,
                grid_rect,
            );
        }

        // Draw separator drag handles and register hit zones.
        let sep_hit_w = 8.0_f64; // wider hit area than the visual bar
        for (sep_idx, sep) in docking.separators().iter().enumerate() {
            use uzor::panels::SeparatorOrientation;
            let visual_thickness = sep.thickness_for_state() as f64;
            let sep_wid = format!("agent:sep:{}", sep_idx);
            match sep.orientation {
                SeparatorOrientation::Vertical => {
                    // position = x, start = y, length = height
                    let sep_x = sep.position as f64 - visual_thickness / 2.0;
                    let sep_y = sep.start as f64;
                    let sep_h = sep.length as f64;
                    ctx.set_fill_color(&theme.separator);
                    ctx.fill_rect(sep_x, sep_y, visual_thickness, sep_h);
                    // Hit zone (wider than visual bar for easier grabbing).
                    let hit_rect = WidgetRect::new(
                        sep.position as f64 - sep_hit_w / 2.0,
                        sep_y,
                        sep_hit_w,
                        sep_h,
                    );
                    input_coordinator.register(sep_wid.as_str(), hit_rect, uzor::input::Sense::CLICK);
                    result.item_rects.push((sep_wid, hit_rect));
                }
                SeparatorOrientation::Horizontal => {
                    // position = y, start = x, length = width
                    let sep_y = sep.position as f64 - visual_thickness / 2.0;
                    let sep_x = sep.start as f64;
                    let sep_w = sep.length as f64;
                    ctx.set_fill_color(&theme.separator);
                    ctx.fill_rect(sep_x, sep_y, sep_w, visual_thickness);
                    // Hit zone.
                    let hit_rect = WidgetRect::new(
                        sep_x,
                        sep.position as f64 - sep_hit_w / 2.0,
                        sep_w,
                        sep_hit_w,
                    );
                    input_coordinator.register(sep_wid.as_str(), hit_rect, uzor::input::Sense::CLICK);
                    result.item_rects.push((sep_wid, hit_rect));
                }
            }
        }
    }

    y += grid_h + pad;
    y - content_y
}

// ── Separator struct fields are positional depending on orientation.
// Vertical:   position=x, offset=y, length=height
// Horizontal: position=y, offset=x, length=width
// This matches how the uzor separator module defines them.

/// Render the Layout dropdown overlay for the agents control row.
///
/// Appears anchored just below the Layout button.
/// Lists Split H, Split V (separator), Expand or Collapse (depending on
/// `any_hidden`), Reset Sizes.
///
/// Render a single agent pane within the docking grid.
fn render_agents_pane(
    ctx: &mut dyn RenderContext,
    leaf_id: uzor::panels::LeafId,
    prect: uzor::panels::PanelRect,
    desc: &crate::agents_dock::AgentLeafDescriptor,
    state: &SidebarState,
    theme: &ToolbarTheme,
    result: &mut RightSidebarResult,
    input_coordinator: &mut InputCoordinator,
    is_focused: bool,
    _is_hovered: bool,
    grid_rect: uzor::panels::PanelRect,
) {
    let header_h = 28.0_f64;
    // Apply a uniform 2 px inset from all four sides of the panel rect so that
    // headers, accent bars, input rows and close buttons do not overlap the
    // docking separators that surround this pane.
    let inset = 8.0_f64;
    let px = prect.x as f64 + inset;
    let py = prect.y as f64 + inset;
    let pw = (prect.width as f64 - inset * 2.0).max(1.0);
    let ph = (prect.height as f64 - inset * 2.0).max(1.0);

    // Pane background.
    ctx.set_fill_color(&theme.background);
    ctx.fill_rounded_rect(px, py, pw, ph, 2.0);

    // Whole-pane hover/focus absorber (registered FIRST so later widgets
    // like buttons and the terminal body take priority on clicks). Used to
    // drive hover-to-focus via InputCoordinator::hovered_widget().
    {
        let pane_rect = WidgetRect::new(px, py, pw, ph);
        let pane_wid = format!("agent:leaf:{}:focus", leaf_id.0);
        input_coordinator.register(pane_wid.as_str(), pane_rect, uzor::input::Sense::CLICK);
        result.item_rects.push((pane_wid, pane_rect));
    }

    // ── Pane header (28 px) — single line ────────────────────────────────────
    {
        // Header background matches leaf body — no separate header color.
        ctx.set_fill_color(&theme.background);
        ctx.fill_rect(px, py, pw, header_h);

        // Visual center between leaf separator (py - inset) and header separator (py + header_h - 1).
        let mid_y  = (py - inset + py + header_h - 1.0) / 2.0; // midpoint of full header zone
        let btn_sz = 22.0; // square hit-area for icon buttons
        let icon_sz = 16.0; // all SVG icons: chevron, plus, close

        // Close button — right-aligned, always visible.
        let close_x   = px + pw - btn_sz - 4.0;
        let close_y   = mid_y - btn_sz / 2.0;
        let close_wid = format!("agent:leaf:{}:close", leaf_id.0);
        let cl_hov    = input_coordinator.is_hovered(&uzor::types::WidgetId::new(close_wid.as_str()));
        ctx.set_fill_color(if cl_hov { &theme.danger_hover_bg } else { &theme.background });
        ctx.fill_rounded_rect(close_x, close_y, btn_sz, btn_sz, 3.0);
        draw_svg_icon(ctx, uzor::render::icons::ui::ICON_CLOSE,
            close_x + (btn_sz - icon_sz) / 2.0, close_y + (btn_sz - icon_sz) / 2.0,
            icon_sz, icon_sz,
            if cl_hov { &theme.item_text_active } else { &theme.item_text_muted });
        let close_rect = WidgetRect::new(close_x, close_y, btn_sz, btn_sz);
        input_coordinator.register(close_wid.as_str(), close_rect, uzor::input::Sense::CLICK);
        result.item_rects.push((close_wid, close_rect));

        // CLI name — uppercase, 13px, centered on mid_y.
        let cli_name = desc.cli.label().to_uppercase();
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(&theme.item_text);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&cli_name, px + 6.0, mid_y);

        // Chat-mode inline buttons: [Sessions + chevron] [+]
        if desc.mode == gate4agent::InstanceMode::Chat {
            // Approximate name text width at 13px ~ 8px per char.
            let name_w = cli_name.len() as f64 * 8.0 + 6.0;
            let mut btn_x = px + 6.0 + name_w + 10.0;

            // [Sessions] + chevron button.
            let sess_text_w = "Sessions".len() as f64 * 8.0; // ~8px per char at 13px
            let sess_w = sess_text_w + 1.0 + icon_sz + 8.0; // text + 1px gap + 16px icon + h-padding
            let sess_y = mid_y - btn_sz / 2.0;
            let sess_wid  = format!("agent:leaf:{}:sessions_toggle", leaf_id.0);
            let sess_open = state.agent_sessions_dropdown == Some(leaf_id);
            let sess_hov  = input_coordinator.is_hovered(&uzor::types::WidgetId::new(sess_wid.as_str()));
            ctx.set_fill_color(if sess_open || sess_hov { &theme.item_bg_hover } else { &theme.background });
            ctx.fill_rounded_rect(btn_x, sess_y, sess_w, btn_sz, 3.0);
            ctx.set_font("13px sans-serif");
            ctx.set_fill_color(&theme.item_text);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("Sessions", btn_x + 4.0, mid_y);
            let chev_x = btn_x + 4.0 + sess_text_w + 1.0;
            let chev_y = mid_y - icon_sz / 2.0; // 16px icon centered on mid_y
            draw_svg_icon(ctx, uzor::render::icons::ui::ICON_CHEVRON_DOWN,
                chev_x, chev_y, icon_sz, icon_sz, &theme.item_text_muted);
            let sess_rect = WidgetRect::new(btn_x, sess_y, sess_w, btn_sz);
            input_coordinator.register(sess_wid.as_str(), sess_rect, uzor::input::Sense::CLICK);
            result.item_rects.push((sess_wid, sess_rect));
            btn_x += sess_w + 4.0;

            // [+] new session button — icon only.
            let new_y   = mid_y - btn_sz / 2.0;
            let new_wid = format!("agent:leaf:{}:new_session", leaf_id.0);
            let new_hov = input_coordinator.is_hovered(&uzor::types::WidgetId::new(new_wid.as_str()));
            ctx.set_fill_color(if new_hov { &theme.item_bg_hover } else { &theme.background });
            ctx.fill_rounded_rect(btn_x, new_y, btn_sz, btn_sz, 3.0);
            draw_svg_icon(ctx, uzor::render::icons::ui::ICON_PLUS,
                btn_x + (btn_sz - icon_sz) / 2.0, new_y + (btn_sz - icon_sz) / 2.0,
                icon_sz, icon_sz, &theme.item_text_muted);
            let new_rect = WidgetRect::new(btn_x, new_y, btn_sz, btn_sz);
            input_coordinator.register(new_wid.as_str(), new_rect, uzor::input::Sense::CLICK);
            result.item_rects.push((new_wid, new_rect));
        }
        // PTY mode: no controls in header — Start button is in the content area.

        // Thin separator at the bottom of the header.
        ctx.set_fill_color(&theme.separator);
        ctx.fill_rect(px, py + header_h - 1.0, pw, 1.0);
    }

    // ── Content area (below header) ───────────────────────────────────────────
    let content_y2 = py + header_h;
    let content_h2 = (ph - header_h).max(1.0);

    let pane_right = prect.x as f64 + prect.width as f64;
    match desc.mode {
        gate4agent::InstanceMode::Pty => {
            render_agents_pty_leaf(
                ctx, leaf_id, px, content_y2, pw, content_h2,
                desc, state, theme, result, input_coordinator, is_focused, grid_rect,
                pane_right,
            );
        }
        gate4agent::InstanceMode::Chat => {
            render_agents_chat_leaf(
                ctx, leaf_id, px, content_y2, pw, content_h2,
                desc, state, theme, result, input_coordinator, is_focused,
                pane_right,
            );
        }
    }

    // ── Sessions dropdown overlay (rendered last so it's on top) ──────────────
    if state.agent_sessions_dropdown == Some(leaf_id) {
        let drop_x = px + 4.0;
        let drop_y = py + header_h;
        let drop_w = (pw - 8.0).max(100.0);
        let item_h = 22.0;
        let sessions = state.agent_past_sessions.get(&leaf_id).map(|v| v.as_slice()).unwrap_or(&[]);
        let max_visible = 8;
        let total_h = (sessions.len() as f64 * item_h + 4.0).max(28.0);
        let drop_h = total_h.min(max_visible as f64 * item_h + 4.0).max(28.0);
        let needs_scroll = total_h > drop_h;
        let scroll_off = state.agent_sessions_scroll.get(&leaf_id)
            .map(|s| s.offset).unwrap_or(0.0)
            .clamp(0.0, (total_h - drop_h).max(0.0));

        // Backdrop to close the dropdown on outside click.
        let backdrop_wid = format!("agent:leaf:{}:sessions_backdrop", leaf_id.0);
        let backdrop_rect = WidgetRect::new(px, py + header_h, pw, content_h2);
        input_coordinator.register(backdrop_wid.as_str(), backdrop_rect, uzor::input::Sense::CLICK);
        result.item_rects.push((backdrop_wid, backdrop_rect));

        // Dropdown background.
        ctx.set_fill_color(&theme.dropdown_bg);
        ctx.fill_rounded_rect(drop_x, drop_y, drop_w, drop_h, 4.0);
        ctx.set_fill_color(&theme.separator);
        ctx.fill_rect(drop_x, drop_y, drop_w, 1.0); // top border

        // Scroll area for wheel routing.
        let scroll_wid = format!("agent:leaf:{}:sessions_scroll_area", leaf_id.0);
        let scroll_rect = WidgetRect::new(drop_x, drop_y, drop_w, drop_h);
        input_coordinator.register(scroll_wid.as_str(), scroll_rect, uzor::input::Sense::HOVER);
        result.item_rects.push((scroll_wid, scroll_rect));

        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        if sessions.is_empty() {
            ctx.set_font("10px sans-serif");
            ctx.set_fill_color(&theme.item_text_muted);
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("No sessions yet", drop_x + drop_w / 2.0, drop_y + drop_h / 2.0);
        } else {
            ctx.save();
            ctx.clip_rect(drop_x, drop_y + 1.0, drop_w, drop_h - 2.0);
            for (idx, session) in sessions.iter().enumerate() {
                let item_y = drop_y + 2.0 + idx as f64 * item_h - scroll_off;
                if item_y + item_h < drop_y || item_y > drop_y + drop_h { continue; }
                let item_wid = format!("agent:leaf:{}:load_session:{}", leaf_id.0, idx);
                let item_rect = WidgetRect::new(drop_x, item_y, drop_w, item_h);
                let item_hov = input_coordinator.is_hovered(&uzor::types::WidgetId::new(item_wid.as_str()));
                if item_hov {
                    ctx.set_fill_color(&theme.item_bg_hover);
                    ctx.fill_rounded_rect(drop_x, item_y, drop_w, item_h, 2.0);
                }

                let age_secs = (now_ts - session.timestamp).max(0) as u64;
                let age_label = if age_secs < 60 {
                    "just now".to_string()
                } else if age_secs < 3600 {
                    format!("{}m ago", age_secs / 60)
                } else if age_secs < 86400 {
                    format!("{}h ago", age_secs / 3600)
                } else if age_secs < 172_800 {
                    "yesterday".to_string()
                } else {
                    format!("{}d ago", age_secs / 86400)
                };

                ctx.set_font("9px sans-serif");
                ctx.set_fill_color(&theme.item_text_muted);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(&age_label, drop_x + 6.0, item_y + item_h / 2.0);

                let ts_w = 44.0;
                let preview_x = drop_x + 6.0 + ts_w;
                let preview = if session.preview.chars().count() > 38 {
                    let truncated: String = session.preview.chars().take(38).collect();
                    format!("{}…", truncated)
                } else {
                    session.preview.clone()
                };
                ctx.set_font("10px sans-serif");
                ctx.set_fill_color(if item_hov { &theme.item_text_hover } else { &theme.item_text_muted });
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(&preview, preview_x, item_y + item_h / 2.0);

                input_coordinator.register(item_wid.as_str(), item_rect, uzor::input::Sense::CLICK);
                result.item_rects.push((item_wid, item_rect));
            }
            ctx.restore();
        }

        // Thin scrollbar.
        if needs_scroll {
            let sb_w = 4.0;
            let sb_x = drop_x + drop_w - sb_w - 1.0;
            let handle_ratio = drop_h / total_h;
            let handle_h = (drop_h * handle_ratio).max(16.0);
            let max_travel = drop_h - handle_h;
            let handle_y = drop_y + (scroll_off / (total_h - drop_h)) * max_travel;
            ctx.set_fill_color(&theme.separator);
            ctx.set_global_alpha(0.2);
            ctx.fill_rect(sb_x, drop_y, sb_w, drop_h);
            ctx.set_global_alpha(0.5);
            ctx.fill_rounded_rect(sb_x, handle_y, sb_w, handle_h, 2.0);
            ctx.set_global_alpha(1.0);
        }
    }

}

// =============================================================================
// PTY terminal grid renderer (per-leaf)
// =============================================================================

/// Render a PTY agent pane for a single leaf.
#[allow(clippy::too_many_arguments)]
fn render_agents_pty_leaf(
    ctx: &mut dyn RenderContext,
    leaf_id: uzor::panels::LeafId,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    desc: &crate::agents_dock::AgentLeafDescriptor,
    state: &SidebarState,
    theme: &ToolbarTheme,
    result: &mut RightSidebarResult,
    input_coordinator: &mut InputCoordinator,
    is_focused: bool,
    _grid_rect: uzor::panels::PanelRect,
    pane_right: f64,
) {
    // Snapshot for this specific instance.
    // The snapshot is stored in agent_snapshots map keyed by leaf_id.
    // We receive it via result — but for rendering we need the snapshot directly.
    // chart-app sets snapshots per-leaf before render; we access via a per-leaf field
    // stored in agent_snapshots on SidebarState. Since state.rs only has a map now,
    // we look it up via leaf_id.
    let snapshot = state.agent_leaf_snapshots.get(&leaf_id);
    let selection = state.agent_pty_selections.get(&leaf_id).copied();
    let pty_scroll_offset = state.agent_pty_scrolls.get(&leaf_id).map(|s| s.offset).unwrap_or(0.0);

    // If focused, expose this leaf's PTY for hover-focus and resize.
    if is_focused {
        let terminal_rect = WidgetRect::new(x, y, w, h);
        result.agent_terminal_rect = Some(terminal_rect);
        let pty_cols = ((w / 7.0) as u16).max(1);
        let pty_rows = ((h / 19.0) as u16).max(1);
        result.agent_terminal_size = Some((pty_cols, pty_rows));
        result.agent_content_rect = Some(terminal_rect);
    }

    // Click-to-focus: the entire content area focuses this leaf.
    let focus_wid = format!("agent:leaf:{}:focus_content", leaf_id.0);
    let focus_rect = WidgetRect::new(x, y, w, h);
    input_coordinator.register(focus_wid.as_str(), focus_rect, uzor::input::Sense::CLICK);
    result.item_rects.push((focus_wid, focus_rect));

    match snapshot {
        Some(snap) => {
            use crate::agent_types::AgentSnapshotMode;
            if let AgentSnapshotMode::Pty(_) = &snap.mode {
                let pty_rows_count = if let AgentSnapshotMode::Pty(ref g) = snap.mode { g.rows as f64 } else { 24.0 };
                let pty_content_h = pty_rows_count * 19.0;
                let max_pty_scroll = (pty_content_h - h).max(0.0);
                let scroll_clamped = pty_scroll_offset.clamp(0.0, max_pty_scroll);

                // Always store per-leaf dimensions so wheel/scrollbar can route to any leaf.
                result.agent_leaf_content_heights.insert(leaf_id, (pty_content_h, h));

                if is_focused {
                    result.agent_pty_content_height = pty_content_h;
                    result.agent_pty_viewport_h = h;
                }

                let scrollbar_x = pane_right - 8.0 + 1.0;
                if let Some((handle_rect, track_rect)) = render_agents_pty_grid(ctx, Some(snap), selection, x, y, w, h, scroll_clamped, &theme.terminal_bg, &theme.selection, &theme.item_text_active, scrollbar_x) {
                    // Always insert into per-leaf map; also keep legacy focused-only fields.
                    result.agent_leaf_scrollbar_rects.insert(leaf_id, (Some(handle_rect), Some(track_rect)));
                    if is_focused {
                        result.agent_pty_scrollbar_handle_rect = Some(handle_rect);
                        result.agent_pty_scrollbar_track_rect = Some(track_rect);
                    }
                } else {
                    result.agent_leaf_scrollbar_rects.entry(leaf_id).or_insert((None, None));
                }
            } else {
                // Snapshot exists but it's not PTY (Idle or Chat) — show idle state.
                render_pty_idle(ctx, leaf_id, x, y, w, h, input_coordinator, result, theme);
            }
        }
        None => {
            render_pty_idle(ctx, leaf_id, x, y, w, h, input_coordinator, result, theme);
        }
    }

    // Store leaf_id in result for lib.rs to use for per-leaf resize routing.
    // We use agent_terminal_rect as the key — for now, only the focused leaf is tracked.
    let _ = desc;
    let _ = is_focused;
}

/// Draw the "Click Start" idle state for a PTY leaf.
fn render_pty_idle(
    ctx: &mut dyn RenderContext,
    leaf_id: uzor::panels::LeafId,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    input_coordinator: &mut InputCoordinator,
    result: &mut RightSidebarResult,
    theme: &ToolbarTheme,
) {
    ctx.set_fill_color(&theme.terminal_bg);
    ctx.fill_rounded_rect(x, y, w, h, 2.0);

    let btn_w = 70.0;
    let btn_h = 22.0;
    let btn_x = x + w / 2.0 - btn_w / 2.0;
    let btn_y = y + h / 2.0 - btn_h / 2.0;
    let start_rect = WidgetRect::new(btn_x, btn_y, btn_w, btn_h);
    let start_wid = format!("agent:leaf:{}:start", leaf_id.0);
    let _hov = input_coordinator.is_hovered(&uzor::types::WidgetId::new(start_wid.as_str()));
    ctx.set_fill_color(&theme.accent);
    ctx.fill_rounded_rect(btn_x, btn_y, btn_w, btn_h, 3.0);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&theme.item_text_active);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Start", btn_x + btn_w / 2.0, btn_y + btn_h / 2.0);
    input_coordinator.register(start_wid.as_str(), start_rect, uzor::input::Sense::CLICK);
    result.item_rects.push((start_wid, start_rect));
}

/// Core PTY grid rendering (shared between legacy and leaf paths).
///
/// Returns `(handle_rect, track_rect)` for the scrollbar drawn (if any).
fn render_agents_pty_grid(
    ctx: &mut dyn RenderContext,
    snapshot: Option<&crate::agent_types::AgentRenderSnapshot>,
    selection: Option<crate::state::PtySelection>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    scroll_offset: f64,
    terminal_bg: &str,
    selection_color: &str,
    cursor_color: &str,
    scrollbar_x: f64,
) -> Option<(WidgetRect, WidgetRect)> {
    use crate::agent_types::AgentSnapshotMode;

    // Terminal background.
    ctx.set_fill_color(terminal_bg);
    ctx.fill_rounded_rect(x, y, w, h, 4.0);

    let grid = match snapshot.and_then(|s| {
        if let AgentSnapshotMode::Pty(ref g) = s.mode { Some(g) } else { None }
    }) {
        Some(g) => g,
        None => return None,
    };

    let char_w = 7.0_f64;
    let char_h = 19.0_f64;
    // Text baseline offset from top of cell (ascender ≈ 70% of line height).
    let baseline_offset = char_h * 0.78;

    ctx.save();
    ctx.clip_rect(x, y, w, h);

    for (row_idx, row) in grid.cells.iter().enumerate() {
        let cell_y = y + row_idx as f64 * char_h - scroll_offset;
        // Skip rows entirely above or below the visible area.
        if cell_y + char_h <= y {
            continue;
        }
        if cell_y >= y + h {
            break;
        }
        for (col_idx, cell) in row.iter().enumerate() {
            let cell_x = x + col_idx as f64 * char_w;
            if cell_x >= x + w {
                break;
            }

            // Skip continuation cells (right half of wide char).
            if cell.width == 0 {
                continue;
            }

            let cell_draw_w = cell.width as f64 * char_w;

            // Background: skip when terminal default (let themed bg show through).
            if !cell.bg_is_default {
                let bg_hex = rgb_to_hex(cell.bg);
                ctx.set_fill_color(&bg_hex);
                ctx.fill_rect(cell_x, cell_y, cell_draw_w, char_h);
            }

            // Character: skip space (no-op).
            if cell.ch == " " {
                continue;
            }

            // Foreground text.
            let fg_hex = rgb_to_hex(cell.fg);
            ctx.set_fill_color(&fg_hex);
            if cell.bold {
                ctx.set_font("bold 11px JetBrainsMono");
            } else {
                ctx.set_font("11px JetBrainsMono");
            }
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Alphabetic);

            ctx.fill_text(&cell.ch, cell_x, cell_y + baseline_offset);
        }
    }

    // ── Selection overlay ────────────────────────────────────────────────
    if let Some(sel) = selection {
        if !sel.is_empty() {
            let ((lo_row, lo_col), (hi_row, hi_col)) = sel.ordered();
            let lo_row = lo_row as usize;
            let hi_row = hi_row as usize;
            let lo_col = lo_col as usize;
            let hi_col = hi_col as usize;
            let total_cols = grid.cols as usize;
            ctx.set_fill_color(selection_color);
            ctx.set_global_alpha(0.35);
            for row in lo_row..=hi_row {
                let (c0, c1) = if lo_row == hi_row {
                    (lo_col, hi_col)
                } else if row == lo_row {
                    (lo_col, total_cols)
                } else if row == hi_row {
                    (0, hi_col)
                } else {
                    (0, total_cols)
                };
                if c1 <= c0 { continue; }
                let rx = x + c0 as f64 * char_w;
                let ry = y + row as f64 * char_h - scroll_offset;
                let rw = (c1 - c0) as f64 * char_w;
                ctx.fill_rect(rx, ry, rw, char_h);
            }
            ctx.set_global_alpha(1.0);
        }
    }

    // ── Cursor ────────────────────────────────────────────────────────────
    if grid.cursor_visible {
        let cur_row = grid.cursor_row as usize;
        let cur_col = grid.cursor_col as usize;
        if cur_row < grid.rows as usize && cur_col < grid.cols as usize {
            let cx = x + cur_col as f64 * char_w;
            let cy = y + cur_row as f64 * char_h - scroll_offset;
            if cy + char_h > y && cy < y + h {
                let cell = &grid.cells[cur_row][cur_col];
                ctx.set_fill_color(cursor_color);
                ctx.fill_rect(cx, cy, char_w, char_h);
                if !cell.ch.is_empty() && cell.ch != " " {
                    ctx.set_fill_color(&rgb_to_hex(cell.bg));
                    ctx.set_font("11px JetBrainsMono");
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Alphabetic);
                    ctx.fill_text(&cell.ch, cx, cy + baseline_offset);
                }
            }
        }
    }

    // ── Scrollbar (right edge) ────────────────────────────────────────────
    let sb_rects = {
        let pty_content_h = grid.rows as f64 * char_h;
        let sb_w = 6.0;
        let sb_rect = uzor::types::Rect::new(scrollbar_x, y, sb_w, h);
        let sb_config = ScrollbarConfig::new(pty_content_h, h, scroll_offset);
        let sb_state = SbState::Active;
        let widget_theme = zengeld_chart::ui::widgets::types::WidgetTheme::default();
        let sb_result = draw_scrollbar(ctx, &sb_config, sb_state, sb_rect, &widget_theme, None);
        if sb_config.needs_scrollbar() {
            Some((sb_result.handle_rect, sb_result.track_rect))
        } else {
            None
        }
    };

    // ── Buddy ASCII art overlay ──────────────────────────────────────────
    // Drawn last so it floats above the main grid. Anchored to the top-right
    // of the visible PTY area regardless of original screen position.
    if let Some(buddy) = grid.buddy.as_ref() {
        let bw = buddy.width as f64 * char_w;
        let bh = buddy.rows.len() as f64 * char_h;
        // Anchor: top-right with a small inset.
        let bx = (x + w - bw - 4.0).max(x);
        let by = y + 4.0;
        // Subtle background panel so the art stays readable over text.
        ctx.set_fill_color("#000000");
        ctx.set_global_alpha(0.55);
        ctx.fill_rect(bx - 2.0, by - 2.0, bw + 4.0, bh + 4.0);
        ctx.set_global_alpha(1.0);
        for (rr, row) in buddy.rows.iter().enumerate() {
            let cy = by + rr as f64 * char_h;
            for (cc, cell) in row.iter().enumerate() {
                if cell.ch == " " || cell.ch.is_empty() { continue; }
                let cx = bx + cc as f64 * char_w;
                let fg_hex = rgb_to_hex(cell.fg);
                ctx.set_fill_color(&fg_hex);
                if cell.bold {
                    ctx.set_font("bold 11px JetBrainsMono");
                } else {
                    ctx.set_font("11px JetBrainsMono");
                }
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Alphabetic);
                ctx.fill_text(&cell.ch, cx, cy + baseline_offset);
            }
        }
    }

    ctx.restore();
    sb_rects
}

// =============================================================================
// Chat renderer (per-leaf)
// =============================================================================

/// Render a Chat agent pane for a single leaf.
#[allow(clippy::too_many_arguments)]
fn render_agents_chat_leaf(
    ctx: &mut dyn RenderContext,
    leaf_id: uzor::panels::LeafId,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    desc: &crate::agents_dock::AgentLeafDescriptor,
    state: &SidebarState,
    theme: &ToolbarTheme,
    result: &mut RightSidebarResult,
    input_coordinator: &mut InputCoordinator,
    is_focused: bool,
    pane_right: f64,
) {
    let row_h = 28.0;
    let send_gap = 4.0;
    let ctrl_bar_h_reserve = 22.0;
    let input_area_h = row_h + send_gap + 1.0 + ctrl_bar_h_reserve;  // input + gap + divider + control bar
    let chat_h = (h - input_area_h).max(20.0);
    let chat_y = y;

    let snapshot = state.agent_leaf_snapshots.get(&leaf_id);
    let chat_scroll_offset = state.agent_chat_scrolls.get(&leaf_id).map(|s| s.offset).unwrap_or(0.0);

    // Content height + scrollbar.
    let chat_content_h = compute_chat_content_height(ctx, snapshot, w, &state.agent_chat_expanded, leaf_id);

    // Always store per-leaf dimensions so wheel/scrollbar can route to any leaf.
    result.agent_leaf_content_heights.insert(leaf_id, (chat_content_h, chat_h));

    if is_focused {
        result.agent_chat_content_height = chat_content_h;
        result.agent_chat_viewport_h = chat_h;
        result.agent_content_rect = Some(WidgetRect::new(x, chat_y, w, chat_h));
    }

    let max_scroll = (chat_content_h - chat_h).max(0.0);
    let scroll_clamped = chat_scroll_offset.clamp(0.0, max_scroll);

    let selection = state.agent_chat_selections.get(&leaf_id);
    let scrollbar_x = pane_right - 8.0 + 1.0;
    if let Some((handle_rect, track_rect)) = render_agents_chat_bubbles(
        ctx, snapshot, theme, x, chat_y, w, chat_h, scroll_clamped, chat_content_h,
        selection, leaf_id, &mut result.agent_chat_line_rects, &state.agent_chat_expanded,
        scrollbar_x,
    ) {
        // Always insert into per-leaf map; also keep legacy focused-only fields.
        result.agent_leaf_scrollbar_rects.insert(leaf_id, (Some(handle_rect), Some(track_rect)));
        if is_focused {
            result.agent_chat_scrollbar_handle_rect = Some(handle_rect);
            result.agent_chat_scrollbar_track_rect = Some(track_rect);
        }
    } else {
        result.agent_leaf_scrollbar_rects.entry(leaf_id).or_insert((None, None));
    }

    // Click-to-focus: chat area registers focus.
    let focus_wid = format!("agent:leaf:{}:focus_content", leaf_id.0);
    let focus_rect = WidgetRect::new(x, y, w, h);
    input_coordinator.register(focus_wid.as_str(), focus_rect, uzor::input::Sense::CLICK);
    result.item_rects.push((focus_wid, focus_rect));

    // ── Input panel (Claude Code style) ──────────────────────────────────────
    let panel_y = y + chat_h + send_gap;
    let ctrl_bar_h = 22.0;
    let panel_h = row_h + 1.0 + ctrl_bar_h;  // input row + divider + control bar
    let panel_radius = 8.0;
    let inner_pad = 8.0;

    // Panel background (user bubble color).
    ctx.set_fill_color(&theme.bubble_user_bg);
    ctx.fill_rounded_rect(x, panel_y, w, panel_h, panel_radius);

    // Focused border.
    if is_focused {
        ctx.set_stroke_color(&theme.accent);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(x + 0.5, panel_y + 0.5, w - 1.0, panel_h - 1.0, panel_radius);
    }

    // ── Text input area (top section) ─────────────────────────────────────
    let input_y = panel_y + 2.0;
    let input_h = row_h - 4.0;

    let input_buffer = state.agent_input_buffers.get(&leaf_id).cloned().unwrap_or_default();
    let input_cursor = state.agent_input_cursors.get(&leaf_id).copied().unwrap_or(0);
    let (sel_start, sel_end) = state.agent_input_selections.get(&leaf_id).copied().unwrap_or((None, None));
    let is_input_focused = is_focused && state.agent_input_focused_leaf == Some(leaf_id);

    let input_rect = WidgetRect::new(x + inner_pad, input_y, w - inner_pad * 2.0, input_h);
    let input_config = InputConfig {
        value: input_buffer,
        placeholder: "Message\u{2026}".to_string(),
        disabled: false,
        focused: is_input_focused,
        cursor: input_cursor,
        selection_start: sel_start,
        selection_end: sel_end,
        font_size: 12.0,
        padding: 4.0,
        radius: 0.0,
        ..InputConfig::default()
    };
    let transparent = "#00000000".to_string();
    let input_widget_theme = WidgetTheme {
        bg_normal: transparent.clone(),
        bg_hover: transparent.clone(),
        bg_pressed: transparent.clone(),
        bg_disabled: transparent.clone(),
        text_normal: theme.item_text.clone(),
        text_hover: theme.item_text.clone(),
        text_disabled: theme.item_text_muted.clone(),
        border_normal: transparent.clone(),
        border_hover: transparent.clone(),
        border_focused: transparent.clone(),
        accent: theme.selection.clone(),
        accent_hover: theme.selection.clone(),
        success: theme.success.clone(),
        warning: theme.warning.clone(),
        danger: theme.danger.clone(),
    };
    let input_draw_result = draw_input(ctx, &input_config, WidgetState::Normal, input_rect, &input_widget_theme);
    if is_input_focused && state.agent_input_cursor_visible {
        draw_input_cursor(ctx, input_draw_result.cursor_x, input_draw_result.cursor_y, input_draw_result.cursor_height, &theme.item_text);
    }

    let input_wid = format!("agent:leaf:{}:input", leaf_id.0);
    input_coordinator.register(input_wid.as_str(), input_rect, uzor::input::Sense::CLICK);
    result.item_rects.push((input_wid.clone(), input_rect));

    if is_focused {
        result.agent_input_rect = Some(input_rect);
        result.agent_input_char_positions = Some(input_draw_result.char_x_positions);
    }

    // ── Divider line ──────────────────────────────────────────────────────
    let divider_y = panel_y + row_h;
    ctx.set_fill_color(&theme.separator);
    ctx.fill_rect(x + inner_pad, divider_y, w - inner_pad * 2.0, 1.0);

    // ── Control bar (bottom section) ──────────────────────────────────────
    let ctrl_y = divider_y + 1.0;
    let ctrl_mid_y = ctrl_y + ctrl_bar_h / 2.0;
    ctx.set_font("12px sans-serif");
    ctx.set_text_baseline(TextBaseline::Middle);

    let cli_tool = match desc.cli {
        gate4agent::AgentCli::Claude   => gate4agent::CliTool::ClaudeCode,
        gate4agent::AgentCli::Codex    => gate4agent::CliTool::Codex,
        gate4agent::AgentCli::Gemini   => gate4agent::CliTool::Gemini,
        gate4agent::AgentCli::OpenCode => gate4agent::CliTool::OpenCode,
    };
    let caps = cli_tool.capabilities();

    // Layout: [model...] [perm] [○ N%] ... [↑]
    // Context % is always visible; model gets truncated with "..." when tight.

    // Compute context % first — it's always shown, so we reserve space for it.
    let ctx_pct = snapshot
        .and_then(|s| s.context_percent)
        .unwrap_or(0.0);
    let ctx_label = format!("{:.0}%", ctx_pct);
    let ctx_text_w = ctx.measure_text(&ctx_label);
    let circle_r = 7.0;
    let circle_d = circle_r * 2.0;
    let ctx_total_w = circle_d + 3.0 + ctx_text_w;

    // Send button width.
    let send_reserved = ctrl_bar_h + inner_pad; // send_sz + gap

    // Permission label.
    let perm_label_full = if let Some(pid) = state.agent_selected_perm.get(&leaf_id) {
        caps.permission_modes.iter()
            .find(|p| p.id == *pid)
            .map(|p| p.display_name.as_str())
            .unwrap_or("Default")
    } else {
        caps.default_permission_mode()
            .map(|p| p.display_name.as_str())
            .unwrap_or("Default")
    };
    let perm_tw = ctx.measure_text(perm_label_full);

    // Available width for model label = total - perm - ctx - send - gaps.
    let fixed_w = perm_tw + 12.0 + ctx_total_w + 12.0 + send_reserved;
    let avail_model_w = (w - inner_pad * 2.0 - fixed_w).max(20.0);

    // Model label — truncate with "..." if needed.
    let model_label_full: &str = if let Some(mid) = state.agent_selected_model.get(&leaf_id) {
        caps.available_models.iter()
            .find(|m| m.id == *mid)
            .map(|m| m.display_name.as_str())
            .unwrap_or("Model")
    } else {
        caps.default_model()
            .map(|m| m.display_name.as_str())
            .unwrap_or("Model")
    };
    let model_full_tw = ctx.measure_text(model_label_full);
    let model_display: String;
    let model_tw: f64;
    if model_full_tw <= avail_model_w {
        model_display = model_label_full.to_string();
        model_tw = model_full_tw;
    } else {
        // Truncate: find how many chars fit, append "..."
        let ellipsis_w = ctx.measure_text("...");
        let target = avail_model_w - ellipsis_w;
        let mut end = model_label_full.len();
        for i in (1..=model_label_full.len()).rev() {
            if model_label_full.is_char_boundary(i) {
                let sub = &model_label_full[..i];
                if ctx.measure_text(sub) <= target {
                    end = i;
                    break;
                }
            }
        }
        model_display = format!("{}...", &model_label_full[..end]);
        model_tw = ctx.measure_text(&model_display);
    }

    let model_open = state.agent_model_dropdown == Some(leaf_id);
    let model_wid = format!("agent:leaf:{}:model", leaf_id.0);
    let model_hov = input_coordinator.is_hovered(&uzor::types::WidgetId::new(model_wid.as_str()));
    let model_rect = WidgetRect::new(x + inner_pad, ctrl_y, model_tw + 4.0, ctrl_bar_h);
    ctx.set_fill_color(if model_open || model_hov { &theme.item_text } else { &theme.item_text_muted });
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(&model_display, x + inner_pad, ctrl_mid_y);
    input_coordinator.register(model_wid.as_str(), model_rect, uzor::input::Sense::CLICK);
    result.item_rects.push((model_wid, model_rect));

    // Permission label (after model + gap).
    let perm_x = x + inner_pad + model_tw + 12.0;
    let perm_open = state.agent_perm_dropdown == Some(leaf_id);
    let perm_wid = format!("agent:leaf:{}:perm", leaf_id.0);
    let perm_hov = input_coordinator.is_hovered(&uzor::types::WidgetId::new(perm_wid.as_str()));
    let perm_rect = WidgetRect::new(perm_x, ctrl_y, perm_tw + 4.0, ctrl_bar_h);
    ctx.set_fill_color(if perm_open || perm_hov { &theme.item_text } else { &theme.item_text_muted });
    ctx.fill_text(perm_label_full, perm_x, ctrl_mid_y);
    if !caps.permission_modes.is_empty() {
        input_coordinator.register(perm_wid.as_str(), perm_rect, uzor::input::Sense::CLICK);
        result.item_rects.push((perm_wid, perm_rect));
    }

    // Context % with circle (always visible, after perm).
    let ctx_x = perm_x + perm_tw + 12.0;
    let circle_cx = ctx_x + circle_r;
    let circle_cy = ctrl_mid_y;

    // Circle track.
    ctx.set_stroke_color(&theme.separator);
    ctx.set_stroke_width(1.5);
    ctx.begin_path();
    ctx.arc(circle_cx, circle_cy, circle_r - 0.5, 0.0, std::f64::consts::TAU);
    ctx.stroke();

    // Progress arc.
    if ctx_pct > 0.0 {
        let angle = ctx_pct / 100.0 * std::f64::consts::TAU;
        let start = -std::f64::consts::FRAC_PI_2;
        ctx.set_stroke_color(&theme.accent);
        ctx.set_stroke_width(1.5);
        ctx.begin_path();
        ctx.arc(circle_cx, circle_cy, circle_r - 0.5, start, start + angle);
        ctx.stroke();
    }

    // % text.
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(&ctx_label, ctx_x + circle_d + 3.0, ctrl_mid_y);

    // Send button [↑] (far right of control bar).
    let send_sz = ctrl_bar_h;
    let send_x = x + w - inner_pad - send_sz;
    let send_y2 = ctrl_y;
    let send_rect = WidgetRect::new(send_x, send_y2, send_sz, send_sz);
    let send_wid = format!("agent:leaf:{}:send", leaf_id.0);
    let _send_hov = input_coordinator.is_hovered(&uzor::types::WidgetId::new(send_wid.as_str()));
    // Inverted: border/bg = text color, arrow = main background.
    if is_focused {
        ctx.set_fill_color(&theme.item_text_active);
    } else {
        ctx.set_fill_color(&theme.item_text_muted);
    }
    ctx.fill_rounded_rect(send_x, send_y2, send_sz, send_sz, 3.0);
    let acx = send_x + send_sz / 2.0;
    let acy = send_y2 + send_sz / 2.0;
    let arrow_sz = 7.0;
    let arrow_head = 5.0;
    ctx.set_stroke_color(&theme.background);
    ctx.begin_path();
    ctx.move_to(acx, acy + arrow_sz);
    ctx.line_to(acx, acy - arrow_sz);
    ctx.move_to(acx - arrow_head, acy - arrow_sz + arrow_head);
    ctx.line_to(acx, acy - arrow_sz);
    ctx.line_to(acx + arrow_head, acy - arrow_sz + arrow_head);
    ctx.set_stroke_width(1.5);
    ctx.stroke();
    input_coordinator.register(send_wid.as_str(), send_rect, uzor::input::Sense::CLICK);
    result.item_rects.push((send_wid, send_rect));

    // ── Model dropdown overlay ─────────────────────────────────────────────
    if state.agent_model_dropdown == Some(leaf_id) {
        let drop_x = x + inner_pad;
        let item_h = 24.0;
        let models = &caps.available_models;
        let drop_w = (w - inner_pad * 2.0).max(120.0);
        let max_visible = 6;
        let total_h = models.len() as f64 * item_h + 6.0;
        let drop_h = total_h.min(max_visible as f64 * item_h + 6.0);
        let needs_scroll = total_h > drop_h;
        let scroll_off = state.agent_model_scroll.get(&leaf_id)
            .map(|s| s.offset).unwrap_or(0.0)
            .clamp(0.0, (total_h - drop_h).max(0.0));
        // Grow upward from the control bar.
        let drop_y = ctrl_y - drop_h;

        // Backdrop.
        let backdrop_wid = format!("agent:leaf:{}:model_backdrop", leaf_id.0);
        let backdrop_rect = WidgetRect::new(x, y, w, h);
        input_coordinator.register(backdrop_wid.as_str(), backdrop_rect, uzor::input::Sense::CLICK);
        result.item_rects.push((backdrop_wid, backdrop_rect));

        // Dropdown background.
        ctx.set_fill_color(&theme.dropdown_bg);
        ctx.fill_rounded_rect(drop_x, drop_y, drop_w, drop_h, 4.0);
        ctx.set_fill_color(&theme.separator);
        ctx.fill_rect(drop_x, drop_y + drop_h - 1.0, drop_w, 1.0);

        // Register scroll area for wheel routing.
        let scroll_wid = format!("agent:leaf:{}:model_scroll_area", leaf_id.0);
        let scroll_rect = WidgetRect::new(drop_x, drop_y, drop_w, drop_h);
        input_coordinator.register(scroll_wid.as_str(), scroll_rect, uzor::input::Sense::HOVER);
        result.item_rects.push((scroll_wid, scroll_rect));

        ctx.save();
        ctx.clip_rect(drop_x, drop_y + 1.0, drop_w, drop_h - 2.0);
        let selected_mid = state.agent_selected_model.get(&leaf_id);
        for (idx, model_info) in models.iter().enumerate() {
            let item_y = drop_y + 3.0 + idx as f64 * item_h - scroll_off;
            if item_y + item_h < drop_y || item_y > drop_y + drop_h { continue; }
            let item_wid = format!("agent:leaf:{}:select_model:{}", leaf_id.0, model_info.id);
            let item_rect = WidgetRect::new(drop_x, item_y, drop_w, item_h);
            let item_hov = input_coordinator.is_hovered(&uzor::types::WidgetId::new(item_wid.as_str()));
            if item_hov {
                ctx.set_fill_color(&theme.item_bg_hover);
                ctx.fill_rounded_rect(drop_x, item_y, drop_w, item_h, 2.0);
            }
            let is_selected = selected_mid.map_or(model_info.is_default, |mid| *mid == model_info.id);
            ctx.set_font("10px sans-serif");
            ctx.set_text_baseline(TextBaseline::Middle);
            let mid_y2 = item_y + item_h / 2.0;
            if is_selected {
                ctx.set_fill_color(&theme.accent);
                ctx.set_text_align(TextAlign::Left);
                ctx.fill_text("\u{2713}", drop_x + 5.0, mid_y2);
            }
            ctx.set_fill_color(if item_hov { &theme.item_text_hover } else { &theme.item_text });
            ctx.set_text_align(TextAlign::Left);
            ctx.fill_text(&model_info.display_name, drop_x + 16.0, mid_y2);
            if let Some(ctx_w) = model_info.context_window {
                let ctx_label2 = if ctx_w >= 1_000_000 {
                    format!("{}M", ctx_w / 1_000_000)
                } else {
                    format!("{}K", ctx_w / 1_000)
                };
                ctx.set_fill_color(&theme.item_text_muted);
                ctx.set_text_align(TextAlign::Right);
                ctx.fill_text(&ctx_label2, drop_x + drop_w - 6.0, mid_y2);
            }
            input_coordinator.register(item_wid.as_str(), item_rect, uzor::input::Sense::CLICK);
            result.item_rects.push((item_wid, item_rect));
        }
        ctx.restore();

        // Thin scrollbar.
        if needs_scroll {
            let sb_w = 4.0;
            let sb_x = drop_x + drop_w - sb_w - 1.0;
            let handle_ratio = drop_h / total_h;
            let handle_h = (drop_h * handle_ratio).max(16.0);
            let max_travel = drop_h - handle_h;
            let handle_y = drop_y + (scroll_off / (total_h - drop_h)) * max_travel;
            ctx.set_fill_color(&theme.separator);
            ctx.set_global_alpha(0.2);
            ctx.fill_rect(sb_x, drop_y, sb_w, drop_h);
            ctx.set_global_alpha(0.5);
            ctx.fill_rounded_rect(sb_x, handle_y, sb_w, handle_h, 2.0);
            ctx.set_global_alpha(1.0);
        }
    }

    // ── Permission dropdown overlay ────────────────────────────────────────
    if state.agent_perm_dropdown == Some(leaf_id) && !caps.permission_modes.is_empty() {
        let drop_x = x + inner_pad + model_tw + 12.0;
        let item_h = 24.0;
        let modes = &caps.permission_modes;
        let drop_w = ((w - inner_pad) - (model_tw + 12.0)).max(120.0);
        let max_visible = 6;
        let total_h = modes.len() as f64 * item_h + 6.0;
        let drop_h = total_h.min(max_visible as f64 * item_h + 6.0);
        let needs_scroll = total_h > drop_h;
        let scroll_off = state.agent_perm_scroll.get(&leaf_id)
            .map(|s| s.offset).unwrap_or(0.0)
            .clamp(0.0, (total_h - drop_h).max(0.0));
        let drop_y = ctrl_y - drop_h;

        // Backdrop.
        let backdrop_wid = format!("agent:leaf:{}:perm_backdrop", leaf_id.0);
        let backdrop_rect = WidgetRect::new(x, y, w, h);
        input_coordinator.register(backdrop_wid.as_str(), backdrop_rect, uzor::input::Sense::CLICK);
        result.item_rects.push((backdrop_wid, backdrop_rect));

        // Dropdown background.
        ctx.set_fill_color(&theme.dropdown_bg);
        ctx.fill_rounded_rect(drop_x, drop_y, drop_w, drop_h, 4.0);
        ctx.set_fill_color(&theme.separator);
        ctx.fill_rect(drop_x, drop_y + drop_h - 1.0, drop_w, 1.0);

        // Scroll area for wheel routing.
        let scroll_wid = format!("agent:leaf:{}:perm_scroll_area", leaf_id.0);
        let scroll_rect = WidgetRect::new(drop_x, drop_y, drop_w, drop_h);
        input_coordinator.register(scroll_wid.as_str(), scroll_rect, uzor::input::Sense::HOVER);
        result.item_rects.push((scroll_wid, scroll_rect));

        ctx.save();
        ctx.clip_rect(drop_x, drop_y + 1.0, drop_w, drop_h - 2.0);
        let selected_pid = state.agent_selected_perm.get(&leaf_id);
        for (idx, perm_info) in modes.iter().enumerate() {
            let item_y = drop_y + 3.0 + idx as f64 * item_h - scroll_off;
            if item_y + item_h < drop_y || item_y > drop_y + drop_h { continue; }
            let item_wid = format!("agent:leaf:{}:select_perm:{}", leaf_id.0, perm_info.id);
            let item_rect = WidgetRect::new(drop_x, item_y, drop_w, item_h);
            let item_hov = input_coordinator.is_hovered(&uzor::types::WidgetId::new(item_wid.as_str()));
            if item_hov {
                ctx.set_fill_color(&theme.item_bg_hover);
                ctx.fill_rounded_rect(drop_x, item_y, drop_w, item_h, 2.0);
            }
            let is_selected = selected_pid.map_or(perm_info.is_default, |pid| *pid == perm_info.id);
            ctx.set_font("10px sans-serif");
            ctx.set_text_baseline(TextBaseline::Middle);
            let mid_y2 = item_y + item_h / 2.0;
            if is_selected {
                ctx.set_fill_color(&theme.accent);
                ctx.set_text_align(TextAlign::Left);
                ctx.fill_text("\u{2713}", drop_x + 5.0, mid_y2);
            }
            ctx.set_fill_color(if item_hov { &theme.item_text_hover } else { &theme.item_text });
            ctx.set_text_align(TextAlign::Left);
            ctx.fill_text(&perm_info.display_name, drop_x + 16.0, mid_y2);
            input_coordinator.register(item_wid.as_str(), item_rect, uzor::input::Sense::CLICK);
            result.item_rects.push((item_wid, item_rect));
        }
        ctx.restore();

        // Thin scrollbar.
        if needs_scroll {
            let sb_w = 4.0;
            let sb_x = drop_x + drop_w - sb_w - 1.0;
            let handle_ratio = drop_h / total_h;
            let handle_h = (drop_h * handle_ratio).max(16.0);
            let max_travel = drop_h - handle_h;
            let handle_y = drop_y + (scroll_off / (total_h - drop_h)) * max_travel;
            ctx.set_fill_color(&theme.separator);
            ctx.set_global_alpha(0.2);
            ctx.fill_rect(sb_x, drop_y, sb_w, drop_h);
            ctx.set_global_alpha(0.5);
            ctx.fill_rounded_rect(sb_x, handle_y, sb_w, handle_h, 2.0);
            ctx.set_global_alpha(1.0);
        }
    }

    let _ = desc;
}

// =============================================================================
// Chat bubble renderer
// =============================================================================

/// Render chat messages as bubbles inside the content area.
///
/// Returns `(handle_rect, track_rect)` for the scrollbar drawn (if any).
fn render_agents_chat_bubbles(
    ctx: &mut dyn RenderContext,
    snapshot: Option<&crate::agent_types::AgentRenderSnapshot>,
    theme: &ToolbarTheme,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    scroll_offset: f64,
    total_content_h: f64,
    selection: Option<&crate::state::ChatSelection>,
    leaf_id: uzor::panels::LeafId,
    line_rects_out: &mut Vec<(u16, u16, f64, f64, uzor::panels::LeafId, String, f64, u8)>,
    expanded: &std::collections::HashSet<(uzor::panels::LeafId, u16)>,
    scrollbar_x: f64,
) -> Option<(WidgetRect, WidgetRect)> {
    use crate::agent_types::{AgentSnapshotMode, ChatRole};

    // Chat content area background (from theme).
    ctx.set_fill_color(&theme.terminal_bg);
    ctx.fill_rounded_rect(x, y, w, h, 4.0);

    let messages = match snapshot.and_then(|s| {
        if let AgentSnapshotMode::Chat(ref msgs) = s.mode { Some(msgs) } else { None }
    }) {
        Some(m) => m,
        None => return None,
    };

    if messages.is_empty() {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&theme.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("No messages yet", x + w / 2.0, y + h / 2.0);
        return None;
    }

    ctx.save();
    ctx.clip_rect(x, y, w, h);

    let bubble_pad_x = 8.0;
    let bubble_pad_y = 5.0;
    let bubble_gap = 6.0;
    let max_bubble_w = w - 24.0; // leave side margins
    let line_h_normal = 17.0;
    let line_h_mono = 14.0;

    // Start above the viewport by the scroll amount.
    let mut cursor_y = y + 8.0 - scroll_offset;

    for (msg_idx, msg) in messages.iter().enumerate() {
        let msg_i = msg_idx as u16;
        match msg.role {
            ChatRole::User => {
                // Fit-content width bubble, right-aligned (iMessage style).
                ctx.set_font("13px sans-serif");
                let lines = word_wrap_text(ctx, &msg.content, max_bubble_w - bubble_pad_x * 2.0);
                let n_lines = lines.len().max(1);
                // Measure the widest line to size the bubble to its content.
                let max_line_w = lines.iter().map(|l| ctx.measure_text(l)).fold(0.0f64, f64::max);
                let bubble_w = (max_line_w + bubble_pad_x * 2.0).min(w - 12.0);
                let bubble_h = n_lines as f64 * line_h_normal + bubble_pad_y * 2.0;
                // Right-align: pin right edge of bubble 4 px from the right edge of the column.
                let bx = x + w - 4.0 - bubble_w;
                let text_x = bx + bubble_pad_x;

                // Record line rects for selection hit-testing (font_tag=0: normal 13px).
                for (li, line_text) in lines.iter().enumerate() {
                    let line_y = cursor_y + bubble_pad_y + li as f64 * line_h_normal;
                    line_rects_out.push((msg_i, li as u16, line_y, line_y + line_h_normal, leaf_id, line_text.clone(), text_x, 0u8));
                }

                // Bubble background.
                ctx.set_fill_color(&theme.bubble_user_bg);
                ctx.fill_rounded_rect(bx, cursor_y, bubble_w, bubble_h, 6.0);

                ctx.set_fill_color(&theme.bubble_user_text);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Top);
                for (li, line) in lines.iter().enumerate() {
                    ctx.fill_text(line, text_x, cursor_y + bubble_pad_y + li as f64 * line_h_normal);
                }
                cursor_y += bubble_h + bubble_gap;
            }

            ChatRole::Assistant => {
                // Left-aligned, no bubble, subtle text.
                ctx.set_font("13px sans-serif");
                let text_x = x + 8.0;
                let lines = word_wrap_text(ctx, &msg.content, max_bubble_w - bubble_pad_x * 2.0);
                let n_lines = lines.len();
                let text_h = n_lines as f64 * line_h_normal;

                // Record line rects for selection hit-testing (font_tag=0: normal 13px).
                for (li, line_text) in lines.iter().enumerate() {
                    let line_y = cursor_y + li as f64 * line_h_normal;
                    line_rects_out.push((msg_i, li as u16, line_y, line_y + line_h_normal, leaf_id, line_text.clone(), text_x, 0u8));
                }

                ctx.set_fill_color(&theme.item_text);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Top);
                for (li, line) in lines.iter().enumerate() {
                    ctx.fill_text(line, text_x, cursor_y + li as f64 * line_h_normal);
                }
                cursor_y += text_h + bubble_gap;
            }

            ChatRole::Tool => {
                // Tool output — monospace, dark translucent bg.
                // Collapsed (default): single-line pill showing the tool name.
                // Expanded: header + full tool output.
                let is_expanded = expanded.contains(&(leaf_id, msg_i));
                let toggle_char = if is_expanded { '▼' } else { '▶' };
                let tool_label = msg.tool_name.as_deref().unwrap_or("tool");
                let header = format!("{} [{}]", toggle_char, tool_label);
                let tool_text_x = x + 4.0 + bubble_pad_x;

                if is_expanded {
                    ctx.set_font("bold 11px JetBrainsMono");
                    let lines = word_wrap_text(ctx, &msg.content, max_bubble_w - bubble_pad_x * 2.0);
                    let n_lines = lines.len().max(1);
                    let bubble_h = line_h_mono + n_lines as f64 * line_h_mono + bubble_pad_y * 2.0;

                    // Record line rects (header + content lines, font_tag=1: mono 11px).
                    let header_y = cursor_y + bubble_pad_y;
                    line_rects_out.push((msg_i, 0, header_y, header_y + line_h_mono, leaf_id, header.clone(), tool_text_x, 1u8));
                    for (li, line_text) in lines.iter().enumerate() {
                        let line_y = cursor_y + bubble_pad_y + line_h_mono + li as f64 * line_h_mono;
                        line_rects_out.push((msg_i, (li + 1) as u16, line_y, line_y + line_h_mono, leaf_id, line_text.clone(), tool_text_x, 1u8));
                    }

                    ctx.set_fill_color(&theme.bubble_tool_bg);
                    ctx.fill_rounded_rect(x + 4.0, cursor_y, w - 12.0, bubble_h, 3.0);

                    ctx.set_fill_color(&theme.item_text_muted);
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Top);
                    ctx.fill_text(&header, tool_text_x, cursor_y + bubble_pad_y);

                    ctx.set_fill_color(&theme.bubble_tool_text);
                    ctx.set_font("11px JetBrainsMono");
                    for (li, line) in lines.iter().enumerate() {
                        ctx.fill_text(line, tool_text_x, cursor_y + bubble_pad_y + line_h_mono + li as f64 * line_h_mono);
                    }
                    cursor_y += bubble_h + bubble_gap;
                } else {
                    // Collapsed: minimal pill with just the tool name.
                    ctx.set_font("bold 11px JetBrainsMono");
                    let header_y = cursor_y;
                    line_rects_out.push((msg_i, 0, header_y, header_y + line_h_mono, leaf_id, header.clone(), tool_text_x, 1u8));

                    let header_w = (ctx.measure_text(&header) + bubble_pad_x * 2.0).min(w - 12.0);
                    ctx.set_fill_color(&theme.bubble_tool_bg);
                    ctx.fill_rounded_rect(x + 4.0, cursor_y, header_w, line_h_mono + bubble_pad_y, 3.0);

                    ctx.set_fill_color(&theme.item_text_muted);
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Top);
                    ctx.fill_text(&header, tool_text_x, cursor_y + bubble_pad_y * 0.5);
                    cursor_y += line_h_mono + bubble_pad_y + bubble_gap;
                }
            }

            ChatRole::Thinking => {
                // Collapsed (default): one-line stub "▶ Thinking".
                // Expanded: header + full italic thinking text.
                let is_expanded = expanded.contains(&(leaf_id, msg_i));
                let toggle_char = if is_expanded { '▼' } else { '▶' };
                let thinking_text_x = x + 20.0; // indented past the toggle glyph
                let header_text_x = x + 8.0;

                ctx.set_font("italic 12px sans-serif");

                if is_expanded {
                    let header = format!("{} Thinking", toggle_char);
                    let lines = word_wrap_text(ctx, &msg.content, max_bubble_w - 16.0);
                    let n_lines = lines.len();
                    let text_h = (n_lines + 1) as f64 * line_h_normal; // +1 for header line

                    // Header line rect (font_tag=2: italic 12px).
                    let header_y = cursor_y;
                    line_rects_out.push((msg_i, 0, header_y, header_y + line_h_normal, leaf_id, header.clone(), header_text_x, 2u8));

                    // Content line rects.
                    for (li, line_text) in lines.iter().enumerate() {
                        let line_y = cursor_y + (li + 1) as f64 * line_h_normal;
                        line_rects_out.push((msg_i, (li + 1) as u16, line_y, line_y + line_h_normal, leaf_id, line_text.clone(), thinking_text_x, 2u8));
                    }

                    ctx.set_fill_color(&theme.item_text_muted);
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Top);
                    ctx.fill_text(&header, header_text_x, cursor_y);
                    for (li, line) in lines.iter().enumerate() {
                        ctx.fill_text(line, thinking_text_x, cursor_y + (li + 1) as f64 * line_h_normal);
                    }
                    cursor_y += text_h + bubble_gap;
                } else {
                    // Collapsed: one-line stub.
                    let stub = format!("{} Thinking", toggle_char);
                    let stub_y = cursor_y;
                    line_rects_out.push((msg_i, 0, stub_y, stub_y + line_h_normal, leaf_id, stub.clone(), header_text_x, 2u8));

                    ctx.set_fill_color(&theme.item_text_muted);
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Top);
                    ctx.fill_text(&stub, header_text_x, cursor_y);
                    cursor_y += line_h_normal + bubble_gap;
                }
            }

            ChatRole::Error => {
                // Red error text.
                ctx.set_font("12px sans-serif");
                let error_text_x = x + 8.0;
                let lines = word_wrap_text(ctx, &msg.content, max_bubble_w);
                let n_lines = lines.len();
                let text_h = n_lines as f64 * line_h_normal;

                // Record line rects for selection hit-testing (font_tag=3: error 12px).
                for (li, line_text) in lines.iter().enumerate() {
                    let line_y = cursor_y + li as f64 * line_h_normal;
                    line_rects_out.push((msg_i, li as u16, line_y, line_y + line_h_normal, leaf_id, line_text.clone(), error_text_x, 3u8));
                }

                ctx.set_fill_color(&theme.danger);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Top);
                for (li, line) in lines.iter().enumerate() {
                    ctx.fill_text(line, x + 8.0, cursor_y + li as f64 * line_h_normal);
                }
                cursor_y += text_h + bubble_gap;
            }
        }

        // Stop rendering if we've exceeded the visible area.
        if cursor_y > y + h {
            break;
        }
    }

    // ── Selection overlay ──────────────────────────────────────────────────
    if let Some(sel) = selection {
        if !sel.is_empty() {
            let ((lo_msg, lo_line, lo_char), (hi_msg, hi_line, hi_char)) = sel.ordered();
            ctx.set_fill_color(&theme.selection);
            ctx.set_global_alpha(0.35);
            for (msg_i, line_i, line_y, line_y_bot, _lid, line_text, text_x, font_tag) in line_rects_out.iter() {
                let (msg_i, line_i) = (*msg_i, *line_i);
                let (line_y, line_y_bot) = (*line_y, *line_y_bot);
                let text_x = *text_x;
                let line_h = line_y_bot - line_y;

                // Only process lines within the selection range.
                let pos = (msg_i, line_i);
                if pos < (lo_msg, lo_line) || pos > (hi_msg, hi_line) {
                    continue;
                }

                // Set the correct font for measurement.
                match font_tag {
                    1 => ctx.set_font("11px JetBrainsMono"),
                    2 => ctx.set_font("italic 12px sans-serif"),
                    3 => ctx.set_font("12px sans-serif"),
                    _ => ctx.set_font("13px sans-serif"),
                }

                let is_first = pos == (lo_msg, lo_line);
                let is_last = pos == (hi_msg, hi_line);

                // Helper: convert char index to byte-safe substring.
                let char_prefix = |n: usize| -> String {
                    line_text.chars().take(n).collect()
                };

                let (sel_x, sel_w) = if is_first && is_last {
                    // Selection starts and ends on the same line.
                    let prefix = char_prefix(lo_char as usize);
                    let selected = char_prefix(hi_char as usize);
                    let prefix_w = ctx.measure_text(&prefix);
                    let selected_w = (ctx.measure_text(&selected) - prefix_w).max(0.0);
                    (text_x + prefix_w, selected_w)
                } else if is_first {
                    // First line: from start_char to end of line.
                    let prefix = char_prefix(lo_char as usize);
                    let prefix_w = ctx.measure_text(&prefix);
                    let full_w = ctx.measure_text(line_text);
                    (text_x + prefix_w, (full_w - prefix_w).max(4.0))
                } else if is_last {
                    // Last line: from start of line to end_char.
                    let selected = char_prefix(hi_char as usize);
                    let selected_w = ctx.measure_text(&selected).max(4.0);
                    (text_x, selected_w)
                } else {
                    // Middle line: full line width.
                    let full_w = ctx.measure_text(line_text).max(4.0);
                    (text_x, full_w)
                };

                if sel_w > 0.0 {
                    ctx.fill_rect(sel_x, line_y, sel_w, line_h);
                }
            }
            ctx.set_global_alpha(1.0);
        }
    }

    // ── Live-status spinner line ──────────────────────────────────────────
    // Drawn after the message loop so it always appears below the last message.
    if let Some(snap) = snapshot {
        use crate::agent_types::LiveStatus;
        if snap.live_status != LiveStatus::Idle {
            // Braille spinner frames — all 10 Unicode braille "loading" glyphs.
            const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let frame_char = FRAMES[((now_ms / 80) as usize) % FRAMES.len()];
            let status_text = match &snap.live_status {
                LiveStatus::Thinking => format!("{} Thinking...", frame_char),
                LiveStatus::RunningTool { name, done } => format!("{} {} · {} done", frame_char, name, done),
                LiveStatus::Idle => String::new(),
            };
            if !status_text.is_empty() {
                ctx.set_font("11px JetBrainsMono");
                ctx.set_fill_color(&theme.item_text_muted);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Top);
                ctx.fill_text(&status_text, x + 8.0, cursor_y);
            }
        }
    }

    // total_content_h is passed in as a parameter (pre-computed via compute_chat_content_height)
    // so the scrollbar handle size stays stable even when the loop breaks early due to scroll.
    let sb_rects = {
        let sb_w = 6.0;
        let sb_rect = uzor::types::Rect::new(scrollbar_x, y, sb_w, h);
        let sb_config = ScrollbarConfig::new(total_content_h, h, scroll_offset);
        let sb_state = SbState::Active;
        let widget_theme = zengeld_chart::ui::widgets::types::WidgetTheme::default();
        let sb_result = draw_scrollbar(ctx, &sb_config, sb_state, sb_rect, &widget_theme, None);
        if sb_config.needs_scrollbar() {
            Some((sb_result.handle_rect, sb_result.track_rect))
        } else {
            None
        }
    };

    ctx.restore();
    sb_rects
}

// =============================================================================
// Agent rendering helpers
// =============================================================================

/// Compute the total content height of the chat message list without rendering.
///
/// Uses the same layout metrics as `render_agents_chat`.  Mutably borrows `ctx`
/// only to set fonts for measurement — no drawing commands are issued.
fn compute_chat_content_height(
    ctx: &mut dyn RenderContext,
    snapshot: Option<&crate::agent_types::AgentRenderSnapshot>,
    w: f64,
    expanded: &std::collections::HashSet<(uzor::panels::LeafId, u16)>,
    leaf_id: uzor::panels::LeafId,
) -> f64 {
    use crate::agent_types::{AgentSnapshotMode, ChatRole};

    let messages = match snapshot.and_then(|s| {
        if let AgentSnapshotMode::Chat(ref msgs) = s.mode { Some(msgs) } else { None }
    }) {
        Some(m) => m,
        None => return 0.0,
    };

    if messages.is_empty() {
        return 0.0;
    }

    let bubble_pad_x = 8.0;
    let bubble_pad_y = 5.0;
    let bubble_gap = 6.0;
    let max_bubble_w = w - 24.0;
    let line_h_normal = 17.0;
    let line_h_mono = 14.0;

    let mut total_h = 8.0; // top padding

    for (msg_idx, msg) in messages.iter().enumerate() {
        let msg_i = msg_idx as u16;
        match msg.role {
            ChatRole::User => {
                // Fit-content width bubble: wrap at max_bubble_w, measure widest line.
                ctx.set_font("13px sans-serif");
                let lines = word_wrap_text(ctx, &msg.content, max_bubble_w - bubble_pad_x * 2.0);
                let n_lines = lines.len().max(1);
                let bubble_h = n_lines as f64 * line_h_normal + bubble_pad_y * 2.0;
                total_h += bubble_h + bubble_gap;
            }
            ChatRole::Assistant => {
                ctx.set_font("13px sans-serif");
                let lines = word_wrap_text(ctx, &msg.content, max_bubble_w - bubble_pad_x * 2.0);
                let text_h = lines.len() as f64 * line_h_normal;
                total_h += text_h + bubble_gap;
            }
            ChatRole::Tool => {
                ctx.set_font("bold 11px JetBrainsMono");
                if expanded.contains(&(leaf_id, msg_i)) {
                    let lines = word_wrap_text(ctx, &msg.content, max_bubble_w - bubble_pad_x * 2.0);
                    let n_lines = lines.len().max(1);
                    let bubble_h = line_h_mono + n_lines as f64 * line_h_mono + bubble_pad_y * 2.0;
                    total_h += bubble_h + bubble_gap;
                } else {
                    total_h += line_h_mono + bubble_pad_y + bubble_gap;
                }
            }
            ChatRole::Thinking => {
                ctx.set_font("italic 12px sans-serif");
                if expanded.contains(&(leaf_id, msg_i)) {
                    let lines = word_wrap_text(ctx, &msg.content, max_bubble_w - 16.0);
                    let text_h = (lines.len() + 1) as f64 * line_h_normal; // +1 for header line
                    total_h += text_h + bubble_gap;
                } else {
                    total_h += line_h_normal + bubble_gap;
                }
            }
            ChatRole::Error => {
                ctx.set_font("12px sans-serif");
                let lines = word_wrap_text(ctx, &msg.content, max_bubble_w);
                let text_h = lines.len() as f64 * line_h_normal;
                total_h += text_h + bubble_gap;
            }
        }
    }

    // Add space for the live-status spinner line when not Idle.
    let live_status_h = if let Some(snap) = snapshot {
        use crate::agent_types::LiveStatus;
        if snap.live_status != LiveStatus::Idle { 22.0 } else { 0.0 }
    } else {
        0.0
    };

    total_h + 8.0 + live_status_h // bottom padding
}


/// Convert an RGB triple to a CSS hex color string like `"#rrggbb"`.
fn rgb_to_hex(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

/// Simple word-wrap: split `text` into lines that each fit within `max_w` px.
///
/// Splits on space boundaries.  Words wider than `max_w` are placed on their
/// own line without further splitting.  The current font on `ctx` is used for
/// measurement — set font BEFORE calling this function.
fn word_wrap_text(ctx: &dyn RenderContext, text: &str, max_w: f64) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines: Vec<String> = Vec::new();
    for paragraph in text.split('\n') {
        let words: Vec<&str> = paragraph.split(' ').collect();
        let mut current = String::new();
        for word in &words {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{} {}", current, word)
            };
            if ctx.measure_text(&candidate) <= max_w {
                current = candidate;
            } else {
                if !current.is_empty() {
                    lines.push(current);
                }
                // Break long words that exceed max_w by themselves.
                if ctx.measure_text(word) > max_w {
                    let mut buf = String::new();
                    for ch in word.chars() {
                        buf.push(ch);
                        if ctx.measure_text(&buf) > max_w {
                            buf.pop();
                            if !buf.is_empty() { lines.push(buf); }
                            buf = ch.to_string();
                        }
                    }
                    current = buf;
                } else {
                    current = word.to_string();
                }
            }
        }
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Return tooltip text for agent panel widget IDs.
pub fn find_agent_tooltip(widget_id: &str) -> Option<&'static str> {
    match widget_id {
        "agent:mode:pty" => Some("Terminal mode (PTY)"),
        "agent:mode:chat" => Some("Chat mode"),
        "agent:spawn:claude" => Some("Open Claude session"),
        "agent:spawn:codex" => Some("Open Codex session"),
        "agent:spawn:gemini" => Some("Open Gemini session"),
        "agent:spawn:opencode" => Some("Open OpenCode session"),
        "agent:split:h" => Some("Split horizontal"),
        "agent:split:v" => Some("Split vertical"),
        "agent:split:replace" => Some("Replace focused pane"),
        "agent:expand_toggle" => Some("Expand / Collapse"),
        "agent:reset_sizes" => Some("Reset pane sizes"),
        "agent:close_pane" => Some("Close focused pane"),
        _ => None,
    }
}
