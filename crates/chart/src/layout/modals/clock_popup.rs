//! Clock popup renderer — timezone selector + 24h toggle.
//!
//! Opens when the user clicks the clock widget in the bottom toolbar.
//! Positioned flush with the right edge and above the bottom toolbar.

use crate::engine::render::RenderContext;
use crate::i18n::{current_language, ClockKey};
use crate::layout::render_chart::FrameTheme;
use crate::scale_settings::TimeFormatSettings;
use crate::ui::toolbar_render::ToolbarTheme;
use crate::ui::z_order::ZLayer;
use uzor::render::{TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;

// =============================================================================
// Result type
// =============================================================================

/// Render result from the clock popup.
#[derive(Clone, Debug, Default)]
pub struct ClockPopupResult {
    /// The popup outer rectangle.
    pub popup_rect: WidgetRect,
}

// =============================================================================
// Layout constants
// =============================================================================

const POPUP_WIDTH: f64 = 200.0;
const HEADER_H: f64 = 36.0;
const ITEM_H: f64 = 24.0;
const SEPARATOR_H: f64 = 13.0; // 4px gap + 1px line + 8px gap
const CHECKBOX_AREA_H: f64 = 24.0;
const CHECKBOX_SIZE: f64 = 14.0;
const TZ_COUNT: f64 = 25.0; // offsets -12..=+12

/// Total popup height: header + TZ list + separator + two checkboxes.
const POPUP_HEIGHT: f64 = HEADER_H + TZ_COUNT * ITEM_H + SEPARATOR_H + CHECKBOX_AREA_H * 2.0;

// =============================================================================
// Renderer
// =============================================================================

/// Render the clock timezone/format popup.
///
/// Returns hit-zone information for click dispatch.
pub fn render_clock_popup(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    _screen_h: f64,
    bottom_toolbar_y: f64,
    active_offset: i32,
    use_24h: bool,
    show_utc: bool,
    hovered_item: Option<&str>,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> ClockPopupResult {
    let mut result = ClockPopupResult::default();

    // -------------------------------------------------------------------------
    // Geometry
    // -------------------------------------------------------------------------
    let popup_x = screen_w - POPUP_WIDTH;
    let popup_y = (bottom_toolbar_y - POPUP_HEIGHT).max(0.0);

    result.popup_rect = WidgetRect::new(popup_x, popup_y, POPUP_WIDTH, POPUP_HEIGHT);

    // -------------------------------------------------------------------------
    // Background + border
    // -------------------------------------------------------------------------
    ctx.set_fill_color(&frame_theme.toolbar_bg);
    ctx.fill_rounded_rect(popup_x, popup_y, POPUP_WIDTH, POPUP_HEIGHT, 4.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(popup_x, popup_y, POPUP_WIDTH, POPUP_HEIGHT, 4.0);

    // -------------------------------------------------------------------------
    // InputCoordinator layer
    // -------------------------------------------------------------------------
    let layer_id = ZLayer::ClockPopup.push(input_coordinator);

    // Full-screen backdrop to close popup when clicking outside.
    input_coordinator.register_on_layer(
        "clock_popup:bg",
        uzor::types::Rect::new(0.0, 0.0, 100_000.0, 100_000.0),
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    // -------------------------------------------------------------------------
    // Header
    // -------------------------------------------------------------------------
    ctx.set_font("bold 13px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(
        ClockKey::Timezone.get(current_language()),
        popup_x + 12.0,
        popup_y + HEADER_H / 2.0,
    );

    // -------------------------------------------------------------------------
    // Timezone list — +12 at top, -12 at bottom
    // -------------------------------------------------------------------------
    let mut cy = popup_y + HEADER_H;
    for offset in (-12..=12_i32).rev() {
        let city = TimeFormatSettings::city_for_offset(offset);
        let label = if offset >= 0 {
            format!("UTC+{}  {}", offset, city)
        } else {
            format!("UTC{}  {}", offset, city)
        };

        let item_id_owned = format!("clock_popup:tz:{}", offset);

        let is_active = offset == active_offset;
        let is_hovered = hovered_item == Some(item_id_owned.as_str());

        // Item background
        if is_active {
            ctx.set_fill_color(toolbar_theme.item_bg_active.as_str());
            ctx.fill_rect(popup_x + 4.0, cy, POPUP_WIDTH - 8.0, ITEM_H);
        } else if is_hovered {
            ctx.set_fill_color(toolbar_theme.item_bg_hover.as_str());
            ctx.fill_rect(popup_x + 4.0, cy, POPUP_WIDTH - 8.0, ITEM_H);
        }

        // Item text — keep primary color on the active row; only the
        // background changes, so the label stays readable on the accent fill.
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(toolbar_theme.item_text.as_str());
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&label, popup_x + 12.0, cy + ITEM_H / 2.0);

        // Hit zone
        input_coordinator.register_on_layer(
            item_id_owned.as_str(),
            uzor::types::Rect::new(popup_x + 4.0, cy, POPUP_WIDTH - 8.0, ITEM_H),
            uzor::input::Sense::CLICK,
            &layer_id,
        );

        cy += ITEM_H;
    }

    // -------------------------------------------------------------------------
    // Separator
    // -------------------------------------------------------------------------
    // 4px gap
    cy += 4.0;
    ctx.set_fill_color(&toolbar_theme.separator);
    ctx.fill_rect(popup_x + 8.0, cy, POPUP_WIDTH - 16.0, 1.0);
    cy += 1.0;
    // 8px gap
    cy += 8.0;

    // -------------------------------------------------------------------------
    // Checkbox "24-часовой формат"
    // -------------------------------------------------------------------------
    let cb_x = popup_x + 12.0;
    let cb_cy = cy + CHECKBOX_AREA_H / 2.0;
    let cb_top = cb_cy - CHECKBOX_SIZE / 2.0;

    // Checkbox square
    if use_24h {
        ctx.set_fill_color(toolbar_theme.accent.as_str());
    } else {
        ctx.set_fill_color(&frame_theme.toolbar_bg);
    }
    ctx.fill_rounded_rect(cb_x, cb_top, CHECKBOX_SIZE, CHECKBOX_SIZE, 3.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(cb_x, cb_top, CHECKBOX_SIZE, CHECKBOX_SIZE, 3.0);

    // Checkmark (white, 2px stroke) when checked
    if use_24h {
        ctx.set_stroke_color("#ffffff");
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.move_to(cb_x + 2.0, cb_top + CHECKBOX_SIZE / 2.0);
        ctx.line_to(cb_x + CHECKBOX_SIZE / 2.0 - 1.0, cb_top + CHECKBOX_SIZE - 3.0);
        ctx.line_to(cb_x + CHECKBOX_SIZE - 2.0, cb_top + 3.0);
        ctx.stroke();
        ctx.set_stroke_width(1.0);
    }

    // Checkbox label
    let cb_text_x = cb_x + CHECKBOX_SIZE + 8.0;
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(ClockKey::Use24h.get(current_language()), cb_text_x, cb_cy);

    // Hit zone for checkbox row
    input_coordinator.register_on_layer(
        "clock_popup:clock:use_24h",
        uzor::types::Rect::new(popup_x + 4.0, cy, POPUP_WIDTH - 8.0, CHECKBOX_AREA_H),
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    cy += CHECKBOX_AREA_H;

    // -------------------------------------------------------------------------
    // Checkbox "Показывать UTC"
    // -------------------------------------------------------------------------
    let cb2_cy = cy + CHECKBOX_AREA_H / 2.0;
    let cb2_top = cb2_cy - CHECKBOX_SIZE / 2.0;

    // Checkbox square
    if show_utc {
        ctx.set_fill_color(toolbar_theme.accent.as_str());
    } else {
        ctx.set_fill_color(&frame_theme.toolbar_bg);
    }
    ctx.fill_rounded_rect(cb_x, cb2_top, CHECKBOX_SIZE, CHECKBOX_SIZE, 3.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(cb_x, cb2_top, CHECKBOX_SIZE, CHECKBOX_SIZE, 3.0);

    // Checkmark (white, 2px stroke) when checked
    if show_utc {
        ctx.set_stroke_color("#ffffff");
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.move_to(cb_x + 2.0, cb2_top + CHECKBOX_SIZE / 2.0);
        ctx.line_to(cb_x + CHECKBOX_SIZE / 2.0 - 1.0, cb2_top + CHECKBOX_SIZE - 3.0);
        ctx.line_to(cb_x + CHECKBOX_SIZE - 2.0, cb2_top + 3.0);
        ctx.stroke();
        ctx.set_stroke_width(1.0);
    }

    // Checkbox label
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(
        ClockKey::ShowUtcPrefix.get(current_language()),
        cb_text_x,
        cb2_cy,
    );

    // Hit zone for show_utc checkbox row
    input_coordinator.register_on_layer(
        "clock_popup:show_utc",
        uzor::types::Rect::new(popup_x + 4.0, cy, POPUP_WIDTH - 8.0, CHECKBOX_AREA_H),
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    result
}
