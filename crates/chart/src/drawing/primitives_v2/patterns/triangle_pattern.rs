//! Triangle Pattern primitive - consolidation pattern

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle, CONTROL_POINT_RADIUS, CONTROL_POINT_HIT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
    config::{LabelStyle, ConfigProperty, PropertyValue, PropertyCategory},
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
pub enum TriangleType { #[default] Symmetrical, Ascending, Descending, Expanding }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrianglePattern {
    pub data: PrimitiveData,
    pub bar1: f64, pub price1_top: f64, pub price1_bottom: f64,
    pub bar2: f64, pub price2_top: f64, pub price2_bottom: f64,
    pub triangle_type: TriangleType,
    #[serde(default = "default_true")] pub show_labels: bool,
    #[serde(default)] pub label_style: LabelStyle,
    #[serde(default = "default_true")] pub show_lines: bool,
}
fn default_true() -> bool { true }

impl TrianglePattern {
    pub fn new(bar1: f64, price1_top: f64, price1_bottom: f64, bar2: f64, price2_top: f64, price2_bottom: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "triangle_pattern".to_string(), display_name: "Triangle Pattern".to_string(), color: PrimitiveColor::new(color), width: 2.0, ..Default::default() },
            bar1, price1_top, price1_bottom, bar2, price2_top, price2_bottom, triangle_type: TriangleType::Symmetrical, show_labels: true,
            label_style: LabelStyle::default(), show_lines: true,
        }
    }
}

impl Primitive for TrianglePattern {
    fn type_id(&self) -> &'static str { "triangle_pattern" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Pattern }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::MultiPoint(4) }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(f64, f64)> { vec![(self.bar1, self.price1_top), (self.bar1, self.price1_bottom), (self.bar2, self.price2_top), (self.bar2, self.price2_bottom)] }
    fn set_points(&mut self, pts: &[(f64, f64)]) {
        if let Some(&(b, p)) = pts.get(0) { self.bar1 = b; self.price1_top = p; }
        if let Some(&(_, p)) = pts.get(1) { self.price1_bottom = p; }
        if let Some(&(b, p)) = pts.get(2) { self.bar2 = b; self.price2_top = p; }
        if let Some(&(_, p)) = pts.get(3) { self.price2_bottom = p; }
    }
    fn translate(&mut self, bd: f64, pd: f64) {
        self.bar1 += bd; self.bar2 += bd;
        self.price1_top += pd; self.price1_bottom += pd; self.price2_top += pd; self.price2_bottom += pd;
    }
    fn move_control_point(&mut self, pt: ControlPointType, bar: f64, price: f64) {
        match pt {
            ControlPointType::Index(0) => { self.bar1 = bar; self.price1_top = price; }
            ControlPointType::Index(1) => { self.price1_bottom = price; }
            ControlPointType::Index(2) => { self.bar2 = bar; self.price2_top = price; }
            ControlPointType::Index(3) => { self.price2_bottom = price; }
            ControlPointType::Move => { let bd = bar - self.bar1; let pd = price - self.price1_top; self.translate(bd, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let pts = [(self.bar1, self.price1_top), (self.bar1, self.price1_bottom), (self.bar2, self.price2_top), (self.bar2, self.price2_bottom)];
        let screen: Vec<_> = pts.iter().map(|(b, p)| (vp.bar_to_x_f64(*b), vp.price_to_y(*p, ps.price_min, ps.price_max))).collect();
        for (i, &(x, y)) in screen.iter().enumerate() {
            if (sx - x).powi(2) + (sy - y).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2) as f64 { return HitTestResult::ControlPoint(ControlPointType::Index(i as u8)); }
        }
        // Check top and bottom lines
        if point_to_line_dist(sx, sy, screen[0].0, screen[0].1, screen[2].0, screen[2].1) < HIT_TOLERANCE { return HitTestResult::Body; }
        if point_to_line_dist(sx, sy, screen[1].0, screen[1].1, screen[3].0, screen[3].1) < HIT_TOLERANCE { return HitTestResult::Body; }
        HitTestResult::Miss
    }
    fn control_points(&self, vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let pts = [(self.bar1, self.price1_top), (self.bar1, self.price1_bottom), (self.bar2, self.price2_top), (self.bar2, self.price2_bottom)];
        pts.iter().enumerate().map(|(i, (b, p))| ControlPoint::index(i as u8, vp.bar_to_x_f64(*b), vp.price_to_y(*p, ps.price_min, ps.price_max))).collect()
    }
    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.bar_to_x(self.bar1);
        let y1_top = ctx.price_to_y(self.price1_top);
        let y1_bot = ctx.price_to_y(self.price1_bottom);
        let x2 = ctx.bar_to_x(self.bar2);
        let y2_top = ctx.price_to_y(self.price2_top);
        let y2_bot = ctx.price_to_y(self.price2_bottom);

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Draw trendlines and vertical bounds if enabled
        if self.show_lines {
            // Draw top trendline
            ctx.begin_path();
            ctx.move_to(crisp(x1, dpr), crisp(y1_top, dpr));
            ctx.line_to(crisp(x2, dpr), crisp(y2_top, dpr));
            ctx.stroke();

            // Draw bottom trendline
            ctx.begin_path();
            ctx.move_to(crisp(x1, dpr), crisp(y1_bot, dpr));
            ctx.line_to(crisp(x2, dpr), crisp(y2_bot, dpr));
            ctx.stroke();

            // Draw vertical bounds
            ctx.set_line_dash(&[3.0, 3.0]);
            ctx.begin_path();
            ctx.move_to(crisp(x1, dpr), crisp(y1_top, dpr));
            ctx.line_to(crisp(x1, dpr), crisp(y1_bot, dpr));
            ctx.stroke();
            ctx.begin_path();
            ctx.move_to(crisp(x2, dpr), crisp(y2_top, dpr));
            ctx.line_to(crisp(x2, dpr), crisp(y2_bot, dpr));
            ctx.stroke();
            ctx.set_line_dash(&[]);
        }

        // Fill triangle area
        ctx.set_fill_color(&format!("{}20", &self.data.color.stroke));
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(y1_top, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(y2_top, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(y2_bot, dpr));
        ctx.line_to(crisp(x1, dpr), crisp(y1_bot, dpr));
        ctx.close_path();
        ctx.fill();

        // Draw label
        if self.show_labels {
            let label_color = self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke);
            ctx.set_fill_color(label_color);
            ctx.set_font(&self.label_style.font_string());
            ctx.set_text_align(crate::render::TextAlign::Center);
            ctx.set_text_baseline(crate::render::TextBaseline::Middle);
            let label = match self.triangle_type {
                TriangleType::Symmetrical => "Sym",
                TriangleType::Ascending => "Asc",
                TriangleType::Descending => "Desc",
                TriangleType::Expanding => "Exp",
            };
            let lx = (x1 + x2) / 2.0;
            let ly = (y1_top + y1_bot + y2_top + y2_bot) / 4.0;

            // Draw background if configured
            if let Some(ref bg_color) = self.label_style.background_color {
                let text_width = ctx.measure_text(label);
                let padding = self.label_style.background_padding;
                let radius = self.label_style.background_radius;
                let bg_x = lx - text_width / 2.0 - padding;
                let bg_y = ly - self.label_style.font_size / 2.0 - padding;
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

            ctx.fill_text(label, lx, ly);
        }

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (x, y) in [(x1, y1_top), (x1, y1_bot), (x2, y2_top), (x2, y2_bot)] {
                ctx.begin_path();
                ctx.arc(x, y, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Get bounding box from all points
                let screen_points = [(x1, y1_top), (x1, y1_bot), (x2, y2_top), (x2, y2_bot)];
                let min_y = screen_points.iter().fold(f64::INFINITY, |a, (_, y)| a.min(*y));
                let max_y = screen_points.iter().fold(f64::NEG_INFINITY, |a, (_, y)| a.max(*y));
                let min_x = screen_points.iter().fold(f64::INFINITY, |a, (x, _)| a.min(*x));
                let max_x = screen_points.iter().fold(f64::NEG_INFINITY, |a, (x, _)| a.max(*x));

                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => (min_x + max_x) / 2.0,
                    super::super::TextAlign::End => max_x,
                };
                // Start = ABOVE upper boundary, Center = middle, End = BELOW lower boundary
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
            ConfigProperty::triangle_type(match self.triangle_type {
                TriangleType::Symmetrical => "symmetrical",
                TriangleType::Ascending => "ascending",
                TriangleType::Descending => "descending",
                TriangleType::Expanding => "expanding",
            })
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
            "triangle_type" => {
                if let Some(s) = value.as_string() {
                    self.triangle_type = match s {
                        "ascending" => TriangleType::Ascending,
                        "descending" => TriangleType::Descending,
                        "expanding" => TriangleType::Expanding,
                        _ => TriangleType::Symmetrical,
                    };
                    return true;
                }
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
        type_id: "triangle_pattern", display_name: "Triangle Pattern", kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::MultiPoint(4), tooltip: "Triangle consolidation pattern", icon: "triangle_pattern", default_color: "#009688",
        factory: |points, color| {
            let (b1, p1t) = points.first().copied().unwrap_or((0.0, 100.0));
            let (_, p1b) = points.get(1).copied().unwrap_or((b1, 90.0));
            let (b2, p2t) = points.get(2).copied().unwrap_or((b1 + 20.0, 97.0));
            let (_, p2b) = points.get(3).copied().unwrap_or((b2, 93.0));
            Box::new(TrianglePattern::new(b1, p1t, p1b, b2, p2t, p2b, color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: true,
    }
}
