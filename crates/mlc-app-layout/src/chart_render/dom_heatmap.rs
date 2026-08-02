//! DOM Heatmap renderer — resting order-book depth as a chart OVERLAY
//! (Bookmap / TapeSurf "Order Book heatmap", refs
//! `tapesurf-refs/01,02,10,11,12`).
//!
//! Real `IndicatorInstance` (`overlay_dom_heatmap`, `StreamKind::OrderBook`),
//! painted UNDER the series so the candles stay readable through the heat —
//! which is how the reference stacks them.
//!
//! ## Geometry (rewritten 2026-08-02)
//!
//! One cell = one BAR column × one price row, matching
//! [`mlc_core::chart::dom_heatmap`]'s grid. The previous revision walked
//! 250 ms buckets through `viewport.bar_to_x_f64`, painting bucket #N at the
//! x of bar #N — right while this was its own chart type (the viewport's
//! bars WERE the buckets), wrong as an overlay by a factor of
//! `timeframe_ms / 250`. The reference re-grains its heat with the chart's
//! timeframe buttons: the heat columns ARE the candle columns.
//!
//! ## Normalisation
//!
//! Over the cells actually on screen, not over a running all-time maximum —
//! a wall from an hour ago must not flatten the visible picture, and zooming
//! should re-expose structure. `sensitivity` is a gain on that divisor,
//! mirroring the reference's slider.
//!
//! ## Colours
//!
//! Asks red, bids teal — as in the reference, where the book above price
//! reads warm and below reads cool.

use mlc_core::chart::dom_heatmap::{DomHeatmapBuffer, DomTradeDot};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::Bar;
use super::ChartRenderState;

/// Minimum on-screen height of one price row.
const MIN_ROW_PX: f64 = 2.0;
/// Cell size below which the per-cell numbers stop being legible and are
/// dropped, leaving the texture — the reference behaves the same way.
const MIN_TEXT_ROW_PX: f64 = 7.0;
const MIN_TEXT_COL_PX: f64 = 16.0;
// The aspect past which a cell stops printing its number is a parameter,
// not a constant — see `DomHeatmapInk::value_aspect`.

/// Compact book size, matching the reference's ladder grammar: whole units
/// once past a hundred, one decimal in the teens, two below that.
fn format_depth(v: f64) -> String {
    if v >= 1000.0 {
        format!("{:.1}k", v / 1000.0)
    } else if v >= 100.0 {
        format!("{v:.0}")
    } else if v >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}

/// Cell colour: the skin's base→hot pair for that side, leaned by `heat`
/// (the level's share of the visible maximum), then taken to `alpha`.
///
/// The pair is the SAME one the DOM ladder's intensity bars use, so a level
/// that glows in the panel glows the same way on the chart — one skin, not
/// a bespoke palette per surface.
fn heat_color(base: &str, hot: &str, heat: f64, alpha: f64) -> String {
    let mixed = mlc_core::utils::color::mix_colors(base, hot, heat.clamp(0.0, 1.0));
    let (r, g, b, _) = mlc_core::utils::color::parse_css_color(&mixed);
    let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
    mlc_core::utils::color::rgba_to_hex(r, g, b, a)
}

/// Index of the last bar at or before `ts_ms`, or `None` when `ts_ms`
/// predates the series. Bars ascend in time, so this is a partition point.
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

/// Draw the DOM Heatmap, conditional on the overlay being on and at least
/// one book snapshot having landed.
pub fn draw_dom_heatmap_conditional(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    if let Some(buf) = state.dom_heatmap_ref {
        draw_dom_heatmap(ctx, state, buf, state.dom_trade_dots_ref);
    }
}

/// Draw the depth grid plus the executed-trade dot overlay.
pub fn draw_dom_heatmap(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    buf: &DomHeatmapBuffer,
    trade_dots: Option<&[DomTradeDot]>,
) {
    // Overlay just enabled and the first snapshot still in flight — paint
    // nothing rather than the full-pane placeholder this used when it owned
    // the pane.
    if buf.cells.is_empty() || state.bars.is_empty() {
        return;
    }

    let rect = &state.chart_rect;
    let vp = state.viewport;
    let ps = state.price_scale;
    let bars = state.bars;

    let price_range = ps.price_max - ps.price_min;
    if price_range <= 0.0 || !price_range.is_finite() {
        return;
    }

    let row_px = (rect.height * buf.row_size() / price_range).max(MIN_ROW_PX);
    let col_w = vp.bar_spacing.max(1.0);
    let right_edge = rect.x + rect.width;

    // Resolve every cell to pixels once, taking the on-screen maximum in the
    // same pass. The bar still forming is drawn from `buf.live` — the book as
    // it stands — instead of from its cell, which holds the bar's running
    // peak and would freeze the newest column at whatever passed through it
    // first. Its cell takes over the moment the bar closes.
    let mut placed: Vec<(f64, f64, f64, f64, bool)> = Vec::with_capacity(buf.cells.len());
    let frozen = buf.cells.iter().filter(|c| c.ts_ms != buf.live_bar_ms);
    for cell in frozen.chain(buf.live.iter()) {
        let Some(bar_idx) = bar_index_at(bars, cell.ts_ms) else { continue };
        let cx = vp.bar_to_x_f64(bar_idx as f64) + rect.x - col_w * 0.5;
        if cx + col_w < rect.x || cx > right_edge {
            continue;
        }
        let price = cell.price(buf.tick_size, buf.row_ticks);
        if price < ps.price_min || price > ps.price_max {
            continue;
        }
        let norm_y = (ps.price_max - price) / price_range;
        let cy = rect.y + norm_y * rect.height - row_px * 0.5;
        // Digits are suppressed wherever the candle covers the cell: half a
        // number sticking out from under a body reads as noise. The heat
        // itself still paints — under the candle there is just no text.
        //
        // The test is the row's RECT against the candle's range, not the
        // row's centre price: a row is `row_size` tall (several price units),
        // so a centre-point test left the rows immediately above and below a
        // candle printing their digits hard against its edge, which is
        // exactly what looked wrong.
        let half_row = buf.row_size() * 0.5;
        let covered = bars
            .get(bar_idx)
            .is_some_and(|b| price + half_row >= b.low && price - half_row <= b.high);
        placed.push((cx, cy, cell.bid_depth, cell.ask_depth, covered));
    }
    if placed.is_empty() {
        return;
    }

    let ink = state.dom_params;
    // Normalise against a PERCENTILE of what is on screen, not the maximum.
    // The maximum is by definition the biggest wall in view, so scaling by it
    // pushed every ordinary level under the alpha cut — the field looked
    // empty until price came close enough for a level to grow, which is what
    // read as the layer loading in late rather than being there already.
    let mut depths: Vec<f64> = placed.iter().map(|(_, _, b, a, _)| b.max(*a)).collect();
    depths.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pct = ink.norm_pct.clamp(10.0, 100.0) / 100.0;
    let pick = ((depths.len() as f64 - 1.0) * pct).round().max(0.0) as usize;
    let max_depth = depths.get(pick).copied().unwrap_or(1e-9).max(1e-9);
    let norm = max_depth / ink.sensitivity.max(0.1);
    let opacity = ink.opacity.clamp(0.05, 1.0);
    // `contrast` is the exponent of depth→alpha. The reference reads as a
    // dark field with a few bright walls, which is what an exponent above 1
    // does; below 1 lifts the weak cells and the whole layer floods.
    let contrast = ink.contrast.clamp(0.2, 6.0);
    // A wall is measured against the visible MEAN, not the maximum: the
    // maximum IS the wall, so normalising by it would mark every frame's
    // biggest cell no matter how ordinary.
    let mean_depth = if depths.is_empty() {
        0.0
    } else {
        depths.iter().sum::<f64>() / depths.len() as f64
    };
    let wall_at = mean_depth * ink.wall_x.max(1.0);

    // Per-cell size numbers, as in the reference. They are legible only when
    // a cell is big enough to hold them, so the same rule the reference
    // follows applies: at coarse zoom the digits drop out and the texture
    // stays.
    let show_text = ink.show_values
        && row_px >= MIN_TEXT_ROW_PX
        && col_w >= MIN_TEXT_COL_PX
        && row_px / col_w <= ink.value_aspect.max(0.2);
    // Bounded by BOTH cell dimensions: a wide short cell and a tall narrow
    // one both have to hold four monospace characters.
    let font_px = ((row_px * 0.46).min(col_w * 0.22) * ink.value_size.clamp(0.3, 3.0)).clamp(6.0, 14.0);

    let theme = state.theme;
    for (cx, cy, bid_depth, ask_depth, covered) in placed {
        let depth = bid_depth.max(ask_depth);
        let is_wall = wall_at > 0.0 && depth >= wall_at;
        let bid_heat = (bid_depth / norm).clamp(0.0, 1.0);
        let ask_heat = (ask_depth / norm).clamp(0.0, 1.0);
        let bid_alpha = if is_wall && bid_depth >= ask_depth { 1.0 } else { bid_heat.powf(contrast) * opacity };
        let ask_alpha = if is_wall && ask_depth > bid_depth { 1.0 } else { ask_heat.powf(contrast) * opacity };
        if bid_alpha > 0.02 {
            ctx.set_fill_color(&heat_color(&theme.heat_bid, &theme.heat_bid_hot, bid_heat, bid_alpha));
            ctx.fill_rect(cx, cy, col_w, row_px);
        }
        if ask_alpha > 0.02 {
            ctx.set_fill_color(&heat_color(&theme.heat_ask, &theme.heat_ask_hot, ask_heat, ask_alpha));
            ctx.fill_rect(cx, cy, col_w, row_px);
        }
        if show_text && !covered && depth > 0.0 && (is_wall || bid_alpha.max(ask_alpha) > 0.02) {
            // Dark digits on a solid wall, muted grey on the field — the
            // reference's contrast trick, and the reason a wall reads at a
            // glance.
            ctx.set_fill_color(&if is_wall {
                mlc_core::utils::color::apply_opacity(&theme.background, 0.92)
            } else {
                mlc_core::utils::color::apply_opacity(&theme.text, 0.62)
            });
            ctx.set_font(&format!("{font_px:.0}px monospace"));
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(&format_depth(depth), cx + col_w * 0.5, cy + row_px * 0.5);
        }
    }

    // ── Executed-trade dots ───────────────────────────────────────────────────
    // Real (ts, price) per trade, so they place on the bar axis directly.
    if let Some(dots) = trade_dots {
        if !dots.is_empty() {
            let max_qty: f64 = dots.iter().map(|d| d.quantity).fold(1e-9_f64, f64::max);
            for dot in dots {
                let Some(bar_idx) = bar_index_at(bars, dot.ts_ms) else { continue };
                let dx = vp.bar_to_x_f64(bar_idx as f64) + rect.x;
                if dx < rect.x || dx > right_edge {
                    continue;
                }
                if dot.price < ps.price_min || dot.price > ps.price_max {
                    continue;
                }
                let norm_y = (ps.price_max - dot.price) / price_range;
                let dy = rect.y + norm_y * rect.height;
                let r = ((dot.quantity / max_qty).sqrt() * 5.0).clamp(1.5, 7.0);
                let color = if dot.side == 1 { "#14b8a6cc" } else { "#dc352bcc" };
                ctx.set_fill_color(color);
                ctx.fill_rect(dx - r, dy - r, r * 2.0, r * 2.0);
            }
        }
    }
}
