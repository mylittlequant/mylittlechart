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
use mlc_app_layout::ui::widgets::{
    render_modal_shell, ModalPlacement, ModalShellSpec, ModalTheme,
};
use mlc_app_panels::trading::order_flow::dom::DomState;
use mlc_core::engine::render::RenderContext;
use mlc_core::render::{TextAlign, TextBaseline};
use uzor::input::{LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::InputCoordinator;

pub const DOM_MODAL_W: f64 = 340.0;
const PAD: f64 = 16.0;
const BTN_H: f64 = 26.0;
const CAPTION_H: f64 = 26.0;
const ROW_GAP: f64 = 20.0;

/// Sidebar modals all wear the toolbar's colours.
fn shell_theme(frame_theme: &FrameTheme, tt: &ToolbarTheme) -> ModalTheme {
    ModalTheme::from_toolbar(&frame_theme.toolbar_bg, &frame_theme.toolbar_border, tt)
}

/// The shared spec: one id space (`dom_modal:*`), one remembered position,
/// height from the rows the caller is about to draw.
fn shell_spec<'a>(title: &'a str, rows_h: f64, pos: Option<(f64, f64)>) -> ModalShellSpec<'a> {
    ModalShellSpec {
        padding: PAD,
        ..ModalShellSpec::new("dom_modal", title, DOM_MODAL_W, rows_h)
            .placed(ModalPlacement::At(pos))
    }
}

/// One accent-style modal button (tc_modal SETUP grammar).
#[allow(clippy::too_many_arguments)]
fn modal_btn(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer: &LayerId,
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
    pid: mlc_core::state::PanelId,
    dom: &DomState,
    style: mlc_core::theme::PanelStyleParams,
) {
    // Mode-aware surface (owner spec 2026-07-30): the modal carries ONLY
    // what has no toolbar/picker home. Multi mode = Mode switch + bar
    // Texture (skin texture layer manual switch). Single mode = Mode +
    // Skin + Texture + Columns + Ladder.
    let multi = dom.multi_mode;
    let rows_h = if multi {
        (CAPTION_H + BTN_H + ROW_GAP) * 2.0              // mode / texture
            + CAPTION_H + BTN_H + ROW_GAP                // book
            + CAPTION_H + BTN_H                          // depth bounds (no trailing gap)
    } else {
        (CAPTION_H + BTN_H + ROW_GAP) * 5.0              // mode / texture / book / skin / columns
            + CAPTION_H + BTN_H                          // ladder (no trailing gap)
    };

    let spec = shell_spec("DOM Settings", rows_h, pos);
    let theme = shell_theme(frame_theme, tt);
    render_modal_shell(ctx, coord, &spec, &theme, (screen_w, screen_h), |ctx, coord, layer_id, shell| {
        let left = shell.content.x;
        let mut row_y = shell.content.y;
        let btn_w = 90.0;

        let mode = dom.mode_index();
        caption(ctx, tt, left, row_y, "Mode");
        row_y += CAPTION_H;
        modal_btn(ctx, coord, layer_id, tt, "mode:dom", left, row_y, btn_w, "DOM", mode == 0);
        modal_btn(ctx, coord, layer_id, tt, "mode:multi", left + btn_w + 8.0, row_y, btn_w + 10.0, "Multi-DOM", mode == 1);
        modal_btn(ctx, coord, layer_id, tt, "mode:agg", left + (btn_w + 8.0) * 2.0 + 10.0, row_y, btn_w + 10.0, "Aggregated", mode == 2);
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
                    ctx, coord, layer_id, tt, id,
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
                modal_btn(ctx, coord, layer_id, tt, "norm:cross", bx, row_y, 88.0, "Norm: Cross", !per_col);
                bx += 88.0 + 6.0;
                modal_btn(ctx, coord, layer_id, tt, "norm:col", bx, row_y, 88.0, "Norm: Col", per_col);
                bx += 88.0 + 6.0;
            } else {
                modal_btn(ctx, coord, layer_id, tt, "wave", bx, row_y, 70.0, "Wave", dom.show_depth_wave);
                bx += 70.0 + 6.0;
                modal_btn(ctx, coord, layer_id, tt, "big", bx, row_y, 70.0, "Big", dom.show_big_markers);
                bx += 70.0 + 6.0;
            }
            modal_btn(ctx, coord, layer_id, tt, "zebra", bx, row_y, 70.0, "Zebra", style.row_zebra);
        }

        // Depth-bounds readout (multi only — Multi-DOM + Aggregated share this
        // `dom.multi` config): total resting size within a configurable band
        // around mid, TapeSurf ref 11/12 parity.
        if multi {
            row_y += BTN_H + ROW_GAP;
            caption(ctx, tt, left, row_y, "Depth bounds");
            row_y += CAPTION_H;
            let db_on = dom.multi.config.show_depth_bounds;
            modal_btn(ctx, coord, layer_id, tt, "depth_bounds", left, row_y, 110.0,
                if db_on { "Depth: on" } else { "Depth: off" }, db_on);
            // Stepper + typed field. The number carries NO unit \u2014 it is a bare
            // percent and the caption already says what the row is; a "%" in the
            // box only gets in the way of typing over it. Arrows walk the preset
            // ladder, clicking the number opens the field, and a typed value is
            // not limited to the ladder.
            let pct_wid = uzor::WidgetId::from(
                format!("{}{}", crate::text_input::DOM_DEPTH_PCT_PREFIX, pid.0),
            );
            let editing = coord.text_fields().is_focused(&pct_wid);
            let pct = dom.multi.config.depth_bounds_pct;
            let pct_label = if editing {
                format!("{}|", coord.text_fields().text(&pct_wid))
            } else if pct > 0.0 {
                format!("{pct}")
            } else {
                format!("auto {:.2}", dom.multi.depth_bounds_effective_pct)
            };
            let mut bx = left + 118.0;
            modal_btn(ctx, coord, layer_id, tt, "depth_dec", bx, row_y, 24.0, "\u{25c2}", false);
            bx += 26.0;
            modal_btn(ctx, coord, layer_id, tt, "depth_pct", bx, row_y, 76.0, &pct_label, editing);
            bx += 78.0;
            modal_btn(ctx, coord, layer_id, tt, "depth_inc", bx, row_y, 24.0, "\u{25b8}", false);
        }

        if !multi {
            row_y += BTN_H + ROW_GAP;

            caption(ctx, tt, left, row_y, "Skin (click to cycle)");
            row_y += CAPTION_H;
            modal_btn(ctx, coord, layer_id, tt, "skin", left, row_y, btn_w * 2.0 + 18.0, &format!("{:?}", dom.skin), false);
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
                    ctx, coord, layer_id, tt,
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
            modal_btn(ctx, coord, layer_id, tt, "am", left, row_y, 80.0,
                if dom.auto_center { "Auto" } else { "Manual" }, dom.auto_center);
            let f_label = if dom.min_volume_filter > 0.0 {
                format!("Filter {:.0}", dom.min_volume_filter)
            } else {
                "Filter off".to_string()
            };
            modal_btn(ctx, coord, layer_id, tt, "filter", left + 88.0, row_y, 90.0, &f_label, dom.min_volume_filter > 0.0);
            modal_btn(ctx, coord, layer_id, tt, "tick", left + 186.0, row_y, 80.0, &format!("Tick {}", dom.tick_size), false);
        }

    });
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
    let rows_h = CAPTION_H + BTN_H + ROW_GAP
        + CAPTION_H + (BTN_H + 6.0) * backends.len() as f64;

    let spec = shell_spec("Agents Settings", rows_h, pos);
    let theme = shell_theme(frame_theme, tt);
    render_modal_shell(ctx, coord, &spec, &theme, (screen_w, screen_h), |ctx, coord, layer_id, shell| {
        let left = shell.content.x;
        let mut row_y = shell.content.y;
        caption(ctx, tt, left, row_y, "Spawn mode");
        row_y += CAPTION_H;
        modal_btn(ctx, coord, layer_id, tt, "agent_mode:pty", left, row_y, 90.0, "PTY", is_pty);
        modal_btn(ctx, coord, layer_id, tt, "agent_mode:chat", left + 98.0, row_y, 90.0, "Chat", !is_pty);
        row_y += BTN_H + ROW_GAP;

        caption(ctx, tt, left, row_y, "Backend (click to spawn)");
        row_y += CAPTION_H;
        for (id, full, _short) in backends.iter() {
            modal_btn(
                ctx, coord, layer_id, tt,
                &format!("agent_spawn:{}", id),
                left, row_y, DOM_MODAL_W - PAD * 2.0,
                full,
                selected_backend == *id,
            );
            row_y += BTN_H + 6.0;
        }

    });
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
    // (size ≥, notional ≥, Some(big-markers-on) — `None` hides the
    // Markers row; the LIQ tape has thresholds but no giant-marker).
    bt_thresholds: Option<(f64, Option<f64>, Option<bool>)>,
    skin_label: Option<&str>,
    // `(mode, offers_agg)`: 0 = Single, 1 = Multi, 2 = Aggregated.
    mode: Option<(u8, bool)>,
    imbalance: Option<(bool, u8)>,
    big_flag: Option<Option<f64>>,
    tt_filter: Option<f64>,
    // (adds, modifies, cancels, executes) row-visibility chips — L2 only.
    l2_events: Option<(bool, bool, bool, bool)>,
) {
    let col_rows = columns.len().div_ceil(4);
    let rows_h = if mode.is_some() { CAPTION_H + BTN_H + ROW_GAP } else { 0.0 }
        // Columns section only when there ARE columns (the LIQ modal
        // rendered an empty caption + a phantom row height that pushed
        // the Skin button past the modal edge — owner defect 2026-08-01).
        + if columns.is_empty() { 0.0 } else { CAPTION_H + (BTN_H + 6.0) * col_rows as f64 }
        + if bt_thresholds.is_some() { ROW_GAP + CAPTION_H + BTN_H } else { 0.0 }
        + if matches!(bt_thresholds, Some((_, _, Some(_)))) { ROW_GAP + CAPTION_H + BTN_H } else { 0.0 }
        + if imbalance.is_some() { ROW_GAP + CAPTION_H + BTN_H } else { 0.0 }
        + if l2_events.is_some() { ROW_GAP + CAPTION_H + BTN_H } else { 0.0 }
        + if big_flag.is_some() || tt_filter.is_some() { ROW_GAP + CAPTION_H + BTN_H } else { 0.0 }
        + if skin_label.is_some() { ROW_GAP + CAPTION_H + BTN_H } else { 0.0 };

    let spec = shell_spec(title, rows_h, pos);
    let theme = shell_theme(frame_theme, tt);
    render_modal_shell(ctx, coord, &spec, &theme, (screen_w, screen_h), |ctx, coord, layer_id, shell| {
        let left = shell.content.x;
        let mut row_y = shell.content.y;

        // Mode row (M/TT · M/L2 · M/BT, owner 2026-07-31; BT gains the
        // all-venue Aggregated mode, owner 2026-08-01) — same grammar as the
        // DOM modal's DOM/Multi-DOM switch.
        if let Some((cur_mode, offers_agg)) = mode {
            caption(ctx, tt, left, row_y, "Mode");
            row_y += CAPTION_H;
            modal_btn(ctx, coord, layer_id, tt, "mode:single", left, row_y, 90.0, "Single", cur_mode == 0);
            modal_btn(ctx, coord, layer_id, tt, "mode:multi", left + 98.0, row_y, 90.0, "Multi", cur_mode == 1);
            if offers_agg {
                modal_btn(ctx, coord, layer_id, tt, "mode:agg", left + 196.0, row_y, 100.0, "Aggregated", cur_mode == 2);
            }
            row_y += BTN_H + ROW_GAP;
        }

        if !columns.is_empty() {
            caption(ctx, tt, left, row_y, "Columns");
            row_y += CAPTION_H;
            let col_w = 70.0;
            for (i, (label, on)) in columns.iter().enumerate() {
                let cx = left + (i % 4) as f64 * (col_w + 6.0);
                if i > 0 && i % 4 == 0 {
                    row_y += BTN_H + 6.0;
                }
                modal_btn(ctx, coord, layer_id, tt, &format!("col:{}", i), cx, row_y, col_w, label, *on);
            }
            row_y += BTN_H + 6.0;
        }

        // Imbalance footer (owner order 2026-07-31; labels self-describing
        // per owner order 2026-08-01 «надо лейблы кнопок сменить»).
        if let Some((imb_on, imb_win)) = imbalance {
            row_y += ROW_GAP - 6.0;
            caption(ctx, tt, left, row_y, "Imbalance footer");
            row_y += CAPTION_H;
            modal_btn(
                ctx, coord, layer_id, tt, "imb",
                left, row_y, 120.0,
                if imb_on { "Imbalance: on" } else { "Imbalance: off" },
                imb_on,
            );
            modal_btn(
                ctx, coord, layer_id, tt, "imb_win",
                left + 128.0, row_y, 110.0,
                &format!("Window: {}m", imb_win.max(1)), false,
            );
            row_y += BTN_H;
        }

        // Event-type row visibility (L2, owner order 2026-08-01 «включение
        // выключение какие именно типы событий показывать»).
        if let Some((adds, mods, cxls, execs)) = l2_events {
            row_y += ROW_GAP - 6.0;
            caption(ctx, tt, left, row_y, "Event types");
            row_y += CAPTION_H;
            modal_btn(ctx, coord, layer_id, tt, "evt:add", left, row_y, 68.0, "Add", adds);
            modal_btn(ctx, coord, layer_id, tt, "evt:mod", left + 74.0, row_y, 68.0, "Modify", mods);
            modal_btn(ctx, coord, layer_id, tt, "evt:cxl", left + 148.0, row_y, 68.0, "Cancel", cxls);
            modal_btn(ctx, coord, layer_id, tt, "evt:exe", left + 222.0, row_y, 68.0, "Execute", execs);
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
                modal_btn(ctx, coord, layer_id, tt, "tt_flag", px, row_y, 120.0, &f_label, flag.is_some());
                px += 128.0;
            }
            if let Some(min_qty) = tt_filter {
                let f_label = if min_qty > 0.0 {
                    format!("Filter ≥ {}", min_qty)
                } else {
                    "Filter: off".to_string()
                };
                modal_btn(ctx, coord, layer_id, tt, "tt_filter", px, row_y, 120.0, &f_label, min_qty > 0.0);
            }
            row_y += BTN_H;
        }

        if let Some((size_thr, notional_thr, big_markers)) = bt_thresholds {
            row_y += ROW_GAP - 6.0;
            caption(ctx, tt, left, row_y, "Thresholds (click to cycle)");
            row_y += CAPTION_H;
            let s_label = if size_thr > 0.0 { format!("Size ≥ {}", size_thr) } else { "Size: all".to_string() };
            modal_btn(ctx, coord, layer_id, tt, "bt_size", left, row_y, 130.0, &s_label, size_thr > 0.0);
            let n_label = match notional_thr {
                Some(v) if v >= 1_000_000.0 => format!("Notional ≥ {:.0}M", v / 1_000_000.0),
                Some(v) => format!("Notional ≥ {:.0}K", v / 1_000.0),
                None => "Notional: all".to_string(),
            };
            modal_btn(ctx, coord, layer_id, tt, "bt_notional", left + 138.0, row_y, 150.0, &n_label, notional_thr.is_some());
            row_y += BTN_H;
            if let Some(big_markers) = big_markers {
                row_y += ROW_GAP - 6.0;
                caption(ctx, tt, left, row_y, "Markers");
                row_y += CAPTION_H;
                modal_btn(
                    ctx, coord, layer_id, tt, "bt_big",
                    left, row_y, 120.0,
                    if big_markers { "Big markers: on" } else { "Big markers: off" },
                    big_markers,
                );
                row_y += BTN_H;
            }
        }

        // Tape skin cycle (Classic / Ramp — DomSkin pattern).
        if let Some(label) = skin_label {
            row_y += ROW_GAP - 6.0;
            caption(ctx, tt, left, row_y, "Skin (click to cycle)");
            row_y += CAPTION_H;
            modal_btn(ctx, coord, layer_id, tt, "skin", left, row_y, 180.0, label, false);
        }

    });
}
