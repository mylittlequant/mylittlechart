//! Date Range - horizontal time measurement

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, ControlPointCursor, PrimitiveColor,
    HIT_TOLERANCE, CONTROL_POINT_HIT_RADIUS,
    RenderContext, crisp, LineStyle,
    render_primitive_text, TextAlign,
    config::{LabelStyle, ConfigProperty, PropertyValue, PropertyCategory, PropertyGroup, StatRow},
};
use super::super::types::{Segment, hit_test_strokes};
use super::{draw_stat_plate, duration_label};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DateRange {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64,
    #[serde(default)]
    pub ts2: i64,
    pub price: f64,
    /// Second price of the date-span box. `None` = legacy (same as `price`);
    /// the verticals then run full-height when `extend_up`/`extend_down`
    /// stay at their serde default of `true`.
    #[serde(default)]
    pub price2: Option<f64>,
    /// Verticals past the price-span box: neither = closed rectangle,
    /// one side = ray, both = full-height (the historical default).
    #[serde(default = "default_true")] pub extend_up: bool,
    #[serde(default = "default_true")] pub extend_down: bool,
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
    fn price2_or(&self) -> f64 { self.price2.unwrap_or(self.price) }

    pub fn new(ts1: i64, ts2: i64, price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "date_range".to_string(), display_name: "Date Range".to_string(), color: PrimitiveColor::new(color), width: 2.0, ..Default::default() },
            ts1, ts2, price, price2: None,
            extend_up: false, extend_down: false,
            show_bars: true, show_time: true,
            label_style: default_label_style(),
        }
    }

    /// The read-only computed-stat block's own rows — the pure decision
    /// `style_properties()` wraps into a declared property
    /// (`ConfigProperty::stat_block`). Reads through the SAME
    /// `super::duration_label` this primitive's own `render` uses for the
    /// identical on-chart stat plate. No bar-count row on purpose:
    /// `style_properties()` carries no `bars: &[Bar]` (the `Primitive`
    /// trait's own signature has none), and `render`'s own bar count is a
    /// live-chart figure derived from `ctx.bars()` — showing a bar count
    /// here would mean inventing a bar slice, not reading a real one.
    pub fn result_stat_rows(&self) -> Vec<StatRow> {
        vec![StatRow::duration(&duration_label(self.ts1, self.ts2))]
    }
}

impl Primitive for DateRange {
    fn type_id(&self) -> &'static str { "date_range" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Measurement }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, self.price), (self.ts2, self.price2_or())] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(ts, p)) = pts.first() { self.ts1 = ts; self.price = p; }
        if let Some(&(ts, p)) = pts.get(1) { self.ts2 = ts; self.price2 = Some(p); }
    }
    fn translate(&mut self, ts_delta_ms: i64, pd: f64) {
        self.ts1 += ts_delta_ms;
        self.ts2 += ts_delta_ms;
        self.price += pd;
        if let Some(p) = self.price2.as_mut() { *p += pd; }
    }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 | ControlPointType::Corner(0) => { self.ts1 = ts_ms; self.price = price; }
            ControlPointType::Corner(1) => { self.ts2 = ts_ms; self.price = price; }
            ControlPointType::Point2 | ControlPointType::Corner(2) => { self.ts2 = ts_ms; self.price2 = Some(price); }
            ControlPointType::Corner(3) => { self.ts1 = ts_ms; self.price2 = Some(price); }
            ControlPointType::Edge(0) => { self.price = price; }
            ControlPointType::Edge(1) => { self.ts2 = ts_ms; }
            ControlPointType::Edge(2) => { self.price2 = Some(price); }
            ControlPointType::Edge(3) => { self.ts1 = ts_ms; }
            ControlPointType::Move => { let td = ts_ms - self.ts1; let pd = price - self.price; self.translate(td, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2));
        let y1 = vp.price_to_y(self.price, ps.price_min, ps.price_max);
        let y2 = vp.price_to_y(self.price2_or(), ps.price_min, ps.price_max);
        let r2 = CONTROL_POINT_HIT_RADIUS.powi(2);
        let corners = [
            (x1, y1, ControlPointType::Corner(0)),
            (x2, y1, ControlPointType::Corner(1)),
            (x2, y2, ControlPointType::Corner(2)),
            (x1, y2, ControlPointType::Corner(3)),
        ];
        for (cx, cy, cp_type) in corners {
            if (sx - cx).powi(2) + (sy - cy).powi(2) <= r2 { return HitTestResult::ControlPoint(cp_type); }
        }
        let edges = [
            ((x1 + x2) / 2.0, y1, ControlPointType::Edge(0)),
            (x2, (y1 + y2) / 2.0, ControlPointType::Edge(1)),
            ((x1 + x2) / 2.0, y2, ControlPointType::Edge(2)),
            (x1, (y1 + y2) / 2.0, ControlPointType::Edge(3)),
        ];
        for (ex, ey, cp_type) in edges {
            if (sx - ex).powi(2) + (sy - ey).powi(2) <= r2 { return HitTestResult::ControlPoint(cp_type); }
        }
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        let (min_y, max_y) = (y1.min(y2), y1.max(y2));
        let (top, bot) = vertical_span(min_y, max_y, vp.chart_height, self.extend_up, self.extend_down);
        if hit_test_strokes(sx, sy, &date_verticals(min_x, max_x, top, bot), HIT_TOLERANCE) {
            return HitTestResult::Body;
        }
        if sx >= min_x - HIT_TOLERANCE && sx <= max_x + HIT_TOLERANCE
            && sy >= top - HIT_TOLERANCE && sy <= bot + HIT_TOLERANCE
        {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2));
        let y1 = vp.price_to_y(self.price, ps.price_min, ps.price_max);
        let y2 = vp.price_to_y(self.price2_or(), ps.price_min, ps.price_max);
        vec![
            ControlPoint::new(ControlPointType::Corner(0), x1, y1, ControlPointCursor::ResizeNWSE),
            ControlPoint::new(ControlPointType::Corner(1), x2, y1, ControlPointCursor::ResizeNESW),
            ControlPoint::new(ControlPointType::Corner(2), x2, y2, ControlPointCursor::ResizeNWSE),
            ControlPoint::new(ControlPointType::Corner(3), x1, y2, ControlPointCursor::ResizeNESW),
            ControlPoint::new(ControlPointType::Edge(0), (x1 + x2) / 2.0, y1, ControlPointCursor::ResizeNS),
            ControlPoint::new(ControlPointType::Edge(1), x2, (y1 + y2) / 2.0, ControlPointCursor::ResizeEW),
            ControlPoint::new(ControlPointType::Edge(2), (x1 + x2) / 2.0, y2, ControlPointCursor::ResizeNS),
            ControlPoint::new(ControlPointType::Edge(3), x1, (y1 + y2) / 2.0, ControlPointCursor::ResizeEW),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y1 = ctx.price_to_y(self.price);
        let y2 = ctx.price_to_y(self.price2_or());

        let min_x = x1.min(x2);
        let max_x = x1.max(x2);
        let min_y = y1.min(y2);
        let max_y = y1.max(y2);
        let w = max_x - min_x;
        let (top, bot) = vertical_span(min_y, max_y, ctx.chart_height(), self.extend_up, self.extend_down);
        let fill_h = bot - top;

        if w.abs() > 0.5 && fill_h.abs() > 0.5 {
            ctx.set_fill_color(&format!("{}1a", &self.data.color.stroke));
            ctx.fill_rect(crisp(min_x, dpr), crisp(top, dpr), w, fill_h);
        }

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_line_style(LineStyle::Solid);
        ctx.set_stroke_width(self.data.width);
        for edge in date_verticals(min_x, max_x, top, bot) {
            ctx.begin_path();
            ctx.move_to(crisp(edge.x1, dpr), crisp(edge.y1, dpr));
            ctx.line_to(crisp(edge.x2, dpr), crisp(edge.y2, dpr));
            ctx.stroke();
        }

        // Stat plate: elapsed time (show_time) and/or bar count (show_bars).
        // Fall back to elapsed time alone so the ruler never renders blank.
        let duration = duration_label(self.ts1, self.ts2);

        let bar1 = timestamp_ms_to_bar_f64(ctx.bars(), self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(ctx.bars(), self.ts2);
        let bar_count = (bar2 - bar1).abs().round() as i64;

        let mut lines = Vec::new();
        if self.show_time {
            lines.push(duration.clone());
        }
        if self.show_bars {
            lines.push(format!("{} bars", bar_count));
        }
        if lines.is_empty() {
            lines.push(duration);
        }
        draw_stat_plate(ctx, min_x + w / 2.0, min_y + (max_y - min_y) / 2.0, &lines, &self.label_style, &self.data.color.stroke);

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
    fn unbounded_in_price(&self) -> bool { self.extend_up || self.extend_down }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        // `PropertyGroup::Values` (W2) — the approved measurement
        // archetype's own `ValuesGroupTitle` (the "show ___" checklist),
        // matching `PriceDateRange::style_properties()`'s own choice.
        let mut props = vec![
            ConfigProperty::extend_up(self.extend_up)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(8).in_group(PropertyGroup::Extend),
            ConfigProperty::extend_down(self.extend_down)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(9).in_group(PropertyGroup::Extend),
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
            // Read-only computed-stat block (W4) — see
            // `PriceRange::style_properties()`'s own comment for why this
            // is an ungrouped default `AppearanceStats` run.
            ConfigProperty::stat_block("date_range_stats", self.result_stat_rows()).with_order(20),
        ];
        props.extend(self.label_style.style_properties_grouped(&self.data.color.stroke, 30, PropertyGroup::TextGroup));
        props
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "date_range_stats" => {}
            "extend_up" => {
                if let Some(v) = value.as_bool() { self.extend_up = v; return true; }
            }
            "extend_down" => {
                if let Some(v) = value.as_bool() { self.extend_down = v; return true; }
            }
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

/// Fill/vertical span. Segment mode stays inside the price box; each
/// checkbox grows the two date-boundary verticals (and the fill) past
/// that box. Screen y grows downward, so "up" is y=0. Top/bottom of the
/// box are never stroked — the fill's own edges are the price bounds.
fn vertical_span(min_y: f64, max_y: f64, chart_height: f64, extend_up: bool, extend_down: bool) -> (f64, f64) {
    (
        if extend_up { 0.0 } else { min_y },
        if extend_down { chart_height } else { max_y },
    )
}

/// The two date-boundary verticals — the only stroked edges. Shared by
/// `render` and `hit_test`.
fn date_verticals(min_x: f64, max_x: f64, v_top: f64, v_bot: f64) -> [Segment; 2] {
    [
        Segment::new(min_x, v_top, min_x, v_bot),
        Segment::new(max_x, v_top, max_x, v_bot),
    ]
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "date_range", display_name: "Date Range", kind: PrimitiveKind::Measurement,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Measure time difference", icon: "date_range", default_color: "#2196F3",
        factory: |points, color| {
            let (ts1, p1) = points.first().copied().unwrap_or((0, 100.0));
            let (ts2, p2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, p1));
            let mut dr = DateRange::new(ts1, ts2, p1, color);
            dr.price2 = Some(p2);
            Box::new(dr)
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod stat_block_tests {
    use super::*;

    fn sample() -> DateRange {
        // 45 minutes apart.
        DateRange::new(0, 45 * 60 * 1000, 100.0, "#2196F3")
    }

    /// `result_stat_rows` reads through the SAME `duration_label` the
    /// on-chart stat plate uses, and is untoned.
    #[test]
    fn rows_match_the_on_chart_figure() {
        let pos = sample();
        let rows = pos.result_stat_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].value, duration_label(pos.ts1, pos.ts2));
        assert_eq!(rows[0].value, "45m");
        assert!(rows[0].tone.is_none());
    }

    /// Rows stay the same regardless of `show_time`/`show_bars`.
    #[test]
    fn rows_ignore_the_on_chart_display_toggles() {
        let mut pos = sample();
        pos.show_time = false;
        pos.show_bars = false;
        assert_eq!(pos.result_stat_rows(), sample().result_stat_rows());
    }

    /// Sending the stat block's own id through the exact same apply path a
    /// click would reach does nothing.
    #[test]
    fn apply_style_property_rejects_the_stat_block_id() {
        let before = sample();
        let mut pos = sample();
        let applied = pos.apply_style_property("date_range_stats", &PropertyValue::String("hacked".to_string()));
        assert!(!applied);
        assert_eq!(pos.ts1, before.ts1);
        assert_eq!(pos.ts2, before.ts2);
    }
}
