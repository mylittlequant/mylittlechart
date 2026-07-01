//! DOM Heatmap renderer (C12) — Bookmap-style time-by-price depth heatmap.
//!
//! Reads `state.dom_heatmap_ref` and `state.dom_trade_dots_ref` that were
//! populated by the `DomHeatmapSnapshot` / `DomTradeDot` live-update arms.
//!
//! ## Axis strategy: virtual-index axis
//!
//! The chart reuses the existing index-axis viewport geometry: each 250 ms
//! time bucket is one virtual "bar", so `viewport.bar_to_x` works
//! without any time_axis plumbing.  The renderer maps bucket timestamps to
//! virtual bar indices and uses `viewport.bar_to_x` / price_scale Y
//! for pixel placement.  This avoids re-enabling the time_axis path that
//! was reverted in commit 951fa58 (C4) to prevent regressions on all other
//! chart types.  A future commit can promote to real time-axis once the
//! infrastructure is stable.
//!
//! ## Color scheme
//! - **Bid cells** (below mid): green, alpha = sqrt(bid_depth / max_depth).
//! - **Ask cells** (above mid): red, alpha = sqrt(ask_depth / max_depth).
//! - **Trade dots**: green (buy) or red (sell), radius ∝ sqrt(quantity).
//!
//! Colors match `LiquidityHeatmapState` for visual consistency.

use mlc_core::chart::dom_heatmap::{DomHeatmapBuffer, DomTradeDot};
use mlc_core::render::RenderContext;
use super::ChartRenderState;

/// BID cell colour (green, #2ecc71) at the given alpha (0–1).
fn bid_color(alpha: f64) -> String {
    let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", 0x2e, 0xcc, 0x71, a)
}

/// ASK cell colour (red, #e74c3c) at the given alpha (0–1).
fn ask_color(alpha: f64) -> String {
    let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", 0xe7, 0x4c, 0x3c, a)
}

/// Draw the DOM heatmap chart-type.
///
/// `buf` is the rolling depth buffer; `trade_dots` is the optional overlay.
pub fn draw_dom_heatmap(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    buf: &DomHeatmapBuffer,
    trade_dots: Option<&[DomTradeDot]>,
) {
    if buf.cells.is_empty() {
        return;
    }

    let rect = &state.chart_rect;
    let vp = state.viewport;
    let ps = state.price_scale;

    // ── Price axis ────────────────────────────────────────────────────────────
    let price_range = ps.price_max - ps.price_min;
    if price_range <= 0.0 || !price_range.is_finite() {
        return;
    }

    // One cell row height in pixels = chart_height * tick_size / price_range.
    let tick_px = (rect.height * buf.tick_size / price_range).max(1.0);

    // ── Time axis (virtual index) ─────────────────────────────────────────────
    // Map each bucket timestamp to a virtual bar index relative to the oldest
    // bucket in the buffer.  The viewport's index-axis geometry then converts
    // index → pixel-x.
    let oldest_ms = buf.oldest_bucket_ms();
    let bucket_ms = buf.time_bucket_ms.max(1);

    // Compute the pixel width of one bucket column.  We use the bar_spacing
    // directly from the viewport because each bucket is one virtual "bar".
    let col_w = vp.bar_spacing.max(1.0);

    // Normalisation: avoid dividing by zero when buffer is just starting.
    let max_depth = buf.max_depth_seen.max(1e-9);

    // ── Render cells ──────────────────────────────────────────────────────────
    for cell in &buf.cells {
        // Virtual bar index for this bucket.
        let bucket_idx = ((cell.ts_ms - oldest_ms) / bucket_ms) as f64;

        // Map to pixel-x using viewport (mirrors how bar candles are placed).
        // `bar_to_x_f64` centres on the bar; we shift left by half a column.
        let cx = vp.bar_to_x_f64(bucket_idx) + rect.x - col_w * 0.5;
        if cx + col_w < rect.x || cx > rect.x + rect.width {
            continue;
        }

        // Map price_tick to pixel-y.  High price → low y (screen top).
        let price = cell.price_tick as f64 * buf.tick_size;
        if price < ps.price_min || price > ps.price_max {
            continue;
        }
        let norm_y = (ps.price_max - price) / price_range;
        let cy = rect.y + norm_y * rect.height - tick_px * 0.5;

        let bid_alpha = (cell.bid_depth / max_depth).sqrt().clamp(0.0, 1.0);
        let ask_alpha = (cell.ask_depth / max_depth).sqrt().clamp(0.0, 1.0);

        if bid_alpha > 0.03 {
            ctx.set_fill_color(&bid_color(bid_alpha));
            ctx.fill_rect(cx, cy, col_w, tick_px);
        }
        if ask_alpha > 0.03 {
            ctx.set_fill_color(&ask_color(ask_alpha));
            ctx.fill_rect(cx, cy, col_w, tick_px);
        }
    }

    // ── Trade dots overlay ────────────────────────────────────────────────────
    if let Some(dots) = trade_dots {
        let max_qty: f64 = dots.iter().map(|d| d.quantity).fold(1e-9_f64, f64::max);
        for dot in dots {
            let bucket_idx = ((dot.ts_ms - oldest_ms) / bucket_ms) as f64;
            let dx = vp.bar_to_x_f64(bucket_idx) + rect.x;
            if dx < rect.x || dx > rect.x + rect.width {
                continue;
            }
            let price = dot.price;
            if price < ps.price_min || price > ps.price_max {
                continue;
            }
            let norm_y = (ps.price_max - price) / price_range;
            let dy = rect.y + norm_y * rect.height;

            let r = ((dot.quantity / max_qty).sqrt() * 6.0).clamp(1.5, 8.0);
            let color = if dot.side == 1 { "#2ecc71cc" } else { "#e74c3ccc" };
            ctx.set_fill_color(color);
            // Use a small filled rect as a circle substitute (no fill_circle API required).
            ctx.fill_rect(dx - r, dy - r, r * 2.0, r * 2.0);
        }
    }

    // ── Axis label: "DOM Heatmap" at top-left of chart ───────────────────────
    ctx.set_fill_color("rgba(180, 180, 180, 0.6)");
    ctx.set_font("9px sans-serif");
    ctx.set_text_align(uzor::render::TextAlign::Left);
    ctx.set_text_baseline(uzor::render::TextBaseline::Top);
    ctx.fill_text("DOM Heatmap", rect.x + 4.0, rect.y + 4.0);
}
