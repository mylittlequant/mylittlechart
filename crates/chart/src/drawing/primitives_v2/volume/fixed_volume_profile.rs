//! Fixed Volume Profile - volume profile over fixed range

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE,
    CONTROL_POINT_FILL, render_primitive_text, TextAlign,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FixedVolumeProfile {
    pub data: PrimitiveData,
    #[serde(default)] pub ts1: i64,
    #[serde(default)] pub ts2: i64,
    #[serde(default = "default_rows")] pub rows: u16,
    #[serde(default = "default_true")] pub show_poc: bool,
    #[serde(default = "default_true")] pub show_value_area: bool,
}
fn default_rows() -> u16 { 24 }
fn default_true() -> bool { true }

impl FixedVolumeProfile {
    pub fn new(ts1: i64, ts2: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "fixed_volume_profile".to_string(), display_name: "Fixed Volume Profile".to_string(), color: PrimitiveColor::new(color), width: 1.0, ..Default::default() },
            ts1, ts2, rows: 24, show_poc: true, show_value_area: true,
        }
    }
}

impl Primitive for FixedVolumeProfile {
    fn type_id(&self) -> &'static str { "fixed_volume_profile" }
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
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2));
        if (sx - x1).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        if sx >= min_x && sx <= max_x { return HitTestResult::Body; }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let cy = vp.price_to_y((ps.price_min + ps.price_max) / 2.0, ps.price_min, ps.price_max);
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2));
        vec![ControlPoint::point1(x1, cy), ControlPoint::point2(x2, cy)]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        let chart_height = ctx.chart_height();

        // Draw vertical boundary lines
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        ctx.set_line_dash(&[]);

        ctx.begin_path();
        ctx.move_to(crisp(min_x, dpr), 0.0);
        ctx.line_to(crisp(min_x, dpr), chart_height);
        ctx.stroke();

        ctx.begin_path();
        ctx.move_to(crisp(max_x, dpr), 0.0);
        ctx.line_to(crisp(max_x, dpr), chart_height);
        ctx.stroke();

        // Draw volume histogram (placeholder - would need actual volume data)
        let row_height = chart_height / self.rows as f64;
        let profile_width = (max_x - min_x) * 0.3;

        ctx.set_fill_color(&self.data.color.stroke);
        ctx.set_global_alpha(0.5);

        for i in 0..self.rows {
            let y = i as f64 * row_height;
            let volume_pct = ((i as f64 - self.rows as f64 / 2.0).abs() / (self.rows as f64 / 2.0)).min(1.0);
            let bar_width = profile_width * (1.0 - volume_pct);

            ctx.begin_path();
            ctx.rect(max_x, y, bar_width, row_height);
            ctx.fill();
        }

        ctx.set_global_alpha(1.0);

        if self.show_poc {
            let poc_y = chart_height / 2.0;
            ctx.set_stroke_color("#FFEB3B");
            ctx.set_stroke_width(2.0 * dpr);
            ctx.begin_path();
            ctx.move_to(crisp(min_x, dpr), crisp(poc_y, dpr));
            ctx.line_to(crisp(max_x, dpr), crisp(poc_y, dpr));
            ctx.stroke();
        }

        if self.show_value_area {
            let va_top = chart_height * 0.35;
            let va_bottom = chart_height * 0.65;
            ctx.set_stroke_color(&self.data.color.stroke);
            ctx.set_global_alpha(0.3);
            ctx.set_stroke_width(1.0 * dpr);
            ctx.set_line_dash(&[5.0 * dpr, 3.0 * dpr]);

            ctx.begin_path();
            ctx.move_to(crisp(min_x, dpr), crisp(va_top, dpr));
            ctx.line_to(crisp(max_x, dpr), crisp(va_top, dpr));
            ctx.stroke();

            ctx.begin_path();
            ctx.move_to(crisp(min_x, dpr), crisp(va_bottom, dpr));
            ctx.line_to(crisp(max_x, dpr), crisp(va_bottom, dpr));
            ctx.stroke();

            ctx.set_global_alpha(1.0);
            ctx.set_line_dash(&[]);
        }

        if is_selected {
            let cy = chart_height / 2.0;
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_stroke_width(1.5 * dpr);

            ctx.begin_path();
            ctx.arc(x1, cy, CONTROL_POINT_RADIUS * dpr, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();

            ctx.begin_path();
            ctx.arc(x2, cy, CONTROL_POINT_RADIUS * dpr, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

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
                    TextAlign::Center => chart_height / 2.0,
                    TextAlign::End => chart_height + text_offset,
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
        type_id: "fixed_volume_profile", display_name: "Fixed Volume Profile", kind: PrimitiveKind::Measurement,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Volume profile over range", icon: "fixed_volume_profile", default_color: "#2196F3",
        factory: |points, color| {
            let (t1, _) = points.first().copied().unwrap_or((0, 0.0));
            let (t2, _) = points.get(1).copied().unwrap_or((t1 + 180_000_000, 0.0));
            Box::new(FixedVolumeProfile::new(t1, t2, color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
