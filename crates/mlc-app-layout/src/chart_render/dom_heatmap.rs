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
use mlc_core::render::RenderContext;
use mlc_core::Bar;
use super::ChartRenderState;

/// Minimum on-screen height of one price row.
const MIN_ROW_PX: f64 = 2.0;

/// Bid (below-price) cell colour — teal, `alpha` in 0–1.
fn bid_color(alpha: f64) -> String {
    let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", 0x14, 0xb8, 0xa6, a)
}

/// Ask (above-price) cell colour — red, `alpha` in 0–1.
fn ask_color(alpha: f64) -> String {
    let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", 0xdc, 0x35, 0x2b, a)
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
    // same pass.
    let mut placed: Vec<(f64, f64, f64, f64)> = Vec::with_capacity(buf.cells.len());
    let mut max_depth = 1e-9_f64;
    for cell in &buf.cells {
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
        max_depth = max_depth.max(cell.bid_depth).max(cell.ask_depth);
        placed.push((cx, cy, cell.bid_depth, cell.ask_depth));
    }
    if placed.is_empty() {
        return;
    }

    let norm = max_depth / state.dom_sensitivity.max(0.1);
    let opacity = state.dom_opacity.clamp(0.05, 1.0);

    for (cx, cy, bid_depth, ask_depth) in placed {
        let bid_alpha = (bid_depth / norm).sqrt().clamp(0.0, 1.0) * opacity;
        let ask_alpha = (ask_depth / norm).sqrt().clamp(0.0, 1.0) * opacity;
        if bid_alpha > 0.03 {
            ctx.set_fill_color(&bid_color(bid_alpha));
            ctx.fill_rect(cx, cy, col_w, row_px);
        }
        if ask_alpha > 0.03 {
            ctx.set_fill_color(&ask_color(ask_alpha));
            ctx.fill_rect(cx, cy, col_w, row_px);
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
