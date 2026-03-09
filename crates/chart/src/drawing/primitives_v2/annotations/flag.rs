//! Flag primitive - flag marker with label
//!
//! Uses centralized PrimitiveText system for text configuration.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, PrimitiveText,
    RenderContext, crisp, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text, TextAlign,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Flag {
    pub data: PrimitiveData,
    pub bar1: f64, pub price1: f64, // Anchor point (base of pole)
    pub bar2: f64, pub price2: f64, // Size point (determines flag size)
    // Legacy field for backwards compatibility
    #[serde(default)]
    pub text: String,
    #[serde(default = "default_flag_color")] pub flag_color: String,
}
fn default_flag_color() -> String { "#F44336".to_string() }

impl Flag {
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, color: &str) -> Self {
        let mut data = PrimitiveData {
            type_id: "flag".to_string(),
            display_name: "Flag".to_string(),
            color: PrimitiveColor::new(color),
            width: 2.0,
            ..Default::default()
        };
        // Initialize centralized text system
        data.text = Some(PrimitiveText::new("Flag"));

        Self {
            data,
            bar1, price1, bar2, price2,
            text: String::new(),
            flag_color: "#F44336".to_string(),
        }
    }

    fn get_text(&self) -> &str {
        if let Some(ref text) = self.data.text {
            &text.content
        } else if !self.text.is_empty() {
            &self.text
        } else {
            ""
        }
    }
}

impl Primitive for Flag {
    fn type_id(&self) -> &'static str { "flag" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Annotation }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(f64, f64)> { vec![(self.bar1, self.price1), (self.bar2, self.price2)] }
    fn set_points(&mut self, points: &[(f64, f64)]) {
        if let Some(&(b, p)) = points.first() { self.bar1 = b; self.price1 = p; }
        if let Some(&(b, p)) = points.get(1) { self.bar2 = b; self.price2 = p; }
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
        // Check flag body area (pole and flag triangle)
        let pole_height = (y2 - y1).abs();
        let flag_width = (x2 - x1).abs().max(25.0);
        let min_x = x1.min(x1 + flag_width);
        let max_x = x1.max(x1 + flag_width);
        let min_y = y1.min(y1 - pole_height);
        let max_y = y1.max(y1 - pole_height);
        if sx >= min_x - 5.0 && sx <= max_x + 5.0 && sy >= min_y - 5.0 && sy <= max_y + 5.0 {
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
        let x1 = ctx.bar_to_x(self.bar1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.bar_to_x(self.bar2);
        let y2 = ctx.price_to_y(self.price2);

        // Calculate flag size based on distance between points
        let pole_height = (y2 - y1).abs().max(30.0); // Minimum height of 30
        let flag_width = (x2 - x1).abs().max(25.0);  // Minimum width of 25
        let flag_height = pole_height * 0.5; // Flag triangle is half the pole height

        // Draw pole (from anchor point upward)
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
        ctx.line_to(crisp(x1, dpr), crisp(y1 - pole_height, dpr));
        ctx.stroke();

        // Draw flag triangle (at top of pole, extending in direction of x2)
        let flag_direction = if x2 >= x1 { 1.0 } else { -1.0 };
        ctx.set_fill_color(&self.flag_color);
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(y1 - pole_height, dpr));
        ctx.line_to(crisp(x1 + flag_width * flag_direction, dpr), crisp(y1 - pole_height + flag_height * 0.5, dpr));
        ctx.line_to(crisp(x1, dpr), crisp(y1 - pole_height + flag_height, dpr));
        ctx.close_path();
        ctx.fill();

        // Render text OUTSIDE boundaries (emoji pattern)
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Bounding box of flag (pole + flag triangle)
                let min_x = x1.min(x1 + flag_width * flag_direction);
                let max_x = x1.max(x1 + flag_width * flag_direction);
                let min_y = y1 - pole_height;
                let max_y = y1;
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => min_x,
                    TextAlign::Center => (min_x + max_x) / 2.0,
                    TextAlign::End => max_x,
                };
                // Start = ABOVE upper boundary, Center = middle, End = BELOW lower boundary
                let text_y = match text.v_align {
                    TextAlign::Start => min_y - text_offset,
                    TextAlign::Center => (min_y + max_y) / 2.0,
                    TextAlign::End => max_y + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (px, py) in [(x1, y1), (x2, y2)] {
                ctx.begin_path();
                ctx.arc(px, py, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "flag", display_name: "Flag", kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Flag marker", icon: "flag", default_color: "#F44336",
        factory: |points, color| { let (b1, p1) = points.first().copied().unwrap_or((0.0, 0.0)); let (b2, p2) = points.get(1).copied().unwrap_or((b1+5.0, p1-30.0)); Box::new(Flag::new(b1, p1, b2, p2, color)) },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
