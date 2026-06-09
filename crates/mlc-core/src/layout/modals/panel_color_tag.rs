//! Panel color tag picker popup renderer.

use crate::engine::render::RenderContext;
use crate::ui::toolbar_render::ToolbarTheme;
use crate::layout::render_frame::ColorPickerRenderResult;

/// Render color picker popup for panel color tags (L1 or L2 based on state)
pub fn render_panel_color_tag_picker_popup(
    ctx: &mut dyn RenderContext,
    panel_picker_state: &crate::ui::color_picker_state::ColorPickerState,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> ColorPickerRenderResult {
    use crate::ui::widgets::{
        draw_color_picker_l1, draw_color_picker_l2,
        ColorPickerLevel, PopupTheme,
    };

    let popup_theme = PopupTheme::new(&toolbar_theme.background, &toolbar_theme.separator)
        .with_active(&toolbar_theme.item_bg_active);
    let origin = panel_picker_state.origin;
    let level = panel_picker_state.level;

    let result = match level {
        ColorPickerLevel::L1 => {
            let config = panel_picker_state.l1_config();
            let hovered = panel_picker_state.hovered_swatch_str();
            ColorPickerRenderResult {
                level,
                l1_result: Some(draw_color_picker_l1(ctx, &config, origin, &popup_theme, hovered)),
                l2_result: None,
            }
        }
        ColorPickerLevel::L2 => {
            let config = panel_picker_state.l2_config();
            let hovered_area = panel_picker_state.hovered_area;
            ColorPickerRenderResult {
                level,
                l1_result: None,
                l2_result: Some(draw_color_picker_l2(ctx, &config, origin, &popup_theme, hovered_area)),
            }
        }
        ColorPickerLevel::Closed => ColorPickerRenderResult {
            level,
            l1_result: None,
            l2_result: None,
        },
    };

    use uzor::{Rect, input::Sense};
    use crate::ui::z_order::ZLayer;

    let layer_id = ZLayer::ColorPicker.push_named(input_coordinator, "color_picker_panel");

    if let Some(ref l1) = result.l1_result {
        let popup_rect = &l1.popup_rect;
        input_coordinator.register_on_layer(
            "color_picker_panel:popup",
            Rect { x: popup_rect.x, y: popup_rect.y, width: popup_rect.width, height: popup_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        for (idx, (_, swatch_rect)) in l1.swatch_rects.iter().enumerate() {
            input_coordinator.register_on_layer(
                format!("color_picker_panel:swatch:{}", idx),
                Rect { x: swatch_rect.x, y: swatch_rect.y, width: swatch_rect.width, height: swatch_rect.height },
                Sense::CLICK,
                &layer_id,
            );
        }

        if let Some(ref plus_rect) = l1.plus_button_rect {
            input_coordinator.register_on_layer(
                "color_picker_panel:plus",
                Rect { x: plus_rect.x, y: plus_rect.y, width: plus_rect.width, height: plus_rect.height },
                Sense::CLICK,
                &layer_id,
            );
        }

        if let Some(ref slider_rect) = l1.opacity_slider_rect {
            input_coordinator.register_on_layer(
                "color_picker_panel:opacity_slider",
                Rect { x: slider_rect.x, y: slider_rect.y, width: slider_rect.width, height: slider_rect.height },
                Sense::CLICK | Sense::DRAG | Sense::SCROLL,
                &layer_id,
            );
        }

        if let Some(ref toggle_rect) = l1.opacity_toggle_rect {
            input_coordinator.register_on_layer(
                "color_picker_panel:opacity_toggle",
                Rect { x: toggle_rect.x, y: toggle_rect.y, width: toggle_rect.width, height: toggle_rect.height },
                Sense::CLICK,
                &layer_id,
            );
        }
    } else if let Some(ref l2) = result.l2_result {
        let popup_rect = &l2.popup_rect;
        input_coordinator.register_on_layer(
            "color_picker_panel:popup",
            Rect { x: popup_rect.x, y: popup_rect.y, width: popup_rect.width, height: popup_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        let sv_rect = &l2.sv_square_rect;
        input_coordinator.register_on_layer(
            "color_picker_panel:sv_square",
            Rect { x: sv_rect.x, y: sv_rect.y, width: sv_rect.width, height: sv_rect.height },
            Sense::CLICK | Sense::DRAG,
            &layer_id,
        );

        let hue_rect = &l2.hue_bar_rect;
        input_coordinator.register_on_layer(
            "color_picker_panel:hue_bar",
            Rect { x: hue_rect.x, y: hue_rect.y, width: hue_rect.width, height: hue_rect.height },
            Sense::CLICK | Sense::DRAG,
            &layer_id,
        );

        let opacity_rect = &l2.opacity_slider_rect;
        input_coordinator.register_on_layer(
            "color_picker_panel:opacity_slider",
            Rect { x: opacity_rect.x, y: opacity_rect.y, width: opacity_rect.width, height: opacity_rect.height },
            Sense::CLICK | Sense::DRAG,
            &layer_id,
        );

        let toggle_rect = &l2.opacity_toggle_rect;
        input_coordinator.register_on_layer(
            "color_picker_panel:opacity_toggle",
            Rect { x: toggle_rect.x, y: toggle_rect.y, width: toggle_rect.width, height: toggle_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        let add_rect = &l2.add_button_rect;
        input_coordinator.register_on_layer(
            "color_picker_panel:add",
            Rect { x: add_rect.x, y: add_rect.y, width: add_rect.width, height: add_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        let back_rect = &l2.back_button_rect;
        input_coordinator.register_on_layer(
            "color_picker_panel:back",
            Rect { x: back_rect.x, y: back_rect.y, width: back_rect.width, height: back_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        let hex_rect = &l2.hex_input_rect;
        input_coordinator.register_on_layer(
            "color_picker_panel:hex_input",
            Rect { x: hex_rect.x, y: hex_rect.y, width: hex_rect.width, height: hex_rect.height },
            Sense::CLICK,
            &layer_id,
        );
    }

    // Note: pop_layer missing in original - keeping consistent
    result
}
