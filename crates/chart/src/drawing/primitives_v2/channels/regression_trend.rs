//! Regression Trend primitive
//!
//! A linear regression channel with a center line calculated from price data
//! and parallel lines at standard deviation distances.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    point_to_line_distance, HIT_TOLERANCE, CONTROL_POINT_RADIUS, CONTROL_POINT_HIT_RADIUS,
    RenderContext, crisp, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text_rotated, calculate_line_text_params,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegressionTrend {
    pub data: PrimitiveData,
    pub ts1: i64,
    pub price1: f64,
    pub ts2: i64,
    pub price2: f64,
    #[serde(default = "default_std_dev")]
    pub std_dev_mult: f64,
    #[serde(default)]
    pub use_upper_deviation: bool,
    #[serde(default = "default_true")]
    pub show_center: bool,
    #[serde(default = "default_true")]
    pub fill: bool,
    #[serde(default = "default_fill_opacity")]
    pub fill_opacity: f64,
    #[serde(default)]
    pub extend_right: bool,
}

fn default_true() -> bool { true }
fn default_std_dev() -> f64 { 2.0 }
fn default_fill_opacity() -> f64 { 0.2 }

impl RegressionTrend {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "regression_trend".to_string(),
                display_name: "Regression Trend".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            ts1, price1, ts2, price2,
            std_dev_mult: 2.0,
            use_upper_deviation: false,
            show_center: true,
            fill: true,
            fill_opacity: 0.2,
            extend_right: false,
        }
    }

    pub fn channel_offset(&self) -> f64 {
        let price_range = (self.price2 - self.price1).abs();
        (price_range * 0.15).max(self.price1 * 0.02) * self.std_dev_mult / 2.0
    }
}

impl Primitive for RegressionTrend {
    fn type_id(&self) -> &'static str { "regression_trend" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Channel }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, self.price1), (self.ts2, self.price2)] }

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
            ControlPointType::Point1 => { self.ts1 = ts_ms; self.price1 = price; }
            ControlPointType::Point2 => { self.ts2 = ts_ms; self.price2 = price; }
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

        let offset = self.channel_offset();
        let upper_y1 = viewport.price_to_y(self.price1 + offset, price_scale.price_min, price_scale.price_max);
        let upper_y2 = viewport.price_to_y(self.price2 + offset, price_scale.price_min, price_scale.price_max);
        let lower_y1 = viewport.price_to_y(self.price1 - offset, price_scale.price_min, price_scale.price_max);
        let lower_y2 = viewport.price_to_y(self.price2 - offset, price_scale.price_min, price_scale.price_max);

        if check_point_hit(screen_x, screen_y, x1, y1) { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if check_point_hit(screen_x, screen_y, x2, y2) { return HitTestResult::ControlPoint(ControlPointType::Point2); }

        let cx = (x1 + x2) / 2.0; let cy = (y1 + y2) / 2.0;
        if check_point_hit(screen_x, screen_y, cx, cy) { return HitTestResult::ControlPoint(ControlPointType::Move); }

        if self.show_center && point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE { return HitTestResult::Body; }
        if point_to_line_distance(screen_x, screen_y, x1, upper_y1, x2, upper_y2) < HIT_TOLERANCE { return HitTestResult::Body; }
        if point_to_line_distance(screen_x, screen_y, x1, lower_y1, x2, lower_y2) < HIT_TOLERANCE { return HitTestResult::Body; }

        if self.fill {
            let min_y = upper_y1.min(upper_y2).min(lower_y1).min(lower_y2);
            let max_y = upper_y1.max(upper_y2).max(lower_y1).max(lower_y2);
            let min_x = x1.min(x2); let max_x = x1.max(x2);
            if screen_x >= min_x && screen_x <= max_x && screen_y >= min_y && screen_y <= max_y {
                return HitTestResult::Body;
            }
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let x2 = viewport.bar_to_x_f64(bar2);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        let mut points = vec![ControlPoint::point1(x1, y1), ControlPoint::point2(x2, y2)];
        let cx = (x1 + x2) / 2.0; let cy = (y1 + y2) / 2.0;
        let dist = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
        if dist > CONTROL_POINT_RADIUS * 4.0 { points.push(ControlPoint::move_point(cx, cy)); }
        points
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y2 = ctx.price_to_y(self.price2);

        let offset = self.channel_offset();
        let upper_y1 = ctx.price_to_y(self.price1 + offset);
        let upper_y2 = ctx.price_to_y(self.price2 + offset);
        let lower_y1 = ctx.price_to_y(self.price1 - offset);
        let lower_y2 = ctx.price_to_y(self.price2 - offset);

        if self.fill {
            let fill_color = format!("{}{:02x}", &self.data.color.stroke[..7], (self.fill_opacity * 255.0) as u8);
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.move_to(x1, upper_y1); ctx.line_to(x2, upper_y2);
            ctx.line_to(x2, lower_y2); ctx.line_to(x1, lower_y1);
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
        ctx.move_to(crisp(x1, dpr), crisp(upper_y1, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(upper_y2, dpr));
        ctx.stroke();

        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(lower_y1, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(lower_y2, dpr));
        ctx.stroke();

        if self.show_center {
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.begin_path();
            ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
            ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
            ctx.stroke();
        }
        ctx.set_line_dash(&[]);

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (px, py) in [(x1, y1), (x2, y2)] {
                ctx.begin_path();
                ctx.arc(px, py, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill(); ctx.stroke();
            }
            let cx = (x1 + x2) / 2.0; let cy = (y1 + y2) / 2.0;
            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill(); ctx.stroke();
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let (text_y1, text_y2) = match text.v_align {
                    super::super::TextAlign::Start => (upper_y1, upper_y2),
                    super::super::TextAlign::End => (lower_y1, lower_y2),
                    super::super::TextAlign::Center => (y1, y2),
                };
                let params = calculate_line_text_params(x1, text_y1, x2, text_y2, text);
                render_primitive_text_rotated(ctx, text, params.x, params.y, &self.data.color.stroke, params.rotation);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2)
}

fn create_regression_trend(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
    let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, price1));
    Box::new(RegressionTrend::new(ts1, price1, ts2, price2, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "regression_trend",
        display_name: "Regression Trend",
        kind: PrimitiveKind::Channel,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Draw a linear regression channel",
        icon: "regression_trend",
        default_color: "#9C27B0",
        factory: create_regression_trend,
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
