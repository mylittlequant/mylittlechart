//! Arrow Line primitive - line segment with arrowhead

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE, point_to_line_distance,
    LineStyle, RenderContext, crisp, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text_rotated, TextAlign, LineTextParams, normalize_text_rotation,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArrowLine {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64, pub price1: f64,
    #[serde(default)]
    pub ts2: i64, pub price2: f64,
    #[serde(default)] pub arrow_start: bool,
    #[serde(default = "default_true")] pub arrow_end: bool,
    #[serde(default = "default_arrow_size")] pub arrow_size: f64,
}
fn default_true() -> bool { true }
fn default_arrow_size() -> f64 { 12.0 }

impl ArrowLine {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "arrow_line".to_string(), display_name: "Arrow Line".to_string(), color: PrimitiveColor::new(color), width: 2.0, ..Default::default() },
            ts1, price1, ts2, price2, arrow_start: false, arrow_end: true, arrow_size: 12.0,
        }
    }

    fn calculate_arrow_text_params(&self, x1: f64, y1: f64, x2: f64, y2: f64, text: &super::super::PrimitiveText) -> LineTextParams {
        let dx = x2 - x1; let dy = y2 - y1;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.001 { return LineTextParams { x: x1, y: y1, rotation: 0.0, text_bbox: None }; }

        let start_offset = if self.arrow_start { self.arrow_size + 4.0 } else { 0.0 };
        let end_offset = if self.arrow_end { self.arrow_size + 4.0 } else { 0.0 };
        let t = match text.h_align {
            TextAlign::Start => start_offset / len,
            TextAlign::Center => 0.5,
            TextAlign::End => 1.0 - end_offset / len,
        }.clamp(0.0, 1.0);

        let base_x = x1 + dx * t; let base_y = y1 + dy * t;
        let raw_angle = dy.atan2(dx);
        let (rotation, flipped) = normalize_text_rotation(raw_angle);
        let perp_x = -dy / len; let perp_y = dx / len;
        let text_offset = 8.0 + text.font_size / 2.0;
        let base_offset = match text.v_align {
            TextAlign::Start => -text_offset,
            TextAlign::Center => 0.0,
            TextAlign::End => text_offset,
        };
        let offset = if flipped { -base_offset } else { base_offset };
        LineTextParams { x: base_x + perp_x * offset, y: base_y + perp_y * offset, rotation, text_bbox: None }
    }
}

impl Primitive for ArrowLine {
    fn type_id(&self) -> &'static str { "arrow_line" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Annotation }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, self.price1), (self.ts2, self.price2)] }
    fn set_points(&mut self, points: &[(i64, f64)]) {
        if let Some(&(t, p)) = points.first() { self.ts1 = t; self.price1 = p; }
        if let Some(&(t, p)) = points.get(1) { self.ts2 = t; self.price2 = p; }
    }
    fn translate(&mut self, ts_delta_ms: i64, pd: f64) {
        self.ts1 += ts_delta_ms; self.ts2 += ts_delta_ms; self.price1 += pd; self.price2 += pd;
    }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.ts1 = ts_ms; self.price1 = price; }
            ControlPointType::Point2 => { self.ts2 = ts_ms; self.price2 = price; }
            ControlPointType::Move => {
                let td = ts_ms - self.ts1; let pd = price - self.price1;
                self.translate(td, pd);
            }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = vp.bar_to_x_f64(bar1); let y1 = vp.price_to_y(self.price1, ps.price_min, ps.price_max);
        let x2 = vp.bar_to_x_f64(bar2); let y2 = vp.price_to_y(self.price2, ps.price_min, ps.price_max);
        let r = 8.0;
        if (sx - x1).powi(2) + (sy - y1).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).powi(2) + (sy - y2).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        if point_to_line_distance(sx, sy, x1, y1, x2, y2) < HIT_TOLERANCE { return HitTestResult::Body; }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        vec![
            ControlPoint::point1(vp.bar_to_x_f64(bar1), vp.price_to_y(self.price1, ps.price_min, ps.price_max)),
            ControlPoint::point2(vp.bar_to_x_f64(bar2), vp.price_to_y(self.price2, ps.price_min, ps.price_max)),
        ]
    }
    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1); let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2); let y2 = ctx.price_to_y(self.price2);
        let dx = x2 - x1; let dy = y2 - y1;
        let len = (dx * dx + dy * dy).sqrt();

        let text_params = if let Some(ref text) = self.data.text {
            if !text.content.is_empty() && len > 0.001 { Some(self.calculate_arrow_text_params(x1, y1, x2, y2, text)) }
            else { None }
        } else { None };

        let needs_gap = self.data.text.as_ref()
            .map(|t| !t.content.is_empty() && matches!(t.v_align, TextAlign::Center))
            .unwrap_or(false);

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        ctx.begin_path();
        if needs_gap && len > 0.001 {
            let text = self.data.text.as_ref().unwrap();
            let start_offset_t = if self.arrow_start { self.arrow_size / len } else { 0.0 };
            let end_offset_t = if self.arrow_end { self.arrow_size / len } else { 0.0 };
            let t_center = match text.h_align {
                TextAlign::Start => start_offset_t,
                TextAlign::Center => 0.5,
                TextAlign::End => 1.0 - end_offset_t,
            };
            let char_count = text.content.len() as f64;
            let text_width = char_count * text.font_size * 0.6 + 8.0;
            let half_gap_t = (text_width / 2.0) / len;
            let t_start = (t_center - half_gap_t).max(0.0);
            let t_end = (t_center + half_gap_t).min(1.0);
            if t_start > 0.001 {
                ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
                ctx.line_to(crisp(x1 + dx * t_start, dpr), crisp(y1 + dy * t_start, dpr));
            }
            if t_end < 0.999 {
                ctx.move_to(crisp(x1 + dx * t_end, dpr), crisp(y1 + dy * t_end, dpr));
                ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
            }
        } else {
            ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
            ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
        }
        ctx.stroke();
        ctx.set_line_dash(&[]);

        if len > 0.0 {
            let nx = dx / len; let ny = dy / len;
            ctx.set_fill_color(&self.data.color.stroke);
            if self.arrow_end {
                let s = self.arrow_size;
                ctx.begin_path();
                ctx.move_to(crisp(x2, dpr), crisp(y2, dpr));
                ctx.line_to(crisp(x2 - nx * s - ny * s * 0.4, dpr), crisp(y2 - ny * s + nx * s * 0.4, dpr));
                ctx.line_to(crisp(x2 - nx * s + ny * s * 0.4, dpr), crisp(y2 - ny * s - nx * s * 0.4, dpr));
                ctx.close_path(); ctx.fill();
            }
            if self.arrow_start {
                let s = self.arrow_size;
                ctx.begin_path();
                ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
                ctx.line_to(crisp(x1 + nx * s - ny * s * 0.4, dpr), crisp(y1 + ny * s + nx * s * 0.4, dpr));
                ctx.line_to(crisp(x1 + nx * s + ny * s * 0.4, dpr), crisp(y1 + ny * s - nx * s * 0.4, dpr));
                ctx.close_path(); ctx.fill();
            }
        }

        if let Some(ref text) = self.data.text {
            if let Some(ref params) = text_params {
                render_primitive_text_rotated(ctx, text, params.x, params.y, &self.data.color.stroke, params.rotation);
            }
        }

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (px, py) in [(x1, y1), (x2, y2)] {
                ctx.begin_path();
                ctx.arc(px, py, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill(); ctx.stroke();
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "arrow_line", display_name: "Arrow Line", kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Line with arrow heads",
        icon: "arrow_line", default_color: "#2196F3",
        factory: |points, color| {
            let (ts1, p1) = points.first().copied().unwrap_or((0, 0.0));
            let (ts2, p2) = points.get(1).copied().unwrap_or((ts1, p1));
            Box::new(ArrowLine::new(ts1, p1, ts2, p2, color))
        },
        supports_text: true, has_levels: false, has_points_config: false,
    }
}
