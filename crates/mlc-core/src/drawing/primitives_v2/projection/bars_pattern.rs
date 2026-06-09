//! Bars Pattern - copy and project price pattern

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BarsPattern {
    pub data: PrimitiveData,
    #[serde(default)] pub ts1: i64,
    #[serde(default)] pub ts2: i64,
    #[serde(default)] pub ts3: i64,
    #[serde(default)] pub price_offset: f64,
    #[serde(default = "default_true")] pub mirror: bool,
}
fn default_true() -> bool { true }

impl BarsPattern {
    pub fn new(ts1: i64, ts2: i64, ts3: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "bars_pattern".to_string(), display_name: "Bars Pattern".to_string(), color: PrimitiveColor::new(color), width: 1.0, ..Default::default() },
            ts1, ts2, ts3, price_offset: 0.0, mirror: false,
        }
    }
}

impl Primitive for BarsPattern {
    fn type_id(&self) -> &'static str { "bars_pattern" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Trading }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::ThreePoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, 0.0), (self.ts2, 0.0), (self.ts3, 0.0)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(t, _)) = pts.first() { self.ts1 = t; }
        if let Some(&(t, _)) = pts.get(1) { self.ts2 = t; }
        if let Some(&(t, _)) = pts.get(2) { self.ts3 = t; }
    }
    fn translate(&mut self, td: i64, pd: f64) { self.ts1 += td; self.ts2 += td; self.ts3 += td; self.price_offset += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, _price: f64) {
        match pt {
            ControlPointType::Point1 => self.ts1 = ts_ms,
            ControlPointType::Point2 => self.ts2 = ts_ms,
            ControlPointType::Point3 => self.ts3 = ts_ms,
            ControlPointType::Move => { let td = ts_ms - self.ts1; self.translate(td, 0.0); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, _sy: f64, bars: &[Bar], vp: &Viewport, _ps: &PriceScale) -> HitTestResult {
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2));
        let x3 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts3));
        if (sx - x1).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        if (sx - x3).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point3); }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let cy = vp.price_to_y((ps.price_min + ps.price_max) / 2.0, ps.price_min, ps.price_max);
        vec![
            ControlPoint::point1(vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1)), cy),
            ControlPoint::point2(vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2)), cy),
            ControlPoint::point3(vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts3)), cy),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let x3 = ctx.ts_to_x_ms(self.ts3);

        // Get chart dimensions
        let chart_height = ctx.chart_height();
        let pattern_width = (x2 - x1).abs();

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

        // Draw source range vertical lines
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), 0.0);
        ctx.line_to(crisp(x1, dpr), chart_height);
        ctx.stroke();

        ctx.begin_path();
        ctx.move_to(crisp(x2, dpr), 0.0);
        ctx.line_to(crisp(x2, dpr), chart_height);
        ctx.stroke();

        // Draw target range vertical line
        ctx.begin_path();
        ctx.move_to(crisp(x3, dpr), 0.0);
        ctx.line_to(crisp(x3, dpr), chart_height);
        ctx.stroke();

        // Draw projected pattern range
        let x4 = x3 + pattern_width;
        ctx.begin_path();
        ctx.move_to(crisp(x4, dpr), 0.0);
        ctx.line_to(crisp(x4, dpr), chart_height);
        ctx.stroke();

        // Draw connecting lines at top and bottom
        let mid_y = chart_height / 2.0;
        ctx.set_line_dash(&[4.0, 4.0]);

        // Top connecting line
        ctx.begin_path();
        ctx.move_to(crisp(x2, dpr), crisp(mid_y - 20.0, dpr));
        ctx.line_to(crisp(x3, dpr), crisp(mid_y - 20.0, dpr));
        ctx.stroke();

        // Bottom connecting line
        ctx.begin_path();
        ctx.move_to(crisp(x2, dpr), crisp(mid_y + 20.0, dpr));
        ctx.line_to(crisp(x3, dpr), crisp(mid_y + 20.0, dpr));
        ctx.stroke();

        // Reset line dash
        ctx.set_line_dash(&[]);

        // Draw control points if selected
        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            ctx.begin_path();
            ctx.arc(x1, mid_y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();

            ctx.begin_path();
            ctx.arc(x2, mid_y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();

            ctx.begin_path();
            ctx.arc(x3, mid_y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let min_x = x1.min(x2).min(x3);
                let max_x = x1.max(x2).max(x3).max(x4);
                let min_y = 0.0;
                let max_y = chart_height;

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
        type_id: "bars_pattern", display_name: "Bars Pattern", kind: PrimitiveKind::Trading,
        click_behavior: ClickBehavior::ThreePoint, tooltip: "Copy and project price pattern", icon: "bars_pattern", default_color: "#9C27B0",
        factory: |points, color| {
            let (t1, _) = points.first().copied().unwrap_or((0, 0.0));
            let (t2, _) = points.get(1).copied().unwrap_or((t1 + 72_000_000, 0.0));
            let (t3, _) = points.get(2).copied().unwrap_or((t2 + 36_000_000, 0.0));
            Box::new(BarsPattern::new(t1, t2, t3, color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
