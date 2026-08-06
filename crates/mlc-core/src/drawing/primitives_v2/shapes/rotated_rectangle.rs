//! Rotated Rectangle primitive

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    point_to_line_distance, HIT_TOLERANCE, CONTROL_POINT_HIT_RADIUS,
    RenderContext, crisp,
    render_primitive_text,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RotatedRectangle {
    pub data: PrimitiveData,
    pub center_ts: i64,
    pub center_price: f64,
    /// Point 2 — sets width AND rotation together: the screen-space
    /// distance and direction from center to this point are the
    /// rectangle's (rotated) horizontal half-axis.
    ///
    /// Stored as a raw point rather than a derived (half_width, rotation)
    /// pair. ms and price are not comparable units, so a Euclidean
    /// distance or an angle computed straight from `(edge_ts - center_ts,
    /// edge_price - center_price)` is meaningless — the real distance and
    /// angle only exist once both deltas are converted to pixels, which
    /// needs a viewport. Derived fresh in `render`/`hit_test`/
    /// `control_points` — the same pattern `Arc` uses for its sweep end
    /// point, and `FibArcs` uses for its baseline angle.
    pub edge_ts: i64,
    pub edge_price: f64,
    /// Point 3 — sets the perpendicular half-height: its screen-space
    /// distance from the rotated width axis (not from center directly).
    pub height_ts: i64,
    pub height_price: f64,
    #[serde(default = "default_true")] pub fill: bool,
    #[serde(default = "default_fill_opacity")] pub fill_opacity: f64,
}

fn default_true() -> bool { true }
fn default_fill_opacity() -> f64 { 0.2 }

impl RotatedRectangle {
    pub fn new(center_ts: i64, center_price: f64, edge_ts: i64, edge_price: f64, height_ts: i64, height_price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "rotated_rectangle".to_string(),
                display_name: "Rotated Rectangle".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            center_ts, center_price, edge_ts, edge_price, height_ts, height_price,
            fill: true, fill_opacity: 0.2,
        }
    }

    /// Screen-space center + 4 rotated corners, from bars/viewport/price_scale
    /// (the `hit_test`/`control_points` path). `render` computes the same
    /// thing from a `RenderContext` directly — see `rotated_geometry` /
    /// `rotated_corners` below, which both share.
    fn screen_layout(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> (f64, f64, [(f64, f64); 4]) {
        let cx = viewport.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.center_ts));
        let cy = viewport.price_to_y(self.center_price, price_scale.price_min, price_scale.price_max);
        let ex = viewport.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.edge_ts));
        let ey = viewport.price_to_y(self.edge_price, price_scale.price_min, price_scale.price_max);
        let hx = viewport.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.height_ts));
        let hy = viewport.price_to_y(self.height_price, price_scale.price_min, price_scale.price_max);
        let (half_width, half_height, rotation) = rotated_geometry(ex - cx, ey - cy, hx - cx, hy - cy);
        (cx, cy, rotated_corners(cx, cy, half_width, half_height, rotation))
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
        vec![
            (self.center_ts, self.center_price),
            (self.edge_ts, self.edge_price),
            (self.height_ts, self.height_price),
        ]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if !points.is_empty() {
            self.center_ts = points[0].0;
            self.center_price = points[0].1;
        }
        if points.len() >= 2 {
            self.edge_ts = points[1].0;
            self.edge_price = points[1].1;
        }
        if points.len() >= 3 {
            self.height_ts = points[2].0;
            self.height_price = points[2].1;
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        // All three anchors are now absolute points (not center-relative
        // offsets), so a whole-shape move has to carry every one of them.
        self.center_ts += ts_delta_ms;
        self.center_price += price_delta;
        self.edge_ts += ts_delta_ms;
        self.edge_price += price_delta;
        self.height_ts += ts_delta_ms;
        self.height_price += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        match point_type {
            ControlPointType::Move => {
                let ts_delta = ts_ms - self.center_ts;
                let price_delta = price - self.center_price;
                self.translate(ts_delta, price_delta);
            }
            ControlPointType::Point2 => {
                self.edge_ts = ts_ms;
                self.edge_price = price;
            }
            ControlPointType::Point3 => {
                self.height_ts = ts_ms;
                self.height_price = price;
            }
            _ => {}
        }
    }

    fn hit_test(&self, screen_x: f64, screen_y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> HitTestResult {
        let (cx, cy, screen_corners) = self.screen_layout(bars, viewport, price_scale);

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
        let (cx, cy, screen_corners) = self.screen_layout(bars, viewport, price_scale);
        vec![
            ControlPoint::move_point(cx, cy),
            ControlPoint::with_type(ControlPointType::Corner(0), screen_corners[0].0, screen_corners[0].1),
            ControlPoint::with_type(ControlPointType::Corner(1), screen_corners[1].0, screen_corners[1].1),
            ControlPoint::with_type(ControlPointType::Corner(2), screen_corners[2].0, screen_corners[2].1),
            ControlPoint::with_type(ControlPointType::Corner(3), screen_corners[3].0, screen_corners[3].1),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let cx = ctx.ts_to_x_ms(self.center_ts);
        let cy = ctx.price_to_y(self.center_price);
        let ex = ctx.ts_to_x_ms(self.edge_ts);
        let ey = ctx.price_to_y(self.edge_price);
        let hx = ctx.ts_to_x_ms(self.height_ts);
        let hy = ctx.price_to_y(self.height_price);
        let (half_width, half_height, rotation) = rotated_geometry(ex - cx, ey - cy, hx - cx, hy - cy);
        let screen_corners = rotated_corners(cx, cy, half_width, half_height, rotation);

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

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let min_x = screen_corners.iter().map(|(x, _)| *x).fold(f64::INFINITY, f64::min);
                let max_x = screen_corners.iter().map(|(x, _)| *x).fold(f64::NEG_INFINITY, f64::max);
                let min_y = screen_corners.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
                let max_y = screen_corners.iter().map(|(_, y)| *y).fold(f64::NEG_INFINITY, f64::max);
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

/// Derive (half_width, half_height, rotation) — all screen-space — from two
/// raw anchor points' pixel deltas from center.
///
/// This is the one place allowed to combine an ms-axis delta with a
/// price-axis delta into a single angle or a single distance, because by
/// the time this runs both deltas have already been converted to pixels by
/// the caller (`render` via `RenderContext`, `hit_test`/`control_points`
/// via `Viewport`/`PriceScale`). Never call this with raw (ms, price)
/// deltas directly.
fn rotated_geometry(edge_dx: f64, edge_dy: f64, height_dx: f64, height_dy: f64) -> (f64, f64, f64) {
    let half_width = (edge_dx * edge_dx + edge_dy * edge_dy).sqrt().max(1.0);
    let rotation = edge_dy.atan2(edge_dx);
    // Inverse-rotate point 3's delta by -rotation (rotation matrices are
    // orthogonal, so the inverse is just the transpose); the local Y
    // component is its perpendicular distance from the rotated width axis.
    let half_height = (height_dy * rotation.cos() - height_dx * rotation.sin()).abs().max(1.0);
    (half_width, half_height, rotation)
}

/// The 4 rectangle corners in screen space, from a pixel center, pixel
/// half-extents and a rotation. A standard pixel-space rotation matrix —
/// valid here because every input is already in pixels, unlike the old
/// `corner_ts_prices()` which applied this same matrix to raw (ms, price)
/// pairs.
fn rotated_corners(cx: f64, cy: f64, half_width: f64, half_height: f64, rotation: f64) -> [(f64, f64); 4] {
    let cos_r = rotation.cos();
    let sin_r = rotation.sin();
    let local = [(-half_width, -half_height), (half_width, -half_height), (half_width, half_height), (-half_width, half_height)];
    let mut corners = [(0.0, 0.0); 4];
    for (i, (lx, ly)) in local.iter().enumerate() {
        corners[i] = (cx + lx * cos_r - ly * sin_r, cy + lx * sin_r + ly * cos_r);
    }
    corners
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
    let (center_ts, center_price) = points.first().copied().unwrap_or((0, 100.0));
    let (edge_ts, edge_price) = points.get(1).copied().unwrap_or((center_ts + 600_000, center_price));
    let (height_ts, height_price) = points.get(2).copied()
        .unwrap_or((center_ts, center_price + center_price.abs() * 0.03 + 1.0));
    Box::new(RotatedRectangle::new(center_ts, center_price, edge_ts, edge_price, height_ts, height_price, color))
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
