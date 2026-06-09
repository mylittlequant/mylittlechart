//! Curve primitive (Bezier)

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    HIT_TOLERANCE, CONTROL_POINT_HIT_RADIUS, CONTROL_POINT_RADIUS,
    RenderContext, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Curve {
    pub data: PrimitiveData,
    pub start_ts: i64,
    pub start_price: f64,
    pub control_ts: i64,
    pub control_price: f64,
    pub end_ts: i64,
    pub end_price: f64,
}

impl Curve {
    pub fn new(start_ts: i64, start_price: f64, control_ts: i64, control_price: f64, end_ts: i64, end_price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "curve".to_string(),
                display_name: "Curve".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            start_ts, start_price, control_ts, control_price, end_ts, end_price,
        }
    }

    /// Evaluate the quadratic Bezier curve at t in [0,1], returns (ts_ms, price)
    fn evaluate(&self, t: f64) -> (i64, f64) {
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let t2 = t * t;
        let ts = (mt2 * self.start_ts as f64 + 2.0 * mt * t * self.control_ts as f64 + t2 * self.end_ts as f64) as i64;
        let price = mt2 * self.start_price + 2.0 * mt * t * self.control_price + t2 * self.end_price;
        (ts, price)
    }
}

impl Primitive for Curve {
    fn type_id(&self) -> &'static str { "curve" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Shape }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::ThreePoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> {
        vec![
            (self.start_ts, self.start_price),
            (self.control_ts, self.control_price),
            (self.end_ts, self.end_price),
        ]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if !points.is_empty() {
            self.start_ts = points[0].0;
            self.start_price = points[0].1;
        }
        if points.len() >= 2 {
            self.end_ts = points[1].0;
            self.end_price = points[1].1;
            self.control_ts = (self.start_ts + self.end_ts) / 2;
            self.control_price = (self.start_price + self.end_price) / 2.0
                + (self.start_price - self.end_price).abs() * 0.3;
        }
        if points.len() >= 3 {
            self.control_ts = points[2].0;
            self.control_price = points[2].1;
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        self.start_ts += ts_delta_ms; self.start_price += price_delta;
        self.control_ts += ts_delta_ms; self.control_price += price_delta;
        self.end_ts += ts_delta_ms; self.end_price += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        match point_type {
            ControlPointType::Point1 => { self.start_ts = ts_ms; self.start_price = price; }
            ControlPointType::Point2 => { self.end_ts = ts_ms; self.end_price = price; }
            ControlPointType::Point3 => { self.control_ts = ts_ms; self.control_price = price; }
            _ => {}
        }
    }

    fn hit_test(&self, screen_x: f64, screen_y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> HitTestResult {
        let to_screen = |ts: i64, price: f64| -> (f64, f64) {
            let bar = timestamp_ms_to_bar_f64(bars, ts);
            (viewport.bar_to_x_f64(bar), viewport.price_to_y(price, price_scale.price_min, price_scale.price_max))
        };

        let (sx1, sy1) = to_screen(self.start_ts, self.start_price);
        let (scx, scy) = to_screen(self.control_ts, self.control_price);
        let (sx2, sy2) = to_screen(self.end_ts, self.end_price);

        if check_point_hit(screen_x, screen_y, sx1, sy1) { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if check_point_hit(screen_x, screen_y, sx2, sy2) { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        if check_point_hit(screen_x, screen_y, scx, scy) { return HitTestResult::ControlPoint(ControlPointType::Point3); }

        for i in 0..20 {
            let (ts1, p1) = self.evaluate(i as f64 / 20.0);
            let (ts2, p2) = self.evaluate((i + 1) as f64 / 20.0);
            let (x1, y1) = to_screen(ts1, p1);
            let (x2, y2) = to_screen(ts2, p2);
            if point_to_segment_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        let to_screen = |ts: i64, price: f64| -> (f64, f64) {
            let bar = timestamp_ms_to_bar_f64(bars, ts);
            (viewport.bar_to_x_f64(bar), viewport.price_to_y(price, price_scale.price_min, price_scale.price_max))
        };
        let (sx1, sy1) = to_screen(self.start_ts, self.start_price);
        let (scx, scy) = to_screen(self.control_ts, self.control_price);
        let (sx2, sy2) = to_screen(self.end_ts, self.end_price);

        vec![
            ControlPoint::point1(sx1, sy1),
            ControlPoint::point2(sx2, sy2),
            ControlPoint::point3(scx, scy),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let sx1 = ctx.ts_to_x_ms(self.start_ts);
        let sy1 = ctx.price_to_y(self.start_price);
        let scx = ctx.ts_to_x_ms(self.control_ts);
        let scy = ctx.price_to_y(self.control_price);
        let sx2 = ctx.ts_to_x_ms(self.end_ts);
        let sy2 = ctx.price_to_y(self.end_price);

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
        ctx.move_to(sx1, sy1);
        ctx.quadratic_curve_to(scx, scy, sx2, sy2);
        ctx.stroke();
        ctx.set_line_dash(&[]);

        if is_selected {
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.set_stroke_color("#888888");
            ctx.begin_path();
            ctx.move_to(sx1, sy1); ctx.line_to(scx, scy); ctx.line_to(sx2, sy2);
            ctx.stroke();
            ctx.set_line_dash(&[]);

            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (x, y) in [(sx1, sy1), (sx2, sy2), (scx, scy)] {
                ctx.begin_path();
                ctx.arc(x, y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill(); ctx.stroke();
            }
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let min_x = sx1.min(scx).min(sx2);
                let max_x = sx1.max(scx).max(sx2);
                let min_y = sy1.min(scy).min(sy2);
                let max_y = sy1.max(scy).max(sy2);
                let (mid_ts, mid_price) = self.evaluate(0.5);
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => ctx.ts_to_x_ms(mid_ts),
                    super::super::TextAlign::End => max_x,
                };
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => min_y - text_offset,
                    super::super::TextAlign::Center => ctx.price_to_y(mid_price),
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

fn point_to_segment_distance(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1; let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 0.0001 { return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt(); }
    let t = (((px - x1) * dx + (py - y1) * dy) / len_sq).clamp(0.0, 1.0);
    ((px - (x1 + t * dx)).powi(2) + (py - (y1 + t * dy)).powi(2)).sqrt()
}

fn create_curve(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (start_ts, start_price) = points.first().copied().unwrap_or((0, 100.0));
    let (end_ts, end_price) = points.get(1).copied().unwrap_or((start_ts + 1_200_000, start_price));
    let (control_ts, control_price) = points.get(2).copied().unwrap_or((
        (start_ts + end_ts) / 2,
        (start_price + end_price) / 2.0 + (start_price - end_price).abs() * 0.3,
    ));
    Box::new(Curve::new(start_ts, start_price, control_ts, control_price, end_ts, end_price, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "curve", display_name: "Curve", kind: PrimitiveKind::Shape,
        click_behavior: ClickBehavior::ThreePoint,
        tooltip: "Draw a Bezier curve (start, end, control)",
        icon: "curve", default_color: "#00BCD4",
        factory: create_curve,
        supports_text: true, has_levels: false, has_points_config: false,
    }
}
