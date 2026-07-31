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
#[allow(clippy::too_many_arguments)]
pub fn render_dom_settings_modal(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    frame_theme: &FrameTheme,
    tt: &ToolbarTheme,
    screen_w: f64,
    screen_h: f64,
    pos: Option<(f64, f64)>,
    dom: &DomState,
    style: mlc_core::theme::PanelStyleParams,
) {
    // Mode-aware surface (owner spec 2026-07-30): the modal carries ONLY
    // what has no toolbar/picker home. Multi mode = Mode switch + bar
    // Texture (skin texture layer manual switch). Single mode = Mode +
    // Skin + Texture + Columns + Ladder.
    let multi = dom.multi_mode;
    let modal_h = if multi {
        HEADER_H + PAD
            + (CAPTION_H + BTN_H + ROW_GAP) * 2.0        // mode / texture
            + CAPTION_H + BTN_H                          // book (no trailing gap)
            + PAD
    } else {
        HEADER_H + PAD
            + (CAPTION_H + BTN_H + ROW_GAP) * 5.0        // mode / texture / book / skin / columns
            + CAPTION_H + BTN_H                          // ladder (no trailing gap)
            + PAD
    };
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

    // Bar texture (skin texture layer manual switch — edits the live
    // theme's panel_style; a skin preset re-applies its own). Cells = L2
    // full-cell intensity, Mosaic = L3 squares (TapeSurf Compact, ref 14).
    {
        use mlc_core::theme::BarTexture;
        caption(ctx, tt, left, row_y, "Texture");
        row_y += CAPTION_H;
        let tex_w = 64.0;
        for (i, (id, label, tex)) in [
            ("tex:gradient", "Gradient", BarTexture::Gradient),
            ("tex:flat", "Flat", BarTexture::Flat),
            ("tex:cells", "Cells", BarTexture::Cells),
            ("tex:mosaic", "Mosaic", BarTexture::Mosaic),
        ]
        .iter()
        .enumerate()
        {
            modal_btn(
                ctx, coord, &layer_id, tt, id,
                left + i as f64 * (tex_w + 6.0), row_y, tex_w,
                label, style.bar_texture == *tex,
            );
        }
        row_y += BTN_H + ROW_GAP;

        // Book row: multi = intensity normalization (TapeSurf Individual
        // vs Cross Exchange, per-panel config); solo = depth Wave + Big
        // marker toggles (Multi-DOM parity). Zebra (theme) in both.
        caption(ctx, tt, left, row_y, "Book");
        row_y += CAPTION_H;
        let mut bx = left;
        if multi {
            let per_col = dom.multi.config.per_column_norm;
            modal_btn(ctx, coord, &layer_id, tt, "norm:cross", bx, row_y, 88.0, "Norm: Cross", !per_col);
            bx += 88.0 + 6.0;
            modal_btn(ctx, coord, &layer_id, tt, "norm:col", bx, row_y, 88.0, "Norm: Col", per_col);
            bx += 88.0 + 6.0;
        } else {
            modal_btn(ctx, coord, &layer_id, tt, "wave", bx, row_y, 70.0, "Wave", dom.show_depth_wave);
            bx += 70.0 + 6.0;
            modal_btn(ctx, coord, &layer_id, tt, "big", bx, row_y, 70.0, "Big", dom.show_big_markers);
            bx += 70.0 + 6.0;
        }
        modal_btn(ctx, coord, &layer_id, tt, "zebra", bx, row_y, 70.0, "Zebra", style.row_zebra);
    }

    if !multi {
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
    }

    coord.pop_layer(&layer_id);
}

/// Agents settings modal (Stage B agents reform, owner spec 2026-07-30) —
/// behind the agents header gear. v1 surface: PTY/Chat mode + backend
/// roster (clicking a backend spawns it, same wire as the header dropdown).
#[allow(clippy::too_many_arguments)]
pub fn render_agents_settings_modal(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    frame_theme: &FrameTheme,
    tt: &ToolbarTheme,
    screen_w: f64,
    screen_h: f64,
    pos: Option<(f64, f64)>,
    is_pty: bool,
    selected_backend: &str,
) {
    let backends = mlc_app_sidebar_content::render::AGENT_BACKENDS;
    let modal_h = HEADER_H
        + PAD
        + CAPTION_H + BTN_H + ROW_GAP
        + CAPTION_H + (BTN_H + 6.0) * backends.len() as f64
        + PAD;
    let (mx, my) = pos.unwrap_or(((screen_w - DOM_MODAL_W) / 2.0, (screen_h - modal_h) / 2.0));
    let mx = mx.max(0.0).min((screen_w - DOM_MODAL_W).max(0.0));
    let my = my.max(0.0).min((screen_h - modal_h).max(0.0));
    let modal_rect = WidgetRect::new(mx, my, DOM_MODAL_W, modal_h);
    let header_rect = WidgetRect::new(mx, my, DOM_MODAL_W, HEADER_H);

    let layer_id = ZLayer::Modal.push_named(coord, "dom_modal");
    coord.register_on_layer("dom_modal:header", header_rect, Sense::DRAG, &layer_id);
    coord.register_on_layer("dom_modal:modal_bg", modal_rect, Sense::CLICK, &layer_id);

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
    ctx.fill_text("Agents Settings", mx + PAD, my + HEADER_H / 2.0);
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

    let left = mx + PAD;
    let mut row_y = my + HEADER_H + PAD;
    caption(ctx, tt, left, row_y, "Spawn mode");
    row_y += CAPTION_H;
    modal_btn(ctx, coord, &layer_id, tt, "agent_mode:pty", left, row_y, 90.0, "PTY", is_pty);
    modal_btn(ctx, coord, &layer_id, tt, "agent_mode:chat", left + 98.0, row_y, 90.0, "Chat", !is_pty);
    row_y += BTN_H + ROW_GAP;

    caption(ctx, tt, left, row_y, "Backend (click to spawn)");
    row_y += CAPTION_H;
    for (id, full, _short) in backends.iter() {
        modal_btn(
            ctx, coord, &layer_id, tt,
            &format!("agent_spawn:{}", id),
            left, row_y, DOM_MODAL_W - PAD * 2.0,
            full,
            selected_backend == *id,
        );
        row_y += BTN_H + 6.0;
    }

    coord.pop_layer(&layer_id);
}

/// Generic data-section settings modal (Trade Tape / L2 / Big Trades) —
/// same chrome and `dom_modal:*` id space as the DOM modal (one section is
/// visible at a time; the click arm routes per the live section kind).
/// Surface: mode switch (Single/Multi roster, owner 2026-07-31) + column
/// visibility (+ Big-Trades thresholds + tape skin).
#[allow(clippy::too_many_arguments)]
pub fn render_section_settings_modal(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    frame_theme: &FrameTheme,
    tt: &ToolbarTheme,
    screen_w: f64,
    screen_h: f64,
    pos: Option<(f64, f64)>,
    title: &str,
    columns: &[(&str, bool)],
    bt_thresholds: Option<(f64, Option<f64>, bool)>,
    skin_label: Option<&str>,
    // `(mode, offers_agg)`: 0 = Single, 1 = Multi, 2 = Aggregated.
    mode: Option<(u8, bool)>,
    imbalance: Option<(bool, u8)>,
    big_flag: Option<Option<f64>>,
    tt_filter: Option<f64>,
) {
    let col_rows = columns.len().div_ceil(4);
    let modal_h = HEADER_H
        + PAD
        + if mode.is_some() { CAPTION_H + BTN_H + ROW_GAP } else { 0.0 }
        + CAPTION_H
        + (BTN_H + 6.0) * col_rows as f64
        + if bt_thresholds.is_some() { (ROW_GAP + CAPTION_H + BTN_H) * 2.0 } else { 0.0 }
        + if imbalance.is_some() { ROW_GAP + CAPTION_H + BTN_H } else { 0.0 }
        + if big_flag.is_some() || tt_filter.is_some() { ROW_GAP + CAPTION_H + BTN_H } else { 0.0 }
        + if skin_label.is_some() { ROW_GAP + CAPTION_H + BTN_H } else { 0.0 }
        + PAD;
    let (mx, my) = pos.unwrap_or(((screen_w - DOM_MODAL_W) / 2.0, (screen_h - modal_h) / 2.0));
    let mx = mx.max(0.0).min((screen_w - DOM_MODAL_W).max(0.0));
    let my = my.max(0.0).min((screen_h - modal_h).max(0.0));
    let modal_rect = WidgetRect::new(mx, my, DOM_MODAL_W, modal_h);
    let header_rect = WidgetRect::new(mx, my, DOM_MODAL_W, HEADER_H);

    let layer_id = ZLayer::Modal.push_named(coord, "dom_modal");
    coord.register_on_layer("dom_modal:header", header_rect, Sense::DRAG, &layer_id);
    coord.register_on_layer("dom_modal:modal_bg", modal_rect, Sense::CLICK, &layer_id);

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
    ctx.fill_text(title, mx + PAD, my + HEADER_H / 2.0);
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

    let left = mx + PAD;
    let mut row_y = my + HEADER_H + PAD;

    // Mode row (M/TT · M/L2 · M/BT, owner 2026-07-31; BT gains the
    // all-venue Aggregated mode, owner 2026-08-01) — same grammar as the
    // DOM modal's DOM/Multi-DOM switch.
    if let Some((cur_mode, offers_agg)) = mode {
        caption(ctx, tt, left, row_y, "Mode");
        row_y += CAPTION_H;
        modal_btn(ctx, coord, &layer_id, tt, "mode:single", left, row_y, 90.0, "Single", cur_mode == 0);
        modal_btn(ctx, coord, &layer_id, tt, "mode:multi", left + 98.0, row_y, 90.0, "Multi", cur_mode == 1);
        if offers_agg {
            modal_btn(ctx, coord, &layer_id, tt, "mode:agg", left + 196.0, row_y, 100.0, "Aggregated", cur_mode == 2);
        }
        row_y += BTN_H + ROW_GAP;
    }

    caption(ctx, tt, left, row_y, "Columns");
    row_y += CAPTION_H;
    let col_w = 70.0;
    for (i, (label, on)) in columns.iter().enumerate() {
        let cx = left + (i % 4) as f64 * (col_w + 6.0);
        if i > 0 && i % 4 == 0 {
            row_y += BTN_H + 6.0;
        }
        modal_btn(ctx, coord, &layer_id, tt, &format!("col:{}", i), cx, row_y, col_w, label, *on);
    }
    row_y += BTN_H + 6.0;

    // Imbalance footer (owner order 2026-07-31; labels self-describing
    // per owner order 2026-08-01 «надо лейблы кнопок сменить»).
    if let Some((imb_on, imb_win)) = imbalance {
        row_y += ROW_GAP - 6.0;
        caption(ctx, tt, left, row_y, "Imbalance footer");
        row_y += CAPTION_H;
        modal_btn(
            ctx, coord, &layer_id, tt, "imb",
            left, row_y, 120.0,
            if imb_on { "Imbalance: on" } else { "Imbalance: off" },
            imb_on,
        );
        modal_btn(
            ctx, coord, &layer_id, tt, "imb_win",
            left + 128.0, row_y, 110.0,
            &format!("Window: {}m", imb_win.max(1)), false,
        );
        row_y += BTN_H;
    }

    // Print controls (TT): big-print flag threshold + draw-time min-qty
    // filter (both click-cycle).
    if big_flag.is_some() || tt_filter.is_some() {
        row_y += ROW_GAP - 6.0;
        caption(ctx, tt, left, row_y, "Prints (click to cycle)");
        row_y += CAPTION_H;
        let mut px = left;
        if let Some(flag) = big_flag {
            let f_label = match flag {
                Some(v) if v >= 1_000_000.0 => format!("Flag ≥ {:.0}M", v / 1_000_000.0),
                Some(v) => format!("Flag ≥ {:.0}K", v / 1_000.0),
                None => "Flag: off".to_string(),
            };
            modal_btn(ctx, coord, &layer_id, tt, "tt_flag", px, row_y, 120.0, &f_label, flag.is_some());
            px += 128.0;
        }
        if let Some(min_qty) = tt_filter {
            let f_label = if min_qty > 0.0 {
                format!("Filter ≥ {}", min_qty)
            } else {
                "Filter: off".to_string()
            };
            modal_btn(ctx, coord, &layer_id, tt, "tt_filter", px, row_y, 120.0, &f_label, min_qty > 0.0);
        }
        row_y += BTN_H;
    }

    if let Some((size_thr, notional_thr, big_markers)) = bt_thresholds {
        row_y += ROW_GAP - 6.0;
        caption(ctx, tt, left, row_y, "Thresholds (click to cycle)");
        row_y += CAPTION_H;
        let s_label = if size_thr > 0.0 { format!("Size ≥ {}", size_thr) } else { "Size: all".to_string() };
        modal_btn(ctx, coord, &layer_id, tt, "bt_size", left, row_y, 130.0, &s_label, size_thr > 0.0);
        let n_label = match notional_thr {
            Some(v) if v >= 1_000_000.0 => format!("Notional ≥ {:.0}M", v / 1_000_000.0),
            Some(v) => format!("Notional ≥ {:.0}K", v / 1_000.0),
            None => "Notional: all".to_string(),
        };
        modal_btn(ctx, coord, &layer_id, tt, "bt_notional", left + 138.0, row_y, 150.0, &n_label, notional_thr.is_some());
        row_y += BTN_H + ROW_GAP - 6.0;
        caption(ctx, tt, left, row_y, "Markers");
        row_y += CAPTION_H;
        modal_btn(
            ctx, coord, &layer_id, tt, "bt_big",
            left, row_y, 120.0,
            if big_markers { "Big markers: on" } else { "Big markers: off" },
            big_markers,
        );
        row_y += BTN_H;
    }

    // Tape skin cycle (Classic / Ramp — DomSkin pattern).
    if let Some(label) = skin_label {
        row_y += ROW_GAP - 6.0;
        caption(ctx, tt, left, row_y, "Skin (click to cycle)");
        row_y += CAPTION_H;
        modal_btn(ctx, coord, &layer_id, tt, "skin", left, row_y, 180.0, label, false);
    }

    coord.pop_layer(&layer_id);
}
