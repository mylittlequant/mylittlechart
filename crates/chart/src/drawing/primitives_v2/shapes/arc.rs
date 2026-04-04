//! Arc primitive
//!
//! A curved arc segment defined by center, radius, and angle range.

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

/// Arc - curved line segment
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Arc {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Center bar index
    pub center_bar: f64,
    /// Center price
    pub center_price: f64,
    /// Radius in bars
    pub radius_bars: f64,
    /// Start angle in degrees (0 = right, 90 = up)
    pub start_angle: f64,
    /// End angle in degrees
    pub end_angle: f64,
}

impl Arc {
    /// Create a new arc
    pub fn new(center_bar: f64, center_price: f64, radius_bars: f64, start_angle: f64, end_angle: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "arc".to_string(),
                display_name: "Arc".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            center_bar,
            center_price,
            radius_bars,
            start_angle,
            end_angle,
        }
    }

    /// Create a semicircle (180 degrees)
    pub fn semicircle(center_bar: f64, center_price: f64, radius_bars: f64, color: &str) -> Self {
        Self::new(center_bar, center_price, radius_bars, 0.0, 180.0, color)
    }
}

impl Primitive for Arc {
    fn type_id(&self) -> &'static str {
        "arc"
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
        let start_rad = self.start_angle.to_radians();
        let end_rad = self.end_angle.to_radians();
        vec![
            (self.center_bar, self.center_price),
            (self.center_bar + self.radius_bars * start_rad.cos(), self.center_price),
            (self.center_bar + self.radius_bars * end_rad.cos(), self.center_price),
        ]
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        if !points.is_empty() {
            self.center_bar = points[0].0;
            self.center_price = points[0].1;
        }
        if points.len() >= 2 {
            // Calculate radius and start angle from second point
            let dx = points[1].0 - self.center_bar;
            self.radius_bars = dx.abs().max(1.0);
            self.start_angle = 0.0;
        }
        if points.len() >= 3 {
            // Calculate end angle from third point
            let dx = points[2].0 - self.center_bar;
            let dy = points[2].1 - self.center_price;
            self.end_angle = dy.atan2(dx).to_degrees();
            if self.end_angle < 0.0 {
                self.end_angle += 360.0;
            }
        }
    }

    fn translate(&mut self, bar_delta: f64, price_delta: f64) {
        self.center_bar += bar_delta;
        self.center_price += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, bar: f64, price: f64) {
        match point_type {
            ControlPointType::Point1 | ControlPointType::Move => {
                self.center_bar = bar;
                self.center_price = price;
            }
            ControlPointType::Point2 => {
                // Adjust radius and start angle
                let dx = bar - self.center_bar;
                self.radius_bars = dx.abs().max(1.0);
            }
            ControlPointType::Point3 => {
                // Adjust end angle
                let dx = bar - self.center_bar;
                let dy = price - self.center_price;
                self.end_angle = dy.atan2(dx).to_degrees();
                if self.end_angle < 0.0 {
                    self.end_angle += 360.0;
                }
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
        let cx = viewport.bar_to_x_f64(self.center_bar);
        let cy = viewport.price_to_y(self.center_price, price_scale.price_min, price_scale.price_max);
        let rx = viewport.bar_to_x_f64(self.center_bar + self.radius_bars);
        let radius = (rx - cx).abs();

        // Check center control point
        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }

        // Distance and angle from center
        let dx = screen_x - cx;
        let dy = screen_y - cy;
        let dist = (dx * dx + dy * dy).sqrt();
        let angle = dy.atan2(dx).to_degrees();
        let angle_norm = if angle < 0.0 { angle + 360.0 } else { angle };

        // Check if on arc
        let on_arc = (dist - radius).abs() < HIT_TOLERANCE;
        let in_angle_range = if self.start_angle <= self.end_angle {
            angle_norm >= self.start_angle && angle_norm <= self.end_angle
        } else {
            angle_norm >= self.start_angle || angle_norm <= self.end_angle
        };

        if on_arc && in_angle_range {
            return HitTestResult::Body;
        }

        HitTestResult::Miss
    }

    fn control_points(
        &self,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Vec<ControlPoint> {
        let cx = viewport.bar_to_x_f64(self.center_bar);
        let cy = viewport.price_to_y(self.center_price, price_scale.price_min, price_scale.price_max);
        let rx = viewport.bar_to_x_f64(self.center_bar + self.radius_bars);
        let radius = (rx - cx).abs();

        let start_rad = self.start_angle.to_radians();
        let end_rad = self.end_angle.to_radians();

        vec![
            ControlPoint::move_point(cx, cy),
            ControlPoint::point2(cx + radius * start_rad.cos(), cy + radius * start_rad.sin()),
            ControlPoint::point3(cx + radius * end_rad.cos(), cy + radius * end_rad.sin()),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let cx = ctx.bar_to_x(self.center_bar);
        let cy = ctx.price_to_y(self.center_price);
        let rx = ctx.bar_to_x(self.center_bar + self.radius_bars);
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

            // Center point
            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();

            // Start angle point
            let start_x = cx + radius * start_rad.cos();
            let start_y = cy + radius * start_rad.sin();
            ctx.begin_path();
            ctx.arc(start_x, start_y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();

            // End angle point
            let end_x = cx + radius * end_rad.cos();
            let end_y = cy + radius * end_rad.sin();
            ctx.begin_path();
            ctx.arc(end_x, end_y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                // Bounding box of arc (using full circle bounds for simplicity)
                let min_x = cx - radius;
                let max_x = cx + radius;
                let min_y = cy - radius;
                let max_y = cy + radius;
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

// =============================================================================
// Factory Registration
// =============================================================================

fn create_arc(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (center_bar, center_price) = points.first().copied().unwrap_or((0.0, 100.0));
    let radius_bars = if points.len() >= 2 {
        (points[1].0 - center_bar).abs().max(1.0)
    } else {
        10.0
    };
    let end_angle = if points.len() >= 3 {
        let dx = points[2].0 - center_bar;
        let dy = points[2].1 - center_price;
        let mut angle = dy.atan2(dx).to_degrees();
        if angle < 0.0 { angle += 360.0; }
        angle
    } else {
        180.0
    };
    Box::new(Arc::new(center_bar, center_price, radius_bars, 0.0, end_angle, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "arc",
        display_name: "Arc",
        kind: PrimitiveKind::Shape,
        click_behavior: ClickBehavior::ThreePoint,
        tooltip: "Draw an arc (center, start, end)",
        icon: "arc",
        default_color: "#E91E63",
        factory: create_arc,
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
