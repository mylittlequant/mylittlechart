//! Price Label primitive - label showing price value
//!
//! Uses centralized PrimitiveText system for text configuration.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, PrimitiveText,
    RenderContext, crisp, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    config::{ConfigProperty, PropertyValue, PropertyCategory},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceLabel {
    pub data: PrimitiveData,
    pub bar: f64,
    pub price: f64,
    // Legacy field for backwards compatibility
    #[serde(default)] pub custom_text: Option<String>,
    #[serde(default = "default_true")] pub show_line: bool,
}
fn default_true() -> bool { true }

impl PriceLabel {
    pub fn new(bar: f64, price: f64, color: &str) -> Self {
        let mut data = PrimitiveData {
            type_id: "price_label".to_string(),
            display_name: "Price Label".to_string(),
            color: PrimitiveColor::new(color),
            width: 1.0,
            ..Default::default()
        };
        // Initialize centralized text system for styling
        data.text = Some(PrimitiveText::new(""));

        Self {
            data,
            bar,
            price,
            custom_text: None,
            show_line: true,
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

impl Primitive for PriceLabel {
    fn type_id(&self) -> &'static str { "price_label" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Annotation }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::SingleClick }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(f64, f64)> { vec![(self.bar, self.price)] }
    fn set_points(&mut self, points: &[(f64, f64)]) { if let Some(&(b, p)) = points.first() { self.bar = b; self.price = p; } }
    fn translate(&mut self, bd: f64, pd: f64) { self.bar += bd; self.price += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, bar: f64, price: f64) {
        if matches!(pt, ControlPointType::Point1 | ControlPointType::Move) { self.bar = bar; self.price = price; }
    }
    fn hit_test(&self, sx: f64, sy: f64, vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let (x, y) = (vp.bar_to_x_f64(self.bar), vp.price_to_y(self.price, ps.price_min, ps.price_max));
        let size = 30.0;
        if ((sx - x).powi(2) + (sy - y).powi(2)).sqrt() < size { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        HitTestResult::Miss
    }
    fn control_points(&self, vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        vec![ControlPoint::point1(vp.bar_to_x_f64(self.bar), vp.price_to_y(self.price, ps.price_min, ps.price_max))]
    }
    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x = ctx.bar_to_x(self.bar);
        let y = ctx.price_to_y(self.price);

        // Get text styling from centralized system
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

        // Price text
        let price_text = super::super::fmt_price(self.price);

        // Measure actual text width and calculate label dimensions
        let text_width = ctx.measure_text(&price_text);
        let padding_h = font_size * 0.6;
        let padding_v = font_size * 0.3;
        let label_width = text_width + padding_h * 2.0;
        let label_height = font_size + padding_v * 2.0;

        // Draw horizontal dashed line if enabled
        if self.show_line {
            ctx.set_stroke_color(&self.data.color.stroke);
            ctx.set_stroke_width(self.data.width);
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.begin_path();
            ctx.move_to(crisp(0.0, dpr), crisp(y, dpr));
            ctx.line_to(crisp(x - label_width / 2.0 - 5.0, dpr), crisp(y, dpr));
            ctx.stroke();
            ctx.set_line_dash(&[]);
        }

        // Draw label background centered on anchor point
        ctx.set_fill_color(&self.data.color.stroke);
        ctx.fill_rect(
            crisp(x - label_width / 2.0, dpr),
            crisp(y - label_height / 2.0, dpr),
            label_width,
            label_height,
        );

        // Draw price text centered
        ctx.set_fill_color(text_color);
        ctx.set_text_align(crate::render::TextAlign::Center);
        ctx.fill_text(&price_text, x, y + font_size * 0.35);

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            ctx.begin_path();
            ctx.arc(x, y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }
    }

    fn text_properties(&self) -> Option<Vec<ConfigProperty>> {
        let text_data = self.data.text.as_ref();
        let font_size = text_data.map(|t| t.font_size).unwrap_or(12.0);
        let bold = text_data.map(|t| t.bold).unwrap_or(false);
        let italic = text_data.map(|t| t.italic).unwrap_or(false);
        let text_color = text_data.and_then(|t| t.color.as_deref()).unwrap_or(&self.data.color.stroke);

        Some(vec![
            ConfigProperty::font_size(font_size)
                .with_category(PropertyCategory::Text)
                .with_order(0),
            ConfigProperty::bold(bold)
                .with_category(PropertyCategory::Text)
                .with_order(1),
            ConfigProperty::italic(italic)
                .with_category(PropertyCategory::Text)
                .with_order(2),
            ConfigProperty::text_color(text_color)
                .with_category(PropertyCategory::Text)
                .with_order(3),
        ])
    }

    fn apply_text_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
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
            _ => {}
        }
        false
    }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::show_line(self.show_line)
                .with_category(PropertyCategory::Style)
                .with_order(0),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "show_line" => {
                if let Some(v) = value.as_bool() {
                    self.show_line = v;
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "price_label", display_name: "Price Label", kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::SingleClick, tooltip: "Price value label", icon: "price_label", default_color: "#FF9800",
        factory: |points, color| { let (b, p) = points.first().copied().unwrap_or((0.0, 0.0)); Box::new(PriceLabel::new(b, p, color)) },
        supports_text: true, // Has custom text_properties
        has_levels: false,
        has_points_config: false,
    }
}
