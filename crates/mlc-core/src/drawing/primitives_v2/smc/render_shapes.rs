//! Small drawing helpers shared between the two NEW smart-money primitives
//! that render structures they did not detect at their own single anchor —
//! [`super::amd_cycle::AmdCycle`] (its own accumulation/manipulation/
//! distribution phases) and [`super::smc_scan::SmcScan`] (every structure
//! kind it surveys).
//!
//! Additive only: nothing here is extracted FROM the four already-committed
//! claim primitives (`order_block`/`fair_value_gap`/`liquidity_sweep`/
//! `structure_break`) — those files are frozen (see the workspace task's own
//! hard rule), so their `render` methods keep their own inlined drawing
//! code untouched. What lives here is the minimum duplication of that same
//! visual grammar (a stroked/filled box for a zone, a level segment with an
//! end marker for a level, a diagonal for a leg) so `amd_cycle` and
//! `smc_scan` read as the same family without reaching into a committed
//! file to get there.

use super::super::{RenderContext, crisp};

/// A filled + stroked rectangle between two bar-index/price corners — the
/// zone language `order_block`/`fair_value_gap` both draw.
pub(super) fn draw_zone(
    ctx: &mut dyn RenderContext,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
    stroke: &str,
    fill_opacity: f64,
    dashed: bool,
) {
    let dpr = ctx.dpr();
    let (min_x, max_x) = (x1.min(x2), x1.max(x2));
    let (min_y, max_y) = (y1.min(y2), y1.max(y2));

    let fill_color = format!("{}{:02x}", &stroke[..7], (fill_opacity.clamp(0.0, 1.0) * 255.0) as u8);
    ctx.set_fill_color(&fill_color);
    ctx.fill_rect(min_x, min_y, max_x - min_x, max_y - min_y);

    ctx.set_stroke_color(stroke);
    ctx.set_stroke_width(1.5);
    let dash = [4.0 * dpr, 3.0 * dpr];
    ctx.set_line_dash(if dashed { &dash } else { &[] });
    ctx.stroke_rect(crisp(min_x, dpr), crisp(min_y, dpr), max_x - min_x, max_y - min_y);
    ctx.set_line_dash(&[]);
}

/// A horizontal level segment with a dot marker at its own right end — the
/// level language `liquidity_sweep`/`structure_break` both draw.
pub(super) fn draw_level(ctx: &mut dyn RenderContext, x1: f64, x2: f64, y: f64, stroke: &str, dashed: bool) {
    let dpr = ctx.dpr();
    ctx.set_stroke_color(stroke);
    ctx.set_stroke_width(2.0);
    let dash = [6.0 * dpr, 3.0 * dpr];
    ctx.set_line_dash(if dashed { &dash } else { &[] });
    ctx.begin_path();
    ctx.move_to(crisp(x1, dpr), crisp(y, dpr));
    ctx.line_to(crisp(x2, dpr), crisp(y, dpr));
    ctx.stroke();
    ctx.set_line_dash(&[]);

    ctx.set_fill_color(stroke);
    ctx.begin_path();
    ctx.arc(x2, y, 3.0 * dpr, 0.0, std::f64::consts::TAU);
    ctx.fill();
}

/// A diagonal line between two points — the leg language `displacement_leg`
/// hits and `amd_cycle`'s own distribution phase use.
pub(super) fn draw_leg(ctx: &mut dyn RenderContext, x1: f64, y1: f64, x2: f64, y2: f64, stroke: &str) {
    let dpr = ctx.dpr();
    ctx.set_stroke_color(stroke);
    ctx.set_stroke_width(2.0);
    ctx.set_line_dash(&[]);
    ctx.begin_path();
    ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
    ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
    ctx.stroke();
}
