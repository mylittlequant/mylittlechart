//! Trend Line primitive
//!
//! A simple line between two points. The most basic drawing tool.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, ExtendMode, LineStyle,
    point_to_line_distance, HIT_TOLERANCE, CONTROL_POINT_RADIUS,
    RenderContext, crisp, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text_rotated, calculate_line_text_params,
};

/// Trend Line - line between two points
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrendLine {
    /// Common primitive data (includes centralized point_timestamps)
    pub data: PrimitiveData,

    /// First point timestamp (Unix ms)
    pub ts1: i64,
    /// First point price
    pub price1: f64,
    /// Second point timestamp (Unix ms)
    pub ts2: i64,
    /// Second point price
    pub price2: f64,

    /// Line extension mode
    #[serde(default)]
    pub extend: ExtendMode,
    /// Show price labels at endpoints
    #[serde(default)]
    pub show_price_labels: bool,
}

impl TrendLine {
    /// Create a new trend line
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "trend_line".to_string(),
                display_name: "Trend Line".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            ts1,
            price1,
            ts2,
            price2,
            extend: ExtendMode::None,
            show_price_labels: false,
        }
    }
}

impl Primitive for TrendLine {
    fn type_id(&self) -> &'static str {
        "trend_line"
    }

    fn display_name(&self) -> &str {
        &self.data.display_name
    }

    fn kind(&self) -> PrimitiveKind {
        PrimitiveKind::Line
    }

    fn click_behavior(&self) -> ClickBehavior {
        ClickBehavior::TwoPoint
    }

    fn data(&self) -> &PrimitiveData {
        &self.data
    }

    fn data_mut(&mut self) -> &mut PrimitiveData {
        &mut self.data
    }

    fn points(&self) -> Vec<(i64, f64)> {
        vec![(self.ts1, self.price1), (self.ts2, self.price2)]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if points.len() >= 2 {
            self.ts1 = points[0].0;
            self.price1 = points[0].1;
            self.ts2 = points[1].0;
            self.price2 = points[1].1;
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        self.ts1 += ts_delta_ms;
        self.ts2 += ts_delta_ms;
        self.price1 += price_delta;
        self.price2 += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        match point_type {
            ControlPointType::Point1 => {
                self.ts1 = ts_ms;
                self.price1 = price;
            }
            ControlPointType::Point2 => {
                self.ts2 = ts_ms;
                self.price2 = price;
            }
            ControlPointType::Move => {
                // Move is handled by translate() via delta
            }
            _ => {}
        }
    }

    fn hit_test(
        &self,
        screen_x: f64,
        screen_y: f64,
        bars: &[Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> HitTestResult {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        // Check control points first
        if let Some(cp) = self.hit_test_control_points(screen_x, screen_y, x1, y1, x2, y2) {
            return HitTestResult::ControlPoint(cp);
        }

        // Check line body
        if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        HitTestResult::Miss
    }

    fn control_points(
        &self,
        bars: &[Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Vec<ControlPoint> {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        let mut points = vec![
            ControlPoint::point1(x1, y1),
            ControlPoint::point2(x2, y2),
        ];

        // Add center point for move (only if line is long enough)
        let cx = (x1 + x2) / 2.0;
        let cy = (y1 + y2) / 2.0;
        let dist = ((cx - x1).powi(2) + (cy - y1).powi(2)).sqrt();
        if dist > CONTROL_POINT_RADIUS * 4.0 {
            points.push(ControlPoint::move_point(cx, cy));
        }

        points
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();

        // Convert to screen coordinates
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y2 = ctx.price_to_y(self.price2);

        // Calculate text params first (needed for line gap)
        let text_params = self.data.text.as_ref()
            .filter(|t| !t.content.is_empty())
            .map(|text| calculate_line_text_params(x1, y1, x2, y2, text));

        // Set stroke style
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);

        // Set line dash based on style
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Calculate line endpoints based on extend mode
        let (line_x1, line_y1, line_x2, line_y2) = match self.extend {
            ExtendMode::None => (x1, y1, x2, y2),
            ExtendMode::Right => {
                let dx = x2 - x1;
                let dy = y2 - y1;
                let extend_x = ctx.chart_width();
                let t = if dx.abs() > 0.001 { (extend_x - x1) / dx } else { 1000.0 };
                let extend_y = y1 + dy * t;
                (x1, y1, extend_x, extend_y)
            }
            ExtendMode::Left => {
                let dx = x2 - x1;
                let dy = y2 - y1;
                let t = if dx.abs() > 0.001 { -x1 / dx } else { -1000.0 };
                let extend_y = y1 + dy * t;
                (0.0, extend_y, x2, y2)
            }
            ExtendMode::Both => {
                let dx = x2 - x1;
                let dy = y2 - y1;
                let t_left = if dx.abs() > 0.001 { -x1 / dx } else { -1000.0 };
                let t_right = if dx.abs() > 0.001 { (ctx.chart_width() - x1) / dx } else { 1000.0 };
                let left_y = y1 + dy * t_left;
                let right_y = y1 + dy * t_right;
                (0.0, left_y, ctx.chart_width(), right_y)
            }
        };

        // Check if we need line gap (only for v_align == Center)
        let needs_gap = self.data.text.as_ref()
            .map(|t| !t.content.is_empty() && matches!(t.v_align, super::super::TextAlign::Center))
            .unwrap_or(false);

        ctx.begin_path();

        if needs_gap {
            let text = self.data.text.as_ref().unwrap();

            // Line length
            let dx = line_x2 - line_x1;
            let dy = line_y2 - line_y1;
            let line_len = (dx * dx + dy * dy).sqrt();

            if line_len > 0.001 {
                // Text position along line (0..1)
                let t_center = match text.h_align {
                    super::super::TextAlign::Start => 0.0,
                    super::super::TextAlign::Center => 0.5,
                    super::super::TextAlign::End => 1.0,
                };

                // Estimate text width in screen pixels
                let char_count = text.content.len() as f64;
                let text_width = char_count * text.font_size * 0.6 + 8.0; // +padding

                // Convert text width to parametric t
                let half_gap_t = (text_width / 2.0) / line_len;

                let t_start = (t_center - half_gap_t).max(0.0);
                let t_end = (t_center + half_gap_t).min(1.0);

                // Draw first segment [0, t_start]
                if t_start > 0.001 {
                    let gap_x1 = line_x1 + dx * t_start;
                    let gap_y1 = line_y1 + dy * t_start;
                    ctx.move_to(crisp(line_x1, dpr), crisp(line_y1, dpr));
                    ctx.line_to(crisp(gap_x1, dpr), crisp(gap_y1, dpr));
                }

                // Draw second segment [t_end, 1]
                if t_end < 0.999 {
                    let gap_x2 = line_x1 + dx * t_end;
                    let gap_y2 = line_y1 + dy * t_end;
                    ctx.move_to(crisp(gap_x2, dpr), crisp(gap_y2, dpr));
                    ctx.line_to(crisp(line_x2, dpr), crisp(line_y2, dpr));
                }
            }
        } else {
            // No gap needed - draw full line
            ctx.move_to(crisp(line_x1, dpr), crisp(line_y1, dpr));
            ctx.line_to(crisp(line_x2, dpr), crisp(line_y2, dpr));
        }

        ctx.stroke();

        // Reset line dash
        ctx.set_line_dash(&[]);

        // Render text if present (rotated along line)
        if let Some(ref text) = self.data.text {
            if let Some(ref params) = text_params {
                render_primitive_text_rotated(ctx, text, params.x, params.y, &self.data.color.stroke, params.rotation);
            }
        }

        // Draw control points if selected
        if is_selected {
            self.draw_control_points(ctx, x1, y1, x2, y2);
        }
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    fn clone_box(&self) -> Box<dyn Primitive> {
        Box::new(self.clone())
    }

    fn extend_mode_raw(&self) -> u8 {
        match self.extend {
            ExtendMode::None => 0,
            ExtendMode::Right => 1,
            ExtendMode::Left => 2,
            ExtendMode::Both => 3,
        }
    }
}

impl TrendLine {
    /// Render control points when selected
    fn draw_control_points(&self, ctx: &mut dyn RenderContext, x1: f64, y1: f64, x2: f64, y2: f64) {
        // Control point style
        ctx.set_stroke_color(CONTROL_POINT_STROKE);
        ctx.set_fill_color(CONTROL_POINT_FILL);
        ctx.set_stroke_width(1.5);
        ctx.set_line_dash(&[]);

        // Point 1
        ctx.begin_path();
        ctx.arc(x1, y1, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
        ctx.fill();
        ctx.stroke();

        // Point 2
        ctx.begin_path();
        ctx.arc(x2, y2, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
        ctx.fill();
        ctx.stroke();

        // Center move handle (only if line is long enough)
        let cx = (x1 + x2) / 2.0;
        let cy = (y1 + y2) / 2.0;
        let dist = ((cx - x1).powi(2) + (cy - y1).powi(2)).sqrt();
        if dist > CONTROL_POINT_RADIUS * 4.0 {
            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }
    }

    /// Hit test control points
    fn hit_test_control_points(
        &self,
        screen_x: f64,
        screen_y: f64,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
    ) -> Option<ControlPointType> {
        use super::super::CONTROL_POINT_HIT_RADIUS;

        // Point 1
        if (screen_x - x1).powi(2) + (screen_y - y1).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2) {
            return Some(ControlPointType::Point1);
        }

        // Point 2
        if (screen_x - x2).powi(2) + (screen_y - y2).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2) {
            return Some(ControlPointType::Point2);
        }

        // Center (Move)
        let cx = (x1 + x2) / 2.0;
        let cy = (y1 + y2) / 2.0;
        if (screen_x - cx).powi(2) + (screen_y - cy).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2) {
            return Some(ControlPointType::Move);
        }

        None
    }
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_trend_line(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
    let (ts2, price2) = points.get(1).copied().unwrap_or((ts1, price1));
    Box::new(TrendLine::new(ts1, price1, ts2, price2, color))
}

/// Get metadata for registry
pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "trend_line",
        display_name: "Trend Line",
        kind: PrimitiveKind::Line,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Draw a trend line between two points",
        icon: "trend_line",
        default_color: "#2196F3",
        factory: create_trend_line,
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
