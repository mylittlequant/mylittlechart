//! Shared VWAP + running-standard-deviation maths and band rendering for
//! `AnchoredVwap` and `FixedVwap`.
//!
//! Both primitives draw the SAME instrument over two different spans: a
//! running volume-weighted average of typical price
//! `(high + low + close) / 3`, accumulated bar by bar from the span's own
//! start, with the running (volume-weighted) population variance of that
//! same price feeding the ±σ bands. This module is the ONE place that
//! maths happens — see [`running_vwap`]'s own doc — so an anchored VWAP and
//! a fixed VWAP over the identical bar range can never disagree about what
//! "VWAP" means. `AnchoredVwap`'s span is one real anchor plus an optional
//! cap that defaults to the last loaded bar; `FixedVwap`'s span is two
//! real, order-agnostic anchors (via [`super::profile_render::window_indices`]
//! — the same window-resolution primitive `FixedVolumeProfile` uses).
//! Everything past resolving which bar INDICES a primitive's own span
//! covers is identical between them, including the band drawing below.

use crate::Bar;
use super::super::RenderContext;

/// One point of a running VWAP curve.
///
/// `pub` rather than `pub(super)`: [`FixedVwap::cached_vwap`]
/// (`super::fixed_vwap::FixedVwap`) is a `pub` field on a `pub` struct
/// reachable from outside this crate, so the type it carries must be at
/// least as visible — matching how `VolumeProfileData` (the volume
/// profiles' own cache payload) is a fully public type too. The
/// COMPUTATION ([`running_vwap`]) stays `pub(super)`; only the value shape
/// needs the wider reach.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VwapPoint {
    /// Absolute index into the `bars` slice [`running_vwap`] was given —
    /// `render`/`hit_test` place this point on the x-axis via
    /// `bar_to_x(bar_idx as f64)`.
    pub bar_idx: usize,
    /// Volume-weighted average of typical price, accumulated from the
    /// window's own start index through `bar_idx` inclusive.
    pub vwap: f64,
    /// Population standard deviation of typical price around `vwap`,
    /// weighted by volume the same way the mean is — the unit `sigma_1`/
    /// `sigma_2` scale to draw the ±σ bands.
    pub stddev: f64,
}

/// The running VWAP + stddev curve over `bars[start_idx..=end_idx]`, seeded
/// fresh at `start_idx` — volume before it never contributes, so an
/// anchored VWAP and a fixed VWAP over the identical `[start_idx, end_idx]`
/// produce byte-identical curves.
///
/// A bar whose cumulative volume through itself is still `<= 0.0` is
/// skipped — there is no honest VWAP to report while dividing by zero
/// volume (matching the pre-extraction behaviour: `if cum_vol <= 0.0 {
/// continue; }`). `bars.is_empty()`, `start_idx > end_idx`, or `end_idx`
/// past the slice's own end returns an empty curve.
pub(super) fn running_vwap(bars: &[Bar], start_idx: usize, end_idx: usize) -> Vec<VwapPoint> {
    if bars.is_empty() || start_idx > end_idx || end_idx >= bars.len() {
        return Vec::new();
    }
    let mut cum_vol_price = 0.0_f64;
    let mut cum_vol = 0.0_f64;
    let mut cum_vol_price2 = 0.0_f64;
    let mut out = Vec::with_capacity(end_idx - start_idx + 1);
    for (offset, b) in bars[start_idx..=end_idx].iter().enumerate() {
        let typical = (b.high + b.low + b.close) / 3.0;
        let v = b.volume.max(0.0);
        cum_vol_price += v * typical;
        cum_vol += v;
        cum_vol_price2 += v * typical * typical;

        if cum_vol <= 0.0 { continue; }

        let vwap = cum_vol_price / cum_vol;
        // Running population variance: E[x²] - E[x]².
        let variance = (cum_vol_price2 / cum_vol) - (vwap * vwap);
        out.push(VwapPoint { bar_idx: start_idx + offset, vwap, stddev: variance.max(0.0).sqrt() });
    }
    out
}

// ─── Drawing helpers (shared band/line rendering) ────────────────────────────

pub(super) fn draw_polyline(ctx: &mut dyn RenderContext, pts: &[(f64, f64)], color: &str, width: f64) {
    if pts.len() < 2 { return; }
    ctx.set_stroke_color(color);
    ctx.set_stroke_width(width);
    ctx.set_line_dash(&[]);
    ctx.begin_path();
    ctx.move_to(pts[0].0, pts[0].1);
    for &(x, y) in &pts[1..] {
        ctx.line_to(x, y);
    }
    ctx.stroke();
}

pub(super) fn draw_band(
    ctx: &mut dyn RenderContext,
    upper: &[(f64, f64)],
    lower: &[(f64, f64)],
    fill_color: &str,
) {
    if upper.is_empty() || lower.is_empty() { return; }
    ctx.set_fill_color(fill_color);
    ctx.set_global_alpha(1.0);
    ctx.begin_path();
    // Forward along upper edge.
    ctx.move_to(upper[0].0, upper[0].1);
    for &(x, y) in &upper[1..] {
        ctx.line_to(x, y);
    }
    // Backward along lower edge (close the shape).
    for &(x, y) in lower.iter().rev() {
        ctx.line_to(x, y);
    }
    ctx.fill();
}

#[cfg(test)]
mod running_vwap_tests {
    use super::*;

    /// Hand-computed example — proves the maths rather than asserting it by
    /// inspection: three bars, typical price `(h+l+c)/3`, weighted by
    /// volume.
    ///
    /// Bar 0: h=10, l=8, c=9  -> typical = 9,  volume = 2
    /// Bar 1: h=14, l=10, c=12 -> typical = 12, volume = 4
    /// Bar 2: h=8,  l=4, c=6  -> typical = 6,  volume = 1
    ///
    /// Cumulative after bar 0: vwap = 9,                    var = 0
    /// Cumulative after bar 1: sum_vp = 18+48=66, vol=6, vwap = 11,
    ///   sum_vp2 = 2*81+4*144=738, E[x²]=123, var=123-121=2, stddev=√2
    /// Cumulative after bar 2: sum_vp = 66+6=72, vol=7, vwap = 72/7,
    ///   sum_vp2 = 738+36=774, E[x²]=774/7, var=774/7-(72/7)² = 4770/49
    #[test]
    fn matches_hand_computed_values() {
        let bars = vec![
            Bar::with_volume(0, 9.0, 10.0, 8.0, 9.0, 2.0),
            Bar::with_volume(1, 12.0, 14.0, 10.0, 12.0, 4.0),
            Bar::with_volume(2, 6.0, 8.0, 4.0, 6.0, 1.0),
        ];
        let curve = running_vwap(&bars, 0, 2);
        assert_eq!(curve.len(), 3);

        assert_eq!(curve[0].bar_idx, 0);
        assert!((curve[0].vwap - 9.0).abs() < 1e-9, "vwap = {}", curve[0].vwap);
        assert!(curve[0].stddev.abs() < 1e-9, "stddev = {}", curve[0].stddev);

        assert_eq!(curve[1].bar_idx, 1);
        assert!((curve[1].vwap - 11.0).abs() < 1e-9, "vwap = {}", curve[1].vwap);
        assert!((curve[1].stddev - 2.0_f64.sqrt()).abs() < 1e-9, "stddev = {}", curve[1].stddev);

        assert_eq!(curve[2].bar_idx, 2);
        let expected_vwap = 72.0 / 7.0;
        let expected_var = 774.0 / 7.0 - expected_vwap * expected_vwap;
        assert!((curve[2].vwap - expected_vwap).abs() < 1e-9, "vwap = {}", curve[2].vwap);
        assert!((curve[2].stddev - expected_var.sqrt()).abs() < 1e-9, "stddev = {}", curve[2].stddev);
    }

    /// A bar with zero (or negative, clamped to zero) volume before any
    /// other bar has traded leaves nothing to report yet — skipped rather
    /// than dividing by zero — but does not otherwise disturb the curve
    /// once real volume arrives.
    #[test]
    fn leading_zero_volume_bar_is_skipped_not_divided_by() {
        let bars = vec![
            Bar::with_volume(0, 100.0, 100.0, 100.0, 100.0, 0.0),
            Bar::with_volume(1, 100.0, 100.0, 100.0, 100.0, 5.0),
        ];
        let curve = running_vwap(&bars, 0, 1);
        assert_eq!(curve.len(), 1, "the zero-volume bar contributes no point");
        assert_eq!(curve[0].bar_idx, 1);
        assert!((curve[0].vwap - 100.0).abs() < 1e-9);
    }

    /// Every bar with zero volume: nothing to report, empty curve — matches
    /// the pre-extraction `if cum_v <= 0.0 { return Miss/return; }` guards
    /// both `render` and `hit_test` used to check for the whole window.
    #[test]
    fn all_zero_volume_window_produces_an_empty_curve() {
        let bars = vec![
            Bar::with_volume(0, 100.0, 100.0, 100.0, 100.0, 0.0),
            Bar::with_volume(1, 100.0, 100.0, 100.0, 100.0, 0.0),
        ];
        assert!(running_vwap(&bars, 0, 1).is_empty());
    }

    /// An out-of-range or inverted window is an empty curve, not a panic.
    #[test]
    fn out_of_range_window_is_empty_not_a_panic() {
        let bars = vec![Bar::with_volume(0, 100.0, 100.0, 100.0, 100.0, 1.0)];
        assert!(running_vwap(&bars, 1, 0).is_empty(), "start > end");
        assert!(running_vwap(&bars, 0, 5).is_empty(), "end past the slice");
        assert!(running_vwap(&[], 0, 0).is_empty(), "empty bars");
    }
}
