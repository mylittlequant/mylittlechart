//! Arc primitive

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    HIT_TOLERANCE, CONTROL_POINT_HIT_RADIUS,
    RenderContext,
    render_primitive_text,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Arc {
    pub data: PrimitiveData,
    pub center_ts: i64,
    pub center_price: f64,
    /// Horizontal radius as ms duration
    pub radius_ms: i64,
    pub start_angle: f64,
    /// Point 3 — where the sweep ends, in data coordinates.
    ///
    /// Stored as a raw point rather than a derived angle. ms and price are
    /// not comparable units, so `(end_price - center_price).atan2(end_ts -
    /// center_ts)` collapses to ~0deg/~360deg on every click (the ms delta,
    /// ~1e7, totally dominates the price delta, ~1e2). The real sweep angle
    /// only exists once both deltas are converted to pixels, which needs a
    /// viewport — so it is derived fresh in `render`/`hit_test`/
    /// `control_points`, the same pattern `FibArcs` already uses for its
    /// baseline angle. `create_arc`, `set_points` and `move_control_point`
    /// just store the point; they never compute an angle at all.
    pub end_ts: i64,
    pub end_price: f64,
}

impl Arc {
    pub fn new(center_ts: i64, center_price: f64, radius_ms: i64, start_angle: f64, end_ts: i64, end_price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "arc".to_string(),
                display_name: "Arc".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            center_ts, center_price, radius_ms: radius_ms.max(1), start_angle, end_ts, end_price,
        }
    }

    /// Screen-space (center_x, center_y, radius, end_angle_rad) from
    /// bars/viewport/price_scale — the `hit_test`/`control_points` path.
    /// `render` computes the same thing from a `RenderContext` directly.
    fn screen_geometry(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> (f64, f64, f64, f64) {
        let center_bar = timestamp_ms_to_bar_f64(bars, self.center_ts);
        let edge_bar = timestamp_ms_to_bar_f64(bars, self.center_ts + self.radius_ms);
        let end_bar = timestamp_ms_to_bar_f64(bars, self.end_ts);
        let cx = viewport.bar_to_x_f64(center_bar);
        let cy = viewport.price_to_y(self.center_price, price_scale.price_min, price_scale.price_max);
        let radius = (viewport.bar_to_x_f64(edge_bar) - cx).abs();
        let ex = viewport.bar_to_x_f64(end_bar);
        let ey = viewport.price_to_y(self.end_price, price_scale.price_min, price_scale.price_max);
        let end_rad = (ey - cy).atan2(ex - cx);
        (cx, cy, radius, end_rad)
    }
}

impl Primitive for Arc {
    fn type_id(&self) -> &'static str { "arc" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Shape }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::ThreePoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> {
        let start_rad = self.start_angle.to_radians();
        vec![
            (self.center_ts, self.center_price),
            (self.center_ts + (self.radius_ms as f64 * start_rad.cos()) as i64, self.center_price),
            (self.end_ts, self.end_price),
        ]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if !points.is_empty() {
            self.center_ts = points[0].0;
            self.center_price = points[0].1;
        }
        if points.len() >= 2 {
            self.radius_ms = (points[1].0 - self.center_ts).abs().max(1);
            // point2 only carries the radius (no angle info) — leave
            // start_angle alone. This used to unconditionally zero it, so
            // editing the radius from the Coordinates tab silently threw
            // away whatever sweep the user had set.
        }
        if points.len() >= 3 {
            self.end_ts = points[2].0;
            self.end_price = points[2].1;
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        self.center_ts += ts_delta_ms;
        self.center_price += price_delta;
        self.end_ts += ts_delta_ms;
        self.end_price += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        match point_type {
            ControlPointType::Point1 | ControlPointType::Move => {
                let ts_delta = ts_ms - self.center_ts;
                let price_delta = price - self.center_price;
                self.translate(ts_delta, price_delta);
            }
            ControlPointType::Point2 => {
                self.radius_ms = (ts_ms - self.center_ts).abs().max(1);
            }
            ControlPointType::Point3 => {
                self.end_ts = ts_ms;
                self.end_price = price;
            }
            _ => {}
        }
    }

    fn hit_test(&self, screen_x: f64, screen_y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> HitTestResult {
        let (cx, cy, radius, end_rad) = self.screen_geometry(bars, viewport, price_scale);

        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }

        // Point 2 (radius) and point 3 (sweep end) both sit ON the arc
        // stroke by construction (control_points() places them exactly on
        // the circle of `radius`), so they must be tested BEFORE the
        // on-stroke body check below — otherwise the body test always wins
        // and the handle can never be picked up, only the whole arc.
        let start_rad = self.start_angle.to_radians();
        let p2x = cx + radius * start_rad.cos();
        let p2y = cy + radius * start_rad.sin();
        if check_point_hit(screen_x, screen_y, p2x, p2y) {
            return HitTestResult::ControlPoint(ControlPointType::Point2);
        }
        let p3x = cx + radius * end_rad.cos();
        let p3y = cy + radius * end_rad.sin();
        if check_point_hit(screen_x, screen_y, p3x, p3y) {
            return HitTestResult::ControlPoint(ControlPointType::Point3);
        }

        let dx = screen_x - cx;
        let dy = screen_y - cy;
        let dist = (dx * dx + dy * dy).sqrt();
        let angle = dy.atan2(dx).to_degrees();
        let angle_norm = if angle < 0.0 { angle + 360.0 } else { angle };

        let start_deg = self.start_angle;
        let end_deg_raw = end_rad.to_degrees();
        let end_deg = if end_deg_raw < 0.0 { end_deg_raw + 360.0 } else { end_deg_raw };

        let on_arc = (dist - radius).abs() < HIT_TOLERANCE;
        let in_angle_range = if start_deg <= end_deg {
            angle_norm >= start_deg && angle_norm <= end_deg
        } else {
            angle_norm >= start_deg || angle_norm <= end_deg
        };

        if on_arc && in_angle_range { return HitTestResult::Body; }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        let (cx, cy, radius, end_rad) = self.screen_geometry(bars, viewport, price_scale);
        let start_rad = self.start_angle.to_radians();

        vec![
            ControlPoint::move_point(cx, cy),
            ControlPoint::point2(cx + radius * start_rad.cos(), cy + radius * start_rad.sin()),
            ControlPoint::point3(cx + radius * end_rad.cos(), cy + radius * end_rad.sin()),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let cx = ctx.ts_to_x_ms(self.center_ts);
        let cy = ctx.price_to_y(self.center_price);
        let rx = ctx.ts_to_x_ms(self.center_ts + self.radius_ms);
        let radius = (rx - cx).abs();

        let start_rad = self.start_angle.to_radians();
        // The sweep end is derived fresh from the stored raw point, in
        // screen space — see the `end_ts`/`end_price` doc comment on the
        // struct for why an angle can't be stored or derived from (ms,
        // price) directly.
        let ex = ctx.ts_to_x_ms(self.end_ts);
        let ey = ctx.price_to_y(self.end_price);
        let end_rad = (ey - cy).atan2(ex - cx);

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

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => cx - radius,
                    super::super::TextAlign::Center => cx,
                    super::super::TextAlign::End => cx + radius,
                };
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => cy - radius - text_offset,
                    super::super::TextAlign::Center => cy,
                    super::super::TextAlign::End => cy + radius + text_offset,
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

fn create_arc(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (center_ts, center_price) = points.first().copied().unwrap_or((0, 100.0));
    let radius_ms = if points.len() >= 2 {
        (points[1].0 - center_ts).abs().max(1)
    } else {
        600_000
    };
    // No angle math here — see the struct-level doc comment on `end_ts`.
    // The factory only ever sees data points, never a viewport, so it just
    // stores whatever point 3 was (or a sane default while only 1-2 points
    // have been clicked so far); the real sweep angle is derived later,
    // in screen space, wherever a viewport exists.
    let (end_ts, end_price) = points.get(2).copied().unwrap_or((center_ts + radius_ms, center_price));
    Box::new(Arc::new(center_ts, center_price, radius_ms, 0.0, end_ts, end_price, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "arc", display_name: "Arc", kind: PrimitiveKind::Shape,
        click_behavior: ClickBehavior::ThreePoint,
        tooltip: "Draw an arc (center, start, end)",
        icon: "arc", default_color: "#E91E63",
        factory: create_arc,
        supports_text: true, has_levels: false,
    }
}
