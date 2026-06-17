//! Point & Figure (P&F) chart renderer.
//!
//! Each visible "column" is either an X-column (rising) or an O-column
//! (falling). A new box is added to the current column for every
//! `box_size` price move in the column's direction. Reversal requires
//! `reversal_count × box_size` of opposing movement and starts a new
//! column one slot to the right; the new column's first box is placed
//! one box away from the last box of the previous column.
//!
//! Source: docs/mlc/research/specs/non-standard-bars-spec-2026-06-18.md
//! + mplfinance `_construct_pointnfig` algorithm.

use mlc_core::render::RenderContext;
use mlc_core::types::Bar;
use super::ChartRenderState;

/// One P&F column.
#[derive(Debug, Clone)]
pub struct PnFColumn {
    /// `true` = X column (up), `false` = O column (down).
    pub up: bool,
    /// Bottom price of the lowest box in the column.
    pub bottom: f64,
    /// Top price of the highest box in the column.
    pub top: f64,
    /// Source bar timestamp that opened this column. Used for x-axis
    /// alignment (columns inherit the source bar's slot index).
    pub start_ts: i64,
}

/// Compute X/O columns from OHLCV bars.
/// Reversal count defaults to 3 (industry standard, TradeStation).
pub fn compute_pnf_columns(bars: &[Bar], box_size: f64, reversal_count: u32) -> Vec<PnFColumn> {
    let mut out: Vec<PnFColumn> = Vec::new();
    if bars.is_empty() || box_size <= 0.0 { return out; }
    let reversal = reversal_count.max(1) as f64;

    // First column seeded from first bar.
    let snap_floor = (bars[0].open / box_size).floor() * box_size;
    let mut cur = PnFColumn {
        up: true,
        bottom: snap_floor,
        top: snap_floor + box_size,
        start_ts: bars[0].timestamp,
    };

    for bar in bars.iter().skip(1) {
        // Extend current column or reverse.
        let p = bar.close;
        let move_up_target = cur.top + box_size;
        let move_dn_target = cur.bottom - box_size;
        let reversal_threshold_up = cur.top + box_size * reversal;
        let reversal_threshold_dn = cur.bottom - box_size * reversal;
        if cur.up {
            // Try to extend up first.
            if p >= move_up_target {
                let steps = ((p - cur.top) / box_size).floor() as i64;
                cur.top += steps as f64 * box_size;
                continue;
            }
            // Then check for downward reversal.
            if p <= cur.top - box_size * reversal {
                // Open a new O column starting one box below cur.top.
                out.push(cur.clone());
                let new_top = cur.top - box_size;
                let drop_steps = (((cur.top - box_size) - p) / box_size).floor() as i64 + 1;
                let new_bot = new_top - drop_steps as f64 * box_size;
                cur = PnFColumn {
                    up: false,
                    bottom: new_bot,
                    top: new_top,
                    start_ts: bar.timestamp,
                };
                let _ = reversal_threshold_up;
                continue;
            }
        } else {
            // Down column.
            if p <= move_dn_target {
                let steps = ((cur.bottom - p) / box_size).floor() as i64;
                cur.bottom -= steps as f64 * box_size;
                continue;
            }
            if p >= cur.bottom + box_size * reversal {
                out.push(cur.clone());
                let new_bot = cur.bottom + box_size;
                let rise_steps = ((p - (cur.bottom + box_size)) / box_size).floor() as i64 + 1;
                let new_top = new_bot + rise_steps as f64 * box_size;
                cur = PnFColumn {
                    up: true,
                    bottom: new_bot,
                    top: new_top,
                    start_ts: bar.timestamp,
                };
                let _ = reversal_threshold_dn;
                continue;
            }
        }
    }
    out.push(cur);
    out
}

fn default_box_size(bars: &[Bar]) -> f64 {
    bars.last().map(|b| (b.close * 0.005).max(0.01)).unwrap_or(1.0)
}

/// Draw the P&F chart. Each column is rendered as a vertical stack of
/// boxes. X-columns render the letter "X"; O-columns render "O".
/// Columns are placed at the source bar slot they opened on.
pub fn draw_pnf(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let bars = state.bars;
    if bars.is_empty() { return; }
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let theme = state.theme;
    let rect = &state.chart_rect;

    let box_size = default_box_size(bars);
    let columns = compute_pnf_columns(bars, box_size, 3);
    if columns.is_empty() { return; }

    let bar_w = viewport.bar_width();
    let price_range = price_scale.price_max - price_scale.price_min;
    if price_range <= 0.0 || !price_range.is_finite() { return; }

    let (start, end) = viewport.visible_range();

    // Map column → source bar slot.
    let mut bar_idx: Vec<usize> = Vec::with_capacity(columns.len());
    let mut idx: usize = 0;
    for col in &columns {
        while idx + 1 < bars.len() && bars[idx + 1].timestamp <= col.start_ts { idx += 1; }
        bar_idx.push(idx);
    }

    // Font sized to the box; ensure legibility for both X/O.
    let box_h_px = (rect.height * box_size / price_range).max(6.0);
    let font_size = (box_h_px * 0.8).clamp(7.0, 18.0);
    ctx.set_font(&format!("{:.0}px monospace", font_size));

    for (col_i, col) in columns.iter().enumerate() {
        let src_idx = bar_idx[col_i];
        if src_idx < start || src_idx >= end { continue; }
        let cx = viewport.x_for_bar(src_idx, bars[src_idx].ts_ms()) + rect.x;
        let color = if col.up { &theme.candle_up } else { &theme.candle_down };
        ctx.set_fill_color(color);
        ctx.set_text_align(uzor::render::TextAlign::Center);
        ctx.set_text_baseline(uzor::render::TextBaseline::Middle);

        let glyph = if col.up { "X" } else { "O" };
        // Render boxes top-down (top is highest price).
        let mut p = col.top;
        while p > col.bottom + 1e-9 {
            let y_top = rect.y + viewport.price_to_y(p, price_scale.price_min, price_scale.price_max);
            let y_bot = rect.y + viewport.price_to_y(p - box_size, price_scale.price_min, price_scale.price_max);
            let cy = (y_top + y_bot) * 0.5;
            ctx.fill_text(glyph, cx, cy);
            p -= box_size;
        }
        let _ = bar_w;
    }

    ctx.set_text_align(uzor::render::TextAlign::Left);
    ctx.set_text_baseline(uzor::render::TextBaseline::Top);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn b(t: i64, c: f64) -> Bar {
        Bar { timestamp: t, open: c, high: c, low: c, close: c, volume: 0.0 }
    }

    #[test]
    fn simple_up_column() {
        let bars: Vec<Bar> = (0..6).map(|i| b(i, 100.0 + i as f64)).collect();
        let cols = compute_pnf_columns(&bars, 1.0, 3);
        assert_eq!(cols.len(), 1);
        assert!(cols[0].up);
        assert!((cols[0].bottom - 100.0).abs() < 1e-9);
        assert!((cols[0].top - 105.0).abs() < 1e-9);
    }

    #[test]
    fn reversal_creates_o_column() {
        // Up to 105 then drop to 100 — 5-box drop crosses the 3-box reversal.
        let bars = vec![
            b(0, 100.0), b(1, 102.0), b(2, 104.0), b(3, 105.0), b(4, 100.0)
        ];
        let cols = compute_pnf_columns(&bars, 1.0, 3);
        assert!(cols.len() >= 2);
        assert!(cols[0].up);
        assert!(!cols[1].up);
    }

    #[test]
    fn small_dip_does_not_reverse() {
        // Up to 105 then dip to 104 — 1-box drop < 3-box reversal.
        let bars = vec![b(0, 100.0), b(1, 105.0), b(2, 104.0)];
        let cols = compute_pnf_columns(&bars, 1.0, 3);
        assert_eq!(cols.len(), 1);
        assert!(cols[0].up);
    }
}
