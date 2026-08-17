//! Sign primitive - icon-based marker with scalable size
//!
//! Uses TwoPoint behavior: first click sets center, second click sets size.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{SettingsKey, current_language};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, PrimitiveText,
    RenderContext, crisp,
    render_primitive_text, TextAlign,
    AREA_FILL_OPACITY, default_area_fill_opacity,
    config::{ConfigProperty, PickerOption, PropertyValue, PropertyCategory, PropertyGroup},
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
pub enum SignType { #[default] Circle, Square, Diamond, Triangle, Star, Cross, Check, X }

impl SignType {
    /// Every shape, in `render`'s own match order — the icon picker's own
    /// catalogue (W4). `Sign` itself never draws every shape in one place
    /// beyond that match, so this is the closed 8-entry list a picker binds
    /// to.
    pub fn all() -> &'static [SignType] {
        &[
            SignType::Circle,
            SignType::Square,
            SignType::Diamond,
            SignType::Triangle,
            SignType::Star,
            SignType::Cross,
            SignType::Check,
            SignType::X,
        ]
    }

    /// Wire id — `sign_type`'s own serialized value used by
    /// [`Self::from_id`] and by the picker's own click id.
    pub fn id(&self) -> &'static str {
        match self {
            SignType::Circle => "circle",
            SignType::Square => "square",
            SignType::Diamond => "diamond",
            SignType::Triangle => "triangle",
            SignType::Star => "star",
            SignType::Cross => "cross",
            SignType::Check => "check",
            SignType::X => "x",
        }
    }

    /// Parse [`Self::id`] back into a variant — the picker's own apply path.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "circle" => Some(SignType::Circle),
            "square" => Some(SignType::Square),
            "diamond" => Some(SignType::Diamond),
            "triangle" => Some(SignType::Triangle),
            "star" => Some(SignType::Star),
            "cross" => Some(SignType::Cross),
            "check" => Some(SignType::Check),
            "x" => Some(SignType::X),
            _ => None,
        }
    }

    /// Display label for the picker's own cell. Plain English, same
    /// convention `EmojiType::label` already ships (see that method's own
    /// doc for why this is not run through i18n: reused as-is, no new keys
    /// minted for this wave).
    pub fn label(&self) -> &'static str {
        match self {
            SignType::Circle => "Circle",
            SignType::Square => "Square",
            SignType::Diamond => "Diamond",
            SignType::Triangle => "Triangle",
            SignType::Star => "Star",
            SignType::Cross => "Cross",
            SignType::Check => "Check",
            SignType::X => "X",
        }
    }

    /// SVG glyph for the picker's own cell — the same outline `render`
    /// draws on the chart for this shape (same stroke-vs-fill treatment:
    /// Circle/Square/Diamond/Triangle/Star are filled, Cross/Check/X are
    /// stroked lines), never a second, differently-shaped icon set.
    pub fn svg(&self) -> &'static str {
        match self {
            SignType::Circle => r#"<svg viewBox="0 0 24 24" fill="currentColor"><circle cx="12" cy="12" r="9"/></svg>"#,
            SignType::Square => r#"<svg viewBox="0 0 24 24" fill="currentColor"><rect x="3" y="3" width="18" height="18"/></svg>"#,
            SignType::Diamond => r#"<svg viewBox="0 0 24 24" fill="currentColor"><polygon points="12,2 22,12 12,22 2,12"/></svg>"#,
            SignType::Triangle => r#"<svg viewBox="0 0 24 24" fill="currentColor"><polygon points="12,2 22,22 2,22"/></svg>"#,
            SignType::Star => r#"<svg viewBox="0 0 24 24" fill="currentColor"><polygon points="12,2 15,9 22,9 17,14 19,22 12,17 5,22 7,14 2,9 9,9"/></svg>"#,
            SignType::Cross => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><line x1="4" y1="12" x2="20" y2="12"/><line x1="12" y1="4" x2="12" y2="20"/></svg>"#,
            SignType::Check => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><polyline points="4,12 9,17 20,6"/></svg>"#,
            SignType::X => r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><line x1="5" y1="5" x2="19" y2="19"/><line x1="19" y1="5" x2="5" y2="19"/></svg>"#,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sign {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64, pub price1: f64, // Center point
    #[serde(default)]
    pub ts2: i64, pub price2: f64, // Size point
    pub sign_type: SignType,
    /// Paint the glyph body at all — off leaves only the outline + optional text.
    #[serde(default = "default_true")]
    pub fill: bool,
    /// Glyph fill opacity (0.0-1.0). These carry no text on the fill itself
    /// (any label sits outside the glyph bounds), so they are treated as
    /// free-form shapes rather than text plates — same tier `shapes/*`
    /// already uses, and their size is user-resizable and unbounded just
    /// like those shapes, not a fixed-size chip.
    #[serde(default = "default_area_fill_opacity")]
    pub fill_opacity: f64,
}
fn default_true() -> bool { true }

impl Sign {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        let mut data = PrimitiveData {
            type_id: "sign".to_string(),
            display_name: "Sign".to_string(),
            color: PrimitiveColor::new(color),
            width: 2.0,
            ..Default::default()
        };
        data.text = Some(PrimitiveText::new(""));

        Self {
            data,
            ts1, price1, ts2, price2,
            sign_type: SignType::Circle,
            fill: true,
            fill_opacity: AREA_FILL_OPACITY,
        }
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

impl Primitive for Sign {
    fn type_id(&self) -> &'static str { "sign" }
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

        // Fill is gated; the stroke outline below is NOT — a translucent
        // glyph still needs a crisp edge or it dissolves into the candles.
        let fill_color = if self.fill {
            let fill_base = self.data.color.fill.as_deref().unwrap_or(&self.data.color.stroke);
            Some(format!("{}{:02x}", &fill_base[..fill_base.len().min(7)], (self.fill_opacity * 255.0) as u8))
        } else {
            None
        };

        ctx.begin_path();
        match self.sign_type {
            SignType::Circle => {
                ctx.arc(x, y, s, 0.0, std::f64::consts::TAU);
                if let Some(ref fc) = fill_color { ctx.set_fill_color(fc); ctx.fill(); }
                ctx.stroke();
            }
            SignType::Square => {
                if let Some(ref fc) = fill_color {
                    ctx.set_fill_color(fc);
                    ctx.fill_rect(crisp(x - s, dpr), crisp(y - s, dpr), size, size);
                }
                ctx.stroke_rect(crisp(x - s, dpr), crisp(y - s, dpr), size, size);
            }
            SignType::Diamond => {
                ctx.move_to(crisp(x, dpr), crisp(y - s, dpr));
                ctx.line_to(crisp(x + s, dpr), crisp(y, dpr));
                ctx.line_to(crisp(x, dpr), crisp(y + s, dpr));
                ctx.line_to(crisp(x - s, dpr), crisp(y, dpr));
                ctx.close_path();
                if let Some(ref fc) = fill_color { ctx.set_fill_color(fc); ctx.fill(); }
                ctx.stroke();
            }
            SignType::Triangle => {
                ctx.move_to(crisp(x, dpr), crisp(y - s, dpr));
                ctx.line_to(crisp(x + s, dpr), crisp(y + s, dpr));
                ctx.line_to(crisp(x - s, dpr), crisp(y + s, dpr));
                ctx.close_path();
                if let Some(ref fc) = fill_color { ctx.set_fill_color(fc); ctx.fill(); }
                ctx.stroke();
            }
            SignType::Star => {
                for i in 0..5 {
                    let angle = (i as f64 * 72.0 - 90.0) * std::f64::consts::PI / 180.0;
                    let px = x + s * angle.cos();
                    let py = y + s * angle.sin();
                    if i == 0 { ctx.move_to(crisp(px, dpr), crisp(py, dpr)); }
                    else { ctx.line_to(crisp(px, dpr), crisp(py, dpr)); }
                }
                ctx.close_path();
                if let Some(ref fc) = fill_color { ctx.set_fill_color(fc); ctx.fill(); }
                ctx.stroke();
            }
            SignType::Cross => {
                ctx.move_to(crisp(x - s, dpr), crisp(y, dpr));
                ctx.line_to(crisp(x + s, dpr), crisp(y, dpr));
                ctx.stroke();
                ctx.begin_path();
                ctx.move_to(crisp(x, dpr), crisp(y - s, dpr));
                ctx.line_to(crisp(x, dpr), crisp(y + s, dpr));
                ctx.stroke();
            }
            SignType::Check => {
                ctx.move_to(crisp(x - s, dpr), crisp(y, dpr));
                ctx.line_to(crisp(x - s / 3.0, dpr), crisp(y + s / 2.0, dpr));
                ctx.line_to(crisp(x + s, dpr), crisp(y - s / 2.0, dpr));
                ctx.stroke();
            }
            SignType::X => {
                ctx.move_to(crisp(x - s, dpr), crisp(y - s, dpr));
                ctx.line_to(crisp(x + s, dpr), crisp(y + s, dpr));
                ctx.stroke();
                ctx.begin_path();
                ctx.move_to(crisp(x + s, dpr), crisp(y - s, dpr));
                ctx.line_to(crisp(x - s, dpr), crisp(y + s, dpr));
                ctx.stroke();
            }
        }

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
    /// `Archetype::Text` — despite living in the `annotations/` module
    /// alongside the text-shaped types (W3, fifth wave, measured this before
    /// declaring anything). Icon archetype was already done in the fourth
    /// wave for `Flag`/`Emoji`; this brings Sign in line with THAT wave
    /// instead — `icon_tabs()` (`mlc-ui-lab`'s `spec.rs`) declares a glyph
    /// picker, a Rendering group and a LabelNextToIcon group.
    ///
    /// The glyph picker (`Row::Icons`) is declared now (W4): `PropertyType::Picker`
    /// binds to `sign_type: SignType`, the 8-entry catalogue this type
    /// already carried (unlike `Flag`, which has no chosen-glyph field at
    /// all and is left out).
    ///
    /// Rendering needs no declaration: colour is the universal base-stroke
    /// row every primitive without a level ladder already gets; `size` has
    /// no single scalar to drive (`ts1,price1`/`ts2,price2`, two independent
    /// points, set `calculate_size`); `IconOutlineToggle` has nothing to
    /// gate (the stroke outline in `render` above is unconditional for
    /// every `SignType` arm).
    ///
    /// LabelNextToIcon: font size / side, real `data.text` fields `render`'s
    /// own `text_offset`/`text_x` match already reads — same pair `Flag`/
    /// `Emoji` got in the fourth wave. "Show" and the free-text content stay
    /// out for the same reason as there: no on/off field backs "Show", and
    /// the content is not separately declared here either — same as those
    /// two types, it goes through the existing Labels-tab text-content
    /// surface `supports_text: true` opens, not a per-type property.
    fn style_properties(&self) -> Vec<ConfigProperty> {
        let default_text = PrimitiveText::default();
        let lang = current_language();
        let picker_options: Vec<PickerOption> =
            SignType::all().iter().map(|t| PickerOption::new(t.id(), t.label(), t.svg())).collect();
        vec![
            ConfigProperty::fill(self.fill)
                .with_category(PropertyCategory::AppearanceFill)
                .with_order(0),
            ConfigProperty::fill_opacity(self.fill_opacity * 100.0)
                .with_category(PropertyCategory::AppearanceFill)
                .with_order(1),
            ConfigProperty::picker("icon_glyph", SettingsKey::CompareInfoSymbol.get(lang), self.sign_type.id(), picker_options)
                .with_category(PropertyCategory::AppearanceLine).with_order(10).in_group(PropertyGroup::IconGlyph),
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
            "icon_glyph" => {
                if let Some(v) = value.as_string() {
                    if let Some(t) = SignType::from_id(v) {
                        self.sign_type = t;
                        return true;
                    }
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
        type_id: "sign", display_name: "Sign", kind: PrimitiveKind::Signal,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Icon marker", icon: "sign", default_color: "#E91E63",
        factory: |points, color| {
            let (t1, p1) = points.first().copied().unwrap_or((0, 0.0));
            let (t2, p2) = points.get(1).copied().unwrap_or((t1 + 120_000, p1));
            Box::new(Sign::new(t1, p1, t2, p2, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod icon_glyph_picker_tests {
    use super::*;

    /// `style_properties()` declares exactly `SignType::all()`'s own 8
    /// entries under the `icon_glyph` id — never a narrowed or foreign
    /// catalogue.
    #[test]
    fn declares_a_picker_with_every_sign_type() {
        let sign = Sign::new(0, 0.0, 120_000, 0.0, "#E91E63");
        let props = sign.style_properties();
        let picker = props.iter().find(|p| p.id == "icon_glyph").expect("declares an icon_glyph property");
        match &picker.prop_type {
            crate::drawing::primitives_v2::config::PropertyType::Picker { options } => {
                assert_eq!(options.len(), SignType::all().len());
                for (opt, t) in options.iter().zip(SignType::all()) {
                    assert_eq!(opt.id, t.id());
                    assert_eq!(opt.label, t.label());
                    assert_eq!(opt.svg, t.svg());
                }
            }
            other => panic!("icon_glyph must be a Picker property, got {other:?}"),
        }
    }

    /// Round trip through the SAME apply path a click on the modal's grid
    /// reaches (`apply_style_property`, not a direct field write) — every
    /// catalogue entry's own id lands that exact variant in `sign_type`.
    #[test]
    fn applying_icon_glyph_by_id_sets_sign_type_for_every_entry() {
        for t in SignType::all() {
            let mut sign = Sign::new(0, 0.0, 120_000, 0.0, "#E91E63");
            let applied = sign.apply_style_property("icon_glyph", &PropertyValue::String(t.id().to_string()));
            assert!(applied, "apply_style_property must recognize icon_glyph for {t:?}");
            assert_eq!(sign.sign_type, *t);
        }
    }

    /// An unknown id is rejected rather than silently clearing the field —
    /// `from_id` returning `None` must not report success.
    #[test]
    fn applying_an_unknown_icon_glyph_id_is_rejected() {
        let mut sign = Sign::new(0, 0.0, 120_000, 0.0, "#E91E63");
        let before = sign.sign_type;
        let applied = sign.apply_style_property("icon_glyph", &PropertyValue::String("not_a_real_shape".to_string()));
        assert!(!applied);
        assert_eq!(sign.sign_type, before);
    }
}
