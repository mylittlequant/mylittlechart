//! Chrome strip render functions — require `dyn RenderContext`.
//!
//! Shared by both the native (vello) shell and the web (canvas2d) shell.
//! Platform-neutral types and geometry logic live in `chrome::types` / `chrome::logic`.

use uzor::render::{draw_svg_icon, RenderContext, TextAlign, TextBaseline};
use mlc_core::state::icons::icon_svg;

use super::types::{
    CHROME_HEIGHT, ChromeColors, ChromeContextMenu, ChromeFlavor, ChromeHit, ChromeState,
};
use crate::tooltip::TooltipState;
use crate::tooltip::calculate_tooltip_position;

// ── Layout constants ──────────────────────────────────────────────────────────

const BUTTON_WIDTH: f64 = 46.0;
const BUTTON_HEIGHT: f64 = CHROME_HEIGHT;
const CLOSE_WINDOW_BUTTON_WIDTH: f64 = 36.0;
const MASCOT_BUTTON_WIDTH: f64 = 0.0;
const MENU_BUTTON_WIDTH: f64 = 36.0;
const NEW_WINDOW_BUTTON_WIDTH: f64 = 36.0;
const NEW_TAB_BUTTON_WIDTH: f64 = 28.0;
const TAB_LEFT_MARGIN: f64 = 4.0;
const TAB_GAP: f64 = 1.0;
const TAB_PADDING_H: f64 = 12.0;
const TAB_CLOSE_SIZE: f64 = 16.0;

const CONTEXT_MENU_WIDTH: f64 = 160.0;
const CONTEXT_MENU_ITEM_HEIGHT: f64 = 28.0;
const CONTEXT_MENU_PADDING: f64 = 4.0;
const CONTEXT_MENU_ITEMS: &[(&str, bool)] = &[
    ("Close Window", false),
    ("Delete Window", true),
];

// ── ButtonPositions ───────────────────────────────────────────────────────────

struct ButtonPositions {
    /// Only valid for Native flavor.
    close_x: f64,
    /// Only valid for Native flavor.
    maximize_x: f64,
    /// Only valid for Native flavor.
    minimize_x: f64,
    /// Only valid for Native flavor.
    close_window_left: f64,
    /// Valid for both flavors (Web: rightmost button).
    menu_left: f64,
    /// Only valid for Native flavor.
    new_window_left: f64,
}

impl ButtonPositions {
    fn compute(width: f64, flavor: ChromeFlavor) -> Self {
        match flavor {
            ChromeFlavor::Native => {
                let close_x           = width - BUTTON_WIDTH;
                let maximize_x        = width - BUTTON_WIDTH * 2.0;
                let minimize_x        = width - BUTTON_WIDTH * 3.0;
                let close_window_left = minimize_x - CLOSE_WINDOW_BUTTON_WIDTH;
                let mascot_left       = close_window_left - MASCOT_BUTTON_WIDTH;
                let menu_left         = mascot_left - MENU_BUTTON_WIDTH;
                let new_window_left   = menu_left - NEW_WINDOW_BUTTON_WIDTH;
                Self { close_x, maximize_x, minimize_x, close_window_left, menu_left, new_window_left }
            }
            ChromeFlavor::Web => {
                // Web: only menu button on the right, no window controls.
                let menu_left = width - MENU_BUTTON_WIDTH;
                Self {
                    close_x:           0.0,
                    maximize_x:        0.0,
                    minimize_x:        0.0,
                    close_window_left: 0.0,
                    menu_left,
                    new_window_left:   0.0,
                }
            }
        }
    }
}

// ── update_tab_widths ─────────────────────────────────────────────────────────

/// Pre-compute tab widths using the render context for accurate text measurement.
///
/// Must be called once per frame before `render`.
pub fn update_tab_widths(ctx: &mut dyn RenderContext, state: &mut ChromeState) {
    ctx.set_font("12px sans-serif");
    state.tab_widths.clear();
    for tab in &state.tabs {
        let text_w = ctx.measure_text(&tab.name);
        let w = TAB_PADDING_H + text_w + TAB_CLOSE_SIZE + TAB_PADDING_H;
        state.tab_widths.push(w);
    }
}

// ── render ────────────────────────────────────────────────────────────────────

/// Render the chrome strip.
///
/// `skeleton_mode` suppresses tabs and right-side icon rendering (used while
/// the window is being resized on native). On Web pass `false`.
pub fn render(ctx: &mut dyn RenderContext, state: &ChromeState, width: f64, skeleton_mode: bool) {
    let c = &state.colors;
    let bp = ButtonPositions::compute(width, state.flavor);

    // Background.
    ctx.set_fill_color(&c.background);
    ctx.fill_rect(0.0, 0.0, width, CHROME_HEIGHT);

    // Close button hover highlight (Native only).
    if state.flavor == ChromeFlavor::Native {
        if state.hovered == ChromeHit::CloseButton {
            ctx.set_fill_color(&c.close_hover);
            ctx.fill_rect(bp.close_x, 0.0, BUTTON_WIDTH, BUTTON_HEIGHT);
        }
    }

    if !skeleton_mode {
        // ── Tabs ──────────────────────────────────────────────────────────────
        let mut cursor = TAB_LEFT_MARGIN;
        for (i, tab) in state.tabs.iter().enumerate() {
            let tw        = state.tab_widths.get(i).copied().unwrap_or(80.0);
            let tab_left  = cursor;
            let tab_right = cursor + tw;

            if tab.active {
                ctx.set_fill_color(&c.tab_accent);
                ctx.fill_rect(tab_left, CHROME_HEIGHT - 3.0, tw, 2.0);
            } else if state.hovered == ChromeHit::Tab(i)
                   || state.hovered == ChromeHit::TabClose(i)
            {
                ctx.set_fill_color(&c.button_hover);
                ctx.fill_rect(tab_left, CHROME_HEIGHT - 3.0, tw, 2.0);
            }

            {
                let text_x = tab_left + TAB_PADDING_H;
                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(&c.icon_normal);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(&tab.name, text_x, CHROME_HEIGHT / 2.0);
            }

            {
                let close_rect_left = tab_right - TAB_CLOSE_SIZE;
                let icon_size = 14.0;
                let icon_color = if state.hovered == ChromeHit::TabClose(i) {
                    &c.close_hover
                } else {
                    &c.icon_normal
                };
                let icon_x = close_rect_left + (TAB_CLOSE_SIZE - icon_size) / 2.0;
                let icon_y = (CHROME_HEIGHT - icon_size) / 2.0;
                if let Some(svg) = icon_svg("close") {
                    draw_svg_icon(ctx, svg, icon_x, icon_y, icon_size, icon_size, icon_color);
                }
            }

            cursor = tab_right + TAB_GAP;
        }

        // ── New-tab (+) button ─────────────────────────────────────────────────
        let new_tab_x = cursor;

        let div_h = CHROME_HEIGHT * 0.6;
        let div_y = (CHROME_HEIGHT - div_h) / 2.0;
        ctx.set_fill_color(&c.separator);
        ctx.fill_rect(new_tab_x, div_y, 1.0, div_h);

        let plus_color = if state.hovered == ChromeHit::NewTabButton {
            &c.icon_hover
        } else {
            &c.icon_normal
        };
        let plus_cx = (new_tab_x + NEW_TAB_BUTTON_WIDTH / 2.0).floor() + 0.5;
        let plus_cy = (CHROME_HEIGHT / 2.0).floor() + 0.5;
        let arm = 5.0;
        ctx.set_stroke_color(plus_color);
        ctx.set_stroke_width(1.5);
        ctx.begin_path();
        ctx.move_to(plus_cx - arm, plus_cy);
        ctx.line_to(plus_cx + arm, plus_cy);
        ctx.stroke();
        ctx.begin_path();
        ctx.move_to(plus_cx, plus_cy - arm);
        ctx.line_to(plus_cx, plus_cy + arm);
        ctx.stroke();

        // ── Right-side buttons (flavor-dependent) ─────────────────────────────
        match state.flavor {
            ChromeFlavor::Native => {
                // new_window icon.
                {
                    let icon_color = if state.hovered == ChromeHit::NewWindowButton {
                        &c.icon_hover
                    } else {
                        &c.icon_normal
                    };
                    let icon_x = bp.new_window_left + (NEW_WINDOW_BUTTON_WIDTH - 18.0) / 2.0;
                    let icon_y = (CHROME_HEIGHT - 18.0) / 2.0;
                    if let Some(svg) = icon_svg("new_window") {
                        draw_svg_icon(ctx, svg, icon_x, icon_y, 18.0, 18.0, icon_color);
                    }
                }

                // menu icon.
                {
                    let icon_color = if state.hovered == ChromeHit::MenuButton {
                        &c.icon_hover
                    } else {
                        &c.icon_normal
                    };
                    let icon_x = bp.menu_left + (MENU_BUTTON_WIDTH - 18.0) / 2.0;
                    let icon_y = (CHROME_HEIGHT - 18.0) / 2.0;
                    if let Some(svg) = icon_svg("settings") {
                        draw_svg_icon(ctx, svg, icon_x, icon_y, 18.0, 18.0, icon_color);
                    }
                }
            }
            ChromeFlavor::Web => {
                // Only menu button on the right.
                {
                    let icon_color = if state.hovered == ChromeHit::MenuButton {
                        &c.icon_hover
                    } else {
                        &c.icon_normal
                    };
                    let icon_x = bp.menu_left + (MENU_BUTTON_WIDTH - 18.0) / 2.0;
                    let icon_y = (CHROME_HEIGHT - 18.0) / 2.0;
                    if let Some(svg) = icon_svg("settings") {
                        draw_svg_icon(ctx, svg, icon_x, icon_y, 18.0, 18.0, icon_color);
                    }
                }
            }
        }
    }

    // ── Window-control buttons (Native only) ──────────────────────────────────
    if state.flavor == ChromeFlavor::Native {
        // Separator before close_window.
        {
            let div_h = CHROME_HEIGHT * 0.6;
            let div_y = (CHROME_HEIGHT - div_h) / 2.0;
            ctx.set_fill_color(&c.separator);
            ctx.fill_rect(bp.close_window_left, div_y, 1.0, div_h);
        }

        // close_window X icon.
        {
            let icon_color = if state.hovered == ChromeHit::CloseWindowButton {
                &c.icon_hover
            } else {
                &c.icon_normal
            };
            let cx = bp.close_window_left + CLOSE_WINDOW_BUTTON_WIDTH / 2.0;
            let cy = CHROME_HEIGHT / 2.0;
            let s = 3.5;
            ctx.set_stroke_color(icon_color);
            ctx.set_stroke_width(1.0);
            ctx.begin_path();
            ctx.move_to(cx - s, cy - s);
            ctx.line_to(cx + s, cy + s);
            ctx.stroke();
            ctx.begin_path();
            ctx.move_to(cx - s, cy + s);
            ctx.line_to(cx + s, cy - s);
            ctx.stroke();
        }

        // Separator before minimize.
        {
            let div_h = CHROME_HEIGHT * 0.6;
            let div_y = (CHROME_HEIGHT - div_h) / 2.0;
            ctx.set_fill_color(&c.separator);
            ctx.fill_rect(bp.minimize_x, div_y, 1.0, div_h);
        }

        // Minimize icon.
        {
            let icon_color = if state.hovered == ChromeHit::MinimizeButton {
                &c.icon_hover
            } else {
                &c.icon_normal
            };
            let cx = bp.minimize_x + BUTTON_WIDTH / 2.0;
            let cy = CHROME_HEIGHT / 2.0;
            ctx.set_fill_color(icon_color);
            ctx.fill_rect(cx - 5.0, cy, 10.0, 1.0);
        }

        // Maximize / restore icon.
        {
            let icon_color = if state.hovered == ChromeHit::MaximizeButton {
                &c.icon_hover
            } else {
                &c.icon_normal
            };
            let cx = bp.maximize_x + BUTTON_WIDTH / 2.0;
            let cy = CHROME_HEIGHT / 2.0;
            let half = 5.0;

            if state.is_maximized {
                draw_rect_outline(ctx, cx - half + 2.0, cy - half - 2.0, half * 2.0, half * 2.0, 1.0, icon_color);
                ctx.set_fill_color(&c.background);
                ctx.fill_rect(cx - half, cy - half, half * 2.0, half * 2.0);
                draw_rect_outline(ctx, cx - half, cy - half, half * 2.0, half * 2.0, 1.0, icon_color);
            } else {
                draw_rect_outline(ctx, cx - half, cy - half, half * 2.0, half * 2.0, 1.0, icon_color);
            }
        }

        // Close (X) icon.
        {
            let icon_color = &c.icon_normal;
            let cx = bp.close_x + BUTTON_WIDTH / 2.0;
            let cy = CHROME_HEIGHT / 2.0;
            let s = 5.0;
            ctx.set_stroke_color(icon_color);
            ctx.set_stroke_width(1.5);
            ctx.begin_path();
            ctx.move_to(cx - s, cy - s);
            ctx.line_to(cx + s, cy + s);
            ctx.stroke();
            ctx.begin_path();
            ctx.move_to(cx - s, cy + s);
            ctx.line_to(cx + s, cy - s);
            ctx.stroke();
        }
    }

    // Bottom separator line.
    ctx.set_fill_color(&c.separator);
    ctx.fill_rect(0.0, CHROME_HEIGHT - 1.0, width, 1.0);
}

// ── render_tooltip ────────────────────────────────────────────────────────────

/// Render the chrome tooltip if one is currently visible.
pub fn render_tooltip(
    ctx: &mut dyn RenderContext,
    state: &ChromeState,
    screen_width: f64,
    screen_height: f64,
) {
    render_tooltip_themed(
        ctx,
        &state.tooltip,
        &state.colors.tooltip_bg,
        &state.colors.tooltip_text,
        screen_width,
        screen_height,
    );
}

/// Render a tooltip using explicit theme colors.
pub fn render_tooltip_themed(
    ctx: &mut dyn RenderContext,
    tooltip: &TooltipState,
    bg_color: &str,
    text_color: &str,
    screen_width: f64,
    screen_height: f64,
) {
    let active = match tooltip.get_active() {
        Some(t) => t,
        None => return,
    };

    let opacity = tooltip.get_opacity();
    if opacity <= 0.0 {
        return;
    }

    let text = &active.text;
    ctx.set_font("12px sans-serif");
    let text_width = ctx.measure_text(text);
    let pad = 6.0;
    let tw = text_width + pad * 2.0;
    let th = 12.0 + pad * 2.0;

    let (tx, ty) = calculate_tooltip_position(
        active.position,
        (tw, th),
        (screen_width, screen_height),
        (0.0, 20.0),
    );

    ctx.save();
    ctx.set_global_alpha(opacity);

    ctx.set_fill_color("#00000060");
    ctx.fill_rounded_rect(tx + 1.0, ty + 1.0, tw, th, 4.0);

    ctx.set_fill_color(bg_color);
    ctx.fill_rounded_rect(tx, ty, tw, th, 4.0);

    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(text, tx + pad, ty + th / 2.0);

    ctx.restore();
}

// ── render_context_menu ───────────────────────────────────────────────────────

/// Render the chrome right-click context menu if open.
pub fn render_context_menu(
    ctx: &mut dyn RenderContext,
    menu: &ChromeContextMenu,
    colors: &ChromeColors,
) {
    if !menu.open {
        return;
    }

    let item_count = CONTEXT_MENU_ITEMS.len() as f64;
    let menu_h = CONTEXT_MENU_PADDING * 2.0 + item_count * CONTEXT_MENU_ITEM_HEIGHT;

    ctx.set_fill_color("rgba(0,0,0,0.3)");
    ctx.fill_rect(menu.x + 2.0, menu.y + 2.0, CONTEXT_MENU_WIDTH, menu_h);

    ctx.set_fill_color(&colors.background);
    ctx.fill_rect(menu.x, menu.y, CONTEXT_MENU_WIDTH, menu_h);

    draw_rect_outline(ctx, menu.x, menu.y, CONTEXT_MENU_WIDTH, menu_h, 1.0, &colors.separator);

    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    for (i, (label, is_danger)) in CONTEXT_MENU_ITEMS.iter().enumerate() {
        let item_y = menu.y + CONTEXT_MENU_PADDING + i as f64 * CONTEXT_MENU_ITEM_HEIGHT;

        if menu.hovered_index == i as i32 {
            let hover_color = if *is_danger { &colors.close_hover } else { &colors.button_hover };
            ctx.set_fill_color(hover_color);
            ctx.fill_rect(menu.x + 1.0, item_y, CONTEXT_MENU_WIDTH - 2.0, CONTEXT_MENU_ITEM_HEIGHT);
        }

        let text_color = if *is_danger && menu.hovered_index == i as i32 {
            &colors.icon_hover
        } else if *is_danger {
            &colors.close_hover
        } else {
            &colors.icon_normal
        };
        ctx.set_fill_color(text_color);
        ctx.fill_text(label, menu.x + 12.0, item_y + CONTEXT_MENU_ITEM_HEIGHT / 2.0);
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn draw_rect_outline(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, h: f64, t: f64, color: &str) {
    ctx.set_fill_color(color);
    ctx.fill_rect(x, y, w, t);
    ctx.fill_rect(x, y + h - t, w, t);
    ctx.fill_rect(x, y + t, t, h - t * 2.0);
    ctx.fill_rect(x + w - t, y + t, t, h - t * 2.0);
}
