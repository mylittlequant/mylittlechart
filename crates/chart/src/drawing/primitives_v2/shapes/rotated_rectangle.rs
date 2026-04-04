//! Rotated Rectangle primitive
//!
//! A rectangle that can be rotated at any angle.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    point_to_line_distance, HIT_TOLERANCE, CONTROL_POINT_HIT_RADIUS, CONTROL_POINT_RADIUS,
    RenderContext, crisp, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

/// Rotated Rectangle - rectangle with rotation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RotatedRectangle {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Center bar index
    pub center_bar: f64,
    /// Center price
    pub center_price: f64,
    /// Half-width in bars
    pub half_width: f64,
    /// Half-height in price units
    pub half_height: f64,
    /// Rotation angle in degrees
    pub rotation: f64,
    /// Fill the rectangle
    #[serde(default = "default_true")]
    pub fill: bool,
    /// Fill opacity
    #[serde(default = "default_fill_opacity")]
    pub fill_opacity: f64,
}

fn default_true() -> bool {
    true
}

fn default_fill_opacity() -> f64 {
    0.2
}

impl RotatedRectangle {
    /// Create a new rotated rectangle
    pub fn new(center_bar: f64, center_price: f64, half_width: f64, half_height: f64, rotation: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "rotated_rectangle".to_string(),
                display_name: "Rotated Rectangle".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            center_bar,
            center_price,
            half_width,
            half_height,
            rotation,
            fill: true,
            fill_opacity: 0.2,
        }
    }

    /// Create from three points (center, corner, rotation reference)
    pub fn from_points(p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), color: &str) -> Self {
        let center_bar = p1.0;
        let center_price = p1.1;

        // Calculate rotation from p1 to p2
        let dx = p2.0 - p1.0;
        let dy = p2.1 - p1.1;
        let rotation = dy.atan2(dx).to_degrees();

        // p2 defines one corner, calculate dimensions
        let half_width = (dx * dx + dy * dy).sqrt() / 2.0;

        // p3 defines the height (perpendicular distance)
        let dx3 = p3.0 - p1.0;
        let dy3 = p3.1 - p1.1;
        let half_height = (dx3 * dx3 + dy3 * dy3).sqrt().abs() / 2.0;

        Self::new(center_bar, center_price, half_width.max(1.0), half_height.max(0.01), rotation, color)
    }

    /// Get corners in data coordinates
    pub fn corners(&self) -> [(f64, f64); 4] {
        let cos_r = self.rotation.to_radians().cos();
        let sin_r = self.rotation.to_radians().sin();

        let hw = self.half_width;
        let hh = self.half_height;

        // Local corners before rotation
        let local = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)];

        let mut corners = [(0.0, 0.0); 4];
        for (i, (lx, ly)) in local.iter().enumerate() {
            // Rotate and translate
            let rx = lx * cos_r - ly * sin_r + self.center_bar;
            let ry = lx * sin_r + ly * cos_r + self.center_price;
            corners[i] = (rx, ry);
        }
        corners
    }
}

impl Primitive for RotatedRectangle {
    fn type_id(&self) -> &'static str {
        "rotated_rectangle"
    }

    fn display_name(&self) -> &str {
        &self.data.display_name
    }

    fn kind(&self) -> PrimitiveKind {
        PrimitiveKind::Shape
    }

    fn click_behavior(&self) -> ClickBehavior {
        ClickBehavior::ThreePoint
    }

    fn data(&self) -> &PrimitiveData {
        &self.data
    }

    fn data_mut(&mut self) -> &mut PrimitiveData {
        &mut self.data
    }

    fn points(&self) -> Vec<(f64, f64)> {
        let corners = self.corners();
        vec![
            (self.center_bar, self.center_price),
            corners[1], // Top-right corner
            corners[2], // Bottom-right corner
        ]
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        if points.len() >= 1 {
            self.center_bar = points[0].0;
            self.center_price = points[0].1;
        }
        if points.len() >= 2 {
            let dx = points[1].0 - self.center_bar;
            let dy = points[1].1 - self.center_price;
            self.rotation = dy.atan2(dx).to_degrees();
            self.half_width = (dx * dx + dy * dy).sqrt().max(1.0);
        }
        if points.len() >= 3 {
            // Third point for height
            let dx = points[2].0 - self.center_bar;
            let dy = points[2].1 - self.center_price;
            self.half_height = (dx * dx + dy * dy).sqrt().abs().max(0.01);
        }
    }

    fn translate(&mut self, bar_delta: f64, price_delta: f64) {
        self.center_bar += bar_delta;
        self.center_price += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, bar: f64, price: f64) {
        match point_type {
            ControlPointType::Move => {
                self.center_bar = bar;
                self.center_price = price;
            }
            ControlPointType::Point2 => {
                // Adjust rotation and width
                let dx = bar - self.center_bar;
                let dy = price - self.center_price;
                self.rotation = dy.atan2(dx).to_degrees();
                self.half_width = (dx * dx + dy * dy).sqrt().max(1.0);
            }
            ControlPointType::Point3 => {
                // Adjust height
                let dx = bar - self.center_bar;
                let dy = price - self.center_price;
                self.half_height = (dx * dx + dy * dy).sqrt().abs().max(0.01);
            }
            _ => {}
        }
    }

    fn hit_test(
        &self,
        screen_x: f64,
        screen_y: f64,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> HitTestResult {
        let corners = self.corners();
        let screen_corners: Vec<(f64, f64)> = corners.iter()
            .map(|(b, p)| (viewport.bar_to_x_f64(*b), viewport.price_to_y(*p, price_scale.price_min, price_scale.price_max)))
            .collect();

        let cx = viewport.bar_to_x_f64(self.center_bar);
        let cy = viewport.price_to_y(self.center_price, price_scale.price_min, price_scale.price_max);

        // Check center move point
        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }

        // Check corner control points
        for (i, (sx, sy)) in screen_corners.iter().enumerate() {
            if check_point_hit(screen_x, screen_y, *sx, *sy) {
                return HitTestResult::ControlPoint(ControlPointType::Corner(i as u8));
            }
        }

        // Check edges
        for i in 0..4 {
            let j = (i + 1) % 4;
            let (x1, y1) = screen_corners[i];
            let (x2, y2) = screen_corners[j];
            if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }

        // Check if inside filled rectangle (point in quadrilateral)
        if self.fill && point_in_quad(screen_x, screen_y, &screen_corners) {
            return HitTestResult::Body;
        }

        HitTestResult::Miss
    }

    fn control_points(
        &self,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Vec<ControlPoint> {
        let corners = self.corners();
        let screen_corners: Vec<(f64, f64)> = corners.iter()
            .map(|(b, p)| (viewport.bar_to_x_f64(*b), viewport.price_to_y(*p, price_scale.price_min, price_scale.price_max)))
            .collect();

        let cx = viewport.bar_to_x_f64(self.center_bar);
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
        let corners = self.corners();
        let screen_corners: Vec<(f64, f64)> = corners.iter()
            .map(|(b, p)| (ctx.bar_to_x(*b), ctx.price_to_y(*p)))
            .collect();

        // Fill if enabled
        if self.fill {
            let fill_color = format!("{}{}",
                &self.data.color.stroke[..7],
                format!("{:02x}", (self.fill_opacity * 255.0) as u8)
            );
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.move_to(screen_corners[0].0, screen_corners[0].1);
            for (x, y) in screen_corners.iter().skip(1) {
                ctx.line_to(*x, *y);
            }
            ctx.close_path();
            ctx.fill();
        }

        // Draw stroke
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
        for (x, y) in screen_corners.iter().skip(1) {
            ctx.line_to(crisp(*x, dpr), crisp(*y, dpr));
        }
        ctx.close_path();
        ctx.stroke();
        ctx.set_line_dash(&[]);

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            // Corner control points
            for (x, y) in &screen_corners {
                ctx.begin_path();
                ctx.arc(*x, *y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }

            // Center move point
            let cx = ctx.bar_to_x(self.center_bar);
            let cy = ctx.price_to_y(self.center_price);
            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                // Calculate bounding box from screen corners
                let min_x = screen_corners.iter().map(|(x, _)| *x).fold(f64::INFINITY, f64::min);
                let max_x = screen_corners.iter().map(|(x, _)| *x).fold(f64::NEG_INFINITY, f64::max);
                let min_y = screen_corners.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
                let max_y = screen_corners.iter().map(|(_, y)| *y).fold(f64::NEG_INFINITY, f64::max);
                // Center of the rotated rectangle
                let cx = ctx.bar_to_x(self.center_bar);
                let cy = ctx.price_to_y(self.center_price);
                // Calculate X based on h_align
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => cx,
                    super::super::TextAlign::End => max_x,
                };
                // Calculate Y based on v_align:
                // Start = ABOVE upper boundary, Center = middle, End = BELOW lower boundary
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => min_y - text_offset,
                    super::super::TextAlign::Center => cy,
                    super::super::TextAlign::End => max_y + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    fn clone_box(&self) -> Box<dyn Primitive> {
        Box::new(self.clone())
    }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2)
}

/// Check if point is inside a quadrilateral
fn point_in_quad(px: f64, py: f64, quad: &[(f64, f64)]) -> bool {
    if quad.len() < 4 {
        return false;
    }

    // Use winding number algorithm
    let mut winding = 0i32;
    for i in 0..4 {
        let (x1, y1) = quad[i];
        let (x2, y2) = quad[(i + 1) % 4];

        if y1 <= py {
            if y2 > py && is_left(x1, y1, x2, y2, px, py) > 0.0 {
                winding += 1;
            }
        } else if y2 <= py && is_left(x1, y1, x2, y2, px, py) < 0.0 {
            winding -= 1;
        }
    }
    winding != 0
}

fn is_left(x0: f64, y0: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0)
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_rotated_rectangle(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    if points.len() >= 3 {
        Box::new(RotatedRectangle::from_points(points[0], points[1], points[2], color))
    } else {
        let (center_bar, center_price) = points.first().copied().unwrap_or((0.0, 100.0));
        Box::new(RotatedRectangle::new(center_bar, center_price, 10.0, center_price * 0.03, 0.0, color))
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "rotated_rectangle",
        display_name: "Rotated Rectangle",
        kind: PrimitiveKind::Shape,
        click_behavior: ClickBehavior::ThreePoint,
        tooltip: "Draw a rotatable rectangle (center, corner, height)",
        icon: "rotated_rectangle",
        default_color: "#3F51B5",
        factory: create_rotated_rectangle,
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
