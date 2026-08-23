//! Image primitive - embedded image
//!
//! Uses 5 data-coordinate points: center + 4 edge points (top, right, bottom, left)

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, Archetype, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, ControlPointCursor, PrimitiveColor,
    RenderContext, crisp, CONTROL_POINT_RADIUS,
};

/// Image primitive with 5 data-coordinate anchor points
///
/// Points are stored as:
/// - center_ts, center_price: Center point (ms timestamp, price)
/// - radius_ms: Horizontal half-size in milliseconds
/// - radius_price: Vertical half-size in price units
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Image {
    pub data: PrimitiveData,
    /// Center timestamp (ms)
    pub center_ts: i64,
    /// Center price
    pub center_price: f64,
    /// Horizontal radius in milliseconds (distance from center to left/right edge)
    #[serde(default = "default_radius_ms")]
    pub radius_ms: i64,
    /// Vertical radius in price units (distance from center to top/bottom edge)
    pub radius_price: f64,
    /// Image URL (data URL or http URL)
    pub url: String,
}

fn default_radius_ms() -> i64 { 300_000 }
fn default_radius_price() -> f64 { 100.0 }

impl Image {
    pub fn new(center_ts: i64, center_price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "image".to_string(),
                display_name: "Image".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            center_ts,
            center_price,
            radius_ms: default_radius_ms(),
            radius_price: default_radius_price(),
            url: String::new(),
        }
    }

    /// Create from center and edge point
    pub fn from_points(center_ts: i64, center_price: f64, edge_ts: i64, edge_price: f64, color: &str) -> Self {
        let radius_ms = (edge_ts - center_ts).abs().max(60_000);
        let radius_price = (edge_price - center_price).abs().max(1.0);
        let mut image = Self::new(center_ts, center_price, color);
        image.radius_ms = radius_ms;
        image.radius_price = radius_price;
        image
    }
}

impl Primitive for Image {
    fn type_id(&self) -> &'static str { "image" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Annotation }
    // A user-picked image marker, not a textual annotation — `Icon`
    // archetype, not the majority `Annotation`->`Text` default.
    fn archetype(&self) -> Archetype { Archetype::Icon }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::SingleClick }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    /// Returns 2 points: center and corner (for TwoPoint behavior)
    fn points(&self) -> Vec<(i64, f64)> {
        vec![
            (self.center_ts, self.center_price),
            (self.center_ts + self.radius_ms, self.center_price + self.radius_price),
        ]
    }

    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(ts, p)) = pts.first() {
            self.center_ts = ts;
            self.center_price = p;
        }
        // Second point defines the corner (for TwoPoint creation)
        if let Some(&(ts2, p2)) = pts.get(1) {
            self.radius_ms = (ts2 - self.center_ts).abs().max(60_000);
            self.radius_price = (p2 - self.center_price).abs().max(1.0);
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, pd: f64) {
        self.center_ts += ts_delta_ms;
        self.center_price += pd;
    }

    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Move => {
                self.center_ts = ts_ms;
                self.center_price = price;
            }
            // Edge points (cross pattern) - resize one dimension
            ControlPointType::Edge(0) => {
                // Top - adjust vertical radius (price)
                self.radius_price = (price - self.center_price).abs().max(1.0);
            }
            ControlPointType::Edge(1) => {
                // Right - adjust horizontal radius (ms)
                self.radius_ms = (ts_ms - self.center_ts).abs().max(60_000);
            }
            ControlPointType::Edge(2) => {
                // Bottom - adjust vertical radius
                self.radius_price = (self.center_price - price).abs().max(1.0);
            }
            ControlPointType::Edge(3) => {
                // Left - adjust horizontal radius
                self.radius_ms = (self.center_ts - ts_ms).abs().max(60_000);
            }
            // Corner points - resize both dimensions
            ControlPointType::Edge(4) => {
                // Top-left corner
                self.radius_ms = (self.center_ts - ts_ms).abs().max(60_000);
                self.radius_price = (price - self.center_price).abs().max(1.0);
            }
            ControlPointType::Edge(5) => {
                // Top-right corner
                self.radius_ms = (ts_ms - self.center_ts).abs().max(60_000);
                self.radius_price = (price - self.center_price).abs().max(1.0);
            }
            ControlPointType::Edge(6) => {
                // Bottom-right corner
                self.radius_ms = (ts_ms - self.center_ts).abs().max(60_000);
                self.radius_price = (self.center_price - price).abs().max(1.0);
            }
            ControlPointType::Edge(7) => {
                // Bottom-left corner
                self.radius_ms = (self.center_ts - ts_ms).abs().max(60_000);
                self.radius_price = (self.center_price - price).abs().max(1.0);
            }
            _ => {}
        }
    }

    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let center_b = timestamp_ms_to_bar_f64(bars, self.center_ts);
        let cx = vp.bar_to_x_f64(center_b);
        let cy = vp.price_to_y(self.center_price, ps.price_min, ps.price_max);

        // Calculate screen-space radii
        let edge_b = timestamp_ms_to_bar_f64(bars, self.center_ts + self.radius_ms);
        let rx = (vp.bar_to_x_f64(edge_b) - cx).abs();
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

    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let center_b = timestamp_ms_to_bar_f64(bars, self.center_ts);
        let cx = vp.bar_to_x_f64(center_b);
        let cy = vp.price_to_y(self.center_price, ps.price_min, ps.price_max);

        // Calculate screen-space radii
        let edge_b = timestamp_ms_to_bar_f64(bars, self.center_ts + self.radius_ms);
        let rx = (vp.bar_to_x_f64(edge_b) - cx).abs();
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

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let cx = ctx.ts_to_x_ms(self.center_ts);
        let cy = ctx.price_to_y(self.center_price);

        // Calculate screen-space half-sizes from data coordinates
        let half_w = (ctx.ts_to_x_ms(self.center_ts + self.radius_ms) - cx).abs();
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
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "image", display_name: "Image", kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::SingleClick, tooltip: "Embedded image", icon: "image", default_color: "#607D8B",
        // Same shape as `emoji::metadata()`'s factory: derive the size from
        // a second point when the caller supplies one, falling back to the
        // struct's own defaults otherwise. `Image::new` alone (the previous
        // body here) ignored `points[1..]` entirely and always used the
        // hardcoded defaults — on a click_behavior::SingleClick primitive
        // that matches production (one click, no second point), but it also
        // meant every non-single-click caller (round-trip/hit-test tests,
        // which construct from the full fixture) got a size that was never
        // derived from anything about the chart it draws on. With hourly
        // bars, `default_radius_ms` (5 minutes) is smaller than one bar
        // interval, and `timestamp_ms_to_bar_f64` resolves any sub-bar
        // offset to the SAME bar index — so the horizontal radius collapsed
        // to exactly 0px and every edge/corner handle pair landed on top of
        // each other. Routing through `from_points` (already defined on
        // `Image`, previously dead code — nothing called it) fixes that for
        // any 2-point construction while leaving the real single-click path
        // byte-identical: with no second point this resolves to
        // `(ts1 + default_radius_ms(), p1 + default_radius_price())`, the
        // exact deltas `Image::new` used to hardcode.
        factory: |points, color| {
            let (ts1, p1) = points.first().copied().unwrap_or((0, 100.0));
            let (ts2, p2) = points.get(1).copied()
                .unwrap_or((ts1 + default_radius_ms(), p1 + default_radius_price()));
            Box::new(Image::from_points(ts1, p1, ts2, p2, color))
        },
        supports_text: false,
        has_levels: false,
    }
}
