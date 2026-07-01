//! Three Line Break (san-sen-ashi) renderer.
//!
//! Lines arrive via `LiveUpdate::ThreeLineBreakUpdate` from
//! `digdigdig3_station::TradeToThreeLineBreakDerived` and land in
//! `window.three_line_break`. The renderer is a pure drawer — no
//! algorithm lives client-side.
//!
//! Per-line rendering:
//! - One unique slot index per line — slots advance left-to-right in
//!   emission order. The chart's `bar_w` defines the slot width.
//! - Filled rectangle from `low()` to `high()` in price space; color
//!   follows `direction` (0 = up, 1 = down).
//! - 1-px border to visually separate adjacent same-direction lines.
//! - No wicks (the TLB algorithm only tracks close-to-close; intra-line
//!   high/low is defined as `max/min(open, close)` by construction).

use mlc_core::render::RenderContext;
use super::ChartRenderState;

/// Draw Three Line Break lines from `state.three_line_break`.
///
/// Each line gets its own slot — the X axis counts emission order (not
/// calendar time). The renderer shares `viewport.bar_to_x` and
/// `bar_width()` with renko / candle charts.
pub fn draw_three_line_break(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let lines = match state.three_line_break {
        Some(ls) if !ls.is_empty() => ls,
        _ => return,
    };
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let rect = &state.chart_rect;

    let bar_w = viewport.bar_width();
    let price_range = price_scale.price_max - price_scale.price_min;
    if price_range <= 0.0 || !price_range.is_finite() {
        return;
    }

    let (start, end) = viewport.visible_range();
    let end = end.min(lines.len());
    if start >= end {
        return;
    }

    for i in start..end {
        let line = &lines[i];
        let high = line.high();
        let low  = line.low();

        let cx = viewport.bar_to_x(i) + rect.x;
        let top_y = rect.y + viewport.price_to_y(high, price_scale.price_min, price_scale.price_max);
        let bot_y = rect.y + viewport.price_to_y(low,  price_scale.price_min, price_scale.price_max);

        let body_y = top_y.min(bot_y);
        let body_h = (bot_y - top_y).abs().max(1.0);
        let body_x = cx - bar_w * 0.5;

        // Fill: green for up (direction == 0), red for down (direction == 1).
        if line.direction == 0 {
            ctx.set_fill_color(&theme.candle_up);
        } else {
            ctx.set_fill_color(&theme.candle_down);
        }
        ctx.fill_rect(body_x, body_y, bar_w.max(2.0), body_h);

        // 1-px border to separate adjacent same-direction lines.
        ctx.set_stroke_color("rgba(0, 0, 0, 0.35)");
        ctx.set_stroke_width(1.0);
        ctx.stroke_rect(body_x + 0.5, body_y + 0.5, bar_w.max(2.0) - 1.0, body_h - 1.0);
    }
}
