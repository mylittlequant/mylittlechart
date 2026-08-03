//! TPO Profile chart overlay renderer (item D).
//!
//! Mirrors `volume_profile.rs`'s layout — a right-anchored histogram strip
//! overlaid on top of candles/other chart types, with POC line + Value Area
//! shading — but renders the block-histogram time-at-price profile
//! ([`TpoProfileData`], counted touches per price row) in a visually
//! DISTINCT style from Volume Profile: single-tone amber blocks (no
//! buy/sell split — a bar touch has no directional meaning the way volume
//! does) and no letters (that's the separate letter-based TPO Market
//! Profile chart type, `chart_render::tpo::draw_tpo`).
//!
//! Key visual elements:
//! - Right-side histogram strip (18% of chart width, same ratio as VP).
//! - Each bucket row: solid amber block, width proportional to touch count.
//! - POC row highlighted in a stronger/brighter amber.
//! - Value-area rows with a tinted background band.
//! - POC horizontal line spanning the full chart width (dashed).
//! - VAH / VAL horizontal lines (thinner, more transparent).
//! - Right-edge numeric labels for POC, VAH, VAL.

use mlc_core::render::RenderContext;
use mlc_core::chart::tpo_profile::TpoProfileData;
use super::ChartRenderState;

/// Minimum chart width below which the strip is skipped.
const MIN_CHART_WIDTH_FOR_TPO_PROFILE: f64 = 120.0;
/// Minimum chart height below which the strip is skipped.
const MIN_CHART_HEIGHT_FOR_TPO_PROFILE: f64 = 80.0;

/// Draw the TPO Profile block-histogram overlay.
///
/// Called after the primary chart type series has been painted (same slot
/// Volume Profile uses — see `render_chart.rs`'s draw order; both overlays
/// CAN be shown together, they occupy the same right-edge strip and will
/// visually overlap if both toggles are on simultaneously — same tradeoff
/// the pre-existing OI/Funding pair accepts).
pub fn draw_tpo_profile_overlay(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    profile: &TpoProfileData,
) {
    if profile.rows.is_empty() || profile.max_row_count == 0 { return; }

    let rect = &state.chart_rect;
    let ps = state.price_scale;
    let price_range = ps.price_max - ps.price_min;
    if price_range <= 0.0 || !price_range.is_finite() { return; }
    if rect.width < MIN_CHART_WIDTH_FOR_TPO_PROFILE || rect.height < MIN_CHART_HEIGHT_FOR_TPO_PROFILE { return; }

    let ink = state.tpo_ink;
    let strip_w = rect.width * ink.width_ratio;
    let strip_x = if ink.left_side { rect.x } else { rect.x + rect.width - strip_w };
    let a = |alpha: f64| alpha * ink.opacity;

    // ── Value Area tinted background ─────────────────────────────────────
    let va_y_top = rect.y + viewport_price_to_y(profile.vah_price, ps.price_min, ps.price_max, rect.height);
    let va_y_bot = rect.y + viewport_price_to_y(profile.val_price, ps.price_min, ps.price_max, rect.height);
    let va_height = (va_y_bot - va_y_top).max(0.0);
    if ink.show_value_area && va_height > 0.0 {
        ctx.set_fill_color(&super::volume_profile::rgba(255, 152, 0, a(0.06)));
        ctx.fill_rect(rect.x, va_y_top, rect.width, va_height);
    }

    // ── Histogram rows (block profile — no letters, no buy/sell split) ────
    let half_bucket = profile.bucket_size * 0.5;
    let max_count = profile.max_row_count as f64;
    for row in &profile.rows {
        if row.count == 0 { continue; }
        let is_poc = (row.price - profile.poc_price).abs() < profile.bucket_size * 0.01;
        let is_va = row.price >= profile.val_price - half_bucket
            && row.price <= profile.vah_price + half_bucket;

        let y_top = rect.y + viewport_price_to_y(row.price + half_bucket, ps.price_min, ps.price_max, rect.height);
        let y_bot = rect.y + viewport_price_to_y(row.price - half_bucket, ps.price_min, ps.price_max, rect.height);
        let row_h = (y_bot - y_top).max(1.0);

        if y_bot < rect.y || y_top > rect.y + rect.height { continue; }

        let bar_w = strip_w * (row.count as f64 / max_count);

        if is_va && ink.show_value_area {
            ctx.set_fill_color(&super::volume_profile::rgba(255, 152, 0, a(0.10)));
            ctx.fill_rect(strip_x, y_top, bar_w, row_h);
        }

        if is_poc {
            // POC: brightest solid block.
            ctx.set_fill_color(&super::volume_profile::rgba(255, 171, 0, a(0.75)));
        } else {
            // Regular block — single amber tone (distinct from VP's green/red
            // buy/sell split; a bar "touch" has no directional meaning).
            ctx.set_fill_color(&super::volume_profile::rgba(255, 152, 0, a(0.45)));
        }
        ctx.fill_rect(strip_x, y_top, bar_w, row_h);
    }

    // ── Horizontal level lines spanning full chart width ──────────────────
    ctx.save();
    ctx.begin_path();
    ctx.rect(rect.x, rect.y, rect.width, rect.height);
    ctx.clip();

    let poc_y = rect.y + viewport_price_to_y(profile.poc_price, ps.price_min, ps.price_max, rect.height);
    if ink.show_poc && poc_y >= rect.y && poc_y <= rect.y + rect.height {
        ctx.set_stroke_color(&super::volume_profile::rgba(255, 171, 0, a(0.80)));
        ctx.set_stroke_width(1.5);
        ctx.set_line_dash(&[6.0, 4.0]);
        ctx.begin_path();
        ctx.move_to(rect.x, poc_y);
        ctx.line_to(rect.x + rect.width, poc_y);
        ctx.stroke();
    }

    let vah_y = rect.y + viewport_price_to_y(profile.vah_price, ps.price_min, ps.price_max, rect.height);
    if ink.show_value_area && vah_y >= rect.y && vah_y <= rect.y + rect.height {
        ctx.set_stroke_color(&super::volume_profile::rgba(255, 152, 0, a(0.55)));
        ctx.set_stroke_width(1.0);
        ctx.set_line_dash(&[4.0, 4.0]);
        ctx.begin_path();
        ctx.move_to(rect.x, vah_y);
        ctx.line_to(rect.x + rect.width, vah_y);
        ctx.stroke();
    }

    let val_y = rect.y + viewport_price_to_y(profile.val_price, ps.price_min, ps.price_max, rect.height);
    if ink.show_value_area && val_y >= rect.y && val_y <= rect.y + rect.height {
        ctx.set_stroke_color(&super::volume_profile::rgba(255, 152, 0, a(0.55)));
        ctx.set_stroke_width(1.0);
        ctx.set_line_dash(&[4.0, 4.0]);
        ctx.begin_path();
        ctx.move_to(rect.x, val_y);
        ctx.line_to(rect.x + rect.width, val_y);
        ctx.stroke();
    }

    ctx.set_line_dash(&[]);
    ctx.restore();

    // ── Right-side price labels ────────────────────────────────────────────
    if ink.show_labels {
        ctx.set_font("10px sans-serif");
        ctx.set_text_baseline(mlc_core::render::TextBaseline::Middle);
        let label_x = if ink.left_side {
            ctx.set_text_align(mlc_core::render::TextAlign::Left);
            rect.x + 2.0
        } else {
            ctx.set_text_align(mlc_core::render::TextAlign::Right);
            rect.x + rect.width - 2.0
        };

        if ink.show_poc && poc_y >= rect.y && poc_y <= rect.y + rect.height {
            ctx.set_fill_color(&super::volume_profile::rgba(255, 171, 0, 0.90));
            ctx.fill_text(&fmt_price(profile.poc_price), label_x, poc_y);
        }
        if ink.show_value_area {
            if vah_y >= rect.y && vah_y <= rect.y + rect.height {
                ctx.set_fill_color(&super::volume_profile::rgba(255, 152, 0, 0.90));
                ctx.fill_text(&fmt_price(profile.vah_price), label_x, vah_y - 7.0);
            }
            if val_y >= rect.y && val_y <= rect.y + rect.height {
                ctx.set_fill_color(&super::volume_profile::rgba(255, 152, 0, 0.90));
                ctx.fill_text(&fmt_price(profile.val_price), label_x, val_y + 7.0);
            }
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Map a price to a Y pixel within a rect of height `chart_h`. Result is
/// relative to the rect's own top edge — same convention `volume_profile.rs`
/// uses (kept as a local duplicate rather than a shared helper for the same
/// reason VP's copy is local: this module has no dependency on VP's).
#[inline]
fn viewport_price_to_y(price: f64, price_min: f64, price_max: f64, chart_h: f64) -> f64 {
    let range = price_max - price_min;
    if range <= 0.0 {
        return chart_h * 0.5;
    }
    chart_h * (1.0 - (price - price_min) / range)
}

/// Format a price for the label strip (mirrors `volume_profile.rs`'s
/// `fmt_price`).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_price_to_y_maps_price_max_to_top() {
        let y = viewport_price_to_y(100.0, 0.0, 100.0, 200.0);
        assert!(y.abs() < 1e-9, "price_max should map to y=0 (top), got {y}");
    }

    #[test]
    fn viewport_price_to_y_maps_price_min_to_bottom() {
        let y = viewport_price_to_y(0.0, 0.0, 100.0, 200.0);
        assert!((y - 200.0).abs() < 1e-9, "price_min should map to y=chart_h (bottom), got {y}");
    }

    #[test]
    fn viewport_price_to_y_degenerate_range_centers() {
        let y = viewport_price_to_y(50.0, 100.0, 100.0, 200.0);
        assert!((y - 100.0).abs() < 1e-9, "degenerate price range should center, got {y}");
    }

    #[test]
    fn fmt_price_zero_is_bare_zero() {
        assert_eq!(fmt_price(0.0), "0");
    }

    #[test]
    fn fmt_price_scales_precision_by_magnitude() {
        assert_eq!(fmt_price(0.005), "0.005000");
        assert_eq!(fmt_price(0.5), "0.5000");
        assert_eq!(fmt_price(100.0), "100.00");
        assert_eq!(fmt_price(50_000.0), "50000.0");
    }
}
