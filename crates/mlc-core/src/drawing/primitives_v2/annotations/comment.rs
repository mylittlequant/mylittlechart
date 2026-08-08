//! Comment primitive - comment marker with popup
//!
//! Uses centralized PrimitiveText system for text configuration.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, PrimitiveText,
    RenderContext, crisp, contrasting_text_color,
    PLATE_FILL_OPACITY, default_plate_fill_opacity,
    config::{ConfigProperty, PropertyValue, PropertyCategory},
};

fn default_true() -> bool { true }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Comment {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64, pub price1: f64, // Anchor point
    #[serde(default)]
    pub ts2: i64, pub price2: f64, // Size point (defines bounding box)
    // Legacy field for backwards compatibility
    #[serde(default)]
    pub text: String,
    #[serde(default)] pub author: String,
    #[serde(default)] pub timestamp: i64,
    /// Paint the bubble fill at all — off leaves only the outline + tail edge.
    #[serde(default = "default_true")]
    pub fill: bool,
    /// Bubble fill opacity (0.0-1.0) — see `b06f9f51`, fill is a real property now
    #[serde(default = "default_plate_fill_opacity")]
    pub fill_opacity: f64,
}

impl Comment {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        let mut data = PrimitiveData {
            type_id: "comment".to_string(),
            display_name: "Comment".to_string(),
            color: PrimitiveColor::with_fill(color, color),
            width: 1.0,
            ..Default::default()
        };
        data.text = Some(PrimitiveText::new("Comment"));

        Self {
            data,
            ts1, price1, ts2, price2,
            text: String::new(),
            author: String::new(),
            timestamp: 0,
            fill: true,
            fill_opacity: PLATE_FILL_OPACITY,
        }
    }

    fn get_text(&self) -> &str {
        if let Some(ref text) = self.data.text {
            &text.content
        } else if !self.text.is_empty() {
            &self.text
        } else {
            "Comment"
        }
    }
}

impl Primitive for Comment {
    fn type_id(&self) -> &'static str { "comment" }
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
    fn translate(&mut self, ts_delta_ms: i64, pd: f64) { self.ts1 += ts_delta_ms; self.ts2 += ts_delta_ms; self.price1 += pd; self.price2 += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.ts1 = ts_ms; self.price1 = price; }
            ControlPointType::Point2 => { self.ts2 = ts_ms; self.price2 = price; }
            ControlPointType::Move => { let td = ts_ms - self.ts1; let pd = price - self.price1; self.translate(td, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let (x1, y1) = (vp.bar_to_x_f64(bar1), vp.price_to_y(self.price1, ps.price_min, ps.price_max));
        let (x2, y2) = (vp.bar_to_x_f64(bar2), vp.price_to_y(self.price2, ps.price_min, ps.price_max));
        let r = 8.0;
        if (sx - x1).powi(2) + (sy - y1).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).powi(2) + (sy - y2).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        // Match render()'s minimum bubble size floor, or a same-point click
        // would draw a real bubble but hit-test a degenerate zero-size box.
        let min_x = x1.min(x2).min(x1 - 16.0);
        let max_x = (x1.max(x2)).max(x1 + 16.0);
        let min_y = y1.min(y2).min(y1 - 10.0);
        let max_y = (y1.max(y2)).max(y1 + 10.0);
        if sx >= min_x && sx <= max_x && sy >= min_y && sy <= max_y {
            return HitTestResult::Body;
        }
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
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y2 = ctx.price_to_y(self.price2);

        // Sane default size: a comment marker drawn with two near-identical
        // points (a single click) still reads as a real bubble, not a speck.
        let min_x = x1.min(x2).min(x1 - 16.0);
        let max_x = (x1.max(x2)).max(x1 + 16.0);
        let min_y = y1.min(y2).min(y1 - 10.0);
        let max_y = (y1.max(y2)).max(y1 + 10.0);
        let width = max_x - min_x;
        let height = max_y - min_y;
        let center_x = (min_x + max_x) / 2.0;
        let center_y = (min_y + max_y) / 2.0;

        let fill_base = self.data.color.fill.as_deref().unwrap_or(&self.data.color.stroke);
        let fill_color = format!("{}{:02x}", &fill_base[..fill_base.len().min(7)], (self.fill_opacity * 255.0) as u8);

        // Speech-bubble body: rounded rect + a small tail merged into the
        // bottom-left edge, matching the tool's own rail icon instead of a
        // solid circle.
        let radius = 6.0_f64.min(width / 3.0).min(height / 3.0);
        if self.fill {
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.rounded_rect(crisp(min_x, dpr), crisp(min_y, dpr), width, height, radius);
            ctx.fill();

            let tail_w = (width * 0.22).clamp(5.0, 12.0);
            let tail_h = (height * 0.35).clamp(5.0, 10.0);
            let tail_x0 = min_x + width * 0.2;
            ctx.begin_path();
            ctx.move_to(crisp(tail_x0, dpr), crisp(max_y - 1.0, dpr));
            ctx.line_to(crisp(tail_x0 + tail_w, dpr), crisp(max_y - 1.0, dpr));
            ctx.line_to(crisp(tail_x0 + tail_w * 0.3, dpr), crisp(max_y + tail_h, dpr));
            ctx.close_path();
            ctx.fill();
        }

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        ctx.begin_path();
        ctx.rounded_rect(crisp(min_x, dpr), crisp(min_y, dpr), width, height, radius);
        ctx.stroke();

        let label = self.get_text();
        let text_data = self.data.text.as_ref();
        let text_color = text_data.and_then(|t| t.color.as_deref()).unwrap_or_else(|| contrasting_text_color(fill_base));
        if !label.is_empty() {
            let font_size = text_data.map(|t| t.font_size.min(height * 0.5)).unwrap_or((height * 0.4).min(12.0)).max(8.0);
            ctx.set_fill_color(text_color);
            ctx.set_font(&format!("{}px sans-serif", font_size as i32));
            ctx.set_text_align(crate::render::TextAlign::Center);
            ctx.set_text_baseline(crate::render::TextBaseline::Middle);
            ctx.fill_text(label, center_x, center_y);
        } else {
            ctx.set_stroke_color(text_color);
            ctx.set_stroke_width(1.0);
            for i in 0..3 {
                let ly = center_y - 3.0 + (i as f64 * 3.0);
                ctx.begin_path();
                ctx.move_to(crisp(center_x - 5.0, dpr), crisp(ly, dpr));
                ctx.line_to(crisp(center_x + 5.0, dpr), crisp(ly, dpr));
                ctx.stroke();
            }
        }

    }

    fn text_properties(&self) -> Option<Vec<ConfigProperty>> {
        let text_data = self.data.text.as_ref();
        let content = text_data.map(|t| t.content.as_str()).unwrap_or(&self.text);
        let font_size = text_data.map(|t| t.font_size).unwrap_or(12.0);
        let fill_base = self.data.color.fill.as_deref().unwrap_or(&self.data.color.stroke);
        let color = text_data.and_then(|t| t.color.as_deref()).unwrap_or_else(|| contrasting_text_color(fill_base));

        Some(vec![
            ConfigProperty::comment(content)
                .with_category(PropertyCategory::LabelsText)
                .with_order(0),
            ConfigProperty::font_size(font_size)
                .with_category(PropertyCategory::LabelsText)
                .with_order(1),
            ConfigProperty::text_color(color)
                .with_category(PropertyCategory::LabelsText)
                .with_order(2),
        ])
    }

    fn apply_text_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "content" => {
                if let Some(s) = value.as_string() {
                    let text_data = self.data.text.get_or_insert_with(|| PrimitiveText::new(""));
                    text_data.content = s.to_string();
                    self.text = s.to_string();
                    return true;
                }
            }
            "font_size" => {
                if let Some(v) = value.as_number() {
                    let text_data = self.data.text.get_or_insert_with(|| PrimitiveText::new(""));
                    text_data.font_size = v;
                    return true;
                }
            }
            "text_color" => {
                if let Some(c) = value.as_color() {
                    let text_data = self.data.text.get_or_insert_with(|| PrimitiveText::new(""));
                    text_data.color = Some(c.to_string());
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::fill(self.fill)
                .with_category(PropertyCategory::AppearanceFill)
                .with_order(19),
            ConfigProperty::fill_opacity(self.fill_opacity * 100.0)
                .with_category(PropertyCategory::AppearanceFill)
                .with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "fill" => { if let Some(v) = value.as_bool() { self.fill = v; return true; } }
            "fill_opacity" => {
                if let Some(v) = value.as_number() {
                    self.fill_opacity = (v / 100.0).clamp(0.05, 1.0);
                    return true;
                }
            }
            _ => {}
        }
        false
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "comment", display_name: "Comment", kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Comment marker", icon: "comment", default_color: "#00BCD4",
        factory: |points, color| { let (t1, p1) = points.first().copied().unwrap_or((0, 0.0)); let (t2, p2) = points.get(1).copied().unwrap_or((t1+180_000, p1+20.0)); Box::new(Comment::new(t1, p1, t2, p2, color)) },
        supports_text: true,
        has_levels: false,
    }
}
