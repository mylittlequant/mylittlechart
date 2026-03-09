//! Context menu modal renderer.

use crate::engine::render::RenderContext;
use uzor::types::Rect as WidgetRect;
use crate::ui::context_menu::ContextMenuState;
use crate::ui::dropdown::DropdownTheme;
use crate::ui::Icon;
use crate::layout::render_frame::ContextMenuResult;

/// Render context menu and return hit zones
pub fn render_context_menu(
    ctx: &mut dyn RenderContext,
    state: &ContextMenuState,
    theme: &DropdownTheme,
    hovered_id: Option<&str>,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> ContextMenuResult {
    use crate::render::{TextAlign, TextBaseline};

    let mut item_rects = Vec::new();

    // Menu dimensions
    let item_height = 32.0;
    let padding_x = 12.0;
    let padding_y = 8.0;
    let icon_size = 16.0;
    let icon_gap = 8.0;
    let min_width = 180.0;

    // Calculate menu size
    let item_count = state.items.iter().filter(|i| !i.is_separator).count();
    let separator_count = state.items.iter().filter(|i| i.is_separator).count();
    let menu_height = (item_count as f64 * item_height) + (separator_count as f64 * 9.0) + padding_y * 2.0;
    let menu_width = min_width;

    let menu_x = state.x;
    let menu_y = state.y;

    // Draw menu background with shadow
    ctx.set_fill_color("rgba(0,0,0,0.3)");
    ctx.fill_rect(menu_x + 3.0, menu_y + 3.0, menu_width, menu_height);

    // Blur background (FrostedGlass/LiquidGlass)
    ctx.draw_blur_background(menu_x, menu_y, menu_width, menu_height);

    ctx.set_fill_color(&theme.background);
    ctx.fill_rect(menu_x, menu_y, menu_width, menu_height);

    // Draw border
    ctx.set_stroke_color(&theme.border);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rect(menu_x, menu_y, menu_width, menu_height);

    // Draw items
    let mut y = menu_y + padding_y;

    for item in &state.items {
        if item.is_separator {
            // Draw separator line
            let sep_y = y + 4.0;
            ctx.set_stroke_color(&theme.border);
            ctx.set_stroke_width(1.0);
            ctx.begin_path();
            ctx.move_to(menu_x + padding_x, sep_y);
            ctx.line_to(menu_x + menu_width - padding_x, sep_y);
            ctx.stroke();
            y += 9.0;
            continue;
        }

        let item_rect = WidgetRect::new(menu_x, y, menu_width, item_height);
        let is_hovered = hovered_id == Some(item.action.as_str());

        // Draw hover background
        if is_hovered && item.enabled {
            ctx.draw_hover_rect(menu_x + 2.0, y, menu_width - 4.0, item_height, &theme.item_bg_hover);
        }

        // Draw icon (SVG)
        if let Some(ref icon_id) = item.icon {
            use crate::engine::render::draw_svg_icon;

            // Map icon ID string to Icon enum
            let icon_enum = match icon_id.as_str() {
                "settings" => Some(Icon::Settings),
                "copy" => Some(Icon::Copy),
                "lock" => Some(Icon::Lock),
                "unlock" => Some(Icon::Unlock),
                "eye" => Some(Icon::Eye),
                "eye_off" => Some(Icon::EyeOff),
                "delete" => Some(Icon::Delete),
                "arrow_up" => Some(Icon::ArrowUp),
                "arrow_down" => Some(Icon::ArrowDown),
                _ => None,
            };

            if let Some(icon) = icon_enum {
                let icon_x = menu_x + padding_x;
                let icon_y = y + (item_height - icon_size) / 2.0;
                let icon_color = if item.enabled {
                    if item.is_danger { &theme.item_danger } else { &theme.item_text }
                } else {
                    &theme.item_text_disabled
                };
                draw_svg_icon(ctx, icon.svg(), icon_x, icon_y, icon_size, icon_size, icon_color);
            }
        }

        // Draw label
        let text_x = menu_x + padding_x + icon_size + icon_gap;
        let text_y = y + item_height / 2.0;

        let text_color = if !item.enabled {
            &theme.item_text_disabled
        } else if item.is_danger {
            &theme.item_danger
        } else {
            &theme.item_text
        };

        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(text_color);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&item.label, text_x, text_y);

        // Store item rect for hit testing
        item_rects.push((item.action.clone(), item_rect));

        y += item_height;
    }

    // === InputCoordinator Integration ===
    use crate::ui::z_order::ZLayer;
    let layer_id = ZLayer::ContextMenu.push_named(input_coordinator, "context_menu");

    // Register menu background (FIRST, so items override it)
    input_coordinator.register_on_layer(
        "context_menu:bg",
        uzor::types::Rect::new(menu_x, menu_y, menu_width, menu_height),
        uzor::input::sense::Sense::CLICK,
        &layer_id,
    );

    // Register each menu item (skip separators)
    for (action, rect) in &item_rects {
        let widget_id = format!("context_menu:item:{}", action);
        input_coordinator.register_on_layer(
            widget_id,
            uzor::types::Rect::new(rect.x, rect.y, rect.width, rect.height),
            uzor::input::sense::Sense::CLICK,
            &layer_id,
        );
    }

    // Pop layer before returning
    input_coordinator.pop_layer(&layer_id);

    ContextMenuResult {
        menu_rect: WidgetRect::new(menu_x, menu_y, menu_width, menu_height),
        item_rects,
        hovered_item_id: hovered_id.map(|s| s.to_string()),
    }
}
