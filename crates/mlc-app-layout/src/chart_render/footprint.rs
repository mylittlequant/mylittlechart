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

/// Stacked-imbalance + absorption + CVD palette (F1, F2, F5).
struct FpExtraColors<'a> {
    stacked_ask_fill: &'a str,
    stacked_bid_fill: &'a str,
    absorption_fill: &'a str,
    cvd_up: &'a str,
    cvd_down: &'a str,
    cvd_axis: &'a str,
}

const EXTRA_COLORS: FpExtraColors<'static> = FpExtraColors {
    stacked_ask_fill: "rgba(105, 240, 174, 0.22)",
    stacked_bid_fill: "rgba(255, 82, 82, 0.22)",
    absorption_fill: "rgba(255, 152, 0, 0.16)",
    cvd_up: "#26a69a",
    cvd_down: "#ef5350",
    cvd_axis: "rgba(120, 144, 156, 0.45)",
};

/// Height (px) of the CVD sub-strip drawn at the bottom of the chart
/// area when ≥ 2 footprint bars are visible. Reserved out of the chart
/// vertical extent — bars render above it.
const CVD_STRIP_HEIGHT_PX: f64 = 28.0;
/// Minimum chart height (px) below which we skip the CVD strip.
const CVD_MIN_CHART_HEIGHT_PX: f64 = 160.0;

/// Entry point — `super::candles::draw_footprint` routes here.
pub fn draw_footprint_cluster(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    let bars = match state.footprint_bars {
        Some(b) if !b.is_empty() => b,
        _ => {
            // No footprint data yet — show explicit empty-state instead of
            // pretending with candles. Owner needs to see "we have nothing"
            // not a fake series.
            eprintln!("[fp-render] state.footprint_bars empty/None — drawing No-data placeholder");
            super::draw_waiting_placeholder(ctx, state, "Footprint: waiting for data…");
            return;
        }
    };

    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let rect = &state.chart_rect;
    let bar_w = viewport.bar_width();

    eprintln!(
        "[fp-render] enter: bars.len()={} bar_w={:.2} chart_rect=({:.0}x{:.0}) price_range={:.2}",
        bars.len(), bar_w, rect.width, rect.height, price_scale.price_max - price_scale.price_min,
    );

    if bar_w < MIN_BAR_WIDTH_PX {
        eprintln!("[fp-render] bar_w {:.2} < MIN_BAR_WIDTH_PX {:.0} — drawing zoom-out placeholder", bar_w, MIN_BAR_WIDTH_PX);
        super::draw_waiting_placeholder(ctx, state, "Footprint: zoom in to see clusters");
        return;
    }

    let font_size = ((bar_w / 8.5).clamp(MIN_FONT_SIZE_PX, 16.0)).round();
    if font_size < MIN_FONT_SIZE_PX {
        eprintln!("[fp-render] font_size {:.0} < MIN — drawing zoom-out placeholder", font_size);
        super::draw_waiting_placeholder(ctx, state, "Footprint: zoom in to see clusters");
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

    // Reserve a strip at the bottom for the CVD line (F5) when there's
    // space. The cluster + wicks render in the area above the strip.
    let cvd_strip = if rect.height >= CVD_MIN_CHART_HEIGHT_PX && end - start >= 2 {
        Some((rect.y + rect.height - CVD_STRIP_HEIGHT_PX, CVD_STRIP_HEIGHT_PX))
    } else {
        None
    };

    // First: draw a thin OHLC wick line per bar so price excursion is
    // still readable behind the cluster text.
    draw_wicks(ctx, state, bars, start, end, bar_w);

    // Then the per-level cluster grid. Bars with empty levels (historical
    // klines-only window — no per-trade data that far back) fall back to a
    // plain OHLC candle body so history reads as candles until clusters
    // exist; wick lines are already drawn above for every bar.
    let mut bars_drawn = 0_usize;
    let mut bars_skipped_empty = 0_usize;
    for i in start..end {
        let bar = &bars[i];
        if bar.levels.is_empty() {
            bars_skipped_empty += 1;
            let cx = viewport.bar_to_x(i) + rect.x;
            let body_w = (bar_w * 0.6).max(1.0);
            if cx + body_w * 0.5 < rect.x || cx - body_w * 0.5 > rect.x + rect.width {
                continue;
            }
            let theme = state.theme;
            let up = bar.close >= bar.open;
            let oy = rect.y + price_to_y_local(bar.open, price_scale, rect.height);
            let cy = rect.y + price_to_y_local(bar.close, price_scale, rect.height);
            ctx.set_fill_color(if up { &theme.candle_up } else { &theme.candle_down });
            ctx.fill_rect(cx - body_w * 0.5, oy.min(cy), body_w, (oy - cy).abs().max(1.0));
            continue;
        }
        bars_drawn += 1;
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

        // F2: absorption bar tint clipped to the actual price excursion
        // (high → low) per MQL5 Part 11 spec. The predecessor painted
        // the full chart vertical; that washed out everything else when
        // an absorption bar was visible.
        if bar.is_absorption {
            let hy = rect.y + price_to_y_local(bar.high, price_scale, rect.height);
            let ly = rect.y + price_to_y_local(bar.low, price_scale, rect.height) + row_h;
            ctx.set_fill_color(EXTRA_COLORS.absorption_fill);
            ctx.fill_rect(cell_left, hy.min(ly), bar_w, (ly - hy).abs().max(row_h));
        }

        // F1: stacked imbalance zones — tinted row band across the
        // stacked levels (ascending price). Drawn AFTER VA so the stack
        // dominates the visual when both regions overlap. Uses the real
        // streak length recorded by compute_extras instead of the
        // hardcoded +2 the predecessor assumed.
        if let Some(start_idx) = bar.stacked_ask_start {
            let len = bar.stacked_ask_len.unwrap_or(3).max(1);
            let hi_idx = (start_idx + len - 1).min(bar.levels.len() - 1);
            let lo_lv = &bar.levels[start_idx];
            let hi_lv = &bar.levels[hi_idx];
            let top_y = rect.y + price_to_y_local(hi_lv.price, price_scale, rect.height);
            let bot_y = rect.y + price_to_y_local(lo_lv.price, price_scale, rect.height) + row_h;
            ctx.set_fill_color(EXTRA_COLORS.stacked_ask_fill);
            ctx.fill_rect(cell_left, top_y, bar_w, (bot_y - top_y).max(row_h));
        }
        if let Some(start_idx) = bar.stacked_bid_start {
            let len = bar.stacked_bid_len.unwrap_or(3).max(1);
            let hi_idx = (start_idx + len - 1).min(bar.levels.len() - 1);
            let lo_lv = &bar.levels[start_idx];
            let hi_lv = &bar.levels[hi_idx];
            let top_y = rect.y + price_to_y_local(hi_lv.price, price_scale, rect.height);
            let bot_y = rect.y + price_to_y_local(lo_lv.price, price_scale, rect.height) + row_h;
            ctx.set_fill_color(EXTRA_COLORS.stacked_bid_fill);
            ctx.fill_rect(cell_left, top_y, bar_w, (bot_y - top_y).max(row_h));
        }

        // Max volume on this bar (for dim cell-background gradient).
        let max_vol = bar.levels.iter().map(|l| l.total_vol()).fold(0.0_f64, f64::max);
        // F3: 30% significance gate — only color cells whose own side's
        // share of the bar's max same-side volume is meaningful. Stops
        // tiny-cell imbalances from over-coloring.
        let max_buy = bar.levels.iter().map(|l| l.buy_vol).fold(0.0_f64, f64::max);
        let max_sell = bar.levels.iter().map(|l| l.sell_vol).fold(0.0_f64, f64::max);
        let sig_buy_gate = max_buy * 0.30;
        let sig_sell_gate = max_sell * 0.30;

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

            // F4: single print dashed border.
            if lv.is_single_print {
                ctx.set_stroke_color("rgba(255, 255, 255, 0.55)");
                ctx.set_stroke_width(1.0);
                ctx.set_line_dash(&[3.0, 2.0]);
                ctx.stroke_rect(cell_left + 0.5, y + 0.5, bar_w - 1.0, row_h - 1.0);
                ctx.set_line_dash(&[]);
            }
            // F4: unfinished auction dot marker (at bar extreme with one
            // side zero) — placed at the relevant cell edge.
            if lv.is_unfinished {
                ctx.set_fill_color("#ffd54f");
                let dot_r = (row_h * 0.18).clamp(2.0, 4.0);
                let dx = if lv.buy_vol == 0.0 { cell_right - dot_r - 1.0 } else { cell_left + dot_r + 1.0 };
                let dy = y + row_h * 0.5;
                ctx.begin_path();
                ctx.arc(dx, dy, dot_r, 0.0, std::f64::consts::TAU);
                ctx.fill();
            }

            // Diagonal imbalance: ask[i] vs bid[i-1] (ask-imbalance),
            // bid[i] vs ask[i+1] (bid-imbalance). With 30% significance
            // gate (F3): the *current* side's volume must clear 30% of
            // the bar's same-side max before we color.
            let ask_imb = li > 0
                && lv.buy_vol > sig_buy_gate
                && bar.levels[li - 1].sell_vol > 0.0
                && lv.buy_vol / bar.levels[li - 1].sell_vol >= IMBALANCE_RATIO;
            let bid_imb = li + 1 < bar.levels.len()
                && lv.sell_vol > sig_sell_gate
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

        // F1 zone-edge triangle markers (layer 8).
        if bar.stacked_ask_start.is_some() {
            ctx.set_fill_color(colors.ask_imb);
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.fill_text("▲", cx, rect.y + 2.0);
        }
        if bar.stacked_bid_start.is_some() {
            ctx.set_fill_color(colors.bid_imb);
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Bottom);
            ctx.fill_text("▼", cx, rect.y + rect.height - 2.0);
        }

        // F2: "ABS" info label on absorption bars.
        if bar.is_absorption {
            ctx.set_fill_color("rgba(255, 152, 0, 0.95)");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.set_font(&format!("{}px sans-serif", (font_size as i32 - 1).max(7)));
            ctx.fill_text("ABS", cx, rect.y + 14.0);
        }

        // Faint vertical separator between bars.
        ctx.set_stroke_color("rgba(120, 144, 156, 0.18)");
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(cell_right, rect.y);
        ctx.line_to(cell_right, rect.y + rect.height);
        ctx.stroke();
    }

    eprintln!(
        "[fp-render] visible_range=[{}..{}) drawn_with_clusters={} skipped_empty_levels={} tick_size={:.4} row_h={:.2}",
        start, end, bars_drawn, bars_skipped_empty, tick_size, row_h,
    );

    // F5: CVD sub-strip across the visible window.
    if let Some((strip_y, strip_h)) = cvd_strip {
        draw_cvd_strip(ctx, state, bars, start, end, bar_w, strip_y, strip_h);
    }

    // Reset text alignment so subsequent renderers do not inherit our
    // RIGHT/LEFT alignment.
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_line_dash(&[]);
}

/// CVD line drawn into a reserved horizontal strip at the bottom of the
/// chart area (F5). Auto-scales to min/max of the visible CVD range so
/// the line uses the full strip vertically. Color flips: green when CVD
/// is currently rising, red when falling, segment-by-segment.
fn draw_cvd_strip(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    bars: &[FootprintBar],
    start: usize,
    end: usize,
    _bar_w: f64,
    strip_y: f64,
    strip_h: f64,
) {
    let rect = &state.chart_rect;
    let viewport = state.viewport;

    // Min/max CVD across the visible window.
    let (mut cvd_min, mut cvd_max) = (f64::INFINITY, f64::NEG_INFINITY);
    for i in start..end {
        let v = bars[i].cumulative_delta;
        if v < cvd_min { cvd_min = v; }
        if v > cvd_max { cvd_max = v; }
    }
    if !cvd_min.is_finite() || !cvd_max.is_finite() { return; }
    let span = (cvd_max - cvd_min).max(1e-9);

    // Strip background.
    ctx.set_fill_color("rgba(0, 0, 0, 0.18)");
    ctx.fill_rect(rect.x, strip_y, rect.width, strip_h);

    // Zero / baseline line.
    let zero_y = strip_y + strip_h * (1.0 - (-cvd_min / span)).clamp(0.0, 1.0);
    ctx.set_stroke_color(EXTRA_COLORS.cvd_axis);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(rect.x, zero_y);
    ctx.line_to(rect.x + rect.width, zero_y);
    ctx.stroke();

    // Line segments.
    ctx.set_stroke_width(1.5);
    let mut last_xy: Option<(f64, f64, f64)> = None;
    for i in start..end {
        let bar = &bars[i];
        let cx = viewport.bar_to_x(i) + rect.x;
        let y = strip_y + strip_h * (1.0 - (bar.cumulative_delta - cvd_min) / span);
        if let Some((px, py, prev_v)) = last_xy {
            let rising = bar.cumulative_delta >= prev_v;
            let c = if rising { EXTRA_COLORS.cvd_up } else { EXTRA_COLORS.cvd_down };
            ctx.set_stroke_color(c);
            ctx.begin_path();
            ctx.move_to(px, py);
            ctx.line_to(cx, y);
            ctx.stroke();
        }
        last_xy = Some((cx, y, bar.cumulative_delta));
    }
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
    // Aim for ~25-40 visible rows per bar (industry-standard density for
    // legible cluster grids). The dig3 aggregator groups by raw f64 price,
    // so each trade tick becomes its own level — thousands of rows per bar
    // is the default, which renders as a vertical noise smear at typical
    // chart heights. Quantize down to a "round" step picked from the
    // ladder {0.01, 0.05, 0.1, 0.5, 1, 5, 10, 50, 100, 500, 1000}.
    const ROW_TARGET: f64 = 32.0;
    const STEP_LADDER: &[f64] = &[
        0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 50.0, 100.0, 500.0, 1000.0,
    ];
    let mut best_span = 0.0_f64;
    for bar in bars.iter().rev().take(8) {
        if bar.high > bar.low && bar.high.is_finite() && bar.low.is_finite() {
            let span = bar.high - bar.low;
            if span > best_span {
                best_span = span;
            }
        }
    }
    if best_span <= 0.0 {
        return 1.0;
    }
    let raw = best_span / ROW_TARGET;
    // Snap up to the next ladder step.
    for &s in STEP_LADDER {
        if s >= raw {
            return s;
        }
    }
    *STEP_LADDER.last().unwrap()
}

/// Bin a bar's per-trade levels into a coarser `tick_size` ladder so the
/// cluster grid has ~25-40 legible rows instead of thousands of single-
/// trade entries from the dig3 aggregator. NOT YET WIRED — pending the
/// pipeline diag pass (we need to see how many levels actually arrive
/// before deciding whether to bin in mlc or fix dig3 aggregator).
#[allow(dead_code)]
fn quantize_levels(bar: &FootprintBar, tick_size: f64) -> Vec<mlc_core::types::FootprintLevel> {
    if tick_size <= 0.0 || bar.levels.is_empty() {
        return bar.levels.clone();
    }
    use std::collections::BTreeMap;
    let mut buckets: BTreeMap<i64, (f64, f64)> = BTreeMap::new();
    for lv in &bar.levels {
        let key = (lv.price / tick_size).round() as i64;
        let entry = buckets.entry(key).or_insert((0.0, 0.0));
        entry.0 += lv.buy_vol;
        entry.1 += lv.sell_vol;
    }
    buckets.into_iter().map(|(key, (buy, sell))| {
        mlc_core::types::FootprintLevel {
            price: key as f64 * tick_size,
            buy_vol: buy,
            sell_vol: sell,
            is_single_print: false,
            is_unfinished: false,
        }
    }).collect()
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
            .map(|(p, b, s)| FootprintLevel { price: p, buy_vol: b, sell_vol: s, is_single_print: false, is_unfinished: false })
            .collect();
        let total: f64 = lvs.iter().map(|l| l.total_vol()).sum();
        let (open, close) = (lvs.first().map(|l| l.price).unwrap_or(0.0), lvs.last().map(|l| l.price).unwrap_or(0.0));
        let high = lvs.iter().map(|l| l.price).fold(f64::NEG_INFINITY, f64::max);
        let low  = lvs.iter().map(|l| l.price).fold(f64::INFINITY, f64::min);
        FootprintBar { open_time: 0, open, high, low, close, volume: total, levels: lvs, cumulative_delta: 0.0, stacked_ask_start: None, stacked_ask_len: None, stacked_bid_start: None, stacked_bid_len: None, is_absorption: false }
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
