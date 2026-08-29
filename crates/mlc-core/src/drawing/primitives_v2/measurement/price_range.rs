//! Price Range - vertical price measurement

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
use super::{draw_stat_plate, price_diff, percentage};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceRange {
    pub data: PrimitiveData,
    pub ts_ms: i64,
    /// Second stem timestamp. `0` = same as `ts_ms` (legacy vertical stem).
    #[serde(default)]
    pub ts2: i64,
    pub price1: f64,
    pub price2: f64,
    #[serde(default = "default_true")] pub show_percentage: bool,
    #[serde(default = "default_true")] pub show_pips: bool,
    /// Horizontals past the date-span box: neither = closed rectangle
    /// (segment), one side = ray, both = infinite line. Serde default is
    /// `true` so drawings saved before this field existed keep the
    /// historical full-width levels; `new()` starts with both off so a
    /// fresh tool is a box like Price/Date Range.
    #[serde(default = "default_true")] pub extend_left: bool,
    #[serde(default = "default_true")] pub extend_right: bool,
    #[serde(default = "default_label_style")]
    pub label_style: LabelStyle,
}
fn default_true() -> bool { true }
fn default_label_style() -> LabelStyle {
    LabelStyle { font_size: 13.0, ..Default::default() }
}

impl PriceRange {
    fn stem_ts2(&self) -> i64 { if self.ts2 == 0 { self.ts_ms } else { self.ts2 } }

    pub fn new(ts_ms: i64, price1: f64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "price_range".to_string(), display_name: "Price Range".to_string(), color: PrimitiveColor::new(color), width: 2.0, ..Default::default() },
            ts_ms, ts2: 0, price1, price2, show_percentage: true, show_pips: true,
            extend_left: false, extend_right: false,
            label_style: default_label_style(),
        }
    }

    /// The read-only computed-stat block's own rows — the pure decision
    /// `style_properties()` wraps into a declared property
    /// (`ConfigProperty::stat_block`). Reads through the SAME
    /// `super::price_diff`/`super::percentage` this primitive's own
    /// `render` uses for the identical on-chart stat plate, so the modal
    /// and the chart can never disagree about the number. Unconditional —
    /// shown regardless of `show_pips`/`show_percentage` (those toggles
    /// gate the on-chart PLATE only, matching Position's own Result block,
    /// which likewise ignores `show_pnl`).
    pub fn result_stat_rows(&self) -> Vec<StatRow> {
        vec![
            StatRow::price_change(&super::super::fmt_price(price_diff(self.price1, self.price2))),
            StatRow::percent_change(&format!("{:.2}%", percentage(self.price1, self.price2))),
        ]
    }
}

impl Primitive for PriceRange {
    fn type_id(&self) -> &'static str { "price_range" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Measurement }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> {
        vec![(self.ts_ms, self.price1), (self.stem_ts2(), self.price2)]
    }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(ts, p)) = pts.first() { self.ts_ms = ts; self.price1 = p; }
        if let Some(&(ts, p)) = pts.get(1) { self.ts2 = ts; self.price2 = p; }
    }
    fn translate(&mut self, ts_delta_ms: i64, pd: f64) {
        self.ts_ms += ts_delta_ms;
        if self.ts2 != 0 { self.ts2 += ts_delta_ms; }
        self.price1 += pd; self.price2 += pd;
    }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            // Same corner/edge convention as `PriceDateRange` / `Rectangle`.
            ControlPointType::Point1 | ControlPointType::Corner(0) => { self.ts_ms = ts_ms; self.price1 = price; }
            ControlPointType::Corner(1) => { self.ts2 = ts_ms; self.price1 = price; }
            ControlPointType::Point2 | ControlPointType::Corner(2) => { self.ts2 = ts_ms; self.price2 = price; }
            ControlPointType::Corner(3) => { self.ts_ms = ts_ms; self.price2 = price; }
            ControlPointType::Edge(0) => { self.price1 = price; }
            ControlPointType::Edge(1) => { self.ts2 = ts_ms; }
            ControlPointType::Edge(2) => { self.price2 = price; }
            ControlPointType::Edge(3) => { self.ts_ms = ts_ms; }
            ControlPointType::Move => { let td = ts_ms - self.ts_ms; let pd = price - self.price1; self.translate(td, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts_ms));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.stem_ts2()));
        let y1 = vp.price_to_y(self.price1, ps.price_min, ps.price_max);
        let y2 = vp.price_to_y(self.price2, ps.price_min, ps.price_max);
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
        let (left, right) = horizontal_span(min_x, max_x, vp.chart_width, self.extend_left, self.extend_right);
        if hit_test_strokes(sx, sy, &price_horizontals(left, right, min_y, max_y), HIT_TOLERANCE) {
            return HitTestResult::Body;
        }
        if sx >= left - HIT_TOLERANCE && sx <= right + HIT_TOLERANCE
            && sy >= min_y - HIT_TOLERANCE && sy <= max_y + HIT_TOLERANCE
        {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts_ms));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.stem_ts2()));
        let y1 = vp.price_to_y(self.price1, ps.price_min, ps.price_max);
        let y2 = vp.price_to_y(self.price2, ps.price_min, ps.price_max);
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
        let x1 = ctx.ts_to_x_ms(self.ts_ms);
        let x2 = ctx.ts_to_x_ms(self.stem_ts2());
        let y1 = ctx.price_to_y(self.price1);
        let y2 = ctx.price_to_y(self.price2);

        let min_x = x1.min(x2);
        let max_x = x1.max(x2);
        let min_y = y1.min(y2);
        let max_y = y1.max(y2);
        let w = max_x - min_x;
        let h = max_y - min_y;

        let (left, right) = horizontal_span(min_x, max_x, ctx.chart_width(), self.extend_left, self.extend_right);
        let fill_w = right - left;

        if fill_w.abs() > 0.5 && h.abs() > 0.5 {
            ctx.set_fill_color(&format!("{}1a", &self.data.color.stroke));
            ctx.fill_rect(crisp(left, dpr), crisp(min_y, dpr), fill_w, h);
        }

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_line_style(LineStyle::Solid);
        ctx.set_stroke_width(self.data.width);
        for edge in price_horizontals(left, right, min_y, max_y) {
            ctx.begin_path();
            ctx.move_to(crisp(edge.x1, dpr), crisp(edge.y1, dpr));
            ctx.line_to(crisp(edge.x2, dpr), crisp(edge.y2, dpr));
            ctx.stroke();
        }

        // Stat plate: price delta (show_pips) and/or percentage change
        // (show_percentage) — independent toggles, either can stand alone.
        let diff = price_diff(self.price1, self.price2);
        let pct = percentage(self.price1, self.price2);

        let mut parts = Vec::new();
        if self.show_pips {
            parts.push(super::super::fmt_price(diff));
        }
        if self.show_percentage {
            parts.push(format!("{:.2}%", pct));
        }
        let line = if parts.is_empty() {
            super::super::fmt_price(diff)
        } else {
            parts.join("  /  ")
        };

        draw_stat_plate(ctx, min_x + w / 2.0, min_y + h / 2.0, &[line], &self.label_style, &self.data.color.stroke);

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
    fn extend_mode_raw(&self) -> u8 {
        super::super::extend_mode_lr(self.extend_left, self.extend_right)
    }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        // `PropertyGroup::Values` (W2) — the approved measurement
        // archetype's own `ValuesGroupTitle` (the "show ___" checklist),
        // matching `PriceDateRange::style_properties()`'s own choice.
        let mut props = vec![
            ConfigProperty::extend_left(self.extend_left)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(8).in_group(PropertyGroup::Extend),
            ConfigProperty::extend_right(self.extend_right)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(9).in_group(PropertyGroup::Extend),
            ConfigProperty::show_price_diff(self.show_pips)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(10).in_group(PropertyGroup::Values),
            ConfigProperty::show_as_percent(self.show_percentage)
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
            // Read-only computed-stat block (W4) — ungrouped default run of
            // `AppearanceStats` (no design-backed `PropertyGroup` exists for
            // the measurement archetype's own Result section; renders under
            // the category's own default "Results"/"Результаты" header,
            // `ConfigKey::GroupResults`, same as it would for any other
            // primitive that declares nothing into this category's `group`).
            ConfigProperty::stat_block("price_range_stats", self.result_stat_rows()).with_order(20),
        ];
        props.extend(self.label_style.style_properties_grouped(&self.data.color.stroke, 30, PropertyGroup::TextGroup));
        props
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "price_range_stats" => {}
            "extend_left" => {
                if let Some(v) = value.as_bool() { self.extend_left = v; return true; }
            }
            "extend_right" => {
                if let Some(v) = value.as_bool() { self.extend_right = v; return true; }
            }
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
        if self.label_style.apply_property(id, value, &self.data.color.stroke) { return true; }
        false
    }
}

/// Horizontal span of the top/bottom edges. Segment mode stays inside the
/// date-span box (`min_x`..`max_x`); each checkbox grows that edge past
/// the box on its side, same as Price/Date Range growing into a slab.
fn horizontal_span(min_x: f64, max_x: f64, chart_width: f64, extend_left: bool, extend_right: bool) -> (f64, f64) {
    (
        if extend_left { 0.0 } else { min_x },
        if extend_right { chart_width } else { max_x },
    )
}

/// The two price-level horizontals — the only stroked edges. Left/right
/// verticals of the date span are never drawn; the fill's own edges are
/// the date bounds. Shared by `render` and `hit_test`.
fn price_horizontals(h_left: f64, h_right: f64, min_y: f64, max_y: f64) -> [Segment; 2] {
    [
        Segment::new(h_left, min_y, h_right, min_y),
        Segment::new(h_left, max_y, h_right, max_y),
    ]
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "price_range", display_name: "Price Range", kind: PrimitiveKind::Measurement,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Measure price difference", icon: "price_range", default_color: "#4CAF50",
        factory: |points, color| {
            let (ts1, p1) = points.first().copied().unwrap_or((0, 100.0));
            let (ts2, p2) = points.get(1).copied().unwrap_or((ts1, p1 + 10.0));
            let mut pr = PriceRange::new(ts1, p1, p2, color);
            pr.ts2 = ts2;
            Box::new(pr)
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod stat_block_tests {
    use super::*;

    fn sample() -> PriceRange {
        // price1 100, price2 130 -> diff 30, 30% move.
        PriceRange::new(0, 100.0, 130.0, "#4CAF50")
    }

    /// `result_stat_rows` reads through the SAME `price_diff`/`percentage`
    /// the on-chart stat plate uses, and is untoned (a ruler reports a
    /// distance, it does not judge direction).
    #[test]
    fn rows_match_the_on_chart_figures() {
        let pos = sample();
        let rows = pos.result_stat_rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].value, super::super::super::fmt_price(price_diff(pos.price1, pos.price2)));
        assert_eq!(rows[1].value, format!("{:.2}%", percentage(pos.price1, pos.price2)));
        assert_eq!(rows[1].value, "30.00%");
        assert!(rows.iter().all(|r| r.tone.is_none()));
    }

    /// Rows stay the same regardless of `show_pips`/`show_percentage` — the
    /// block is unconditional, matching Position's own Result block.
    #[test]
    fn rows_ignore_the_on_chart_display_toggles() {
        let mut pos = sample();
        pos.show_pips = false;
        pos.show_percentage = false;
        assert_eq!(pos.result_stat_rows(), sample().result_stat_rows());
    }

    /// Sending the stat block's own id through the exact same apply path a
    /// click would reach does nothing.
    #[test]
    fn apply_style_property_rejects_the_stat_block_id() {
        let before = sample();
        let mut pos = sample();
        let applied = pos.apply_style_property("price_range_stats", &PropertyValue::String("hacked".to_string()));
        assert!(!applied);
        assert_eq!(pos.price1, before.price1);
        assert_eq!(pos.price2, before.price2);
    }
}
