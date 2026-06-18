//! Volume Profile chart overlay renderer (C10).
//!
//! Draws a horizontal histogram on the RIGHT side of the chart area,
//! overlaid on top of candles/other chart types. The histogram shows
//! buy (up-bar) vs sell (down-bar) volume per price bucket.
//!
//! Key visual elements:
//! - Right-side histogram strip (18 % of chart width, configurable).
//! - Each bucket row: buy-volume portion in green + sell in red.
//! - POC row highlighted in a stronger color.
//! - Value-area rows with a tinted background band.
//! - POC horizontal line spanning the full chart width (dashed).
//! - VAH / VAL horizontal lines (thinner, more transparent).
//! - Right-edge numeric labels for POC, VAH, VAL.

use mlc_core::render::RenderContext;
use mlc_core::chart::volume_profile::VolumeProfileData;
use super::ChartRenderState;

/// Fraction of chart width used for the histogram strip.
const STRIP_WIDTH_RATIO: f64 = 0.18;
/// Minimum chart width below which the strip is skipped.
const MIN_CHART_WIDTH_FOR_VP: f64 = 120.0;
/// Minimum chart height below which the strip is skipped.
const MIN_CHART_HEIGHT_FOR_VP: f64 = 80.0;

/// Draw the Volume Profile histogram overlay.
///
/// Called after the primary chart type series has been painted.
pub fn draw_volume_profile_overlay(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    vp: &VolumeProfileData,
) {
    if vp.rows.is_empty() || vp.max_row_volume <= 0.0 { return; }

    let rect = &state.chart_rect;
    let ps = state.price_scale;
    let price_range = ps.price_max - ps.price_min;
    if price_range <= 0.0 || !price_range.is_finite() { return; }
    if rect.width < MIN_CHART_WIDTH_FOR_VP || rect.height < MIN_CHART_HEIGHT_FOR_VP { return; }

    let strip_w = rect.width * STRIP_WIDTH_RATIO;
    let strip_x = rect.x + rect.width - strip_w;

    // ── Value Area tinted background ─────────────────────────────────────────
    let va_y_top = rect.y + viewport_price_to_y(vp.vah_price, ps.price_min, ps.price_max, rect.height);
    let va_y_bot = rect.y + viewport_price_to_y(vp.val_price, ps.price_min, ps.price_max, rect.height);
    let va_height = (va_y_bot - va_y_top).max(0.0);
    if va_height > 0.0 {
        ctx.set_fill_color("rgba(41, 98, 255, 0.06)");
        ctx.fill_rect(rect.x, va_y_top, rect.width, va_height);
    }

    // ── Histogram rows ────────────────────────────────────────────────────────
    let half_bucket = vp.bucket_size * 0.5;
    for row in &vp.rows {
        let is_poc = (row.price - vp.poc_price).abs() < vp.bucket_size * 0.01;
        let is_va = row.price >= vp.val_price - half_bucket
            && row.price <= vp.vah_price + half_bucket;

        // Convert the bucket center ± half_bucket to Y pixels.
        let y_top = rect.y + viewport_price_to_y(
            row.price + half_bucket, ps.price_min, ps.price_max, rect.height,
        );
        let y_bot = rect.y + viewport_price_to_y(
            row.price - half_bucket, ps.price_min, ps.price_max, rect.height,
        );
        let row_h = (y_bot - y_top).max(1.0);

        // Skip rows that are off-screen.
        if y_bot < rect.y || y_top > rect.y + rect.height { continue; }

        // Total bar width proportional to volume.
        let total_bar_w = strip_w * (row.total_volume / vp.max_row_volume);

        if is_va {
            // Faint value-area bar background.
            ctx.set_fill_color("rgba(41, 98, 255, 0.10)");
            ctx.fill_rect(strip_x, y_top, total_bar_w, row_h);
        }

        // Split buy/sell within the total bar width.
        let buy_fraction = if row.total_volume > 0.0 {
            row.buy_volume / row.total_volume
        } else {
            0.5
        };
        let buy_w = total_bar_w * buy_fraction;
        let sell_w = total_bar_w - buy_w;

        if is_poc {
            // POC: stronger single-color fill (gold).
            ctx.set_fill_color("rgba(255, 235, 59, 0.55)");
            ctx.fill_rect(strip_x, y_top, total_bar_w, row_h);
        } else {
            // Buy (green, lower-alpha).
            if buy_w > 0.0 {
                ctx.set_fill_color("rgba(38, 166, 154, 0.45)");
                ctx.fill_rect(strip_x, y_top, buy_w, row_h);
            }
            // Sell (red, lower-alpha) stacked to the right of buy.
            if sell_w > 0.0 {
                ctx.set_fill_color("rgba(239, 83, 80, 0.45)");
                ctx.fill_rect(strip_x + buy_w, y_top, sell_w, row_h);
            }
        }
    }

    // ── Horizontal level lines spanning full chart width ──────────────────────
    ctx.save();
    ctx.begin_path();
    ctx.rect(rect.x, rect.y, rect.width, rect.height);
    ctx.clip();

    // POC line — dashed, opaque.
    let poc_y = rect.y + viewport_price_to_y(vp.poc_price, ps.price_min, ps.price_max, rect.height);
    if poc_y >= rect.y && poc_y <= rect.y + rect.height {
        ctx.set_stroke_color("rgba(255, 235, 59, 0.80)");
        ctx.set_stroke_width(1.5);
        ctx.set_line_dash(&[6.0, 4.0]);
        ctx.begin_path();
        ctx.move_to(rect.x, poc_y);
        ctx.line_to(rect.x + rect.width, poc_y);
        ctx.stroke();
    }

    // VAH line — thinner, more transparent.
    let vah_y = rect.y + viewport_price_to_y(vp.vah_price, ps.price_min, ps.price_max, rect.height);
    if vah_y >= rect.y && vah_y <= rect.y + rect.height {
        ctx.set_stroke_color("rgba(41, 98, 255, 0.55)");
        ctx.set_stroke_width(1.0);
        ctx.set_line_dash(&[4.0, 4.0]);
        ctx.begin_path();
        ctx.move_to(rect.x, vah_y);
        ctx.line_to(rect.x + rect.width, vah_y);
        ctx.stroke();
    }

    // VAL line.
    let val_y = rect.y + viewport_price_to_y(vp.val_price, ps.price_min, ps.price_max, rect.height);
    if val_y >= rect.y && val_y <= rect.y + rect.height {
        ctx.set_stroke_color("rgba(41, 98, 255, 0.55)");
        ctx.set_stroke_width(1.0);
        ctx.set_line_dash(&[4.0, 4.0]);
        ctx.begin_path();
        ctx.move_to(rect.x, val_y);
        ctx.line_to(rect.x + rect.width, val_y);
        ctx.stroke();
    }

    // Reset dash.
    ctx.set_line_dash(&[]);
    ctx.restore();

    // ── Right-side price labels ───────────────────────────────────────────────
    ctx.set_font("10px sans-serif");
    ctx.set_text_align(mlc_core::render::TextAlign::Right);
    ctx.set_text_baseline(mlc_core::render::TextBaseline::Middle);

    let label_x = rect.x + rect.width - 2.0;

    if poc_y >= rect.y && poc_y <= rect.y + rect.height {
        ctx.set_fill_color("rgba(255, 235, 59, 0.90)");
        ctx.fill_text(&fmt_price(vp.poc_price), label_x, poc_y);
    }
    if vah_y >= rect.y && vah_y <= rect.y + rect.height {
        ctx.set_fill_color("rgba(100, 149, 237, 0.90)");
        ctx.fill_text(&fmt_price(vp.vah_price), label_x, vah_y - 7.0);
    }
    if val_y >= rect.y && val_y <= rect.y + rect.height {
        ctx.set_fill_color("rgba(100, 149, 237, 0.90)");
        ctx.fill_text(&fmt_price(vp.val_price), label_x, val_y + 7.0);
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Map a price to a Y pixel within a rect of height `chart_h`.
/// Result is relative to the rect's own top edge (not screen-absolute).
#[inline]
fn viewport_price_to_y(price: f64, price_min: f64, price_max: f64, chart_h: f64) -> f64 {
    let range = price_max - price_min;
    if range <= 0.0 {
        return chart_h * 0.5;
    }
    chart_h * (1.0 - (price - price_min) / range)
}

/// Format a price for the label strip (mirrors mlc-layout's `format_price_smart`).
fn fmt_price(price: f64) -> String {
    if price == 0.0 {
        return "0".to_string();
    }
    let abs = price.abs();
    if abs < 0.01 {
        format!("{:.6}", price)
    } else if abs < 1.0 {
        format!("{:.4}", price)
    } else if abs < 10_000.0 {
        format!("{:.2}", price)
    } else {
        format!("{:.1}", price)
    }
}
