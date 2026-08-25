//! The one shape every markup-engine claim draws when its own detector
//! resolved nothing at the bar the analyst claimed.
//!
//! # Why this is not a full-height rule
//!
//! Each of these claims used to fall back to a vertical line spanning
//! `0.0..chart_height` at the anchor's own x. Measured on the live binary:
//! three unresolved claims on one chart produce three rules across the
//! whole plot, and the price action reads through them, not past them.
//! Unresolved is the COMMON state, not the exceptional one — a hand-placed
//! claim resolves only when its anchor is the exact bar the detector names
//! as the event — so that shape is what the tool looks like most of the
//! time it is used.
//!
//! What replaces it is anchored in BOTH axes: a capped segment over the
//! claimed bar's own high..low. It says the same thing (here is the bar you
//! claimed, and the guard's own colour says whether the bars agree) at the
//! scale of the bar it is talking about.

use crate::types::{Bar, find_bar_for_timestamp_ms};
use crate::{PriceScale, Viewport};
use super::{RenderContext, crisp};

/// Half the vertical padding this mark adds beyond the claimed bar's own
/// high/low, in logical pixels — enough that the mark reads as a bracket
/// around the bar rather than a line drawn on top of it.
const BAR_PAD_PX: f64 = 6.0;

/// The cap drawn at each end, in logical pixels — the same 5px cap
/// `CandlePattern::render` puts on its own bracket, so a claim that
/// resolved nothing and a candle-pattern bracket read as one grammar.
const CAP_PX: f64 = 5.0;

/// Half-height of the placeholder used when the anchor resolves to no bar
/// at all (claim anchored outside the loaded series).
const ORPHAN_HALF_PX: f64 = 20.0;

/// Draw the unresolved-claim mark at `x`.
///
/// `bar_span` is the claimed bar's own `(high, low)` in PRICE. `None` means
/// the anchor resolves to no loaded bar — the mark then centres on the
/// chart with a fixed half-height, because there is no bar to bracket.
/// Never spans the chart height in either case.
///
/// The caller owns the colour: `stroke` already carries the guard's own
/// violation recolour where judging applies, and this helper never
/// second-guesses it.
pub(super) fn draw_unresolved_anchor(
    ctx: &mut dyn RenderContext,
    x: f64,
    bar_span: Option<(f64, f64)>,
    stroke: &str,
    width: f64,
) {
    let dpr = ctx.dpr();
    let (y_top, y_bot) = match bar_span {
        Some((high, low)) => (
            ctx.price_to_y(high) - BAR_PAD_PX * dpr,
            ctx.price_to_y(low) + BAR_PAD_PX * dpr,
        ),
        None => {
            let mid = ctx.chart_height() / 2.0;
            (mid - ORPHAN_HALF_PX * dpr, mid + ORPHAN_HALF_PX * dpr)
        }
    };

    ctx.set_stroke_color(stroke);
    ctx.set_stroke_width(width);
    ctx.set_line_dash(&[]);

    ctx.begin_path();
    ctx.move_to(crisp(x, dpr), crisp(y_top, dpr));
    ctx.line_to(crisp(x, dpr), crisp(y_bot, dpr));
    ctx.stroke();

    let cap = CAP_PX * dpr;
    for y in [y_top, y_bot] {
        ctx.begin_path();
        ctx.move_to(crisp(x - cap, dpr), crisp(y, dpr));
        ctx.line_to(crisp(x + cap, dpr), crisp(y, dpr));
        ctx.stroke();
    }
}

/// The claimed bar's own `(high, low)`, resolved from a millisecond anchor
/// through the SAME [`find_bar_for_timestamp_ms`] every claim in this
/// family already uses for its own anchor index — so the bar this mark
/// brackets and the bar the detector was asked about are always the same
/// bar. `None` when the anchor names no loaded bar.
pub(super) fn bar_span_at(bars: &[Bar], ts_ms: i64) -> Option<(f64, f64)> {
    let idx = find_bar_for_timestamp_ms(bars, ts_ms)?;
    bars.get(idx).map(|b| (b.high, b.low))
}

/// The y a bar-anchored claim's own control point sits at when the claim
/// resolved nothing.
///
/// The middle of the CLAIMED BAR, not the middle of the viewport. The
/// viewport's own centre was the old fallback in eight of these claims, and
/// it is wrong for the same reason the full-height rule this module
/// replaced was wrong: an unresolved claim is the common state, and a
/// handle parked at the centre of the visible price range detaches from the
/// bar it belongs to and slides across the chart on every scroll and zoom,
/// while the claim itself does not move at all. Grabbing that handle means
/// hunting for it.
///
/// The viewport's centre survives only as the last resort, for an anchor
/// naming no loaded bar — there is genuinely no bar to sit on then, the
/// same case [`draw_unresolved_anchor`] draws its orphan placeholder for.
pub(super) fn anchor_control_y(bars: &[Bar], ts_ms: i64, vp: &Viewport, ps: &PriceScale) -> f64 {
    match bar_span_at(bars, ts_ms) {
        Some((high, low)) => vp.price_to_y((high + low) / 2.0, ps.price_min, ps.price_max),
        None => vp.price_to_y((ps.price_min + ps.price_max) / 2.0, ps.price_min, ps.price_max),
    }
}
