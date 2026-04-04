//! Price Note primitive - note attached to a price level
//!
//! Uses centralized PrimitiveText system for text configuration.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, PrimitiveText,
    RenderContext, crisp, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
};
use super::super::config::{ConfigProperty, PropertyValue, PropertyCategory};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceNote {
    pub data: PrimitiveData,
    pub bar1: f64, pub price1: f64, // Anchor point
    pub bar2: f64, pub price2: f64, // Size point (defines bounding box)
    // Legacy field for backwards compatibility
    #[serde(default)]
    pub text: String,
    #[serde(default = "default_true")] pub show_price: bool,
}
fn default_true() -> bool { true }

impl PriceNote {
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, color: &str) -> Self {
        let mut data = PrimitiveData {
            type_id: "price_note".to_string(),
            display_name: "Price Note".to_string(),
            color: PrimitiveColor::new(color),
            width: 1.0,
            ..Default::default()
        };
        // Initialize centralized text system
        data.text = Some(PrimitiveText::new("Price Note"));

        Self {
            data,
            bar1, price1, bar2, price2,
            text: String::new(),
            show_price: true,
        }
    }

    fn get_text(&self) -> &str {
        if let Some(ref text) = self.data.text {
            &text.content
        } else if !self.text.is_empty() {
            &self.text
        } else {
            "Price Note"
        }
    }

    fn get_font_size(&self) -> f64 {
        if let Some(ref text) = self.data.text {
            text.font_size
        } else {
            12.0
        }
    }
}

impl Primitive for PriceNote {
    fn type_id(&self) -> &'static str { "price_note" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Annotation }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(f64, f64)> { vec![(self.bar1, self.price1), (self.bar2, self.price2)] }
    fn set_points(&mut self, points: &[(f64, f64)]) {
        if let Some(&(b, p)) = points.first() { self.bar1 = b; self.price1 = p; }
        if let Some(&(b, p)) = points.get(1) { self.bar2 = b; self.price2 = p; }
    }
    fn translate(&mut self, bd: f64, pd: f64) { self.bar1 += bd; self.bar2 += bd; self.price1 += pd; self.price2 += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, bar: f64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.bar1 = bar; self.price1 = price; }
            ControlPointType::Point2 => { self.bar2 = bar; self.price2 = price; }
            ControlPointType::Move => { let bd = bar - self.bar1; let pd = price - self.price1; self.translate(bd, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let (x1, y1) = (vp.bar_to_x_f64(self.bar1), vp.price_to_y(self.price1, ps.price_min, ps.price_max));
        let (x2, y2) = (vp.bar_to_x_f64(self.bar2), vp.price_to_y(self.price2, ps.price_min, ps.price_max));
        let r = 8.0;
        if (sx - x1).powi(2) + (sy - y1).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).powi(2) + (sy - y2).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        // Check body area (bounding box between the two points)
        let min_x = x1.min(x2);
        let max_x = x1.max(x2);
        let min_y = y1.min(y2);
        let max_y = y1.max(y2);
        if sx >= min_x && sx <= max_x && sy >= min_y && sy <= max_y {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }
    fn control_points(&self, vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        vec![
            ControlPoint::point1(vp.bar_to_x_f64(self.bar1), vp.price_to_y(self.price1, ps.price_min, ps.price_max)),
            ControlPoint::point2(vp.bar_to_x_f64(self.bar2), vp.price_to_y(self.price2, ps.price_min, ps.price_max)),
        ]
    }
    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.bar_to_x(self.bar1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.bar_to_x(self.bar2);
        let y2 = ctx.price_to_y(self.price2);
        let chart_width = ctx.chart_width();

        // Get text styling
        let text_data = self.data.text.as_ref();
        let font_size = self.get_font_size();
        let bold = text_data.map(|t| t.bold).unwrap_or(false);
        let italic = text_data.map(|t| t.italic).unwrap_or(false);
        let text_color = text_data.and_then(|t| t.color.as_deref()).unwrap_or("#000000");

        // Build font string and set it first for measure_text
        let font_style = match (bold, italic) {
            (true, true) => "bold italic",
            (true, false) => "bold",
            (false, true) => "italic",
            (false, false) => "",
        };
        let font = if font_style.is_empty() {
            format!("{}px sans-serif", font_size as i32)
        } else {
            format!("{} {}px sans-serif", font_style, font_size as i32)
        };
        ctx.set_font(&font);

        // Build label text
        let text_content = self.get_text();
        let label_text = if self.show_price {
            format!("{} - {}", super::super::fmt_price(self.price1), text_content)
        } else {
            text_content.to_string()
        };

        // Measure text and calculate label dimensions based on font_size
        let text_width = ctx.measure_text(&label_text);
        let padding_h = font_size * 0.5;
        let padding_v = font_size * 0.3;
        let label_width = text_width + padding_h * 2.0;
        let label_height = font_size + padding_v * 2.0;

        // Draw horizontal line from anchor point to label
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        ctx.set_line_dash(&[4.0, 4.0]);
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
        ctx.line_to(crisp(chart_width - label_width - 4.0, dpr), crisp(y1, dpr));
        ctx.stroke();
        ctx.set_line_dash(&[]);

        // Draw label background at right edge, centered on y1
        ctx.set_fill_color(&self.data.color.stroke);
        ctx.fill_rect(
            crisp(chart_width - label_width - 4.0, dpr),
            crisp(y1 - label_height / 2.0, dpr),
            label_width + 4.0,
            label_height,
        );

        // Draw label text centered in label
        ctx.set_fill_color(text_color);
        ctx.set_text_align(crate::render::TextAlign::Center);
        ctx.fill_text(&label_text, chart_width - label_width / 2.0 - 2.0, y1 + font_size * 0.35);

        // Draw control points and bounding box when selected
        if is_selected {
            // Bounding box between control points
            let min_x = x1.min(x2);
            let max_x = x1.max(x2);
            let min_y = y1.min(y2);
            let max_y = y1.max(y2);
            ctx.set_stroke_color(&self.data.color.stroke);
            ctx.set_stroke_width(1.0);
            ctx.set_line_dash(&[2.0, 2.0]);
            ctx.stroke_rect(crisp(min_x, dpr), crisp(min_y, dpr), max_x - min_x, max_y - min_y);
            ctx.set_line_dash(&[]);

            // Control points
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (px, py) in [(x1, y1), (x2, y2)] {
                ctx.begin_path();
                ctx.arc(px, py, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    fn text_properties(&self) -> Option<Vec<ConfigProperty>> {
        let text_data = self.data.text.as_ref();
        let content = text_data.map(|t| t.content.as_str()).unwrap_or("");
        let font_size = text_data.map(|t| t.font_size).unwrap_or(12.0);
        let bold = text_data.map(|t| t.bold).unwrap_or(false);
        let italic = text_data.map(|t| t.italic).unwrap_or(false);
        let text_color = text_data.and_then(|t| t.color.as_deref()).unwrap_or(&self.data.color.stroke);

        Some(vec![
            ConfigProperty::content(content)
                .with_category(PropertyCategory::Text)
                .with_order(0),
            ConfigProperty::font_size(font_size)
                .with_category(PropertyCategory::Text)
                .with_order(1),
            ConfigProperty::bold(bold)
                .with_category(PropertyCategory::Text)
                .with_order(2),
            ConfigProperty::italic(italic)
                .with_category(PropertyCategory::Text)
                .with_order(3),
            ConfigProperty::text_color(text_color)
                .with_category(PropertyCategory::Text)
                .with_order(4),
            ConfigProperty::show_price(self.show_price)
                .with_category(PropertyCategory::Text)
                .with_order(5),
        ])
    }

    fn apply_text_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "content" => {
                if let Some(s) = value.as_string() {
                    let text_data = self.data.text.get_or_insert_with(|| PrimitiveText::new(""));
                    text_data.content = s.to_string();
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
            "bold" => {
                if let Some(v) = value.as_bool() {
                    let text_data = self.data.text.get_or_insert_with(|| PrimitiveText::new(""));
                    text_data.bold = v;
                    return true;
                }
            }
            "italic" => {
                if let Some(v) = value.as_bool() {
                    let text_data = self.data.text.get_or_insert_with(|| PrimitiveText::new(""));
                    text_data.italic = v;
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
            "show_price" => {
                if let Some(v) = value.as_bool() {
                    self.show_price = v;
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
        type_id: "price_note", display_name: "Price Note", kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Note attached to price level", icon: "price_note", default_color: "#FFC107",
        factory: |points, color| { let (b1, p1) = points.first().copied().unwrap_or((0.0, 0.0)); let (b2, p2) = points.get(1).copied().unwrap_or((b1+5.0, p1+10.0)); Box::new(PriceNote::new(b1, p1, b2, p2, color)) },
        supports_text: true, // Uses standard PrimitiveText system via data.text
        has_levels: false,
        has_points_config: false,
    }
}
