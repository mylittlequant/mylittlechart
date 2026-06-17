//! Kagi chart renderer — variable-thickness directional polyline.
//!
//! Kagi traces the price as a vertical-horizontal polyline. The line
//! thickness ("yang" thick, "yin" thin) flips when the line crosses
//! the previous turning point's extreme (shoulder for an up-flip,
//! waist for a down-flip). Reversal of direction itself happens when
//! the price moves at least `reversal` away from the current segment's
//! extreme; below that, the current segment is extended.
//!
//! Sources:
//! - docs/mlc/research/specs/non-standard-bars-spec-2026-06-18.md
//! - https://en.wikipedia.org/wiki/Kagi_chart
//! - StockCharts: https://chartschool.stockcharts.com/.../kagi-charts
//!
//! Pixel pipeline: vertical "stem" per segment (top-to-bottom of the
//! segment's price excursion) + a horizontal "shoulder/waist" stub
//! connecting it to the next segment when a reversal fires. Color is
//! green for yang (up-strong) and red for yin (down-strong); the
//! thickness reflects the same.

use mlc_core::render::RenderContext;
use mlc_core::types::Bar;
use super::ChartRenderState;

/// One Kagi segment.
#[derive(Debug, Clone, Copy)]
pub struct KagiSegment {
    /// `true` = up-segment (line moves higher), `false` = down.
    pub up: bool,
    /// `true` = yang (thick), `false` = yin (thin).
    pub yang: bool,
    /// Segment start price.
    pub start_price: f64,
    /// Segment end price.
    pub end_price: f64,
    /// Source bar timestamp this segment closed at.
    pub close_ts: i64,
}

/// Compute Kagi segments from a Bar stream.
///
/// `reversal` is the absolute price move required to flip direction;
/// callers that prefer a percentage should pass `last_close * pct` etc.
/// Default = 1% of last close.
pub fn compute_kagi(bars: &[Bar], reversal: f64) -> Vec<KagiSegment> {
    let mut out: Vec<KagiSegment> = Vec::new();
    if bars.is_empty() || reversal <= 0.0 { return out; }

    // Find the first directional move large enough to seed the first
    // segment.
    let start_price = bars[0].close;
    let mut anchor = start_price;
    let mut up: bool = true; // seed direction; updated on first seed-move
    let mut last_yang = true;
    // Track the last shoulder (highest peak) and waist (lowest trough)
    // so we can flip thickness on cross.
    let mut last_shoulder = f64::NEG_INFINITY;
    let mut last_waist = f64::INFINITY;

    let mut seeded = false;

    for bar in bars.iter().skip(1) {
        let p = bar.close;
        let ts = bar.timestamp;
        if !seeded {
            if (p - anchor).abs() >= reversal {
                up = p > anchor;
                seeded = true;
                if up { last_shoulder = anchor; } else { last_waist = anchor; }
            } else {
                continue;
            }
        }
        // Extend current segment if price keeps moving in our direction.
        if up && p > anchor {
            anchor = p;
            continue;
        }
        if !up && p < anchor {
            anchor = p;
            continue;
        }
        // Otherwise, candidate reversal: price moved opposite by >= reversal.
        let move_against = if up { anchor - p } else { p - anchor };
        if move_against >= reversal {
            // Close out the current segment.
            let prev_anchor = if up { last_shoulder.min(start_price) } else { last_waist.max(start_price) };
            // Determine yang flip.
            let yang = if up {
                // Up segments turn yang when price > last_shoulder.
                anchor > last_shoulder
            } else {
                // Down segments turn yin when price < last_waist.
                anchor < last_waist
            };
            // Persist last extremum.
            if up { last_shoulder = anchor; } else { last_waist = anchor; }
            out.push(KagiSegment {
                up,
                yang,
                start_price: prev_anchor,
                end_price: anchor,
                close_ts: ts,
            });
            // Flip direction.
            up = !up;
            anchor = p;
            last_yang = yang;
        }
    }
    let _ = last_yang;
    // Flush trailing open segment if there is one.
    if seeded {
        let prev_anchor = if up { last_shoulder.min(start_price) } else { last_waist.max(start_price) };
        let yang = if up {
            anchor > last_shoulder.max(f64::NEG_INFINITY)
        } else {
            anchor < last_waist.min(f64::INFINITY)
        };
        out.push(KagiSegment {
            up,
            yang,
            start_price: prev_anchor,
            end_price: anchor,
            close_ts: bars.last().unwrap().timestamp,
        });
    }
    out
}

fn default_reversal(bars: &[Bar]) -> f64 {
    bars.last().map(|b| (b.close * 0.01).max(0.01)).unwrap_or(1.0)
}

/// Draw the Kagi chart. Each segment maps to its source bar's slot;
/// segments stack horizontally by slot index. The connecting
/// shoulder/waist between two consecutive segments is drawn as a
/// thin horizontal stub linking the new column's X to the old column's
/// extreme price.
pub fn draw_kagi(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let bars = state.bars;
    if bars.is_empty() { return; }
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let rect = &state.chart_rect;

    let reversal = default_reversal(bars);
    let segments = compute_kagi(bars, reversal);
    if segments.is_empty() { return; }

    let (start, end) = viewport.visible_range();

    // Map segment → source bar slot.
    let mut bar_idx: Vec<usize> = Vec::with_capacity(segments.len());
    let mut idx: usize = 0;
    for seg in &segments {
        while idx + 1 < bars.len() && bars[idx + 1].timestamp <= seg.close_ts { idx += 1; }
        bar_idx.push(idx);
    }

    ctx.set_line_dash(&[]);

    for (s_i, seg) in segments.iter().enumerate() {
        let src_idx = bar_idx[s_i];
        if src_idx < start || src_idx >= end { continue; }
        let cx = viewport.x_for_bar(src_idx, bars[src_idx].ts_ms()) + rect.x;
        let y_start = rect.y + viewport.price_to_y(seg.start_price, price_scale.price_min, price_scale.price_max);
        let y_end = rect.y + viewport.price_to_y(seg.end_price, price_scale.price_min, price_scale.price_max);
        let color = if seg.up { &theme.candle_up } else { &theme.candle_down };
        ctx.set_stroke_color(color);
        ctx.set_stroke_width(if seg.yang { 3.0 } else { 1.0 });
        ctx.begin_path();
        ctx.move_to(cx, y_start);
        ctx.line_to(cx, y_end);
        ctx.stroke();

        // Horizontal connector to the previous segment's extreme (if any).
        if s_i > 0 {
            let prev_src_idx = bar_idx[s_i - 1];
            if prev_src_idx >= start && prev_src_idx < end {
                let prev_cx = viewport.x_for_bar(prev_src_idx, bars[prev_src_idx].ts_ms()) + rect.x;
                ctx.set_stroke_color("rgba(120, 144, 156, 0.55)");
                ctx.set_stroke_width(1.0);
                ctx.begin_path();
                ctx.move_to(prev_cx, y_start);
                ctx.line_to(cx, y_start);
                ctx.stroke();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn b(t: i64, c: f64) -> Bar {
        Bar { timestamp: t, open: c, high: c, low: c, close: c, volume: 0.0 }
    }

    #[test]
    fn single_uptrend_one_segment() {
        let bars: Vec<Bar> = (0..5).map(|i| b(i, 100.0 + i as f64 * 2.0)).collect();
        let segs = compute_kagi(&bars, 1.0);
        // 1 trailing segment (no reversal yet).
        assert!(segs.iter().all(|s| s.up));
        assert!(segs.len() >= 1);
    }

    #[test]
    fn reversal_creates_two_segments() {
        let bars = vec![b(0, 100.0), b(1, 108.0), b(2, 95.0)];
        let segs = compute_kagi(&bars, 2.0);
        assert!(segs.len() >= 2);
        assert!(segs[0].up);
        assert!(!segs[1].up);
    }
}
