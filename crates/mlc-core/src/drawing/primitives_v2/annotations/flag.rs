//! Flag primitive - flag marker with label
//!
//! Uses centralized PrimitiveText system for text configuration.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{SettingsKey, current_language};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, Archetype, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, PrimitiveText,
    RenderContext, crisp,
    render_primitive_text, TextAlign,
    AREA_FILL_OPACITY, default_area_fill_opacity,
    config::{ConfigProperty, PropertyValue, PropertyCategory, PropertyGroup},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Flag {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64, pub price1: f64, // Anchor point (base of pole)
    #[serde(default)]
    pub ts2: i64, pub price2: f64, // Size point (determines flag size)
    // Legacy field for backwards compatibility
    #[serde(default)]
    pub text: String,
    #[serde(default = "default_flag_color")] pub flag_color: String,
    /// Paint the banner fill at all — off leaves the pole + banner outline.
    #[serde(default = "default_true")]
    pub fill: bool,
    /// Banner fill opacity (0.0-1.0) — the banner carries no text on its
    /// fill (any label sits along the pole, not overlaid on the flag), so
    /// it is treated as a free-form shape like `shapes/*`, not a text
    /// plate; its size is user-resizable and unbounded.
    #[serde(default = "default_area_fill_opacity")]
    pub fill_opacity: f64,
}
fn default_flag_color() -> String { "#F44336".to_string() }
fn default_true() -> bool { true }

impl Flag {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        let mut data = PrimitiveData {
            type_id: "flag".to_string(),
            display_name: "Flag".to_string(),
            color: PrimitiveColor::new(color),
            width: 2.0,
            ..Default::default()
        };
        data.text = Some(PrimitiveText::new("Flag"));

        Self {
            data,
            ts1, price1, ts2, price2,
            text: String::new(),
            flag_color: "#F44336".to_string(),
            fill: true,
            fill_opacity: AREA_FILL_OPACITY,
        }
    }
}

impl Primitive for Flag {
    fn type_id(&self) -> &'static str { "flag" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Annotation }
    // A user-picked banner marker, not a textual annotation — `Icon`
    // archetype, not the majority `Annotation`->`Text` default.
    fn archetype(&self) -> Archetype { Archetype::Icon }
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
        let pole_height = (y2 - y1).abs();
        let flag_width = (x2 - x1).abs().max(25.0);
        let min_x = x1.min(x1 + flag_width);
        let max_x = x1.max(x1 + flag_width);
        let min_y = y1.min(y1 - pole_height);
        let max_y = y1.max(y1 - pole_height);
        if sx >= min_x - 5.0 && sx <= max_x + 5.0 && sy >= min_y - 5.0 && sy <= max_y + 5.0 {
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

        let pole_height = (y2 - y1).abs().max(30.0);
        let flag_width = (x2 - x1).abs().max(25.0);
        let flag_height = pole_height * 0.5;

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
        ctx.line_to(crisp(x1, dpr), crisp(y1 - pole_height, dpr));
        ctx.stroke();

        let flag_direction = if x2 >= x1 { 1.0 } else { -1.0 };
        let tip_x = x1 + flag_width * flag_direction;

        // Fill is gated; the banner outline below is NOT — a translucent
        // banner still needs a crisp edge or it dissolves into the candles.
        // Outline uses `flag_color` (the banner's own hue), not the pole's
        // `data.color.stroke`, so the banner stays a self-consistent shape.
        if self.fill {
            let fill_color = format!("{}{:02x}", &self.flag_color[..self.flag_color.len().min(7)], (self.fill_opacity * 255.0) as u8);
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.move_to(crisp(x1, dpr), crisp(y1 - pole_height, dpr));
            ctx.line_to(crisp(tip_x, dpr), crisp(y1 - pole_height + flag_height * 0.5, dpr));
            ctx.line_to(crisp(x1, dpr), crisp(y1 - pole_height + flag_height, dpr));
            ctx.close_path();
            ctx.fill();
        }

        ctx.set_stroke_color(&self.flag_color);
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(y1 - pole_height, dpr));
        ctx.line_to(crisp(tip_x, dpr), crisp(y1 - pole_height + flag_height * 0.5, dpr));
        ctx.line_to(crisp(x1, dpr), crisp(y1 - pole_height + flag_height, dpr));
        ctx.close_path();
        ctx.stroke();

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let min_x = x1.min(tip_x);
                let max_x = x1.max(tip_x);
                let min_y = y1 - pole_height;
                let max_y = y1;
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => min_x,
                    TextAlign::Center => (min_x + max_x) / 2.0,
                    TextAlign::End => max_x,
                };
                let text_y = match text.v_align {
                    TextAlign::Start => min_y - text_offset,
                    TextAlign::Center => (min_y + max_y) / 2.0,
                    TextAlign::End => max_y + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }

    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        // Icon archetype (W3, `docs/mlc/plans/archetype-modals-into-the-app-2026-08-11.md`):
        // `icon_tabs()` (`mlc-ui-lab`'s `spec.rs`) — this crate's `Flag` is
        // its reference concept type — declares a glyph picker, a Rendering
        // group (colour/size/outline) and a LabelNextToIcon group on the
        // "icon" subject tab. `fill`/`fill_opacity` are a `Flag`-only quirk
        // the archetype has no group for (the lab draws no fill row at all
        // for the Icon archetype) — kept in the category's own default,
        // unnamed run, exactly as before this wave; per this plan's own
        // overriding rule a quirk with no archetype home stays visible, not
        // dropped, and an unnamed default run is a visible home.
        //
        // Rendering: colour is a real gap this wave closes — `flag_color`
        // (the BANNER's own colour, distinct from `data.color.stroke`, the
        // POLE's) is read unconditionally by `render`'s banner outline and
        // conditionally by its fill, but had no declared control at all
        // before this wave — same "field exists, nothing reaches it from
        // the modal" gap `Brush::style_properties` closed for `brush_size`.
        // `size`/`IconOutlineToggle` stay out: this primitive's on-screen
        // size comes from two independent points (`ts1,price1`/`ts2,price2`,
        // dragged directly), not one scalar a slider could drive, and the
        // banner's outline stroke is unconditional in `render` — there is no
        // on/off branch to gate.
        //
        // LabelNextToIcon: font size / side, both real `data.text` fields
        // `render`'s own `text_offset`/`text_x` match already reads — same
        // shape as `Rectangle`'s `TextOnShape` pair (W3 second wave).
        // "Show" and the free-text content itself stay out: no on/off field
        // backs "Show" (content emptiness is the only signal, as everywhere
        // else this wave has found it), and the content is already editable
        // through the existing Labels-tab text-content block — a second
        // control for the same field would duplicate it, not add one. The
        // glyph picker itself (`Row::Icons`) needs a property kind this
        // crate has none of yet (W4) — `Flag` has no chosen-icon field at
        // all (it always draws the same pole+banner shape), so unlike
        // `Emoji` there is nothing for a future picker to bind to here
        // without a new field first.
        let default_text = PrimitiveText::default();
        vec![
            ConfigProperty::fill(self.fill)
                .with_category(PropertyCategory::AppearanceFill)
                .with_order(0),
            ConfigProperty::fill_opacity(self.fill_opacity * 100.0)
                .with_category(PropertyCategory::AppearanceFill)
                .with_order(1),
            ConfigProperty::color("flag_color", SettingsKey::Color.get(current_language()), &self.flag_color)
                .with_category(PropertyCategory::AppearanceLine).with_order(20).in_group(PropertyGroup::Rendering),
            ConfigProperty::label_font_size(self.data.text.as_ref().map(|t| t.font_size).unwrap_or(default_text.font_size))
                .with_category(PropertyCategory::AppearanceLine).with_order(21).in_group(PropertyGroup::LabelNextToIcon),
            ConfigProperty::h_align(self.data.text.as_ref().map(|t| t.h_align.as_str()).unwrap_or(default_text.h_align.as_str()))
                .with_category(PropertyCategory::AppearanceLine).with_order(22).in_group(PropertyGroup::LabelNextToIcon),
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
            "flag_color" => {
                if let Some(v) = value.as_color() {
                    self.flag_color = v.to_string();
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
        type_id: "flag", display_name: "Flag", kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Flag marker", icon: "flag", default_color: "#F44336",
        factory: |points, color| { let (t1, p1) = points.first().copied().unwrap_or((0, 0.0)); let (t2, p2) = points.get(1).copied().unwrap_or((t1+600_000, p1-30.0)); Box::new(Flag::new(t1, p1, t2, p2, color)) },
        supports_text: true,
        has_levels: false,
    }
}
