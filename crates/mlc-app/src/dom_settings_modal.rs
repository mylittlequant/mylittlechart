//! DOM settings modal (global-docking Stage B, owner spec 2026-07-30):
//! one modal managing BOTH DOM modes — single-ladder (skin, columns,
//! auto-center, volume filter, tick) and multi-exchange (exchange roster,
//! grid, depth). Opened by the DOM section header's gear (`tch:sec_gear`).
//!
//! Built on the SAME modal grammar as the system modals
//! (`render_modal_frame_only` + `ModalTheme`, tc_modal button rows) —
//! draggable header, ✕, `dom_modal:*` ids dispatched in `panel_click.rs`,
//! outside-close via `modals.rs::close_topmost_modal_layer`.

use mlc_app_layout::layout::FrameTheme;
use mlc_app_layout::ui::toolbar_theme::ToolbarTheme;
use mlc_app_layout::ui::widgets::{render_modal_frame_only, ModalTheme};
use mlc_app_layout::ui::z_order::ZLayer;
use mlc_app_layout::ui::Icon;
use mlc_app_panels::trading::order_flow::dom::DomState;
use mlc_core::engine::render::{draw_svg_icon, RenderContext};
use mlc_core::render::{TextAlign, TextBaseline};
use uzor::input::Sense;
use uzor::types::Rect as WidgetRect;
use uzor::InputCoordinator;

pub const DOM_MODAL_W: f64 = 340.0;
const HEADER_H: f64 = 44.0;
const CLOSE_BTN: f64 = 16.0;
const PAD: f64 = 16.0;
const BTN_H: f64 = 26.0;
const CAPTION_H: f64 = 26.0;
const ROW_GAP: f64 = 20.0;

/// One accent-style modal button (tc_modal SETUP grammar).
#[allow(clippy::too_many_arguments)]
fn modal_btn(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer: &uzor::input::LayerId,
    tt: &ToolbarTheme,
    id: &str,
    x: f64,
    y: f64,
    w: f64,
    label: &str,
    active: bool,
) {
    let wid = format!("dom_modal:{}", id);
    let hovered = coord.is_hovered(&uzor::WidgetId::from(wid.as_str()));
    let bg = if active {
        tt.accent.clone()
    } else if hovered {
        tt.item_bg_hover.clone()
    } else {
        mlc_core::apply_opacity(&tt.item_text, 0.06)
    };
    ctx.set_fill_color(&bg);
    ctx.fill_rounded_rect(x, y, w, BTN_H, 4.0);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(if active { &tt.item_text_active } else { &tt.item_text });
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, x + w / 2.0, y + BTN_H / 2.0);
    coord.register_on_layer(
        wid.as_str(),
        WidgetRect::new(x, y, w, BTN_H),
        Sense::CLICK | Sense::HOVER,
        layer,
    );
}

fn caption(ctx: &mut dyn RenderContext, tt: &ToolbarTheme, x: f64, y: f64, text: &str) {
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(text, x, y + 10.0);
}

/// Render the modal on its own `dom_modal` modal layer.
pub fn render_dom_settings_modal(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    frame_theme: &FrameTheme,
    tt: &ToolbarTheme,
    screen_w: f64,
    screen_h: f64,
    pos: Option<(f64, f64)>,
    dom: &DomState,
) {
    // Height from content: mode + skin + columns + ladder + multi block.
    let n_ex = dom.multi.config.exchanges.len();
    let modal_h = HEADER_H
        + PAD
        + (CAPTION_H + BTN_H + ROW_GAP) * 4.0            // mode / skin / columns / ladder
        + CAPTION_H                                       // multi caption
        + (BTN_H + 6.0) * (n_ex as f64 + 1.0)             // exchange rows + add
        + CAPTION_H + BTN_H                               // grid+depth row
        + PAD;
    let (mx, my) = pos.unwrap_or(((screen_w - DOM_MODAL_W) / 2.0, (screen_h - modal_h) / 2.0));
    let mx = mx.max(0.0).min((screen_w - DOM_MODAL_W).max(0.0));
    let my = my.max(0.0).min((screen_h - modal_h).max(0.0));
    let modal_rect = WidgetRect::new(mx, my, DOM_MODAL_W, modal_h);
    let header_rect = WidgetRect::new(mx, my, DOM_MODAL_W, HEADER_H);

    let layer_id = ZLayer::Modal.push_named(coord, "dom_modal");
    coord.register_on_layer("dom_modal:header", header_rect, Sense::DRAG, &layer_id);
    coord.register_on_layer("dom_modal:modal_bg", modal_rect, Sense::CLICK, &layer_id);

    // ── Frame + header (canonical modal chrome) ──────────────────────────
    let modal_theme = ModalTheme::from_frame_theme(
        &frame_theme.toolbar_bg,
        &frame_theme.toolbar_border,
        &tt.item_text,
        &tt.item_text_active,
        &frame_theme.toolbar_border,
    );
    render_modal_frame_only(ctx, modal_rect, &modal_theme, 0.0);

    ctx.set_fill_color(&frame_theme.toolbar_bg);
    ctx.fill_rect(mx, my, DOM_MODAL_W, HEADER_H);
    ctx.set_font("bold 13px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("DOM Settings", mx + PAD, my + HEADER_H / 2.0);

    let close_x = mx + DOM_MODAL_W - CLOSE_BTN - 12.0;
    let close_y = my + (HEADER_H - CLOSE_BTN) / 2.0;
    draw_svg_icon(ctx, Icon::Close.svg(), close_x, close_y, CLOSE_BTN, CLOSE_BTN, &tt.item_text);
    coord.register_on_layer(
        "dom_modal:close",
        WidgetRect::new(close_x - 4.0, close_y - 4.0, CLOSE_BTN + 8.0, CLOSE_BTN + 8.0),
        Sense::CLICK | Sense::HOVER,
        &layer_id,
    );

    ctx.set_stroke_color(&frame_theme.toolbar_border);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(mx, my + HEADER_H);
    ctx.line_to(mx + DOM_MODAL_W, my + HEADER_H);
    ctx.stroke();

    // ── Rows ─────────────────────────────────────────────────────────────
    let left = mx + PAD;
    let mut row_y = my + HEADER_H + PAD;
    let btn_w = 90.0;

    caption(ctx, tt, left, row_y, "Mode");
    row_y += CAPTION_H;
    modal_btn(ctx, coord, &layer_id, tt, "mode:dom", left, row_y, btn_w, "DOM", !dom.multi_mode);
    modal_btn(ctx, coord, &layer_id, tt, "mode:multi", left + btn_w + 8.0, row_y, btn_w + 10.0, "Multi-DOM", dom.multi_mode);
    row_y += BTN_H + ROW_GAP;

    caption(ctx, tt, left, row_y, "Skin (click to cycle)");
    row_y += CAPTION_H;
    modal_btn(ctx, coord, &layer_id, tt, "skin", left, row_y, btn_w * 2.0 + 18.0, &format!("{:?}", dom.skin), false);
    row_y += BTN_H + ROW_GAP;

    caption(ctx, tt, left, row_y, "Columns");
    row_y += CAPTION_H;
    let cc = &dom.column_config;
    let col_w = 70.0;
    for (i, (label, on)) in [
        ("Bid ord", cc.show_bid_orders),
        ("Sell trd", cc.show_sell_trades),
        ("Buy trd", cc.show_buy_trades),
        ("Ask ord", cc.show_ask_orders),
    ]
    .iter()
    .enumerate()
    {
        modal_btn(
            ctx, coord, &layer_id, tt,
            &format!("col:{}", i),
            left + i as f64 * (col_w + 6.0),
            row_y,
            col_w,
            label,
            *on,
        );
    }
    row_y += BTN_H + ROW_GAP;

    caption(ctx, tt, left, row_y, "Ladder");
    row_y += CAPTION_H;
    modal_btn(ctx, coord, &layer_id, tt, "am", left, row_y, 80.0,
        if dom.auto_center { "Auto" } else { "Manual" }, dom.auto_center);
    let f_label = if dom.min_volume_filter > 0.0 {
        format!("Filter {:.0}", dom.min_volume_filter)
    } else {
        "Filter off".to_string()
    };
    modal_btn(ctx, coord, &layer_id, tt, "filter", left + 88.0, row_y, 90.0, &f_label, dom.min_volume_filter > 0.0);
    modal_btn(ctx, coord, &layer_id, tt, "tick", left + 186.0, row_y, 80.0, &format!("Tick {}", dom.tick_size), false);
    row_y += BTN_H + ROW_GAP;

    caption(ctx, tt, left, row_y, "Multi-exchange roster");
    row_y += CAPTION_H;
    for (i, ex) in dom.multi.config.exchanges.iter().enumerate() {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&tt.item_text);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(ex, left + 4.0, row_y + BTN_H / 2.0);
        modal_btn(ctx, coord, &layer_id, tt, &format!("ex_del:{}", i), mx + DOM_MODAL_W - PAD - 54.0, row_y, 54.0, "Remove", false);
        row_y += BTN_H + 6.0;
    }
    modal_btn(ctx, coord, &layer_id, tt, "ex_add", left, row_y, DOM_MODAL_W - PAD * 2.0, "+ Add next exchange", false);
    row_y += BTN_H + 6.0 + CAPTION_H - 12.0;

    modal_btn(ctx, coord, &layer_id, tt, "grid", left, row_y, 130.0, &format!("Grid {}", dom.multi.config.grid_size), false);
    modal_btn(ctx, coord, &layer_id, tt, "depth", left + 138.0, row_y, 130.0, &format!("Depth {}", dom.multi.config.depth_per_side), false);

    coord.pop_layer(&layer_id);
}
