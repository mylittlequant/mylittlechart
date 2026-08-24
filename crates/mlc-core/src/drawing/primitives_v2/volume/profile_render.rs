//! Shared volume-profile drawing for the two profile primitives.
//!
//! Both `FixedVolumeProfile` and `AnchoredVolumeProfile` used to draw a
//! SYNTHETIC histogram — a symmetric diamond from `|i - rows/2|`, a POC line at
//! half the chart height and a value area at a fixed 35 % / 65 %. None of it
//! read a single bar. This module puts them on `VolumeProfileData::compute`,
//! the same engine the chart's volume-profile OVERLAY uses, and paints them in
//! the overlay's colours so the two read as the same instrument at two scopes:
//! the overlay follows the screen, a primitive is pinned to a range the user
//! chose.

use crate::chart::volume_profile::{VolumeProfileData, VpMode};
use crate::Bar;
use super::super::RenderContext;

/// Overlay palette, kept byte-identical so a primitive profile and the overlay
/// profile cannot drift apart. `mlc-core` has no theme to ask.
const BUY: (u8, u8, u8) = (38, 166, 154);
const SELL: (u8, u8, u8) = (239, 83, 80);
const POC: (u8, u8, u8) = (255, 235, 59);
const VALUE_AREA: (u8, u8, u8) = (41, 98, 255);

fn rgba(c: (u8, u8, u8), alpha: f64) -> String {
    format!("rgba({}, {}, {}, {:.3})", c.0, c.1, c.2, alpha.clamp(0.0, 1.0))
}

/// Where a profile's histogram lives on screen.
pub(super) struct ProfileGeometry {
    /// The edge the bars grow FROM.
    pub origin_x: f64,
    /// How far they may grow. Negative grows leftward.
    pub max_width: f64,
    /// The x range the POC / value-area lines span.
    pub span: (f64, f64),
}

/// The inclusive bar-INDEX range covering every bar inside a closed
/// `[from_ms, to_ms]` window — see [`super::super::bar_window::window_indices`],
/// the shared implementation this re-exports for every call site in this
/// module (promoted 2026-08-25, markup-engines wave, out of this file —
/// see that module's own doc for why THIS was the copy kept canonical).
pub(super) use super::super::bar_window::window_indices;

/// The bars inside a closed `[from_ms, to_ms]` window, in the order given.
/// See [`window_indices`] for the range this slices.
pub(super) fn bars_in_window(bars: &[Bar], from_ms: i64, to_ms: i64) -> &[Bar] {
    match window_indices(bars, from_ms, to_ms) {
        Some((start, end)) => &bars[start..=end],
        None => &[],
    }
}

/// The low/high span the bars of `window` cover, for hit-testing the band a
/// profile actually occupies.
pub(super) fn price_span(window: &[Bar]) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for b in window {
        if b.low < lo { lo = b.low; }
        if b.high > hi { hi = b.high; }
    }
    (lo.is_finite() && hi.is_finite() && hi > lo).then_some((lo, hi))
}

/// Compute a profile over `window`, or `None` when there is nothing to show.
pub(super) fn profile_for(window: &[Bar], rows: u16) -> Option<VolumeProfileData> {
    if window.is_empty() || rows == 0 {
        return None;
    }
    let from = window.first()?.timestamp * 1000;
    let to = window.last()?.timestamp * 1000;
    let vp = VolumeProfileData::compute(
        window,
        VpMode::FixedRange { start_ts_ms: from, end_ts_ms: to },
        0.70,
        rows as usize,
    );
    (!vp.rows.is_empty() && vp.max_row_volume > 0.0).then_some(vp)
}

/// Draw one profile: value-area band, buy/sell rows, POC row, level lines.
///
/// The row geometry comes from PRICE, not from an even split of the chart
/// height — that was the other half of the old fake, and it is why the old
/// histogram never lined up with the candles beside it.
pub(super) fn draw_profile(
    ctx: &mut dyn RenderContext,
    vp: &VolumeProfileData,
    geom: &ProfileGeometry,
    show_poc: bool,
    show_value_area: bool,
) {
    let half_bucket = vp.bucket_size * 0.5;
    let (span_lo, span_hi) = (geom.span.0.min(geom.span.1), geom.span.0.max(geom.span.1));

    // ── Value-area band across the whole range ───────────────────────────────
    if show_value_area {
        let y_top = ctx.price_to_y(vp.vah_price);
        let y_bot = ctx.price_to_y(vp.val_price);
        let (y_top, y_bot) = (y_top.min(y_bot), y_top.max(y_bot));
        if y_bot > y_top {
            ctx.set_fill_color(&rgba(VALUE_AREA, 0.06));
            ctx.fill_rect(span_lo, y_top, span_hi - span_lo, y_bot - y_top);
        }
    }

    // ── Histogram rows ───────────────────────────────────────────────────────
    for row in &vp.rows {
        let is_poc = (row.price - vp.poc_price).abs() < vp.bucket_size * 0.01;
        let is_va = row.price >= vp.val_price - half_bucket
            && row.price <= vp.vah_price + half_bucket;

        let y_top = ctx.price_to_y(row.price + half_bucket);
        let y_bot = ctx.price_to_y(row.price - half_bucket);
        let (y_top, y_bot) = (y_top.min(y_bot), y_top.max(y_bot));
        let row_h = (y_bot - y_top).max(1.0);

        let total_w = geom.max_width * (row.total_volume / vp.max_row_volume);
        if total_w.abs() < 0.5 {
            continue;
        }
        // A leftward profile is drawn from its own right edge, so `fill_rect`
        // always gets a positive width.
        let bar_x = |w: f64| if geom.max_width < 0.0 { geom.origin_x + w } else { geom.origin_x };

        if is_va && show_value_area {
            ctx.set_fill_color(&rgba(VALUE_AREA, 0.10));
            ctx.fill_rect(bar_x(total_w), y_top, total_w.abs(), row_h);
        }

        if is_poc {
            ctx.set_fill_color(&rgba(POC, 0.55));
            ctx.fill_rect(bar_x(total_w), y_top, total_w.abs(), row_h);
            continue;
        }

        let buy_fraction = if row.total_volume > 0.0 {
            row.buy_volume / row.total_volume
        } else {
            0.5
        };
        let buy_w = total_w * buy_fraction;
        let sell_w = total_w - buy_w;
        if buy_w.abs() > 0.0 {
            ctx.set_fill_color(&rgba(BUY, 0.45));
            ctx.fill_rect(bar_x(buy_w), y_top, buy_w.abs(), row_h);
        }
        if sell_w.abs() > 0.0 {
            ctx.set_fill_color(&rgba(SELL, 0.45));
            ctx.fill_rect(bar_x(total_w) + if geom.max_width < 0.0 { 0.0 } else { buy_w.abs() },
                          y_top, sell_w.abs(), row_h);
        }
    }

    // ── Level lines across the range ─────────────────────────────────────────
    let mut level = |price: f64, color: (u8, u8, u8), alpha: f64, width: f64, dash: &[f64]| {
        let y = ctx.price_to_y(price);
        ctx.set_stroke_color(&rgba(color, alpha));
        ctx.set_stroke_width(width);
        ctx.set_line_dash(dash);
        ctx.begin_path();
        ctx.move_to(span_lo, y);
        ctx.line_to(span_hi, y);
        ctx.stroke();
    };
    if show_poc {
        level(vp.poc_price, POC, 0.80, 1.5, &[6.0, 4.0]);
    }
    if show_value_area {
        level(vp.vah_price, VALUE_AREA, 0.50, 1.0, &[4.0, 4.0]);
        level(vp.val_price, VALUE_AREA, 0.50, 1.0, &[4.0, 4.0]);
    }
    ctx.set_line_dash(&[]);
}
