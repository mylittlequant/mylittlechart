//! Fixed Volume Profile - volume profile over fixed range

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{ConfigKey, current_language};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, render_primitive_text, TextAlign,
    config::{ConfigProperty, PropertyValue},
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
    /// The two boundaries, then the histogram's own area.
    ///
    /// It used to claim every pixel between the boundaries at ANY price — the
    /// whole column of the chart, candles included. A profile occupies the
    /// price band its bars cover, and nothing above or below it.
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2));
        if (sx - x1).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        if sx < min_x || sx > max_x { return HitTestResult::Miss; }
        // Vertical boundaries are lines and are grabbable over their full run.
        if (sx - min_x).abs() < HIT_TOLERANCE || (sx - max_x).abs() < HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        let window = super::profile_render::bars_in_window(bars, self.ts1, self.ts2);
        let Some((lo, hi)) = super::profile_render::price_span(window) else { return HitTestResult::Miss };
        let y_lo = vp.price_to_y(hi, ps.price_min, ps.price_max);
        let y_hi = vp.price_to_y(lo, ps.price_min, ps.price_max);
        if sy >= y_lo - HIT_TOLERANCE && sy <= y_hi + HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let cy = vp.price_to_y((ps.price_min + ps.price_max) / 2.0, ps.price_min, ps.price_max);
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2));
        vec![ControlPoint::point1(x1, cy), ControlPoint::point2(x2, cy)]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
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

        // The real profile over the bars this range covers, growing rightward
        // from the LEFT boundary — the direction a fixed-range profile is read
        // in, and the direction the chart's own overlay grows in.
        let window = super::profile_render::bars_in_window(ctx.bars(), self.ts1, self.ts2);
        let Some(vp) = super::profile_render::profile_for(window, self.rows) else { return };
        let geom = super::profile_render::ProfileGeometry {
            origin_x: min_x,
            max_width: max_x - min_x,
            span: (min_x, max_x),
        };
        super::profile_render::draw_profile(ctx, &vp, &geom, self.show_poc, self.show_value_area);

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

    fn style_properties(&self) -> Vec<ConfigProperty> {
        let lang = current_language();
        vec![
            ConfigProperty::number("rows", ConfigKey::Rows.get(lang), self.rows as f64, Some(5.0), Some(100.0))
                .with_order(20),
            ConfigProperty::boolean("show_poc", ConfigKey::ShowPoc.get(lang), self.show_poc)
                .with_order(21),
            ConfigProperty::boolean("show_value_area", ConfigKey::ShowValueArea.get(lang), self.show_value_area)
                .with_order(22),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "rows" => { if let Some(v) = value.as_number() { self.rows = v.round().clamp(5.0, 100.0) as u16; return true; } }
            "show_poc" => { if let Some(v) = value.as_bool() { self.show_poc = v; return true; } }
            "show_value_area" => { if let Some(v) = value.as_bool() { self.show_value_area = v; return true; } }
            _ => {}
        }
        false
    }
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
    }
}
