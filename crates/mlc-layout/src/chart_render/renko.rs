//! Renko brick renderer — consumes pre-computed bricks from the station.
//!
//! Bricks arrive via `LiveUpdate::RenkoBarUpdate` from
//! `digdigdig3_station::TradeToRenkoBarDerived` and land in
//! `window.renko_bars`. The renderer is a pure drawer — no algorithm
//! lives client-side. The brick-level math (box size, reversal,
//! gap-fill) is enforced once on the trade stream in dig3 and shared by
//! every consumer of the same `(symbol, box_size, reversal)` key.
//!
//! Per-brick rendering:
//! - One unique slot index per brick — slots advance left-to-right in
//!   emission order. The chart's `bar_w` defines the slot width.
//! - Filled rectangle from `brick.bottom` to `brick.top` in price
//!   space; color follows `brick.up`.
//! - 1-px black border to separate adjacent same-direction bricks.

use mlc_core::render::RenderContext;
use super::ChartRenderState;

/// Draw Renko bricks from `state.renko_bars`. Each brick gets its own
/// slot — the X axis ceases to represent calendar time and instead
/// counts emission order.
pub fn draw_renko(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let bricks = match state.renko_bars {
        Some(b) if !b.is_empty() => b,
        _ => return,
    };
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let rect = &state.chart_rect;

    let bar_w = viewport.bar_width();
    let price_range = price_scale.price_max - price_scale.price_min;
    if price_range <= 0.0 || !price_range.is_finite() { return; }

    let (start, end) = viewport.visible_range();
    let end = end.min(bricks.len());
    if start >= end { return; }

    for i in start..end {
        let brick = &bricks[i];
        // Use viewport's bar_to_x — bricks share the index-axis with
        // candle charts; ts is irrelevant for X positioning since each
        // brick owns a slot regardless of when it closed.
        let cx = viewport.bar_to_x(i) + rect.x;
        let top_y = rect.y + viewport.price_to_y(
            brick.top, price_scale.price_min, price_scale.price_max,
        );
        let bot_y = rect.y + viewport.price_to_y(
            brick.bottom, price_scale.price_min, price_scale.price_max,
        );
        let body_y = top_y.min(bot_y);
        let body_h = (bot_y - top_y).abs().max(1.0);
        let body_x = cx - bar_w * 0.5;

        if brick.up {
            ctx.set_fill_color(&theme.candle_up);
        } else {
            ctx.set_fill_color(&theme.candle_down);
        }
        ctx.fill_rect(body_x, body_y, bar_w.max(2.0), body_h);

        ctx.set_stroke_color("rgba(0, 0, 0, 0.35)");
        ctx.set_stroke_width(1.0);
        ctx.stroke_rect(body_x + 0.5, body_y + 0.5, bar_w.max(2.0) - 1.0, body_h - 1.0);
    }
}

