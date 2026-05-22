//! Triangle primitive

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    point_to_line_distance, HIT_TOLERANCE, CONTROL_POINT_HIT_RADIUS, CONTROL_POINT_RADIUS,
    RenderContext, crisp, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Triangle {
    pub data: PrimitiveData,
    pub ts1: i64, pub price1: f64,
    pub ts2: i64, pub price2: f64,
    pub ts3: i64, pub price3: f64,
    #[serde(default = "default_true")] pub fill: bool,
    #[serde(default = "default_fill_opacity")] pub fill_opacity: f64,
}

fn default_true() -> bool { true }
fn default_fill_opacity() -> f64 { 0.2 }

impl Triangle {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, ts3: i64, price3: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "triangle".to_string(),
                display_name: "Triangle".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            ts1, price1, ts2, price2, ts3, price3,
            fill: true, fill_opacity: 0.2,
        }
    }
}

impl Primitive for Triangle {
    fn type_id(&self) -> &'static str { "triangle" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Shape }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::ThreePoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> {
        vec![(self.ts1, self.price1), (self.ts2, self.price2), (self.ts3, self.price3)]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if !points.is_empty() { self.ts1 = points[0].0; self.price1 = points[0].1; }
        if points.len() >= 2 { self.ts2 = points[1].0; self.price2 = points[1].1; }
        if points.len() >= 3 { self.ts3 = points[2].0; self.price3 = points[2].1; }
    }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        self.ts1 += ts_delta_ms; self.ts2 += ts_delta_ms; self.ts3 += ts_delta_ms;
        self.price1 += price_delta; self.price2 += price_delta; self.price3 += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        match point_type {
            ControlPointType::Point1 => { self.ts1 = ts_ms; self.price1 = price; }
            ControlPointType::Point2 => { self.ts2 = ts_ms; self.price2 = price; }
            ControlPointType::Point3 => { self.ts3 = ts_ms; self.price3 = price; }
            _ => {}
        }
    }

    fn hit_test(&self, screen_x: f64, screen_y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> HitTestResult {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let bar3 = timestamp_ms_to_bar_f64(bars, self.ts3);
        let x1 = viewport.bar_to_x_f64(bar1); let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(bar2); let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);
        let x3 = viewport.bar_to_x_f64(bar3); let y3 = viewport.price_to_y(self.price3, price_scale.price_min, price_scale.price_max);

        if check_point_hit(screen_x, screen_y, x1, y1) { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if check_point_hit(screen_x, screen_y, x2, y2) { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        if check_point_hit(screen_x, screen_y, x3, y3) { return HitTestResult::ControlPoint(ControlPointType::Point3); }
        if check_point_hit(screen_x, screen_y, (x1 + x2 + x3) / 3.0, (y1 + y2 + y3) / 3.0) {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }

        if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE { return HitTestResult::Body; }
        if point_to_line_distance(screen_x, screen_y, x2, y2, x3, y3) < HIT_TOLERANCE { return HitTestResult::Body; }
        if point_to_line_distance(screen_x, screen_y, x3, y3, x1, y1) < HIT_TOLERANCE { return HitTestResult::Body; }

        if self.fill && point_in_triangle(screen_x, screen_y, x1, y1, x2, y2, x3, y3) { return HitTestResult::Body; }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let bar3 = timestamp_ms_to_bar_f64(bars, self.ts3);
        let x1 = viewport.bar_to_x_f64(bar1); let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(bar2); let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);
        let x3 = viewport.bar_to_x_f64(bar3); let y3 = viewport.price_to_y(self.price3, price_scale.price_min, price_scale.price_max);

        vec![
            ControlPoint::point1(x1, y1),
            ControlPoint::point2(x2, y2),
            ControlPoint::point3(x3, y3),
            ControlPoint::move_point((x1 + x2 + x3) / 3.0, (y1 + y2 + y3) / 3.0),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1); let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2); let y2 = ctx.price_to_y(self.price2);
        let x3 = ctx.ts_to_x_ms(self.ts3); let y3 = ctx.price_to_y(self.price3);

        if self.fill {
            let fill_color = format!("{}{:02x}", &self.data.color.stroke[..7], (self.fill_opacity * 255.0) as u8);
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.move_to(x1, y1); ctx.line_to(x2, y2); ctx.line_to(x3, y3);
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
        ctx.line_to(crisp(x3, dpr), crisp(y3, dpr));
        ctx.close_path();
        ctx.stroke();
        ctx.set_line_dash(&[]);

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (px, py) in [(x1, y1), (x2, y2), (x3, y3)] {
                ctx.begin_path();
                ctx.arc(px, py, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill(); ctx.stroke();
            }
            let cx = (x1 + x2 + x3) / 3.0; let cy = (y1 + y2 + y3) / 3.0;
            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill(); ctx.stroke();
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let min_x = x1.min(x2).min(x3);
                let max_x = x1.max(x2).max(x3);
                let min_y = y1.min(y2).min(y3);
                let max_y = y1.max(y2).max(y3);
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => (x1 + x2 + x3) / 3.0,
                    super::super::TextAlign::End => max_x,
                };
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => min_y - text_offset,
                    super::super::TextAlign::Center => (y1 + y2 + y3) / 3.0,
                    super::super::TextAlign::End => max_y + text_offset,
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

fn point_in_triangle(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) -> bool {
    let denom = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3);
    if denom.abs() < 0.0001 { return false; }
    let a = ((y2 - y3) * (px - x3) + (x3 - x2) * (py - y3)) / denom;
    let b = ((y3 - y1) * (px - x3) + (x1 - x3) * (py - y3)) / denom;
    let c = 1.0 - a - b;
    (0.0..=1.0).contains(&a) && (0.0..=1.0).contains(&b) && (0.0..=1.0).contains(&c)
}

fn create_triangle(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts1, price1) = points.first().copied().unwrap_or((0, 100.0));
    let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 600_000, price1 * 1.05));
    let (ts3, price3) = points.get(2).copied().unwrap_or((ts1 + 300_000, price1 * 0.95));
    Box::new(Triangle::new(ts1, price1, ts2, price2, ts3, price3, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "triangle", display_name: "Triangle", kind: PrimitiveKind::Shape,
        click_behavior: ClickBehavior::ThreePoint,
        tooltip: "Draw a triangle with three points",
        icon: "triangle", default_color: "#FF9800",
        factory: create_triangle,
        supports_text: true, has_levels: false, has_points_config: false,
    }
}
