//! Preset name input modal renderer — small dialog for naming presets.

use mlc_core::engine::render::RenderContext;
use uzor::types::Rect as WidgetRect;
use crate::ui::modal_settings::{PresetNameInputState, PresetNameInputMode};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{
    ModalFooter, ModalPlacement, ModalShellSpec, ModalTheme, render_modal_shell,
};
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig};
use crate::ui::widgets::types::WidgetState;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::layout::render_chart::FrameTheme;

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

/// A dialog that names a thing which does not exist yet: one field, and a
/// button that brings it into existence. The ✕ is the abort, so there is no
/// Cancel — `cancel_btn_rect` stays empty and is kept only because callers
/// read the struct.
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

    const INPUT_H: f64 = 32.0;

    let title = match state.mode {
        PresetNameInputMode::SaveAs => "Save As",
        PresetNameInputMode::Rename => "Rename",
        PresetNameInputMode::NewChart => "New Chart",
        PresetNameInputMode::CreateIndicatorSet => "Create Indicator Set",
    };

    let save_label = mlc_core::i18n::TextKey::Save.get(mlc_core::i18n::current_language());
    let spec = ModalShellSpec {
        placement: ModalPlacement::At(state.position),
        ..ModalShellSpec::new("preset_name_input", title, 400.0, INPUT_H)
    }
    .with_footer(ModalFooter::Creator { label: save_label });

    let theme = ModalTheme::from_frame_theme(
        &frame_theme.toolbar_bg,
        &toolbar_theme.separator,
        &toolbar_theme.item_text,
        &toolbar_theme.item_text_hover,
        &toolbar_theme.separator,
    );

    let shell = render_modal_shell(
        ctx,
        input_coordinator,
        &spec,
        &theme,
        (screen_w, screen_h),
        |ctx, coord, layer_id, shell| {
            let input_rect = WidgetRect::new(
                shell.content.x,
                shell.content.y,
                shell.content.width,
                INPUT_H,
            );
            result.input_rect = input_rect;

            // Text, caret and selection all live in the shared TextFieldStore —
            // this modal owns none of them.
            let pni_id = uzor::WidgetId::from("preset_name_input");
            let text = coord.text_fields().text(&pni_id).to_string();
            let cursor = coord.text_fields().cursor(&pni_id);
            let (sel_start, sel_end) = match coord.text_fields().selection_range(&pni_id) {
                Some((lo, hi)) => (Some(lo), Some(hi)),
                None => (None, None),
            };

            let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
            let config = InputConfig::new(&text)
                .with_focused(true)
                .with_cursor(cursor)
                .with_placeholder("Preset name...")
                .with_selection(sel_start, sel_end);

            let drawn = draw_input(ctx, &config, WidgetState::Normal, input_rect, &widget_theme);
            result.input_text_rect = drawn.text_rect;
            result.char_x_positions = drawn.char_x_positions;

            coord.register_on_layer(
                "preset_name_input:input",
                input_rect,
                uzor::input::Sense::CLICK,
                layer_id,
            );

            if coord.text_fields().cursor_visible(current_time_ms) {
                draw_input_cursor(
                    ctx,
                    drawn.cursor_x,
                    drawn.cursor_y,
                    drawn.cursor_height,
                    &toolbar_theme.item_text,
                );
            }
        },
    );

    result.modal_rect = shell.frame;
    result.header_rect = shell.header;
    result.close_btn_rect = shell.close_btn.unwrap_or_default();
    result.save_btn_rect = shell.primary_btn.unwrap_or_default();
    result
}
