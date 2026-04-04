//! Circle primitive
//!
//! A circle/ellipse defined by center and 4 edge points.
//! Uses 5 data-coordinate points: center + 4 edge points (top, right, bottom, left)

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType, ControlPointCursor,
    PrimitiveColor, LineStyle,
    HIT_TOLERANCE, CONTROL_POINT_HIT_RADIUS, CONTROL_POINT_RADIUS,
    RenderContext, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

/// Circle - defined by center and radii in data coordinates
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Circle {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Center bar index
    pub center_bar: f64,
    /// Center price
    pub center_price: f64,
    /// Horizontal radius in bars
    pub radius_bars: f64,
    /// Vertical radius in price units
    pub radius_price: f64,
    /// Fill the circle
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

impl Circle {
    /// Create a new circle
    pub fn new(center_bar: f64, center_price: f64, radius_bars: f64, radius_price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "circle".to_string(),
                display_name: "Circle".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            center_bar,
            center_price,
            radius_bars,
            radius_price,
            fill: true,
            fill_opacity: 0.2,
        }
    }

    /// Create from center and edge point
    pub fn from_points(center_bar: f64, center_price: f64, edge_bar: f64, edge_price: f64) -> Self {
        let radius_bars = (edge_bar - center_bar).abs().max(1.0);
        let radius_price = (edge_price - center_price).abs().max(1.0);
        Self::new(center_bar, center_price, radius_bars, radius_price, "#2196F3")
    }
}

impl Primitive for Circle {
    fn type_id(&self) -> &'static str {
        "circle"
    }

    fn display_name(&self) -> &str {
        &self.data.display_name
    }

    fn kind(&self) -> PrimitiveKind {
        PrimitiveKind::Shape
    }

    fn click_behavior(&self) -> ClickBehavior {
        ClickBehavior::TwoPoint
    }

    fn data(&self) -> &PrimitiveData {
        &self.data
    }

    fn data_mut(&mut self) -> &mut PrimitiveData {
        &mut self.data
    }

    /// Returns 2 points: center and corner (for TwoPoint behavior)
    fn points(&self) -> Vec<(f64, f64)> {
        vec![
            (self.center_bar, self.center_price),
            (self.center_bar + self.radius_bars, self.center_price + self.radius_price),
        ]
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        if !points.is_empty() {
            self.center_bar = points[0].0;
            self.center_price = points[0].1;
        }
        if points.len() >= 2 {
            // Calculate radius from second point (edge)
            self.radius_bars = (points[1].0 - self.center_bar).abs().max(1.0);
            self.radius_price = (points[1].1 - self.center_price).abs().max(1.0);
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
            ControlPointType::Edge(0) => {
                // Top - adjust vertical radius
                self.radius_price = (price - self.center_price).abs().max(1.0);
            }
            ControlPointType::Edge(1) => {
                // Right - adjust horizontal radius
                self.radius_bars = (bar - self.center_bar).abs().max(1.0);
            }
            ControlPointType::Edge(2) => {
                // Bottom - adjust vertical radius
                self.radius_price = (self.center_price - price).abs().max(1.0);
            }
            ControlPointType::Edge(3) => {
                // Left - adjust horizontal radius
                self.radius_bars = (self.center_bar - bar).abs().max(1.0);
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

        // Calculate screen-space radii
        let rx = (viewport.bar_to_x_f64(self.center_bar + self.radius_bars) - cx).abs();
        let ry = (viewport.price_to_y(self.center_price + self.radius_price, price_scale.price_min, price_scale.price_max) - cy).abs();

        // Check edge control points (cross pattern: top, right, bottom, left)
        let edges = [
            (cx, cy - ry, 0), // top
            (cx + rx, cy, 1), // right
            (cx, cy + ry, 2), // bottom
            (cx - rx, cy, 3), // left
        ];
        for (ex, ey, idx) in edges {
            if check_point_hit(screen_x, screen_y, ex, ey) {
                return HitTestResult::ControlPoint(ControlPointType::Edge(idx));
            }
        }

        // Check center control point
        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }

        // Calculate normalized distance from center (for ellipse equation)
        if rx.abs() < 0.001 || ry.abs() < 0.001 {
            return HitTestResult::Miss;
        }
        let nx = (screen_x - cx) / rx;
        let ny = (screen_y - cy) / ry;
        let dist_sq = nx * nx + ny * ny;

        // Check if on ellipse edge (dist = 1)
        if (dist_sq.sqrt() - 1.0).abs() < HIT_TOLERANCE / rx.min(ry) {
            return HitTestResult::Body;
        }

        // Check if inside filled ellipse
        if self.fill && dist_sq <= 1.0 {
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

        // Calculate screen-space radii
        let rx = (viewport.bar_to_x_f64(self.center_bar + self.radius_bars) - cx).abs();
        let ry = (viewport.price_to_y(self.center_price + self.radius_price, price_scale.price_min, price_scale.price_max) - cy).abs();

        vec![
            // Center move point
            ControlPoint::move_point(cx, cy),
            // Edge points (cross pattern)
            ControlPoint::new(ControlPointType::Edge(0), cx, cy - ry, ControlPointCursor::ResizeNS),      // top
            ControlPoint::new(ControlPointType::Edge(1), cx + rx, cy, ControlPointCursor::ResizeEW),      // right
            ControlPoint::new(ControlPointType::Edge(2), cx, cy + ry, ControlPointCursor::ResizeNS),      // bottom
            ControlPoint::new(ControlPointType::Edge(3), cx - rx, cy, ControlPointCursor::ResizeEW),      // left
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let cx = ctx.bar_to_x(self.center_bar);
        let cy = ctx.price_to_y(self.center_price);

        // Calculate screen-space radii from data coordinates
        let rx = (ctx.bar_to_x(self.center_bar + self.radius_bars) - cx).abs();
        let ry = (ctx.price_to_y(self.center_price + self.radius_price) - cy).abs();

        // Fill if enabled
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
            // Draw bounding box
            ctx.set_stroke_color("#2196F3");
            ctx.set_stroke_width(1.5);
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.begin_path();
            ctx.move_to(cx - rx, cy - ry);
            ctx.line_to(cx + rx, cy - ry);
            ctx.line_to(cx + rx, cy + ry);
            ctx.line_to(cx - rx, cy + ry);
            ctx.close_path();
            ctx.stroke();
            ctx.set_line_dash(&[]);

            // Draw edge control handles
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            let edges = [
                (cx, cy - ry), // top
                (cx + rx, cy), // right
                (cx, cy + ry), // bottom
                (cx - rx, cy), // left
            ];
            for (ex, ey) in edges {
                ctx.begin_path();
                ctx.arc(ex, ey, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }

            // Draw center move handle
            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                // Bounding box of circle
                let min_x = cx - rx;
                let max_x = cx + rx;
                let min_y = cy - ry;
                let max_y = cy + ry;
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

fn create_circle(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (center_bar, center_price) = points.first().copied().unwrap_or((0.0, 0.0));
    if points.len() >= 2 {
        let radius_bars = (points[1].0 - center_bar).abs().max(1.0);
        let radius_price = (points[1].1 - center_price).abs().max(1.0);
        Box::new(Circle::new(center_bar, center_price, radius_bars, radius_price, color))
    } else {
        Box::new(Circle::new(center_bar, center_price, 10.0, center_price * 0.02, color))
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "circle",
        display_name: "Circle",
        kind: PrimitiveKind::Shape,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Draw a circle from center to edge",
        icon: "circle",
        default_color: "#4CAF50",
        factory: create_circle,
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
