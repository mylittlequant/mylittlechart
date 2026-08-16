//! Disjoint Channel primitive
//!
//! A channel with two non-parallel lines (widening or narrowing).

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
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
pub struct DisjointChannel {
    pub data: PrimitiveData,
    pub line1_ts1: i64, pub line1_price1: f64,
    pub line1_ts2: i64, pub line1_price2: f64,
    pub line2_ts1: i64, pub line2_price1: f64,
    pub line2_ts2: i64, pub line2_price2: f64,
    #[serde(default = "default_true")] pub fill: bool,
    #[serde(default = "default_area_fill_opacity")] pub fill_opacity: f64,
    #[serde(default)] pub extend_left: bool,
    #[serde(default)] pub extend_right: bool,
}

fn default_true() -> bool { true }

impl DisjointChannel {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, offset: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "disjoint_channel".to_string(),
                display_name: "Disjoint Channel".to_string(),
                color: PrimitiveColor::with_fill(color, color),
                width: 2.0,
                ..Default::default()
            },
            line1_ts1: ts1, line1_price1: price1,
            line1_ts2: ts2, line1_price2: price2,
            line2_ts1: ts1, line2_price1: price1 + offset,
            line2_ts2: ts2, line2_price2: price2 + offset,
            fill: true, fill_opacity: AREA_FILL_OPACITY, extend_left: false, extend_right: false,
        }
    }

    pub fn with_points(
        l1_ts1: i64, l1_price1: f64, l1_ts2: i64, l1_price2: f64,
        l2_ts1: i64, l2_price1: f64, l2_ts2: i64, l2_price2: f64,
        color: &str,
    ) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "disjoint_channel".to_string(),
                display_name: "Disjoint Channel".to_string(),
                color: PrimitiveColor::with_fill(color, color),
                width: 2.0,
                ..Default::default()
            },
            line1_ts1: l1_ts1, line1_price1: l1_price1,
            line1_ts2: l1_ts2, line1_price2: l1_price2,
            line2_ts1: l2_ts1, line2_price1: l2_price1,
            line2_ts2: l2_ts2, line2_price2: l2_price2,
            fill: true, fill_opacity: AREA_FILL_OPACITY, extend_left: false, extend_right: false,
        }
    }
}

impl Primitive for DisjointChannel {
    fn type_id(&self) -> &'static str { "disjoint_channel" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Channel }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::FourPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> {
        vec![
            (self.line1_ts1, self.line1_price1),
            (self.line1_ts2, self.line1_price2),
            (self.line2_ts1, self.line2_price1),
            (self.line2_ts2, self.line2_price2),
        ]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if !points.is_empty() { self.line1_ts1 = points[0].0; self.line1_price1 = points[0].1; }
        if points.len() >= 2 { self.line1_ts2 = points[1].0; self.line1_price2 = points[1].1; }
        if points.len() >= 3 { self.line2_ts1 = points[2].0; self.line2_price1 = points[2].1; }
        if points.len() >= 4 {
            self.line2_ts2 = points[3].0; self.line2_price2 = points[3].1;
        } else if points.len() == 3 {
            let offset = self.line2_price1 - self.line1_price1;
            self.line2_ts2 = self.line1_ts2;
            self.line2_price2 = self.line1_price2 + offset;
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        self.line1_ts1 += ts_delta_ms; self.line1_ts2 += ts_delta_ms;
        self.line1_price1 += price_delta; self.line1_price2 += price_delta;
        self.line2_ts1 += ts_delta_ms; self.line2_ts2 += ts_delta_ms;
        self.line2_price1 += price_delta; self.line2_price2 += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        match point_type {
            ControlPointType::Point1 => { self.line1_ts1 = ts_ms; self.line1_price1 = price; }
            ControlPointType::Point2 => { self.line1_ts2 = ts_ms; self.line1_price2 = price; }
            ControlPointType::Point3 => { self.line2_ts1 = ts_ms; self.line2_price1 = price; }
            ControlPointType::Point4 => { self.line2_ts2 = ts_ms; self.line2_price2 = price; }
            ControlPointType::Move => {
                let mid_ts = (self.line1_ts1 + self.line1_ts2 + self.line2_ts1 + self.line2_ts2) / 4;
                let mid_price = (self.line1_price1 + self.line1_price2 + self.line2_price1 + self.line2_price2) / 4.0;
                let td = ts_ms - mid_ts;
                let pd = price - mid_price;
                self.translate(td, pd);
            }
            _ => {}
        }
    }

    fn hit_test(&self, screen_x: f64, screen_y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> HitTestResult {
        let l1_bar1 = timestamp_ms_to_bar_f64(bars, self.line1_ts1);
        let l1_bar2 = timestamp_ms_to_bar_f64(bars, self.line1_ts2);
        let l2_bar1 = timestamp_ms_to_bar_f64(bars, self.line2_ts1);
        let l2_bar2 = timestamp_ms_to_bar_f64(bars, self.line2_ts2);
        let l1_x1 = viewport.bar_to_x_f64(l1_bar1);
        let l1_y1 = viewport.price_to_y(self.line1_price1, price_scale.price_min, price_scale.price_max);
        let l1_x2 = viewport.bar_to_x_f64(l1_bar2);
        let l1_y2 = viewport.price_to_y(self.line1_price2, price_scale.price_min, price_scale.price_max);
        let l2_x1 = viewport.bar_to_x_f64(l2_bar1);
        let l2_y1 = viewport.price_to_y(self.line2_price1, price_scale.price_min, price_scale.price_max);
        let l2_x2 = viewport.bar_to_x_f64(l2_bar2);
        let l2_y2 = viewport.price_to_y(self.line2_price2, price_scale.price_min, price_scale.price_max);

        if check_point_hit(screen_x, screen_y, l1_x1, l1_y1) { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if check_point_hit(screen_x, screen_y, l1_x2, l1_y2) { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        if check_point_hit(screen_x, screen_y, l2_x1, l2_y1) { return HitTestResult::ControlPoint(ControlPointType::Point3); }
        if check_point_hit(screen_x, screen_y, l2_x2, l2_y2) { return HitTestResult::ControlPoint(ControlPointType::Point4); }

        let cx = (l1_x1 + l1_x2 + l2_x1 + l2_x2) / 4.0;
        let cy = (l1_y1 + l1_y2 + l2_y1 + l2_y2) / 4.0;
        if check_point_hit(screen_x, screen_y, cx, cy) { return HitTestResult::ControlPoint(ControlPointType::Move); }

        // Reach exactly as far as `render` draws — the two lines are NOT
        // parallel in general, so each extends independently along its own
        // slope to the same panel edge.
        let (e1x1, e1y1, e1x2, e1y2) = extend_segment_to_edges(l1_x1, l1_y1, l1_x2, l1_y2, self.extend_left, self.extend_right, viewport.chart_width);
        let (e2x1, e2y1, e2x2, e2y2) = extend_segment_to_edges(l2_x1, l2_y1, l2_x2, l2_y2, self.extend_left, self.extend_right, viewport.chart_width);
        if point_to_line_distance(screen_x, screen_y, e1x1, e1y1, e1x2, e1y2) < HIT_TOLERANCE { return HitTestResult::Body; }
        if point_to_line_distance(screen_x, screen_y, e2x1, e2y1, e2x2, e2y2) < HIT_TOLERANCE { return HitTestResult::Body; }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        let l1_bar1 = timestamp_ms_to_bar_f64(bars, self.line1_ts1);
        let l1_bar2 = timestamp_ms_to_bar_f64(bars, self.line1_ts2);
        let l2_bar1 = timestamp_ms_to_bar_f64(bars, self.line2_ts1);
        let l2_bar2 = timestamp_ms_to_bar_f64(bars, self.line2_ts2);
        let l1_x1 = viewport.bar_to_x_f64(l1_bar1);
        let l1_y1 = viewport.price_to_y(self.line1_price1, price_scale.price_min, price_scale.price_max);
        let l1_x2 = viewport.bar_to_x_f64(l1_bar2);
        let l1_y2 = viewport.price_to_y(self.line1_price2, price_scale.price_min, price_scale.price_max);
        let l2_x1 = viewport.bar_to_x_f64(l2_bar1);
        let l2_y1 = viewport.price_to_y(self.line2_price1, price_scale.price_min, price_scale.price_max);
        let l2_x2 = viewport.bar_to_x_f64(l2_bar2);
        let l2_y2 = viewport.price_to_y(self.line2_price2, price_scale.price_min, price_scale.price_max);

        let mut points = vec![
            ControlPoint::point1(l1_x1, l1_y1),
            ControlPoint::point2(l1_x2, l1_y2),
            ControlPoint::point3(l2_x1, l2_y1),
            ControlPoint::point4(l2_x2, l2_y2),
        ];
        let cx = (l1_x1 + l1_x2 + l2_x1 + l2_x2) / 4.0;
        let cy = (l1_y1 + l1_y2 + l2_y1 + l2_y2) / 4.0;
        let dist = ((l1_x2 - l1_x1).powi(2) + (l1_y2 - l1_y1).powi(2)).sqrt();
        if dist > CONTROL_POINT_RADIUS * 4.0 { points.push(ControlPoint::move_point(cx, cy)); }
        points
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let l1_x1 = ctx.ts_to_x_ms(self.line1_ts1);
        let l1_y1 = ctx.price_to_y(self.line1_price1);
        let l1_x2 = ctx.ts_to_x_ms(self.line1_ts2);
        let l1_y2 = ctx.price_to_y(self.line1_price2);
        let l2_x1 = ctx.ts_to_x_ms(self.line2_ts1);
        let l2_y1 = ctx.price_to_y(self.line2_price1);
        let l2_x2 = ctx.ts_to_x_ms(self.line2_ts2);
        let l2_y2 = ctx.price_to_y(self.line2_price2);

        // An extended side draws to the panel edge in that direction — off
        // by default, so an unextended primitive draws the exact same two
        // segments as before this pass. The two lines are NOT parallel in
        // general, so each extends independently along its own slope.
        let chart_width = ctx.chart_width();
        let (e1x1, e1y1, e1x2, e1y2) = extend_segment_to_edges(l1_x1, l1_y1, l1_x2, l1_y2, self.extend_left, self.extend_right, chart_width);
        let (e2x1, e2y1, e2x2, e2y2) = extend_segment_to_edges(l2_x1, l2_y1, l2_x2, l2_y2, self.extend_left, self.extend_right, chart_width);

        if self.fill {
            let base = self.data.color.fill.as_deref().unwrap_or(&self.data.color.stroke);
            let fill_color = format!("{}{:02x}", &base[..7], (self.fill_opacity * 255.0) as u8);
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.move_to(e1x1, e1y1); ctx.line_to(e1x2, e1y2);
            ctx.line_to(e2x2, e2y2); ctx.line_to(e2x1, e2y1);
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
        ctx.move_to(crisp(e1x1, dpr), crisp(e1y1, dpr));
        ctx.line_to(crisp(e1x2, dpr), crisp(e1y2, dpr));
        ctx.stroke();

        ctx.begin_path();
        ctx.move_to(crisp(e2x1, dpr), crisp(e2y1, dpr));
        ctx.line_to(crisp(e2x2, dpr), crisp(e2y2, dpr));
        ctx.stroke();
        ctx.set_line_dash(&[]);

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let mid_y1 = (l1_y1 + l2_y1) / 2.0;
                let mid_y2 = (l1_y2 + l2_y2) / 2.0;
                let mid_x1 = (l1_x1 + l2_x1) / 2.0;
                let mid_x2 = (l1_x2 + l2_x2) / 2.0;
                let (text_x1, text_y1, text_x2, text_y2) = match text.v_align {
                    super::super::TextAlign::Start => {
                        if l1_y1 < l2_y1 { (l1_x1, l1_y1, l1_x2, l1_y2) } else { (l2_x1, l2_y1, l2_x2, l2_y2) }
                    }
                    super::super::TextAlign::End => {
                        if l1_y1 > l2_y1 { (l1_x1, l1_y1, l1_x2, l1_y2) } else { (l2_x1, l2_y1, l2_x2, l2_y2) }
                    }
                    super::super::TextAlign::Center => (mid_x1, mid_y1, mid_x2, mid_y2),
                };
                let params = calculate_line_text_params(text_x1, text_y1, text_x2, text_y2, text);
                render_primitive_text_rotated(ctx, text, params.x, params.y, &self.data.color.stroke, params.rotation);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        // Channel archetype (W3) — same skeleton `parallel_channel.rs`'s own
        // `style_properties` doc explains in full: Outline needs no
        // declaration (universal bespoke base stroke) and Fill's default
        // `AppearanceFill` header already matches the lab's title.
        //
        // `PropertyGroup::MiddleLine` is not this type's to declare at all —
        // the lab gives that group to Parallel Channel only (see
        // `PropertyGroup::MiddleLine`'s own doc); a disjoint (non-parallel)
        // channel has no single trend line a "middle" would even average.
        //
        // `PropertyGroup::Extend` — `extend_left`/`extend_right` now really
        // extend both (independently sloped) lines to the panel edge
        // (`render`/`hit_test` above, via `extend_segment_to_edges`), so
        // this is a real declared toggle, matching `parallel_channel.rs`.
        let default_text = super::super::PrimitiveText::default();
        vec![
            ConfigProperty::fill(self.fill).with_category(PropertyCategory::AppearanceFill).with_order(20),
            ConfigProperty::fill_opacity(self.fill_opacity * 100.0).with_category(PropertyCategory::AppearanceFill).with_order(21),
            ConfigProperty::extend_left(self.extend_left).with_category(PropertyCategory::AppearanceLine).with_order(22).in_group(PropertyGroup::Extend),
            ConfigProperty::extend_right(self.extend_right).with_category(PropertyCategory::AppearanceLine).with_order(23).in_group(PropertyGroup::Extend),
            // `PropertyGroup::TextOnShape` — font size + horizontal
            // position write into `data.text`, already read by `render`
            // above (`render_primitive_text_rotated`) to place the caption.
            ConfigProperty::label_font_size(self.data.text.as_ref().map(|t| t.font_size).unwrap_or(default_text.font_size))
                .with_category(PropertyCategory::AppearanceLine).with_order(24).in_group(PropertyGroup::TextOnShape),
            ConfigProperty::h_align(self.data.text.as_ref().map(|t| t.h_align.as_str()).unwrap_or(default_text.h_align.as_str()))
                .with_category(PropertyCategory::AppearanceLine).with_order(25).in_group(PropertyGroup::TextOnShape),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "fill" => { if let Some(v) = value.as_bool() { self.fill = v; return true; } }
            "fill_opacity" => { if let Some(v) = value.as_number() { self.fill_opacity = (v / 100.0).clamp(0.0, 1.0); return true; } }
            "extend_left" => { if let Some(v) = value.as_bool() { self.extend_left = v; return true; } }
            "extend_right" => { if let Some(v) = value.as_bool() { self.extend_right = v; return true; } }
            "label_font_size" => {
                if let Some(v) = value.as_number() {
                    self.data.text.get_or_insert_with(super::super::PrimitiveText::default).font_size = v.clamp(8.0, 32.0);
                    return true;
                }
            }
            "text_h_align" => {
                if let PropertyValue::String(v) = value {
                    self.data.text.get_or_insert_with(super::super::PrimitiveText::default).h_align = super::super::TextAlign::from_str(v);
                    return true;
                }
            }
            _ => {}
        }
        false
    }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2)
}

fn create_disjoint_channel(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts1, price1) = points.first().copied().unwrap_or((0, 100.0));
    let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, price1 * 1.05));

    if points.len() >= 4 {
        Box::new(DisjointChannel::with_points(
            ts1, price1, ts2, price2,
            points[2].0, points[2].1, points[3].0, points[3].1,
            color,
        ))
    } else if points.len() >= 3 {
        let offset = points[2].1 - price1;
        Box::new(DisjointChannel::new(ts1, price1, ts2, price2, offset, color))
    } else {
        let offset = price1 * 0.05;
        Box::new(DisjointChannel::new(ts1, price1, ts2, price2, offset, color))
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "disjoint_channel",
        display_name: "Disjoint Channel",
        kind: PrimitiveKind::Channel,
        click_behavior: ClickBehavior::FourPoint,
        tooltip: "Draw a non-parallel (widening/narrowing) channel",
        icon: "disjoint_channel",
        default_color: "#4CAF50",
        factory: create_disjoint_channel,
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

    /// The two lines are independently sloped (`with_points`, not the
    /// shared-slope `new`), so `extend_left` must extend each one along its
    /// OWN slope — a probe on line 2's extended left reach misses when
    /// unextended and resolves to `Body` once `extend_left` is on.
    #[test]
    fn extend_left_reaches_the_panel_edge_on_the_second_line() {
        let (bars, viewport, price_scale) = fixture();
        // ts values land exactly on bar indices of `fixture()`'s 6-bar
        // slice, so every anchor sits well inside `chart_width` at the
        // fixture's default 8px bar spacing. Line 2 starts at bar index 2
        // (not 0), so its own left endpoint is NOT already at the panel's
        // left edge — the probe below is only reachable via extension, not
        // because line 2 already starts there on its own.
        let channel = DisjointChannel::with_points(
            0, 100.0, 5_000, 105.0,
            2_000, 90.0, 5_000, 120.0,
            "#4CAF50",
        );

        let l2_bar1 = timestamp_ms_to_bar_f64(&bars, channel.line2_ts1);
        let l2_bar2 = timestamp_ms_to_bar_f64(&bars, channel.line2_ts2);
        let l2x1 = viewport.bar_to_x_f64(l2_bar1);
        let l2y1 = viewport.price_to_y(channel.line2_price1, price_scale.price_min, price_scale.price_max);
        let l2x2 = viewport.bar_to_x_f64(l2_bar2);
        let l2y2 = viewport.price_to_y(channel.line2_price2, price_scale.price_min, price_scale.price_max);
        let (probe_x, probe_y, _, _) = extend_segment_to_edges(l2x1, l2y1, l2x2, l2y2, true, false, viewport.chart_width);
        assert_eq!(probe_x, 0.0, "the probe sits at the panel's own left edge");

        assert!(
            matches!(channel.hit_test(probe_x, probe_y, &bars, &viewport, &price_scale), HitTestResult::Miss),
            "unextended channel misses a point on the panel's left edge",
        );

        let mut extended = channel;
        extended.extend_left = true;
        assert!(
            matches!(extended.hit_test(probe_x, probe_y, &bars, &viewport, &price_scale), HitTestResult::Body),
            "extend_left reaches the same point once line 2 is drawn that far",
        );
    }
}
