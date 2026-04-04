//! Double Curve primitive (S-curve)
//!
//! A cubic Bezier curve with two control points, creating an S-shape.

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

/// Double Curve - cubic Bezier (S-curve)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DoubleCurve {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Start point bar
    pub start_bar: f64,
    /// Start point price
    pub start_price: f64,
    /// First control point bar
    pub control1_bar: f64,
    /// First control point price
    pub control1_price: f64,
    /// Second control point bar
    pub control2_bar: f64,
    /// Second control point price
    pub control2_price: f64,
    /// End point bar
    pub end_bar: f64,
    /// End point price
    pub end_price: f64,
}

impl DoubleCurve {
    /// Create a new cubic Bezier curve
    pub fn new(
        start_bar: f64, start_price: f64,
        control1_bar: f64, control1_price: f64,
        control2_bar: f64, control2_price: f64,
        end_bar: f64, end_price: f64,
        color: &str,
    ) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "double_curve".to_string(),
                display_name: "Double Curve".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            start_bar,
            start_price,
            control1_bar,
            control1_price,
            control2_bar,
            control2_price,
            end_bar,
            end_price,
        }
    }

    /// Evaluate the cubic Bezier curve at parameter t (0..1)
    pub fn evaluate(&self, t: f64) -> (f64, f64) {
        let t2 = t * t;
        let t3 = t2 * t;
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let mt3 = mt2 * mt;

        let bar = mt3 * self.start_bar
            + 3.0 * mt2 * t * self.control1_bar
            + 3.0 * mt * t2 * self.control2_bar
            + t3 * self.end_bar;

        let price = mt3 * self.start_price
            + 3.0 * mt2 * t * self.control1_price
            + 3.0 * mt * t2 * self.control2_price
            + t3 * self.end_price;

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

impl Primitive for DoubleCurve {
    fn type_id(&self) -> &'static str {
        "double_curve"
    }

    fn display_name(&self) -> &str {
        &self.data.display_name
    }

    fn kind(&self) -> PrimitiveKind {
        PrimitiveKind::Shape
    }

    fn click_behavior(&self) -> ClickBehavior {
        ClickBehavior::FourPoint
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
            (self.end_bar, self.end_price),
            (self.control1_bar, self.control1_price),
            (self.control2_bar, self.control2_price),
        ]
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        if points.len() >= 1 {
            self.start_bar = points[0].0;
            self.start_price = points[0].1;
        }
        if points.len() >= 2 {
            self.end_bar = points[1].0;
            self.end_price = points[1].1;
            // Default control points for S-curve
            let dx = self.end_bar - self.start_bar;
            let dy = self.end_price - self.start_price;
            self.control1_bar = self.start_bar + dx * 0.33;
            self.control1_price = self.start_price + dy * 0.5;
            self.control2_bar = self.start_bar + dx * 0.67;
            self.control2_price = self.end_price - dy * 0.5;
        }
        if points.len() >= 3 {
            self.control1_bar = points[2].0;
            self.control1_price = points[2].1;
        }
        if points.len() >= 4 {
            self.control2_bar = points[3].0;
            self.control2_price = points[3].1;
        }
    }

    fn translate(&mut self, bar_delta: f64, price_delta: f64) {
        self.start_bar += bar_delta;
        self.start_price += price_delta;
        self.control1_bar += bar_delta;
        self.control1_price += price_delta;
        self.control2_bar += bar_delta;
        self.control2_price += price_delta;
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
                self.control1_bar = bar;
                self.control1_price = price;
            }
            ControlPointType::Point4 => {
                self.control2_bar = bar;
                self.control2_price = price;
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
        let sc1x = viewport.bar_to_x_f64(self.control1_bar);
        let sc1y = viewport.price_to_y(self.control1_price, price_scale.price_min, price_scale.price_max);
        let sc2x = viewport.bar_to_x_f64(self.control2_bar);
        let sc2y = viewport.price_to_y(self.control2_price, price_scale.price_min, price_scale.price_max);
        let sx2 = viewport.bar_to_x_f64(self.end_bar);
        let sy2 = viewport.price_to_y(self.end_price, price_scale.price_min, price_scale.price_max);

        // Check control points
        if check_point_hit(screen_x, screen_y, sx1, sy1) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        if check_point_hit(screen_x, screen_y, sx2, sy2) {
            return HitTestResult::ControlPoint(ControlPointType::Point2);
        }
        if check_point_hit(screen_x, screen_y, sc1x, sc1y) {
            return HitTestResult::ControlPoint(ControlPointType::Point3);
        }
        if check_point_hit(screen_x, screen_y, sc2x, sc2y) {
            return HitTestResult::ControlPoint(ControlPointType::Point4);
        }

        // Sample curve and check distance
        for i in 0..30 {
            let t1 = i as f64 / 30.0;
            let t2 = (i + 1) as f64 / 30.0;

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
        let sc1x = viewport.bar_to_x_f64(self.control1_bar);
        let sc1y = viewport.price_to_y(self.control1_price, price_scale.price_min, price_scale.price_max);
        let sc2x = viewport.bar_to_x_f64(self.control2_bar);
        let sc2y = viewport.price_to_y(self.control2_price, price_scale.price_min, price_scale.price_max);
        let sx2 = viewport.bar_to_x_f64(self.end_bar);
        let sy2 = viewport.price_to_y(self.end_price, price_scale.price_min, price_scale.price_max);

        vec![
            ControlPoint::point1(sx1, sy1),
            ControlPoint::point2(sx2, sy2),
            ControlPoint::point3(sc1x, sc1y),
            ControlPoint::point4(sc2x, sc2y),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let sx1 = ctx.bar_to_x(self.start_bar);
        let sy1 = ctx.price_to_y(self.start_price);
        let sc1x = ctx.bar_to_x(self.control1_bar);
        let sc1y = ctx.price_to_y(self.control1_price);
        let sc2x = ctx.bar_to_x(self.control2_bar);
        let sc2y = ctx.price_to_y(self.control2_price);
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
        ctx.bezier_curve_to(sc1x, sc1y, sc2x, sc2y, sx2, sy2);
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
            ctx.line_to(sc1x, sc1y);
            ctx.stroke();
            ctx.begin_path();
            ctx.move_to(sx2, sy2);
            ctx.line_to(sc2x, sc2y);
            ctx.stroke();
            ctx.set_line_dash(&[]);

            // Control points
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            for (x, y) in [(sx1, sy1), (sx2, sy2), (sc1x, sc1y), (sc2x, sc2y)] {
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
                // Calculate bounding box from curve control points
                let min_x = sx1.min(sc1x).min(sc2x).min(sx2);
                let max_x = sx1.max(sc1x).max(sc2x).max(sx2);
                let min_y = sy1.min(sc1y).min(sc2y).min(sy2);
                let max_y = sy1.max(sc1y).max(sc2y).max(sy2);
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

fn create_double_curve(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (start_bar, start_price) = points.first().copied().unwrap_or((0.0, 100.0));
    let (end_bar, end_price) = points.get(1).copied().unwrap_or((start_bar + 30.0, start_price));

    let dx = end_bar - start_bar;
    let dy = end_price - start_price;

    let (control1_bar, control1_price) = points.get(2).copied().unwrap_or((
        start_bar + dx * 0.33,
        start_price + dy.abs() * 0.3,
    ));
    let (control2_bar, control2_price) = points.get(3).copied().unwrap_or((
        start_bar + dx * 0.67,
        end_price - dy.abs() * 0.3,
    ));

    Box::new(DoubleCurve::new(
        start_bar, start_price,
        control1_bar, control1_price,
        control2_bar, control2_price,
        end_bar, end_price,
        color,
    ))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "double_curve",
        display_name: "Double Curve",
        kind: PrimitiveKind::Shape,
        click_behavior: ClickBehavior::FourPoint,
        tooltip: "Draw an S-curve with two control points",
        icon: "double_curve",
        default_color: "#8BC34A",
        factory: create_double_curve,
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
