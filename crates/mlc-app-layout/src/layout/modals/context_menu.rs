//! Context menu modal renderer.
//!
//! Harvest: uzor context-menu (the checkbox glyph + submenu recursion added
//! 2026-09-24 for the messenger concept's `Mute ▸`/`Add to folder ▸` rows —
//! see [`ContextMenuItemState::checkbox`]/[`ContextMenuItemState::submenu`]
//! own doc; the pre-existing flat-menu body above it is this crate's own
//! earlier code, not part of that pass).

use mlc_core::engine::render::RenderContext;
use uzor::types::Rect as WidgetRect;
use crate::ui::context_menu::{ContextMenuItemState, ContextMenuState};
use crate::ui::dropdown::DropdownTheme;
use crate::ui::Icon;
use crate::layout::render_frame::ContextMenuResult;

/// Menu geometry shared by the top-level render and every recursive
/// submenu call — kept in one place so a submenu panel can never silently
/// drift out of step with its parent's own sizing.
const ITEM_HEIGHT: f64 = 32.0;
const PADDING_X: f64 = 12.0;
const PADDING_Y: f64 = 8.0;
const ICON_SIZE: f64 = 16.0;
const ICON_GAP: f64 = 8.0;
const MIN_WIDTH: f64 = 180.0;
const SEPARATOR_H: f64 = 9.0;

/// Total menu height for `items` at this module's own fixed row metrics —
/// shared by [`render_context_menu`] and [`crate::ui::context_menu::
/// ContextMenuState::open_smart`]'s own (separately maintained) copy of
/// this arithmetic, so a caller sizing a submenu panel before drawing it
/// (e.g. to clamp it on screen) never has to re-derive it by hand.
pub fn menu_height(items: &[ContextMenuItemState]) -> f64 {
    let item_count = items.iter().filter(|i| !i.is_separator).count();
    let separator_count = items.iter().filter(|i| i.is_separator).count();
    (item_count as f64 * ITEM_HEIGHT) + (separator_count as f64 * SEPARATOR_H) + PADDING_Y * 2.0
}

/// Render context menu and return hit zones
pub fn render_context_menu(
    ctx: &mut dyn RenderContext,
    state: &ContextMenuState,
    theme: &DropdownTheme,
    hovered_id: Option<&str>,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> ContextMenuResult {
    let menu_width = MIN_WIDTH;
    let menu_x = state.x;
    let menu_y = state.y;
    let menu_h = menu_height(&state.items);

    let mut item_rects = draw_menu_panel(ctx, &state.items, menu_x, menu_y, menu_width, theme, hovered_id);

    // A submenu opens to the right of its own parent row (concept: `Mute
    // ▸`/`Add to folder ▸`) — one level, drawn as a second panel using the
    // SAME layout function, positioned off the parent row's own rect
    // (already collected into `item_rects` above). No on-screen clamping
    // here (unlike `ContextMenuState::open_smart`'s own top-level clamp):
    // a later pass wiring up actual hover/click dispatch is what would
    // also own picking a clamped position, same as opening the top-level
    // menu already does.
    if let Some((parent, parent_rect)) = state
        .items
        .iter()
        .find(|item| item.submenu_open && !item.submenu.is_empty())
        .and_then(|item| {
            item_rects.iter().find(|(action, _)| action == &item.action).map(|(_, rect)| (item, *rect))
        })
    {
        let sub_x = parent_rect.x + parent_rect.width - 1.0;
        let sub_y = parent_rect.y;
        let sub_rects = draw_menu_panel(ctx, &parent.submenu, sub_x, sub_y, menu_width, theme, hovered_id);
        item_rects.extend(sub_rects);
    }

    // === InputCoordinator Integration ===
    use crate::ui::z_order::ZLayer;
    let layer_id = ZLayer::ContextMenu.push_named(input_coordinator, "context_menu");

    // Register menu background (FIRST, so items override it)
    input_coordinator.register_on_layer(
        "context_menu:bg",
        uzor::types::Rect::new(menu_x, menu_y, menu_width, menu_h),
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    // Register each menu item (skip separators) — submenu rows share the
    // same flat list, so their own action strings must stay distinct from
    // their parent menu's (same discipline every OTHER `context_menu:
    // item:{action}` caller already follows for id collisions).
    for (action, rect) in &item_rects {
        let widget_id = format!("context_menu:item:{}", action);
        input_coordinator.register_on_layer(
            widget_id,
            uzor::types::Rect::new(rect.x, rect.y, rect.width, rect.height),
            uzor::input::Sense::CLICK,
            &layer_id,
        );
    }

    // Pop layer before returning
    input_coordinator.pop_layer(&layer_id);

    ContextMenuResult {
        menu_rect: WidgetRect::new(menu_x, menu_y, menu_width, menu_h),
        item_rects,
        hovered_item_id: hovered_id.map(|s| s.to_string()),
    }
}

/// Paint one flat menu panel (background, border, every row) at
/// `(panel_x, panel_y)` and return each non-separator item's own
/// `(action, rect)` — the body shared by the top-level menu and its one
/// open submenu (see [`render_context_menu`]'s own doc). Does NOT touch
/// the `InputCoordinator`: both callers register hit zones together, once,
/// after every panel has painted.
fn draw_menu_panel(
    ctx: &mut dyn RenderContext,
    items: &[ContextMenuItemState],
    panel_x: f64,
    panel_y: f64,
    menu_width: f64,
    theme: &DropdownTheme,
    hovered_id: Option<&str>,
) -> Vec<(String, WidgetRect)> {
    use mlc_core::render::{TextAlign, TextBaseline};

    let mut item_rects = Vec::new();
    let panel_h = menu_height(items);

    // Draw menu background with shadow
    ctx.set_fill_color("rgba(0,0,0,0.3)");
    ctx.fill_rect(panel_x + 3.0, panel_y + 3.0, menu_width, panel_h);

    // Blur background (FrostedGlass/LiquidGlass)
    ctx.draw_blur_background(panel_x, panel_y, menu_width, panel_h);

    crate::ui::overlay_fill::fill_overlay_rounded(ctx, &theme.background, panel_x, panel_y, menu_width, panel_h, 0.0);

    // Draw border
    ctx.set_stroke_color(&theme.border);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rect(panel_x, panel_y, menu_width, panel_h);

    // Draw items
    let mut y = panel_y + PADDING_Y;

    for item in items {
        if item.is_separator {
            // Draw separator line
            let sep_y = y + 4.0;
            ctx.set_stroke_color(&theme.border);
            ctx.set_stroke_width(1.0);
            ctx.begin_path();
            ctx.move_to(panel_x + PADDING_X, sep_y);
            ctx.line_to(panel_x + menu_width - PADDING_X, sep_y);
            ctx.stroke();
            y += SEPARATOR_H;
            continue;
        }

        let item_rect = WidgetRect::new(panel_x, y, menu_width, ITEM_HEIGHT);
        let is_hovered = hovered_id == Some(item.action.as_str());

        // Draw hover background
        if is_hovered && item.enabled {
            ctx.draw_hover_rect(panel_x + 2.0, y, menu_width - 4.0, ITEM_HEIGHT, &theme.item_bg_hover);
        }

        let icon_x = panel_x + PADDING_X;
        let icon_y = y + (ITEM_HEIGHT - ICON_SIZE) / 2.0;
        let icon_color = if item.enabled {
            if item.is_danger { &theme.item_danger } else { &theme.item_text }
        } else {
            &theme.item_text_disabled
        };

        if let Some(checked) = item.checkbox {
            // Checkbox glyph — the concept's `.cb`/`.cb.on` (e.g. an
            // "Add to folder ▸" submenu's per-folder checklist row):
            // replaces the icon slot, never drawn alongside one.
            crate::ui::overlay_fill::fill_checkbox(
                ctx, checked, &theme.accent, &theme.background, &theme.border,
                icon_x, icon_y, ICON_SIZE, ICON_SIZE, 3.0,
            );
        } else if let Some(ref icon_id) = item.icon {
            use mlc_core::engine::render::draw_svg_icon;

            // Map icon ID string to Icon enum.
            //
            // Elliott ids added 2026-08-14 (owner report: wave-menu rows
            // built in `ui::context_menu::{build_conversion_block,
            // build_wave_menu, build_successor_menu}` were text-only,
            // although every one of these five structures already has a
            // registered `toolbar.rs` icon). `elliott_double_combo` and
            // `elliott_triple_combo` both resolve to the one
            // `Icon::ElliottCombo` variant — this enum has no separate
            // double/triple combo member (only the raw `icon_svg` string
            // table does), same collapse `Icon::from_name` already applies.
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
                "elliott_impulse" => Some(Icon::ElliottImpulse),
                "elliott_correction" => Some(Icon::ElliottCorrection),
                "elliott_triangle" => Some(Icon::ElliottTriangle),
                "elliott_double_combo" | "elliott_triple_combo" => Some(Icon::ElliottCombo),
                _ => None,
            };

            if let Some(icon) = icon_enum {
                draw_svg_icon(ctx, icon.svg(), icon_x, icon_y, ICON_SIZE, ICON_SIZE, icon_color);
            }
        }

        // Draw label
        let text_x = panel_x + PADDING_X + ICON_SIZE + ICON_GAP;
        let text_y = y + ITEM_HEIGHT / 2.0;

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

        // Right-edge chevron for a row with a submenu — the concept's own
        // `.ar` (`i-right`). Never combined with a keyboard-shortcut label
        // (this menu draws none today); if one is ever added, it goes to
        // the LEFT of this glyph, same as the concept's `.kb`/`.ar` split.
        if !item.submenu.is_empty() {
            ctx.set_fill_color(&theme.item_text_disabled);
            ctx.set_text_align(TextAlign::Right);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("\u{203a}", panel_x + menu_width - PADDING_X, text_y);
        }

        // Store item rect for hit testing
        item_rects.push((item.action.clone(), item_rect));

        y += ITEM_HEIGHT;
    }

    item_rects
}
