//! Date Range - horizontal time measurement

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle,
    render_primitive_text, TextAlign,
    config::{LabelStyle, ConfigProperty, PropertyValue, PropertyCategory, PropertyGroup},
};
use super::super::types::{Segment, hit_test_strokes};
use super::draw_stat_plate;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DateRange {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64,
    #[serde(default)]
    pub ts2: i64,
    pub price: f64,
    #[serde(default = "default_true")] pub show_bars: bool,
    #[serde(default = "default_true")] pub show_time: bool,
    #[serde(default = "default_label_style")]
    pub label_style: LabelStyle,
}
fn default_true() -> bool { true }
fn default_label_style() -> LabelStyle {
    LabelStyle { font_size: 13.0, ..Default::default() }
}

impl DateRange {
    pub fn new(ts1: i64, ts2: i64, price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "date_range".to_string(), display_name: "Date Range".to_string(), color: PrimitiveColor::new(color), width: 2.0, ..Default::default() },
            ts1, ts2, price, show_bars: true, show_time: true,
            label_style: default_label_style(),
        }
    }
}

impl Primitive for DateRange {
    fn type_id(&self) -> &'static str { "date_range" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Measurement }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, self.price), (self.ts2, self.price)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(ts, p)) = pts.first() { self.ts1 = ts; self.price = p; }
        if let Some(&(ts, _)) = pts.get(1) { self.ts2 = ts; }
    }
    fn translate(&mut self, ts_delta_ms: i64, pd: f64) { self.ts1 += ts_delta_ms; self.ts2 += ts_delta_ms; self.price += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.ts1 = ts_ms; self.price = price; }
            ControlPointType::Point2 => self.ts2 = ts_ms,
            ControlPointType::Move => { let td = ts_ms - self.ts1; let pd = price - self.price; self.translate(td, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = vp.bar_to_x_f64(b1);
        let x2 = vp.bar_to_x_f64(b2);
        let y = vp.price_to_y(self.price, ps.price_min, ps.price_max);
        let r = 8.0;
        if (sx - x1).powi(2) + (sy - y).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).powi(2) + (sy - y).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        // Both vertical lines, over their whole drawn height — same
        // geometry `render` strokes via `vertical_lines`.
        if hit_test_strokes(sx, sy, &vertical_lines(x1, x2, vp.chart_height), HIT_TOLERANCE) { return HitTestResult::Body; }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let y = vp.price_to_y(self.price, ps.price_min, ps.price_max);
        vec![ControlPoint::point1(vp.bar_to_x_f64(b1), y), ControlPoint::point2(vp.bar_to_x_f64(b2), y)]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y = ctx.price_to_y(self.price);

        let min_x = x1.min(x2);
        let max_x = x1.max(x2);
        let w = max_x - min_x;

        // Light translucent band between the two vertical lines — a ruler
        // measures the range, it should not paint over it.
        ctx.set_fill_color(&format!("{}1a", &self.data.color.stroke));
        ctx.fill_rect(crisp(min_x, dpr), 0.0, w, ctx.chart_height());

        // Draw the two vertical lines — same geometry `hit_test` tests via
        // `vertical_lines`, so the drawn and clickable lines can't drift.
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_line_style(LineStyle::Solid);
        ctx.set_stroke_width(self.data.width);

        for line in vertical_lines(x1, x2, ctx.chart_height()) {
            ctx.begin_path();
            ctx.move_to(crisp(line.x1, dpr), line.y1);
            ctx.line_to(crisp(line.x2, dpr), line.y2);
            ctx.stroke();
        }

        // Stat plate: elapsed time (show_time) and/or bar count (show_bars).
        // Fall back to elapsed time alone so the ruler never renders blank.
        let duration_ms = (self.ts2 - self.ts1).abs();
        let duration_secs = duration_ms / 1000;
        let duration_label = if duration_secs >= 86400 {
            format!("{:.1}d", duration_secs as f64 / 86400.0)
        } else if duration_secs >= 3600 {
            format!("{:.1}h", duration_secs as f64 / 3600.0)
        } else if duration_secs >= 60 {
            format!("{:.0}m", duration_secs as f64 / 60.0)
        } else {
            format!("{}s", duration_secs)
        };

        let bar1 = timestamp_ms_to_bar_f64(ctx.bars(), self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(ctx.bars(), self.ts2);
        let bar_count = (bar2 - bar1).abs().round() as i64;

        let mut lines = Vec::new();
        if self.show_time {
            lines.push(duration_label.clone());
        }
        if self.show_bars {
            lines.push(format!("{} bars", bar_count));
        }
        if lines.is_empty() {
            lines.push(duration_label);
        }
        draw_stat_plate(ctx, min_x + w / 2.0, y - 20.0, &lines, &self.label_style, &self.data.color.stroke);

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => min_x,
                    TextAlign::Center => (min_x + max_x) / 2.0,
                    TextAlign::End => max_x,
                };
                let text_y = match text.v_align {
                    TextAlign::Start => 0.0 - text_offset,
                    TextAlign::Center => ctx.chart_height() / 2.0,
                    TextAlign::End => ctx.chart_height() + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        // `PropertyGroup::Values` (W2) — the approved measurement
        // archetype's own `ValuesGroupTitle` (the "show ___" checklist),
        // matching `PriceDateRange::style_properties()`'s own choice.
        let mut props = vec![
            ConfigProperty::show_duration(self.show_time)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(10).in_group(PropertyGroup::Values),
            ConfigProperty::show_bar_count(self.show_bars)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(11).in_group(PropertyGroup::Values),
            // `PropertyGroup::TextGroup` (W2) — the approved design's own
            // measurement "labels" tab text group (`SettingsKey::Text`),
            // matching `PriceDateRange::style_properties()`'s own choice.
            ConfigProperty::label_font_size(self.label_style.font_size)
                .with_category(PropertyCategory::LabelsText)
                .with_order(13).in_group(PropertyGroup::TextGroup),
            ConfigProperty::label_color(self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke))
                .with_category(PropertyCategory::LabelsText)
                .with_order(14).in_group(PropertyGroup::TextGroup),
        ];
        props.extend(self.label_style.style_properties_grouped(&self.data.color.stroke, 30, PropertyGroup::TextGroup));
        props
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "show_time" => {
                if let Some(v) = value.as_bool() { self.show_time = v; return true; }
            }
            "show_bars" => {
                if let Some(v) = value.as_bool() { self.show_bars = v; return true; }
            }
            "label_font_size" => {
                if let Some(v) = value.as_number() { self.label_style.font_size = v; return true; }
            }
            "label_color" => {
                if let Some(c) = value.as_color() { self.label_style.color = Some(c.to_string()); return true; }
            }
            _ => {}
        }
        if self.label_style.apply_property(id, value, &self.data.color.stroke) { return true; }
        false
    }
}

/// The two full-height vertical lines — shared by `render` (draws each) and
/// `hit_test` (tests each) so the drawn lines and the clickable lines can
/// never disagree.
fn vertical_lines(x1: f64, x2: f64, chart_height: f64) -> [Segment; 2] {
    [
        Segment::new(x1, 0.0, x1, chart_height),
        Segment::new(x2, 0.0, x2, chart_height),
    ]
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "date_range", display_name: "Date Range", kind: PrimitiveKind::Measurement,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Measure time difference", icon: "date_range", default_color: "#2196F3",
        factory: |points, color| {
            let (ts1, p) = points.first().copied().unwrap_or((0, 100.0));
            let (ts2, _) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, p));
            Box::new(DateRange::new(ts1, ts2, p, color))
        },
        supports_text: true,
        has_levels: false,
    }
}
