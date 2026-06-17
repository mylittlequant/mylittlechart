//! Footprint cluster chart rendering — Bid × Ask split per price level.
//!
//! Textbook spec sourced from
//! `nemo/docs/mlc/research/footprint-cluster-modes-2026-06-17.md`
//! (ATAS "Cluster Chart", Sierra Chart "Numbers Bars", TradingView
//! Volume Footprint, Quantower, NinjaTrader Order Flow+, MotiveWave
//! Volume Imprint, Bookmap Footprint addon, flowsurface-rs,
//! focus1691/orderflow, MQL5 Part 9/11).
//!
//! Implements the minimum-viable mode (ATAS: "the most informative
//! footprint"):
//! - Per price level: `sell_vol × buy_vol` rendered side-by-side,
//!   bid (red) left of bar center, ask (green) right of bar center.
//! - POC row (max total volume) gets a yellow highlight band across
//!   the bar's vertical strip.
//! - Value Area (70% of total volume, greedy bidirectional expansion
//!   from POC) gets a subtle background shade across the whole bar
//!   column.
//! - Imbalance cell on the diagonal: `ask[i] / bid[i-1] >= 3.0` →
//!   accent the ask cell green; symmetric for bid.
//! - At extreme zoom-out (bar width < 40px or font < 7) → collapse to
//!   candle (fall through to `draw_candles`), matching ATAS / Bookmap.
//!
//! Pixel math (verbatim from MQL5 reference):
//! - bar_w = `viewport.bar_width()`
//! - row_h = `chart_height * tick_size / price_range`, min 1px
//! - font_size = `clamp(bar_w / 8.5, 7, 16)`
//! - bid column: right-aligned text at `x - bar_w/4`
//! - ask column: left-aligned text at `x + bar_w/4`
//!
//! Tick size: inferred from the most common price step between adjacent
//! levels of the latest bar with >= 2 levels; falls back to
//! `price_range / 50` if no inference is possible.

use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::types::FootprintBar;
use super::ChartRenderState;
use super::candles::draw_candles;

/// Minimum bar width (px) before we render text. Below this, collapse
/// to a candle visual.
const MIN_BAR_WIDTH_PX: f64 = 40.0;
/// Minimum font size (px) considered legible. Below this, collapse.
const MIN_FONT_SIZE_PX: f64 = 7.0;
/// Imbalance threshold (ratio of one side to the other on the diagonal
/// compare). Industry default across ATAS / Sierra / TradingView is
/// 3.0× (i.e. 300%). Configurable later.
const IMBALANCE_RATIO: f64 = 3.0;
/// Value Area cumulative fraction (industry standard).
const VALUE_AREA_FRACTION: f64 = 0.70;

/// Color palette for the footprint cluster.
struct FpColors<'a> {
    bid: &'a str,
    ask: &'a str,
    bid_imb: &'a str,
    ask_imb: &'a str,
    poc_fill: &'a str,
    va_fill: &'a str,
    cell_bg_dim: &'a str,
}

const DEFAULT_COLORS: FpColors<'static> = FpColors {
    bid: "#ef5350",
    ask: "#26a69a",
    bid_imb: "#ff5252",
    ask_imb: "#69f0ae",
    poc_fill: "rgba(255, 235, 59, 0.18)",
    va_fill: "rgba(120, 144, 156, 0.10)",
    cell_bg_dim: "rgba(120, 144, 156, 0.04)",
};

/// Entry point — `super::candles::draw_footprint` routes here.
pub fn draw_footprint_cluster(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let bars = match state.footprint_bars {
        Some(b) if !b.is_empty() => b,
        _ => {
            // No footprint data yet — show OHLCV candles so the chart is not
            // a blank canvas while the bridge accumulates the first bar.
            draw_candles(ctx, state);
            return;
        }
    };

    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let rect = &state.chart_rect;
    let bar_w = viewport.bar_width();

    // Collapse to candle visual when too zoomed-out for legible text.
    if bar_w < MIN_BAR_WIDTH_PX {
        draw_candles(ctx, state);
        return;
    }

    let font_size = ((bar_w / 8.5).clamp(MIN_FONT_SIZE_PX, 16.0)).round();
    if font_size < MIN_FONT_SIZE_PX {
        draw_candles(ctx, state);
        return;
    }

    let colors = DEFAULT_COLORS;
    let price_range = price_scale.price_max - price_scale.price_min;
    if price_range <= 0.0 || !price_range.is_finite() {
        return;
    }

    let tick_size = infer_tick_size(bars).max(price_range / 200.0);
    let row_h = (rect.height * tick_size / price_range).max(1.0);

    // Visible range — bars share the index-axis with the kline path.
    let (start, end) = viewport.visible_range();
    let end = end.min(bars.len());
    if start >= end {
        return;
    }

    // First: draw a thin OHLC wick line per bar so price excursion is
    // still readable behind the cluster text.
    draw_wicks(ctx, state, bars, start, end, bar_w);

    // Then the per-level cluster grid.
    for i in start..end {
        let bar = &bars[i];
        if bar.levels.is_empty() {
            continue;
        }
        let cx = viewport.bar_to_x(i) + rect.x;
        let cell_left  = cx - bar_w * 0.5;
        let cell_right = cx + bar_w * 0.5;

        // Skip bars entirely off-screen horizontally.
        if cell_right < rect.x || cell_left > rect.x + rect.width {
            continue;
        }

        let poc_idx = bar.poc_idx();
        let va = bar.value_area(VALUE_AREA_FRACTION);

        // Value Area background strip (covers the whole column band).
        if let (Some(poc), Some((va_lo, va_hi))) = (poc_idx, va) {
            let top_price = bar.levels[va_hi].price;
            let bot_price = bar.levels[va_lo].price;
            let top_y = rect.y + price_to_y_local(top_price, price_scale, rect.height);
            let bot_y = rect.y + price_to_y_local(bot_price, price_scale, rect.height) + row_h;
            let h = (bot_y - top_y).max(row_h);
            ctx.set_fill_color(colors.va_fill);
            ctx.fill_rect(cell_left, top_y, bar_w, h);
            let _ = poc;
        }

        // Max volume on this bar (for dim cell-background gradient).
        let max_vol = bar.levels.iter().map(|l| l.total_vol()).fold(0.0_f64, f64::max);

        // Per-level rendering.
        let bid_x = cx - bar_w * 0.25;
        let ask_x = cx + bar_w * 0.25;

        ctx.set_font(&format!("{}px sans-serif", font_size as i32));

        for (li, lv) in bar.levels.iter().enumerate() {
            let y = rect.y + price_to_y_local(lv.price, price_scale, rect.height);
            if y + row_h < rect.y || y > rect.y + rect.height {
                continue;
            }

            // POC row tinted highlight.
            if Some(li) == poc_idx {
                ctx.set_fill_color(colors.poc_fill);
                ctx.fill_rect(cell_left, y, bar_w, row_h);
            } else if max_vol > 0.0 {
                // Faint gradient background by share of bar's max volume.
                let ratio = (lv.total_vol() / max_vol).clamp(0.0, 1.0);
                if ratio > 0.05 {
                    ctx.set_fill_color(colors.cell_bg_dim);
                    ctx.fill_rect(cell_left, y, bar_w, row_h);
                }
            }

            // Diagonal imbalance: ask[i] vs bid[i-1] (ask-imbalance),
            // bid[i] vs ask[i+1] (bid-imbalance). Industry-standard
            // "stacked" logic compares neighbours, not same-row sides.
            let ask_imb = li > 0
                && lv.buy_vol > 0.0
                && bar.levels[li - 1].sell_vol > 0.0
                && lv.buy_vol / bar.levels[li - 1].sell_vol >= IMBALANCE_RATIO;
            let bid_imb = li + 1 < bar.levels.len()
                && lv.sell_vol > 0.0
                && bar.levels[li + 1].buy_vol > 0.0
                && lv.sell_vol / bar.levels[li + 1].buy_vol >= IMBALANCE_RATIO;

            // Bid (sell vol) on the LEFT, right-aligned.
            if lv.sell_vol > 0.0 {
                let color = if bid_imb { colors.bid_imb } else { colors.bid };
                ctx.set_fill_color(color);
                ctx.set_text_align(TextAlign::Right);
                ctx.set_text_baseline(TextBaseline::Middle);
                let label = fmt_vol(lv.sell_vol);
                ctx.fill_text(&label, bid_x, y + row_h * 0.5);
            }
            // Ask (buy vol) on the RIGHT, left-aligned.
            if lv.buy_vol > 0.0 {
                let color = if ask_imb { colors.ask_imb } else { colors.ask };
                ctx.set_fill_color(color);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                let label = fmt_vol(lv.buy_vol);
                ctx.fill_text(&label, ask_x, y + row_h * 0.5);
            }
        }

        // Faint vertical separator between bars so the columns read as
        // distinct objects when many bars are visible.
        ctx.set_stroke_color("rgba(120, 144, 156, 0.18)");
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(cell_right, rect.y);
        ctx.line_to(cell_right, rect.y + rect.height);
        ctx.stroke();
    }

    // Reset text alignment so subsequent renderers do not inherit our
    // RIGHT/LEFT alignment.
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
}

/// Draw a thin wick from each bar's low to high so the candle silhouette
/// remains visible behind the cluster numbers.
fn draw_wicks(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    bars: &[FootprintBar],
    start: usize,
    end: usize,
    bar_w: f64,
) {
    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let rect = &state.chart_rect;
    let theme = state.theme;
    ctx.set_stroke_width(1.0);
    ctx.set_line_dash(&[]);
    for i in start..end {
        let bar = &bars[i];
        let cx = viewport.bar_to_x(i) + rect.x;
        let is_up = bar.is_bullish();
        let color = if is_up { &theme.wick_up } else { &theme.wick_down };
        ctx.set_stroke_color(color);
        let hy = rect.y + price_to_y_local(bar.high, price_scale, rect.height);
        let ly = rect.y + price_to_y_local(bar.low, price_scale, rect.height);
        ctx.begin_path();
        ctx.move_to(cx, hy);
        ctx.line_to(cx, ly);
        ctx.stroke();
        let _ = bar_w;
    }
}

/// Format a volume number compactly. < 100: 1 decimal; >= 1000: K; >= 1e6: M.
fn fmt_vol(v: f64) -> String {
    let v = v.max(0.0);
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1_000.0 {
        format!("{:.1}K", v / 1_000.0)
    } else if v >= 100.0 {
        format!("{:.0}", v)
    } else if v >= 10.0 {
        format!("{:.1}", v)
    } else {
        format!("{:.2}", v)
    }
}

/// Best-effort tick size from level spacing on the latest non-empty bar.
fn infer_tick_size(bars: &[FootprintBar]) -> f64 {
    for bar in bars.iter().rev().take(20) {
        if bar.levels.len() < 2 {
            continue;
        }
        // Smallest positive delta between consecutive levels.
        let mut min_step = f64::INFINITY;
        for w in bar.levels.windows(2) {
            let d = (w[1].price - w[0].price).abs();
            if d > 0.0 && d < min_step {
                min_step = d;
            }
        }
        if min_step.is_finite() {
            return min_step;
        }
    }
    1.0
}

fn price_to_y_local(price: f64, ps: &mlc_core::chart::types::PriceScale, h: f64) -> f64 {
    let range = ps.price_max - ps.price_min;
    if range <= 0.0 {
        return h * 0.5;
    }
    h * (1.0 - (price - ps.price_min) / range)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlc_core::types::{FootprintBar, FootprintLevel};

    fn mk_bar(levels: Vec<(f64, f64, f64)>) -> FootprintBar {
        let lvs: Vec<FootprintLevel> = levels
            .into_iter()
            .map(|(p, b, s)| FootprintLevel { price: p, buy_vol: b, sell_vol: s })
            .collect();
        let total: f64 = lvs.iter().map(|l| l.total_vol()).sum();
        let (open, close) = (lvs.first().map(|l| l.price).unwrap_or(0.0), lvs.last().map(|l| l.price).unwrap_or(0.0));
        let high = lvs.iter().map(|l| l.price).fold(f64::NEG_INFINITY, f64::max);
        let low  = lvs.iter().map(|l| l.price).fold(f64::INFINITY, f64::min);
        FootprintBar { open_time: 0, open, high, low, close, volume: total, levels: lvs }
    }

    #[test]
    fn poc_picks_max_volume_level() {
        let bar = mk_bar(vec![
            (100.0, 1.0, 2.0),
            (101.0, 5.0, 4.0), // POC: 9.0
            (102.0, 0.5, 0.5),
        ]);
        assert_eq!(bar.poc_idx(), Some(1));
    }

    #[test]
    fn value_area_expands_70_percent_around_poc() {
        let bar = mk_bar(vec![
            (100.0, 1.0, 1.0),  // 2
            (101.0, 5.0, 5.0),  // 10 POC
            (102.0, 3.0, 3.0),  // 6
            (103.0, 1.0, 1.0),  // 2
        ]);
        // total=20, target=14. POC=10. Next-highest neighbour 102 (6) → 16 ≥ 14.
        let (lo, hi) = bar.value_area(0.70).unwrap();
        assert_eq!((lo, hi), (1, 2));
    }

    #[test]
    fn infer_tick_size_finds_min_step() {
        let bar = mk_bar(vec![
            (100.0, 1.0, 1.0),
            (100.5, 1.0, 1.0),
            (101.5, 1.0, 1.0),
        ]);
        assert!((infer_tick_size(&[bar]) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn fmt_vol_unit_buckets() {
        assert_eq!(fmt_vol(0.05),   "0.05");
        assert_eq!(fmt_vol(12.5),   "12.5");
        assert_eq!(fmt_vol(345.0),  "345");
        assert_eq!(fmt_vol(1_500.0), "1.5K");
        assert_eq!(fmt_vol(2_500_000.0), "2.5M");
    }
}
