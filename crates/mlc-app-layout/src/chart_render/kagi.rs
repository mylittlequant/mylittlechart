//! Kagi chart renderer — consumes pre-computed segments from the
//! station.
//!
//! Segments arrive via `LiveUpdate::KagiBarUpdate` from
//! `digdigdig3_station::TradeToKagiBarDerived` and land in
//! `window.kagi_segments`. The state machine (seed direction, yang/yin
//! flip at shoulder/waist) is enforced server-side; the renderer is a
//! pure drawer.
//!
//! Layout: vertical segment per slot, with horizontal connectors
//! straddling adjacent segments. Yang segments (thick) render at 3 px;
//! yin (thin) at 1 px. Connectors use a muted gray.

use mlc_core::render::RenderContext;
use super::ChartRenderState;

pub fn draw_kagi(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let segments = match state.kagi_segments {
        Some(s) if !s.is_empty() => s,
        _ => {
            super::draw_waiting_placeholder(ctx, state, "Kagi: waiting for live data…");
            return;
        }
    };
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let rect = &state.chart_rect;

    let (start, end) = viewport.visible_range();
    let end = end.min(segments.len());
    if start >= end { return; }

    ctx.set_line_dash(&[]);

    let mut prev_xy: Option<(f64, f64)> = None;

    for i in start..end {
        let seg = &segments[i];
        let cx = viewport.bar_to_x(i) + rect.x;
        let y_start = rect.y + viewport.price_to_y(
            seg.start_price, price_scale.price_min, price_scale.price_max,
        );
        let y_end = rect.y + viewport.price_to_y(
            seg.end_price, price_scale.price_min, price_scale.price_max,
        );

        if seg.is_connector {
            ctx.set_stroke_color("rgba(120, 144, 156, 0.55)");
            ctx.set_stroke_width(1.0);
            ctx.begin_path();
            if let Some((px, py)) = prev_xy {
                ctx.move_to(px, py);
            } else {
                ctx.move_to(cx, y_start);
            }
            ctx.line_to(cx, y_end);
            ctx.stroke();
        } else {
            let color = if seg.up { &theme.candle_up } else { &theme.candle_down };
            ctx.set_stroke_color(color);
            ctx.set_stroke_width(if seg.yang { 3.0 } else { 1.0 });
            ctx.begin_path();
            ctx.move_to(cx, y_start);
            ctx.line_to(cx, y_end);
            ctx.stroke();

            // Inline horizontal connector to the previous vertical
            // segment's terminal price (drawn as a thin gray stub).
            if let Some((px, py)) = prev_xy {
                ctx.set_stroke_color("rgba(120, 144, 156, 0.55)");
                ctx.set_stroke_width(1.0);
                ctx.begin_path();
                ctx.move_to(px, py);
                ctx.line_to(cx, y_start);
                ctx.stroke();
            }
        }
        prev_xy = Some((cx, y_end));
    }
}
