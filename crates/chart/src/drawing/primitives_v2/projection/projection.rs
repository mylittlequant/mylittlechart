//! Projection - general projection tool

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE, point_to_line_distance,
    RenderContext, crisp, LineStyle, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Projection {
    pub data: PrimitiveData,
    pub bar1: f64, pub price1: f64,
    pub bar2: f64, pub price2: f64,
    #[serde(default = "default_true")] pub show_labels: bool,
    #[serde(default = "default_true")] pub show_levels: bool,
}
fn default_true() -> bool { true }

impl Projection {
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "projection".to_string(), display_name: "Projection".to_string(), color: PrimitiveColor::new(color), width: 2.0, ..Default::default() },
            bar1, price1, bar2, price2, show_labels: true, show_levels: true,
        }
    }
}

impl Primitive for Projection {
    fn type_id(&self) -> &'static str { "projection" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Trading }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(f64, f64)> { vec![(self.bar1, self.price1), (self.bar2, self.price2)] }
    fn set_points(&mut self, pts: &[(f64, f64)]) {
        if let Some(&(b, p)) = pts.first() { self.bar1 = b; self.price1 = p; }
        if let Some(&(b, p)) = pts.get(1) { self.bar2 = b; self.price2 = p; }
    }
    fn translate(&mut self, bd: f64, pd: f64) { self.bar1 += bd; self.bar2 += bd; self.price1 += pd; self.price2 += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, bar: f64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.bar1 = bar; self.price1 = price; }
            ControlPointType::Point2 => { self.bar2 = bar; self.price2 = price; }
            ControlPointType::Move => { let bd = bar - self.bar1; let pd = price - self.price1; self.translate(bd, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let (x1, y1) = (vp.bar_to_x_f64(self.bar1), vp.price_to_y(self.price1, ps.price_min, ps.price_max));
        let (x2, y2) = (vp.bar_to_x_f64(self.bar2), vp.price_to_y(self.price2, ps.price_min, ps.price_max));
        let r = 8.0;
        if (sx - x1).powi(2) + (sy - y1).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).powi(2) + (sy - y2).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        if point_to_line_distance(sx, sy, x1, y1, x2, y2) < HIT_TOLERANCE { return HitTestResult::Body; }
        HitTestResult::Miss
    }
    fn control_points(&self, vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        vec![
            ControlPoint::point1(vp.bar_to_x_f64(self.bar1), vp.price_to_y(self.price1, ps.price_min, ps.price_max)),
            ControlPoint::point2(vp.bar_to_x_f64(self.bar2), vp.price_to_y(self.price2, ps.price_min, ps.price_max)),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.bar_to_x(self.bar1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.bar_to_x(self.bar2);
        let y2 = ctx.price_to_y(self.price2);

        // Calculate standard Fibonacci projection levels
        let price_range = self.price2 - self.price1;
        let fib_levels = [0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0, 1.272, 1.618, 2.0];

        // Set stroke style
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);

        // Set line dash based on style
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Draw main projection line
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
        ctx.stroke();

        // Draw projection levels if enabled
        if self.show_levels {
            ctx.set_stroke_width(1.0);
            ctx.set_line_dash(&[4.0, 2.0]);

            let chart_width = ctx.chart_width();

            for &level in &fib_levels {
                let projected_price = self.price1 + (price_range * level);
                let y_level = ctx.price_to_y(projected_price);

                // Draw horizontal level line extending to the right
                ctx.begin_path();
                ctx.move_to(crisp(x2, dpr), crisp(y_level, dpr));
                ctx.line_to(crisp(chart_width, dpr), crisp(y_level, dpr));
                ctx.stroke();
            }
        }

        // Reset line dash
        ctx.set_line_dash(&[]);

        // Draw control points if selected
        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            // Point 1
            ctx.begin_path();
            ctx.arc(x1, y1, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();

            // Point 2
            ctx.begin_path();
            ctx.arc(x2, y2, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Calculate bounding box
                let min_x = x1.min(x2);
                let max_x = x1.max(x2);
                let min_y = y1.min(y2);
                let max_y = y1.max(y2);

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
        type_id: "projection", display_name: "Projection", kind: PrimitiveKind::Trading,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "General projection", icon: "projection", default_color: "#607D8B",
        factory: |points, color| { let (b1, p1) = points.first().copied().unwrap_or((0.0, 100.0)); let (b2, p2) = points.get(1).copied().unwrap_or((b1 + 20.0, p1 + 10.0)); Box::new(Projection::new(b1, p1, b2, p2, color)) },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
