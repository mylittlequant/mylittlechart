//! Image primitive - embedded image
//!
//! Uses 5 data-coordinate points: center + 4 edge points (top, right, bottom, left)

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, ControlPointCursor, PrimitiveColor,
    RenderContext, crisp, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
};

/// Image primitive with 5 data-coordinate anchor points
///
/// Points are stored as:
/// - center_bar, center_price: Center point
/// - radius_bars: Horizontal half-size in bars
/// - radius_price: Vertical half-size in price units
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Image {
    pub data: PrimitiveData,
    /// Center bar
    pub center_bar: f64,
    /// Center price
    pub center_price: f64,
    /// Horizontal radius in bars (distance from center to left/right edge)
    pub radius_bars: f64,
    /// Vertical radius in price units (distance from center to top/bottom edge)
    pub radius_price: f64,
    /// Image URL (data URL or http URL)
    pub url: String,
}

fn default_radius_bars() -> f64 { 5.0 }
fn default_radius_price() -> f64 { 100.0 }

impl Image {
    pub fn new(bar: f64, price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "image".to_string(),
                display_name: "Image".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            center_bar: bar,
            center_price: price,
            radius_bars: default_radius_bars(),
            radius_price: default_radius_price(),
            url: String::new(),
        }
    }

    /// Create from center and edge point
    pub fn from_points(center_bar: f64, center_price: f64, edge_bar: f64, edge_price: f64, color: &str) -> Self {
        let radius_bars = (edge_bar - center_bar).abs().max(1.0);
        let radius_price = (edge_price - center_price).abs().max(1.0);
        let mut image = Self::new(center_bar, center_price, color);
        image.radius_bars = radius_bars;
        image.radius_price = radius_price;
        image
    }
}

impl Primitive for Image {
    fn type_id(&self) -> &'static str { "image" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Annotation }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::SingleClick }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    /// Returns 2 points: center and corner (for TwoPoint behavior)
    fn points(&self) -> Vec<(f64, f64)> {
        vec![
            (self.center_bar, self.center_price),
            (self.center_bar + self.radius_bars, self.center_price + self.radius_price),
        ]
    }

    fn set_points(&mut self, pts: &[(f64, f64)]) {
        if let Some(&(b, p)) = pts.first() {
            self.center_bar = b;
            self.center_price = p;
        }
        // Second point defines the corner (for TwoPoint creation)
        if let Some(&(b2, p2)) = pts.get(1) {
            self.radius_bars = (b2 - self.center_bar).abs().max(0.5);
            self.radius_price = (p2 - self.center_price).abs().max(1.0);
        }
    }

    fn translate(&mut self, bd: f64, pd: f64) {
        self.center_bar += bd;
        self.center_price += pd;
    }

    fn move_control_point(&mut self, pt: ControlPointType, bar: f64, price: f64) {
        match pt {
            ControlPointType::Move => {
                self.center_bar = bar;
                self.center_price = price;
            }
            // Edge points (cross pattern) - resize one dimension
            ControlPointType::Edge(0) => {
                // Top - adjust vertical radius (price)
                self.radius_price = (price - self.center_price).abs().max(1.0);
            }
            ControlPointType::Edge(1) => {
                // Right - adjust horizontal radius (bars)
                self.radius_bars = (bar - self.center_bar).abs().max(0.5);
            }
            ControlPointType::Edge(2) => {
                // Bottom - adjust vertical radius
                self.radius_price = (self.center_price - price).abs().max(1.0);
            }
            ControlPointType::Edge(3) => {
                // Left - adjust horizontal radius
                self.radius_bars = (self.center_bar - bar).abs().max(0.5);
            }
            // Corner points - resize both dimensions
            ControlPointType::Edge(4) => {
                // Top-left corner
                self.radius_bars = (self.center_bar - bar).abs().max(0.5);
                self.radius_price = (price - self.center_price).abs().max(1.0);
            }
            ControlPointType::Edge(5) => {
                // Top-right corner
                self.radius_bars = (bar - self.center_bar).abs().max(0.5);
                self.radius_price = (price - self.center_price).abs().max(1.0);
            }
            ControlPointType::Edge(6) => {
                // Bottom-right corner
                self.radius_bars = (bar - self.center_bar).abs().max(0.5);
                self.radius_price = (self.center_price - price).abs().max(1.0);
            }
            ControlPointType::Edge(7) => {
                // Bottom-left corner
                self.radius_bars = (self.center_bar - bar).abs().max(0.5);
                self.radius_price = (self.center_price - price).abs().max(1.0);
            }
            _ => {}
        }
    }

    fn hit_test(&self, sx: f64, sy: f64, vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let cx = vp.bar_to_x_f64(self.center_bar);
        let cy = vp.price_to_y(self.center_price, ps.price_min, ps.price_max);

        // Calculate screen-space radii
        let rx = (vp.bar_to_x_f64(self.center_bar + self.radius_bars) - cx).abs();
        let ry = (vp.price_to_y(self.center_price + self.radius_price, ps.price_min, ps.price_max) - cy).abs();

        let hit_radius = CONTROL_POINT_RADIUS + 4.0;

        // Check corner control points first (higher priority for diagonal resize)
        let corners = [
            (cx - rx, cy - ry, 4), // top-left
            (cx + rx, cy - ry, 5), // top-right
            (cx + rx, cy + ry, 6), // bottom-right
            (cx - rx, cy + ry, 7), // bottom-left
        ];
        for (ex, ey, idx) in corners {
            if ((sx - ex).powi(2) + (sy - ey).powi(2)).sqrt() < hit_radius {
                return HitTestResult::ControlPoint(ControlPointType::Edge(idx));
            }
        }

        // Check edge control points (cross pattern: top, right, bottom, left)
        let edges = [
            (cx, cy - ry, 0), // top
            (cx + rx, cy, 1), // right
            (cx, cy + ry, 2), // bottom
            (cx - rx, cy, 3), // left
        ];
        for (ex, ey, idx) in edges {
            if ((sx - ex).powi(2) + (sy - ey).powi(2)).sqrt() < hit_radius {
                return HitTestResult::ControlPoint(ControlPointType::Edge(idx));
            }
        }

        // Check center (move point)
        if ((sx - cx).powi(2) + (sy - cy).powi(2)).sqrt() < hit_radius {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }

        // Check body (bounding box)
        if sx >= cx - rx && sx <= cx + rx && sy >= cy - ry && sy <= cy + ry {
            return HitTestResult::Body;
        }

        HitTestResult::Miss
    }

    fn control_points(&self, vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let cx = vp.bar_to_x_f64(self.center_bar);
        let cy = vp.price_to_y(self.center_price, ps.price_min, ps.price_max);

        // Calculate screen-space radii
        let rx = (vp.bar_to_x_f64(self.center_bar + self.radius_bars) - cx).abs();
        let ry = (vp.price_to_y(self.center_price + self.radius_price, ps.price_min, ps.price_max) - cy).abs();

        vec![
            // Center move point
            ControlPoint::move_point(cx, cy),
            // Edge points (cross pattern)
            ControlPoint::new(ControlPointType::Edge(0), cx, cy - ry, ControlPointCursor::ResizeNS),      // top
            ControlPoint::new(ControlPointType::Edge(1), cx + rx, cy, ControlPointCursor::ResizeEW),      // right
            ControlPoint::new(ControlPointType::Edge(2), cx, cy + ry, ControlPointCursor::ResizeNS),      // bottom
            ControlPoint::new(ControlPointType::Edge(3), cx - rx, cy, ControlPointCursor::ResizeEW),      // left
            // Corner points (diagonal resize)
            ControlPoint::new(ControlPointType::Edge(4), cx - rx, cy - ry, ControlPointCursor::ResizeNWSE), // top-left
            ControlPoint::new(ControlPointType::Edge(5), cx + rx, cy - ry, ControlPointCursor::ResizeNESW), // top-right
            ControlPoint::new(ControlPointType::Edge(6), cx + rx, cy + ry, ControlPointCursor::ResizeNWSE), // bottom-right
            ControlPoint::new(ControlPointType::Edge(7), cx - rx, cy + ry, ControlPointCursor::ResizeNESW), // bottom-left
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let cx = ctx.bar_to_x(self.center_bar);
        let cy = ctx.price_to_y(self.center_price);

        // Calculate screen-space half-sizes from data coordinates
        let half_w = (ctx.bar_to_x(self.center_bar + self.radius_bars) - cx).abs();
        let half_h = (ctx.price_to_y(self.center_price + self.radius_price) - cy).abs();

        // Top-left corner for image drawing
        let img_x = cx - half_w;
        let img_y = cy - half_h;
        let img_w = half_w * 2.0;
        let img_h = half_h * 2.0;

        // URL-based image drawing requires the opt-in ImagePainter capability.
        // Chart's RenderContext does not bind ImagePainter, so always fall through
        // to the placeholder rendering below.
        let image_drawn = false;

        // Draw placeholder if image not loaded or no URL
        if !image_drawn {
            ctx.set_stroke_color(&self.data.color.stroke);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rect(
                crisp(img_x, dpr),
                crisp(img_y, dpr),
                img_w,
                img_h
            );

            // Draw X through the rectangle to indicate image placeholder
            ctx.begin_path();
            ctx.move_to(crisp(img_x, dpr), crisp(img_y, dpr));
            ctx.line_to(crisp(img_x + img_w, dpr), crisp(img_y + img_h, dpr));
            ctx.move_to(crisp(img_x + img_w, dpr), crisp(img_y, dpr));
            ctx.line_to(crisp(img_x, dpr), crisp(img_y + img_h, dpr));
            ctx.stroke();
        }

        // Draw control points if selected
        if is_selected {
            // Draw bounding box
            ctx.set_stroke_color("#2196F3");
            ctx.set_stroke_width(1.5);
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.begin_path();
            ctx.move_to(cx - half_w, cy - half_h);
            ctx.line_to(cx + half_w, cy - half_h);
            ctx.line_to(cx + half_w, cy + half_h);
            ctx.line_to(cx - half_w, cy + half_h);
            ctx.close_path();
            ctx.stroke();
            ctx.set_line_dash(&[]);

            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            // Draw corner control handles (diagonal resize)
            let corners = [
                (cx - half_w, cy - half_h), // top-left
                (cx + half_w, cy - half_h), // top-right
                (cx + half_w, cy + half_h), // bottom-right
                (cx - half_w, cy + half_h), // bottom-left
            ];
            for (ex, ey) in corners {
                ctx.begin_path();
                ctx.arc(ex, ey, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }

            // Draw edge control handles (cross pattern)
            let edges = [
                (cx, cy - half_h), // top
                (cx + half_w, cy), // right
                (cx, cy + half_h), // bottom
                (cx - half_w, cy), // left
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
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "image", display_name: "Image", kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::SingleClick, tooltip: "Embedded image", icon: "image", default_color: "#607D8B",
        factory: |points, color| {
            let (b, p) = points.first().copied().unwrap_or((0.0, 100.0));
            Box::new(Image::new(b, p, color))
        },
        supports_text: false,
        has_levels: false,
        has_points_config: false,
    }
}
