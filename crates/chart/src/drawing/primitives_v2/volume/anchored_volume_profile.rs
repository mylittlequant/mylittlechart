//! Anchored Volume Profile - volume profile from anchor

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor,
    RenderContext, crisp, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE,
    CONTROL_POINT_FILL, render_primitive_text, TextAlign,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnchoredVolumeProfile {
    pub data: PrimitiveData,
    #[serde(default)] pub anchor_ts: i64,
    #[serde(default = "default_rows")] pub rows: u16,
    #[serde(default = "default_true")] pub show_poc: bool,
}
fn default_rows() -> u16 { 24 }
fn default_true() -> bool { true }

impl AnchoredVolumeProfile {
    pub fn new(anchor_ts: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "anchored_volume_profile".to_string(), display_name: "Anchored Volume Profile".to_string(), color: PrimitiveColor::new(color), width: 1.0, ..Default::default() },
            anchor_ts, rows: 24, show_poc: true,
        }
    }
}

impl Primitive for AnchoredVolumeProfile {
    fn type_id(&self) -> &'static str { "anchored_volume_profile" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Measurement }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::SingleClick }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.anchor_ts, 0.0)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) { if let Some(&(t, _)) = pts.first() { self.anchor_ts = t; } }
    fn translate(&mut self, td: i64, _pd: f64) { self.anchor_ts += td; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, _price: f64) {
        if matches!(pt, ControlPointType::Point1 | ControlPointType::Move) { self.anchor_ts = ts_ms; }
    }
    fn hit_test(&self, sx: f64, _sy: f64, bars: &[Bar], vp: &Viewport, _ps: &PriceScale) -> HitTestResult {
        let b = timestamp_ms_to_bar_f64(bars, self.anchor_ts);
        let x = vp.bar_to_x_f64(b);
        if (sx - x).abs() < 20.0 { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let b = timestamp_ms_to_bar_f64(bars, self.anchor_ts);
        let cy = vp.price_to_y((ps.price_min + ps.price_max) / 2.0, ps.price_min, ps.price_max);
        vec![ControlPoint::point1(vp.bar_to_x_f64(b), cy)]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x = ctx.ts_to_x_ms(self.anchor_ts);
        let chart_width = ctx.chart_width();
        let chart_height = ctx.chart_height();

        // Draw vertical anchor line
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        ctx.set_line_dash(&[]);

        ctx.begin_path();
        ctx.move_to(crisp(x, dpr), 0.0);
        ctx.line_to(crisp(x, dpr), chart_height);
        ctx.stroke();

        // Draw volume histogram from anchor to right edge
        let row_height = chart_height / self.rows as f64;
        let max_profile_width = (chart_width - x) * 0.4;

        ctx.set_fill_color(&self.data.color.stroke);
        ctx.set_global_alpha(0.5);

        for i in 0..self.rows {
            let y = i as f64 * row_height;
            let volume_pct = ((i as f64 - self.rows as f64 / 2.0).abs() / (self.rows as f64 / 2.0)).min(1.0);
            let bar_width = max_profile_width * (1.0 - volume_pct);

            ctx.begin_path();
            ctx.rect(x, y, bar_width, row_height);
            ctx.fill();
        }

        ctx.set_global_alpha(1.0);

        if self.show_poc {
            let poc_y = chart_height / 2.0;
            let poc_x_end = x + max_profile_width;
            ctx.set_stroke_color("#FFEB3B");
            ctx.set_stroke_width(2.0 * dpr);
            ctx.begin_path();
            ctx.move_to(crisp(x, dpr), crisp(poc_y, dpr));
            ctx.line_to(crisp(poc_x_end, dpr), crisp(poc_y, dpr));
            ctx.stroke();
        }

        // Draw anchor marker
        let cy = chart_height / 2.0;
        ctx.set_fill_color(&self.data.color.stroke);
        ctx.set_global_alpha(1.0);
        ctx.begin_path();
        ctx.arc(x, cy, 4.0 * dpr, 0.0, std::f64::consts::TAU);
        ctx.fill();

        if is_selected {
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_stroke_width(1.5 * dpr);
            ctx.begin_path();
            ctx.arc(x, cy, CONTROL_POINT_RADIUS * dpr, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => x,
                    TextAlign::Center => (x + chart_width) / 2.0,
                    TextAlign::End => chart_width,
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
        type_id: "anchored_volume_profile", display_name: "Anchored Volume Profile", kind: PrimitiveKind::Measurement,
        click_behavior: ClickBehavior::SingleClick, tooltip: "Volume profile from anchor", icon: "anchored_volume_profile", default_color: "#9C27B0",
        factory: |points, color| {
            let (t, _) = points.first().copied().unwrap_or((0, 0.0));
            Box::new(AnchoredVolumeProfile::new(t, color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
