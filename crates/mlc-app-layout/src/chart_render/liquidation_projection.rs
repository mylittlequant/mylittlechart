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
//! A band that has NOT been eaten runs to the NEWEST BAR — that is the whole
//! point of the layer: standing liquidation liquidity ahead of price. An
//! eaten one stops at the bar that ate it. Neither runs into the empty space
//! right of the series: that gap is time that has not elapsed, and a band
//! crossing it asserts the level survived bars that do not exist yet.

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

    // A standing band runs to the last BAR, never to the right edge of the
    // panel. The gap past the newest candle is unelapsed time: a band drawn
    // across it claims the level survived hours that have not happened yet
    // (owner report 2026-08-02 — "он двигается бесконечно в будущее").
    let series_right = (state.viewport.bar_to_x_f64(state.bars.len() as f64) + rect.x)
        .min(right_edge);
    // A STANDING band may be extended past the newest bar by the owner's
    // `future_bars`: the level sits ahead of price, so showing it ahead of
    // the series reads naturally. Measured in bar-widths so it holds its
    // meaning across zoom. A CONSUMED band is never extended — it ended at a
    // bar that exists.
    let bar_px = state.viewport.bar_spacing.max(0.5);
    let standing_right =
        (series_right + ink.future_bars.max(0.0) * bar_px).min(right_edge);

    // Normalise against a HIGH PERCENTILE of the rows in view, not the
    // maximum. The layer's claim is relative ("more pressure sits here than
    // there"), and the distribution is heavy-tailed: a row that stood for
    // days accumulates orders of magnitude more than a fresh one, so
    // dividing by the single heaviest row pushed everything else to
    // invisible. The top decile saturates and the rest keeps its spread.
    // Sensitivity divides the CEILING rather than multiplying the alpha —
    // multiplying drove every mid-sized row to full opacity instead.
    let mut visible: Vec<f64> = levels
        .iter()
        .filter(|l| l.price >= ps.price_min && l.price <= ps.price_max)
        .map(|l| l.notional)
        .collect();
    if visible.is_empty() {
        return;
    }
    visible.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let ceiling = visible[(visible.len() as f64 * 0.9) as usize % visible.len()].max(1e-9)
        / ink.sensitivity.clamp(0.1, 10.0);

    // Rows are drawn SHORT of their true height, leaving a lane between
    // neighbours. Filling the full row height makes a cluster close into one
    // solid block — the layer stops reading as levels and starts reading as
    // a stain over the series, which is what the owner saw.
    let row_px = rect.height * (ps.price_max * (ink.row_pct / 100.0)) / price_range;
    let band_px = (row_px * 0.72).clamp(MIN_BAND_PX, (row_px - 1.0).max(MIN_BAND_PX));

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
        // Uneaten runs to the newest bar — the standing liquidity ahead of
        // price IS the layer. Eaten stops where price took it.
        let x1 = match l.consumed_ts {
            Some(ts) => x_for_ts(state, ts).unwrap_or(series_right).min(series_right),
            None => standing_right,
        };
        let x0 = x0.max(rect.x);
        let x1 = x1.min(right_edge);
        if x1 <= x0 {
            continue;
        }
        // Steeper than linear, with only a whisper of a floor. A heatmap is
        // readable because MOST of it is nearly dark and the clusters carry
        // the ink — a compressive curve (sqrt) plus a generous floor lifts
        // every row to mid-opacity, and 1100 mid-opacity rows are a wash,
        // not a picture.
        let heat = (l.notional / ceiling).clamp(0.0, 1.0).powf(1.3).max(0.04);
        let mut alpha = heat * ink.opacity.clamp(0.02, 1.0);
        if l.consumed() {
            alpha *= 0.5;
        }
        let alpha = alpha.clamp(0.0, 1.0);
        if alpha <= 0.01 {
            continue;
        }
        // Longs die below price and shorts above, so the colour says which
        // side gets hurt. The tokens are the skin's own bid/ask pair — the
        // same ones the DOM ladder and the on-chart depth heat use — so the
        // layer belongs to the skin instead of competing with the series at
        // the candles' saturation.
        //
        // Weight rides the ALPHA only, never a lean toward the `_hot` token:
        // in the shipped skin `heat_ask_hot` is yellow, so leaning turned a
        // heavy long-liquidation row olive and the side it names stopped
        // being legible as a side.
        let tone = if l.side == 1 { &theme.heat_ask } else { &theme.heat_bid };
        let y = y_for(l.price) - band_px * 0.5;

        // The band splits at the newest bar. Everything left of it is over
        // elapsed time — the level demonstrably stood there — and everything
        // right of it is the `future_bars` extension, which is drawing, not
        // evidence. The two halves are painted by different rules.
        let now_x = x1.min(series_right);

        // ── Elapsed half: leans INTO the present, never fades. The far end
        // is where the level still stands and that is the end worth looking
        // at; dimming it would bury the layer's actual claim.
        let wh = now_x - x0;
        if wh > 0.0 {
            if wh < 24.0 {
                ctx.set_fill_color(&mlc_core::utils::color::apply_opacity(tone, alpha));
                ctx.fill_rect(x0, y, wh, band_px);
            } else {
                let step = wh / 3.0;
                for (i, k) in [0.62_f64, 0.82, 1.0].into_iter().enumerate() {
                    ctx.set_fill_color(&mlc_core::utils::color::apply_opacity(tone, alpha * k));
                    ctx.fill_rect(x0 + step * i as f64, y, step + 0.5, band_px);
                }
            }
        }

        // ── Future half: only exists when the owner asked for it, which is
        // also the only case where `fade` has anything to act on. Fading the
        // WHOLE band (the first cut) spent the ramp on elapsed time and left
        // the extension invisible — exactly backwards, since the extension
        // is the uncertain part.
        let wf = x1 - now_x;
        if wf <= 0.0 {
            continue;
        }
        if !ink.fade {
            ctx.set_fill_color(&mlc_core::utils::color::apply_opacity(tone, alpha));
            ctx.fill_rect(now_x, y, wf, band_px);
            continue;
        }
        // Dissolve across the extension. Stepped alpha rather than a real
        // blur: a canvas filter costs a full-layer composite per frame, and
        // a fine enough ramp reads as a fade. Segment count follows width so
        // a long extension dissolves smoothly and a short one does not pay
        // for segments nobody can see.
        let n = ((wf / 10.0).round() as usize).clamp(3, 24);
        let step = wf / n as f64;
        for i in 0..n {
            // Ease out: full where it leaves the last bar, gone at the tip.
            let t = i as f64 / n as f64;
            let a = alpha * (1.0 - t).powf(1.6);
            if a <= 0.01 {
                break;
            }
            ctx.set_fill_color(&mlc_core::utils::color::apply_opacity(tone, a));
            ctx.fill_rect(now_x + step * i as f64, y, step + 0.5, band_px);
        }
    }
}
