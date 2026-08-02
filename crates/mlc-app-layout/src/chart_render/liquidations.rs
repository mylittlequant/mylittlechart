//! Realized-liquidation overlay — the forced closes that actually happened.
//!
//! Rewritten 2026-08-02. The previous revision painted a 250 ms × price grid
//! through the bar-index geometry, which was wrong twice: a liquidation is an
//! INSTANT and has no duration to fill a cell with, and a 250 ms column
//! landed at the x of a whole BAR, so on a 1-minute chart every print sat
//! 240× away from its own time.
//!
//! What it draws now:
//!
//! * **Prints** at their real `(ts, price)`, sized by notional. One forced
//!   close, one mark — a cascade reads as a cascade because the marks stack,
//!   not because a cell got brighter.
//! * **Levels** — horizontal bars over price rows that collected enough
//!   notional, spanning the time between their first and last print. NOT
//!   extending to the right edge: a realized liquidation is history, and a
//!   line running into the future would be claiming something about where
//!   price is going, which is the projected-levels layer's job, not this one.
//!
//! Colour follows MLC's shipped side convention (bridge → core → render):
//! `side == 1` a LONG was liquidated, `side == 0` a SHORT was. Longs take the
//! skin's sell colour and shorts its buy colour — the side that got hurt is
//! the side that was forced to trade against itself.

use mlc_core::chart::liquidations::LiquidationTape;
use mlc_core::render::RenderContext;
use mlc_core::Bar;
use super::ChartRenderState;

/// Minimum on-screen height of a level bar.
const MIN_LEVEL_PX: f64 = 2.0;
/// Minimum on-screen width of a level bar — a level whose prints all landed
/// inside one second still has to be visible.
const MIN_LEVEL_W: f64 = 6.0;

/// Index of the last bar at or before `ts_ms`, or `None` when `ts_ms`
/// predates the series.
fn bar_index_at(bars: &[Bar], ts_ms: i64) -> Option<usize> {
    if bars.is_empty() || bars[0].ts_ms() > ts_ms {
        return None;
    }
    let mut lo = 0usize;
    let mut hi = bars.len();
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if bars[mid].ts_ms() <= ts_ms {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(lo)
}

/// Continuous x for a timestamp: the bar it falls in, plus the fraction of
/// the way through that bar. Prints inside one bar must not collapse onto
/// the same pixel — a cascade is a sequence, and its shape is the point.
fn x_for_ts(state: &ChartRenderState, ts_ms: i64) -> Option<f64> {
    let bars = state.bars;
    let idx = bar_index_at(bars, ts_ms)?;
    let bar_ts = bars[idx].ts_ms();
    let next_ts = bars
        .get(idx + 1)
        .map(|b| b.ts_ms())
        .unwrap_or(bar_ts + state.timeframe_interval_ms.unwrap_or(60_000).max(1));
    let span = (next_ts - bar_ts).max(1) as f64;
    let frac = ((ts_ms - bar_ts) as f64 / span).clamp(0.0, 1.0);
    Some(state.viewport.bar_to_x_f64(idx as f64 + frac) + state.chart_rect.x)
}

/// Levels only — drawn UNDER the series, where a price band belongs.
pub fn draw_liquidation_levels_conditional(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    if let Some(tape) = state.liq_tape_ref {
        draw_liquidations(ctx, state, tape, Layer::Levels);
    }
}

/// Prints only — drawn OVER the series.
///
/// A forced close is an EVENT at a price, not background: the print that
/// matters most is the one at the current price, and under the series that
/// is exactly the one its own candle hides (owner report 2026-08-02 — the
/// prints were landing and the chart still looked empty). Levels stay
/// underneath; they are a band the candles must remain readable through.
pub fn draw_liquidation_prints_conditional(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    if let Some(tape) = state.liq_tape_ref {
        draw_liquidations(ctx, state, tape, Layer::Prints);
    }
}

/// Which half of the layer a call paints.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// Horizontal price bands — under the series.
    Levels,
    /// Per-event marks — over the series.
    Prints,
}

/// Prints + levels.
pub fn draw_liquidations(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    tape: &LiquidationTape,
    layer: Layer,
) {
    if tape.prints.is_empty() || state.bars.is_empty() {
        return;
    }
    let rect = &state.chart_rect;
    let ps = state.price_scale;
    let price_range = ps.price_max - ps.price_min;
    if price_range <= 0.0 || !price_range.is_finite() {
        return;
    }
    let ink = state.liq_ink;
    let theme = state.theme;
    let right_edge = rect.x + rect.width;

    let y_for = |price: f64| rect.y + ((ps.price_max - price) / price_range) * rect.height;

    // Window: the visible bars. Levels fold only what is on screen, so
    // zooming re-scales the picture instead of keeping a level bright
    // because of a cascade that happened hours off-screen.
    let from_ts = state.bars.first().map(|b| b.ts_ms()).unwrap_or(0);
    let to_ts = state
        .bars
        .last()
        .map(|b| b.ts_ms() + state.timeframe_interval_ms.unwrap_or(60_000))
        .unwrap_or(i64::MAX);

    if ink.show_levels && layer == Layer::Levels {
        let row_fraction = (ink.row_pct / 100.0).max(1e-6);
        let levels = tape.levels(row_fraction, ink.min_usd.max(0.0), from_ts, to_ts);
        if !levels.is_empty() {
            // Normalise against the visible maximum — a level's whole meaning
            // is "bigger than the others in view".
            let max_usd = levels
                .iter()
                .map(|l| l.total_usd())
                .fold(1e-9_f64, f64::max)
                / ink.sensitivity.max(0.1);
            let row_px = (rect.height * (levels[0].price * row_fraction) / price_range).max(MIN_LEVEL_PX);
            for l in &levels {
                if l.price < ps.price_min || l.price > ps.price_max {
                    continue;
                }
                let Some(x0) = x_for_ts(state, l.first_ts) else { continue };
                let x1 = x_for_ts(state, l.last_ts).unwrap_or(x0);
                let w = (x1 - x0).max(MIN_LEVEL_W);
                if x0 + w < rect.x || x0 > right_edge {
                    continue;
                }
                let heat = (l.total_usd() / max_usd).clamp(0.0, 1.0);
                let alpha = heat.sqrt() * ink.opacity.clamp(0.05, 1.0);
                let base = if l.long_dominant() { &theme.candle_down } else { &theme.candle_up };
                ctx.set_fill_color(&mlc_core::utils::color::apply_opacity(base, alpha));
                ctx.fill_rect(x0, y_for(l.price) - row_px * 0.5, w, row_px);
            }
        }
    }

    if ink.show_dots && layer == Layer::Prints {
        // A forced close is a PRICE that got taken out, so it draws as a
        // horizontal mark at that price — a line spanning bars either side
        // of the bar it fired on, with the firing point accented (owner
        // spec 2026-08-02: «это как минимум линия должна быть, на несколько
        // баров вправо и влево ... с акцентом на точке где она
        // активировалась»). A bare dot said nothing about the level; the
        // line is what makes a cluster of closes at one price visible as a
        // level rather than as scattered marks.
        //
        // Reach scales with notional, sub-linearly: a 100× bigger close
        // reaches 10× further, so one outlier cannot span the whole screen
        // while everything else collapses to a tick.
        let max_print = tape
            .prints
            .iter()
            .map(|p| p.value_usd)
            .fold(1e-9_f64, f64::max);
        let bar_px = state.viewport.bar_spacing.max(0.5);
        let scale = ink.dot_scale.clamp(0.2, 4.0);
        ctx.set_line_dash(&[]);
        for p in tape.prints.iter() {
            if p.ts_ms < from_ts || p.ts_ms > to_ts {
                continue;
            }
            if p.price < ps.price_min || p.price > ps.price_max {
                continue;
            }
            let Some(x) = x_for_ts(state, p.ts_ms) else { continue };
            let norm = (p.value_usd / max_print).clamp(0.0, 1.0).sqrt();
            // Half-reach in BARS, not pixels: the mark keeps its meaning
            // ("this many bars either side") across every zoom level.
            let half_px = ((0.8 + norm * ink.span_bars.clamp(0.5, 40.0)) * bar_px).min(rect.width);
            let x0 = (x - half_px).max(rect.x);
            let x1 = (x + half_px).min(right_edge);
            if x1 <= rect.x || x0 >= right_edge {
                continue;
            }
            let color = if p.side == 1 { &theme.candle_down } else { &theme.candle_up };
            let cy = y_for(p.price);
            let alpha = ((0.45 + 0.55 * norm) * ink.opacity.clamp(0.05, 1.0)).clamp(0.05, 1.0);

            // The level itself.
            ctx.set_stroke_color(&mlc_core::utils::color::apply_opacity(color, alpha));
            ctx.set_stroke_width((1.0 + norm * 2.0) * scale);
            ctx.begin_path();
            ctx.move_to(x0, cy);
            ctx.line_to(x1, cy);
            ctx.stroke();

            // The accent: where it actually fired. Ringed in the background
            // colour because over a candle of its own hue a bare fill has no
            // edge at all.
            if x >= rect.x && x <= right_edge {
                let r = ((2.0 + norm * 4.0) * scale).clamp(1.5, 12.0);
                ctx.set_fill_color(&mlc_core::utils::color::apply_opacity(color, (alpha + 0.25).min(1.0)));
                ctx.begin_path();
                ctx.arc(x, cy, r, 0.0, std::f64::consts::TAU);
                ctx.close_path();
                ctx.fill();
                ctx.set_stroke_color(&mlc_core::utils::color::apply_opacity(&theme.background, 0.9));
                ctx.set_stroke_width(1.0);
                ctx.begin_path();
                ctx.arc(x, cy, r, 0.0, std::f64::consts::TAU);
                ctx.close_path();
                ctx.stroke();
            }
        }
    }
}
