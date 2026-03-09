//! Chart browser modal renderer — scrollable list of all saved presets.
//!
//! Shown when the user selects "Open Chart..." from the preset dropdown.
//! Allows searching, loading, renaming, and deleting presets.

use crate::engine::render::RenderContext;
use crate::engine::render::draw_svg_icon;
use uzor::render::{TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;
use crate::ui::modal_settings::ChartBrowserState;
use crate::ui::toolbar_render::ToolbarTheme;
use crate::ui::widgets::{render_modal_frame_only, ModalTheme};
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig};
use crate::ui::widgets::types::WidgetState;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::layout::render_chart::FrameTheme;
use crate::ui::z_order::ZLayer;
use crate::ui::Icon;
use crate::preset::preset::ChartPreset;

// =============================================================================
// Result type
// =============================================================================

/// Render result from the chart browser modal.
#[derive(Clone, Debug, Default)]
pub struct ChartBrowserResult {
    /// The modal rectangle (for backdrop hit testing).
    pub modal_rect: WidgetRect,
    /// Header rectangle (title + close button row) — used for drag.
    pub header_rect: WidgetRect,
    /// Close (X) button rectangle.
    pub close_btn_rect: WidgetRect,
    /// Search input field rectangle.
    pub search_input_rect: WidgetRect,
    /// Pre-computed character X positions for click-to-cursor in the search input.
    pub search_char_positions: Vec<f64>,
    /// Per-item hit zones: (preset_id, item_rect, rename_btn_rect, delete_btn_rect).
    pub item_rects: Vec<(String, WidgetRect, WidgetRect, WidgetRect)>,
    /// Scrollable list viewport rect (used for scroll hit testing).
    pub list_viewport_rect: WidgetRect,
    /// Total height of all items (needed to clamp scroll offset).
    pub total_content_height: f64,
}

// =============================================================================
// Renderer
// =============================================================================

/// Render the chart browser modal.
///
/// Returns hit-zone information used by the input handler for click dispatch,
/// drag, and scroll handling.
pub fn render_chart_browser(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    state: &ChartBrowserState,
    presets: &std::collections::HashMap<String, ChartPreset>,
    active_preset_id: &str,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> ChartBrowserResult {
    let mut result = ChartBrowserResult::default();

    // --- Layout constants ---
    let modal_w = (screen_w * 0.6).min(480.0).max(340.0);
    let modal_h = (screen_h * 0.75).min(500.0).max(280.0);
    let header_h = 44.0;
    let search_h = 40.0;
    let col_header_h = 28.0;
    let padding = 16.0;
    let input_h = 28.0;
    let item_h = 52.0;
    let icon_size = 18.0;
    let icon_btn_size = 24.0;

    // --- Position (draggable, centered by default) ---
    let (modal_x, modal_y) = state.position.unwrap_or_else(|| {
        ((screen_w - modal_w) / 2.0, (screen_h - modal_h) / 2.0)
    });
    let modal_x = modal_x.max(0.0).min(screen_w - modal_w);
    let modal_y = modal_y.max(0.0).min(screen_h - modal_h);

    let modal_rect = WidgetRect::new(modal_x, modal_y, modal_w, modal_h);
    result.modal_rect = modal_rect;

    // --- Modal frame (blur + background + border) ---
    let modal_theme = ModalTheme::from_frame_theme(
        &frame_theme.toolbar_bg,
        &toolbar_theme.separator,
        &toolbar_theme.item_text,
        &toolbar_theme.item_text_hover,
        &toolbar_theme.separator,
    );
    render_modal_frame_only(ctx, modal_rect, &modal_theme, 0.0);

    // --- InputCoordinator layer ---
    let layer_id = ZLayer::Modal.push_named(input_coordinator, "chart_browser");

    // Register modal background (absorbs clicks inside the modal that don't hit a widget)
    input_coordinator.register_on_layer(
        "chart_browser:modal_bg",
        uzor::types::Rect::new(modal_x, modal_y, modal_w, modal_h),
        uzor::input::sense::Sense::CLICK,
        &layer_id,
    );

    // ==========================================================================
    // Header
    // ==========================================================================

    let header_rect = WidgetRect::new(modal_x, modal_y, modal_w, header_h);
    result.header_rect = header_rect;

    // Header background (slightly lighter than modal bg)
    ctx.set_fill_color(&frame_theme.toolbar_bg);
    ctx.fill_rect(modal_x, modal_y, modal_w, header_h);

    // Title
    ctx.set_font("bold 14px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Charts", modal_x + padding, modal_y + header_h / 2.0);

    // Close button (X)
    let close_x = modal_x + modal_w - icon_size - 12.0;
    let close_y = modal_y + (header_h - icon_size) / 2.0;
    let close_rect = WidgetRect::new(close_x, close_y, icon_size, icon_size);
    result.close_btn_rect = close_rect;

    draw_svg_icon(ctx, Icon::Close.svg(), close_x, close_y, icon_size, icon_size, &toolbar_theme.item_text);

    input_coordinator.register_on_layer(
        "chart_browser:close",
        uzor::types::Rect::new(close_x, close_y, icon_size, icon_size),
        uzor::input::sense::Sense::CLICK,
        &layer_id,
    );

    // Header separator
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, modal_y + header_h);
    ctx.line_to(modal_x + modal_w, modal_y + header_h);
    ctx.stroke();

    // ==========================================================================
    // Search row
    // ==========================================================================

    let search_row_y = modal_y + header_h;
    let search_input_x = modal_x + padding;
    let search_input_w = modal_w - padding * 2.0;
    let search_input_y = search_row_y + (search_h - input_h) / 2.0;
    let search_input_rect = WidgetRect::new(search_input_x, search_input_y, search_input_w, input_h);
    result.search_input_rect = search_input_rect;

    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let input_config = InputConfig::new(&state.search_editing.text)
        .with_focused(true)
        .with_cursor(state.search_editing.cursor)
        .with_placeholder("Search charts...")
        .with_padding(26.0);  // Leave room for the search icon on left

    let input_result = draw_input(ctx, &input_config, WidgetState::Normal, search_input_rect, &widget_theme);
    result.search_char_positions = input_result.char_x_positions;

    // Search icon inside the input
    let search_icon_size = 13.0;
    draw_svg_icon(
        ctx, Icon::Search.svg(),
        search_input_x + 7.0,
        search_input_y + (input_h - search_icon_size) / 2.0,
        search_icon_size, search_icon_size,
        &toolbar_theme.item_text_muted,
    );

    // Register search input for click-to-cursor
    input_coordinator.register_on_layer(
        "chart_browser:search_input",
        uzor::types::Rect::new(search_input_x, search_input_y, search_input_w, input_h),
        uzor::input::sense::Sense::CLICK,
        &layer_id,
    );

    // Blinking cursor in search input
    if state.search_editing.is_cursor_visible(current_time_ms) {
        draw_input_cursor(
            ctx,
            input_result.cursor_x,
            input_result.cursor_y,
            input_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }

    // Search row bottom separator
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, search_row_y + search_h);
    ctx.line_to(modal_x + modal_w, search_row_y + search_h);
    ctx.stroke();

    // ==========================================================================
    // Column header
    // ==========================================================================

    let col_header_y = search_row_y + search_h;

    ctx.set_font("bold 11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("CHART NAME", modal_x + padding, col_header_y + col_header_h / 2.0);

    // Column header bottom separator
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, col_header_y + col_header_h);
    ctx.line_to(modal_x + modal_w, col_header_y + col_header_h);
    ctx.stroke();

    // ==========================================================================
    // Preset list (scrollable)
    // ==========================================================================

    let list_top = col_header_y + col_header_h;
    let list_h = modal_y + modal_h - list_top;
    let list_viewport_rect = WidgetRect::new(modal_x, list_top, modal_w, list_h);
    result.list_viewport_rect = list_viewport_rect;

    // --- Filter and sort presets ---
    let query = state.search_query.to_lowercase();
    let mut sorted_presets: Vec<&ChartPreset> = presets
        .values()
        .filter(|p| p.id != "__default__")
        .filter(|p| {
            if query.is_empty() {
                true
            } else {
                p.name.to_lowercase().contains(&query)
            }
        })
        .collect();

    // Sort newest first (descending created_at)
    sorted_presets.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let total_h = sorted_presets.len() as f64 * item_h;
    result.total_content_height = total_h;

    // Clip to viewport
    ctx.save();
    ctx.clip_rect(modal_x, list_top, modal_w, list_h);

    let scroll = state.scroll_offset;
    let mut current_y = list_top - scroll;

    for preset in &sorted_presets {
        // Skip items entirely above the viewport
        if current_y + item_h < list_top {
            // Still push rects for hit testing (at their scrolled positions)
            let item_rect = WidgetRect::new(modal_x, current_y, modal_w, item_h);
            let rename_rect = WidgetRect::new(
                modal_x + modal_w - padding - icon_btn_size * 2.0 - 4.0,
                current_y + (item_h - icon_btn_size) / 2.0,
                icon_btn_size, icon_btn_size,
            );
            let delete_rect = WidgetRect::new(
                modal_x + modal_w - padding - icon_btn_size,
                current_y + (item_h - icon_btn_size) / 2.0,
                icon_btn_size, icon_btn_size,
            );
            result.item_rects.push((preset.id.clone(), item_rect, rename_rect, delete_rect));
            current_y += item_h;
            continue;
        }

        // Skip items entirely below the viewport
        if current_y > list_top + list_h {
            let item_rect = WidgetRect::new(modal_x, current_y, modal_w, item_h);
            let rename_rect = WidgetRect::new(
                modal_x + modal_w - padding - icon_btn_size * 2.0 - 4.0,
                current_y + (item_h - icon_btn_size) / 2.0,
                icon_btn_size, icon_btn_size,
            );
            let delete_rect = WidgetRect::new(
                modal_x + modal_w - padding - icon_btn_size,
                current_y + (item_h - icon_btn_size) / 2.0,
                icon_btn_size, icon_btn_size,
            );
            result.item_rects.push((preset.id.clone(), item_rect, rename_rect, delete_rect));
            current_y += item_h;
            continue;
        }

        let is_active = preset.id == active_preset_id;
        let is_hovered = state.hovered_preset_id.as_deref() == Some(&preset.id);

        // Item background
        if is_active {
            // Left accent bar only (no full-row fill)
            ctx.set_fill_color(&toolbar_theme.accent);
            ctx.fill_rounded_rect(modal_x, current_y + 2.0, 3.0, item_h - 4.0, 1.5);
        } else if is_hovered {
            ctx.draw_hover_rect(modal_x, current_y, modal_w, item_h, &toolbar_theme.item_bg_hover);
        }

        // Text start x (8px extra to clear the accent bar)
        let text_x = modal_x + padding + if is_active { 6.0 } else { 3.0 };

        // Line 1: preset name (bold, 14px)
        let name_color = if is_active { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text };
        ctx.set_font("bold 13px sans-serif");
        ctx.set_fill_color(name_color);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&preset.name, text_x, current_y + 10.0);

        // Line 2: subtitle — use window info if available, else just created date
        let subtitle = build_preset_subtitle(preset);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&subtitle, text_x, current_y + 29.0);

        // Right side: rename + delete icons (only on hover or active)
        let icon_y = current_y + (item_h - icon_btn_size) / 2.0;
        let delete_x = modal_x + modal_w - padding - icon_btn_size;
        let rename_x = delete_x - icon_btn_size - 4.0;

        let rename_rect = WidgetRect::new(rename_x, icon_y, icon_btn_size, icon_btn_size);
        let delete_rect = WidgetRect::new(delete_x, icon_y, icon_btn_size, icon_btn_size);

        if is_hovered || is_active {
            let icon_inner_size = 15.0;
            let icon_x_off = (icon_btn_size - icon_inner_size) / 2.0;

            draw_svg_icon(
                ctx, Icon::Pencil.svg(),
                rename_x + icon_x_off, icon_y + icon_x_off,
                icon_inner_size, icon_inner_size,
                &toolbar_theme.item_text_muted,
            );
            draw_svg_icon(
                ctx, Icon::Delete.svg(),
                delete_x + icon_x_off, icon_y + icon_x_off,
                icon_inner_size, icon_inner_size,
                &toolbar_theme.item_text_muted,
            );
        }

        // Row separator
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(modal_x, current_y + item_h);
        ctx.line_to(modal_x + modal_w, current_y + item_h);
        ctx.stroke();

        let item_rect = WidgetRect::new(modal_x, current_y, modal_w, item_h);
        result.item_rects.push((preset.id.clone(), item_rect, rename_rect, delete_rect));

        current_y += item_h;
    }

    // Empty state
    if sorted_presets.is_empty() {
        let center_x = modal_x + modal_w / 2.0;
        let center_y = list_top + list_h / 2.0;
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        let msg = if state.search_query.is_empty() {
            "No saved charts"
        } else {
            "No charts match your search"
        };
        ctx.fill_text(msg, center_x, center_y);
    }

    ctx.restore();

    // ==========================================================================
    // Register item hit zones with InputCoordinator
    // ==========================================================================

    for (preset_id, item_rect, rename_rect, delete_rect) in &result.item_rects {
        // Main item click (load preset)
        let item_id = format!("chart_browser:item:{}", preset_id);
        input_coordinator.register_on_layer(
            item_id.as_str(),
            uzor::types::Rect::new(item_rect.x, item_rect.y, item_rect.width, item_rect.height),
            uzor::input::sense::Sense::CLICK,
            &layer_id,
        );

        // Rename button
        let rename_id = format!("chart_browser:rename:{}", preset_id);
        input_coordinator.register_on_layer(
            rename_id.as_str(),
            uzor::types::Rect::new(rename_rect.x, rename_rect.y, rename_rect.width, rename_rect.height),
            uzor::input::sense::Sense::CLICK,
            &layer_id,
        );

        // Delete button
        let delete_id = format!("chart_browser:delete:{}", preset_id);
        input_coordinator.register_on_layer(
            delete_id.as_str(),
            uzor::types::Rect::new(delete_rect.x, delete_rect.y, delete_rect.width, delete_rect.height),
            uzor::input::sense::Sense::CLICK,
            &layer_id,
        );
    }

    input_coordinator.pop_layer(&layer_id);
    result
}

// =============================================================================
// Helpers
// =============================================================================

/// Build a subtitle string for a preset row.
///
/// Format: "SYMBOL, TF (DD MMM YYYY, HH:MM)" when window data is available,
/// otherwise just the creation date.
fn build_preset_subtitle(preset: &ChartPreset) -> String {
    // Try to extract symbol and timeframe from the first window snapshot
    let (symbol, tf) = preset.windows.first()
        .map(|w| (w.symbol.as_str(), w.timeframe.name.as_str()))
        .unwrap_or(("", ""));

    let date_str = format_unix_timestamp(preset.created_at);

    if !symbol.is_empty() && !tf.is_empty() {
        format!("{}, {} ({})", symbol, tf, date_str)
    } else {
        date_str
    }
}

/// Format a Unix timestamp (seconds) as "DD Mon YYYY, HH:MM".
fn format_unix_timestamp(secs: u64) -> String {
    // Simple manual formatter — avoids external crate dependency
    const SECS_PER_MIN: u64 = 60;
    const SECS_PER_HOUR: u64 = 3600;
    const SECS_PER_DAY: u64 = 86400;

    let mins = (secs / SECS_PER_MIN) % 60;
    let hours = (secs / SECS_PER_HOUR) % 24;
    let total_days = secs / SECS_PER_DAY;

    // Days since epoch → year/month/day (proleptic Gregorian)
    let (year, month, day) = days_to_ymd(total_days as i64);

    let month_name = match month {
        1  => "Jan", 2  => "Feb", 3  => "Mar",
        4  => "Apr", 5  => "May", 6  => "Jun",
        7  => "Jul", 8  => "Aug", 9  => "Sep",
        10 => "Oct", 11 => "Nov", 12 => "Dec",
        _  => "???",
    };

    format!("{:02} {} {}, {:02}:{:02}", day, month_name, year, hours, mins)
}

/// Convert days since Unix epoch (1970-01-01) to (year, month, day).
fn days_to_ymd(z: i64) -> (i64, u32, u32) {
    // Algorithm from https://howardhinnant.github.io/date_algorithms.html
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}
