//! Price Range - vertical price measurement

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle,
    render_primitive_text, TextAlign,
    config::{LabelStyle, ConfigProperty, PropertyValue, PropertyCategory},
};
use super::super::types::{Segment, hit_test_strokes};
use super::draw_stat_plate;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceRange {
    pub data: PrimitiveData,
    pub ts_ms: i64,
    pub price1: f64,
    pub price2: f64,
    #[serde(default = "default_true")] pub show_percentage: bool,
    #[serde(default = "default_true")] pub show_pips: bool,
    #[serde(default = "default_label_style")]
    pub label_style: LabelStyle,
}
fn default_true() -> bool { true }
fn default_label_style() -> LabelStyle {
    LabelStyle { font_size: 13.0, ..Default::default() }
}

impl PriceRange {
    pub fn new(ts_ms: i64, price1: f64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "price_range".to_string(), display_name: "Price Range".to_string(), color: PrimitiveColor::new(color), width: 2.0, ..Default::default() },
            ts_ms, price1, price2, show_percentage: true, show_pips: true,
            label_style: default_label_style(),
        }
    }
}

impl Primitive for PriceRange {
    fn type_id(&self) -> &'static str { "price_range" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Measurement }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts_ms, self.price1), (self.ts_ms, self.price2)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(ts, p)) = pts.first() { self.ts_ms = ts; self.price1 = p; }
        if let Some(&(_, p)) = pts.get(1) { self.price2 = p; }
    }
    fn translate(&mut self, ts_delta_ms: i64, pd: f64) { self.ts_ms += ts_delta_ms; self.price1 += pd; self.price2 += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.ts_ms = ts_ms; self.price1 = price; }
            ControlPointType::Point2 => self.price2 = price,
            ControlPointType::Move => { let td = ts_ms - self.ts_ms; let pd = price - self.price1; self.translate(td, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let b = timestamp_ms_to_bar_f64(bars, self.ts_ms);
        let x = vp.bar_to_x_f64(b);
        let y1 = vp.price_to_y(self.price1, ps.price_min, ps.price_max);
        let y2 = vp.price_to_y(self.price2, ps.price_min, ps.price_max);
        let r = 8.0;
        if (sx - x).powi(2) + (sy - y1).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x).powi(2) + (sy - y2).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        // Both horizontal lines, over their whole drawn width — same
        // geometry `render` strokes via `horizontal_lines`.
        if hit_test_strokes(sx, sy, &horizontal_lines(y1, y2, vp.chart_width), HIT_TOLERANCE) { return HitTestResult::Body; }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let b = timestamp_ms_to_bar_f64(bars, self.ts_ms);
        let x = vp.bar_to_x_f64(b);
        vec![ControlPoint::point1(x, vp.price_to_y(self.price1, ps.price_min, ps.price_max)), ControlPoint::point2(x, vp.price_to_y(self.price2, ps.price_min, ps.price_max))]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let x = ctx.ts_to_x_ms(self.ts_ms);
        let y1 = ctx.price_to_y(self.price1);
        let y2 = ctx.price_to_y(self.price2);

        let min_y = y1.min(y2);
        let max_y = y1.max(y2);
        let h = max_y - min_y;

        // Light translucent band between the two horizontal lines — a ruler
        // measures the range, it should not paint over it.
        ctx.set_fill_color(&format!("{}1a", &self.data.color.stroke));
        ctx.fill_rect(0.0, crisp(min_y, dpr), ctx.chart_width(), h);

        // Draw the two horizontal lines — same geometry `hit_test` tests via
        // `horizontal_lines`, so the drawn and clickable lines can't drift.
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_line_style(LineStyle::Solid);
        ctx.set_stroke_width(self.data.width);

        for line in horizontal_lines(y1, y2, ctx.chart_width()) {
            ctx.begin_path();
            ctx.move_to(crisp(line.x1, dpr), crisp(line.y1, dpr));
            ctx.line_to(crisp(line.x2, dpr), crisp(line.y2, dpr));
            ctx.stroke();
        }

        // Stat plate: price delta (show_pips) and/or percentage change
        // (show_percentage) — independent toggles, either can stand alone.
        let price_diff = (self.price2 - self.price1).abs();
        let percentage = if self.price1 != 0.0 {
            (price_diff / self.price1.abs()) * 100.0
        } else {
            0.0
        };

        let mut parts = Vec::new();
        if self.show_pips {
            parts.push(super::super::fmt_price(price_diff));
        }
        if self.show_percentage {
            parts.push(format!("{:.2}%", percentage));
        }
        let line = if parts.is_empty() {
            super::super::fmt_price(price_diff)
        } else {
            parts.join("  /  ")
        };

        draw_stat_plate(ctx, x + 60.0, min_y + h / 2.0, &[line], &self.label_style, &self.data.color.stroke);

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => 0.0,
                    TextAlign::Center => ctx.chart_width() / 2.0,
                    TextAlign::End => ctx.chart_width(),
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
        let mut props = vec![
            ConfigProperty::show_price_diff(self.show_pips)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(10),
            ConfigProperty::show_as_percent(self.show_percentage)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(11),
            ConfigProperty::label_font_size(self.label_style.font_size)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(13),
            ConfigProperty::label_color(self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke))
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(14),
        ];
        props.extend(self.label_style.style_properties(&self.data.color.stroke, 30));
        props
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "show_pips" => {
                if let Some(v) = value.as_bool() { self.show_pips = v; return true; }
            }
            "show_as_percent" => {
                if let Some(v) = value.as_bool() { self.show_percentage = v; return true; }
            }
            "label_font_size" => {
                if let Some(v) = value.as_number() { self.label_style.font_size = v; return true; }
            }
            "label_color" => {
                if let Some(c) = value.as_color() { self.label_style.color = Some(c.to_string()); return true; }
            }
            _ => {}
        }
        if self.label_style.apply_property(id, value) { return true; }
        false
    }
}

/// The two full-width horizontal lines — shared by `render` (draws each)
/// and `hit_test` (tests each) so the drawn lines and the clickable lines
/// can never disagree.
fn horizontal_lines(y1: f64, y2: f64, chart_width: f64) -> [Segment; 2] {
    [
        Segment::new(0.0, y1, chart_width, y1),
        Segment::new(0.0, y2, chart_width, y2),
    ]
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "price_range", display_name: "Price Range", kind: PrimitiveKind::Measurement,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Measure price difference", icon: "price_range", default_color: "#4CAF50",
        factory: |points, color| {
            let (ts, p1) = points.first().copied().unwrap_or((0, 100.0));
            let (_, p2) = points.get(1).copied().unwrap_or((ts, p1 + 10.0));
            Box::new(PriceRange::new(ts, p1, p2, color))
        },
        supports_text: true,
        has_levels: false,
    }
}
