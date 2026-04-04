//! Triangle Down primitive - downward pointing triangle marker
//!
//! Uses TwoPoint behavior: first click sets center, second click sets size.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, PrimitiveText,
    RenderContext, crisp, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text, TextAlign,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TriangleDown {
    pub data: PrimitiveData,
    pub bar1: f64, pub price1: f64, // Center point
    pub bar2: f64, pub price2: f64, // Size point
}

impl TriangleDown {
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, color: &str) -> Self {
        let mut data = PrimitiveData {
            type_id: "triangle_down".to_string(),
            display_name: "Triangle Down".to_string(),
            color: PrimitiveColor::new(color),
            width: 2.0,
            ..Default::default()
        };
        data.text = Some(PrimitiveText::new(""));

        Self { data, bar1, price1, bar2, price2 }
    }

    fn calculate_size(&self, vp: &Viewport, ps: &PriceScale) -> f64 {
        let x1 = vp.bar_to_x_f64(self.bar1);
        let y1 = vp.price_to_y(self.price1, ps.price_min, ps.price_max);
        let x2 = vp.bar_to_x_f64(self.bar2);
        let y2 = vp.price_to_y(self.price2, ps.price_min, ps.price_max);
        ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt().max(10.0)
    }

    fn calculate_size_render(&self, ctx: &dyn RenderContext) -> f64 {
        let x1 = ctx.bar_to_x(self.bar1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.bar_to_x(self.bar2);
        let y2 = ctx.price_to_y(self.price2);
        ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt().max(10.0)
    }
}

impl Primitive for TriangleDown {
    fn type_id(&self) -> &'static str { "triangle_down" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Signal }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(f64, f64)> { vec![(self.bar1, self.price1), (self.bar2, self.price2)] }
    fn set_points(&mut self, points: &[(f64, f64)]) {
        if let Some(&(b, p)) = points.first() { self.bar1 = b; self.price1 = p; }
        if let Some(&(b, p)) = points.get(1) { self.bar2 = b; self.price2 = p; }
    }
    fn translate(&mut self, bd: f64, pd: f64) {
        self.bar1 += bd; self.bar2 += bd;
        self.price1 += pd; self.price2 += pd;
    }
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
        let size = self.calculate_size(vp, ps);
        if (sx - x1).powi(2) + (sy - y1).powi(2) <= size * size {
            return HitTestResult::Body;
        }
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
        let x = ctx.bar_to_x(self.bar1);
        let y = ctx.price_to_y(self.price1);
        let size = self.calculate_size_render(ctx);
        let s = size / 2.0;

        ctx.set_fill_color(&self.data.color.stroke);
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);

        // Draw downward pointing triangle
        ctx.begin_path();
        ctx.move_to(crisp(x, dpr), crisp(y + s, dpr)); // bottom point
        ctx.line_to(crisp(x - s, dpr), crisp(y - s, dpr)); // top left
        ctx.line_to(crisp(x + s, dpr), crisp(y - s, dpr)); // top right
        ctx.close_path();
        ctx.fill();
        ctx.stroke();

        if is_selected {
            let x2 = ctx.bar_to_x(self.bar2);
            let y2 = ctx.price_to_y(self.price2);
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (px, py) in [(x, y), (x2, y2)] {
                ctx.begin_path();
                ctx.arc(px, py, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }
        }

        // Render text OUTSIDE boundaries (emoji pattern)
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let min_x = x - s;
                let max_x = x + s;
                let min_y = y - s;
                let max_y = y + s;
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => min_x,
                    TextAlign::Center => x,
                    TextAlign::End => max_x,
                };
                let text_y = match text.v_align {
                    TextAlign::Start => min_y - text_offset,
                    TextAlign::Center => y,
                    TextAlign::End => max_y + text_offset,
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
        type_id: "triangle_down", display_name: "Triangle Down", kind: PrimitiveKind::Signal,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Downward triangle marker", icon: "arrow_down", default_color: "#F44336",
        factory: |points, color| {
            let (b1, p1) = points.first().copied().unwrap_or((0.0, 0.0));
            let (b2, p2) = points.get(1).copied().unwrap_or((b1 + 2.0, p1));
            Box::new(TriangleDown::new(b1, p1, b2, p2, color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
