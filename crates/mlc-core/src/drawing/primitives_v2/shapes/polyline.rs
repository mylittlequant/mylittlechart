//! Polyline primitive

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    point_to_line_distance, HIT_TOLERANCE, CONTROL_POINT_HIT_RADIUS, CONTROL_POINT_RADIUS,
    RenderContext, crisp, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Polyline {
    pub data: PrimitiveData,
    pub points_data: Vec<(i64, f64)>,
    #[serde(default)] pub closed: bool,
    #[serde(default)] pub fill: bool,
    #[serde(default = "default_fill_opacity")] pub fill_opacity: f64,
}

fn default_fill_opacity() -> f64 { 0.2 }

impl Polyline {
    pub fn new(points: Vec<(i64, f64)>, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "polyline".to_string(),
                display_name: "Polyline".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            points_data: points,
            closed: false, fill: false, fill_opacity: 0.2,
        }
    }

    fn center_ts_price(&self) -> (i64, f64) {
        if self.points_data.is_empty() { return (0, 0.0); }
        let n = self.points_data.len() as f64;
        let sum_ts: i64 = self.points_data.iter().map(|(ts, _)| *ts).sum();
        let sum_price: f64 = self.points_data.iter().map(|(_, p)| *p).sum();
        ((sum_ts as f64 / n) as i64, sum_price / n)
    }
}

impl Primitive for Polyline {
    fn type_id(&self) -> &'static str { "polyline" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Shape }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::MultiPoint(2) }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> { self.points_data.clone() }
    fn set_points(&mut self, points: &[(i64, f64)]) { self.points_data = points.to_vec(); }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        for p in &mut self.points_data { p.0 += ts_delta_ms; p.1 += price_delta; }
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        if let ControlPointType::Index(i) = point_type {
            if let Some(p) = self.points_data.get_mut(i as usize) {
                p.0 = ts_ms; p.1 = price;
            }
        }
    }

    fn hit_test(&self, screen_x: f64, screen_y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> HitTestResult {
        if self.points_data.is_empty() { return HitTestResult::Miss; }

        let screen_pts: Vec<(f64, f64)> = self.points_data.iter()
            .map(|(ts, p)| {
                let bar = timestamp_ms_to_bar_f64(bars, *ts);
                (viewport.bar_to_x_f64(bar), viewport.price_to_y(*p, price_scale.price_min, price_scale.price_max))
            })
            .collect();

        for (i, (sx, sy)) in screen_pts.iter().enumerate() {
            if check_point_hit(screen_x, screen_y, *sx, *sy) {
                return HitTestResult::ControlPoint(ControlPointType::Index(i as u8));
            }
        }

        let (center_ts, center_price) = self.center_ts_price();
        let center_bar = timestamp_ms_to_bar_f64(bars, center_ts);
        let cx = viewport.bar_to_x_f64(center_bar);
        let cy = viewport.price_to_y(center_price, price_scale.price_min, price_scale.price_max);
        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }

        for i in 0..screen_pts.len() {
            let j = if i + 1 < screen_pts.len() {
                i + 1
            } else if self.closed {
                0
            } else {
                continue;
            };
            let (x1, y1) = screen_pts[i];
            let (x2, y2) = screen_pts[j];
            if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        let mut cps: Vec<ControlPoint> = self.points_data.iter()
            .enumerate()
            .map(|(i, (ts, p))| {
                let bar = timestamp_ms_to_bar_f64(bars, *ts);
                let x = viewport.bar_to_x_f64(bar);
                let y = viewport.price_to_y(*p, price_scale.price_min, price_scale.price_max);
                ControlPoint::with_type(ControlPointType::Index(i as u8), x, y)
            })
            .collect();

        if !self.points_data.is_empty() {
            let (center_ts, center_price) = self.center_ts_price();
            let center_bar = timestamp_ms_to_bar_f64(bars, center_ts);
            let cx = viewport.bar_to_x_f64(center_bar);
            let cy = viewport.price_to_y(center_price, price_scale.price_min, price_scale.price_max);
            cps.push(ControlPoint::move_point(cx, cy));
        }
        cps
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        if self.points_data.len() < 2 { return; }
        let dpr = ctx.dpr();
        let screen_pts: Vec<(f64, f64)> = self.points_data.iter()
            .map(|(ts, p)| (ctx.ts_to_x_ms(*ts), ctx.price_to_y(*p)))
            .collect();

        if self.closed && self.fill && screen_pts.len() >= 3 {
            let fill_color = format!("{}{:02x}", &self.data.color.stroke[..7], (self.fill_opacity * 255.0) as u8);
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.move_to(screen_pts[0].0, screen_pts[0].1);
            for (x, y) in screen_pts.iter().skip(1) { ctx.line_to(*x, *y); }
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
        ctx.move_to(crisp(screen_pts[0].0, dpr), crisp(screen_pts[0].1, dpr));
        for (x, y) in screen_pts.iter().skip(1) { ctx.line_to(crisp(*x, dpr), crisp(*y, dpr)); }
        if self.closed { ctx.close_path(); }
        ctx.stroke();
        ctx.set_line_dash(&[]);

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (x, y) in &screen_pts {
                ctx.begin_path();
                ctx.arc(*x, *y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill(); ctx.stroke();
            }
            let (center_ts, center_price) = self.center_ts_price();
            let cx = ctx.ts_to_x_ms(center_ts);
            let cy = ctx.price_to_y(center_price);
            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill(); ctx.stroke();
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let min_x = screen_pts.iter().map(|(x, _)| *x).fold(f64::INFINITY, f64::min);
                let max_x = screen_pts.iter().map(|(x, _)| *x).fold(f64::NEG_INFINITY, f64::max);
                let min_y = screen_pts.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
                let max_y = screen_pts.iter().map(|(_, y)| *y).fold(f64::NEG_INFINITY, f64::max);
                let n = screen_pts.len() as f64;
                let (sum_x, sum_y) = screen_pts.iter().fold((0.0, 0.0), |(sx, sy), (x, y)| (sx + x, sy + y));
                let (cen_x, cen_y) = (sum_x / n, sum_y / n);
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => cen_x,
                    super::super::TextAlign::End => max_x,
                };
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => min_y - text_offset,
                    super::super::TextAlign::Center => cen_y,
                    super::super::TextAlign::End => max_y + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2)
}

fn create_polyline(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    Box::new(Polyline::new(points.to_vec(), color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "polyline", display_name: "Polyline", kind: PrimitiveKind::Shape,
        click_behavior: ClickBehavior::MultiPoint(2),
        tooltip: "Draw connected line segments (double-click to finish)",
        icon: "polyline", default_color: "#795548",
        factory: create_polyline,
        supports_text: true, has_levels: false, has_points_config: false,
    }
}
