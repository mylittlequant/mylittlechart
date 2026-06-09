//! Indicator overlay renderer.

use crate::engine::render::RenderContext;
use crate::ui::toolbar_render::ToolbarTheme;
use uzor::types::Rect as WidgetRect;
use crate::apply_opacity;
use crate::layout::render_frame::IndicatorOverlayResult;
use crate::layout::render_chart::FrameTheme;
use crate::layout::render_ui::IndicatorOverlayInfo;
use crate::ui::Icon;
use super::indicator_overlay_dropdown::render_indicator_overlay_dropdown;

/// Render indicator overlay button + optional dropdown for a single window
pub fn render_indicator_overlay(
    ctx: &mut dyn RenderContext,
    chart_rect: &WidgetRect,
    indicators: &[IndicatorOverlayInfo],
    overlay_state: &crate::ui::modal_settings::IndicatorOverlayState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
) -> IndicatorOverlayResult {
    use crate::render::{TextAlign, TextBaseline};
    use crate::engine::render::draw_svg_icon;

    let mut result = IndicatorOverlayResult::default();

    if indicators.is_empty() {
        return result;
    }

    let is_visible = overlay_state.is_open;

    let button_padding_h = 6.0;
    let button_height = 18.0;
    let chevron_size = 12.0;
    let gap = 3.0;
    let font_size = 12.0;

    // Position below the overlay tab header with minimal gap.
    // Tab is at (chart_rect.x + 2.0, chart_rect.y + 2.0) with height LEAF_TAB_HEIGHT.
    let tab_x = chart_rect.x + 2.0;
    let tab_bottom = chart_rect.y + 2.0 + crate::layout::panel_overlay::LEAF_TAB_HEIGHT;
    let button_gap = 3.0; // small gap between tab and chevron button
    let button_x = tab_x;  // left-aligned with overlay tab
    let button_y = tab_bottom + button_gap;

    if is_visible {
        // === OPEN STATE: Show indicator list + close chevron at bottom ===

        let dropdown_result = render_indicator_overlay_dropdown(
            ctx,
            button_x,
            button_y,
            indicators,
            overlay_state,
            frame_theme,
            toolbar_theme,
        );
        result.dropdown_rect = Some(dropdown_result.0);
        result.indicator_rows = dropdown_result.1;

        let close_button_y = dropdown_result.0.y + dropdown_result.0.height + 4.0;
        let close_button_width = button_padding_h * 2.0 + chevron_size;

        let close_button_rect = WidgetRect::new(button_x, close_button_y, close_button_width, button_height);
        result.close_button_rect = Some(close_button_rect);

        // Background matching overlay tab style
        let bg_color = apply_opacity(&toolbar_theme.background, 0.75);
        ctx.set_fill_color(&bg_color);
        ctx.fill_rounded_rect(button_x, close_button_y, close_button_width, button_height, 2.0);

        if overlay_state.button_hovered {
            ctx.draw_hover_rect(button_x, close_button_y, close_button_width, button_height, &toolbar_theme.item_bg_hover);
        }

        let border_color = apply_opacity(&toolbar_theme.separator, 0.9);
        ctx.set_stroke_color(&border_color);
        ctx.set_stroke_width(0.7);
        ctx.stroke_rect(button_x, close_button_y, close_button_width, button_height);

        let chevron_x = button_x + button_padding_h;
        let chevron_y = close_button_y + (button_height - chevron_size) / 2.0;
        let icon_color = if overlay_state.button_hovered {
            toolbar_theme.item_text_hover.clone()
        } else {
            apply_opacity(&toolbar_theme.item_text, 0.85)
        };
        draw_svg_icon(ctx, Icon::ChevronUp.svg(), chevron_x, chevron_y, chevron_size, chevron_size, &icon_color);

        result.button_rect = WidgetRect::new(0.0, 0.0, 0.0, 0.0);
    } else {
        // === CLOSED STATE: Show button with chevron down + count ===

        let count_text = format!("{}", indicators.len());
        ctx.set_font(&format!("bold {}px sans-serif", font_size));
        let count_width = ctx.measure_text(&count_text);
        let button_width = button_padding_h * 2.0 + chevron_size + gap + count_width;

        let button_rect = WidgetRect::new(button_x, button_y, button_width, button_height);
        result.button_rect = button_rect;

        // Background matching overlay tab style
        let bg_color = apply_opacity(&toolbar_theme.background, 0.75);
        ctx.set_fill_color(&bg_color);
        ctx.fill_rounded_rect(button_x, button_y, button_width, button_height, 2.0);

        if overlay_state.button_hovered {
            ctx.draw_hover_rect(button_x, button_y, button_width, button_height, &toolbar_theme.item_bg_hover);
        }

        let border_color = apply_opacity(&toolbar_theme.separator, 0.9);
        ctx.set_stroke_color(&border_color);
        ctx.set_stroke_width(0.7);
        ctx.stroke_rect(button_x, button_y, button_width, button_height);

        let chevron_x = button_x + button_padding_h;
        let chevron_y = button_y + (button_height - chevron_size) / 2.0;
        let icon_color = if overlay_state.button_hovered {
            toolbar_theme.item_text_hover.clone()
        } else {
            apply_opacity(&toolbar_theme.item_text, 0.85)
        };
        draw_svg_icon(ctx, Icon::ChevronDown.svg(), chevron_x, chevron_y, chevron_size, chevron_size, &icon_color);

        ctx.set_font(&format!("bold {}px sans-serif", font_size));
        ctx.set_fill_color(&icon_color);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&count_text, chevron_x + chevron_size + gap, button_y + button_height / 2.0);
    }

    result
}
