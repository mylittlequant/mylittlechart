//! Price Projection - project price movement

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE, point_to_line_distance,
    RenderContext, crisp, LineStyle, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceProjection {
    pub data: PrimitiveData,
    pub bar1: f64, pub price1: f64, // Source start
    pub bar2: f64, pub price2: f64, // Source end
    pub bar3: f64, pub price3: f64, // Projection point
    #[serde(default = "default_true")] pub show_percentage: bool,
}
fn default_true() -> bool { true }

impl PriceProjection {
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, bar3: f64, price3: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "price_projection".to_string(), display_name: "Price Projection".to_string(), color: PrimitiveColor::new(color), width: 2.0, ..Default::default() },
            bar1, price1, bar2, price2, bar3, price3, show_percentage: true,
        }
    }
}

impl Primitive for PriceProjection {
    fn type_id(&self) -> &'static str { "price_projection" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Trading }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::ThreePoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(f64, f64)> { vec![(self.bar1, self.price1), (self.bar2, self.price2), (self.bar3, self.price3)] }
    fn set_points(&mut self, pts: &[(f64, f64)]) {
        if let Some(&(b, p)) = pts.first() { self.bar1 = b; self.price1 = p; }
        if let Some(&(b, p)) = pts.get(1) { self.bar2 = b; self.price2 = p; }
        if let Some(&(b, p)) = pts.get(2) { self.bar3 = b; self.price3 = p; }
    }
    fn translate(&mut self, bd: f64, pd: f64) {
        self.bar1 += bd; self.bar2 += bd; self.bar3 += bd;
        self.price1 += pd; self.price2 += pd; self.price3 += pd;
    }
    fn move_control_point(&mut self, pt: ControlPointType, bar: f64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.bar1 = bar; self.price1 = price; }
            ControlPointType::Point2 => { self.bar2 = bar; self.price2 = price; }
            ControlPointType::Point3 => { self.bar3 = bar; self.price3 = price; }
            ControlPointType::Move => { let bd = bar - self.bar1; let pd = price - self.price1; self.translate(bd, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let pts = [(self.bar1, self.price1), (self.bar2, self.price2), (self.bar3, self.price3)];
        let screen: Vec<_> = pts.iter().map(|(b, p)| (vp.bar_to_x_f64(*b), vp.price_to_y(*p, ps.price_min, ps.price_max))).collect();
        let r = 8.0;
        if (sx - screen[0].0).powi(2) + (sy - screen[0].1).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - screen[1].0).powi(2) + (sy - screen[1].1).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        if (sx - screen[2].0).powi(2) + (sy - screen[2].1).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point3); }
        for i in 0..2 {
            if point_to_line_distance(sx, sy, screen[i].0, screen[i].1, screen[i+1].0, screen[i+1].1) < HIT_TOLERANCE { return HitTestResult::Body; }
        }
        HitTestResult::Miss
    }
    fn control_points(&self, vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        vec![
            ControlPoint::point1(vp.bar_to_x_f64(self.bar1), vp.price_to_y(self.price1, ps.price_min, ps.price_max)),
            ControlPoint::point2(vp.bar_to_x_f64(self.bar2), vp.price_to_y(self.price2, ps.price_min, ps.price_max)),
            ControlPoint::point3(vp.bar_to_x_f64(self.bar3), vp.price_to_y(self.price3, ps.price_min, ps.price_max)),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.bar_to_x(self.bar1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.bar_to_x(self.bar2);
        let y2 = ctx.price_to_y(self.price2);
        let x3 = ctx.bar_to_x(self.bar3);
        let y3 = ctx.price_to_y(self.price3);

        // Calculate the price movement to project
        let price_delta = self.price2 - self.price1;
        let projected_price = self.price3 + price_delta;
        let y4 = ctx.price_to_y(projected_price);

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

        // Draw source measurement line (point 1 to point 2)
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
        ctx.stroke();

        // Draw projection line (point 3 to projected point)
        ctx.set_line_dash(&[4.0, 4.0]); // Dashed for projection
        ctx.begin_path();
        ctx.move_to(crisp(x3, dpr), crisp(y3, dpr));
        ctx.line_to(crisp(x3, dpr), crisp(y4, dpr));
        ctx.stroke();

        // Draw horizontal levels
        ctx.set_line_dash(&[]);
        ctx.set_stroke_width(1.0);

        // Source start level
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(y1, dpr));
        ctx.stroke();

        // Source end level
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(y2, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
        ctx.stroke();

        // Projection start level
        ctx.begin_path();
        ctx.move_to(crisp(x3, dpr), crisp(y3, dpr));
        ctx.line_to(crisp(x3 + 50.0, dpr), crisp(y3, dpr));
        ctx.stroke();

        // Projection end level
        ctx.begin_path();
        ctx.move_to(crisp(x3, dpr), crisp(y4, dpr));
        ctx.line_to(crisp(x3 + 50.0, dpr), crisp(y4, dpr));
        ctx.stroke();

        // Reset line dash
        ctx.set_line_dash(&[]);

        // Draw control points if selected
        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            // Point 1
            ctx.begin_path();
            ctx.arc(x1, y1, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();

            // Point 2
            ctx.begin_path();
            ctx.arc(x2, y2, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();

            // Point 3
            ctx.begin_path();
            ctx.arc(x3, y3, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Calculate bounding box from all points including projected point
                let min_x = x1.min(x2).min(x3);
                let max_x = x1.max(x2).max(x3);
                let min_y = y1.min(y2).min(y3).min(y4);
                let max_y = y1.max(y2).max(y3).max(y4);

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
        type_id: "price_projection", display_name: "Price Projection", kind: PrimitiveKind::Trading,
        click_behavior: ClickBehavior::ThreePoint, tooltip: "Project price movement", icon: "price_projection", default_color: "#FF9800",
        factory: |points, color| {
            let (b1, p1) = points.first().copied().unwrap_or((0.0, 100.0));
            let (b2, p2) = points.get(1).copied().unwrap_or((b1 + 10.0, p1 + 5.0));
            let (b3, p3) = points.get(2).copied().unwrap_or((b2 + 10.0, p2 + 5.0));
            Box::new(PriceProjection::new(b1, p1, b2, p2, b3, p3, color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
