//! Liquidation Heatmap renderer (C13) — Hyblock/CoinGlass-style time-by-price liquidation heatmap.
//!
//! Builtin overlay (chart-type → overlay migration) — renders as a
//! background layer UNDER the main series on ANY chart_type, toggled via
//! the indicators picker (`overlay_liquidation_heatmap` in the mli catalog)
//! → `ChartOutEvent::ToggleLiquidationHeatmapOverlay`. Was previously its
//! own full-pane `chart_type` ("liquidation_heatmap").
//!
//! Reads `state.liq_heatmap_ref` and `state.liq_dots_ref` that were populated
//! by the `LiquidationEvent` / `LiquidationBucketUpdate` live-update arms,
//! already gated on the window's `liq_heatmap_show` flag by
//! `build_chart_render_state`.
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

/// Draw the Liquidation Heatmap background overlay, conditional on
/// `state.liq_heatmap_ref` being populated (overlay enabled AND at least
/// one event/bucket has landed).
///
/// Called unconditionally from `render_chart::render_chart`'s overlay pass
/// (before `draw_series`, so the heat layer paints under the main series on
/// ANY chart type) — no-ops silently when the ref is absent (overlay off or
/// no data yet).
pub fn draw_liquidation_heatmap_conditional(ctx: &mut dyn RenderContext, state: &ChartRenderState) {
    if let Some(buf) = state.liq_heatmap_ref {
        draw_liquidation_heatmap(ctx, state, buf, state.liq_dots_ref);
    }
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
    // Chart-type → overlay migration: this is now a background layer under
    // whatever real chart_type is active (candles/bars/line/...), never the
    // sole content of the pane — an empty buffer (overlay just enabled,
    // first event/bucket in flight) must silently paint nothing instead of
    // the full-pane waiting spinner `draw_waiting_placeholder` used when
    // this was its own chart_type with nothing else on screen.
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
