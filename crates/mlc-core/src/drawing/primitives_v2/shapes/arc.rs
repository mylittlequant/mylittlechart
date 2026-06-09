//! Arc primitive

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    HIT_TOLERANCE, CONTROL_POINT_HIT_RADIUS, CONTROL_POINT_RADIUS,
    RenderContext, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Arc {
    pub data: PrimitiveData,
    pub center_ts: i64,
    pub center_price: f64,
    /// Horizontal radius as ms duration
    pub radius_ms: i64,
    pub start_angle: f64,
    pub end_angle: f64,
}

impl Arc {
    pub fn new(center_ts: i64, center_price: f64, radius_ms: i64, start_angle: f64, end_angle: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "arc".to_string(),
                display_name: "Arc".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            center_ts, center_price, radius_ms: radius_ms.max(1), start_angle, end_angle,
        }
    }
}

impl Primitive for Arc {
    fn type_id(&self) -> &'static str { "arc" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Shape }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::ThreePoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> {
        let start_rad = self.start_angle.to_radians();
        let end_rad = self.end_angle.to_radians();
        vec![
            (self.center_ts, self.center_price),
            (self.center_ts + (self.radius_ms as f64 * start_rad.cos()) as i64, self.center_price),
            (self.center_ts + (self.radius_ms as f64 * end_rad.cos()) as i64, self.center_price),
        ]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if !points.is_empty() {
            self.center_ts = points[0].0;
            self.center_price = points[0].1;
        }
        if points.len() >= 2 {
            self.radius_ms = (points[1].0 - self.center_ts).abs().max(1);
            self.start_angle = 0.0;
        }
        if points.len() >= 3 {
            let dx = (points[2].0 - self.center_ts) as f64;
            let dy = points[2].1 - self.center_price;
            let mut angle = dy.atan2(dx).to_degrees();
            if angle < 0.0 { angle += 360.0; }
            self.end_angle = angle;
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        self.center_ts += ts_delta_ms;
        self.center_price += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        match point_type {
            ControlPointType::Point1 | ControlPointType::Move => {
                self.center_ts = ts_ms;
                self.center_price = price;
            }
            ControlPointType::Point2 => {
                self.radius_ms = (ts_ms - self.center_ts).abs().max(1);
            }
            ControlPointType::Point3 => {
                let dx = (ts_ms - self.center_ts) as f64;
                let dy = price - self.center_price;
                let mut angle = dy.atan2(dx).to_degrees();
                if angle < 0.0 { angle += 360.0; }
                self.end_angle = angle;
            }
            _ => {}
        }
    }

    fn hit_test(&self, screen_x: f64, screen_y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> HitTestResult {
        let center_bar = timestamp_ms_to_bar_f64(bars, self.center_ts);
        let edge_bar = timestamp_ms_to_bar_f64(bars, self.center_ts + self.radius_ms);
        let cx = viewport.bar_to_x_f64(center_bar);
        let cy = viewport.price_to_y(self.center_price, price_scale.price_min, price_scale.price_max);
        let radius = (viewport.bar_to_x_f64(edge_bar) - cx).abs();

        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }

        let dx = screen_x - cx;
        let dy = screen_y - cy;
        let dist = (dx * dx + dy * dy).sqrt();
        let angle = dy.atan2(dx).to_degrees();
        let angle_norm = if angle < 0.0 { angle + 360.0 } else { angle };

        let on_arc = (dist - radius).abs() < HIT_TOLERANCE;
        let in_angle_range = if self.start_angle <= self.end_angle {
            angle_norm >= self.start_angle && angle_norm <= self.end_angle
        } else {
            angle_norm >= self.start_angle || angle_norm <= self.end_angle
        };

        if on_arc && in_angle_range { return HitTestResult::Body; }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        let center_bar = timestamp_ms_to_bar_f64(bars, self.center_ts);
        let edge_bar = timestamp_ms_to_bar_f64(bars, self.center_ts + self.radius_ms);
        let cx = viewport.bar_to_x_f64(center_bar);
        let cy = viewport.price_to_y(self.center_price, price_scale.price_min, price_scale.price_max);
        let radius = (viewport.bar_to_x_f64(edge_bar) - cx).abs();

        let start_rad = self.start_angle.to_radians();
        let end_rad = self.end_angle.to_radians();

        vec![
            ControlPoint::move_point(cx, cy),
            ControlPoint::point2(cx + radius * start_rad.cos(), cy + radius * start_rad.sin()),
            ControlPoint::point3(cx + radius * end_rad.cos(), cy + radius * end_rad.sin()),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let cx = ctx.ts_to_x_ms(self.center_ts);
        let cy = ctx.price_to_y(self.center_price);
        let rx = ctx.ts_to_x_ms(self.center_ts + self.radius_ms);
        let radius = (rx - cx).abs();

        let start_rad = self.start_angle.to_radians();
        let end_rad = self.end_angle.to_radians();

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
        ctx.arc(cx, cy, radius, start_rad, end_rad);
        ctx.stroke();
        ctx.set_line_dash(&[]);

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill(); ctx.stroke();

            let start_x = cx + radius * start_rad.cos();
            let start_y = cy + radius * start_rad.sin();
            ctx.begin_path();
            ctx.arc(start_x, start_y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill(); ctx.stroke();

            let end_x = cx + radius * end_rad.cos();
            let end_y = cy + radius * end_rad.sin();
            ctx.begin_path();
            ctx.arc(end_x, end_y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill(); ctx.stroke();
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => cx - radius,
                    super::super::TextAlign::Center => cx,
                    super::super::TextAlign::End => cx + radius,
                };
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => cy - radius - text_offset,
                    super::super::TextAlign::Center => cy,
                    super::super::TextAlign::End => cy + radius + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2)
}

fn create_arc(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (center_ts, center_price) = points.first().copied().unwrap_or((0, 100.0));
    let radius_ms = if points.len() >= 2 {
        (points[1].0 - center_ts).abs().max(1)
    } else {
        600_000
    };
    let end_angle = if points.len() >= 3 {
        let dx = (points[2].0 - center_ts) as f64;
        let dy = points[2].1 - center_price;
        let mut angle = dy.atan2(dx).to_degrees();
        if angle < 0.0 { angle += 360.0; }
        angle
    } else {
        180.0
    };
    Box::new(Arc::new(center_ts, center_price, radius_ms, 0.0, end_angle, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "arc", display_name: "Arc", kind: PrimitiveKind::Shape,
        click_behavior: ClickBehavior::ThreePoint,
        tooltip: "Draw an arc (center, start, end)",
        icon: "arc", default_color: "#E91E63",
        factory: create_arc,
        supports_text: true, has_levels: false, has_points_config: false,
    }
}
