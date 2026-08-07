//! Parallel Channel primitive
//!
//! Two parallel trend lines forming a channel. Created with 3 clicks:
//! - First two clicks define the main trend line
//! - Third click defines the width of the channel

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    point_to_line_distance, HIT_TOLERANCE, CONTROL_POINT_RADIUS, CONTROL_POINT_HIT_RADIUS,
    RenderContext, crisp,
    render_primitive_text_rotated, calculate_line_text_params,
    config::{ConfigProperty, PropertyValue},
    AREA_FILL_OPACITY, default_area_fill_opacity,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParallelChannel {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64,
    pub price1: f64,
    #[serde(default)]
    pub ts2: i64,
    pub price2: f64,
    pub channel_offset: f64,
    #[serde(default)]
    pub extend_left: bool,
    #[serde(default)]
    pub extend_right: bool,
    #[serde(default = "default_true")]
    pub fill: bool,
    #[serde(default = "default_area_fill_opacity")]
    pub fill_opacity: f64,
}

fn default_true() -> bool { true }

impl ParallelChannel {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, channel_offset: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "parallel_channel".to_string(),
                display_name: "Parallel Channel".to_string(),
                color: PrimitiveColor::with_fill(color, color),
                width: 2.0,
                ..Default::default()
            },
            ts1, price1, ts2, price2,
            channel_offset,
            extend_left: false,
            extend_right: false,
            fill: true,
            fill_opacity: AREA_FILL_OPACITY,
        }
    }
}

impl Primitive for ParallelChannel {
    fn type_id(&self) -> &'static str { "parallel_channel" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Channel }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::ThreePoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> {
        vec![
            (self.ts1, self.price1),
            (self.ts2, self.price2),
            (self.ts1, self.price1 + self.channel_offset),
        ]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if points.len() >= 2 {
            self.ts1 = points[0].0; self.price1 = points[0].1;
            self.ts2 = points[1].0; self.price2 = points[1].1;
        }
        if points.len() >= 3 {
            self.channel_offset = points[2].1 - self.price1;
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
            ControlPointType::Point3 => { self.channel_offset = price - self.price1; }
            ControlPointType::Point4 => { self.channel_offset = price - self.price2; }
            ControlPointType::Move => {
                let mid_ts = (self.ts1 + self.ts2) / 2;
                let mid_price = (self.price1 + self.price2) / 2.0 + self.channel_offset / 2.0;
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
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);
        let py1 = viewport.price_to_y(self.price1 + self.channel_offset, price_scale.price_min, price_scale.price_max);
        let py2 = viewport.price_to_y(self.price2 + self.channel_offset, price_scale.price_min, price_scale.price_max);

        if check_point_hit(screen_x, screen_y, x1, y1) { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if check_point_hit(screen_x, screen_y, x2, y2) { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        if check_point_hit(screen_x, screen_y, x1, py1) { return HitTestResult::ControlPoint(ControlPointType::Point3); }
        if check_point_hit(screen_x, screen_y, x2, py2) { return HitTestResult::ControlPoint(ControlPointType::Point4); }

        let cx = (x1 + x2) / 2.0;
        let cy = (y1 + y2 + py1 + py2) / 4.0;
        if check_point_hit(screen_x, screen_y, cx, cy) { return HitTestResult::ControlPoint(ControlPointType::Move); }

        if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE { return HitTestResult::Body; }
        if point_to_line_distance(screen_x, screen_y, x1, py1, x2, py2) < HIT_TOLERANCE { return HitTestResult::Body; }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);
        let py1 = viewport.price_to_y(self.price1 + self.channel_offset, price_scale.price_min, price_scale.price_max);
        let py2 = viewport.price_to_y(self.price2 + self.channel_offset, price_scale.price_min, price_scale.price_max);

        let mut points = vec![
            ControlPoint::point1(x1, y1),
            ControlPoint::point2(x2, y2),
            ControlPoint::point3(x1, py1),
            ControlPoint::point4(x2, py2),
        ];
        let cx = (x1 + x2) / 2.0;
        let cy = (y1 + y2 + py1 + py2) / 4.0;
        let dist = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
        if dist > CONTROL_POINT_RADIUS * 4.0 {
            points.push(ControlPoint::move_point(cx, cy));
        }
        points
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y2 = ctx.price_to_y(self.price2);
        let py1 = ctx.price_to_y(self.price1 + self.channel_offset);
        let py2 = ctx.price_to_y(self.price2 + self.channel_offset);

        if self.fill {
            let base = self.data.color.fill.as_deref().unwrap_or(&self.data.color.stroke);
            let fill_color = format!("{}{:02x}", &base[..7], (self.fill_opacity * 255.0) as u8);
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.move_to(x1, y1); ctx.line_to(x2, y2);
            ctx.line_to(x2, py2); ctx.line_to(x1, py1);
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
        ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
        ctx.stroke();

        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(py1, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(py2, dpr));
        ctx.stroke();
        ctx.set_line_dash(&[]);

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let (text_y1, text_y2) = match text.v_align {
                    super::super::TextAlign::Start => (y1.min(py1), y2.min(py2)),
                    super::super::TextAlign::End => (y1.max(py1), y2.max(py2)),
                    super::super::TextAlign::Center => ((y1 + py1) / 2.0, (y2 + py2) / 2.0),
                };
                let params = calculate_line_text_params(x1, text_y1, x2, text_y2, text);
                render_primitive_text_rotated(ctx, text, params.x, params.y, &self.data.color.stroke, params.rotation);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::fill(self.fill).with_order(20),
            ConfigProperty::fill_opacity(self.fill_opacity * 100.0).with_order(21),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "fill" => { if let Some(v) = value.as_bool() { self.fill = v; return true; } }
            "fill_opacity" => { if let Some(v) = value.as_number() { self.fill_opacity = (v / 100.0).clamp(0.0, 1.0); return true; } }
            _ => {}
        }
        false
    }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2)
}

fn create_parallel_channel(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
    let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, price1));
    let channel_offset = if points.len() >= 3 {
        points[2].1 - price1
    } else {
        price1 * 0.05
    };
    Box::new(ParallelChannel::new(ts1, price1, ts2, price2, channel_offset, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "parallel_channel",
        display_name: "Parallel Channel",
        kind: PrimitiveKind::Channel,
        click_behavior: ClickBehavior::ThreePoint,
        tooltip: "Draw a channel with two parallel trend lines",
        icon: "parallel_channel",
        default_color: "#2196F3",
        factory: create_parallel_channel,
        supports_text: true,
        has_levels: false,
    }
}
