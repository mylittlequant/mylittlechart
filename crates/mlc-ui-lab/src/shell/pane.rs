//! Empty chart pane: a grid, a legend, a "no data" watermark, a static
//! price-scale label column, and a static time-scale label row. No bars,
//! no primitives, no chart engine — purely decorative geometry mirroring
//! `sim/index.html`'s `#pane`/`#pscale`/`#tscale`/`#corner`.
//!
//! Not interactive — this module never touches `InputCoordinator`.

use mlc_core::i18n::{t_shell, ShellKey};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::theme::RuntimeTheme;
use mlc_core::types::{PRICE_SCALE_WIDTH, TIME_SCALE_HEIGHT};

use super::{ShellRect, STUB_LEGEND_LABEL};

/// Grid spacing — mirrors `sim/app.css`'s own `#pane { background-size:
/// 120px 60px }` (marked NO-TOKEN there: real spacing is data-derived,
/// there is no chart engine here to derive it from).
const GRID_STEP_X: f64 = 120.0;
const GRID_STEP_Y: f64 = 60.0;

/// Placeholder price labels for the static price-scale column, high to low
/// — plausible numbers, not real data (this lab has no data plane).
const STUB_PRICES: &[&str] = &["43,000", "41,500", "40,000", "38,500", "37,000"];
/// Placeholder time labels for the static time-scale row.
const STUB_TIMES: &[&str] = &["00:00", "04:00", "08:00", "12:00", "16:00", "20:00"];

pub(super) fn draw(ctx: &mut dyn RenderContext, rt: &RuntimeTheme, rect: ShellRect, selected_name: Option<&str>) {
    let (rx, ry, rw, rh) = rect;

    ctx.set_fill_color(&rt.chart.background);
    ctx.fill_rect(rx, ry, rw, rh);

    let price_scale_w = PRICE_SCALE_WIDTH.min(rw);
    let time_scale_h = TIME_SCALE_HEIGHT.min(rh);
    let pane_w = (rw - price_scale_w).max(0.0);
    let pane_h = (rh - time_scale_h).max(0.0);

    draw_grid(ctx, rt, rx, ry, pane_w, pane_h);
    draw_legend(ctx, rt, rx, ry);
    draw_watermark(ctx, rt, rx, ry, pane_w, pane_h);
    if let Some(name) = selected_name {
        draw_selected_placeholder(ctx, rt, rx, ry, pane_w, pane_h, name);
    }
    draw_price_scale(ctx, rt, rx + pane_w, ry, pane_h);
    draw_time_scale(ctx, rt, rx, ry + pane_h, pane_w, time_scale_h);
}

fn draw_grid(ctx: &mut dyn RenderContext, rt: &RuntimeTheme, x: f64, y: f64, w: f64, h: f64) {
    ctx.set_stroke_color(&rt.colors.border_light);
    ctx.set_stroke_width(1.0);

    ctx.begin_path();
    let mut gx = x + GRID_STEP_X;
    while gx < x + w {
        ctx.move_to(gx, y);
        ctx.line_to(gx, y + h);
        gx += GRID_STEP_X;
    }
    let mut gy = y + GRID_STEP_Y;
    while gy < y + h {
        ctx.move_to(x, gy);
        ctx.line_to(x + w, gy);
        gy += GRID_STEP_Y;
    }
    ctx.stroke();

    ctx.set_stroke_color(&rt.colors.border_light);
    ctx.begin_path();
    ctx.move_to(x + w, y);
    ctx.line_to(x + w, y + h);
    ctx.move_to(x, y + h);
    ctx.line_to(x + w, y + h);
    ctx.stroke();
}

fn draw_legend(ctx: &mut dyn RenderContext, rt: &RuntimeTheme, x: f64, y: f64) {
    ctx.set_font(&format!("{}px {}", rt.fonts.legend_size as i32, rt.fonts.family_chart));
    ctx.set_fill_color(&rt.colors.text_secondary);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(STUB_LEGEND_LABEL, x + 8.0, y + 6.0);
}

fn draw_watermark(ctx: &mut dyn RenderContext, rt: &RuntimeTheme, x: f64, y: f64, w: f64, h: f64) {
    ctx.set_font(&format!("{}px {}", rt.fonts.size_small as i32, rt.fonts.family));
    ctx.set_fill_color(&rt.colors.text_muted);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_shell(ShellKey::SimulatorWatermark), x + w / 2.0, y + h / 2.0);
}

fn draw_price_scale(ctx: &mut dyn RenderContext, rt: &RuntimeTheme, x: f64, y: f64, h: f64) {
    ctx.set_font(&format!("{}px {}", rt.fonts.size_small as i32, rt.fonts.family_chart));
    ctx.set_fill_color(&rt.colors.text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    let n = STUB_PRICES.len();
    for (i, label) in STUB_PRICES.iter().enumerate() {
        let row_y = y + h * (i as f64 + 1.0) / (n as f64 + 1.0);
        ctx.fill_text(label, x + 5.0, row_y);
    }
}

fn draw_time_scale(ctx: &mut dyn RenderContext, rt: &RuntimeTheme, x: f64, y: f64, w: f64, h: f64) {
    ctx.set_font(&format!("{}px {}", rt.fonts.size_small as i32, rt.fonts.family_chart));
    ctx.set_fill_color(&rt.colors.text_muted);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);

    let n = STUB_TIMES.len();
    for (i, label) in STUB_TIMES.iter().enumerate() {
        let col_x = x + w * (i as f64 + 0.5) / n as f64;
        ctx.fill_text(label, col_x, y + h / 2.0);
    }
}

/// Schematic placeholder for the Object Tree's selected object — a
/// bordered box captioned with its display name, drawn over the middle of
/// the empty pane. Not real primitive geometry (this lab has no chart
/// engine to derive it from) — mirrors `sim/index.html`'s own `.obj` box.
fn draw_selected_placeholder(ctx: &mut dyn RenderContext, rt: &RuntimeTheme, x: f64, y: f64, w: f64, h: f64, name: &str) {
    ctx.set_font(&format!("{}px {}", rt.fonts.size_normal as i32, rt.fonts.family));
    let text_w = ctx.measure_text(name);
    let pad_x = rt.sizing.button_padding_x as f64;
    let box_h = rt.sizing.button_height as f64;
    let box_w = (text_w + pad_x * 2.0).max(box_h * 3.0).min((w - 16.0).max(0.0));
    let bx = x + (w - box_w) / 2.0;
    let by = y + (h - box_h) / 2.0;
    let radius = rt.sizing.border_radius as f64;

    ctx.set_fill_color(&rt.colors.toolbar_bg);
    ctx.fill_rounded_rect(bx, by, box_w, box_h, radius);
    ctx.set_stroke_color(&rt.colors.accent);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(bx, by, box_w, box_h, radius);

    ctx.set_fill_color(&rt.colors.text_primary);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(name, bx + box_w / 2.0, by + box_h / 2.0);
}
