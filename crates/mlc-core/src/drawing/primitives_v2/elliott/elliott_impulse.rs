//! Elliott Impulse Wave - 5-wave motive pattern

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle, CONTROL_POINT_RADIUS, CONTROL_POINT_HIT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
    config::{WaveDegree, LabelStyle, ConfigProperty, PropertyValue, PropertyCategory, SelectOption},
};

fn default_true() -> bool { true }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElliottImpulse {
    pub data: PrimitiveData,
    /// 6 points: start (0), then waves 1-5. Each is (timestamp_ms, price).
    pub points: [(i64, f64); 6],
    #[serde(default = "default_true")]
    pub show_labels: bool,
    #[serde(default)]
    pub degree: WaveDegree,
    #[serde(default)]
    pub label_style: LabelStyle,
    #[serde(default = "default_true")]
    pub show_lines: bool,
}

impl ElliottImpulse {
    pub fn new(points: [(i64, f64); 6], color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "elliott_impulse".to_string(),
                display_name: "Elliott Impulse Wave".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            points,
            show_labels: true,
            degree: WaveDegree::Intermediate,
            label_style: LabelStyle::default(),
            show_lines: true,
        }
    }

    pub fn wave_labels(&self) -> [&'static str; 6] {
        let impulse = self.degree.impulse_labels();
        ["", impulse[0], impulse[1], impulse[2], impulse[3], impulse[4]]
    }
}

impl Primitive for ElliottImpulse {
    fn type_id(&self) -> &'static str { "elliott_impulse" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Pattern }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::MultiPoint(6) }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { self.points.to_vec() }
    fn set_points(&mut self, pts: &[(i64, f64)]) { for (i, &p) in pts.iter().take(6).enumerate() { self.points[i] = p; } }
    fn translate(&mut self, td: i64, pd: f64) { for p in &mut self.points { p.0 += td; p.1 += pd; } }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Index(i) if (i as usize) < 6 => self.points[i as usize] = (ts_ms, price),
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
        for i in 0..5 {
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
        let screen: Vec<(f64, f64)> = self.points.iter().map(|(ts, price)| (ctx.ts_to_x_ms(*ts), ctx.price_to_y(*price))).collect();

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);

        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        if self.show_lines {
            ctx.begin_path();
            ctx.move_to(crisp(screen[0].0, dpr), crisp(screen[0].1, dpr));
            for &(sx, sy) in &screen[1..6] {
                ctx.line_to(crisp(sx, dpr), crisp(sy, dpr));
            }
            ctx.stroke();
        }

        ctx.set_line_dash(&[]);

        if self.show_labels {
            let labels = self.wave_labels();
            let label_color = self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke);
            ctx.set_fill_color(label_color);
            ctx.set_font(&self.label_style.font_string());
            ctx.set_text_align(crate::render::TextAlign::Center);
            ctx.set_text_baseline(crate::render::TextBaseline::Middle);

            for (i, label) in labels.iter().enumerate() {
                if label.is_empty() { continue; }
                let (x, y) = screen[i];
                let offset = if i > 0 && screen[i].1 < screen[i - 1].1 {
                    -self.label_style.offset_y
                } else {
                    self.label_style.offset_y
                };

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

                ctx.fill_text(label, x, y + offset);
            }
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
        let degree_options: Vec<SelectOption> = WaveDegree::all()
            .iter()
            .map(|d| SelectOption::new(d.as_str(), d.display_name()))
            .collect();

        vec![
            ConfigProperty::show_labels(self.show_labels).with_category(PropertyCategory::Style).with_order(10),
            ConfigProperty::show_lines(self.show_lines).with_category(PropertyCategory::Style).with_order(11),
            ConfigProperty::wave_degree(self.degree.as_str(), degree_options).with_category(PropertyCategory::Style).with_order(12),
            ConfigProperty::label_font_size(self.label_style.font_size).with_category(PropertyCategory::Style).with_order(13),
            ConfigProperty::label_color(self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke)).with_category(PropertyCategory::Style).with_order(14),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "show_labels" => { if let Some(v) = value.as_bool() { self.show_labels = v; return true; } }
            "show_lines" => { if let Some(v) = value.as_bool() { self.show_lines = v; return true; } }
            "degree" => { if let Some(s) = value.as_string() { if let Some(d) = WaveDegree::from_str(s) { self.degree = d; return true; } } }
            "label_font_size" => { if let Some(v) = value.as_number() { self.label_style.font_size = v; return true; } }
            "label_color" => { if let Some(c) = value.as_color() { self.label_style.color = Some(c.to_string()); return true; } }
            _ => {}
        }
        false
    }
}

fn point_to_line_dist(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 { return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt(); }
    let t = (((px - x1) * dx + (py - y1) * dy) / len_sq).clamp(0.0, 1.0);
    ((px - (x1 + t * dx)).powi(2) + (py - (y1 + t * dy)).powi(2)).sqrt()
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "elliott_impulse", display_name: "Elliott Impulse Wave", kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::MultiPoint(6), tooltip: "5-wave impulse pattern", icon: "elliott_impulse", default_color: "#2196F3",
        factory: |points, color| {
            let mut arr = [(0i64, 0.0f64); 6];
            for (i, &p) in points.iter().take(6).enumerate() { arr[i] = p; }
            Box::new(ElliottImpulse::new(arr, color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: true,
    }
}
