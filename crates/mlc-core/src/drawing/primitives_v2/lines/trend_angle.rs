//! Trend Angle primitive
//!
//! A trend line that displays the angle of the line in degrees.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    point_to_line_distance, HIT_TOLERANCE, CONTROL_POINT_RADIUS, CONTROL_POINT_HIT_RADIUS,
    RenderContext, crisp, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text_rotated, calculate_line_text_params,
};

/// Trend Angle - trend line with angle measurement
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrendAngle {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64,
    pub price1: f64,
    #[serde(default)]
    pub ts2: i64,
    pub price2: f64,
    #[serde(default = "default_true")]
    pub show_arc: bool,
    #[serde(default = "default_true")]
    pub show_label: bool,
}

fn default_true() -> bool { true }

impl TrendAngle {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "trend_angle".to_string(),
                display_name: "Trend Angle".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            ts1, price1, ts2, price2, show_arc: true, show_label: true,
        }
    }
}

impl Primitive for TrendAngle {
    fn type_id(&self) -> &'static str { "trend_angle" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Line }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> {
        vec![(self.ts1, self.price1), (self.ts2, self.price2)]
    }

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

        if check_point_hit(screen_x, screen_y, x1, y1) { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if check_point_hit(screen_x, screen_y, x2, y2) { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        let cx = (x1 + x2) / 2.0; let cy = (y1 + y2) / 2.0;
        if check_point_hit(screen_x, screen_y, cx, cy) { return HitTestResult::ControlPoint(ControlPointType::Move); }
        if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE { return HitTestResult::Body; }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        let mut points = vec![ControlPoint::point1(x1, y1), ControlPoint::point2(x2, y2)];
        let cx = (x1 + x2) / 2.0; let cy = (y1 + y2) / 2.0;
        let dist = ((cx - x1).powi(2) + (cy - y1).powi(2)).sqrt();
        if dist > CONTROL_POINT_RADIUS * 4.0 { points.push(ControlPoint::move_point(cx, cy)); }
        points
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y2 = ctx.price_to_y(self.price2);

        let text_params = self.data.text.as_ref()
            .filter(|t| !t.content.is_empty())
            .map(|text| calculate_line_text_params(x1, y1, x2, y2, text));

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        let needs_gap = self.data.text.as_ref()
            .map(|t| !t.content.is_empty() && matches!(t.v_align, super::super::TextAlign::Center))
            .unwrap_or(false);

        ctx.begin_path();
        if needs_gap {
            let text = self.data.text.as_ref().unwrap();
            let dx = x2 - x1; let dy = y2 - y1;
            let line_len = (dx * dx + dy * dy).sqrt();
            if line_len > 0.001 {
                let t_center = match text.h_align {
                    super::super::TextAlign::Start => 0.0,
                    super::super::TextAlign::Center => 0.5,
                    super::super::TextAlign::End => 1.0,
                };
                let char_count = text.content.len() as f64;
                let text_width = char_count * text.font_size * 0.6 + 8.0;
                let half_gap_t = (text_width / 2.0) / line_len;
                let t_start = (t_center - half_gap_t).max(0.0);
                let t_end = (t_center + half_gap_t).min(1.0);
                if t_start > 0.001 {
                    ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
                    ctx.line_to(crisp(x1 + dx * t_start, dpr), crisp(y1 + dy * t_start, dpr));
                }
                if t_end < 0.999 {
                    ctx.move_to(crisp(x1 + dx * t_end, dpr), crisp(y1 + dy * t_end, dpr));
                    ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
                }
            }
        } else {
            ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
            ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
        }
        ctx.stroke();
        ctx.set_line_dash(&[]);

        let dx = x2 - x1; let dy = y1 - y2;
        let angle_rad = if dx.abs() > 0.001 { (dy / dx).atan() } else if dy >= 0.0 { std::f64::consts::FRAC_PI_2 } else { -std::f64::consts::FRAC_PI_2 };
        let angle_deg = angle_rad.to_degrees();

        if self.show_arc {
            let line_length = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
            let arc_radius = (line_length * 0.3).clamp(20.0, 60.0);
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.begin_path();
            ctx.move_to(x1, y1);
            ctx.line_to(x1 + arc_radius * 1.5, y1);
            ctx.stroke();
            ctx.set_line_dash(&[]);
            let start_angle = 0.0; let end_angle = -angle_rad;
            ctx.begin_path();
            if end_angle >= start_angle { ctx.arc(x1, y1, arc_radius, start_angle, end_angle); }
            else { ctx.arc(x1, y1, arc_radius, end_angle, start_angle); }
            ctx.stroke();
        }

        if self.show_label {
            let angle_text = format!("{:.1}°", angle_deg);
            let label_distance = 40.0;
            let label_angle = -angle_rad / 2.0;
            let label_x = x1 + label_distance * label_angle.cos();
            let label_y = y1 - label_distance * label_angle.sin();
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(&self.data.color.stroke);
            use crate::render::{TextAlign, TextBaseline};
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(&angle_text, label_x, label_y);
        }

        if let Some(ref text) = self.data.text {
            if let Some(ref params) = text_params {
                render_primitive_text_rotated(ctx, text, params.x, params.y, &self.data.color.stroke, params.rotation);
            }
        }

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (x, y) in [(x1, y1), (x2, y2)] {
                ctx.begin_path();
                ctx.arc(x, y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill(); ctx.stroke();
            }
            let cx = (x1 + x2) / 2.0; let cy = (y1 + y2) / 2.0;
            let dist = ((cx - x1).powi(2) + (cy - y1).powi(2)).sqrt();
            if dist > CONTROL_POINT_RADIUS * 4.0 {
                ctx.begin_path();
                ctx.arc(cx, cy, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill(); ctx.stroke();
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2)
}

fn create_trend_angle(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
    let (ts2, price2) = points.get(1).copied().unwrap_or((ts1, price1));
    Box::new(TrendAngle::new(ts1, price1, ts2, price2, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "trend_angle", display_name: "Trend Angle", kind: PrimitiveKind::Line,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Draw a trend line showing the angle in degrees",
        icon: "trend_angle", default_color: "#FF9800", factory: create_trend_angle,
        supports_text: true, has_levels: false, has_points_config: false,
    }
}
