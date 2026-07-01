//! Liquidation Heatmap renderer (C13) — Hyblock/CoinGlass-style time-by-price liquidation heatmap.
//!
//! Reads `state.liq_heatmap_ref` and `state.liq_dots_ref` that were populated
//! by the `LiquidationEvent` / `LiquidationBucketUpdate` live-update arms.
//!
//! ## Axis strategy: virtual-index axis
//!
//! Mirrors C12: each 250 ms time bucket is one virtual "bar" so the chart
//! can reuse the existing index-axis viewport geometry without touching the
//! time_axis path reverted in C4.
//!
//! ## Color scheme
//! - **Long-liquidated cells** (side == 1, Sell-side forced close): red (#e74c3c).
//! - **Short-liquidated cells** (side == 0, Buy-side forced close): green (#2ecc71).
//! - **Dot overlay**: same color coding, radius ∝ sqrt(value_usd).
//!
//! ## Out of scope
//! CoinGlass "Liquidation Levels" (predicted future liquidations by leverage-distribution
//! model) is not rendered — dig3 does not expose that model layer.

use mlc_core::chart::liquidation_heatmap::{LiquidationHeatmapBuffer, LiquidationDot};
use mlc_core::render::RenderContext;
use super::ChartRenderState;

/// Long-liquidated cell colour (red, #e74c3c) at the given alpha (0–1).
fn long_liq_color(alpha: f64) -> String {
    let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", 0xe7, 0x4c, 0x3c, a)
}

/// Short-liquidated cell colour (green, #2ecc71) at the given alpha (0–1).
fn short_liq_color(alpha: f64) -> String {
    let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", 0x2e, 0xcc, 0x71, a)
}

/// Draw the liquidation heatmap chart-type.
///
/// `buf` is the rolling liquidation cell buffer; `dots` is the optional per-event overlay.
pub fn draw_liquidation_heatmap(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    buf: &LiquidationHeatmapBuffer,
    dots: Option<&[LiquidationDot]>,
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
    let oldest_ms = buf.oldest_bucket_ms();
    let bucket_ms = buf.time_bucket_ms.max(1);

    // Pixel width of one bucket column — same as C12.
    let col_w = vp.bar_spacing.max(1.0);

    // Normalisation guard.
    let max_vol = buf.max_volume_seen.max(1e-9);

    // ── Render cells ──────────────────────────────────────────────────────────
    for cell in &buf.cells {
        // price_tick == 0 encodes "price unknown" — renders at price 0 which is
        // always outside any visible range.  Guard here avoids the useless work
        // and the misleading price=0 row if the range ever includes 0.
        if cell.price_tick == 0 {
            continue;
        }

        let bucket_idx = ((cell.ts_ms - oldest_ms) / bucket_ms) as f64;
        let cx = vp.bar_to_x_f64(bucket_idx) + rect.x - col_w * 0.5;
        if cx + col_w < rect.x || cx > rect.x + rect.width {
            continue;
        }

        let price = cell.price_tick as f64 * buf.tick_size;
        if price < ps.price_min || price > ps.price_max {
            continue;
        }
        let norm_y = (ps.price_max - price) / price_range;
        let cy = rect.y + norm_y * rect.height - tick_px * 0.5;

        let long_alpha = (cell.long_value / max_vol).sqrt().clamp(0.0, 1.0);
        let short_alpha = (cell.short_value / max_vol).sqrt().clamp(0.0, 1.0);

        if long_alpha > 0.03 {
            ctx.set_fill_color(&long_liq_color(long_alpha));
            ctx.fill_rect(cx, cy, col_w, tick_px);
        }
        if short_alpha > 0.03 {
            ctx.set_fill_color(&short_liq_color(short_alpha));
            ctx.fill_rect(cx, cy, col_w, tick_px);
        }
    }

    // ── Liquidation dot overlay ───────────────────────────────────────────────
    if let Some(dot_slice) = dots {
        if !dot_slice.is_empty() {
            let max_val: f64 = dot_slice.iter().map(|d| d.value_usd).fold(1e-9_f64, f64::max);
            for dot in dot_slice {
                let bucket_idx = ((dot.ts_ms - oldest_ms) / bucket_ms) as f64;
                let dx = vp.bar_to_x_f64(bucket_idx) + rect.x;
                if dx < rect.x || dx > rect.x + rect.width {
                    continue;
                }
                if dot.price < ps.price_min || dot.price > ps.price_max {
                    continue;
                }
                let norm_y = (ps.price_max - dot.price) / price_range;
                let dy = rect.y + norm_y * rect.height;

                let r = ((dot.value_usd / max_val).sqrt() * 6.0).clamp(1.5, 8.0);
                // side == 1 → long liquidated → red; side == 0 → short liquidated → green.
                let color = if dot.side == 1 { "#e74c3ccc" } else { "#2ecc71cc" };
                ctx.set_fill_color(color);
                ctx.fill_rect(dx - r, dy - r, r * 2.0, r * 2.0);
            }
        }
    }

    // ── Axis label ────────────────────────────────────────────────────────────
    ctx.set_fill_color("rgba(180, 180, 180, 0.6)");
    ctx.set_font("9px sans-serif");
    ctx.set_text_align(uzor::render::TextAlign::Left);
    ctx.set_text_baseline(uzor::render::TextBaseline::Top);
    ctx.fill_text("Liquidation Heatmap", rect.x + 4.0, rect.y + 4.0);
}
