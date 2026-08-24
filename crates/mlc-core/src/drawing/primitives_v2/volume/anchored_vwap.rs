//! Anchored VWAP drawing primitive.
//!
//! Computes a running VWAP from a user-chosen anchor bar to the last bar
//! (or an optional cap). Typical price = (high + low + close) / 3. Running
//! variance yields ±1σ and ±2σ bands drawn as semi-transparent fills — the
//! maths itself lives in [`super::vwap_calc::running_vwap`], the ONE place
//! both this type and its "fixed" sibling ([`super::fixed_vwap::FixedVwap`])
//! compute a VWAP curve, so the two can never disagree over the same span.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{ConfigKey, current_language};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, render_primitive_text, TextAlign,
    config::{ConfigProperty, PropertyValue, PropertyGroup},
};
use super::vwap_calc::{running_vwap, draw_band, draw_polyline};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnchoredVwap {
    pub data: PrimitiveData,
    /// Timestamp (ms) of the anchor bar. VWAP is accumulated from this bar onward.
    #[serde(default)]
    pub anchor_ts: i64,
    /// Whether to draw ±1 σ deviation bands.
    #[serde(default = "default_true")]
    pub show_stddev_1: bool,
    /// Whether to draw ±2 σ deviation bands.
    #[serde(default = "default_true")]
    pub show_stddev_2: bool,
    /// Standard-deviation multiplier for the inner band (was a hardcoded
    /// `1.0` in both `hit_test` and `render`).
    #[serde(default = "default_sigma_1")]
    pub sigma_1: f64,
    /// Standard-deviation multiplier for the outer band (was a hardcoded
    /// `2.0`).
    #[serde(default = "default_sigma_2")]
    pub sigma_2: f64,
    /// Optional cap on the accumulation range. `None` runs to the last
    /// loaded bar exactly as before — this field is purely additive.
    /// `Some(ts)` stops the running VWAP/bands at that bar, letting the
    /// user pull the anchored VWAP into a bounded box instead of an
    /// unbounded ray.
    #[serde(default)]
    pub end_ts_ms: Option<i64>,
}

fn default_true() -> bool { true }
fn default_sigma_1() -> f64 { 1.0 }
fn default_sigma_2() -> f64 { 2.0 }

impl AnchoredVwap {
    pub fn new(anchor_ts: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "anchored_vwap".to_string(),
                display_name: "Anchored VWAP".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            anchor_ts,
            show_stddev_1: true,
            show_stddev_2: true,
            sigma_1: 1.0,
            sigma_2: 2.0,
            end_ts_ms: None,
        }
    }

    /// Index of the anchor bar (floor of `anchor_ts` when between bars).
    fn anchor_bar_idx(&self, bars: &[Bar]) -> usize {
        let anchor_sec = self.anchor_ts / 1000;
        match bars.binary_search_by_key(&anchor_sec, |b| b.timestamp) {
            Ok(i) => i,
            Err(i) => i.min(bars.len().saturating_sub(1)),
        }
    }

    /// Index of the bar the accumulation stops at.
    ///
    /// `None` (the default) resolves to the last loaded bar, byte-identical
    /// to the pre-cap behaviour. `Some(ts)` resolves to the last bar at or
    /// before `ts`, clamped so it never falls before `anchor_idx` — a cap
    /// dragged past the anchor degenerates to a single-bar VWAP instead of
    /// an inverted range.
    fn end_bar_idx(&self, bars: &[Bar], anchor_idx: usize) -> usize {
        let last = bars.len().saturating_sub(1);
        match self.end_ts_ms {
            Some(end_ms) => {
                let end_sec = end_ms / 1000;
                let count = bars.partition_point(|b| b.timestamp <= end_sec);
                count.saturating_sub(1).max(anchor_idx).min(last)
            }
            None => last,
        }
    }

    /// The absolute `[start_idx, end_idx]` bar-index window this VWAP
    /// accumulates over — [`Self::anchor_bar_idx`] through
    /// [`Self::end_bar_idx`], exposed (rather than re-derived) so a cross-
    /// check against [`super::fixed_vwap::FixedVwap`] over the identical
    /// span reads the SAME window `render`/`hit_test` resolve internally.
    /// `None` for an empty `bars` slice — there is no window to report.
    pub(super) fn bar_window(&self, bars: &[Bar]) -> Option<(usize, usize)> {
        if bars.is_empty() { return None; }
        let anchor_idx = self.anchor_bar_idx(bars);
        let end_idx = self.end_bar_idx(bars, anchor_idx);
        (end_idx >= anchor_idx).then_some((anchor_idx, end_idx))
    }
}

impl Primitive for AnchoredVwap {
    fn type_id(&self) -> &'static str { "anchored_vwap" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Measurement }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::SingleClick }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.anchor_ts, 0.0)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(t, _)) = pts.first() { self.anchor_ts = t; }
    }
    fn translate(&mut self, td: i64, _pd: f64) {
        self.anchor_ts += td;
        if let Some(end) = self.end_ts_ms.as_mut() { *end += td; }
    }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, _price: f64) {
        match pt {
            ControlPointType::Point1 => self.anchor_ts = ts_ms,
            ControlPointType::Point2 => self.end_ts_ms = Some(ts_ms),
            ControlPointType::Move => self.translate(ts_ms - self.anchor_ts, 0.0),
            _ => {}
        }
    }

    /// The anchor, then the VWAP curve itself.
    ///
    /// It used to be the anchor alone: the curve — the thing the primitive IS
    /// — could not be clicked anywhere along its length. The accumulation is
    /// run only as far as the bar under the cursor, so this stays one pass
    /// over the bars left of `sx` rather than a full curve build per mouse
    /// move.
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let b = timestamp_ms_to_bar_f64(bars, self.anchor_ts);
        let x = vp.bar_to_x_f64(b);
        if (sx - x).abs() < 20.0 {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        let Some((anchor_idx, end_idx)) = self.bar_window(bars) else {
            return HitTestResult::Miss;
        };
        let end_x = vp.bar_to_x_f64(end_idx as f64);
        if (sx - end_x).abs() < 20.0 {
            return HitTestResult::ControlPoint(ControlPointType::Point2);
        }
        if sx < x {
            return HitTestResult::Miss;
        }
        // Which bar the cursor is over.
        let hover = vp.x_to_bar_f64(sx).round();
        if !hover.is_finite() || hover < anchor_idx as f64 {
            return HitTestResult::Miss;
        }
        let hover_idx = (hover as usize).min(bars.len() - 1);
        if hover_idx < anchor_idx || hover_idx > end_idx {
            return HitTestResult::Miss;
        }

        // Same shared computation `render` draws from — the cumulative sum
        // through `hover_idx` is the curve's own last point.
        let Some(last) = running_vwap(bars, anchor_idx, hover_idx).into_iter().last() else {
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

    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let b = timestamp_ms_to_bar_f64(bars, self.anchor_ts);
        let cy = vp.price_to_y((ps.price_min + ps.price_max) / 2.0, ps.price_min, ps.price_max);
        let mut pts = vec![ControlPoint::point1(vp.bar_to_x_f64(b), cy)];
        if let Some((_, end_idx)) = self.bar_window(bars) {
            pts.push(ControlPoint::point2(vp.bar_to_x_f64(end_idx as f64), cy));
        }
        pts
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let bars = ctx.bars();
        if bars.is_empty() { return; }

        let chart_width = ctx.chart_width();
        let chart_height = ctx.chart_height();
        let anchor_x = ctx.ts_to_x_ms(self.anchor_ts);

        // ── Find anchor + end bar index ──────────────────────────────────────
        let Some((anchor_idx, end_idx)) = self.bar_window(bars) else { return; };

        // ── Compute running VWAP + stddev ────────────────────────────────────
        // Shared with `FixedVwap` — see `super::vwap_calc::running_vwap`'s
        // own doc.
        let curve = running_vwap(bars, anchor_idx, end_idx);
        if curve.is_empty() { return; }

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

        // ── Draw ±2σ band ────────────────────────────────────────────────────
        if self.show_stddev_2 && sd2_upper.len() >= 2 {
            draw_band(ctx, &sd2_upper, &sd2_lower, "rgba(100, 149, 237, 0.10)");
        }

        // ── Draw ±1σ band ────────────────────────────────────────────────────
        if self.show_stddev_1 && sd1_upper.len() >= 2 {
            draw_band(ctx, &sd1_upper, &sd1_lower, "rgba(100, 149, 237, 0.18)");
        }

        // ── Draw ±2σ outlines ────────────────────────────────────────────────
        if self.show_stddev_2 && sd2_upper.len() >= 2 {
            draw_polyline(ctx, &sd2_upper, "rgba(100, 149, 237, 0.40)", 1.0 * dpr);
            draw_polyline(ctx, &sd2_lower, "rgba(100, 149, 237, 0.40)", 1.0 * dpr);
        }

        // ── Draw ±1σ outlines ────────────────────────────────────────────────
        if self.show_stddev_1 && sd1_upper.len() >= 2 {
            draw_polyline(ctx, &sd1_upper, "rgba(100, 149, 237, 0.60)", 1.0 * dpr);
            draw_polyline(ctx, &sd1_lower, "rgba(100, 149, 237, 0.60)", 1.0 * dpr);
        }

        // ── Draw VWAP line ───────────────────────────────────────────────────
        if vwap_points.len() >= 2 {
            draw_polyline(ctx, &vwap_points, &self.data.color.stroke, self.data.width * dpr);
        }

        // ── Draw vertical anchor line ────────────────────────────────────────
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width * dpr);
        ctx.set_line_dash(&[4.0 * dpr, 4.0 * dpr]);
        ctx.begin_path();
        ctx.move_to(crisp(anchor_x, dpr), 0.0);
        ctx.line_to(crisp(anchor_x, dpr), chart_height);
        ctx.stroke();
        ctx.set_line_dash(&[]);

        // ── Anchor marker dot ────────────────────────────────────────────────
        let anchor_mid_y = chart_height / 2.0;
        ctx.set_fill_color(&self.data.color.stroke);
        ctx.set_global_alpha(1.0);
        ctx.begin_path();
        ctx.arc(anchor_x, anchor_mid_y, 4.0 * dpr, 0.0, std::f64::consts::TAU);
        ctx.fill();

        // ── Optional text label ──────────────────────────────────────────────
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => anchor_x,
                    TextAlign::Center => (anchor_x + chart_width) / 2.0,
                    TextAlign::End => chart_width,
                };
                let text_y = match text.v_align {
                    TextAlign::Start => -text_offset,
                    TextAlign::Center => chart_height / 2.0,
                    TextAlign::End => chart_height + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }

        let _ = chart_width;
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        let lang = current_language();
        // `PropertyGroup::Sigmas` — no design-approved archetype names this
        // family (owner-measured, `docs/mlc/plans/archetype-modals-into-
        // the-app-2026-08-11.md` has no volume-profile/VWAP concept), but
        // `Sigmas` is an EXISTING group whose own meaning — a σ-band
        // toggle paired with its own multiplier — is exactly what these
        // four rows are: the regression channel's own sigma-band group
        // reused for the same subject (a ±σ band), not invented for this
        // type. `AppearanceLine`'s default category (unset here, same as
        // before this pass) still governs which TAB the group renders on.
        vec![
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
            "show_stddev_1" => { if let Some(v) = value.as_bool() { self.show_stddev_1 = v; return true; } }
            "show_stddev_2" => { if let Some(v) = value.as_bool() { self.show_stddev_2 = v; return true; } }
            "sigma_1" => { if let Some(v) = value.as_number() { self.sigma_1 = v.clamp(0.1, 5.0); return true; } }
            "sigma_2" => { if let Some(v) = value.as_number() { self.sigma_2 = v.clamp(0.1, 10.0); return true; } }
            _ => {}
        }
        false
    }
}

// ─── Registration ─────────────────────────────────────────────────────────────

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "anchored_vwap",
        display_name: "Anchored VWAP",
        kind: PrimitiveKind::Measurement,
        click_behavior: ClickBehavior::SingleClick,
        tooltip: "VWAP from anchor bar + ±1/±2σ deviation bands",
        icon: "anchored_vwap",
        default_color: "#5C6BC0",
        factory: |points, color| {
            let (t, _) = points.first().copied().unwrap_or((0, 0.0));
            Box::new(AnchoredVwap::new(t, color))
        },
        supports_text: true,
        has_levels: false,
    }
}
