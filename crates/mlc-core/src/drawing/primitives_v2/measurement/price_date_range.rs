//! Price Date Range - combined price and time measurement

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
use super::{draw_stat_plate, price_diff, percentage, duration_label};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceDateRange {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64, pub price1: f64,
    #[serde(default)]
    pub ts2: i64, pub price2: f64,
    #[serde(default = "default_true")] pub show_percentage: bool,
    #[serde(default = "default_true")] pub show_bars: bool,
    #[serde(default = "default_true")] pub show_pips: bool,
    #[serde(default = "default_label_style")]
    pub label_style: LabelStyle,
}
fn default_true() -> bool { true }
fn default_label_style() -> LabelStyle {
    LabelStyle { font_size: 13.0, ..Default::default() }
}

impl PriceDateRange {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "price_date_range".to_string(), display_name: "Price/Date Range".to_string(), color: PrimitiveColor::new(color), width: 2.0, ..Default::default() },
            ts1, price1, ts2, price2, show_percentage: true, show_bars: true, show_pips: true,
            label_style: default_label_style(),
        }
    }

    /// The read-only computed-stat block's own rows — the pure decision
    /// `style_properties()` wraps into a declared property
    /// (`ConfigProperty::stat_block`). Reads through the SAME
    /// `super::price_diff`/`super::percentage`/`super::duration_label`
    /// this primitive's own `render` uses for the identical on-chart stat
    /// plate. No bar-count row — see `DateRange::result_stat_rows`'s own
    /// doc for why (`style_properties()` has no `bars: &[Bar]` to read).
    pub fn result_stat_rows(&self) -> Vec<StatRow> {
        vec![
            StatRow::price_change(&super::super::fmt_price(price_diff(self.price1, self.price2))),
            StatRow::percent_change(&format!("{:.2}%", percentage(self.price1, self.price2))),
            StatRow::duration(&duration_label(self.ts1, self.ts2)),
        ]
    }
}

impl Primitive for PriceDateRange {
    fn type_id(&self) -> &'static str { "price_date_range" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Measurement }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, self.price1), (self.ts2, self.price2)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(ts, p)) = pts.first() { self.ts1 = ts; self.price1 = p; }
        if let Some(&(ts, p)) = pts.get(1) { self.ts2 = ts; self.price2 = p; }
    }
    fn translate(&mut self, ts_delta_ms: i64, pd: f64) { self.ts1 += ts_delta_ms; self.ts2 += ts_delta_ms; self.price1 += pd; self.price2 += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            // Corner index convention matches `shapes::rectangle::Rectangle`
            // exactly: 0=(ts1,price1), 1=(ts2,price1), 2=(ts2,price2),
            // 3=(ts1,price2). Every corner writes BOTH axes — a box corner
            // that only moved one axis would silently do half of what it
            // looks like it does.
            ControlPointType::Corner(0) => { self.ts1 = ts_ms; self.price1 = price; }
            ControlPointType::Corner(1) => { self.ts2 = ts_ms; self.price1 = price; }
            ControlPointType::Corner(2) => { self.ts2 = ts_ms; self.price2 = price; }
            ControlPointType::Corner(3) => { self.ts1 = ts_ms; self.price2 = price; }
            // Edge midpoints: 0=top(price1), 1=right(ts2), 2=bottom(price2),
            // 3=left(ts1) — same convention as Rectangle.
            ControlPointType::Edge(0) => { self.price1 = price; }
            ControlPointType::Edge(1) => { self.ts2 = ts_ms; }
            ControlPointType::Edge(2) => { self.price2 = price; }
            ControlPointType::Edge(3) => { self.ts1 = ts_ms; }
            ControlPointType::Move => { let td = ts_ms - self.ts1; let pd = price - self.price1; self.translate(td, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let (x1, y1) = (vp.bar_to_x_f64(b1), vp.price_to_y(self.price1, ps.price_min, ps.price_max));
        let (x2, y2) = (vp.bar_to_x_f64(b2), vp.price_to_y(self.price2, ps.price_min, ps.price_max));

        // Corners tested before edges before the body stroke, so a click at
        // a corner (where an edge midpoint could never land — edges sit at
        // the midpoint of a side, not at a shared endpoint) always resolves
        // to the corner.
        let corners = [
            (x1, y1, ControlPointType::Corner(0)),
            (x2, y1, ControlPointType::Corner(1)),
            (x2, y2, ControlPointType::Corner(2)),
            (x1, y2, ControlPointType::Corner(3)),
        ];
        for (cx, cy, cp_type) in corners {
            if (sx - cx).powi(2) + (sy - cy).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2) {
                return HitTestResult::ControlPoint(cp_type);
            }
        }

        let edges = [
            ((x1 + x2) / 2.0, y1, ControlPointType::Edge(0)),
            (x2, (y1 + y2) / 2.0, ControlPointType::Edge(1)),
            ((x1 + x2) / 2.0, y2, ControlPointType::Edge(2)),
            (x1, (y1 + y2) / 2.0, ControlPointType::Edge(3)),
        ];
        for (ex, ey, cp_type) in edges {
            if (sx - ex).powi(2) + (sy - ey).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2) {
                return HitTestResult::ControlPoint(cp_type);
            }
        }

        // Four real rectangle edges — the same geometry `render` strokes via
        // `rect_edges` below. There is no rendered diagonal to test.
        if hit_test_strokes(sx, sy, &rect_edges(x1, y1, x2, y2), HIT_TOLERANCE) { return HitTestResult::Body; }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = vp.bar_to_x_f64(b1);
        let y1 = vp.price_to_y(self.price1, ps.price_min, ps.price_max);
        let x2 = vp.bar_to_x_f64(b2);
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
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y2 = ctx.price_to_y(self.price2);

        let min_x = x1.min(x2);
        let min_y = y1.min(y2);
        let w = (x2 - x1).abs();
        let h = (y2 - y1).abs();

        // Light translucent rectangle — a ruler measures the range, it
        // should not paint over it.
        ctx.set_fill_color(&format!("{}1a", &self.data.color.stroke));
        ctx.fill_rect(crisp(min_x, dpr), crisp(min_y, dpr), w, h);

        // Draw rectangle border — same 4 edges `hit_test` tests via
        // `rect_edges`, so the clickable outline can never drift from what's
        // actually drawn.
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_line_style(LineStyle::Solid);
        ctx.set_stroke_width(self.data.width);
        for edge in rect_edges(x1, y1, x2, y2) {
            ctx.begin_path();
            ctx.move_to(crisp(edge.x1, dpr), crisp(edge.y1, dpr));
            ctx.line_to(crisp(edge.x2, dpr), crisp(edge.y2, dpr));
            ctx.stroke();
        }

        // Metrics: price delta (+ %), elapsed time (always shown — this tool
        // combines both dimensions), bar count (real count, not a
        // relabelled duration string).
        let diff = price_diff(self.price1, self.price2);
        let pct = percentage(self.price1, self.price2);
        let duration = duration_label(self.ts1, self.ts2);
        let bar1 = timestamp_ms_to_bar_f64(ctx.bars(), self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(ctx.bars(), self.ts2);
        let bar_count = (bar2 - bar1).abs().round() as i64;

        let mut lines = Vec::new();
        let mut price_parts = Vec::new();
        if self.show_pips {
            price_parts.push(super::super::fmt_price(diff));
        }
        if self.show_percentage {
            price_parts.push(format!("{:.2}%", pct));
        }
        if !price_parts.is_empty() {
            lines.push(price_parts.join("  /  "));
        }
        lines.push(duration);
        if self.show_bars {
            lines.push(format!("{} bars", bar_count));
        }

        draw_stat_plate(ctx, crisp(min_x + w / 2.0, dpr), crisp(min_y + h / 2.0, dpr), &lines, &self.label_style, &self.data.color.stroke);

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let max_x = x1.max(x2);
                let max_y = y1.max(y2);
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
        let mut props = vec![
            // `PropertyGroup::Values` (W2) — the approved measurement
            // archetype's own `ValuesGroupTitle` group (the "show ___"
            // checklist).
            ConfigProperty::show_price_diff(self.show_pips)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(10).in_group(PropertyGroup::Values),
            ConfigProperty::show_as_percent(self.show_percentage)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(11).in_group(PropertyGroup::Values),
            ConfigProperty::show_bar_count(self.show_bars)
                .with_category(PropertyCategory::AppearanceLine)
                .with_order(12).in_group(PropertyGroup::Values),
            // `PropertyGroup::TextGroup` (W2) — the approved design's own
            // measurement "labels" tab text group (`SettingsKey::Text`).
            ConfigProperty::label_font_size(self.label_style.font_size)
                .with_category(PropertyCategory::LabelsText)
                .with_order(13).in_group(PropertyGroup::TextGroup),
            ConfigProperty::label_color(self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke))
                .with_category(PropertyCategory::LabelsText)
                .with_order(14).in_group(PropertyGroup::TextGroup),
            // Read-only computed-stat block (W4) — see
            // `PriceRange::style_properties()`'s own comment for why this
            // is an ungrouped default `AppearanceStats` run.
            ConfigProperty::stat_block("price_date_range_stats", self.result_stat_rows()).with_order(20),
        ];
        props.extend(self.label_style.style_properties_grouped(&self.data.color.stroke, 30, PropertyGroup::TextGroup));
        props
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "price_date_range_stats" => {}
            "show_pips" => {
                if let Some(v) = value.as_bool() { self.show_pips = v; return true; }
            }
            "show_as_percent" => {
                if let Some(v) = value.as_bool() { self.show_percentage = v; return true; }
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

/// The 4 screen-space edges of the rectangle border — shared by `render`
/// (draws each edge) and `hit_test` (tests each edge) so the drawn outline
/// and the clickable outline can never disagree.
fn rect_edges(x1: f64, y1: f64, x2: f64, y2: f64) -> [Segment; 4] {
    let (min_x, max_x) = (x1.min(x2), x1.max(x2));
    let (min_y, max_y) = (y1.min(y2), y1.max(y2));
    [
        Segment::new(min_x, min_y, max_x, min_y), // top
        Segment::new(max_x, min_y, max_x, max_y), // right
        Segment::new(max_x, max_y, min_x, max_y), // bottom
        Segment::new(min_x, max_y, min_x, min_y), // left
    ]
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "price_date_range", display_name: "Price/Date Range", kind: PrimitiveKind::Measurement,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Measure price and time", icon: "price_date_range", default_color: "#FF9800",
        factory: |points, color| {
            let (ts1, p1) = points.first().copied().unwrap_or((0, 100.0));
            let (ts2, p2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, p1 + 10.0));
            Box::new(PriceDateRange::new(ts1, p1, ts2, p2, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod stat_block_tests {
    use super::*;

    fn sample() -> PriceDateRange {
        // price 100 -> 130 (30% move), 45 minutes apart.
        PriceDateRange::new(0, 100.0, 45 * 60 * 1000, 130.0, "#FF9800")
    }

    /// `result_stat_rows` reads through the SAME `price_diff`/`percentage`/
    /// `duration_label` the on-chart stat plate uses, and is untoned.
    #[test]
    fn rows_match_the_on_chart_figures() {
        let pos = sample();
        let rows = pos.result_stat_rows();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].value, super::super::super::fmt_price(price_diff(pos.price1, pos.price2)));
        assert_eq!(rows[1].value, format!("{:.2}%", percentage(pos.price1, pos.price2)));
        assert_eq!(rows[1].value, "30.00%");
        assert_eq!(rows[2].value, duration_label(pos.ts1, pos.ts2));
        assert_eq!(rows[2].value, "45m");
        assert!(rows.iter().all(|r| r.tone.is_none()));
    }

    /// Rows stay the same regardless of `show_pips`/`show_as_percent`/
    /// `show_bars`.
    #[test]
    fn rows_ignore_the_on_chart_display_toggles() {
        let mut pos = sample();
        pos.show_pips = false;
        pos.show_percentage = false;
        pos.show_bars = false;
        assert_eq!(pos.result_stat_rows(), sample().result_stat_rows());
    }

    /// Sending the stat block's own id through the exact same apply path a
    /// click would reach does nothing.
    #[test]
    fn apply_style_property_rejects_the_stat_block_id() {
        let before = sample();
        let mut pos = sample();
        let applied = pos.apply_style_property("price_date_range_stats", &PropertyValue::String("hacked".to_string()));
        assert!(!applied);
        assert_eq!(pos.price1, before.price1);
        assert_eq!(pos.price2, before.price2);
        assert_eq!(pos.ts1, before.ts1);
        assert_eq!(pos.ts2, before.ts2);
    }
}
