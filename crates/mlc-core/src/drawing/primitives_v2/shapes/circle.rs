//! Circle primitive

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType, ControlPointCursor,
    PrimitiveColor, LineStyle,
    HIT_TOLERANCE, CONTROL_POINT_HIT_RADIUS, CONTROL_POINT_RADIUS,
    RenderContext, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

/// Circle — center in (timestamp_ms, price), radius_ms = horizontal radius as duration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Circle {
    pub data: PrimitiveData,
    pub center_ts: i64,
    pub center_price: f64,
    /// Horizontal radius as a millisecond duration
    pub radius_ms: i64,
    /// Vertical radius in price units
    pub radius_price: f64,
    #[serde(default = "default_true")] pub fill: bool,
    #[serde(default = "default_fill_opacity")] pub fill_opacity: f64,
}

fn default_true() -> bool { true }
fn default_fill_opacity() -> f64 { 0.2 }

impl Circle {
    pub fn new(center_ts: i64, center_price: f64, radius_ms: i64, radius_price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "circle".to_string(),
                display_name: "Circle".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            center_ts, center_price,
            radius_ms: radius_ms.max(1),
            radius_price: radius_price.abs().max(f64::EPSILON),
            fill: true, fill_opacity: 0.2,
        }
    }
}

impl Primitive for Circle {
    fn type_id(&self) -> &'static str { "circle" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Shape }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> {
        vec![
            (self.center_ts, self.center_price),
            (self.center_ts + self.radius_ms, self.center_price + self.radius_price),
        ]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if !points.is_empty() {
            self.center_ts = points[0].0;
            self.center_price = points[0].1;
        }
        if points.len() >= 2 {
            self.radius_ms = (points[1].0 - self.center_ts).abs().max(1);
            self.radius_price = (points[1].1 - self.center_price).abs().max(f64::EPSILON);
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        self.center_ts += ts_delta_ms;
        self.center_price += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        match point_type {
            ControlPointType::Move => {
                self.center_ts = ts_ms;
                self.center_price = price;
            }
            ControlPointType::Edge(0) | ControlPointType::Edge(2) => {
                self.radius_price = (price - self.center_price).abs().max(f64::EPSILON);
            }
            ControlPointType::Edge(1) | ControlPointType::Edge(3) => {
                self.radius_ms = (ts_ms - self.center_ts).abs().max(1);
            }
            _ => {}
        }
    }

    fn hit_test(&self, screen_x: f64, screen_y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> HitTestResult {
        let center_bar = timestamp_ms_to_bar_f64(bars, self.center_ts);
        let edge_bar = timestamp_ms_to_bar_f64(bars, self.center_ts + self.radius_ms);
        let cx = viewport.bar_to_x_f64(center_bar);
        let cy = viewport.price_to_y(self.center_price, price_scale.price_min, price_scale.price_max);
        let rx = (viewport.bar_to_x_f64(edge_bar) - cx).abs();
        let ry = (viewport.price_to_y(self.center_price + self.radius_price, price_scale.price_min, price_scale.price_max) - cy).abs();

        let edges = [(cx, cy - ry, 0u8), (cx + rx, cy, 1), (cx, cy + ry, 2), (cx - rx, cy, 3)];
        for (ex, ey, idx) in edges {
            if check_point_hit(screen_x, screen_y, ex, ey) {
                return HitTestResult::ControlPoint(ControlPointType::Edge(idx));
            }
        }
        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }

        if rx < 0.001 || ry < 0.001 { return HitTestResult::Miss; }
        let nx = (screen_x - cx) / rx;
        let ny = (screen_y - cy) / ry;
        let dist_sq = nx * nx + ny * ny;

        if (dist_sq.sqrt() - 1.0).abs() < HIT_TOLERANCE / rx.min(ry) { return HitTestResult::Body; }
        if self.fill && dist_sq <= 1.0 { return HitTestResult::Body; }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        let center_bar = timestamp_ms_to_bar_f64(bars, self.center_ts);
        let edge_bar = timestamp_ms_to_bar_f64(bars, self.center_ts + self.radius_ms);
        let cx = viewport.bar_to_x_f64(center_bar);
        let cy = viewport.price_to_y(self.center_price, price_scale.price_min, price_scale.price_max);
        let rx = (viewport.bar_to_x_f64(edge_bar) - cx).abs();
        let ry = (viewport.price_to_y(self.center_price + self.radius_price, price_scale.price_min, price_scale.price_max) - cy).abs();

        vec![
            ControlPoint::move_point(cx, cy),
            ControlPoint::new(ControlPointType::Edge(0), cx, cy - ry, ControlPointCursor::ResizeNS),
            ControlPoint::new(ControlPointType::Edge(1), cx + rx, cy, ControlPointCursor::ResizeEW),
            ControlPoint::new(ControlPointType::Edge(2), cx, cy + ry, ControlPointCursor::ResizeNS),
            ControlPoint::new(ControlPointType::Edge(3), cx - rx, cy, ControlPointCursor::ResizeEW),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let cx = ctx.ts_to_x_ms(self.center_ts);
        let cy = ctx.price_to_y(self.center_price);
        let rx = (ctx.ts_to_x_ms(self.center_ts + self.radius_ms) - cx).abs();
        let ry = (ctx.price_to_y(self.center_price + self.radius_price) - cy).abs();

        if self.fill {
            let fill_color = format!("{}{:02x}", &self.data.color.stroke[..7], (self.fill_opacity * 255.0) as u8);
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.ellipse(cx, cy, rx, ry, 0.0, 0.0, std::f64::consts::TAU);
            ctx.fill();
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
        ctx.ellipse(cx, cy, rx, ry, 0.0, 0.0, std::f64::consts::TAU);
        ctx.stroke();
        ctx.set_line_dash(&[]);

        if is_selected {
            ctx.set_stroke_color("#2196F3");
            ctx.set_stroke_width(1.5);
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.begin_path();
            ctx.move_to(cx - rx, cy - ry); ctx.line_to(cx + rx, cy - ry);
            ctx.line_to(cx + rx, cy + ry); ctx.line_to(cx - rx, cy + ry);
            ctx.close_path(); ctx.stroke();
            ctx.set_line_dash(&[]);

            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (ex, ey) in [(cx, cy - ry), (cx + rx, cy), (cx, cy + ry), (cx - rx, cy)] {
                ctx.begin_path();
                ctx.arc(ex, ey, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill(); ctx.stroke();
            }
            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill(); ctx.stroke();
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => cx - rx,
                    super::super::TextAlign::Center => cx,
                    super::super::TextAlign::End => cx + rx,
                };
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => cy - ry - text_offset,
                    super::super::TextAlign::Center => cy,
                    super::super::TextAlign::End => cy + ry + text_offset,
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

fn create_circle(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (center_ts, center_price) = points.first().copied().unwrap_or((0, 0.0));
    if points.len() >= 2 {
        let radius_ms = (points[1].0 - center_ts).abs().max(1);
        let radius_price = (points[1].1 - center_price).abs().max(f64::EPSILON);
        Box::new(Circle::new(center_ts, center_price, radius_ms, radius_price, color))
    } else {
        Box::new(Circle::new(center_ts, center_price, 600_000, center_price.abs() * 0.02 + 1.0, color))
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "circle", display_name: "Circle", kind: PrimitiveKind::Shape,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Draw a circle from center to edge",
        icon: "circle", default_color: "#4CAF50",
        factory: create_circle,
        supports_text: true, has_levels: false, has_points_config: false,
    }
}
