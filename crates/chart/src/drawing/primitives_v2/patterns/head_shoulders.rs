//! Head and Shoulders primitive - reversal pattern

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle, CONTROL_POINT_RADIUS, CONTROL_POINT_HIT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
    config::{LabelStyle, ConfigProperty, PropertyValue, PropertyCategory},
};

fn default_true() -> bool { true }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeadShoulders {
    pub data: PrimitiveData,
    pub points: [(i64, f64); 7], // Left shoulder start, LS top, LS end/Head start, Head top, Head end/RS start, RS top, RS end
    #[serde(default = "default_true")]
    pub show_neckline: bool,
    #[serde(default)]
    pub inverted: bool,
    #[serde(default)]
    pub label_style: LabelStyle,
    #[serde(default = "default_true")]
    pub show_lines: bool,
}

impl HeadShoulders {
    pub fn new(points: [(i64, f64); 7], color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "head_shoulders".to_string(),
                display_name: "Head & Shoulders".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            points,
            show_neckline: true,
            inverted: false,
            label_style: LabelStyle::default(),
            show_lines: true,
        }
    }
}

impl Primitive for HeadShoulders {
    fn type_id(&self) -> &'static str { "head_shoulders" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Pattern }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::MultiPoint(7) }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { self.points.to_vec() }
    fn set_points(&mut self, pts: &[(i64, f64)]) { for (i, &p) in pts.iter().take(7).enumerate() { self.points[i] = p; } }
    fn translate(&mut self, ts_delta_ms: i64, pd: f64) { for p in &mut self.points { p.0 += ts_delta_ms; p.1 += pd; } }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Index(i) if (i as usize) < 7 => self.points[i as usize] = (ts_ms, price),
            ControlPointType::Move => { let td = ts_ms - self.points[0].0; let pd = price - self.points[0].1; self.translate(td, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let screen: Vec<_> = self.points.iter().map(|(ts, p)| {
            let b = timestamp_ms_to_bar_f64(bars, *ts);
            (vp.bar_to_x_f64(b), vp.price_to_y(*p, ps.price_min, ps.price_max))
        }).collect();
        for (i, &(x, y)) in screen.iter().enumerate() {
            if (sx - x).powi(2) + (sy - y).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2) { return HitTestResult::ControlPoint(ControlPointType::Index(i as u8)); }
        }
        for i in 0..6 {
            if point_to_line_dist(sx, sy, screen[i].0, screen[i].1, screen[i+1].0, screen[i+1].1) < HIT_TOLERANCE { return HitTestResult::Body; }
        }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        self.points.iter().enumerate().map(|(i, (ts, p))| {
            let b = timestamp_ms_to_bar_f64(bars, *ts);
            ControlPoint::index(i as u8, vp.bar_to_x_f64(b), vp.price_to_y(*p, ps.price_min, ps.price_max))
        }).collect()
    }
    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let screen: Vec<_> = self.points.iter().map(|(ts, p)| (ctx.ts_to_x_ms(*ts), ctx.price_to_y(*p))).collect();

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Draw the pattern lines (7 points connected)
        if self.show_lines {
            ctx.begin_path();
            ctx.move_to(crisp(screen[0].0, dpr), crisp(screen[0].1, dpr));
            for (x, y) in screen.iter().skip(1) {
                ctx.line_to(crisp(*x, dpr), crisp(*y, dpr));
            }
            ctx.stroke();
        }

        // Draw neckline if enabled (connecting points 2 and 4 - the lows)
        if self.show_neckline {
            ctx.set_line_dash(&[6.0, 3.0]);
            ctx.begin_path();
            ctx.move_to(crisp(screen[2].0, dpr), crisp(screen[2].1, dpr));
            ctx.line_to(crisp(screen[4].0, dpr), crisp(screen[4].1, dpr));
            // Extend neckline
            let dx = screen[4].0 - screen[2].0;
            let dy = screen[4].1 - screen[2].1;
            ctx.line_to(crisp(screen[4].0 + dx * 0.5, dpr), crisp(screen[4].1 + dy * 0.5, dpr));
            ctx.stroke();
            ctx.set_line_dash(&[]);
        }

        // Draw labels
        let labels = ["", "LS", "", "H", "", "RS", ""];
        let label_color = self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke);
        ctx.set_fill_color(label_color);
        ctx.set_font(&self.label_style.font_string());
        ctx.set_text_align(crate::render::TextAlign::Center);
        ctx.set_text_baseline(crate::render::TextBaseline::Middle);

        for (i, (x, y)) in screen.iter().enumerate() {
            if labels[i].is_empty() { continue; }
            let label = labels[i];
            let offset = if i == 3 { -self.label_style.offset_y } else { self.label_style.offset_y };

            if let Some(ref bg_color) = self.label_style.background_color {
                let text_width = ctx.measure_text(label);
                let padding = self.label_style.background_padding;
                let radius = self.label_style.background_radius;
                let bg_x = x - text_width / 2.0 - padding;
                let bg_y = y + offset - self.label_style.font_size / 2.0 - padding;
                let bg_w = text_width + padding * 2.0;
                let bg_h = self.label_style.font_size + padding * 2.0;

                ctx.set_fill_color(bg_color);
                ctx.begin_path();
                ctx.rounded_rect(bg_x, bg_y, bg_w, bg_h, radius);
                ctx.fill();

                if let Some(ref border_color) = self.label_style.border_color {
                    ctx.set_stroke_color(border_color);
                    ctx.set_stroke_width(self.label_style.border_width);
                    ctx.stroke();
                }

                ctx.set_fill_color(label_color);
            }

            ctx.fill_text(label, *x, *y + offset);
        }

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (x, y) in &screen {
                ctx.begin_path();
                ctx.arc(*x, *y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let min_y = screen.iter().fold(f64::INFINITY, |a, (_, y)| a.min(*y));
                let max_y = screen.iter().fold(f64::NEG_INFINITY, |a, (_, y)| a.max(*y));
                let min_x = screen.iter().fold(f64::INFINITY, |a, (x, _)| a.min(*x));
                let max_x = screen.iter().fold(f64::NEG_INFINITY, |a, (x, _)| a.max(*x));

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

    fn style_properties(&self) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::show_neckline(self.show_neckline)
                .with_category(PropertyCategory::Style)
                .with_order(10),
            ConfigProperty::show_lines(self.show_lines)
                .with_category(PropertyCategory::Style)
                .with_order(11),
            ConfigProperty::inverted(self.inverted)
                .with_category(PropertyCategory::Style)
                .with_order(12),
            ConfigProperty::label_font_size(self.label_style.font_size)
                .with_category(PropertyCategory::Style)
                .with_order(13),
            ConfigProperty::label_color(self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke))
                .with_category(PropertyCategory::Style)
                .with_order(14),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "show_neckline" => {
                if let Some(v) = value.as_bool() { self.show_neckline = v; return true; }
            }
            "show_lines" => {
                if let Some(v) = value.as_bool() { self.show_lines = v; return true; }
            }
            "inverted" => {
                if let Some(v) = value.as_bool() { self.inverted = v; return true; }
            }
            "label_font_size" => {
                if let Some(v) = value.as_number() { self.label_style.font_size = v; return true; }
            }
            "label_color" => {
                if let Some(c) = value.as_color() { self.label_style.color = Some(c.to_string()); return true; }
            }
            _ => {}
        }
        false
    }
}

fn point_to_line_dist(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 { return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt(); }
    let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);
    ((px - (x1 + t * dx)).powi(2) + (py - (y1 + t * dy)).powi(2)).sqrt()
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "head_shoulders", display_name: "Head & Shoulders", kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::MultiPoint(7), tooltip: "Head and shoulders reversal", icon: "head_shoulders", default_color: "#E91E63",
        factory: |points, color| {
            let mut arr = [(0i64, 0.0); 7];
            for (i, &p) in points.iter().take(7).enumerate() { arr[i] = p; }
            Box::new(HeadShoulders::new(arr, color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: true,
    }
}
