//! Curve primitive (Bezier)
//!
//! A quadratic Bezier curve defined by start, control, and end points.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    HIT_TOLERANCE, CONTROL_POINT_HIT_RADIUS, CONTROL_POINT_RADIUS,
    RenderContext, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

/// Curve - quadratic Bezier curve
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Curve {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Start point bar
    pub start_bar: f64,
    /// Start point price
    pub start_price: f64,
    /// Control point bar
    pub control_bar: f64,
    /// Control point price
    pub control_price: f64,
    /// End point bar
    pub end_bar: f64,
    /// End point price
    pub end_price: f64,
}

impl Curve {
    /// Create a new Bezier curve
    pub fn new(start_bar: f64, start_price: f64, control_bar: f64, control_price: f64, end_bar: f64, end_price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "curve".to_string(),
                display_name: "Curve".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            start_bar,
            start_price,
            control_bar,
            control_price,
            end_bar,
            end_price,
        }
    }

    /// Evaluate the Bezier curve at parameter t (0..1)
    pub fn evaluate(&self, t: f64) -> (f64, f64) {
        let t2 = t * t;
        let mt = 1.0 - t;
        let mt2 = mt * mt;

        let bar = mt2 * self.start_bar + 2.0 * mt * t * self.control_bar + t2 * self.end_bar;
        let price = mt2 * self.start_price + 2.0 * mt * t * self.control_price + t2 * self.end_price;

        (bar, price)
    }

    /// Get points along the curve for rendering
    pub fn sample_points(&self, num_points: usize) -> Vec<(f64, f64)> {
        (0..=num_points)
            .map(|i| {
                let t = i as f64 / num_points as f64;
                self.evaluate(t)
            })
            .collect()
    }
}

impl Primitive for Curve {
    fn type_id(&self) -> &'static str {
        "curve"
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
            (self.start_bar, self.start_price),
            (self.control_bar, self.control_price),
            (self.end_bar, self.end_price),
        ]
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        if !points.is_empty() {
            self.start_bar = points[0].0;
            self.start_price = points[0].1;
        }
        if points.len() >= 2 {
            self.end_bar = points[1].0;
            self.end_price = points[1].1;
            // Default control point to midpoint above
            self.control_bar = (self.start_bar + self.end_bar) / 2.0;
            self.control_price = (self.start_price + self.end_price) / 2.0 + (self.start_price - self.end_price).abs() * 0.3;
        }
        if points.len() >= 3 {
            self.control_bar = points[2].0;
            self.control_price = points[2].1;
        }
    }

    fn translate(&mut self, bar_delta: f64, price_delta: f64) {
        self.start_bar += bar_delta;
        self.start_price += price_delta;
        self.control_bar += bar_delta;
        self.control_price += price_delta;
        self.end_bar += bar_delta;
        self.end_price += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, bar: f64, price: f64) {
        match point_type {
            ControlPointType::Point1 => {
                self.start_bar = bar;
                self.start_price = price;
            }
            ControlPointType::Point2 => {
                self.end_bar = bar;
                self.end_price = price;
            }
            ControlPointType::Point3 => {
                self.control_bar = bar;
                self.control_price = price;
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
        let sx1 = viewport.bar_to_x_f64(self.start_bar);
        let sy1 = viewport.price_to_y(self.start_price, price_scale.price_min, price_scale.price_max);
        let scx = viewport.bar_to_x_f64(self.control_bar);
        let scy = viewport.price_to_y(self.control_price, price_scale.price_min, price_scale.price_max);
        let sx2 = viewport.bar_to_x_f64(self.end_bar);
        let sy2 = viewport.price_to_y(self.end_price, price_scale.price_min, price_scale.price_max);

        // Check control points
        if check_point_hit(screen_x, screen_y, sx1, sy1) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        if check_point_hit(screen_x, screen_y, sx2, sy2) {
            return HitTestResult::ControlPoint(ControlPointType::Point2);
        }
        if check_point_hit(screen_x, screen_y, scx, scy) {
            return HitTestResult::ControlPoint(ControlPointType::Point3);
        }

        // Sample curve and check distance
        for i in 0..20 {
            let t1 = i as f64 / 20.0;
            let t2 = (i + 1) as f64 / 20.0;

            let (b1, p1) = self.evaluate(t1);
            let (b2, p2) = self.evaluate(t2);

            let x1 = viewport.bar_to_x_f64(b1);
            let y1 = viewport.price_to_y(p1, price_scale.price_min, price_scale.price_max);
            let x2 = viewport.bar_to_x_f64(b2);
            let y2 = viewport.price_to_y(p2, price_scale.price_min, price_scale.price_max);

            let dist = point_to_segment_distance(screen_x, screen_y, x1, y1, x2, y2);
            if dist < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }

        HitTestResult::Miss
    }

    fn control_points(
        &self,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Vec<ControlPoint> {
        let sx1 = viewport.bar_to_x_f64(self.start_bar);
        let sy1 = viewport.price_to_y(self.start_price, price_scale.price_min, price_scale.price_max);
        let scx = viewport.bar_to_x_f64(self.control_bar);
        let scy = viewport.price_to_y(self.control_price, price_scale.price_min, price_scale.price_max);
        let sx2 = viewport.bar_to_x_f64(self.end_bar);
        let sy2 = viewport.price_to_y(self.end_price, price_scale.price_min, price_scale.price_max);

        vec![
            ControlPoint::point1(sx1, sy1),
            ControlPoint::point2(sx2, sy2),
            ControlPoint::point3(scx, scy),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let sx1 = ctx.bar_to_x(self.start_bar);
        let sy1 = ctx.price_to_y(self.start_price);
        let scx = ctx.bar_to_x(self.control_bar);
        let scy = ctx.price_to_y(self.control_price);
        let sx2 = ctx.bar_to_x(self.end_bar);
        let sy2 = ctx.price_to_y(self.end_price);

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
        ctx.move_to(sx1, sy1);
        ctx.quadratic_curve_to(scx, scy, sx2, sy2);
        ctx.stroke();
        ctx.set_line_dash(&[]);

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            // Draw control point handles (dashed lines)
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.set_stroke_color("#888888");
            ctx.begin_path();
            ctx.move_to(sx1, sy1);
            ctx.line_to(scx, scy);
            ctx.line_to(sx2, sy2);
            ctx.stroke();
            ctx.set_line_dash(&[]);

            // Control points
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            for (x, y) in [(sx1, sy1), (sx2, sy2), (scx, scy)] {
                ctx.begin_path();
                ctx.arc(x, y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                // Calculate bounding box from curve points
                let min_x = sx1.min(scx).min(sx2);
                let max_x = sx1.max(scx).max(sx2);
                let min_y = sy1.min(scy).min(sy2);
                let max_y = sy1.max(scy).max(sy2);
                // Calculate X based on h_align
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => {
                        let (mid_bar, _) = self.evaluate(0.5);
                        ctx.bar_to_x(mid_bar)
                    },
                    super::super::TextAlign::End => max_x,
                };
                // Calculate Y based on v_align:
                // Start = ABOVE upper boundary, Center = middle, End = BELOW lower boundary
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => min_y - text_offset,
                    super::super::TextAlign::Center => {
                        let (_, mid_price) = self.evaluate(0.5);
                        ctx.price_to_y(mid_price)
                    },
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

fn point_to_segment_distance(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;

    if len_sq < 0.0001 {
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
    }

    let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);

    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;

    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_curve(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (start_bar, start_price) = points.first().copied().unwrap_or((0.0, 100.0));
    let (end_bar, end_price) = points.get(1).copied().unwrap_or((start_bar + 20.0, start_price));
    let (control_bar, control_price) = points.get(2).copied().unwrap_or((
        (start_bar + end_bar) / 2.0,
        (start_price + end_price) / 2.0 + (start_price - end_price).abs() * 0.3,
    ));
    Box::new(Curve::new(start_bar, start_price, control_bar, control_price, end_bar, end_price, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "curve",
        display_name: "Curve",
        kind: PrimitiveKind::Shape,
        click_behavior: ClickBehavior::ThreePoint,
        tooltip: "Draw a Bezier curve (start, end, control)",
        icon: "curve",
        default_color: "#00BCD4",
        factory: create_curve,
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
