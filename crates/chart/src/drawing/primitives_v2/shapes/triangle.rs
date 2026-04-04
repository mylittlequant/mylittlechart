//! Triangle primitive
//!
//! A three-point polygon shape.

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

/// Triangle - three-point polygon
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Triangle {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Point 1 bar index
    pub bar1: f64,
    /// Point 1 price
    pub price1: f64,
    /// Point 2 bar index
    pub bar2: f64,
    /// Point 2 price
    pub price2: f64,
    /// Point 3 bar index
    pub bar3: f64,
    /// Point 3 price
    pub price3: f64,
    /// Fill the triangle
    #[serde(default = "default_true")]
    pub fill: bool,
    /// Fill opacity (0.0 - 1.0)
    #[serde(default = "default_fill_opacity")]
    pub fill_opacity: f64,
}

fn default_true() -> bool {
    true
}

fn default_fill_opacity() -> f64 {
    0.2
}

impl Triangle {
    /// Create a new triangle
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, bar3: f64, price3: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "triangle".to_string(),
                display_name: "Triangle".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            bar1,
            price1,
            bar2,
            price2,
            bar3,
            price3,
            fill: true,
            fill_opacity: 0.2,
        }
    }

    /// Get center point
    pub fn center(&self) -> (f64, f64) {
        ((self.bar1 + self.bar2 + self.bar3) / 3.0, (self.price1 + self.price2 + self.price3) / 3.0)
    }
}

impl Primitive for Triangle {
    fn type_id(&self) -> &'static str {
        "triangle"
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
        vec![
            (self.bar1, self.price1),
            (self.bar2, self.price2),
            (self.bar3, self.price3),
        ]
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        if points.len() >= 1 {
            self.bar1 = points[0].0;
            self.price1 = points[0].1;
        }
        if points.len() >= 2 {
            self.bar2 = points[1].0;
            self.price2 = points[1].1;
        }
        if points.len() >= 3 {
            self.bar3 = points[2].0;
            self.price3 = points[2].1;
        }
    }

    fn translate(&mut self, bar_delta: f64, price_delta: f64) {
        self.bar1 += bar_delta;
        self.bar2 += bar_delta;
        self.bar3 += bar_delta;
        self.price1 += price_delta;
        self.price2 += price_delta;
        self.price3 += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, bar: f64, price: f64) {
        match point_type {
            ControlPointType::Point1 => {
                self.bar1 = bar;
                self.price1 = price;
            }
            ControlPointType::Point2 => {
                self.bar2 = bar;
                self.price2 = price;
            }
            ControlPointType::Point3 => {
                self.bar3 = bar;
                self.price3 = price;
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
        let x1 = viewport.bar_to_x_f64(self.bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(self.bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);
        let x3 = viewport.bar_to_x_f64(self.bar3);
        let y3 = viewport.price_to_y(self.price3, price_scale.price_min, price_scale.price_max);

        // Check control points
        if check_point_hit(screen_x, screen_y, x1, y1) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        if check_point_hit(screen_x, screen_y, x2, y2) {
            return HitTestResult::ControlPoint(ControlPointType::Point2);
        }
        if check_point_hit(screen_x, screen_y, x3, y3) {
            return HitTestResult::ControlPoint(ControlPointType::Point3);
        }

        // Check center/move point
        let cx = (x1 + x2 + x3) / 3.0;
        let cy = (y1 + y2 + y3) / 3.0;
        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }

        // Check edges
        if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        if point_to_line_distance(screen_x, screen_y, x2, y2, x3, y3) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        if point_to_line_distance(screen_x, screen_y, x3, y3, x1, y1) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        // Check if inside filled triangle using barycentric coordinates
        if self.fill && point_in_triangle(screen_x, screen_y, x1, y1, x2, y2, x3, y3) {
            return HitTestResult::Body;
        }

        HitTestResult::Miss
    }

    fn control_points(
        &self,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Vec<ControlPoint> {
        let x1 = viewport.bar_to_x_f64(self.bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(self.bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);
        let x3 = viewport.bar_to_x_f64(self.bar3);
        let y3 = viewport.price_to_y(self.price3, price_scale.price_min, price_scale.price_max);

        vec![
            ControlPoint::point1(x1, y1),
            ControlPoint::point2(x2, y2),
            ControlPoint::point3(x3, y3),
            ControlPoint::move_point((x1 + x2 + x3) / 3.0, (y1 + y2 + y3) / 3.0),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.bar_to_x(self.bar1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.bar_to_x(self.bar2);
        let y2 = ctx.price_to_y(self.price2);
        let x3 = ctx.bar_to_x(self.bar3);
        let y3 = ctx.price_to_y(self.price3);

        if self.fill {
            let fill_color = format!("{}{}",
                &self.data.color.stroke[..7],
                format!("{:02x}", (self.fill_opacity * 255.0) as u8)
            );
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.move_to(x1, y1);
            ctx.line_to(x2, y2);
            ctx.line_to(x3, y3);
            ctx.close_path();
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
                ctx.fill();
                ctx.stroke();
            }

            let cx = (x1 + x2 + x3) / 3.0;
            let cy = (y1 + y2 + y3) / 3.0;
            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                // Calculate bounding box of the triangle
                let min_x = x1.min(x2).min(x3);
                let max_x = x1.max(x2).max(x3);
                let min_y = y1.min(y2).min(y3);
                let max_y = y1.max(y2).max(y3);
                // Calculate X based on h_align
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => (x1 + x2 + x3) / 3.0,
                    super::super::TextAlign::End => max_x,
                };
                // Calculate Y based on v_align:
                // Start = ABOVE upper boundary, Center = middle, End = BELOW lower boundary
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => min_y - text_offset,
                    super::super::TextAlign::Center => (y1 + y2 + y3) / 3.0,
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

/// Check if point is inside triangle using barycentric coordinates
fn point_in_triangle(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) -> bool {
    let denom = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3);
    if denom.abs() < 0.0001 {
        return false;
    }

    let a = ((y2 - y3) * (px - x3) + (x3 - x2) * (py - y3)) / denom;
    let b = ((y3 - y1) * (px - x3) + (x1 - x3) * (py - y3)) / denom;
    let c = 1.0 - a - b;

    a >= 0.0 && a <= 1.0 && b >= 0.0 && b <= 1.0 && c >= 0.0 && c <= 1.0
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_triangle(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar1, price1) = points.first().copied().unwrap_or((0.0, 100.0));
    let (bar2, price2) = points.get(1).copied().unwrap_or((bar1 + 10.0, price1 * 1.05));
    let (bar3, price3) = points.get(2).copied().unwrap_or((bar1 + 5.0, price1 * 0.95));
    Box::new(Triangle::new(bar1, price1, bar2, price2, bar3, price3, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "triangle",
        display_name: "Triangle",
        kind: PrimitiveKind::Shape,
        click_behavior: ClickBehavior::ThreePoint,
        tooltip: "Draw a triangle with three points",
        icon: "triangle",
        default_color: "#FF9800",
        factory: create_triangle,
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
