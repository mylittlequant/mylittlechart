//! Three Drives primitive - harmonic pattern

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle, CONTROL_POINT_RADIUS, CONTROL_POINT_HIT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
    config::{LabelStyle, ConfigProperty, PropertyValue, PropertyCategory},
};

fn default_true() -> bool { true }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThreeDrives {
    pub data: PrimitiveData,
    pub points: [(f64, f64); 7], // 7 points: start, drive1, retrace1, drive2, retrace2, drive3, end
    #[serde(default = "default_true")]
    pub show_labels: bool,
    #[serde(default = "default_true")]
    pub show_ratios: bool,
    #[serde(default)]
    pub label_style: LabelStyle,
    #[serde(default = "default_true")]
    pub show_lines: bool,
}

impl ThreeDrives {
    pub fn new(points: [(f64, f64); 7], color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "three_drives".to_string(),
                display_name: "Three Drives".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            points,
            show_labels: true,
            show_ratios: true,
            label_style: LabelStyle::default(),
            show_lines: true,
        }
    }
}

impl Primitive for ThreeDrives {
    fn type_id(&self) -> &'static str { "three_drives" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Pattern }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::MultiPoint(7) }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(f64, f64)> { self.points.to_vec() }
    fn set_points(&mut self, pts: &[(f64, f64)]) { for (i, &p) in pts.iter().take(7).enumerate() { self.points[i] = p; } }
    fn translate(&mut self, bd: f64, pd: f64) { for p in &mut self.points { p.0 += bd; p.1 += pd; } }
    fn move_control_point(&mut self, pt: ControlPointType, bar: f64, price: f64) {
        match pt {
            ControlPointType::Index(i) if (i as usize) < 7 => self.points[i as usize] = (bar, price),
            ControlPointType::Move => { let bd = bar - self.points[0].0; let pd = price - self.points[0].1; self.translate(bd, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let screen: Vec<_> = self.points.iter().map(|(b, p)| (vp.bar_to_x_f64(*b), vp.price_to_y(*p, ps.price_min, ps.price_max))).collect();
        for (i, &(x, y)) in screen.iter().enumerate() {
            if (sx - x).powi(2) + (sy - y).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2) as f64 { return HitTestResult::ControlPoint(ControlPointType::Index(i as u8)); }
        }
        for i in 0..6 {
            if point_to_line_dist(sx, sy, screen[i].0, screen[i].1, screen[i+1].0, screen[i+1].1) < HIT_TOLERANCE { return HitTestResult::Body; }
        }
        HitTestResult::Miss
    }
    fn control_points(&self, vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        self.points.iter().enumerate().map(|(i, (b, p))| ControlPoint::index(i as u8, vp.bar_to_x_f64(*b), vp.price_to_y(*p, ps.price_min, ps.price_max))).collect()
    }
    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let screen: Vec<_> = self.points.iter().map(|(b, p)| (ctx.bar_to_x(*b), ctx.price_to_y(*p))).collect();
        let labels = ["", "D1", "R1", "D2", "R2", "D3", ""];

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Draw connecting lines
        if self.show_lines {
            ctx.begin_path();
            ctx.move_to(crisp(screen[0].0, dpr), crisp(screen[0].1, dpr));
            for (x, y) in screen.iter().skip(1) {
                ctx.line_to(crisp(*x, dpr), crisp(*y, dpr));
            }
            ctx.stroke();

            // Draw drive lines (connecting peaks)
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.begin_path();
            ctx.move_to(crisp(screen[1].0, dpr), crisp(screen[1].1, dpr));
            ctx.line_to(crisp(screen[3].0, dpr), crisp(screen[3].1, dpr));
            ctx.stroke();
            ctx.begin_path();
            ctx.move_to(crisp(screen[3].0, dpr), crisp(screen[3].1, dpr));
            ctx.line_to(crisp(screen[5].0, dpr), crisp(screen[5].1, dpr));
            ctx.stroke();
            ctx.set_line_dash(&[]);
        }

        // Draw labels
        if self.show_labels {
            let label_color = self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke);
            ctx.set_fill_color(label_color);
            ctx.set_font(&self.label_style.font_string());
            ctx.set_text_align(crate::render::TextAlign::Center);
            ctx.set_text_baseline(crate::render::TextBaseline::Middle);

            for (i, (x, y)) in screen.iter().enumerate() {
                let label = labels[i];
                let offset = if i % 2 == 1 { -self.label_style.offset_y } else { self.label_style.offset_y };

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
        }

        if self.show_ratios {
            let ratio_color = self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke);
            let ratio_font_size = (self.label_style.font_size - 1.0).max(9.0);
            ctx.set_font(&format!("{}px sans-serif", ratio_font_size as i32));
            ctx.set_fill_color(ratio_color);
            ctx.set_text_align(crate::render::TextAlign::Center);
            ctx.set_text_baseline(crate::render::TextBaseline::Middle);

            // Points: start=0, drive1=1, retrace1=2, drive2=3, retrace2=4, drive3=5, end=6
            let d1 = (self.points[1].1 - self.points[0].1).abs(); // start to drive1
            let r1 = (self.points[2].1 - self.points[1].1).abs(); // drive1 to retrace1
            let d2 = (self.points[3].1 - self.points[2].1).abs(); // retrace1 to drive2
            let r2 = (self.points[4].1 - self.points[3].1).abs(); // drive2 to retrace2
            let d3 = (self.points[5].1 - self.points[4].1).abs(); // retrace2 to drive3

            // R1/D1 on retrace1 leg (≈0.618)
            if d1 > 0.0 {
                draw_ratio_label(ctx, screen[1], screen[2], r1 / d1);
            }
            // D2/R1 on drive2 leg (≈1.272)
            if r1 > 0.0 {
                draw_ratio_label(ctx, screen[2], screen[3], d2 / r1);
            }
            // R2/D2 on retrace2 leg (≈0.618)
            if d2 > 0.0 {
                draw_ratio_label(ctx, screen[3], screen[4], r2 / d2);
            }
            // D3/R2 on drive3 leg (≈1.272)
            if r2 > 0.0 {
                draw_ratio_label(ctx, screen[4], screen[5], d3 / r2);
            }
        }

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (x, y) in &screen {
                ctx.begin_path();
                ctx.arc(*x, *y, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
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
            ConfigProperty::show_labels(self.show_labels)
                .with_category(PropertyCategory::Style)
                .with_order(10),
            ConfigProperty::show_lines(self.show_lines)
                .with_category(PropertyCategory::Style)
                .with_order(11),
            ConfigProperty::show_ratios(self.show_ratios)
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
            "show_labels" => {
                if let Some(v) = value.as_bool() { self.show_labels = v; return true; }
            }
            "show_lines" => {
                if let Some(v) = value.as_bool() { self.show_lines = v; return true; }
            }
            "show_ratios" => {
                if let Some(v) = value.as_bool() { self.show_ratios = v; return true; }
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

fn draw_ratio_label(ctx: &mut dyn RenderContext, p1: (f64, f64), p2: (f64, f64), ratio: f64) {
    let mid_x = (p1.0 + p2.0) / 2.0;
    let mid_y = (p1.1 + p2.1) / 2.0;
    let dx = p2.0 - p1.0;
    let dy = p2.1 - p1.1;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let ox = -dy / len * 10.0;
    let oy = dx / len * 10.0;
    ctx.fill_text(&format!("{:.3}", ratio), mid_x + ox, mid_y + oy);
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
        type_id: "three_drives", display_name: "Three Drives", kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::MultiPoint(7), tooltip: "Three drives harmonic pattern", icon: "three_drives", default_color: "#795548",
        factory: |points, color| {
            let mut arr = [(0.0, 0.0); 7];
            for (i, &p) in points.iter().take(7).enumerate() { arr[i] = p; }
            Box::new(ThreeDrives::new(arr, color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: true,
    }
}
