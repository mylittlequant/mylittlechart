//! Note primitive - expandable note with content
//!
//! Uses centralized PrimitiveText system for text configuration.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, PrimitiveText,
    RenderContext, crisp, contrasting_text_color,
    BOX_ANNOTATION_FILL_OPACITY, default_box_annotation_fill_opacity,
    config::{ConfigProperty, PropertyValue, PropertyCategory, PropertyGroup},
};

fn default_true() -> bool { true }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Note {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64, pub price1: f64, // Top-left corner
    #[serde(default)]
    pub ts2: i64, pub price2: f64, // Bottom-right corner
    // Legacy fields for backwards compatibility
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)] pub expanded: bool,
    /// Paint the note body fill at all — off leaves only the outline + fold.
    #[serde(default = "default_true")]
    pub fill: bool,
    /// Note fill opacity (0.0-1.0) — see `b06f9f51`, fill is a real property now
    #[serde(default = "default_box_annotation_fill_opacity")]
    pub fill_opacity: f64,
}

impl Note {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        let mut data = PrimitiveData {
            type_id: "note".to_string(),
            display_name: "Note".to_string(),
            color: PrimitiveColor::with_fill(color, color),
            width: 1.0,
            ..Default::default()
        };
        data.text = Some(PrimitiveText::new("Note"));

        Self {
            data,
            ts1, price1, ts2, price2,
            title: String::new(),
            content: String::new(),
            expanded: false,
            fill: true,
            fill_opacity: BOX_ANNOTATION_FILL_OPACITY,
        }
    }

    fn get_content(&self) -> &str {
        if let Some(ref text) = self.data.text {
            &text.content
        } else if !self.content.is_empty() {
            &self.content
        } else {
            "Note"
        }
    }
}

impl Primitive for Note {
    fn type_id(&self) -> &'static str { "note" }
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
        let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
        let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };
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

        let width = (x2 - x1).abs();
        let height = (y2 - y1).abs();
        let min_x = x1.min(x2);
        let min_y = y1.min(y2);

        let fill_base = self.data.color.fill.as_deref().unwrap_or(&self.data.color.stroke);
        let fill_color = format!("{}{:02x}", &fill_base[..fill_base.len().min(7)], (self.fill_opacity * 255.0) as u8);

        ctx.set_fill_color(&fill_color);
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);

        if self.fill { ctx.fill_rect(crisp(min_x, dpr), crisp(min_y, dpr), width, height); }
        ctx.stroke_rect(crisp(min_x, dpr), crisp(min_y, dpr), width, height);

        // Folded corner (sticky-note metaphor)
        let fold_size = (12.0_f64).min(width / 4.0).min(height / 4.0);
        let corner_x = min_x + width;
        let corner_y = min_y;
        ctx.begin_path();
        ctx.move_to(crisp(corner_x - fold_size, dpr), crisp(corner_y, dpr));
        ctx.line_to(crisp(corner_x - fold_size, dpr), crisp(corner_y + fold_size, dpr));
        ctx.line_to(crisp(corner_x, dpr), crisp(corner_y + fold_size, dpr));
        ctx.stroke();

        let content = self.get_content();
        let text_data = self.data.text.as_ref();
        let text_color = text_data.and_then(|t| t.color.as_deref()).unwrap_or_else(|| contrasting_text_color(fill_base));
        if !content.is_empty() {
            let font_size = text_data.map(|t| t.font_size).unwrap_or(12.0);
            let default_text = super::super::PrimitiveText::default();
            let h_align = text_data.map(|t| t.h_align).unwrap_or(default_text.h_align);
            ctx.set_fill_color(text_color);
            ctx.set_font(&format!("{}px sans-serif", font_size as i32));
            ctx.set_text_align(match h_align {
                super::super::TextAlign::Start => crate::render::TextAlign::Left,
                super::super::TextAlign::Center => crate::render::TextAlign::Center,
                super::super::TextAlign::End => crate::render::TextAlign::Right,
            });
            let text_x = match h_align {
                super::super::TextAlign::Start => min_x + 6.0,
                super::super::TextAlign::Center => min_x + width / 2.0,
                super::super::TextAlign::End => min_x + width - 6.0,
            };
            let center_y = min_y + height / 2.0 + font_size / 3.0;
            ctx.fill_text(content, text_x, center_y);
        }
    }

    /// Text archetype (W3, `docs/mlc/plans/archetype-modals-into-the-app-2026-08-11.md`,
    /// fifth wave): `text_tabs()` — Content/Font/Layout/Plate groups apply.
    /// `expanded` is this type's own quirk (its sticky-note collapse state)
    /// with no home in the design's groups, so it stays in the default run.
    ///
    /// Layout (W4): `render` now reads `data.text.h_align` to pick BOTH the
    /// caption's own x position along the box (start/center/end of its own
    /// width) and the canvas alignment it draws with — a two-line change in
    /// how the text origin is computed, the same shape rectangle.rs's own
    /// on-shape caption already uses. Default (Center) reproduces the fixed
    /// centred draw this box always used, byte for byte.
    ///
    /// Border/BorderColor/Rounding/Padding/Shadow have no field to back
    /// them — this box paints sharp corners at a fixed radius only for its
    /// folded-corner mark, never user-set.
    fn text_properties(&self) -> Option<Vec<ConfigProperty>> {
        let text_data = self.data.text.as_ref();
        let default_text = super::super::PrimitiveText::default();
        let content = text_data.map(|t| t.content.as_str()).unwrap_or(&self.content);
        let font_size = text_data.map(|t| t.font_size).unwrap_or(12.0);
        let fill_base = self.data.color.fill.as_deref().unwrap_or(&self.data.color.stroke);
        let color = text_data.and_then(|t| t.color.as_deref()).unwrap_or_else(|| contrasting_text_color(fill_base));
        let h_align = text_data.map(|t| t.h_align).unwrap_or(default_text.h_align);

        Some(vec![
            ConfigProperty::comment(content)
                .with_category(PropertyCategory::LabelsText)
                .with_order(0).in_group(PropertyGroup::Content),
            ConfigProperty::font_size(font_size)
                .with_category(PropertyCategory::LabelsText)
                .with_order(1).in_group(PropertyGroup::Font),
            ConfigProperty::text_color(color)
                .with_category(PropertyCategory::LabelsText)
                .with_order(2).in_group(PropertyGroup::Font),
            ConfigProperty::expanded(self.expanded)
                .with_category(PropertyCategory::LabelsText)
                .with_order(3),
            ConfigProperty::text_align_pills(h_align.as_str())
                .with_category(PropertyCategory::LabelsText)
                .with_order(4).in_group(PropertyGroup::Layout),
        ])
    }

    fn apply_text_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "content" => {
                if let Some(s) = value.as_string() {
                    let text_data = self.data.text.get_or_insert_with(|| PrimitiveText::new(""));
                    text_data.content = s.to_string();
                    self.content = s.to_string();
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
            "expanded" => {
                if let Some(v) = value.as_bool() {
                    self.expanded = v;
                    return true;
                }
            }
            "text_h_align" => {
                if let Some(s) = value.as_string() {
                    let text_data = self.data.text.get_or_insert_with(|| PrimitiveText::new(""));
                    text_data.h_align = super::super::TextAlign::from_str(s);
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    /// Plate group (fill on/off + opacity) — the design's own Background
    /// toggle/opacity, differently shaped than the lab's full border+colour+
    /// rounding+padding+shadow set but the same named group, exactly as
    /// `PropertyGroup::Plate`'s own doc records for Callout.
    fn style_properties(&self) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::fill(self.fill)
                .with_category(PropertyCategory::AppearanceFill)
                .with_order(19).in_group(PropertyGroup::Plate),
            ConfigProperty::fill_opacity(self.fill_opacity * 100.0)
                .with_category(PropertyCategory::AppearanceFill)
                .with_order(20).in_group(PropertyGroup::Plate),
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
        type_id: "note", display_name: "Note", kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Expandable note", icon: "note", default_color: "#FFC107",
        factory: |points, color| { let (t1, p1) = points.first().copied().unwrap_or((0, 0.0)); let (t2, p2) = points.get(1).copied().unwrap_or((t1+180_000, p1-20.0)); Box::new(Note::new(t1, p1, t2, p2, color)) },
        supports_text: true,
        has_levels: false,
    }
}
