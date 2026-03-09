//! Date Range - horizontal time measurement

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text, TextAlign,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DateRange {
    pub data: PrimitiveData,
    pub bar1: f64,
    pub bar2: f64,
    pub price: f64,
    #[serde(default = "default_true")] pub show_bars: bool,
    #[serde(default = "default_true")] pub show_time: bool,
}
fn default_true() -> bool { true }

impl DateRange {
    pub fn new(bar1: f64, bar2: f64, price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "date_range".to_string(), display_name: "Date Range".to_string(), color: PrimitiveColor::new(color), width: 2.0, ..Default::default() },
            bar1, bar2, price, show_bars: true, show_time: true,
        }
    }
}

impl Primitive for DateRange {
    fn type_id(&self) -> &'static str { "date_range" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Measurement }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(f64, f64)> { vec![(self.bar1, self.price), (self.bar2, self.price)] }
    fn set_points(&mut self, pts: &[(f64, f64)]) {
        if let Some(&(b, p)) = pts.first() { self.bar1 = b; self.price = p; }
        if let Some(&(b, _)) = pts.get(1) { self.bar2 = b; }
    }
    fn translate(&mut self, bd: f64, pd: f64) { self.bar1 += bd; self.bar2 += bd; self.price += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, bar: f64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.bar1 = bar; self.price = price; }
            ControlPointType::Point2 => self.bar2 = bar,
            ControlPointType::Move => { let bd = bar - self.bar1; let pd = price - self.price; self.translate(bd, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let x1 = vp.bar_to_x_f64(self.bar1);
        let x2 = vp.bar_to_x_f64(self.bar2);
        let y = vp.price_to_y(self.price, ps.price_min, ps.price_max);
        let r = 8.0;
        if (sx - x1).powi(2) + (sy - y).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).powi(2) + (sy - y).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        if (sy - y).abs() < HIT_TOLERANCE && sx >= min_x && sx <= max_x { return HitTestResult::Body; }
        HitTestResult::Miss
    }
    fn control_points(&self, vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let y = vp.price_to_y(self.price, ps.price_min, ps.price_max);
        vec![ControlPoint::point1(vp.bar_to_x_f64(self.bar1), y), ControlPoint::point2(vp.bar_to_x_f64(self.bar2), y)]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.bar_to_x(self.bar1);
        let x2 = ctx.bar_to_x(self.bar2);
        let y = ctx.price_to_y(self.price);

        let min_x = x1.min(x2);
        let max_x = x1.max(x2);
        let w = max_x - min_x;

        // Draw filled area between the two vertical lines
        ctx.set_fill_color(&format!("{}40", &self.data.color.stroke));
        ctx.fill_rect(crisp(min_x, dpr), 0.0, w, ctx.chart_height());

        // Draw the two vertical lines
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_line_style(LineStyle::Solid);
        ctx.set_stroke_width(self.data.width);

        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), 0.0);
        ctx.line_to(crisp(x1, dpr), ctx.chart_height());
        ctx.stroke();

        ctx.begin_path();
        ctx.move_to(crisp(x2, dpr), 0.0);
        ctx.line_to(crisp(x2, dpr), ctx.chart_height());
        ctx.stroke();

        // Draw bar count label
        let bar_count = (self.bar2 - self.bar1).abs();

        ctx.set_fill_color(&self.data.color.stroke);
        ctx.set_font("12px sans-serif");

        let label = if self.show_bars {
            format!("{:.0} bars", bar_count)
        } else {
            format!("{:.0}", bar_count)
        };

        ctx.fill_text(&label, crisp(min_x + w / 2.0, dpr), crisp(y - 10.0, dpr));

        // Draw control points if selected
        if is_selected {
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_stroke_width(1.0);

            ctx.begin_path();
            ctx.arc(crisp(x1, dpr), crisp(y, dpr), CONTROL_POINT_RADIUS, 0.0, std::f64::consts::PI * 2.0);
            ctx.fill();
            ctx.stroke();

            ctx.begin_path();
            ctx.arc(crisp(x2, dpr), crisp(y, dpr), CONTROL_POINT_RADIUS, 0.0, std::f64::consts::PI * 2.0);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => min_x,
                    TextAlign::Center => (min_x + max_x) / 2.0,
                    TextAlign::End => max_x,
                };
                // Start = ABOVE upper boundary, Center = middle, End = BELOW lower boundary
                let text_y = match text.v_align {
                    TextAlign::Start => 0.0 - text_offset,
                    TextAlign::Center => ctx.chart_height() / 2.0,
                    TextAlign::End => ctx.chart_height() + text_offset,
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
        type_id: "date_range", display_name: "Date Range", kind: PrimitiveKind::Measurement,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Measure time difference", icon: "date_range", default_color: "#2196F3",
        factory: |points, color| { let (b1, p) = points.first().copied().unwrap_or((0.0, 100.0)); let (b2, _) = points.get(1).copied().unwrap_or((b1 + 20.0, p)); Box::new(DateRange::new(b1, b2, p, color)) },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
