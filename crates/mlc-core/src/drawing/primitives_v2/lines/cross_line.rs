//! Cross Line primitive
//!
//! A crosshair consisting of a horizontal and vertical line
//! crossing at a single point.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::chart::types::time_scale::format_time_full;
use crate::i18n::{ConfigKey, current_language};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle, fmt_price, draw_label_plate,
    HIT_TOLERANCE, CONTROL_POINT_HIT_RADIUS,
    RenderContext, crisp,
    render_primitive_text,
    config::{ConfigProperty, PropertyValue, LabelStyle},
};

/// Cross Line - intersecting horizontal and vertical lines
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CrossLine {
    pub data: PrimitiveData,
    pub ts_ms: i64,
    pub price: f64,
    #[serde(default = "default_true")]
    pub show_price_label: bool,
    #[serde(default = "default_true")]
    pub show_bar_label: bool,
}

fn default_true() -> bool { true }

impl CrossLine {
    pub fn new(ts_ms: i64, price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "cross_line".to_string(),
                display_name: "Cross Line".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            ts_ms, price, show_price_label: true, show_bar_label: true,
        }
    }
}

impl Primitive for CrossLine {
    fn type_id(&self) -> &'static str { "cross_line" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Line }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::SingleClick }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts_ms, self.price)] }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if let Some(&(ts, price)) = points.first() {
            self.ts_ms = ts; self.price = price;
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        self.ts_ms += ts_delta_ms; self.price += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        match point_type {
            ControlPointType::Point1 | ControlPointType::Move => {
                self.ts_ms = ts_ms; self.price = price;
            }
            _ => {}
        }
    }

    fn hit_test(&self, screen_x: f64, screen_y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> HitTestResult {
        let bar = timestamp_ms_to_bar_f64(bars, self.ts_ms);
        let cx = viewport.bar_to_x_f64(bar);
        let cy = viewport.price_to_y(self.price, price_scale.price_min, price_scale.price_max);

        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        if (screen_y - cy).abs() < HIT_TOLERANCE { return HitTestResult::Body; }
        if (screen_x - cx).abs() < HIT_TOLERANCE { return HitTestResult::Body; }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        let bar = timestamp_ms_to_bar_f64(bars, self.ts_ms);
        let cx = viewport.bar_to_x_f64(bar);
        let cy = viewport.price_to_y(self.price, price_scale.price_min, price_scale.price_max);
        vec![ControlPoint::point1(cx, cy)]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let cx = ctx.ts_to_x_ms(self.ts_ms);
        let cy = ctx.price_to_y(self.price);
        let crisp_x = crisp(cx, dpr);
        let crisp_y = crisp(cy, dpr);

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        ctx.begin_path();
        ctx.move_to(0.0, crisp_y);
        ctx.line_to(ctx.chart_width(), crisp_y);
        ctx.stroke();

        ctx.begin_path();
        ctx.move_to(crisp_x, 0.0);
        ctx.line_to(crisp_x, ctx.chart_height());
        ctx.stroke();

        ctx.set_line_dash(&[]);

        // `show_price_label`/`show_bar_label` used to be struct fields with
        // no reader anywhere. `ts_ms` is milliseconds, `format_time_full`
        // takes SECONDS, matching `Bar::timestamp`'s unit.
        if self.show_price_label || self.show_bar_label {
            let label_style = LabelStyle::default();
            ctx.set_font(&label_style.font_string());
            ctx.set_text_align(crate::render::TextAlign::Center);
            ctx.set_text_baseline(crate::render::TextBaseline::Middle);
            if self.show_price_label {
                let label = fmt_price(self.price);
                let label_x = ctx.chart_width() - 40.0;
                draw_label_plate(ctx, label_x, cy, &label, label_style.font_size, &label_style, &self.data.color.stroke);
            }
            if self.show_bar_label {
                let label = format_time_full(self.ts_ms / 1000);
                draw_label_plate(ctx, cx, 16.0, &label, label_style.font_size, &label_style, &self.data.color.stroke);
            }
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let min_x = 0.0; let max_x = ctx.chart_width();
                let min_y = 0.0; let max_y = ctx.chart_height();
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => (min_x + max_x) / 2.0,
                    super::super::TextAlign::End => max_x,
                };
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => min_y - text_offset,
                    super::super::TextAlign::Center => (min_y + max_y) / 2.0,
                    super::super::TextAlign::End => max_y + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }

    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
    fn extend_mode_raw(&self) -> u8 { 3 }
    fn unbounded_in_price(&self) -> bool { true }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        let lang = current_language();
        vec![
            ConfigProperty::show_price(self.show_price_label).with_order(20),
            ConfigProperty::boolean("show_time_label", ConfigKey::ShowTimeLabel.get(lang), self.show_bar_label).with_order(21),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "show_price" => { if let Some(v) = value.as_bool() { self.show_price_label = v; return true; } }
            "show_time_label" => { if let Some(v) = value.as_bool() { self.show_bar_label = v; return true; } }
            _ => {}
        }
        false
    }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2)
}

fn create_cross_line(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts_ms, price) = points.first().copied().unwrap_or((0, 0.0));
    Box::new(CrossLine::new(ts_ms, price, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "cross_line", display_name: "Cross Line", kind: PrimitiveKind::Line,
        click_behavior: ClickBehavior::SingleClick,
        tooltip: "Draw crossing horizontal and vertical lines",
        icon: "cross_line", default_color: "#607D8B", factory: create_cross_line,
        supports_text: false, has_levels: false,
    }
}
