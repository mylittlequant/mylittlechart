//! Template name input modal renderer.
//!
//! Shown as an overlay when the user clicks "Save As..." in any of the three
//! settings-modal template dropdowns (primitive, indicator, compare).
//!
//! Text is owned by the uzor `TextFieldStore` (keyed by `field_id`).
//! Rendered on `ZLayer::ModalOverlay` so it appears above the parent settings modal.

use mlc_core::engine::render::RenderContext;
use uzor::types::Rect as WidgetRect;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{ModalFooter, ModalShellSpec, ModalTheme, render_modal_shell};
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig};
use crate::ui::widgets::types::WidgetState;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::layout::render_chart::FrameTheme;
use crate::ui::z_order::ZLayer;
use mlc_core::i18n::{ModalKey, t_modal};

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
/// `field_id` — the `TextFieldStore` key whose text, cursor, and selection are
/// rendered inside the input box.  The field must already be registered and
/// focused by the caller before this function is called.
///
/// This one dims the screen and cannot be dragged, both deliberately: it opens
/// ON TOP of the settings modal that asked for it, so there is nowhere useful
/// to drag it to and nothing behind it the user is reading. Every other
/// settings dialog is the opposite case and dims nothing.
///
/// Returns hit-test rectangles the caller uses for rendering the blinking
/// cursor and for input handling.
pub fn render_template_name_modal(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    field_id: &str,
    ns_prefix: &str,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> TemplateNameModalResult {
    let mut result = TemplateNameModalResult::default();

    const INPUT_H: f64 = 30.0;

    let spec = ModalShellSpec {
        draggable: false,
        dim_backdrop: true,
        ..ModalShellSpec::new(ns_prefix, t_modal(ModalKey::SaveTemplateAs), 360.0, INPUT_H)
            .on_layer(ZLayer::ModalOverlay)
            .with_footer(ModalFooter::Creator {
                label: t_modal(ModalKey::SaveTemplate),
            })
    };

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

            let wid = uzor::WidgetId::from(field_id);
            let display_text = coord.text_fields().text(&wid).to_string();
            let cursor_pos = coord.text_fields().cursor(&wid);
            let (sel_start, sel_end) = match coord.text_fields().selection_range(&wid) {
                Some((lo, hi)) => (Some(lo), Some(hi)),
                None => (None, None),
            };

            let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
            let engaged = coord.text_fields().is_engaged(&wid);
            let config = InputConfig::new(&display_text)
                .with_focused(engaged)
                .with_cursor(cursor_pos)
                .with_placeholder(t_modal(ModalKey::TemplateNamePlaceholder))
                .with_selection(sel_start, sel_end);

            let drawn = draw_input(ctx, &config, WidgetState::Normal, input_rect, &widget_theme);
            result.input_text_rect = drawn.text_rect;
            result.char_x_positions = drawn.char_x_positions;

            coord.register_on_layer(
                format!("{ns_prefix}:input"),
                input_rect,
                uzor::input::Sense::CLICK,
                layer_id,
            );

            if engaged && coord.text_fields().cursor_visible(current_time_ms) {
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
    result.close_btn_rect = shell.close_btn.unwrap_or_default();
    result.save_btn_rect = shell.primary_btn.unwrap_or_default();
    result
}
