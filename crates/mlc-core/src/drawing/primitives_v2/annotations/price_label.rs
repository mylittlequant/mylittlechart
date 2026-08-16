//! Price Label primitive - label showing price value
//!
//! Uses centralized PrimitiveText system for text configuration.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, PrimitiveText,
    RenderContext, crisp,
    PLATE_FILL_OPACITY, default_plate_fill_opacity,
    config::{ConfigProperty, PropertyValue, PropertyCategory, PropertyGroup},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceLabel {
    pub data: PrimitiveData,
    pub ts_ms: i64,
    pub price: f64,
    // Legacy field for backwards compatibility
    #[serde(default)] pub custom_text: Option<String>,
    #[serde(default = "default_true")] pub show_line: bool,
    /// Paint the price chip fill at all — off leaves only the price text.
    #[serde(default = "default_true")]
    pub fill: bool,
    /// Chip fill opacity (0.0-1.0) — same translucent plate as the other
    /// annotations (`b06f9f51`); this chip used to be painted fully opaque.
    #[serde(default = "default_plate_fill_opacity")]
    pub fill_opacity: f64,
}
fn default_true() -> bool { true }

impl PriceLabel {
    pub fn new(ts_ms: i64, price: f64, color: &str) -> Self {
        let mut data = PrimitiveData {
            type_id: "price_label".to_string(),
            display_name: "Price Label".to_string(),
            color: PrimitiveColor::new(color),
            width: 1.0,
            ..Default::default()
        };
        data.text = Some(PrimitiveText::new(""));

        Self {
            data,
            ts_ms,
            price,
            custom_text: None,
            show_line: true,
            fill: true,
            fill_opacity: PLATE_FILL_OPACITY,
        }
    }

    fn get_font_size(&self) -> f64 {
        if let Some(ref text) = self.data.text {
            text.font_size
        } else {
            12.0
        }
    }

    /// The label's own drawn text: `custom_text` when set to a non-empty
    /// string, the formatted price otherwise — the single source of truth
    /// `render` reads, so `custom_text: None` renders byte-for-byte what
    /// `fmt_price(self.price)` alone used to produce.
    fn display_text(&self) -> String {
        self.custom_text
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| super::super::fmt_price(self.price))
    }
}

impl Primitive for PriceLabel {
    fn type_id(&self) -> &'static str { "price_label" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Annotation }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::SingleClick }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts_ms, self.price)] }
    fn set_points(&mut self, points: &[(i64, f64)]) { if let Some(&(t, p)) = points.first() { self.ts_ms = t; self.price = p; } }
    fn translate(&mut self, ts_delta_ms: i64, pd: f64) { self.ts_ms += ts_delta_ms; self.price += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        if matches!(pt, ControlPointType::Point1 | ControlPointType::Move) { self.ts_ms = ts_ms; self.price = price; }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let bar = timestamp_ms_to_bar_f64(bars, self.ts_ms);
        let (x, y) = (vp.bar_to_x_f64(bar), vp.price_to_y(self.price, ps.price_min, ps.price_max));
        let size = 30.0;
        if ((sx - x).powi(2) + (sy - y).powi(2)).sqrt() < size { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let bar = timestamp_ms_to_bar_f64(bars, self.ts_ms);
        vec![ControlPoint::point1(vp.bar_to_x_f64(bar), vp.price_to_y(self.price, ps.price_min, ps.price_max))]
    }
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let x = ctx.ts_to_x_ms(self.ts_ms);
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

        // Price text — `custom_text` when set, the formatted price
        // otherwise (`display_text`'s own doc).
        let price_text = self.display_text();

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
        if self.fill {
            let fill_base = self.data.color.fill.as_deref().unwrap_or(&self.data.color.stroke);
            let fill_color = format!("{}{:02x}", &fill_base[..fill_base.len().min(7)], (self.fill_opacity * 255.0) as u8);
            ctx.set_fill_color(&fill_color);
            ctx.fill_rect(
                crisp(x - label_width / 2.0, dpr),
                crisp(y - label_height / 2.0, dpr),
                label_width,
                label_height,
            );
        }

        // Draw price text centered
        ctx.set_fill_color(text_color);
        ctx.set_text_align(crate::render::TextAlign::Center);
        ctx.fill_text(&price_text, x, y + font_size * 0.35);

    }

    /// Text archetype (W3, `docs/mlc/plans/archetype-modals-into-the-app-2026-08-11.md`,
    /// fifth wave): `text_tabs()` — Content/Font/Plate groups apply.
    /// `custom_text` used to be a genuine dead field (declared, constructed
    /// to `None`, never read anywhere) — closed in the 2026-08-16 defect
    /// pass: `render` now draws it via `display_text()` (that method's own
    /// doc), falling back to the formatted price when unset. This IS the
    /// Content row the archetype's Content group wants — the "no free-text
    /// field" reasoning that used to keep this type out of that group no
    /// longer holds.
    fn text_properties(&self) -> Option<Vec<ConfigProperty>> {
        let text_data = self.data.text.as_ref();
        let font_size = text_data.map(|t| t.font_size).unwrap_or(12.0);
        let bold = text_data.map(|t| t.bold).unwrap_or(false);
        let italic = text_data.map(|t| t.italic).unwrap_or(false);
        let text_color = text_data.and_then(|t| t.color.as_deref()).unwrap_or(&self.data.color.stroke);

        Some(vec![
            ConfigProperty::content(self.custom_text.as_deref().unwrap_or(""))
                .with_category(PropertyCategory::LabelsText)
                .with_order(0).in_group(PropertyGroup::Content),
            ConfigProperty::font_size(font_size)
                .with_category(PropertyCategory::LabelsText)
                .with_order(1).in_group(PropertyGroup::Font),
            ConfigProperty::bold(bold)
                .with_category(PropertyCategory::LabelsText)
                .with_order(2).in_group(PropertyGroup::Font),
            ConfigProperty::italic(italic)
                .with_category(PropertyCategory::LabelsText)
                .with_order(3).in_group(PropertyGroup::Font),
            ConfigProperty::text_color(text_color)
                .with_category(PropertyCategory::LabelsText)
                .with_order(4).in_group(PropertyGroup::Font),
        ])
    }

    fn apply_text_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "content" => {
                if let Some(s) = value.as_string() {
                    self.custom_text = if s.is_empty() { None } else { Some(s.to_string()) };
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
            _ => {}
        }
        false
    }

    /// `show_line` is this type's own quirk (the dashed line out to the
    /// axis) with no home in the design's groups, kept in the default run.
    /// `fill`/`fill_opacity` are this type's Plate group — the price chip's
    /// Background toggle + opacity, per [`PropertyGroup::Plate`]'s own
    /// "differently-shaped but same-titled" placement.
    fn style_properties(&self) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::show_line(self.show_line)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(0),
            ConfigProperty::fill(self.fill)
                .with_category(PropertyCategory::AppearanceFill)
                .with_order(1).in_group(PropertyGroup::Plate),
            ConfigProperty::fill_opacity(self.fill_opacity * 100.0)
                .with_category(PropertyCategory::AppearanceFill)
                .with_order(2).in_group(PropertyGroup::Plate),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "show_line" => { if let Some(v) = value.as_bool() { self.show_line = v; return true; } }
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

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "price_label", display_name: "Price Label", kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::SingleClick, tooltip: "Price value label", icon: "price_label", default_color: "#FF9800",
        factory: |points, color| { let (t, p) = points.first().copied().unwrap_or((0, 0.0)); Box::new(PriceLabel::new(t, p, color)) },
        supports_text: true, // Has custom text_properties
        has_levels: false,
    }
}

#[cfg(test)]
mod custom_text_tests {
    use super::*;

    /// `custom_text: None` (the default, and every drawing saved before
    /// this field was wired) draws byte-for-byte what `fmt_price` alone
    /// used to produce — `display_text` is the single source of truth both
    /// `render` and this test read.
    #[test]
    fn custom_text_none_renders_the_formatted_price_unchanged() {
        let label = PriceLabel::new(0, 12345.678, "#FF9800");
        assert_eq!(label.custom_text, None, "custom_text defaults to None");
        assert_eq!(label.display_text(), super::super::super::fmt_price(label.price));
    }

    /// `custom_text: Some(_)` draws the custom string instead of the price.
    #[test]
    fn custom_text_some_draws_the_custom_string() {
        let mut label = PriceLabel::new(0, 12345.678, "#FF9800");
        label.custom_text = Some("Target".to_string());
        assert_eq!(label.display_text(), "Target");
    }
}
