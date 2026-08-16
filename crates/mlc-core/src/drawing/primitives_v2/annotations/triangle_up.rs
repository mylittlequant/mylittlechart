//! Triangle Up primitive - upward pointing triangle marker
//!
//! Uses TwoPoint behavior: first click sets center, second click sets size.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, PrimitiveText,
    RenderContext, crisp,
    render_primitive_text, TextAlign,
    AREA_FILL_OPACITY, default_area_fill_opacity,
    config::{ConfigProperty, PropertyValue, PropertyCategory, PropertyGroup},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TriangleUp {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64, pub price1: f64, // Center point
    #[serde(default)]
    pub ts2: i64, pub price2: f64, // Size point
    /// Paint the triangle body at all — off leaves only the outline + optional text.
    #[serde(default = "default_true")]
    pub fill: bool,
    /// Triangle fill opacity (0.0-1.0) — this glyph carries no text on its
    /// fill, so it is treated as a free-form shape (same tier `shapes/*`
    /// uses), not a text plate; its size is user-resizable and unbounded.
    #[serde(default = "default_area_fill_opacity")]
    pub fill_opacity: f64,
}
fn default_true() -> bool { true }

impl TriangleUp {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        let mut data = PrimitiveData {
            type_id: "triangle_up".to_string(),
            display_name: "Triangle Up".to_string(),
            color: PrimitiveColor::new(color),
            width: 2.0,
            ..Default::default()
        };
        data.text = Some(PrimitiveText::new(""));

        Self { data, ts1, price1, ts2, price2, fill: true, fill_opacity: AREA_FILL_OPACITY }
    }

    fn calculate_size(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> f64 {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = vp.bar_to_x_f64(bar1);
        let y1 = vp.price_to_y(self.price1, ps.price_min, ps.price_max);
        let x2 = vp.bar_to_x_f64(bar2);
        let y2 = vp.price_to_y(self.price2, ps.price_min, ps.price_max);
        ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt().max(10.0)
    }

    fn calculate_size_render(&self, ctx: &dyn RenderContext) -> f64 {
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y2 = ctx.price_to_y(self.price2);
        ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt().max(10.0)
    }
}

impl Primitive for TriangleUp {
    fn type_id(&self) -> &'static str { "triangle_up" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Signal }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, self.price1), (self.ts2, self.price2)] }
    fn set_points(&mut self, points: &[(i64, f64)]) {
        if let Some(&(t, p)) = points.first() { self.ts1 = t; self.price1 = p; }
        if let Some(&(t, p)) = points.get(1) { self.ts2 = t; self.price2 = p; }
    }
    fn translate(&mut self, ts_delta_ms: i64, pd: f64) {
        self.ts1 += ts_delta_ms; self.ts2 += ts_delta_ms;
        self.price1 += pd; self.price2 += pd;
    }
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
        let size = self.calculate_size(bars, vp, ps);
        if (sx - x1).powi(2) + (sy - y1).powi(2) <= size * size {
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
        let x = ctx.ts_to_x_ms(self.ts1);
        let y = ctx.price_to_y(self.price1);
        let size = self.calculate_size_render(ctx);
        let s = size / 2.0;

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);

        ctx.begin_path();
        ctx.move_to(crisp(x, dpr), crisp(y - s, dpr));
        ctx.line_to(crisp(x - s, dpr), crisp(y + s, dpr));
        ctx.line_to(crisp(x + s, dpr), crisp(y + s, dpr));
        ctx.close_path();
        // Fill is gated; the stroke below is NOT — a translucent triangle
        // still needs a crisp edge or it dissolves into the candles.
        if self.fill {
            let fill_base = self.data.color.fill.as_deref().unwrap_or(&self.data.color.stroke);
            let fill_color = format!("{}{:02x}", &fill_base[..fill_base.len().min(7)], (self.fill_opacity * 255.0) as u8);
            ctx.set_fill_color(&fill_color);
            ctx.fill();
        }
        ctx.stroke();

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let min_x = x - s;
                let max_x = x + s;
                let min_y = y - s;
                let max_y = y + s;
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => min_x,
                    TextAlign::Center => x,
                    TextAlign::End => max_x,
                };
                let text_y = match text.v_align {
                    TextAlign::Start => min_y - text_offset,
                    TextAlign::Center => y,
                    TextAlign::End => max_y + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    /// This type's `kind()` is `PrimitiveKind::Signal`, which
    /// `Primitive::archetype()`'s own default maps to `Archetype::Icon`, NOT
    /// `Archetype::Text` — measured before declaring anything (W3, fifth
    /// wave). Icon archetype was already done in the fourth wave for
    /// `Flag`/`Emoji`; this brings TriangleUp in line with THAT wave —
    /// `icon_tabs()` (`mlc-ui-lab`'s `spec.rs`) LabelNextToIcon group: font
    /// size / side, real `data.text` fields `render`'s own `text_offset`/
    /// `text_x` match already reads.
    ///
    /// No glyph picker to report here (unlike `Sign`'s `SignType`): this
    /// type always draws the same fixed triangle, with no chosen-glyph
    /// field at all — the same shape `Flag` was in the fourth wave.
    /// Rendering needs no declaration for the same reasons as `Sign`:
    /// colour is the universal base-stroke row, size is two independent
    /// points not one scalar, and the outline stroke is unconditional.
    /// "Show" and the free-text content stay out too — no on/off field
    /// backs "Show", and content goes through the existing Labels-tab
    /// surface `supports_text: true` opens, not a per-type property.
    fn style_properties(&self) -> Vec<ConfigProperty> {
        let default_text = PrimitiveText::default();
        vec![
            ConfigProperty::fill(self.fill)
                .with_category(PropertyCategory::AppearanceFill)
                .with_order(0),
            ConfigProperty::fill_opacity(self.fill_opacity * 100.0)
                .with_category(PropertyCategory::AppearanceFill)
                .with_order(1),
            ConfigProperty::label_font_size(self.data.text.as_ref().map(|t| t.font_size).unwrap_or(default_text.font_size))
                .with_category(PropertyCategory::AppearanceLine).with_order(20).in_group(PropertyGroup::LabelNextToIcon),
            ConfigProperty::h_align(self.data.text.as_ref().map(|t| t.h_align.as_str()).unwrap_or(default_text.h_align.as_str()))
                .with_category(PropertyCategory::AppearanceLine).with_order(21).in_group(PropertyGroup::LabelNextToIcon),
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
            "label_font_size" => {
                if let Some(v) = value.as_number() {
                    self.data.text.get_or_insert_with(PrimitiveText::default).font_size = v.clamp(8.0, 32.0);
                    return true;
                }
            }
            "text_h_align" => {
                if let PropertyValue::String(v) = value {
                    self.data.text.get_or_insert_with(PrimitiveText::default).h_align = TextAlign::from_str(v);
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
        type_id: "triangle_up", display_name: "Triangle Up", kind: PrimitiveKind::Signal,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Upward triangle marker", icon: "arrow_up", default_color: "#4CAF50",
        factory: |points, color| {
            let (t1, p1) = points.first().copied().unwrap_or((0, 0.0));
            let (t2, p2) = points.get(1).copied().unwrap_or((t1 + 120_000, p1));
            Box::new(TriangleUp::new(t1, p1, t2, p2, color))
        },
        supports_text: true,
        has_levels: false,
    }
}
