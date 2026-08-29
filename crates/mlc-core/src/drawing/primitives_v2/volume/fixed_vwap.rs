//! Fixed VWAP drawing primitive.
//!
//! `FixedVwap` is to [`super::anchored_vwap::AnchoredVwap`] what
//! [`super::fixed_volume_profile::FixedVolumeProfile`] is to
//! [`super::anchored_volume_profile::AnchoredVolumeProfile`]: the same
//! instrument, placed over a SPAN with two real, order-agnostic endpoints
//! (`ClickBehavior::TwoPoint`) instead of one anchor plus an optional cap
//! that defaults to the last loaded bar. The VWAP/standard-deviation maths
//! is UNCHANGED and shared byte-for-byte with the anchored sibling — see
//! [`super::vwap_calc::running_vwap`], the ONE place that computation
//! happens. This module only resolves WHICH bars are in scope
//! (`[ts1, ts2]`, via the same [`super::profile_render::window_indices`]
//! window resolution `FixedVolumeProfile` already uses) and draws the
//! result.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{ConfigKey, current_language};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, render_primitive_text, TextAlign,
    config::{ConfigProperty, PropertyValue, PropertyGroup},
};
use super::vwap_calc::{running_vwap, draw_band, draw_polyline, VwapPoint};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FixedVwap {
    pub data: PrimitiveData,
    #[serde(default)] pub ts1: i64,
    #[serde(default)] pub ts2: i64,
    /// Whether to draw ±1 σ deviation bands.
    #[serde(default = "default_true")]
    pub show_stddev_1: bool,
    /// Whether to draw ±2 σ deviation bands.
    #[serde(default = "default_true")]
    pub show_stddev_2: bool,
    /// Standard-deviation multiplier for the inner band.
    #[serde(default = "default_sigma_1")]
    pub sigma_1: f64,
    /// Standard-deviation multiplier for the outer band.
    #[serde(default = "default_sigma_2")]
    pub sigma_2: f64,
    /// Full-height vertical fences at both span ends. Default on — that is
    /// the historical look. Off = fences only across the VWAP/band price
    /// span (a bounded range, not an infinite pair of lines).
    #[serde(default = "default_true")]
    pub extend_lines: bool,
    /// The VWAP curve computed from the current bars — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract. Filled by
    /// that hook from the SAME [`super::vwap_calc::running_vwap`] call
    /// `render`/`hit_test` make directly; read by nothing yet inside this
    /// crate today (a `&self`-only settings-modal summary — e.g. "current
    /// VWAP" — is the anticipated first reader, matching the volume
    /// profiles' own `cached_profile` doc). `render`/`hit_test` never read
    /// this field, so a primitive the hook has never touched still draws
    /// exactly right.
    ///
    /// `#[serde(skip)]`: a stale curve surviving a reload/undo would
    /// misreport a span that no longer matches this primitive's own
    /// current `[ts1, ts2]` — a freshly deserialized primitive starts at
    /// `None` until the owning bubble's own bar-derived-cache pass runs.
    #[serde(skip)]
    pub cached_vwap: Option<Vec<VwapPoint>>,
}

fn default_true() -> bool { true }
fn default_sigma_1() -> f64 { 1.0 }
fn default_sigma_2() -> f64 { 2.0 }

impl FixedVwap {
    pub fn new(ts1: i64, ts2: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fixed_vwap".to_string(),
                display_name: "Fixed VWAP".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            ts1, ts2,
            show_stddev_1: true,
            show_stddev_2: true,
            sigma_1: 1.0,
            sigma_2: 2.0,
            extend_lines: true,
            cached_vwap: None,
        }
    }
}

impl Primitive for FixedVwap {
    fn type_id(&self) -> &'static str { "fixed_vwap" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Measurement }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, 0.0), (self.ts2, 0.0)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(t, _)) = pts.first() { self.ts1 = t; }
        if let Some(&(t, _)) = pts.get(1) { self.ts2 = t; }
    }
    fn translate(&mut self, td: i64, _pd: f64) { self.ts1 += td; self.ts2 += td; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, _price: f64) {
        match pt {
            ControlPointType::Point1 => self.ts1 = ts_ms,
            ControlPointType::Point2 => self.ts2 = ts_ms,
            ControlPointType::Move => { let td = ts_ms - self.ts1; self.translate(td, 0.0); }
            _ => {}
        }
    }

    /// The two boundaries, then the VWAP curve's own body — bounded to
    /// `[ts1, ts2]` (never past it, unlike the anchored sibling's open
    /// right end), matching `FixedVolumeProfile::hit_test`'s own bounded
    /// shape for the same reason: both ends here are real, dragged points.
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2));
        if bars.is_empty() { return HitTestResult::Miss; }

        let Some((start_idx, end_idx)) = super::profile_render::window_indices(bars, self.ts1, self.ts2) else {
            return HitTestResult::Miss;
        };
        let curve = running_vwap(bars, start_idx, end_idx);
        let (fy0, fy1) = fence_ys(self, &curve, vp, ps, vp.chart_height);
        let on_fence_y = sy >= fy0 - HIT_TOLERANCE && sy <= fy1 + HIT_TOLERANCE;
        if (sx - x1).abs() < HIT_TOLERANCE && on_fence_y { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).abs() < HIT_TOLERANCE && on_fence_y { return HitTestResult::ControlPoint(ControlPointType::Point2); }

        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        if sx < min_x || sx > max_x { return HitTestResult::Miss; }
        let hover = vp.x_to_bar_f64(sx).round();
        if !hover.is_finite() { return HitTestResult::Miss; }
        let hover_idx = (hover as i64).clamp(start_idx as i64, end_idx as i64) as usize;

        // Same shared computation `render` draws from — the cumulative sum
        // through `hover_idx` is the curve's own last point.
        let Some(last) = running_vwap(bars, start_idx, hover_idx).into_iter().last() else {
            return HitTestResult::Miss;
        };
        let (vwap, stddev) = (last.vwap, last.stddev);

        let y_of = |price: f64| vp.price_to_y(price, ps.price_min, ps.price_max);
        for price in [
            vwap,
            vwap + self.sigma_1 * stddev, vwap - self.sigma_1 * stddev,
            vwap + self.sigma_2 * stddev, vwap - self.sigma_2 * stddev,
        ] {
            if (sy - y_of(price)).abs() < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }
        HitTestResult::Miss
    }

    /// Both endpoints at the price scale's own vertical center — same
    /// convention as `AnchoredVwap::control_points`: a VWAP is a LINE, not
    /// a price band, so there is no top/bottom pair the way a volume
    /// profile's histogram has.
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2));
        let cy = vp.price_to_y((ps.price_min + ps.price_max) / 2.0, ps.price_min, ps.price_max);
        vec![ControlPoint::point1(x1, cy), ControlPoint::point2(x2, cy)]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        let chart_height = ctx.chart_height();

        let bars = ctx.bars();
        let Some((start_idx, end_idx)) = super::profile_render::window_indices(bars, self.ts1, self.ts2) else {
            return;
        };
        let curve = running_vwap(bars, start_idx, end_idx);
        if curve.is_empty() { return; }

        let (fy0, fy1) = {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for pt in &curve {
                let mut prices = vec![pt.vwap];
                if self.show_stddev_1 {
                    prices.push(pt.vwap + self.sigma_1 * pt.stddev);
                    prices.push(pt.vwap - self.sigma_1 * pt.stddev);
                }
                if self.show_stddev_2 {
                    prices.push(pt.vwap + self.sigma_2 * pt.stddev);
                    prices.push(pt.vwap - self.sigma_2 * pt.stddev);
                }
                for p in prices {
                    let y = ctx.price_to_y(p);
                    lo = lo.min(y);
                    hi = hi.max(y);
                }
            }
            if self.extend_lines { (0.0, chart_height) } else if lo.is_finite() { (lo, hi) } else { (0.0, chart_height) }
        };

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        ctx.set_line_dash(&[]);
        ctx.begin_path();
        ctx.move_to(crisp(min_x, dpr), fy0);
        ctx.line_to(crisp(min_x, dpr), fy1);
        ctx.stroke();
        ctx.begin_path();
        ctx.move_to(crisp(max_x, dpr), fy0);
        ctx.line_to(crisp(max_x, dpr), fy1);
        ctx.stroke();

        let n = curve.len();
        let mut vwap_points: Vec<(f64, f64)> = Vec::with_capacity(n);
        let mut sd1_upper: Vec<(f64, f64)> = Vec::with_capacity(n);
        let mut sd1_lower: Vec<(f64, f64)> = Vec::with_capacity(n);
        let mut sd2_upper: Vec<(f64, f64)> = Vec::with_capacity(n);
        let mut sd2_lower: Vec<(f64, f64)> = Vec::with_capacity(n);

        for pt in &curve {
            let x = ctx.bar_to_x(pt.bar_idx as f64);
            vwap_points.push((x, ctx.price_to_y(pt.vwap)));
            sd1_upper.push((x, ctx.price_to_y(pt.vwap + self.sigma_1 * pt.stddev)));
            sd1_lower.push((x, ctx.price_to_y(pt.vwap - self.sigma_1 * pt.stddev)));
            sd2_upper.push((x, ctx.price_to_y(pt.vwap + self.sigma_2 * pt.stddev)));
            sd2_lower.push((x, ctx.price_to_y(pt.vwap - self.sigma_2 * pt.stddev)));
        }

        if self.show_stddev_2 && sd2_upper.len() >= 2 {
            draw_band(ctx, &sd2_upper, &sd2_lower, "rgba(100, 149, 237, 0.10)");
        }
        if self.show_stddev_1 && sd1_upper.len() >= 2 {
            draw_band(ctx, &sd1_upper, &sd1_lower, "rgba(100, 149, 237, 0.18)");
        }
        if self.show_stddev_2 && sd2_upper.len() >= 2 {
            draw_polyline(ctx, &sd2_upper, "rgba(100, 149, 237, 0.40)", 1.0 * dpr);
            draw_polyline(ctx, &sd2_lower, "rgba(100, 149, 237, 0.40)", 1.0 * dpr);
        }
        if self.show_stddev_1 && sd1_upper.len() >= 2 {
            draw_polyline(ctx, &sd1_upper, "rgba(100, 149, 237, 0.60)", 1.0 * dpr);
            draw_polyline(ctx, &sd1_lower, "rgba(100, 149, 237, 0.60)", 1.0 * dpr);
        }
        if vwap_points.len() >= 2 {
            draw_polyline(ctx, &vwap_points, &self.data.color.stroke, self.data.width * dpr);
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => min_x,
                    TextAlign::Center => (min_x + max_x) / 2.0,
                    TextAlign::End => max_x,
                };
                let text_y = match text.v_align {
                    TextAlign::Start => -text_offset,
                    TextAlign::Center => chart_height / 2.0,
                    TextAlign::End => chart_height + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
    fn unbounded_in_price(&self) -> bool { self.extend_lines }

    /// Recompute [`Self::cached_vwap`] from `bars` through the SAME
    /// [`super::profile_render::window_indices`] +
    /// [`super::vwap_calc::running_vwap`] calls `render`/`hit_test` make
    /// directly — see [`Primitive::refresh_bar_derived_cache`]'s own
    /// contract. One pass over the span's own bars (never the whole bar
    /// array), so this needs no separate short-circuit of its own,
    /// matching `FixedVolumeProfile`'s own override.
    fn refresh_bar_derived_cache(&mut self, bars: &[Bar]) {
        self.cached_vwap = super::profile_render::window_indices(bars, self.ts1, self.ts2)
            .map(|(start, end)| running_vwap(bars, start, end));
    }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        let lang = current_language();
        // `PropertyGroup::Sigmas` — see `AnchoredVwap::style_properties`'s
        // own comment: no design-approved archetype names this family, and
        // `Sigmas` is the existing group whose own meaning (a σ-band
        // toggle paired with its own multiplier) matches these four rows,
        // same choice as the anchored sibling.
        vec![
            ConfigProperty::extend_lines(self.extend_lines)
                .with_order(8).in_group(PropertyGroup::Extend),
            ConfigProperty::boolean("show_stddev_1", ConfigKey::ShowStdDev1.get(lang), self.show_stddev_1)
                .with_order(20).in_group(PropertyGroup::Sigmas),
            ConfigProperty::number("sigma_1", ConfigKey::SigmaMultiplier1.get(lang), self.sigma_1, Some(0.1), Some(5.0))
                .with_order(21).in_group(PropertyGroup::Sigmas),
            ConfigProperty::boolean("show_stddev_2", ConfigKey::ShowStdDev2.get(lang), self.show_stddev_2)
                .with_order(22).in_group(PropertyGroup::Sigmas),
            ConfigProperty::number("sigma_2", ConfigKey::SigmaMultiplier2.get(lang), self.sigma_2, Some(0.1), Some(10.0))
                .with_order(23).in_group(PropertyGroup::Sigmas),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "extend_lines" => { if let Some(v) = value.as_bool() { self.extend_lines = v; return true; } }
            "show_stddev_1" => { if let Some(v) = value.as_bool() { self.show_stddev_1 = v; return true; } }
            "show_stddev_2" => { if let Some(v) = value.as_bool() { self.show_stddev_2 = v; return true; } }
            "sigma_1" => { if let Some(v) = value.as_number() { self.sigma_1 = v.clamp(0.1, 5.0); return true; } }
            "sigma_2" => { if let Some(v) = value.as_number() { self.sigma_2 = v.clamp(0.1, 10.0); return true; } }
            _ => {}
        }
        false
    }
}

fn fence_ys(
    this: &FixedVwap,
    curve: &[VwapPoint],
    vp: &Viewport,
    ps: &PriceScale,
    chart_height: f64,
) -> (f64, f64) {
    if this.extend_lines {
        return (0.0, chart_height);
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let y_of = |price: f64| vp.price_to_y(price, ps.price_min, ps.price_max);
    for pt in curve {
        let mut prices = vec![pt.vwap];
        if this.show_stddev_1 {
            prices.push(pt.vwap + this.sigma_1 * pt.stddev);
            prices.push(pt.vwap - this.sigma_1 * pt.stddev);
        }
        if this.show_stddev_2 {
            prices.push(pt.vwap + this.sigma_2 * pt.stddev);
            prices.push(pt.vwap - this.sigma_2 * pt.stddev);
        }
        for p in prices {
            let y = y_of(p);
            lo = lo.min(y);
            hi = hi.max(y);
        }
    }
    if lo.is_finite() { (lo, hi) } else { (0.0, chart_height) }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "fixed_vwap",
        display_name: "Fixed VWAP",
        kind: PrimitiveKind::Measurement,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "VWAP over a fixed range + ±1/±2σ deviation bands",
        icon: "fixed_vwap",
        default_color: "#5C6BC0",
        factory: |points, color| {
            let (t1, _) = points.first().copied().unwrap_or((0, 0.0));
            let (t2, _) = points.get(1).copied().unwrap_or((t1 + 72_000_000, 0.0));
            Box::new(FixedVwap::new(t1, t2, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::anchored_vwap::AnchoredVwap;
    use super::super::vwap_calc::running_vwap as direct_running_vwap;

    /// Six one-second bars, `[0, 5]`, with varying price AND volume — rich
    /// enough that a hand-rolled second copy of the running mean/variance
    /// (different summation order, a two-pass variance, ...) would very
    /// likely land on a different float than the shared implementation.
    fn bars_in_range() -> Vec<Bar> {
        vec![
            Bar::with_volume(0, 100.0, 102.0, 99.0, 101.0, 12.0),
            Bar::with_volume(1, 101.0, 103.0, 100.0, 102.5, 7.0),
            Bar::with_volume(2, 102.5, 104.0, 101.5, 103.0, 21.0),
            Bar::with_volume(3, 103.0, 105.5, 102.0, 104.0, 3.0),
            Bar::with_volume(4, 104.0, 106.0, 103.5, 105.5, 16.0),
            Bar::with_volume(5, 105.5, 107.0, 104.0, 106.0, 9.0),
        ]
    }

    /// A bar clearly OUTSIDE `[ts1, ts2]` — wild price/volume, so if it
    /// ever leaked into the window the curve would visibly move.
    fn bar_outside_range() -> Bar {
        Bar::with_volume(6, 1000.0, 1000.0, 1000.0, 1000.0, 1000.0)
    }

    /// #1 + #3 (TESTS list): the fixed VWAP over a synthetic series equals
    /// the anchored one over the SAME span — proven by comparing the two
    /// primitives' own resolved windows fed through the ONE shared
    /// [`running_vwap`], not asserted by inspection. Anchors sit exactly on
    /// bar boundaries (`ts_ms` a multiple of the 1-second bar interval), so
    /// `AnchoredVwap::bar_window`'s snap-to-nearest-bar and
    /// `window_indices`'s range-inclusion both resolve to the identical
    /// `(0, 5)` window — verified explicitly below, not assumed.
    #[test]
    fn fixed_and_anchored_vwap_agree_over_the_same_span() {
        let bars = bars_in_range();

        let mut anchored = AnchoredVwap::new(0, "#5C6BC0");
        anchored.end_ts_ms = Some(5_000);
        let fixed = FixedVwap::new(0, 5_000, "#5C6BC0");

        let anchored_window = anchored.bar_window(&bars).expect("bars are non-empty");
        let fixed_window = super::super::profile_render::window_indices(&bars, fixed.ts1, fixed.ts2)
            .expect("all six bars fall inside [0, 5000]");
        assert_eq!(anchored_window, fixed_window, "both primitives resolve the identical bar window");
        assert_eq!(anchored_window, (0, 5));

        let anchored_curve = direct_running_vwap(&bars, anchored_window.0, anchored_window.1);
        let fixed_curve = direct_running_vwap(&bars, fixed_window.0, fixed_window.1);
        assert_eq!(anchored_curve, fixed_curve, "same window through the SAME running_vwap call must be byte-identical");
        assert_eq!(anchored_curve.len(), 6);

        // Sanity: this is not a degenerate all-equal fixture — the VWAP
        // genuinely moves bar to bar, so an equality assertion here is
        // actually exercising the maths, not trivially always true.
        assert_ne!(anchored_curve[0].vwap, anchored_curve[5].vwap);
    }

    /// #2 (TESTS list): the span is honoured — a bar outside `[ts1, ts2]`
    /// never moves the line, on either end.
    #[test]
    fn bars_outside_the_span_do_not_move_the_curve() {
        let mut bars = bars_in_range();
        let baseline = FixedVwap::new(0, 5_000, "#5C6BC0");
        let baseline_window = super::super::profile_render::window_indices(&bars, baseline.ts1, baseline.ts2).unwrap();
        let baseline_curve = direct_running_vwap(&bars, baseline_window.0, baseline_window.1);

        // Append a wild bar past ts2 (timestamp 6s, ts2 caps at 5s).
        bars.push(bar_outside_range());
        let widened_window = super::super::profile_render::window_indices(&bars, baseline.ts1, baseline.ts2).unwrap();
        let widened_curve = direct_running_vwap(&bars, widened_window.0, widened_window.1);
        assert_eq!(baseline_curve, widened_curve, "a bar past ts2 must not enter the window");

        // Prepend a wild bar before ts1 by shifting the whole fixture's
        // timestamps 10s later and re-anchoring the span to the SAME 6
        // in-range bars — the wild bar now sits chronologically first.
        let mut shifted: Vec<Bar> = bars_in_range().iter().map(|b| Bar::with_volume(b.timestamp + 10, b.open, b.high, b.low, b.close, b.volume)).collect();
        let mut with_leading_outlier = vec![Bar::with_volume(0, 1000.0, 1000.0, 1000.0, 1000.0, 1000.0)];
        with_leading_outlier.append(&mut shifted);
        let leading_window = super::super::profile_render::window_indices(&with_leading_outlier, 10_000, 15_000).unwrap();
        let leading_curve = direct_running_vwap(&with_leading_outlier, leading_window.0, leading_window.1);
        // `bar_idx` is an ABSOLUTE index into the array, and prepending a
        // bar legitimately shifts every real bar's own index by one — the
        // claim under test is that the outlier's PRICE/VOLUME never enters
        // the vwap/stddev maths, so compare those two fields only.
        let vwap_stddev = |curve: &[VwapPoint]| -> Vec<(f64, f64)> {
            curve.iter().map(|p| (p.vwap, p.stddev)).collect()
        };
        assert_eq!(
            vwap_stddev(&baseline_curve), vwap_stddev(&leading_curve),
            "a bar before ts1 must not enter the window either",
        );
    }

    /// #4 (TESTS list): the cache matches a direct computation of the SAME
    /// call `render`/`hit_test` make — the hook fills `cached_vwap` from
    /// real bars through `window_indices` + `running_vwap`, never a
    /// second, independently-computed curve.
    #[test]
    fn hook_fills_cached_vwap_from_the_same_render_path() {
        let bars = bars_in_range();
        let mut fvwap = FixedVwap::new(0, 5_000, "#5C6BC0");

        // Before the hook ever runs, a freshly constructed primitive has no
        // cached curve — `render`/`hit_test` never read this field, so
        // this is a legitimate state, not an error.
        assert!(fvwap.cached_vwap.is_none());

        fvwap.refresh_bar_derived_cache(&bars);

        let window = super::super::profile_render::window_indices(&bars, fvwap.ts1, fvwap.ts2).unwrap();
        let direct = direct_running_vwap(&bars, window.0, window.1);
        assert_eq!(fvwap.cached_vwap, Some(direct), "cache must equal the SAME running_vwap() call render/hit_test make");
    }

    /// `refresh_bar_derived_cache` never mutates anything an analyst set —
    /// running it twice over a primitive with edited style leaves every
    /// non-cache field byte-for-byte identical (`cached_vwap` is
    /// `#[serde(skip)]`, so it never appears in `to_json()` at all), and
    /// the cache itself is idempotent over the same bars — mirrors
    /// `FixedVolumeProfile`'s own `pass_never_mutates_analyst_set_style_run_twice`.
    #[test]
    fn pass_never_mutates_analyst_set_style_run_twice() {
        let bars = bars_in_range();
        let mut fvwap = FixedVwap::new(0, 5_000, "#5C6BC0");
        fvwap.data.color.stroke = "#ff0000".to_string();
        fvwap.data.width = 3.0;
        fvwap.sigma_1 = 1.5;
        fvwap.sigma_2 = 3.0;
        fvwap.show_stddev_1 = false;

        let before = fvwap.to_json();
        fvwap.refresh_bar_derived_cache(&bars);
        let after_first = fvwap.to_json();
        assert_eq!(before, after_first, "the pass must not change any persisted field");

        let curve_after_first = fvwap.cached_vwap.clone();
        fvwap.refresh_bar_derived_cache(&bars);
        let after_second = fvwap.to_json();
        assert_eq!(after_first, after_second, "a second run over the same bars must not drift anything either");
        assert_eq!(fvwap.cached_vwap, curve_after_first, "re-running over the SAME bars must be idempotent");
    }
}
