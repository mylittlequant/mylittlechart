//! Template name input modal renderer.
//!
//! Shown as an overlay when the user clicks "Save As..." in any of the three
//! settings-modal template dropdowns (primitive, indicator, compare).
//!
//! Reuses the `TextEditingState` already stored in the three settings states
//! (`template_name_editing`).  Rendered on `ZLayer::ModalOverlay` so it
//! appears above the parent settings modal.

use crate::engine::render::{RenderContext, draw_svg_icon};
use uzor::render::{TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;
use crate::ui::modal_settings::TextEditingState;
use crate::ui::toolbar_render::ToolbarTheme;
use crate::ui::widgets::{render_modal_frame_only, ModalTheme};
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig};
use crate::ui::widgets::types::WidgetState;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::layout::render_chart::FrameTheme;
use crate::ui::z_order::ZLayer;
use crate::ui::Icon;
use crate::i18n::{ModalKey, TextKey, t_modal, t_text};

/// Hit-test rectangles returned from a `render_template_name_modal` call.
#[derive(Clone, Debug, Default)]
pub struct TemplateNameModalResult {
    /// The modal rectangle (absorbs all clicks that don't hit a child).
    pub modal_rect: WidgetRect,
    /// The text input rectangle.
    pub input_rect: WidgetRect,
    /// Text area used for click-to-cursor calculation.
    pub input_text_rect: WidgetRect,
    /// Pre-computed character x-positions for click-to-cursor.
    pub char_x_positions: Vec<f64>,
    /// "Save" button rectangle.
    pub save_btn_rect: WidgetRect,
    /// "Cancel" button rectangle.
    pub cancel_btn_rect: WidgetRect,
    /// Close (X) button rectangle.
    pub close_btn_rect: WidgetRect,
}

/// Render the "Save As Template" overlay modal.
///
/// `ns_prefix` — namespace prefix for input-coordinator widget IDs, e.g.
/// `"prim_tmpl"`, `"ind_tmpl"`, or `"cmp_tmpl"`.  All registered widget IDs
/// have the form `"{ns_prefix}:{id}"`.
///
/// Returns hit-test rectangles the caller uses for rendering the blinking
/// cursor and for input handling.
pub fn render_template_name_modal(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    editing: &TextEditingState,
    ns_prefix: &str,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> TemplateNameModalResult {
    let mut result = TemplateNameModalResult::default();

    let modal_w = 360.0_f64;
    let modal_h = 160.0_f64;
    let header_h = 40.0_f64;
    let footer_h = 48.0_f64;
    let padding = 14.0_f64;
    let input_h = 30.0_f64;
    let btn_h = 28.0_f64;
    let btn_w = 80.0_f64;
    let btn_gap = 8.0_f64;

    // Centered on screen.
    let modal_x = ((screen_w - modal_w) / 2.0).max(0.0).min(screen_w - modal_w);
    let modal_y = ((screen_h - modal_h) / 2.0).max(0.0).min(screen_h - modal_h);

    let modal_rect = WidgetRect::new(modal_x, modal_y, modal_w, modal_h);
    result.modal_rect = modal_rect;

    // --- Draw dim backdrop ---
    ctx.set_fill_color("rgba(0,0,0,0.45)");
    ctx.fill_rect(0.0, 0.0, screen_w, screen_h);

    // --- Modal frame ---
    let modal_theme = ModalTheme::from_frame_theme(
        &frame_theme.toolbar_bg,
        &toolbar_theme.separator,
        &toolbar_theme.item_text,
        &toolbar_theme.item_text_hover,
        &toolbar_theme.separator,
    );
    render_modal_frame_only(ctx, modal_rect, &modal_theme, 0.0);

    // --- InputCoordinator layer (ModalOverlay so it sits above settings modal) ---
    let layer_id = ZLayer::ModalOverlay.push_named(input_coordinator, ns_prefix);

    // Register modal background (absorb clicks that don't hit children).
    input_coordinator.register_on_layer(
        format!("{}:modal_bg", ns_prefix),
        WidgetRect::new(modal_x, modal_y, modal_w, modal_h),
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    // --- Header ---
    ctx.set_font("bold 13px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_modal(ModalKey::SaveTemplateAs), modal_x + padding, modal_y + header_h / 2.0);

    // Close (X) button — right side of header.
    let close_size = 16.0;
    let close_x = modal_x + modal_w - close_size - 10.0;
    let close_y = modal_y + (header_h - close_size) / 2.0;
    let close_rect = WidgetRect::new(close_x, close_y, close_size, close_size);
    result.close_btn_rect = close_rect;
    draw_svg_icon(ctx, Icon::Close.svg(), close_x, close_y, close_size, close_size, &toolbar_theme.item_text_muted);

    input_coordinator.register_on_layer(
        format!("{}:close", ns_prefix),
        close_rect,
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    // Header separator.
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, modal_y + header_h);
    ctx.line_to(modal_x + modal_w, modal_y + header_h);
    ctx.stroke();

    // --- Content: text input ---
    let content_y = modal_y + header_h + (modal_h - header_h - footer_h - input_h) / 2.0;
    let input_rect = WidgetRect::new(
        modal_x + padding,
        content_y,
        modal_w - padding * 2.0,
        input_h,
    );
    result.input_rect = input_rect;

    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let (sel_start, sel_end) = if let Some((lo, hi)) = editing.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    let input_config = InputConfig::new(&editing.text)
        .with_focused(true)
        .with_cursor(editing.cursor)
        .with_placeholder(t_modal(ModalKey::TemplateNamePlaceholder))
        .with_selection(sel_start, sel_end);

    let input_result = draw_input(ctx, &input_config, WidgetState::Normal, input_rect, &widget_theme);
    result.input_text_rect = input_result.text_rect;
    result.char_x_positions = input_result.char_x_positions;

    input_coordinator.register_on_layer(
        format!("{}:input", ns_prefix),
        input_rect,
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    // Blinking cursor.
    if editing.is_cursor_visible(current_time_ms) {
        draw_input_cursor(
            ctx,
            input_result.cursor_x,
            input_result.cursor_y,
            input_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }

    // --- Footer separator ---
    let footer_y = modal_y + modal_h - footer_h;
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, footer_y);
    ctx.line_to(modal_x + modal_w, footer_y);
    ctx.stroke();

    // --- Footer buttons (right-aligned) ---
    let btns_y = footer_y + (footer_h - btn_h) / 2.0;
    let cancel_x = modal_x + modal_w - padding - btn_w;
    let save_x = cancel_x - btn_gap - btn_w;

    // "Сохранить" button (primary).
    let save_rect = WidgetRect::new(save_x, btns_y, btn_w, btn_h);
    result.save_btn_rect = save_rect;
    ctx.set_fill_color("#2962ff");
    ctx.fill_rounded_rect(save_rect.x, save_rect.y, save_rect.width, save_rect.height, 4.0);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color("#ffffff");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_modal(ModalKey::SaveTemplate), save_rect.center_x(), save_rect.center_y());

    input_coordinator.register_on_layer(
        format!("{}:save", ns_prefix),
        save_rect,
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    // "Отмена" button (secondary).
    let cancel_rect = WidgetRect::new(cancel_x, btns_y, btn_w, btn_h);
    result.cancel_btn_rect = cancel_rect;
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(cancel_rect.x, cancel_rect.y, cancel_rect.width, cancel_rect.height, 4.0);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_text(TextKey::Cancel), cancel_rect.center_x(), cancel_rect.center_y());

    input_coordinator.register_on_layer(
        format!("{}:cancel", ns_prefix),
        cancel_rect,
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    input_coordinator.pop_layer(&layer_id);

    result
}
