//! Preset name input modal renderer — small dialog for naming presets.

use crate::engine::render::RenderContext;
use crate::engine::render::draw_svg_icon;
use uzor::render::{TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;
use crate::ui::modal_settings::{PresetNameInputState, PresetNameInputMode};
use crate::ui::toolbar_render::ToolbarTheme;
use crate::ui::widgets::{render_modal_frame_only, ModalTheme};
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig};
use crate::ui::widgets::types::WidgetState;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::layout::render_chart::FrameTheme;
use crate::ui::z_order::ZLayer;
use crate::ui::Icon;

/// Render result from the preset name input modal.
#[derive(Clone, Debug, Default)]
pub struct PresetNameInputResult {
    /// The modal rectangle (for backdrop hit testing).
    pub modal_rect: WidgetRect,
    /// Header rectangle (title + close button row).
    pub header_rect: WidgetRect,
    /// Close (X) button rectangle.
    pub close_btn_rect: WidgetRect,
    /// "Save" button rectangle.
    pub save_btn_rect: WidgetRect,
    /// "Cancel" button rectangle.
    pub cancel_btn_rect: WidgetRect,
    /// Text input field rectangle (for click-to-focus).
    pub input_rect: WidgetRect,
    /// Text area inside the input (inset by padding) — used for click-to-cursor.
    pub input_text_rect: WidgetRect,
    /// Character X positions for click-to-cursor.
    pub char_x_positions: Vec<f64>,
}

/// Render the preset name input modal dialog.
pub fn render_preset_name_input(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    state: &PresetNameInputState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> PresetNameInputResult {
    let mut result = PresetNameInputResult::default();

    let modal_w = 400.0;
    let modal_h = 170.0;
    let header_h = 44.0;
    let footer_h = 52.0;
    let padding = 16.0;
    let input_h = 32.0;
    let btn_h = 32.0;
    let btn_w = 80.0;
    let btn_gap = 8.0;

    // Position (draggable, centered by default)
    let (modal_x, modal_y) = state.position.unwrap_or_else(|| {
        ((screen_w - modal_w) / 2.0, (screen_h - modal_h) / 2.0)
    });
    let modal_x = modal_x.max(0.0).min(screen_w - modal_w);
    let modal_y = modal_y.max(0.0).min(screen_h - modal_h);

    let modal_rect = WidgetRect::new(modal_x, modal_y, modal_w, modal_h);
    result.modal_rect = modal_rect;

    // Modal frame (blur + background + border)
    let modal_theme = ModalTheme::from_frame_theme(
        &frame_theme.toolbar_bg,
        &toolbar_theme.separator,
        &toolbar_theme.item_text,
        &toolbar_theme.item_text_hover,
        &toolbar_theme.separator,
    );
    render_modal_frame_only(ctx, modal_rect, &modal_theme, 0.0);

    // InputCoordinator layer
    let layer_id = ZLayer::ModalOverlay.push_named(input_coordinator, "preset_name_input");

    // Register modal background (absorbs clicks)
    input_coordinator.register_on_layer(
        "preset_name_input:modal_bg",
        uzor::types::Rect::new(modal_x, modal_y, modal_w, modal_h),
        uzor::input::sense::Sense::CLICK,
        &layer_id,
    );

    // Header
    let header_rect = WidgetRect::new(modal_x, modal_y, modal_w, header_h);
    result.header_rect = header_rect;

    let title = match state.mode {
        PresetNameInputMode::SaveAs => "Save As",
        PresetNameInputMode::Rename => "Rename",
        PresetNameInputMode::NewChart => "New Chart",
        PresetNameInputMode::CreateIndicatorSet => "Create Indicator Set",
    };
    ctx.set_font("bold 14px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(title, modal_x + padding, modal_y + header_h / 2.0);

    // Close button (X) — right side of header
    let close_size = 18.0;
    let close_x = modal_x + modal_w - close_size - 12.0;
    let close_y = modal_y + (header_h - close_size) / 2.0;
    let close_rect = WidgetRect::new(close_x, close_y, close_size, close_size);
    result.close_btn_rect = close_rect;

    draw_svg_icon(ctx, Icon::Close.svg(), close_x, close_y, close_size, close_size, &toolbar_theme.item_text);

    input_coordinator.register_on_layer(
        "preset_name_input:close",
        uzor::types::Rect::new(close_x, close_y, close_size, close_size),
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

    // Content area — text input field
    let content_y = modal_y + header_h + padding;
    let input_rect = WidgetRect::new(
        modal_x + padding,
        content_y,
        modal_w - padding * 2.0,
        input_h,
    );
    result.input_rect = input_rect;

    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let (sel_start, sel_end) = if let Some((lo, hi)) = state.editing.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    let input_config = InputConfig::new(&state.editing.text)
        .with_focused(true)
        .with_cursor(state.editing.cursor)
        .with_placeholder("Preset name...")
        .with_selection(sel_start, sel_end);

    let input_result = draw_input(ctx, &input_config, WidgetState::Normal, input_rect, &widget_theme);
    result.input_text_rect = input_result.text_rect;
    result.char_x_positions = input_result.char_x_positions;

    // Register text input area for click-to-cursor
    input_coordinator.register_on_layer(
        "preset_name_input:input",
        uzor::types::Rect::new(input_rect.x, input_rect.y, input_rect.width, input_rect.height),
        uzor::input::sense::Sense::CLICK,
        &layer_id,
    );

    // Blinking cursor
    if state.editing.is_cursor_visible(current_time_ms) {
        draw_input_cursor(
            ctx,
            input_result.cursor_x,
            input_result.cursor_y,
            input_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }

    // Footer separator
    let footer_y = modal_y + modal_h - footer_h;
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, footer_y);
    ctx.line_to(modal_x + modal_w, footer_y);
    ctx.stroke();

    // Footer buttons — right-aligned
    let btns_y = footer_y + (footer_h - btn_h) / 2.0;
    let cancel_x = modal_x + modal_w - padding - btn_w;
    let save_x = cancel_x - btn_gap - btn_w;

    // "Save" button (primary)
    let save_rect = WidgetRect::new(save_x, btns_y, btn_w, btn_h);
    result.save_btn_rect = save_rect;
    ctx.set_fill_color("#2962ff");
    ctx.fill_rounded_rect(save_rect.x, save_rect.y, save_rect.width, save_rect.height, 4.0);
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color("#ffffff");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Save", save_rect.center_x(), save_rect.center_y());

    input_coordinator.register_on_layer(
        "preset_name_input:save",
        uzor::types::Rect::new(save_rect.x, save_rect.y, save_rect.width, save_rect.height),
        uzor::input::sense::Sense::CLICK,
        &layer_id,
    );

    // "Cancel" button (secondary — themed border + text)
    let cancel_rect = WidgetRect::new(cancel_x, btns_y, btn_w, btn_h);
    result.cancel_btn_rect = cancel_rect;
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(cancel_rect.x, cancel_rect.y, cancel_rect.width, cancel_rect.height, 4.0);
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Cancel", cancel_rect.center_x(), cancel_rect.center_y());

    input_coordinator.register_on_layer(
        "preset_name_input:cancel",
        uzor::types::Rect::new(cancel_rect.x, cancel_rect.y, cancel_rect.width, cancel_rect.height),
        uzor::input::sense::Sense::CLICK,
        &layer_id,
    );

    result
}
