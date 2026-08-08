//! Rectangle primitive

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType, ControlPointCursor,
    PrimitiveColor, LineStyle,
    point_to_line_distance, HIT_TOLERANCE, CONTROL_POINT_HIT_RADIUS,
    RenderContext, crisp,
    render_primitive_text,
    config::{ConfigProperty, PropertyValue, PropertyCategory},
    AREA_FILL_OPACITY, default_area_fill_opacity,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rectangle {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64,
    pub price1: f64,
    #[serde(default)]
    pub ts2: i64,
    pub price2: f64,
    #[serde(default = "default_true")] pub fill: bool,
    #[serde(default = "default_area_fill_opacity")] pub fill_opacity: f64,
    #[serde(default)] pub border_radius: f64,
}

fn default_true() -> bool { true }

impl Rectangle {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "rectangle".to_string(),
                display_name: "Rectangle".to_string(),
                color: PrimitiveColor::with_fill(color, color),
                width: 2.0,
                ..Default::default()
            },
            ts1, price1, ts2, price2,
            fill: true, fill_opacity: AREA_FILL_OPACITY, border_radius: 0.0,
        }
    }

    pub fn center_price(&self) -> f64 { (self.price1 + self.price2) / 2.0 }
    pub fn height_price(&self) -> f64 { (self.price2 - self.price1).abs() }
}

impl Primitive for Rectangle {
    fn type_id(&self) -> &'static str { "rectangle" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Shape }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::ClickDrag }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> {
        vec![(self.ts1, self.price1), (self.ts2, self.price2)]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if points.len() >= 2 {
            self.ts1 = points[0].0; self.price1 = points[0].1;
            self.ts2 = points[1].0; self.price2 = points[1].1;
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        self.ts1 += ts_delta_ms; self.ts2 += ts_delta_ms;
        self.price1 += price_delta; self.price2 += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        match point_type {
            ControlPointType::Corner(0) => { self.ts1 = ts_ms; self.price1 = price; }
            ControlPointType::Corner(1) => { self.ts2 = ts_ms; self.price1 = price; }
            ControlPointType::Corner(2) => { self.ts2 = ts_ms; self.price2 = price; }
            ControlPointType::Corner(3) => { self.ts1 = ts_ms; self.price2 = price; }
            ControlPointType::Edge(0) => { self.price1 = price; }
            ControlPointType::Edge(1) => { self.ts2 = ts_ms; }
            ControlPointType::Edge(2) => { self.price2 = price; }
            ControlPointType::Edge(3) => { self.ts1 = ts_ms; }
            ControlPointType::Move => {
                let td = ts_ms - self.ts1; let pd = price - self.price1;
                self.translate(td, pd);
            }
            _ => {}
        }
    }

    fn hit_test(&self, screen_x: f64, screen_y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> HitTestResult {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        let corners = [
            (x1, y1, ControlPointType::Corner(0)),
            (x2, y1, ControlPointType::Corner(1)),
            (x2, y2, ControlPointType::Corner(2)),
            (x1, y2, ControlPointType::Corner(3)),
        ];
        for (cx, cy, cp_type) in corners {
            if check_point_hit(screen_x, screen_y, cx, cy) {
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
            if check_point_hit(screen_x, screen_y, ex, ey) {
                return HitTestResult::ControlPoint(cp_type);
            }
        }

        if check_point_hit(screen_x, screen_y, (x1 + x2) / 2.0, (y1 + y2) / 2.0) {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }

        let edges_lines = [(x1, y1, x2, y1), (x2, y1, x2, y2), (x2, y2, x1, y2), (x1, y2, x1, y1)];
        for (lx1, ly1, lx2, ly2) in edges_lines {
            if point_to_line_distance(screen_x, screen_y, lx1, ly1, lx2, ly2) < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }

        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        vec![
            ControlPoint::new(ControlPointType::Corner(0), x1, y1, ControlPointCursor::ResizeNWSE),
            ControlPoint::new(ControlPointType::Corner(1), x2, y1, ControlPointCursor::ResizeNESW),
            ControlPoint::new(ControlPointType::Corner(2), x2, y2, ControlPointCursor::ResizeNWSE),
            ControlPoint::new(ControlPointType::Corner(3), x1, y2, ControlPointCursor::ResizeNESW),
            ControlPoint::new(ControlPointType::Edge(0), (x1 + x2) / 2.0, y1, ControlPointCursor::ResizeNS),
            ControlPoint::new(ControlPointType::Edge(1), x2, (y1 + y2) / 2.0, ControlPointCursor::ResizeEW),
            ControlPoint::new(ControlPointType::Edge(2), (x1 + x2) / 2.0, y2, ControlPointCursor::ResizeNS),
            ControlPoint::new(ControlPointType::Edge(3), x1, (y1 + y2) / 2.0, ControlPointCursor::ResizeEW),
            ControlPoint::move_point((x1 + x2) / 2.0, (y1 + y2) / 2.0),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y2 = ctx.price_to_y(self.price2);

        let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
        let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };
        let width = max_x - min_x;
        let height = max_y - min_y;

        if self.fill {
            let base = self.data.color.fill.as_deref().unwrap_or(&self.data.color.stroke);
            let fill_color = format!("{}{:02x}", &base[..7], (self.fill_opacity * 255.0) as u8);
            ctx.set_fill_color(&fill_color);
            ctx.fill_rect(min_x, min_y, width, height);
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

        ctx.stroke_rect(crisp(min_x, dpr), crisp(min_y, dpr), width, height);
        ctx.set_line_dash(&[]);

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => (x1 + x2) / 2.0,
                    super::super::TextAlign::End => max_x,
                };
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => min_y - text_offset,
                    super::super::TextAlign::Center => (y1 + y2) / 2.0,
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
            ConfigProperty::fill(self.fill).with_category(PropertyCategory::AppearanceFill).with_order(20),
            ConfigProperty::fill_opacity(self.fill_opacity * 100.0).with_category(PropertyCategory::AppearanceFill).with_order(21),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "fill" => { if let Some(v) = value.as_bool() { self.fill = v; return true; } }
            "fill_opacity" => { if let Some(v) = value.as_number() { self.fill_opacity = (v / 100.0).clamp(0.0, 1.0); return true; } }
            _ => {}
        }
        false
    }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2)
}

fn create_rectangle(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
    let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 600_000, price1 * 1.05));
    Box::new(Rectangle::new(ts1, price1, ts2, price2, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "rectangle", display_name: "Rectangle", kind: PrimitiveKind::Shape,
        click_behavior: ClickBehavior::ClickDrag,
        tooltip: "Draw a rectangle by dragging",
        icon: "rectangle", default_color: "#2196F3",
        factory: create_rectangle,
        supports_text: true, has_levels: false,
    }
}
