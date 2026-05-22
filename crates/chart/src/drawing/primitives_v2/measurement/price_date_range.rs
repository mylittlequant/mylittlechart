//! Price Date Range - combined price and time measurement

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE, point_to_line_distance,
    RenderContext, crisp, LineStyle, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text, TextAlign,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceDateRange {
    pub data: PrimitiveData,
    pub ts1: i64, pub price1: f64,
    pub ts2: i64, pub price2: f64,
    #[serde(default = "default_true")] pub show_percentage: bool,
    #[serde(default = "default_true")] pub show_bars: bool,
    #[serde(default = "default_true")] pub show_pips: bool,
}
fn default_true() -> bool { true }

impl PriceDateRange {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "price_date_range".to_string(), display_name: "Price/Date Range".to_string(), color: PrimitiveColor::new(color), width: 2.0, ..Default::default() },
            ts1, price1, ts2, price2, show_percentage: true, show_bars: true, show_pips: true,
        }
    }
}

impl Primitive for PriceDateRange {
    fn type_id(&self) -> &'static str { "price_date_range" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Measurement }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, self.price1), (self.ts2, self.price2)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(ts, p)) = pts.first() { self.ts1 = ts; self.price1 = p; }
        if let Some(&(ts, p)) = pts.get(1) { self.ts2 = ts; self.price2 = p; }
    }
    fn translate(&mut self, ts_delta_ms: i64, pd: f64) { self.ts1 += ts_delta_ms; self.ts2 += ts_delta_ms; self.price1 += pd; self.price2 += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.ts1 = ts_ms; self.price1 = price; }
            ControlPointType::Point2 => { self.ts2 = ts_ms; self.price2 = price; }
            ControlPointType::Move => { let td = ts_ms - self.ts1; let pd = price - self.price1; self.translate(td, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let (x1, y1) = (vp.bar_to_x_f64(b1), vp.price_to_y(self.price1, ps.price_min, ps.price_max));
        let (x2, y2) = (vp.bar_to_x_f64(b2), vp.price_to_y(self.price2, ps.price_min, ps.price_max));
        let r = 8.0;
        if (sx - x1).powi(2) + (sy - y1).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).powi(2) + (sy - y2).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        if point_to_line_distance(sx, sy, x1, y1, x2, y2) < HIT_TOLERANCE { return HitTestResult::Body; }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        vec![
            ControlPoint::point1(vp.bar_to_x_f64(b1), vp.price_to_y(self.price1, ps.price_min, ps.price_max)),
            ControlPoint::point2(vp.bar_to_x_f64(b2), vp.price_to_y(self.price2, ps.price_min, ps.price_max)),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y2 = ctx.price_to_y(self.price2);

        let min_x = x1.min(x2);
        let min_y = y1.min(y2);
        let w = (x2 - x1).abs();
        let h = (y2 - y1).abs();

        // Draw filled rectangle
        ctx.set_fill_color(&format!("{}40", &self.data.color.stroke));
        ctx.fill_rect(crisp(min_x, dpr), crisp(min_y, dpr), w, h);

        // Draw rectangle border
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_line_style(LineStyle::Solid);
        ctx.set_stroke_width(self.data.width);
        ctx.stroke_rect(crisp(min_x, dpr), crisp(min_y, dpr), w, h);

        // Calculate metrics
        let price_diff = (self.price2 - self.price1).abs();
        let percentage = if self.price1 != 0.0 {
            (price_diff / self.price1.abs()) * 100.0
        } else {
            0.0
        };
        let duration_ms = (self.ts2 - self.ts1).abs();
        let duration_secs = duration_ms / 1000;

        // Draw labels
        ctx.set_fill_color(&self.data.color.stroke);
        ctx.set_font("12px sans-serif");

        let center_x = crisp(min_x + w / 2.0, dpr);
        let center_y = crisp(min_y + h / 2.0, dpr);

        // Price label
        let mut y_offset = center_y - 15.0;
        if self.show_pips {
            let price_label = if self.show_percentage {
                format!("{} ({:.2}%)", super::super::fmt_price(price_diff), percentage)
            } else {
                super::super::fmt_price(price_diff)
            };
            ctx.fill_text(&price_label, center_x, y_offset);
            y_offset += 15.0;
        }

        // Time duration label
        if self.show_bars {
            let time_label = if duration_secs >= 86400 {
                format!("{:.1}d", duration_secs as f64 / 86400.0)
            } else if duration_secs >= 3600 {
                format!("{:.1}h", duration_secs as f64 / 3600.0)
            } else if duration_secs >= 60 {
                format!("{:.0}m", duration_secs as f64 / 60.0)
            } else {
                format!("{}s", duration_secs)
            };
            ctx.fill_text(&time_label, center_x, y_offset);
        }

        // Draw control points if selected
        if is_selected {
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_stroke_width(1.0);

            ctx.begin_path();
            ctx.arc(crisp(x1, dpr), crisp(y1, dpr), CONTROL_POINT_RADIUS, 0.0, std::f64::consts::PI * 2.0);
            ctx.fill();
            ctx.stroke();

            ctx.begin_path();
            ctx.arc(crisp(x2, dpr), crisp(y2, dpr), CONTROL_POINT_RADIUS, 0.0, std::f64::consts::PI * 2.0);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let max_x = x1.max(x2);
                let max_y = y1.max(y2);
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => min_x,
                    TextAlign::Center => (min_x + max_x) / 2.0,
                    TextAlign::End => max_x,
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
        type_id: "price_date_range", display_name: "Price/Date Range", kind: PrimitiveKind::Measurement,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Measure price and time", icon: "price_date_range", default_color: "#FF9800",
        factory: |points, color| {
            let (ts1, p1) = points.first().copied().unwrap_or((0, 100.0));
            let (ts2, p2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, p1 + 10.0));
            Box::new(PriceDateRange::new(ts1, p1, ts2, p2, color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
