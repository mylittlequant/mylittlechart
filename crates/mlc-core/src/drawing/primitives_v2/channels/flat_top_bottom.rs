//! Flat Top/Bottom primitive
//!
//! A channel with one sloped line and one horizontal line.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    point_to_line_distance, HIT_TOLERANCE, CONTROL_POINT_RADIUS, CONTROL_POINT_HIT_RADIUS,
    extend_segment_to_edges,
    RenderContext, crisp,
    render_primitive_text_rotated, calculate_line_text_params,
    config::{ConfigProperty, PropertyValue, PropertyCategory, PropertyGroup},
    AREA_FILL_OPACITY, default_area_fill_opacity,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FlatType {
    #[default] Top,
    Bottom,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FlatTopBottom {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64,
    pub price1: f64,
    #[serde(default)]
    pub ts2: i64,
    pub price2: f64,
    pub flat_price: f64,
    #[serde(default)]
    pub flat_type: FlatType,
    #[serde(default = "default_true")]
    pub fill: bool,
    #[serde(default = "default_area_fill_opacity")]
    pub fill_opacity: f64,
    #[serde(default)]
    pub extend_left: bool,
    #[serde(default)]
    pub extend_right: bool,
}

fn default_true() -> bool { true }

impl FlatTopBottom {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, flat_price: f64, color: &str) -> Self {
        let flat_type = if flat_price > (price1 + price2) / 2.0 { FlatType::Top } else { FlatType::Bottom };
        Self {
            data: PrimitiveData {
                type_id: "flat_top_bottom".to_string(),
                display_name: if flat_type == FlatType::Top { "Flat Top" } else { "Flat Bottom" }.to_string(),
                color: PrimitiveColor::with_fill(color, color),
                width: 2.0,
                ..Default::default()
            },
            ts1, price1, ts2, price2,
            flat_price, flat_type,
            fill: true, fill_opacity: AREA_FILL_OPACITY,
            extend_left: false, extend_right: false,
        }
    }
}

impl Primitive for FlatTopBottom {
    fn type_id(&self) -> &'static str { "flat_top_bottom" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Channel }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::ThreePoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> {
        vec![(self.ts1, self.price1), (self.ts2, self.price2), (self.ts1, self.flat_price)]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if points.len() >= 2 {
            self.ts1 = points[0].0; self.price1 = points[0].1;
            self.ts2 = points[1].0; self.price2 = points[1].1;
        }
        if points.len() >= 3 {
            self.flat_price = points[2].1;
            self.flat_type = if self.flat_price > (self.price1 + self.price2) / 2.0 { FlatType::Top } else { FlatType::Bottom };
            self.data.display_name = if self.flat_type == FlatType::Top { "Flat Top".to_string() } else { "Flat Bottom".to_string() };
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        self.ts1 += ts_delta_ms; self.ts2 += ts_delta_ms;
        self.price1 += price_delta; self.price2 += price_delta;
        self.flat_price += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        match point_type {
            ControlPointType::Point1 => { self.ts1 = ts_ms; self.price1 = price; }
            ControlPointType::Point2 => { self.ts2 = ts_ms; self.price2 = price; }
            ControlPointType::Point3 | ControlPointType::Point4 => { self.flat_price = price; }
            ControlPointType::Move => {
                let mid_ts = (self.ts1 + self.ts2) / 2;
                let mid_price = (self.price1 + self.price2 + 2.0 * self.flat_price) / 4.0;
                let td = ts_ms - mid_ts;
                let pd = price - mid_price;
                self.translate(td, pd);
            }
            _ => {}
        }
    }

    fn hit_test(&self, screen_x: f64, screen_y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> HitTestResult {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let x2 = viewport.bar_to_x_f64(bar2);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);
        let flat_y = viewport.price_to_y(self.flat_price, price_scale.price_min, price_scale.price_max);

        if check_point_hit(screen_x, screen_y, x1, y1) { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if check_point_hit(screen_x, screen_y, x2, y2) { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        if check_point_hit(screen_x, screen_y, x1, flat_y) { return HitTestResult::ControlPoint(ControlPointType::Point3); }
        if check_point_hit(screen_x, screen_y, x2, flat_y) { return HitTestResult::ControlPoint(ControlPointType::Point4); }

        let cx = (x1 + x2) / 2.0;
        let cy = (y1 + y2 + flat_y + flat_y) / 4.0;
        if check_point_hit(screen_x, screen_y, cx, cy) { return HitTestResult::ControlPoint(ControlPointType::Move); }

        // Reach exactly as far as `render` draws — the flat line has zero
        // slope, so extending it just moves its x-endpoints; the sloped
        // line extends along its own slope. Independent per line, same as
        // the other channel types.
        let (sx1, sy1, sx2, sy2) = extend_segment_to_edges(x1, y1, x2, y2, self.extend_left, self.extend_right, viewport.chart_width);
        let (fx1, fy1, fx2, fy2) = extend_segment_to_edges(x1, flat_y, x2, flat_y, self.extend_left, self.extend_right, viewport.chart_width);
        if point_to_line_distance(screen_x, screen_y, sx1, sy1, sx2, sy2) < HIT_TOLERANCE { return HitTestResult::Body; }
        if point_to_line_distance(screen_x, screen_y, fx1, fy1, fx2, fy2) < HIT_TOLERANCE { return HitTestResult::Body; }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let x2 = viewport.bar_to_x_f64(bar2);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);
        let flat_y = viewport.price_to_y(self.flat_price, price_scale.price_min, price_scale.price_max);

        let mut points = vec![
            ControlPoint::point1(x1, y1),
            ControlPoint::point2(x2, y2),
            ControlPoint::point3(x1, flat_y),
            ControlPoint::point4(x2, flat_y),
        ];
        let cx = (x1 + x2) / 2.0;
        let cy = (y1 + y2 + flat_y + flat_y) / 4.0;
        let dist = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
        if dist > CONTROL_POINT_RADIUS * 4.0 { points.push(ControlPoint::move_point(cx, cy)); }
        points
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y2 = ctx.price_to_y(self.price2);
        let flat_y = ctx.price_to_y(self.flat_price);

        // An extended side draws to the panel edge in that direction — off
        // by default, so an unextended primitive draws the exact same two
        // segments as before this pass.
        let chart_width = ctx.chart_width();
        let (sx1, sy1, sx2, sy2) = extend_segment_to_edges(x1, y1, x2, y2, self.extend_left, self.extend_right, chart_width);
        let (fx1, fy1, fx2, fy2) = extend_segment_to_edges(x1, flat_y, x2, flat_y, self.extend_left, self.extend_right, chart_width);

        if self.fill {
            let base = self.data.color.fill.as_deref().unwrap_or(&self.data.color.stroke);
            let fill_color = format!("{}{:02x}", &base[..7], (self.fill_opacity * 255.0) as u8);
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.move_to(sx1, sy1); ctx.line_to(sx2, sy2);
            ctx.line_to(fx2, fy2); ctx.line_to(fx1, fy1);
            ctx.close_path(); ctx.fill();
        }

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
        ctx.move_to(crisp(sx1, dpr), crisp(sy1, dpr));
        ctx.line_to(crisp(sx2, dpr), crisp(sy2, dpr));
        ctx.stroke();

        ctx.begin_path();
        ctx.move_to(crisp(fx1, dpr), crisp(fy1, dpr));
        ctx.line_to(crisp(fx2, dpr), crisp(fy2, dpr));
        ctx.stroke();
        ctx.set_line_dash(&[]);

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let mid_y1 = (y1 + flat_y) / 2.0;
                let mid_y2 = (y2 + flat_y) / 2.0;
                let (text_y1, text_y2) = match text.v_align {
                    super::super::TextAlign::Start => (y1.min(flat_y), y2.min(flat_y)),
                    super::super::TextAlign::End => (y1.max(flat_y), y2.max(flat_y)),
                    super::super::TextAlign::Center => (mid_y1, mid_y2),
                };
                let params = calculate_line_text_params(x1, text_y1, x2, text_y2, text);
                render_primitive_text_rotated(ctx, text, params.x, params.y, &self.data.color.stroke, params.rotation);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
    fn extend_mode_raw(&self) -> u8 {
        super::super::extend_mode_lr(self.extend_left, self.extend_right)
    }
    fn unbounded_in_price(&self) -> bool { self.extend_left || self.extend_right }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        // Channel archetype (W3) — same skeleton `parallel_channel.rs`'s own
        // `style_properties` doc explains in full: Outline needs no
        // declaration (universal bespoke base stroke) and Fill's default
        // `AppearanceFill` header already matches the lab's title.
        //
        // `PropertyGroup::MiddleLine` is the lab's own Parallel-Channel-only
        // group (see that variant's own doc) — a flat top/bottom has one
        // sloped line and one horizontal line, not a pair a middle would
        // average between, so there is no home for it here either.
        //
        // `PropertyGroup::Extend` — `extend_left`/`extend_right` are new
        // fields on this struct (this type never had them before this
        // pass, unlike its two siblings): the sloped line extends along its
        // own slope, the flat line's own zero slope just moves its
        // x-endpoints (`render`/`hit_test` above, via
        // `extend_segment_to_edges`).
        let default_text = super::super::PrimitiveText::default();
        vec![
            ConfigProperty::fill(self.fill).with_category(PropertyCategory::AppearanceFill).with_order(20),
            ConfigProperty::fill_opacity(self.fill_opacity * 100.0).with_category(PropertyCategory::AppearanceFill).with_order(21),
            ConfigProperty::extend_left(self.extend_left).with_category(PropertyCategory::AppearanceLine).with_order(22).in_group(PropertyGroup::Extend),
            ConfigProperty::extend_right(self.extend_right).with_category(PropertyCategory::AppearanceLine).with_order(23).in_group(PropertyGroup::Extend),
            // `PropertyGroup::TextOnShape` — font size + horizontal
            // position write into `data.text`, already read by `render`
            // above (`render_primitive_text_rotated`) to place the caption.
            ConfigProperty::label_font_size(self.data.text.as_ref().map(|t| t.font_size).unwrap_or(default_text.font_size))
                .with_category(PropertyCategory::AppearanceLine).with_order(24).in_group(PropertyGroup::TextOnShape),
            ConfigProperty::h_align(self.data.text.as_ref().map(|t| t.h_align.as_str()).unwrap_or(default_text.h_align.as_str()))
                .with_category(PropertyCategory::AppearanceLine).with_order(25).in_group(PropertyGroup::TextOnShape),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "fill" => { if let Some(v) = value.as_bool() { self.fill = v; return true; } }
            "fill_opacity" => { if let Some(v) = value.as_number() { self.fill_opacity = (v / 100.0).clamp(0.0, 1.0); return true; } }
            "extend_left" => { if let Some(v) = value.as_bool() { self.extend_left = v; return true; } }
            "extend_right" => { if let Some(v) = value.as_bool() { self.extend_right = v; return true; } }
            "label_font_size" => {
                if let Some(v) = value.as_number() {
                    self.data.text.get_or_insert_with(super::super::PrimitiveText::default).font_size = v.clamp(8.0, 32.0);
                    return true;
                }
            }
            "text_h_align" => {
                if let PropertyValue::String(v) = value {
                    self.data.text.get_or_insert_with(super::super::PrimitiveText::default).h_align = super::super::TextAlign::from_str(v);
                    return true;
                }
            }
            _ => {}
        }
        false
    }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2)
}

fn create_flat_top_bottom(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts1, price1) = points.first().copied().unwrap_or((0, 100.0));
    let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, price1 * 1.05));
    let flat_price = if points.len() >= 3 { points[2].1 } else { price1 * 1.1 };
    Box::new(FlatTopBottom::new(ts1, price1, ts2, price2, flat_price, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "flat_top_bottom",
        display_name: "Flat Top/Bottom",
        kind: PrimitiveKind::Channel,
        click_behavior: ClickBehavior::ThreePoint,
        tooltip: "Draw a channel with one flat (horizontal) line",
        icon: "flat_top_bottom",
        default_color: "#FF9800",
        factory: create_flat_top_bottom,
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod extend_tests {
    use super::*;
    use crate::{Bar, PriceScale, Viewport};

    fn fixture() -> (Vec<Bar>, Viewport, PriceScale) {
        let bars: Vec<Bar> = (0..6).map(|i| Bar::new(i, 100.0, 102.0, 98.0, 100.0)).collect();
        let viewport = Viewport { chart_width: 1200.0, chart_height: 600.0, bar_count: bars.len(), ..Default::default() };
        let price_scale = PriceScale { price_min: 50.0, price_max: 150.0, ..Default::default() };
        (bars, viewport, price_scale)
    }

    /// The flat (horizontal) line has zero slope, so extending it just
    /// moves its x-endpoints at the same screen y — a probe at the panel's
    /// own right edge, level with the flat line, misses an unextended
    /// primitive and resolves to `Body` once `extend_right` is on.
    #[test]
    fn extend_right_reaches_the_panel_edge_on_the_flat_line() {
        let (bars, viewport, price_scale) = fixture();
        // ts values land exactly on bar indices 0 and 5 of `fixture()`'s
        // 6-bar slice, so both anchors sit well inside `chart_width` at the
        // fixture's default 8px bar spacing — the probe below is only
        // reachable via extension, not because the raw segment already
        // overshoots the panel on its own.
        let channel = FlatTopBottom::new(0, 100.0, 5_000, 105.0, 120.0, "#FF9800");

        let flat_y = viewport.price_to_y(channel.flat_price, price_scale.price_min, price_scale.price_max);
        let probe_x = viewport.chart_width;
        let probe_y = flat_y;

        assert!(
            matches!(channel.hit_test(probe_x, probe_y, &bars, &viewport, &price_scale), HitTestResult::Miss),
            "unextended channel misses a point on the panel's right edge",
        );

        let mut extended = channel;
        extended.extend_right = true;
        assert!(
            matches!(extended.hit_test(probe_x, probe_y, &bars, &viewport, &price_scale), HitTestResult::Body),
            "extend_right reaches the same point once the flat line is drawn that far",
        );
    }
}
