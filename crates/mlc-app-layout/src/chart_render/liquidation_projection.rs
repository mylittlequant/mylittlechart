//! Projected-liquidation overlay — the bands price walks into.
//!
//! Model in `mlc_core::chart::liquidation_projection`; this only paints it.
//! Under the series, like every other background heat: these are levels the
//! candles must stay readable through, not events.
//!
//! Geometry deliberately matches the realized layer's: a horizontal band at
//! a price, running from when the exposure was opened to when price traded
//! through it. Same shape, one measured and one modelled, so the two read as
//! the same kind of statement about a price.
//!
//! A band that has NOT been eaten runs to the right edge — that is the whole
//! point of the layer: standing liquidation liquidity ahead of price. An
//! eaten one stops at the bar that ate it.

use mlc_core::chart::liquidation_projection::ProjectedLevel;
use mlc_core::render::RenderContext;
use mlc_core::Bar;
use super::ChartRenderState;

/// Minimum on-screen band height.
const MIN_BAND_PX: f64 = 1.5;

/// Index of the last bar at or before `ts_ms`.
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

/// Continuous x for a timestamp — same placement rule the realized layer
/// uses, so a projected band and a realized print at the same instant line
/// up instead of drifting by half a bar.
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

/// Draw the layer, conditional on the overlay carrying projected levels.
pub fn draw_liquidation_projection_conditional(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
) {
    if let Some(levels) = state.liq_projection_ref {
        draw_liquidation_projection(ctx, state, levels);
    }
}

/// Paint the bands.
pub fn draw_liquidation_projection(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    levels: &[ProjectedLevel],
) {
    if levels.is_empty() || state.bars.is_empty() {
        return;
    }
    let rect = &state.chart_rect;
    let ps = state.price_scale;
    let price_range = ps.price_max - ps.price_min;
    if price_range <= 0.0 || !price_range.is_finite() {
        return;
    }
    let ink = state.liq_projection_ink;
    let theme = state.theme;
    let right_edge = rect.x + rect.width;
    let y_for = |price: f64| rect.y + ((ps.price_max - price) / price_range) * rect.height;

    // Normalise against the heaviest row IN VIEW: the layer's claim is
    // relative ("more pressure sits here than there"), and a row far off
    // screen must not set the scale for what the owner is looking at.
    let max_notional = levels
        .iter()
        .filter(|l| l.price >= ps.price_min && l.price <= ps.price_max)
        .map(|l| l.notional)
        .fold(1e-9_f64, f64::max);

    let band_px = (rect.height * (ps.price_max * (ink.row_pct / 100.0)) / price_range)
        .max(MIN_BAND_PX);

    for l in levels {
        if l.price < ps.price_min || l.price > ps.price_max {
            continue;
        }
        // An eaten band is history and fades; hide it entirely when the
        // owner only wants what is still standing.
        if l.consumed() && !ink.show_consumed {
            continue;
        }
        let Some(x0) = x_for_ts(state, l.from_ts) else { continue };
        // Uneaten runs to the right edge — the standing liquidity ahead of
        // price IS the layer. Eaten stops where price took it.
        let x1 = match l.consumed_ts {
            Some(ts) => x_for_ts(state, ts).unwrap_or(right_edge),
            None => right_edge,
        };
        let x0 = x0.max(rect.x);
        let x1 = x1.min(right_edge);
        if x1 <= x0 {
            continue;
        }
        let heat = (l.notional / max_notional).clamp(0.0, 1.0).powf(0.5);
        let mut alpha = heat * ink.opacity.clamp(0.02, 1.0) * ink.sensitivity.clamp(0.1, 10.0);
        if l.consumed() {
            alpha *= 0.35;
        }
        let alpha = alpha.clamp(0.0, 1.0);
        if alpha <= 0.01 {
            continue;
        }
        // Longs die below price and shorts above, so the colour says which
        // side gets hurt — the same convention the realized layer uses.
        let base = if l.side == 1 { &theme.candle_down } else { &theme.candle_up };
        ctx.set_fill_color(&mlc_core::utils::color::apply_opacity(base, alpha));
        ctx.fill_rect(x0, y_for(l.price) - band_px * 0.5, x1 - x0, band_px);
    }
}
