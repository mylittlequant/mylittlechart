//! Regression Trend primitive
//!
//! A linear regression channel with a center line calculated from price data
//! and parallel lines at standard deviation distances.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{SettingsKey, current_language};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, Archetype, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    point_to_line_distance, HIT_TOLERANCE, CONTROL_POINT_RADIUS, CONTROL_POINT_HIT_RADIUS,
    extend_segment_to_edges,
    RenderContext, crisp,
    render_primitive_text_rotated, calculate_line_text_params,
    config::{ConfigProperty, PropertyValue, PropertyCategory, PropertyGroup},
    AREA_FILL_OPACITY, default_area_fill_opacity,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegressionTrend {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64,
    pub price1: f64,
    #[serde(default)]
    pub ts2: i64,
    pub price2: f64,
    #[serde(default = "default_std_dev")]
    pub std_dev_mult: f64,
    #[serde(default)]
    pub use_upper_deviation: bool,
    #[serde(default = "default_true")]
    pub show_center: bool,
    #[serde(default = "default_true")]
    pub fill: bool,
    #[serde(default = "default_area_fill_opacity")]
    pub fill_opacity: f64,
    #[serde(default)]
    pub extend_left: bool,
    #[serde(default)]
    pub extend_right: bool,
}

fn default_true() -> bool { true }
fn default_std_dev() -> f64 { 2.0 }

impl RegressionTrend {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "regression_trend".to_string(),
                display_name: "Regression Trend".to_string(),
                color: PrimitiveColor::with_fill(color, color),
                width: 2.0,
                ..Default::default()
            },
            ts1, price1, ts2, price2,
            std_dev_mult: 2.0,
            use_upper_deviation: false,
            show_center: true,
            fill: true,
            fill_opacity: AREA_FILL_OPACITY,
            extend_left: false,
            extend_right: false,
        }
    }

    pub fn channel_offset(&self) -> f64 {
        let price_range = (self.price2 - self.price1).abs();
        (price_range * 0.15).max(self.price1 * 0.02) * self.std_dev_mult / 2.0
    }
}

impl Primitive for RegressionTrend {
    fn type_id(&self) -> &'static str { "regression_trend" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Channel }
    // Shares `PrimitiveKind::Channel` with parallel/disjoint/flat-top
    // channels, but is its own sigma-band archetype, not the generic
    // `Channel` shape.
    fn archetype(&self) -> Archetype { Archetype::Regression }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, self.price1), (self.ts2, self.price2)] }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if points.len() >= 2 {
            self.ts1 = points[0].0; self.price1 = points[0].1;
            self.ts2 = points[1].0; self.price2 = points[1].1;
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        self.ts1 += ts_delta_ms; self.ts2 += ts_delta_ms;
        self.price1 += price_delta; self.price2 += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        match point_type {
            ControlPointType::Point1 => { self.ts1 = ts_ms; self.price1 = price; }
            ControlPointType::Point2 => { self.ts2 = ts_ms; self.price2 = price; }
            ControlPointType::Move => {
                let mid_ts = (self.ts1 + self.ts2) / 2;
                let mid_price = (self.price1 + self.price2) / 2.0;
                let td = ts_ms - mid_ts;
                let pd = price - mid_price;
                self.translate(td, pd);
            }
            _ => {}
        }
    }

    fn hit_test(&self, screen_x: f64, screen_y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> HitTestResult {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let x2 = viewport.bar_to_x_f64(bar2);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        let offset = self.channel_offset();
        let upper_y1 = viewport.price_to_y(self.price1 + offset, price_scale.price_min, price_scale.price_max);
        let upper_y2 = viewport.price_to_y(self.price2 + offset, price_scale.price_min, price_scale.price_max);
        let lower_y1 = viewport.price_to_y(self.price1 - offset, price_scale.price_min, price_scale.price_max);
        let lower_y2 = viewport.price_to_y(self.price2 - offset, price_scale.price_min, price_scale.price_max);

        if check_point_hit(screen_x, screen_y, x1, y1) { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if check_point_hit(screen_x, screen_y, x2, y2) { return HitTestResult::ControlPoint(ControlPointType::Point2); }

        let cx = (x1 + x2) / 2.0; let cy = (y1 + y2) / 2.0;
        if check_point_hit(screen_x, screen_y, cx, cy) { return HitTestResult::ControlPoint(ControlPointType::Move); }

        // Reach exactly as far as `render` draws — all three lines share
        // `ts1`/`ts2`, so their extended x-coordinates always agree.
        let (cx1, cy1, cx2, cy2) = extend_segment_to_edges(x1, y1, x2, y2, self.extend_left, self.extend_right, viewport.chart_width);
        let (eu_x1, eu_y1, eu_x2, eu_y2) = extend_segment_to_edges(x1, upper_y1, x2, upper_y2, self.extend_left, self.extend_right, viewport.chart_width);
        let (el_x1, el_y1, el_x2, el_y2) = extend_segment_to_edges(x1, lower_y1, x2, lower_y2, self.extend_left, self.extend_right, viewport.chart_width);
        if self.show_center && point_to_line_distance(screen_x, screen_y, cx1, cy1, cx2, cy2) < HIT_TOLERANCE { return HitTestResult::Body; }
        if point_to_line_distance(screen_x, screen_y, eu_x1, eu_y1, eu_x2, eu_y2) < HIT_TOLERANCE { return HitTestResult::Body; }
        if point_to_line_distance(screen_x, screen_y, el_x1, el_y1, el_x2, el_y2) < HIT_TOLERANCE { return HitTestResult::Body; }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let x2 = viewport.bar_to_x_f64(bar2);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        let mut points = vec![ControlPoint::point1(x1, y1), ControlPoint::point2(x2, y2)];
        let cx = (x1 + x2) / 2.0; let cy = (y1 + y2) / 2.0;
        let dist = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
        if dist > CONTROL_POINT_RADIUS * 4.0 { points.push(ControlPoint::move_point(cx, cy)); }
        points
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y2 = ctx.price_to_y(self.price2);

        let offset = self.channel_offset();
        let upper_y1 = ctx.price_to_y(self.price1 + offset);
        let upper_y2 = ctx.price_to_y(self.price2 + offset);
        let lower_y1 = ctx.price_to_y(self.price1 - offset);
        let lower_y2 = ctx.price_to_y(self.price2 - offset);

        // An extended side draws to the panel edge in that direction — off
        // by default, so an unextended primitive draws the exact same
        // three segments as before this pass. All three lines share
        // `ts1`/`ts2`, so their extended x-coordinates always agree.
        let chart_width = ctx.chart_width();
        let (cx1, cy1, cx2, cy2) = extend_segment_to_edges(x1, y1, x2, y2, self.extend_left, self.extend_right, chart_width);
        let (eu_x1, eu_y1, eu_x2, eu_y2) = extend_segment_to_edges(x1, upper_y1, x2, upper_y2, self.extend_left, self.extend_right, chart_width);
        let (el_x1, el_y1, el_x2, el_y2) = extend_segment_to_edges(x1, lower_y1, x2, lower_y2, self.extend_left, self.extend_right, chart_width);

        if self.fill {
            let base = self.data.color.fill.as_deref().unwrap_or(&self.data.color.stroke);
            let fill_color = format!("{}{:02x}", &base[..7], (self.fill_opacity * 255.0) as u8);
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.move_to(eu_x1, eu_y1); ctx.line_to(eu_x2, eu_y2);
            ctx.line_to(el_x2, el_y2); ctx.line_to(el_x1, el_y1);
            ctx.close_path(); ctx.fill();
        }

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        ctx.begin_path();
        ctx.move_to(crisp(eu_x1, dpr), crisp(eu_y1, dpr));
        ctx.line_to(crisp(eu_x2, dpr), crisp(eu_y2, dpr));
        ctx.stroke();

        ctx.begin_path();
        ctx.move_to(crisp(el_x1, dpr), crisp(el_y1, dpr));
        ctx.line_to(crisp(el_x2, dpr), crisp(el_y2, dpr));
        ctx.stroke();

        if self.show_center {
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.begin_path();
            ctx.move_to(crisp(cx1, dpr), crisp(cy1, dpr));
            ctx.line_to(crisp(cx2, dpr), crisp(cy2, dpr));
            ctx.stroke();
        }
        ctx.set_line_dash(&[]);

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let (text_y1, text_y2) = match text.v_align {
                    super::super::TextAlign::Start => (upper_y1, upper_y2),
                    super::super::TextAlign::End => (lower_y1, lower_y2),
                    super::super::TextAlign::Center => (y1, y2),
                };
                let params = calculate_line_text_params(x1, text_y1, x2, text_y2, text);
                render_primitive_text_rotated(ctx, text, params.x, params.y, &self.data.color.stroke, params.rotation);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        // Regression archetype (W3, `docs/mlc/plans/archetype-modals-into-the-app-2026-08-11.md`):
        // `regression_tabs()` (`mlc-ui-lab`'s `spec.rs`) declares four
        // groups on the "sigma" subject tab — Calculation, Sigmas, Baseline,
        // Metrics — plus an Extend group. This primitive's own
        // `archetype()` override already points at `Archetype::Regression`
        // (see above); this is the first pass that gives it the design's
        // own named groups instead of one anonymous run.
        //
        // `PropertyGroup::Calculation` (source dropdown / period mode /
        // bar-count) is not declared: this primitive draws a fixed offset
        // from `std_dev_mult` (see `channel_offset` above), never an actual
        // least-squares fit against a price series — there is no source
        // bar, no sampled window, nothing a "Close/Open/High/Low" or
        // "bars"/"points" toggle could drive. No backing field exists and
        // none can be added inside this trait method either: `style_properties(&self)`
        // is never handed `bars`, so a real fit is a compute-layer change,
        // not a declarative one.
        //
        // `PropertyGroup::Sigmas` — the lab's own ratio-style table (one row
        // per ±1σ/±2σ/±3σ band: multiplier, colour swatch, per-band fill
        // checkbox) needs a property kind this crate has none of yet (W4).
        // This primitive only ever tracks ONE band width (`std_dev_mult`)
        // and one shared fill toggle, not three independent bands, so
        // there is no honest per-row mapping even once W4 lands — what
        // DOES have a real single-band equivalent is the group's own
        // trailing "Fill Opacity" slider and this primitive's existing
        // `fill` master toggle, both re-homed into this group below rather
        // than left in the default run.
        //
        // `PropertyGroup::Baseline` — the lab's "Show"/colour/width trio for
        // the dashed centre line. `show_center` is a real, already-rendered
        // field (`render`'s own `if self.show_center` branch draws the
        // dashed line; `hit_test` gates the same way) — declared below.
        // Colour and width are NOT declared: this primitive has no
        // baseline-specific stroke of its own, the centre line reuses
        // `data.color.stroke`/`data.width` — the SAME pair the two sigma
        // band lines draw with — so there is no independent field a
        // baseline-only colour/width control could write into without
        // silently also repainting the bands.
        //
        // `PropertyGroup::Extend` — `extend_left`/`extend_right` now really
        // extend all three lines (centre + both sigma bands) to the panel
        // edge (`render`/`hit_test` above, via `extend_segment_to_edges`),
        // so this is a real declared toggle rather than the dead field it
        // used to be.
        //
        // The Metrics group (`ShowSlope`/`ShowRSquared` toggles + the
        // Slope/R² stat rows) is left out ENTIRELY: the two stat rows need
        // a computed-stat block, a property kind that does not exist (W4),
        // and even with that kind built there is nothing to compute from —
        // this primitive has no linear-regression fit at all (see the
        // Calculation note above), so the two toggles that would gate that
        // computed display have no backing field or compute path either.
        vec![
            ConfigProperty::fill(self.fill).with_category(PropertyCategory::AppearanceFill).with_order(20).in_group(PropertyGroup::Sigmas),
            ConfigProperty::fill_opacity(self.fill_opacity * 100.0).with_category(PropertyCategory::AppearanceFill).with_order(21).in_group(PropertyGroup::Sigmas),
            ConfigProperty::boolean("show_center", SettingsKey::Show.get(current_language()), self.show_center)
                .with_category(PropertyCategory::AppearanceLine).with_order(22).in_group(PropertyGroup::Baseline),
            ConfigProperty::extend_left(self.extend_left).with_category(PropertyCategory::AppearanceLine).with_order(23).in_group(PropertyGroup::Extend),
            ConfigProperty::extend_right(self.extend_right).with_category(PropertyCategory::AppearanceLine).with_order(24).in_group(PropertyGroup::Extend),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "fill" => { if let Some(v) = value.as_bool() { self.fill = v; return true; } }
            "fill_opacity" => { if let Some(v) = value.as_number() { self.fill_opacity = (v / 100.0).clamp(0.0, 1.0); return true; } }
            "show_center" => { if let Some(v) = value.as_bool() { self.show_center = v; return true; } }
            "extend_left" => { if let Some(v) = value.as_bool() { self.extend_left = v; return true; } }
            "extend_right" => { if let Some(v) = value.as_bool() { self.extend_right = v; return true; } }
            _ => {}
        }
        false
    }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2)
}

fn create_regression_trend(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
    let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, price1));
    Box::new(RegressionTrend::new(ts1, price1, ts2, price2, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "regression_trend",
        display_name: "Regression Trend",
        kind: PrimitiveKind::Channel,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Draw a linear regression channel",
        icon: "regression_trend",
        default_color: "#9C27B0",
        factory: create_regression_trend,
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod extend_tests {
    use super::*;
    use crate::{Bar, PriceScale, Viewport};

    fn fixture() -> (Vec<Bar>, Viewport, PriceScale) {
        let bars: Vec<Bar> = (0..6).map(|i| Bar::new(i, 100.0, 102.0, 98.0, 100.0)).collect();
        let viewport = Viewport { chart_width: 1200.0, chart_height: 600.0, bar_count: bars.len(), ..Default::default() };
        let price_scale = PriceScale { price_min: 50.0, price_max: 150.0, ..Default::default() };
        (bars, viewport, price_scale)
    }

    /// A probe point sitting exactly on the extended centre line, at the
    /// panel's own left edge — misses an unextended regression channel but
    /// resolves to `Body` once `extend_left` is on, because `hit_test` now
    /// tests the SAME extended segment `render` draws
    /// (`extend_segment_to_edges`).
    #[test]
    fn extend_left_reaches_the_panel_edge_on_the_centre_line() {
        let (bars, viewport, price_scale) = fixture();
        // ts values land exactly on bar indices 2 and 5 of `fixture()`'s
        // 6-bar slice (NOT 0 — the left anchor must not already sit at the
        // panel's own left edge, or "extending" it would be a no-op and
        // the probe would already be an existing control point).
        let channel = RegressionTrend::new(2_000, 100.0, 5_000, 110.0, "#9C27B0");
        assert!(channel.show_center, "the centre line must be on for this probe to land on it");

        let bar1 = timestamp_ms_to_bar_f64(&bars, channel.ts1);
        let bar2 = timestamp_ms_to_bar_f64(&bars, channel.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let y1 = viewport.price_to_y(channel.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(bar2);
        let y2 = viewport.price_to_y(channel.price2, price_scale.price_min, price_scale.price_max);
        let (probe_x, probe_y, _, _) = extend_segment_to_edges(x1, y1, x2, y2, true, false, viewport.chart_width);
        assert_eq!(probe_x, 0.0, "the probe sits at the panel's own left edge");

        assert!(
            matches!(channel.hit_test(probe_x, probe_y, &bars, &viewport, &price_scale), HitTestResult::Miss),
            "unextended channel misses a point on the panel's left edge",
        );

        let mut extended = channel;
        extended.extend_left = true;
        assert!(
            matches!(extended.hit_test(probe_x, probe_y, &bars, &viewport, &price_scale), HitTestResult::Body),
            "extend_left reaches the same point once the centre line is drawn that far",
        );
    }
}
