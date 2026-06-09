//! Cycle Lines - vertical lines at regular intervals

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CycleLines {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64, pub ts2: i64, // Define the cycle period
    #[serde(default = "default_count")] pub count: u8,
    #[serde(default = "default_true")] pub extend_left: bool,
    #[serde(default = "default_true")] pub extend_right: bool,
}
fn default_count() -> u8 { 10 }
fn default_true() -> bool { true }

impl CycleLines {
    pub fn new(ts1: i64, ts2: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "cycle_lines".to_string(), display_name: "Cycle Lines".to_string(), color: PrimitiveColor::new(color), width: 1.0, ..Default::default() },
            ts1, ts2, count: 10, extend_left: true, extend_right: true,
        }
    }
    pub fn period_ms(&self) -> i64 { (self.ts2 - self.ts1).abs() }
}

impl Primitive for CycleLines {
    fn type_id(&self) -> &'static str { "cycle_lines" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Measurement }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, 0.0), (self.ts2, 0.0)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(t, _)) = pts.first() { self.ts1 = t; }
        if let Some(&(t, _)) = pts.get(1) { self.ts2 = t; }
    }
    fn translate(&mut self, td: i64, _pd: f64) { self.ts1 += td; self.ts2 += td; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, _price: f64) {
        match pt {
            ControlPointType::Point1 => self.ts1 = ts_ms,
            ControlPointType::Point2 => self.ts2 = ts_ms,
            ControlPointType::Move => { let td = ts_ms - self.ts1; self.translate(td, 0.0); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, _sy: f64, bars: &[Bar], vp: &Viewport, _ps: &PriceScale) -> HitTestResult {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = vp.bar_to_x_f64(bar1);
        let x2 = vp.bar_to_x_f64(bar2);
        if (sx - x1).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        let period_px = (x2 - x1).abs();
        if period_px > 0.0 {
            for i in 0..self.count as i32 {
                let line_x = x1 + (i as f64) * period_px;
                if (sx - line_x).abs() < HIT_TOLERANCE { return HitTestResult::Body; }
            }
        }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let cy = vp.price_to_y((ps.price_min + ps.price_max) / 2.0, ps.price_min, ps.price_max);
        vec![ControlPoint::point1(vp.bar_to_x_f64(bar1), cy), ControlPoint::point2(vp.bar_to_x_f64(bar2), cy)]
    }
    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let period = (x2 - x1).abs();

        if period < 0.1 {
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

        let chart_top = 0.0;
        let chart_bottom = ctx.canvas_height();

        let start_x = if self.extend_left {
            x1.min(x2) - (self.count as f64) * period
        } else {
            x1.min(x2)
        };

        let total_lines = if self.extend_left && self.extend_right {
            self.count * 3
        } else if self.extend_left || self.extend_right {
            self.count * 2
        } else {
            self.count
        };

        for i in 0..total_lines {
            let line_x = start_x + (i as f64) * period;
            ctx.begin_path();
            ctx.move_to(crisp(line_x, dpr), crisp(chart_top, dpr));
            ctx.line_to(crisp(line_x, dpr), crisp(chart_bottom, dpr));
            ctx.stroke();
        }

        if is_selected {
            let mid_y = chart_bottom / 2.0;

            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_stroke_width(2.0);
            ctx.set_line_dash(&[]);
            ctx.begin_path();
            ctx.arc(crisp(x1, dpr), crisp(mid_y, dpr), CONTROL_POINT_RADIUS, 0.0, 2.0 * std::f64::consts::PI);
            ctx.fill();
            ctx.stroke();

            ctx.begin_path();
            ctx.arc(crisp(x2, dpr), crisp(mid_y, dpr), CONTROL_POINT_RADIUS, 0.0, 2.0 * std::f64::consts::PI);
            ctx.fill();
            ctx.stroke();
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let min_x = x1.min(x2);
                let max_x = x1.max(x2);
                let min_y = chart_top;
                let max_y = chart_bottom;

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
        type_id: "cycle_lines", display_name: "Cycle Lines", kind: PrimitiveKind::Measurement,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Vertical lines at regular intervals", icon: "cycle_lines", default_color: "#00BCD4",
        factory: |points, color| {
            let (t1, _) = points.first().copied().unwrap_or((0, 0.0));
            let (t2, _) = points.get(1).copied().unwrap_or((t1 + 1_200_000, 0.0));
            Box::new(CycleLines::new(t1, t2, color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
