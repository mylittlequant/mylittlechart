//! Rotated Rectangle primitive

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
pub struct RotatedRectangle {
    pub data: PrimitiveData,
    pub center_ts: i64,
    pub center_price: f64,
    /// Half-width as ms duration
    pub half_width_ms: i64,
    pub half_height: f64,
    pub rotation: f64,
    #[serde(default = "default_true")] pub fill: bool,
    #[serde(default = "default_fill_opacity")] pub fill_opacity: f64,
}

fn default_true() -> bool { true }
fn default_fill_opacity() -> f64 { 0.2 }

impl RotatedRectangle {
    pub fn new(center_ts: i64, center_price: f64, half_width_ms: i64, half_height: f64, rotation: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "rotated_rectangle".to_string(),
                display_name: "Rotated Rectangle".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            center_ts, center_price,
            half_width_ms: half_width_ms.max(1),
            half_height: half_height.abs().max(f64::EPSILON),
            rotation, fill: true, fill_opacity: 0.2,
        }
    }

    /// Get corners as (ts_ms, price) pairs (conceptual data coords, before screen projection)
    fn corner_ts_prices(&self) -> [(i64, f64); 4] {
        let cos_r = self.rotation.to_radians().cos();
        let sin_r = self.rotation.to_radians().sin();
        let hw = self.half_width_ms as f64;
        let hh = self.half_height;
        let local = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)];
        let mut corners = [(0i64, 0.0f64); 4];
        for (i, (lx, ly)) in local.iter().enumerate() {
            corners[i] = (
                self.center_ts + (lx * cos_r - ly * sin_r) as i64,
                self.center_price + lx * sin_r + ly * cos_r,
            );
        }
        corners
    }
}

impl Primitive for RotatedRectangle {
    fn type_id(&self) -> &'static str { "rotated_rectangle" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Shape }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::ThreePoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> {
        let corners = self.corner_ts_prices();
        vec![
            (self.center_ts, self.center_price),
            corners[1],
            corners[2],
        ]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if !points.is_empty() {
            self.center_ts = points[0].0;
            self.center_price = points[0].1;
        }
        if points.len() >= 2 {
            let dt = (points[1].0 - self.center_ts) as f64;
            let dp = points[1].1 - self.center_price;
            self.rotation = dp.atan2(dt).to_degrees();
            self.half_width_ms = ((dt * dt + dp * dp).sqrt() as i64).max(1);
        }
        if points.len() >= 3 {
            let dt = (points[2].0 - self.center_ts) as f64;
            let dp = points[2].1 - self.center_price;
            self.half_height = (dt * dt + dp * dp).sqrt().max(f64::EPSILON);
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        self.center_ts += ts_delta_ms;
        self.center_price += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        match point_type {
            ControlPointType::Move => { self.center_ts = ts_ms; self.center_price = price; }
            ControlPointType::Point2 => {
                let dt = (ts_ms - self.center_ts) as f64;
                let dp = price - self.center_price;
                self.rotation = dp.atan2(dt).to_degrees();
                self.half_width_ms = ((dt * dt + dp * dp).sqrt() as i64).max(1);
            }
            ControlPointType::Point3 => {
                let dt = (ts_ms - self.center_ts) as f64;
                let dp = price - self.center_price;
                self.half_height = (dt * dt + dp * dp).sqrt().max(f64::EPSILON);
            }
            _ => {}
        }
    }

    fn hit_test(&self, screen_x: f64, screen_y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> HitTestResult {
        let corners = self.corner_ts_prices();
        let screen_corners: Vec<(f64, f64)> = corners.iter()
            .map(|(ts, p)| {
                let bar = timestamp_ms_to_bar_f64(bars, *ts);
                (viewport.bar_to_x_f64(bar), viewport.price_to_y(*p, price_scale.price_min, price_scale.price_max))
            })
            .collect();

        let center_bar = timestamp_ms_to_bar_f64(bars, self.center_ts);
        let cx = viewport.bar_to_x_f64(center_bar);
        let cy = viewport.price_to_y(self.center_price, price_scale.price_min, price_scale.price_max);

        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }
        for (i, (sx, sy)) in screen_corners.iter().enumerate() {
            if check_point_hit(screen_x, screen_y, *sx, *sy) {
                return HitTestResult::ControlPoint(ControlPointType::Corner(i as u8));
            }
        }
        for i in 0..4 {
            let j = (i + 1) % 4;
            let (x1, y1) = screen_corners[i];
            let (x2, y2) = screen_corners[j];
            if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }
        if self.fill && point_in_quad(screen_x, screen_y, &screen_corners) {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        let corners = self.corner_ts_prices();
        let screen_corners: Vec<(f64, f64)> = corners.iter()
            .map(|(ts, p)| {
                let bar = timestamp_ms_to_bar_f64(bars, *ts);
                (viewport.bar_to_x_f64(bar), viewport.price_to_y(*p, price_scale.price_min, price_scale.price_max))
            })
            .collect();

        let center_bar = timestamp_ms_to_bar_f64(bars, self.center_ts);
        let cx = viewport.bar_to_x_f64(center_bar);
        let cy = viewport.price_to_y(self.center_price, price_scale.price_min, price_scale.price_max);

        vec![
            ControlPoint::move_point(cx, cy),
            ControlPoint::with_type(ControlPointType::Corner(0), screen_corners[0].0, screen_corners[0].1),
            ControlPoint::with_type(ControlPointType::Corner(1), screen_corners[1].0, screen_corners[1].1),
            ControlPoint::with_type(ControlPointType::Corner(2), screen_corners[2].0, screen_corners[2].1),
            ControlPoint::with_type(ControlPointType::Corner(3), screen_corners[3].0, screen_corners[3].1),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let corners = self.corner_ts_prices();
        let screen_corners: Vec<(f64, f64)> = corners.iter()
            .map(|(ts, p)| (ctx.ts_to_x_ms(*ts), ctx.price_to_y(*p)))
            .collect();

        if self.fill {
            let fill_color = format!("{}{:02x}", &self.data.color.stroke[..7], (self.fill_opacity * 255.0) as u8);
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.move_to(screen_corners[0].0, screen_corners[0].1);
            for (x, y) in screen_corners.iter().skip(1) { ctx.line_to(*x, *y); }
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
        ctx.move_to(crisp(screen_corners[0].0, dpr), crisp(screen_corners[0].1, dpr));
        for (x, y) in screen_corners.iter().skip(1) { ctx.line_to(crisp(*x, dpr), crisp(*y, dpr)); }
        ctx.close_path();
        ctx.stroke();
        ctx.set_line_dash(&[]);

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (x, y) in &screen_corners {
                ctx.begin_path();
                ctx.arc(*x, *y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill(); ctx.stroke();
            }
            let cx = ctx.ts_to_x_ms(self.center_ts);
            let cy = ctx.price_to_y(self.center_price);
            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill(); ctx.stroke();
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let min_x = screen_corners.iter().map(|(x, _)| *x).fold(f64::INFINITY, f64::min);
                let max_x = screen_corners.iter().map(|(x, _)| *x).fold(f64::NEG_INFINITY, f64::max);
                let min_y = screen_corners.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
                let max_y = screen_corners.iter().map(|(_, y)| *y).fold(f64::NEG_INFINITY, f64::max);
                let cx = ctx.ts_to_x_ms(self.center_ts);
                let cy = ctx.price_to_y(self.center_price);
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => cx,
                    super::super::TextAlign::End => max_x,
                };
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => min_y - text_offset,
                    super::super::TextAlign::Center => cy,
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

fn point_in_quad(px: f64, py: f64, quad: &[(f64, f64)]) -> bool {
    if quad.len() < 4 { return false; }
    let mut winding = 0i32;
    for i in 0..4 {
        let (x1, y1) = quad[i];
        let (x2, y2) = quad[(i + 1) % 4];
        if y1 <= py {
            if y2 > py && is_left(x1, y1, x2, y2, px, py) > 0.0 { winding += 1; }
        } else if y2 <= py && is_left(x1, y1, x2, y2, px, py) < 0.0 {
            winding -= 1;
        }
    }
    winding != 0
}

fn is_left(x0: f64, y0: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0)
}

fn create_rotated_rectangle(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    if points.len() >= 3 {
        let (center_ts, center_price) = points[0];
        let dt = (points[1].0 - center_ts) as f64;
        let dp = points[1].1 - center_price;
        let rotation = dp.atan2(dt).to_degrees();
        let half_width_ms = ((dt * dt + dp * dp).sqrt() as i64).max(1);
        let dt3 = (points[2].0 - center_ts) as f64;
        let dp3 = points[2].1 - center_price;
        let half_height = (dt3 * dt3 + dp3 * dp3).sqrt().max(f64::EPSILON);
        Box::new(RotatedRectangle::new(center_ts, center_price, half_width_ms, half_height, rotation, color))
    } else {
        let (center_ts, center_price) = points.first().copied().unwrap_or((0, 100.0));
        Box::new(RotatedRectangle::new(center_ts, center_price, 600_000, center_price.abs() * 0.03 + 1.0, 0.0, color))
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "rotated_rectangle", display_name: "Rotated Rectangle", kind: PrimitiveKind::Shape,
        click_behavior: ClickBehavior::ThreePoint,
        tooltip: "Draw a rotatable rectangle (center, corner, height)",
        icon: "rotated_rectangle", default_color: "#3F51B5",
        factory: create_rotated_rectangle,
        supports_text: true, has_levels: false, has_points_config: false,
    }
}
