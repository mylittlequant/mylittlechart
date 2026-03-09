//! Time Cycles - circular time cycles

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeCycles {
    pub data: PrimitiveData,
    pub bar: f64,
    pub price: f64,
    pub radius_bars: f64,
    /// Vertical radius in price units (for proper ellipse behavior)
    #[serde(default = "default_radius_price")]
    pub radius_price: f64,
    #[serde(default = "default_count")] pub count: u8,
}
fn default_count() -> u8 { 5 }
fn default_radius_price() -> f64 { 0.0 }

impl TimeCycles {
    pub fn new(bar: f64, price: f64, radius_bars: f64, radius_price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "time_cycles".to_string(), display_name: "Time Cycles".to_string(), color: PrimitiveColor::new(color), width: 1.0, ..Default::default() },
            bar, price, radius_bars, radius_price, count: 5,
        }
    }
}

impl Primitive for TimeCycles {
    fn type_id(&self) -> &'static str { "time_cycles" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Measurement }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(f64, f64)> {
        vec![
            (self.bar, self.price),
            (self.bar + self.radius_bars, self.price + self.radius_price)
        ]
    }
    fn set_points(&mut self, pts: &[(f64, f64)]) {
        if let Some(&(b, p)) = pts.first() { self.bar = b; self.price = p; }
        if let Some(&(b, p)) = pts.get(1) {
            self.radius_bars = (b - self.bar).abs();
            self.radius_price = (p - self.price).abs();
        }
    }
    fn translate(&mut self, bd: f64, pd: f64) { self.bar += bd; self.price += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, bar: f64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.bar = bar; self.price = price; }
            ControlPointType::Point2 => {
                self.radius_bars = (bar - self.bar).abs();
                self.radius_price = (price - self.price).abs();
            }
            ControlPointType::Move => { let bd = bar - self.bar; let pd = price - self.price; self.translate(bd, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let cx = vp.bar_to_x_f64(self.bar);
        let cy = vp.price_to_y(self.price, ps.price_min, ps.price_max);
        let r = 8.0;
        if (sx - cx).powi(2) + (sy - cy).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }

        // Calculate screen-space radii
        let base_rx = (vp.bar_to_x_f64(self.bar + self.radius_bars) - cx).abs();
        let base_ry = (vp.price_to_y(self.price + self.radius_price, ps.price_min, ps.price_max) - cy).abs();

        // Control point 2 at edge
        let p2x = cx + base_rx;
        let p2y = cy + base_ry;
        if (sx - p2x).powi(2) + (sy - p2y).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }

        // Check ellipses (normalized distance)
        if base_rx > 0.001 && base_ry > 0.001 {
            for i in 1..=self.count {
                let rx = base_rx * i as f64;
                let ry = base_ry * i as f64;
                let nx = (sx - cx) / rx;
                let ny = (sy - cy) / ry;
                let dist = (nx * nx + ny * ny).sqrt();
                if (dist - 1.0).abs() < HIT_TOLERANCE / rx.min(ry) { return HitTestResult::Body; }
            }
        }
        HitTestResult::Miss
    }
    fn control_points(&self, vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let cx = vp.bar_to_x_f64(self.bar);
        let cy = vp.price_to_y(self.price, ps.price_min, ps.price_max);
        let base_rx = (vp.bar_to_x_f64(self.bar + self.radius_bars) - cx).abs();
        let base_ry = (vp.price_to_y(self.price + self.radius_price, ps.price_min, ps.price_max) - cy).abs();
        vec![ControlPoint::point1(cx, cy), ControlPoint::point2(cx + base_rx, cy + base_ry)]
    }
    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let cx = ctx.bar_to_x(self.bar);
        let cy = ctx.price_to_y(self.price);

        // Calculate screen-space radii from data coordinates
        let edge_x = ctx.bar_to_x(self.bar + self.radius_bars);
        let edge_y = ctx.price_to_y(self.price + self.radius_price);
        let base_rx = (edge_x - cx).abs();
        let base_ry = (edge_y - cy).abs();

        if base_rx < 0.1 && base_ry < 0.1 {
            return; // Radii too small to render
        }

        // Draw concentric ellipses
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);

        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[5.0, 5.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 3.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        for i in 1..=self.count {
            let rx = base_rx * (i as f64);
            let ry = base_ry * (i as f64);
            ctx.begin_path();
            ctx.ellipse(crisp(cx, dpr), crisp(cy, dpr), rx, ry, 0.0, 0.0, std::f64::consts::TAU);
            ctx.stroke();
        }

        // Draw vertical line at center to show time axis
        ctx.set_line_dash(&[3.0, 3.0]);
        let chart_top = 0.0;
        let chart_bottom = ctx.canvas_height();
        ctx.begin_path();
        ctx.move_to(crisp(cx, dpr), crisp(chart_top, dpr));
        ctx.line_to(crisp(cx, dpr), crisp(chart_bottom, dpr));
        ctx.stroke();

        // Draw control points if selected
        if is_selected {
            ctx.set_line_dash(&[]);

            // Center point
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_stroke_width(2.0);
            ctx.begin_path();
            ctx.arc(crisp(cx, dpr), crisp(cy, dpr), CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();

            // Radius control point (at edge position)
            ctx.begin_path();
            ctx.arc(crisp(edge_x, dpr), crisp(edge_y, dpr), CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Bounding box based on outermost ellipse
                let outer_rx = base_rx * (self.count as f64);
                let outer_ry = base_ry * (self.count as f64);
                let min_x = cx - outer_rx;
                let max_x = cx + outer_rx;
                let min_y = cy - outer_ry;
                let max_y = cy + outer_ry;

                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => (min_x + max_x) / 2.0,
                    super::super::TextAlign::End => max_x,
                };
                // Start = ABOVE upper boundary, Center = middle, End = BELOW lower boundary
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => min_y - text_offset,
                    super::super::TextAlign::Center => (min_y + max_y) / 2.0,
                    super::super::TextAlign::End => max_y + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }
    }
    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "time_cycles", display_name: "Time Cycles", kind: PrimitiveKind::Measurement,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Circular time cycles", icon: "time_cycles", default_color: "#9C27B0",
        factory: |points, color| {
            let (b, p) = points.first().copied().unwrap_or((0.0, 100.0));
            let (b2, p2) = points.get(1).copied().unwrap_or((b + 20.0, p + p * 0.05));
            Box::new(TimeCycles::new(b, p, (b2 - b).abs(), (p2 - p).abs(), color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
