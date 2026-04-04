//! Trend Angle primitive
//!
//! A trend line that displays the angle of the line in degrees.
//! Useful for measuring the slope of price movements.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    point_to_line_distance, HIT_TOLERANCE, CONTROL_POINT_RADIUS, CONTROL_POINT_HIT_RADIUS,
    RenderContext, crisp, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text_rotated, calculate_line_text_params,
};

/// Trend Angle - trend line with angle measurement
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrendAngle {
    /// Common primitive data
    pub data: PrimitiveData,
    /// First point bar index
    pub bar1: f64,
    /// First point price
    pub price1: f64,
    /// Second point bar index
    pub bar2: f64,
    /// Second point price
    pub price2: f64,
    /// Show angle arc visualization
    #[serde(default = "default_true")]
    pub show_arc: bool,
    /// Show angle value label
    #[serde(default = "default_true")]
    pub show_label: bool,
}

fn default_true() -> bool {
    true
}

impl TrendAngle {
    /// Create a new trend angle
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "trend_angle".to_string(),
                display_name: "Trend Angle".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            bar1,
            price1,
            bar2,
            price2,
            show_arc: true,
            show_label: true,
        }
    }

    /// Calculate the angle in degrees (relative to horizontal)
    /// Note: This calculates the visual angle based on screen coordinates,
    /// which depends on the chart's aspect ratio and scale.
    pub fn angle_degrees(&self, viewport: &Viewport, price_scale: &PriceScale) -> f64 {
        let x1 = viewport.bar_to_x_f64(self.bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(self.bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        let dx = x2 - x1;
        let dy = y1 - y2; // Inverted because screen Y is flipped

        if dx.abs() < 1e-10 {
            if dy >= 0.0 { 90.0 } else { -90.0 }
        } else {
            (dy / dx).atan().to_degrees()
        }
    }

    /// Get the arc radius for drawing the angle visualization
    pub fn arc_radius(&self, viewport: &Viewport, price_scale: &PriceScale) -> f64 {
        let x1 = viewport.bar_to_x_f64(self.bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(self.bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        let line_length = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
        // Arc radius is 30% of line length, clamped to reasonable values
        (line_length * 0.3).clamp(20.0, 60.0)
    }

    /// Get formatted angle text
    pub fn angle_text(&self, viewport: &Viewport, price_scale: &PriceScale) -> String {
        format!("{:.1}°", self.angle_degrees(viewport, price_scale))
    }
}

impl Primitive for TrendAngle {
    fn type_id(&self) -> &'static str {
        "trend_angle"
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

    fn points(&self) -> Vec<(f64, f64)> {
        vec![(self.bar1, self.price1), (self.bar2, self.price2)]
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        if points.len() >= 2 {
            self.bar1 = points[0].0;
            self.price1 = points[0].1;
            self.bar2 = points[1].0;
            self.price2 = points[1].1;
        }
    }

    fn translate(&mut self, bar_delta: f64, price_delta: f64) {
        self.bar1 += bar_delta;
        self.bar2 += bar_delta;
        self.price1 += price_delta;
        self.price2 += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, bar: f64, price: f64) {
        match point_type {
            ControlPointType::Point1 => {
                self.bar1 = bar;
                self.price1 = price;
            }
            ControlPointType::Point2 => {
                self.bar2 = bar;
                self.price2 = price;
            }
            _ => {}
        }
    }

    fn hit_test(
        &self,
        screen_x: f64,
        screen_y: f64,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> HitTestResult {
        let x1 = viewport.bar_to_x_f64(self.bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(self.bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        // Check control points first
        if check_point_hit(screen_x, screen_y, x1, y1) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        if check_point_hit(screen_x, screen_y, x2, y2) {
            return HitTestResult::ControlPoint(ControlPointType::Point2);
        }

        // Check center/move point
        let cx = (x1 + x2) / 2.0;
        let cy = (y1 + y2) / 2.0;
        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }

        // Check line body
        if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        HitTestResult::Miss
    }

    fn control_points(
        &self,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Vec<ControlPoint> {
        let x1 = viewport.bar_to_x_f64(self.bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(self.bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        let mut points = vec![
            ControlPoint::point1(x1, y1),
            ControlPoint::point2(x2, y2),
        ];

        // Center point for move
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
        let x1 = ctx.bar_to_x(self.bar1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.bar_to_x(self.bar2);
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

        // Check if we need line gap (only for v_align == Center)
        let needs_gap = self.data.text.as_ref()
            .map(|t| !t.content.is_empty() && matches!(t.v_align, super::super::TextAlign::Center))
            .unwrap_or(false);

        ctx.begin_path();

        if needs_gap {
            let text = self.data.text.as_ref().unwrap();

            // Line length
            let dx = x2 - x1;
            let dy = y2 - y1;
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
                    let gap_x1 = x1 + dx * t_start;
                    let gap_y1 = y1 + dy * t_start;
                    ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
                    ctx.line_to(crisp(gap_x1, dpr), crisp(gap_y1, dpr));
                }

                // Draw second segment [t_end, 1]
                if t_end < 0.999 {
                    let gap_x2 = x1 + dx * t_end;
                    let gap_y2 = y1 + dy * t_end;
                    ctx.move_to(crisp(gap_x2, dpr), crisp(gap_y2, dpr));
                    ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
                }
            }
        } else {
            // No gap needed - draw full line
            ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
            ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
        }

        ctx.stroke();

        // Reset line dash for arc and annotations
        ctx.set_line_dash(&[]);

        // Calculate angle for display
        let dx = x2 - x1;
        let dy = y1 - y2; // Inverted because screen Y is flipped
        let angle_rad = if dx.abs() > 0.001 { (dy / dx).atan() } else if dy >= 0.0 { std::f64::consts::FRAC_PI_2 } else { -std::f64::consts::FRAC_PI_2 };
        let angle_deg = angle_rad.to_degrees();

        // Draw angle arc if enabled
        if self.show_arc {
            let line_length = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
            let arc_radius = (line_length * 0.3).clamp(20.0, 60.0);

            // Draw horizontal reference line (dashed)
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.begin_path();
            ctx.move_to(x1, y1);
            ctx.line_to(x1 + arc_radius * 1.5, y1);
            ctx.stroke();
            ctx.set_line_dash(&[]);

            // Draw angle arc
            let start_angle = 0.0;
            let end_angle = -angle_rad; // Negative because screen Y is flipped
            ctx.begin_path();
            if end_angle >= start_angle {
                ctx.arc(x1, y1, arc_radius, start_angle, end_angle);
            } else {
                ctx.arc(x1, y1, arc_radius, end_angle, start_angle);
            }
            ctx.stroke();
        }

        // Draw angle label if enabled
        if self.show_label {
            let angle_text = format!("{:.1}°", angle_deg);
            let label_distance = 40.0;
            let label_angle = -angle_rad / 2.0;
            let label_x = x1 + label_distance * label_angle.cos();
            let label_y = y1 - label_distance * label_angle.sin();

            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(&self.data.color.stroke);
            use crate::render::{TextAlign, TextBaseline};
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(&angle_text, label_x, label_y);
        }

        // Render text if present (rotated along line)
        if let Some(ref text) = self.data.text {
            if let Some(ref params) = text_params {
                render_primitive_text_rotated(ctx, text, params.x, params.y, &self.data.color.stroke, params.rotation);
            }
        }

        // Draw control points if selected
        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

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

            // Center move handle
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
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    fn clone_box(&self) -> Box<dyn Primitive> {
        Box::new(self.clone())
    }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2)
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_trend_angle(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar1, price1) = points.first().copied().unwrap_or((0.0, 0.0));
    let (bar2, price2) = points.get(1).copied().unwrap_or((bar1, price1));
    Box::new(TrendAngle::new(bar1, price1, bar2, price2, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "trend_angle",
        display_name: "Trend Angle",
        kind: PrimitiveKind::Line,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Draw a trend line showing the angle in degrees",
        icon: "trend_angle",
        default_color: "#FF9800",
        factory: create_trend_angle,
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
