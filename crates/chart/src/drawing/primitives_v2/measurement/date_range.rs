//! Date Range - horizontal time measurement

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text, TextAlign,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DateRange {
    pub data: PrimitiveData,
    pub ts1: i64,
    pub ts2: i64,
    pub price: f64,
    #[serde(default = "default_true")] pub show_bars: bool,
    #[serde(default = "default_true")] pub show_time: bool,
}
fn default_true() -> bool { true }

impl DateRange {
    pub fn new(ts1: i64, ts2: i64, price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "date_range".to_string(), display_name: "Date Range".to_string(), color: PrimitiveColor::new(color), width: 2.0, ..Default::default() },
            ts1, ts2, price, show_bars: true, show_time: true,
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
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, self.price), (self.ts2, self.price)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(ts, p)) = pts.first() { self.ts1 = ts; self.price = p; }
        if let Some(&(ts, _)) = pts.get(1) { self.ts2 = ts; }
    }
    fn translate(&mut self, ts_delta_ms: i64, pd: f64) { self.ts1 += ts_delta_ms; self.ts2 += ts_delta_ms; self.price += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.ts1 = ts_ms; self.price = price; }
            ControlPointType::Point2 => self.ts2 = ts_ms,
            ControlPointType::Move => { let td = ts_ms - self.ts1; let pd = price - self.price; self.translate(td, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = vp.bar_to_x_f64(b1);
        let x2 = vp.bar_to_x_f64(b2);
        let y = vp.price_to_y(self.price, ps.price_min, ps.price_max);
        let r = 8.0;
        if (sx - x1).powi(2) + (sy - y).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).powi(2) + (sy - y).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        if (sy - y).abs() < HIT_TOLERANCE && sx >= min_x && sx <= max_x { return HitTestResult::Body; }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let y = vp.price_to_y(self.price, ps.price_min, ps.price_max);
        vec![ControlPoint::point1(vp.bar_to_x_f64(b1), y), ControlPoint::point2(vp.bar_to_x_f64(b2), y)]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
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

        // Draw duration label
        let duration_ms = (self.ts2 - self.ts1).abs();
        let duration_secs = duration_ms / 1000;

        ctx.set_fill_color(&self.data.color.stroke);
        ctx.set_font("12px sans-serif");

        let label = if self.show_time {
            if duration_secs >= 86400 {
                format!("{:.1}d", duration_secs as f64 / 86400.0)
            } else if duration_secs >= 3600 {
                format!("{:.1}h", duration_secs as f64 / 3600.0)
            } else if duration_secs >= 60 {
                format!("{:.0}m", duration_secs as f64 / 60.0)
            } else {
                format!("{}s", duration_secs)
            }
        } else {
            format!("{}ms", duration_ms)
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
        factory: |points, color| {
            let (ts1, p) = points.first().copied().unwrap_or((0, 100.0));
            let (ts2, _) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, p));
            Box::new(DateRange::new(ts1, ts2, p, color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
