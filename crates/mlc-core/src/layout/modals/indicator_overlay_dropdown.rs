//! Indicator overlay dropdown renderer.

use crate::engine::render::RenderContext;
use crate::ui::toolbar_render::ToolbarTheme;
use uzor::types::Rect as WidgetRect;
use crate::apply_opacity;
use crate::layout::render_frame::IndicatorRowResult;
use crate::layout::render_chart::FrameTheme;
use crate::layout::render_ui::IndicatorOverlayInfo;
use crate::ui::Icon;

/// Render indicator overlay dropdown list.
///
/// Style: transparent background, no border, left-aligned rows (like TradingView)
pub fn render_indicator_overlay_dropdown(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    indicators: &[IndicatorOverlayInfo],
    overlay_state: &crate::ui::modal_settings::IndicatorOverlayState,
    _frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
) -> (WidgetRect, Vec<IndicatorRowResult>) {
    use crate::render::{TextAlign, TextBaseline};
    use crate::engine::render::draw_svg_icon;

    let mut rows = Vec::new();

    let row_height = 20.0;
    let row_gap = 2.0;
    let icon_size = 14.0;
    let icon_gap = 4.0;
    let font_size = 12.0;
    let text_icon_gap = 6.0;

    // Running compare index counter: each compare entry gets a 0-based index
    // used as its instance_id so click handlers can find it by index.
    let mut compare_idx: u64 = 0;

    ctx.set_font(&format!("{}px sans-serif", font_size));

    let mut row_widths: Vec<f64> = Vec::new();
    for indicator in indicators {
        let name_width = ctx.measure_text(&indicator.display_name);
        let action_icons_width = (icon_size + icon_gap) * 4.0 - icon_gap;
        let row_width = name_width + text_icon_gap + action_icons_width;
        row_widths.push(row_width);
    }

    let max_width = row_widths.iter().copied().fold(0.0_f64, f64::max);
    let total_height = row_height * indicators.len() as f64 + row_gap * (indicators.len().saturating_sub(1)) as f64;

    let dropdown_rect = WidgetRect::new(x, y, max_width, total_height);

    // Subtle background matching overlay tab style
    let dropdown_bg = apply_opacity(&toolbar_theme.background, 0.75);
    ctx.set_fill_color(&dropdown_bg);
    ctx.fill_rounded_rect(x, y, max_width, total_height, 2.0);

    for (i, indicator) in indicators.iter().enumerate() {
        let row_y = y + i as f64 * (row_height + row_gap);
        let row_width = row_widths[i];
        let is_hovered = overlay_state.is_indicator_hovered(indicator.id);

        // For compare entries, instance_id encodes the compare series index (0-based).
        // For regular indicators, instance_id is the indicator's unique instance ID.
        let (row_instance_id, row_is_compare) = if indicator.is_compare {
            let idx = compare_idx;
            compare_idx += 1;
            (idx, true)
        } else {
            (indicator.id, false)
        };
        let mut row_result = IndicatorRowResult {
            instance_id: row_instance_id,
            is_compare: row_is_compare,
            row_rect: WidgetRect::new(x, row_y, row_width, row_height),
            ..Default::default()
        };
        if is_hovered {
            ctx.draw_hover_rect(x, row_y, row_width, row_height, &toolbar_theme.item_bg_hover);
        }

        let base_color = if indicator.visible {
            toolbar_theme.item_text.clone()
        } else {
            apply_opacity(&toolbar_theme.item_text, 0.5)
        };

        ctx.set_font(&format!("{}px sans-serif", font_size));
        ctx.set_fill_color(&base_color);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&indicator.display_name, x, row_y + row_height / 2.0);

        let name_width = ctx.measure_text(&indicator.display_name);
        let icons_x = x + name_width + text_icon_gap;
        let icon_y = row_y + (row_height - icon_size) / 2.0;

        // 1. Visibility toggle (eye/eye-off)
        let vis_x = icons_x;
        let vis_icon = if indicator.visible { Icon::Eye } else { Icon::EyeOff };
        let vis_color = if overlay_state.is_action_hovered(indicator.id, "visibility") {
            "#2962ff".to_string()
        } else {
            base_color.clone()
        };
        draw_svg_icon(ctx, vis_icon.svg(), vis_x, icon_y, icon_size, icon_size, &vis_color);
        row_result.visibility_btn = WidgetRect::new(vis_x, icon_y, icon_size, icon_size);

        // 2. Alert (bell)
        let alert_x = vis_x + icon_size + icon_gap;
        let alert_color = if overlay_state.is_action_hovered(indicator.id, "alert") {
            "#2962ff".to_string()
        } else {
            base_color.clone()
        };
        draw_svg_icon(ctx, Icon::Alert.svg(), alert_x, icon_y, icon_size, icon_size, &alert_color);
        row_result.alert_btn = WidgetRect::new(alert_x, icon_y, icon_size, icon_size);

        // 3. Settings (gear)
        let settings_x = alert_x + icon_size + icon_gap;
        let settings_color = if overlay_state.is_action_hovered(indicator.id, "settings") {
            "#2962ff".to_string()
        } else {
            base_color.clone()
        };
        draw_svg_icon(ctx, Icon::Settings.svg(), settings_x, icon_y, icon_size, icon_size, &settings_color);
        row_result.settings_btn = WidgetRect::new(settings_x, icon_y, icon_size, icon_size);

        // 4. Delete (trash)
        let delete_x = settings_x + icon_size + icon_gap;
        let delete_color = if overlay_state.is_action_hovered(indicator.id, "delete") {
            "#f23645".to_string()
        } else {
            base_color.clone()
        };
        draw_svg_icon(ctx, Icon::Delete.svg(), delete_x, icon_y, icon_size, icon_size, &delete_color);
        row_result.delete_btn = WidgetRect::new(delete_x, icon_y, icon_size, icon_size);

        rows.push(row_result);
    }

    (dropdown_rect, rows)
}
