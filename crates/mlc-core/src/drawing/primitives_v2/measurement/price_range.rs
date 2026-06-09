//! Price Range - vertical price measurement

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text, TextAlign,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceRange {
    pub data: PrimitiveData,
    pub ts_ms: i64,
    pub price1: f64,
    pub price2: f64,
    #[serde(default = "default_true")] pub show_percentage: bool,
    #[serde(default = "default_true")] pub show_pips: bool,
}
fn default_true() -> bool { true }

impl PriceRange {
    pub fn new(ts_ms: i64, price1: f64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "price_range".to_string(), display_name: "Price Range".to_string(), color: PrimitiveColor::new(color), width: 2.0, ..Default::default() },
            ts_ms, price1, price2, show_percentage: true, show_pips: true,
        }
    }
}

impl Primitive for PriceRange {
    fn type_id(&self) -> &'static str { "price_range" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Measurement }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts_ms, self.price1), (self.ts_ms, self.price2)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(ts, p)) = pts.first() { self.ts_ms = ts; self.price1 = p; }
        if let Some(&(_, p)) = pts.get(1) { self.price2 = p; }
    }
    fn translate(&mut self, ts_delta_ms: i64, pd: f64) { self.ts_ms += ts_delta_ms; self.price1 += pd; self.price2 += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.ts_ms = ts_ms; self.price1 = price; }
            ControlPointType::Point2 => self.price2 = price,
            ControlPointType::Move => { let td = ts_ms - self.ts_ms; let pd = price - self.price1; self.translate(td, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let b = timestamp_ms_to_bar_f64(bars, self.ts_ms);
        let x = vp.bar_to_x_f64(b);
        let y1 = vp.price_to_y(self.price1, ps.price_min, ps.price_max);
        let y2 = vp.price_to_y(self.price2, ps.price_min, ps.price_max);
        let r = 8.0;
        if (sx - x).powi(2) + (sy - y1).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x).powi(2) + (sy - y2).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        let (min_y, max_y) = (y1.min(y2), y1.max(y2));
        if (sx - x).abs() < HIT_TOLERANCE && sy >= min_y && sy <= max_y { return HitTestResult::Body; }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let b = timestamp_ms_to_bar_f64(bars, self.ts_ms);
        let x = vp.bar_to_x_f64(b);
        vec![ControlPoint::point1(x, vp.price_to_y(self.price1, ps.price_min, ps.price_max)), ControlPoint::point2(x, vp.price_to_y(self.price2, ps.price_min, ps.price_max))]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x = ctx.ts_to_x_ms(self.ts_ms);
        let y1 = ctx.price_to_y(self.price1);
        let y2 = ctx.price_to_y(self.price2);

        let min_y = y1.min(y2);
        let max_y = y1.max(y2);
        let h = max_y - min_y;

        // Draw filled area between the two horizontal lines
        ctx.set_fill_color(&format!("{}40", &self.data.color.stroke));
        ctx.fill_rect(0.0, crisp(min_y, dpr), ctx.chart_width(), h);

        // Draw the two horizontal lines
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_line_style(LineStyle::Solid);
        ctx.set_stroke_width(self.data.width);

        ctx.begin_path();
        ctx.move_to(0.0, crisp(y1, dpr));
        ctx.line_to(ctx.chart_width(), crisp(y1, dpr));
        ctx.stroke();

        ctx.begin_path();
        ctx.move_to(0.0, crisp(y2, dpr));
        ctx.line_to(ctx.chart_width(), crisp(y2, dpr));
        ctx.stroke();

        // Draw price difference label
        let price_diff = (self.price2 - self.price1).abs();
        let percentage = if self.price1 != 0.0 {
            (price_diff / self.price1.abs()) * 100.0
        } else {
            0.0
        };

        ctx.set_fill_color(&self.data.color.stroke);
        ctx.set_font("12px sans-serif");

        let label = if self.show_percentage && self.show_pips {
            format!("{} ({:.2}%)", super::super::fmt_price(price_diff), percentage)
        } else if self.show_percentage {
            format!("{:.2}%", percentage)
        } else if self.show_pips {
            super::super::fmt_price(price_diff)
        } else {
            super::super::fmt_price(price_diff)
        };

        ctx.fill_text(&label, crisp(x + 10.0, dpr), crisp(min_y + h / 2.0, dpr));

        // Draw control points if selected
        if is_selected {
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_stroke_width(1.0);

            ctx.begin_path();
            ctx.arc(crisp(x, dpr), crisp(y1, dpr), CONTROL_POINT_RADIUS, 0.0, std::f64::consts::PI * 2.0);
            ctx.fill();
            ctx.stroke();

            ctx.begin_path();
            ctx.arc(crisp(x, dpr), crisp(y2, dpr), CONTROL_POINT_RADIUS, 0.0, std::f64::consts::PI * 2.0);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => 0.0,
                    TextAlign::Center => ctx.chart_width() / 2.0,
                    TextAlign::End => ctx.chart_width(),
                };
                let text_y = match text.v_align {
                    TextAlign::Start => min_y - text_offset,
                    TextAlign::Center => (min_y + max_y) / 2.0,
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
        type_id: "price_range", display_name: "Price Range", kind: PrimitiveKind::Measurement,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Measure price difference", icon: "price_range", default_color: "#4CAF50",
        factory: |points, color| {
            let (ts, p1) = points.first().copied().unwrap_or((0, 100.0));
            let (_, p2) = points.get(1).copied().unwrap_or((ts, p1 + 10.0));
            Box::new(PriceRange::new(ts, p1, p2, color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
