//! Top toolbar: symbol / timeframe / chart-type / indicators / chart
//! settings / undo / redo / layout / template / snapshot, plus the lab-only
//! "Modals" button on the far right. Composition and order mirror
//! `sim/index.html`'s `#top` toolbar exactly.

use mlc_core::i18n::{t_shell, ShellKey};
use mlc_core::render::RenderContext;
use mlc_core::theme::RuntimeTheme;
use uzor::input::{InputCoordinator, LayerId};

use super::{flow_btn, separator, ShellRect, GAP, MODALS_TOGGLE_ID, STUB_SYMBOL_LABEL, STUB_TIMEFRAME_LABEL};

pub(super) fn draw(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer: &LayerId,
    rt: &RuntimeTheme,
    rect: ShellRect,
    modals_active: bool,
) {
    let (rx, ry, rw, rh) = rect;

    ctx.set_fill_color(&rt.colors.toolbar_bg);
    ctx.fill_rect(rx, ry, rw, rh);
    ctx.set_stroke_color(&rt.colors.toolbar_divider);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(rx, ry + rh);
    ctx.line_to(rx + rw, ry + rh);
    ctx.stroke();

    let pad = rt.sizing.button_padding_x as f64;
    let btn_h = rt.sizing.button_height as f64;
    let by = ry + (rh - btn_h) / 2.0;
    let mut x = rx + pad;

    x += flow_btn(ctx, coord, layer, rt, "shell:top:symbol", "search", Some(STUB_SYMBOL_LABEL), x, by, btn_h, false) + GAP;
    x += separator(ctx, rt, x, ry, rh) + GAP;
    x += flow_btn(ctx, coord, layer, rt, "shell:top:tf", "clock", Some(STUB_TIMEFRAME_LABEL), x, by, btn_h, false) + GAP;
    x += separator(ctx, rt, x, ry, rh) + GAP;

    for (id, icon) in [
        ("shell:top:charttype", "candlestick"),
        ("shell:top:indicators", "indicators"),
        ("shell:top:chart_settings", "settings"),
    ] {
        x += flow_btn(ctx, coord, layer, rt, id, icon, None, x, by, btn_h, false) + GAP;
    }
    x += separator(ctx, rt, x, ry, rh) + GAP;

    for (id, icon) in [("shell:top:undo", "undo"), ("shell:top:redo", "redo")] {
        x += flow_btn(ctx, coord, layer, rt, id, icon, None, x, by, btn_h, false) + GAP;
    }
    x += separator(ctx, rt, x, ry, rh) + GAP;

    for (id, icon) in [
        ("shell:top:layout", "layout_single"),
        ("shell:top:template", "bookmark"),
        ("shell:top:snapshot", "screenshot"),
    ] {
        x += flow_btn(ctx, coord, layer, rt, id, icon, None, x, by, btn_h, false) + GAP;
    }

    // Right-aligned "Modals" button (owner spec: icon `grid`, plain button,
    // no menu yet — menu is wave 3).
    let label = t_shell(ShellKey::ModalsButton);
    ctx.set_font(&format!("{}px {}", rt.fonts.size_normal as i32, rt.fonts.family));
    let icon_size = rt.sizing.icon_size as f64;
    let label_w = ctx.measure_text(label);
    let modals_w = pad + icon_size + 6.0 + label_w + pad;
    let modals_x = (rx + rw - pad - modals_w).max(x);
    flow_btn(ctx, coord, layer, rt, MODALS_TOGGLE_ID, "grid", Some(label), modals_x, by, btn_h, modals_active);
}
