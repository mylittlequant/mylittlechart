//! Projection - general projection tool

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE, point_to_line_distance,
    RenderContext, crisp, LineStyle, fmt_price, draw_label_plate,
    render_primitive_text,
    config::{ConfigProperty, PropertyValue, LabelStyle},
};

/// Standard Fibonacci extension ratios, shared by `render` and `hit_test` so
/// the drawn level lines and their hit regions never drift apart.
const FIB_LEVELS: [f64; 10] = [0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0, 1.272, 1.618, 2.0];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Projection {
    pub data: PrimitiveData,
    #[serde(default)] pub ts1: i64, pub price1: f64,
    #[serde(default)] pub ts2: i64, pub price2: f64,
    #[serde(default = "default_true")] pub show_labels: bool,
    #[serde(default = "default_true")] pub show_levels: bool,
}
fn default_true() -> bool { true }

impl Projection {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "projection".to_string(), display_name: "Projection".to_string(), color: PrimitiveColor::new(color), width: 2.0, ..Default::default() },
            ts1, price1, ts2, price2, show_labels: true, show_levels: true,
        }
    }

    /// End timestamp the extension level lines run out to — the anchor
    /// span (`ts1`..`ts2`) re-applied past `ts2`, the same
    /// derived-relative-to-the-anchors relationship `BarsPattern::
    /// projected_end_ms` uses, instead of the live viewport edge.
    fn level_end_ts_ms(&self) -> i64 {
        self.ts2 + (self.ts2 - self.ts1)
    }
}

impl Primitive for Projection {
    fn type_id(&self) -> &'static str { "projection" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Trading }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, self.price1), (self.ts2, self.price2)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(t, p)) = pts.first() { self.ts1 = t; self.price1 = p; }
        if let Some(&(t, p)) = pts.get(1) { self.ts2 = t; self.price2 = p; }
    }
    fn translate(&mut self, td: i64, pd: f64) { self.ts1 += td; self.ts2 += td; self.price1 += pd; self.price2 += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.ts1 = ts_ms; self.price1 = price; }
            ControlPointType::Point2 => { self.ts2 = ts_ms; self.price2 = price; }
            ControlPointType::Move => { let td = ts_ms - self.ts1; let pd = price - self.price1; self.translate(td, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let (x1, y1) = (vp.bar_to_x_f64(b1), vp.price_to_y(self.price1, ps.price_min, ps.price_max));
        let (x2, y2) = (vp.bar_to_x_f64(b2), vp.price_to_y(self.price2, ps.price_min, ps.price_max));
        let r = 8.0;
        if (sx - x1).powi(2) + (sy - y1).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).powi(2) + (sy - y2).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        if point_to_line_distance(sx, sy, x1, y1, x2, y2) < HIT_TOLERANCE { return HitTestResult::Body; }

        // Every extension level line drawn by `render`, over its true
        // (anchor-relative, finite) extent — only when actually rendered.
        if self.show_levels {
            let price_range = self.price2 - self.price1;
            let level_end_bar = timestamp_ms_to_bar_f64(bars, self.level_end_ts_ms());
            let level_end_x = vp.bar_to_x_f64(level_end_bar);
            for &level in &FIB_LEVELS {
                let y_level = vp.price_to_y(self.price1 + price_range * level, ps.price_min, ps.price_max);
                if point_to_line_distance(sx, sy, x2, y_level, level_end_x, y_level) < HIT_TOLERANCE {
                    return HitTestResult::Body;
                }
            }
        }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        vec![
            ControlPoint::point1(vp.bar_to_x_f64(b1), vp.price_to_y(self.price1, ps.price_min, ps.price_max)),
            ControlPoint::point2(vp.bar_to_x_f64(b2), vp.price_to_y(self.price2, ps.price_min, ps.price_max)),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y2 = ctx.price_to_y(self.price2);

        // Calculate standard Fibonacci projection levels
        let price_range = self.price2 - self.price1;

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);

        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Draw main projection line
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
        ctx.stroke();

        // Draw projection levels if enabled
        if self.show_levels {
            ctx.set_stroke_width(1.0);
            ctx.set_line_dash(&[4.0, 2.0]);

            let level_end_x = ctx.ts_to_x_ms(self.level_end_ts_ms());
            let label_style = LabelStyle::default();

            for &level in &FIB_LEVELS {
                let projected_price = self.price1 + (price_range * level);
                let y_level = ctx.price_to_y(projected_price);

                ctx.begin_path();
                ctx.move_to(crisp(x2, dpr), crisp(y_level, dpr));
                ctx.line_to(crisp(level_end_x, dpr), crisp(y_level, dpr));
                ctx.stroke();

                // `show_labels` used to be a struct field with no reader
                // anywhere — the levels drew as bare lines with no way to
                // tell which price/ratio each one carried.
                if self.show_labels {
                    let label = format!("{:.3} — {}", level, fmt_price(projected_price));
                    ctx.set_font(&label_style.font_string());
                    ctx.set_fill_color(&self.data.color.stroke);
                    ctx.set_text_align(crate::render::TextAlign::Center);
                    ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                    let label_x = (level_end_x - 50.0).max(x2 + 50.0);
                    draw_label_plate(ctx, label_x, y_level, &label, label_style.font_size, &label_style, &self.data.color.stroke);
                }
            }
        }

        ctx.set_line_dash(&[]);

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let min_x = x1.min(x2);
                let max_x = x1.max(x2);
                let min_y = y1.min(y2);
                let max_y = y1.max(y2);

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

    fn style_properties(&self) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::show_labels(self.show_labels).with_order(20),
            ConfigProperty::show_levels(self.show_levels).with_order(21),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "show_labels" => { if let Some(v) = value.as_bool() { self.show_labels = v; return true; } }
            "show_percentages" => { if let Some(v) = value.as_bool() { self.show_levels = v; return true; } }
            _ => {}
        }
        false
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "projection", display_name: "Projection", kind: PrimitiveKind::Trading,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "General projection", icon: "projection", default_color: "#607D8B",
        factory: |points, color| {
            let (t1, p1) = points.first().copied().unwrap_or((0, 100.0));
            let (t2, p2) = points.get(1).copied().unwrap_or((t1 + 72_000_000, p1 + 10.0));
            Box::new(Projection::new(t1, p1, t2, p2, color))
        },
        supports_text: true,
        has_levels: false,
    }
}
