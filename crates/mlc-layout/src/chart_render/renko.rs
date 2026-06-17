//! Renko brick rendering.
//!
//! Renko transforms a price stream into fixed-height bricks: a new
//! brick is emitted whenever price moves by `brick_size` in one
//! direction; reversals require `reversal_count` × brick_size
//! (default 2 in mlc to match TradeStation's classic 2-brick reversal).
//!
//! Source: docs/mlc/research/specs/non-standard-bars-spec-2026-06-18.md
//! and mplfinance `_construct_renko` algorithm.
//!
//! Algorithm (per visible OHLCV bar, sequential):
//! 1. Seed the first brick from the first bar's open price, snapped to
//!    the nearest `brick_size` grid line.
//! 2. For each subsequent bar's close, walk the brick grid from the
//!    last brick: emit consecutive same-direction bricks while the
//!    price has moved at least one full brick. Reversal requires
//!    `reversal_count` brick crossings before flipping direction.
//! 3. Each emitted brick spans `[base_price, base_price + brick_size]`
//!    (up) or `[base_price - brick_size, base_price]` (down).
//!
//! Pixel pipeline mirrors `draw_candles`: bricks render as filled
//! rectangles aligned to `viewport.bar_to_x(i)`, body height ∝
//! `brick_size / price_range` × chart_height. No wicks (Renko bricks
//! are by definition the same price height; intra-brick volatility is
//! filtered out — this is the whole point of the chart type).

use mlc_core::render::RenderContext;
use mlc_core::types::Bar;
use super::ChartRenderState;

/// One Renko brick with direction.
#[derive(Debug, Clone, Copy)]
pub struct RenkoBrick {
    /// Bottom price of the brick (so `top = bottom + brick_size`).
    pub bottom: f64,
    /// Top price of the brick.
    pub top: f64,
    /// Direction: `true` = up brick, `false` = down brick.
    pub up: bool,
    /// Source bar timestamp this brick closed at — for x-positioning
    /// when the chart still uses kline X axis.
    pub close_ts: i64,
}

/// Convert OHLCV bars into Renko bricks. Default reversal_count = 2
/// (TradeStation classic Renko 2-brick reversal).
pub fn compute_renko_bricks(
    bars: &[Bar],
    brick_size: f64,
    reversal_count: u32,
) -> Vec<RenkoBrick> {
    let mut out = Vec::new();
    if bars.is_empty() || brick_size <= 0.0 {
        return out;
    }
    let reversal = reversal_count.max(1) as f64;

    // Snap to the nearest grid line below the first bar's open.
    let mut anchor = (bars[0].open / brick_size).floor() * brick_size;
    let mut last_dir: Option<bool> = None;

    for bar in bars {
        let close = bar.close;
        let ts = bar.timestamp;
        loop {
            let up_target = anchor + brick_size;
            let down_target = anchor - brick_size;
            // Try up:
            if close >= up_target {
                // Same-direction continuation or reversal threshold met.
                if last_dir == Some(false) {
                    // Reversal — requires `reversal * brick_size` jump.
                    let needed = anchor + brick_size * reversal;
                    if close < needed { break; }
                }
                out.push(RenkoBrick {
                    bottom: anchor,
                    top: up_target,
                    up: true,
                    close_ts: ts,
                });
                anchor = up_target;
                last_dir = Some(true);
                continue;
            }
            // Try down:
            if close <= down_target {
                if last_dir == Some(true) {
                    let needed = anchor - brick_size * reversal;
                    if close > needed { break; }
                }
                out.push(RenkoBrick {
                    bottom: down_target,
                    top: anchor,
                    up: false,
                    close_ts: ts,
                });
                anchor = down_target;
                last_dir = Some(false);
                continue;
            }
            break;
        }
    }
    out
}

/// Default Renko brick size when none is configured: 0.5 % of the
/// last bar's close. Quick heuristic; users can override via the
/// chart-config UI in a follow-up.
fn default_brick_size(bars: &[Bar]) -> f64 {
    bars.last().map(|b| (b.close * 0.005).max(0.01)).unwrap_or(1.0)
}

/// Draw Renko bricks on the chart. Reuses the index-axis layout so the
/// bricks share `viewport.bar_to_x` with all other chart types. Brick
/// X is the index of the source bar at which the brick closed; multiple
/// bricks from one source bar pack horizontally at the same slot —
/// renderer staggers them by a 0.18-bar offset so they don't overlap
/// perfectly.
pub fn draw_renko(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let bars = state.bars;
    if bars.is_empty() { return; }
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let rect = &state.chart_rect;

    let brick_size = default_brick_size(bars);
    let bricks = compute_renko_bricks(bars, brick_size, 2);
    if bricks.is_empty() { return; }

    let bar_w = viewport.bar_width();
    let price_range = price_scale.price_max - price_scale.price_min;
    if price_range <= 0.0 || !price_range.is_finite() { return; }

    // Build a parallel `bar_idx_for_brick`: which source-bar's slot
    // owns this brick. Multiple bricks per bar pack horizontally with
    // a small offset (left-to-right by emission order).
    let mut bar_idx: Vec<(usize, u32)> = Vec::with_capacity(bricks.len());
    let mut last_close_ts: i64 = i64::MIN;
    let mut intra: u32 = 0;
    let mut idx: usize = 0;
    for brick in &bricks {
        // Advance source bar index until its timestamp matches.
        while idx + 1 < bars.len() && bars[idx + 1].timestamp <= brick.close_ts {
            idx += 1;
        }
        if brick.close_ts != last_close_ts {
            intra = 0;
            last_close_ts = brick.close_ts;
        } else {
            intra = intra.saturating_add(1);
        }
        bar_idx.push((idx, intra));
    }

    let (start, end) = viewport.visible_range();
    let off_per_intra = bar_w * 0.18;

    for (i, brick) in bricks.iter().enumerate() {
        let (src_idx, intra) = bar_idx[i];
        if src_idx < start || src_idx >= end { continue; }
        let cx = viewport.x_for_bar(src_idx, bars[src_idx].ts_ms())
            + rect.x
            + (intra as f64 * off_per_intra);

        let top_y = rect.y + viewport.price_to_y(brick.top, price_scale.price_min, price_scale.price_max);
        let bot_y = rect.y + viewport.price_to_y(brick.bottom, price_scale.price_min, price_scale.price_max);
        let body_y = top_y.min(bot_y);
        let body_h = (bot_y - top_y).abs().max(1.0);
        let body_x = cx - bar_w * 0.5;

        if brick.up {
            ctx.set_fill_color(&theme.candle_up);
        } else {
            ctx.set_fill_color(&theme.candle_down);
        }
        ctx.fill_rect(body_x, body_y, bar_w.max(2.0), body_h);

        // 1-px border to separate adjacent same-direction bricks.
        ctx.set_stroke_color("rgba(0, 0, 0, 0.35)");
        ctx.set_stroke_width(1.0);
        ctx.stroke_rect(body_x + 0.5, body_y + 0.5, bar_w.max(2.0) - 1.0, body_h - 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlc_core::Bar;

    fn b(t: i64, c: f64) -> Bar {
        Bar { timestamp: t, open: c, high: c, low: c, close: c, volume: 0.0 }
    }

    #[test]
    fn simple_up_run() {
        // Price walks 100 → 105 in 1-unit steps with brick_size 1.0.
        let bars: Vec<Bar> = (0..6).map(|i| b(i, 100.0 + i as f64)).collect();
        let bricks = compute_renko_bricks(&bars, 1.0, 2);
        assert_eq!(bricks.len(), 5);
        assert!(bricks.iter().all(|x| x.up));
        assert!((bricks[0].bottom - 100.0).abs() < 1e-9);
        assert!((bricks[4].top - 105.0).abs() < 1e-9);
    }

    #[test]
    fn reversal_requires_two_bricks() {
        // Up to 102 then a small dip — not enough to reverse with 2-brick rule.
        let bars = vec![b(0, 100.0), b(1, 101.0), b(2, 102.0), b(3, 101.5)];
        let bricks = compute_renko_bricks(&bars, 1.0, 2);
        assert_eq!(bricks.len(), 2);
        assert!(bricks.iter().all(|x| x.up));
    }

    #[test]
    fn reversal_fires_after_double_jump() {
        // Up to 102, then drop to 99 (3-brick drop) — reverses after 2.
        let bars = vec![b(0, 100.0), b(1, 101.0), b(2, 102.0), b(3, 99.0)];
        let bricks = compute_renko_bricks(&bars, 1.0, 2);
        // 2 up bricks + 2 down bricks (the reversal eats 2*brick_size).
        assert_eq!(bricks.len(), 4);
        assert!(bricks[0].up && bricks[1].up);
        assert!(!bricks[2].up && !bricks[3].up);
    }
}
