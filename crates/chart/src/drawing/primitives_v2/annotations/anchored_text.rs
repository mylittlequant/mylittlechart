//! Anchored Text primitive - text anchored to price/time with optional background
//!
//! Uses centralized PrimitiveText system for text configuration.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, PrimitiveText,
    RenderContext, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
    config::{ConfigProperty, PropertyValue, PropertyCategory},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnchoredText {
    pub data: PrimitiveData,
    pub bar: f64,
    pub price: f64,
    // Legacy fields for backwards compatibility
    #[serde(default)]
    pub text: String,
    #[serde(default = "default_font_size")]
    pub font_size: f64,
    #[serde(default = "default_background")]
    pub background: bool,
}
fn default_font_size() -> f64 { 14.0 }
fn default_background() -> bool { true }

impl AnchoredText {
    pub fn new(bar: f64, price: f64, color: &str) -> Self {
        let mut data = PrimitiveData {
            type_id: "anchored_text".to_string(),
            display_name: "Anchored Text".to_string(),
            color: PrimitiveColor::new(color),
            width: 1.0,
            ..Default::default()
        };
        // Initialize centralized text system
        data.text = Some(PrimitiveText::new("Anchored Text"));

        Self {
            data,
            bar,
            price,
            text: String::new(),
            font_size: 14.0,
            background: true,
        }
    }

    fn get_text(&self) -> &str {
        if let Some(ref text) = self.data.text {
            &text.content
        } else if !self.text.is_empty() {
            &self.text
        } else {
            "Anchored Text"
        }
    }

    fn get_font_size(&self) -> f64 {
        if let Some(ref text) = self.data.text {
            text.font_size
        } else {
            self.font_size
        }
    }
}

impl Primitive for AnchoredText {
    fn type_id(&self) -> &'static str { "anchored_text" }
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
        let text_content = self.get_text();
        let font_size = self.get_font_size();
        let w = text_content.len() as f64 * font_size * 0.6;
        let h = font_size * 1.2;
        if sx >= x && sx <= x + w && sy >= y - h && sy <= y { return HitTestResult::Body; }
        HitTestResult::Miss
    }
    fn control_points(&self, vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        vec![ControlPoint::point1(vp.bar_to_x_f64(self.bar), vp.price_to_y(self.price, ps.price_min, ps.price_max))]
    }
    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let x = ctx.bar_to_x(self.bar);
        let y = ctx.price_to_y(self.price);

        // Render text directly
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                render_primitive_text(ctx, text, x, y, &self.data.color.stroke);
            }
        }

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
        let content = text_data.map(|t| t.content.as_str()).unwrap_or("");
        let font_size = text_data.map(|t| t.font_size).unwrap_or(14.0);
        let bold = text_data.map(|t| t.bold).unwrap_or(false);
        let italic = text_data.map(|t| t.italic).unwrap_or(false);
        let color = text_data.and_then(|t| t.color.as_deref()).unwrap_or(&self.data.color.stroke);

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
            ConfigProperty::text_color(color)
                .with_category(PropertyCategory::Text)
                .with_order(4),
            ConfigProperty::show_background(self.background)
                .with_category(PropertyCategory::Text)
                .with_order(5),
        ])
    }

    fn apply_text_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "background" => {
                if let Some(v) = value.as_bool() {
                    self.background = v;
                    return true;
                }
            }
            _ => {
                // Delegate to text data
                let text_data = self.data.text.get_or_insert_with(|| PrimitiveText::new(""));
                match id {
                    "content" => {
                        if let Some(s) = value.as_string() {
                            text_data.content = s.to_string();
                            return true;
                        }
                    }
                    "font_size" => {
                        if let Some(v) = value.as_number() {
                            text_data.font_size = v;
                            return true;
                        }
                    }
                    "bold" => {
                        if let Some(v) = value.as_bool() {
                            text_data.bold = v;
                            return true;
                        }
                    }
                    "italic" => {
                        if let Some(v) = value.as_bool() {
                            text_data.italic = v;
                            return true;
                        }
                    }
                    "text_color" => {
                        if let Some(c) = value.as_color() {
                            text_data.color = Some(c.to_string());
                            return true;
                        }
                    }
                    _ => {}
                }
            }
        }
        false
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "anchored_text", display_name: "Anchored Text", kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::SingleClick, tooltip: "Text anchored to price/time", icon: "anchored_text", default_color: "#FFFFFF",
        factory: |points, color| { let (b, p) = points.first().copied().unwrap_or((0.0, 0.0)); Box::new(AnchoredText::new(b, p, color)) },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
