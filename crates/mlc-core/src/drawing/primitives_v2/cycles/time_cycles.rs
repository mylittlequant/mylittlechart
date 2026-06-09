//! Time Cycles - circular time cycles

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeCycles {
    pub data: PrimitiveData,
    pub ts_ms: i64,
    pub price: f64,
    /// Radius in milliseconds (time dimension)
    pub radius_ms: i64,
    /// Vertical radius in price units (for proper ellipse behavior)
    #[serde(default = "default_radius_price")]
    pub radius_price: f64,
    #[serde(default = "default_count")] pub count: u8,
}
fn default_count() -> u8 { 5 }
fn default_radius_price() -> f64 { 0.0 }

impl TimeCycles {
    pub fn new(ts_ms: i64, price: f64, radius_ms: i64, radius_price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "time_cycles".to_string(), display_name: "Time Cycles".to_string(), color: PrimitiveColor::new(color), width: 1.0, ..Default::default() },
            ts_ms, price, radius_ms, radius_price, count: 5,
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
    fn points(&self) -> Vec<(i64, f64)> {
        vec![
            (self.ts_ms, self.price),
            (self.ts_ms + self.radius_ms, self.price + self.radius_price),
        ]
    }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(t, p)) = pts.first() { self.ts_ms = t; self.price = p; }
        if let Some(&(t, p)) = pts.get(1) {
            self.radius_ms = (t - self.ts_ms).abs();
            self.radius_price = (p - self.price).abs();
        }
    }
    fn translate(&mut self, td: i64, pd: f64) { self.ts_ms += td; self.price += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.ts_ms = ts_ms; self.price = price; }
            ControlPointType::Point2 => {
                self.radius_ms = (ts_ms - self.ts_ms).abs();
                self.radius_price = (price - self.price).abs();
            }
            ControlPointType::Move => { let td = ts_ms - self.ts_ms; let pd = price - self.price; self.translate(td, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let center_bar = timestamp_ms_to_bar_f64(bars, self.ts_ms);
        let edge_bar = timestamp_ms_to_bar_f64(bars, self.ts_ms + self.radius_ms);
        let cx = vp.bar_to_x_f64(center_bar);
        let cy = vp.price_to_y(self.price, ps.price_min, ps.price_max);
        let r = 8.0;
        if (sx - cx).powi(2) + (sy - cy).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }

        let base_rx = (vp.bar_to_x_f64(edge_bar) - cx).abs();
        let base_ry = (vp.price_to_y(self.price + self.radius_price, ps.price_min, ps.price_max) - cy).abs();

        let p2x = cx + base_rx;
        let p2y = cy + base_ry;
        if (sx - p2x).powi(2) + (sy - p2y).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }

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
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let center_bar = timestamp_ms_to_bar_f64(bars, self.ts_ms);
        let edge_bar = timestamp_ms_to_bar_f64(bars, self.ts_ms + self.radius_ms);
        let cx = vp.bar_to_x_f64(center_bar);
        let cy = vp.price_to_y(self.price, ps.price_min, ps.price_max);
        let base_rx = (vp.bar_to_x_f64(edge_bar) - cx).abs();
        let base_ry = (vp.price_to_y(self.price + self.radius_price, ps.price_min, ps.price_max) - cy).abs();
        vec![ControlPoint::point1(cx, cy), ControlPoint::point2(cx + base_rx, cy + base_ry)]
    }
    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let cx = ctx.ts_to_x_ms(self.ts_ms);
        let cy = ctx.price_to_y(self.price);

        let edge_x = ctx.ts_to_x_ms(self.ts_ms + self.radius_ms);
        let edge_y = ctx.price_to_y(self.price + self.radius_price);
        let base_rx = (edge_x - cx).abs();
        let base_ry = (edge_y - cy).abs();

        if base_rx < 0.1 && base_ry < 0.1 {
            return;
        }

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

        ctx.set_line_dash(&[3.0, 3.0]);
        let chart_top = 0.0;
        let chart_bottom = ctx.canvas_height();
        ctx.begin_path();
        ctx.move_to(crisp(cx, dpr), crisp(chart_top, dpr));
        ctx.line_to(crisp(cx, dpr), crisp(chart_bottom, dpr));
        ctx.stroke();

        if is_selected {
            ctx.set_line_dash(&[]);

            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_stroke_width(2.0);
            ctx.begin_path();
            ctx.arc(crisp(cx, dpr), crisp(cy, dpr), CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();

            ctx.begin_path();
            ctx.arc(crisp(edge_x, dpr), crisp(edge_y, dpr), CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
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
            let (t, p) = points.first().copied().unwrap_or((0, 100.0));
            let (t2, p2) = points.get(1).copied().unwrap_or((t + 1_200_000, p + p * 0.05));
            Box::new(TimeCycles::new(t, p, (t2 - t).abs(), (p2 - p).abs(), color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
