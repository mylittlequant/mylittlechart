//! Signpost primitive - directional sign marker
//!
//! Uses centralized PrimitiveText system for text configuration.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, PrimitiveText,
    RenderContext, crisp, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    config::{ConfigProperty, PropertyValue, PropertyCategory},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Signpost {
    pub data: PrimitiveData,
    pub ts1: i64, pub price1: f64, // Anchor point
    pub ts2: i64, pub price2: f64, // Size point (determines scale)
    // Legacy field for backwards compatibility
    #[serde(default)]
    pub text: String,
    #[serde(default)] pub direction: SignpostDirection,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
pub enum SignpostDirection { #[default] Right, Left, Up, Down }

impl Signpost {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        let mut data = PrimitiveData {
            type_id: "signpost".to_string(),
            display_name: "Signpost".to_string(),
            color: PrimitiveColor::new(color),
            width: 1.0,
            ..Default::default()
        };
        data.text = Some(PrimitiveText::new("Signpost"));

        Self {
            data,
            ts1, price1, ts2, price2,
            text: String::new(),
            direction: SignpostDirection::Right,
        }
    }

    fn get_text(&self) -> &str {
        if let Some(ref text) = self.data.text {
            &text.content
        } else if !self.text.is_empty() {
            &self.text
        } else {
            "Signpost"
        }
    }
}

impl Primitive for Signpost {
    fn type_id(&self) -> &'static str { "signpost" }
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
        let size = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt().max(30.0);
        let half_size = size / 2.0;
        if sx >= x1 - half_size && sx <= x1 + size + 10.0 && sy >= y1 - half_size && sy <= y1 + half_size {
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
    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y2 = ctx.price_to_y(self.price2);

        let size = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt().max(30.0);
        let half_height = size / 3.0;

        ctx.set_fill_color(&self.data.color.stroke);
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);

        ctx.begin_path();
        match self.direction {
            SignpostDirection::Right => {
                ctx.move_to(crisp(x1, dpr), crisp(y1 - half_height, dpr));
                ctx.line_to(crisp(x1 + size, dpr), crisp(y1 - half_height, dpr));
                ctx.line_to(crisp(x1 + size + half_height, dpr), crisp(y1, dpr));
                ctx.line_to(crisp(x1 + size, dpr), crisp(y1 + half_height, dpr));
                ctx.line_to(crisp(x1, dpr), crisp(y1 + half_height, dpr));
            }
            SignpostDirection::Left => {
                ctx.move_to(crisp(x1, dpr), crisp(y1 - half_height, dpr));
                ctx.line_to(crisp(x1 - size, dpr), crisp(y1 - half_height, dpr));
                ctx.line_to(crisp(x1 - size - half_height, dpr), crisp(y1, dpr));
                ctx.line_to(crisp(x1 - size, dpr), crisp(y1 + half_height, dpr));
                ctx.line_to(crisp(x1, dpr), crisp(y1 + half_height, dpr));
            }
            SignpostDirection::Up => {
                ctx.move_to(crisp(x1 - half_height, dpr), crisp(y1, dpr));
                ctx.line_to(crisp(x1 - half_height, dpr), crisp(y1 - size, dpr));
                ctx.line_to(crisp(x1, dpr), crisp(y1 - size - half_height, dpr));
                ctx.line_to(crisp(x1 + half_height, dpr), crisp(y1 - size, dpr));
                ctx.line_to(crisp(x1 + half_height, dpr), crisp(y1, dpr));
            }
            SignpostDirection::Down => {
                ctx.move_to(crisp(x1 - half_height, dpr), crisp(y1, dpr));
                ctx.line_to(crisp(x1 - half_height, dpr), crisp(y1 + size, dpr));
                ctx.line_to(crisp(x1, dpr), crisp(y1 + size + half_height, dpr));
                ctx.line_to(crisp(x1 + half_height, dpr), crisp(y1 + size, dpr));
                ctx.line_to(crisp(x1 + half_height, dpr), crisp(y1, dpr));
            }
        }
        ctx.close_path();
        ctx.fill();
        ctx.stroke();

        if is_selected {
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

        let label = self.get_text();
        if !label.is_empty() {
            let font_size = if let Some(ref text) = self.data.text { text.font_size } else { 12.0 };
            ctx.set_fill_color("#000000");
            ctx.set_font(&format!("{}px sans-serif", font_size as i32));
            ctx.set_text_align(crate::render::TextAlign::Center);
            let (text_x, text_y) = match self.direction {
                SignpostDirection::Right => (x1 + size / 2.0, y1 + font_size / 3.0),
                SignpostDirection::Left => (x1 - size / 2.0, y1 + font_size / 3.0),
                SignpostDirection::Up => (x1, y1 - size / 2.0 + font_size / 3.0),
                SignpostDirection::Down => (x1, y1 + size / 2.0 + font_size / 3.0),
            };
            ctx.fill_text(label, text_x, text_y);
        }
    }

    fn text_properties(&self) -> Option<Vec<ConfigProperty>> {
        let text_data = self.data.text.as_ref();
        let content = text_data.map(|t| t.content.as_str()).unwrap_or(&self.text);
        let font_size = text_data.map(|t| t.font_size).unwrap_or(12.0);
        let color = text_data.and_then(|t| t.color.as_deref()).unwrap_or("#000000");
        let dir = match self.direction {
            SignpostDirection::Right => "right",
            SignpostDirection::Left => "left",
            SignpostDirection::Up => "up",
            SignpostDirection::Down => "down",
        };

        Some(vec![
            ConfigProperty::content(content)
                .with_category(PropertyCategory::Text)
                .with_order(0),
            ConfigProperty::font_size(font_size)
                .with_category(PropertyCategory::Text)
                .with_order(1),
            ConfigProperty::text_color(color)
                .with_category(PropertyCategory::Text)
                .with_order(2),
            ConfigProperty::direction(dir)
                .with_category(PropertyCategory::Text)
                .with_order(3),
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
            "direction" => {
                if let Some(d) = value.as_string() {
                    self.direction = match d {
                        "right" => SignpostDirection::Right,
                        "left" => SignpostDirection::Left,
                        "up" => SignpostDirection::Up,
                        "down" => SignpostDirection::Down,
                        _ => SignpostDirection::Right,
                    };
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
        type_id: "signpost", display_name: "Signpost", kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Directional sign marker", icon: "signpost", default_color: "#9C27B0",
        factory: |points, color| { let (t1, p1) = points.first().copied().unwrap_or((0, 0.0)); let (t2, p2) = points.get(1).copied().unwrap_or((t1+180_000, p1)); Box::new(Signpost::new(t1, p1, t2, p2, color)) },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
