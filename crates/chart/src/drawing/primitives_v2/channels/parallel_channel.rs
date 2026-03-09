//! Parallel Channel primitive
//!
//! Two parallel trend lines forming a channel. Created with 3 clicks:
//! - First two clicks define the main trend line
//! - Third click defines the width of the channel

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

/// Parallel Channel - two parallel trend lines
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParallelChannel {
    /// Common primitive data
    pub data: PrimitiveData,
    /// First point of main line (bar index)
    pub bar1: f64,
    /// First point of main line (price)
    pub price1: f64,
    /// Second point of main line (bar index)
    pub bar2: f64,
    /// Second point of main line (price)
    pub price2: f64,
    /// Price offset for the parallel line (can be positive or negative)
    pub channel_offset: f64,
    /// Extend lines to the left
    #[serde(default)]
    pub extend_left: bool,
    /// Extend lines to the right
    #[serde(default)]
    pub extend_right: bool,
    /// Fill the channel with semi-transparent color
    #[serde(default = "default_true")]
    pub fill: bool,
    /// Fill opacity (0.0 - 1.0)
    #[serde(default = "default_fill_opacity")]
    pub fill_opacity: f64,
}

fn default_true() -> bool {
    true
}

fn default_fill_opacity() -> f64 {
    0.2
}

impl ParallelChannel {
    /// Create a new parallel channel
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, channel_offset: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "parallel_channel".to_string(),
                display_name: "Parallel Channel".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            bar1,
            price1,
            bar2,
            price2,
            channel_offset,
            extend_left: false,
            extend_right: false,
            fill: true,
            fill_opacity: 0.2,
        }
    }

    /// Get the parallel line points (offset by channel_offset in price)
    pub fn parallel_line(&self) -> ((f64, f64), (f64, f64)) {
        (
            (self.bar1, self.price1 + self.channel_offset),
            (self.bar2, self.price2 + self.channel_offset),
        )
    }

    /// Calculate center line points (middle of channel)
    pub fn center_line(&self) -> ((f64, f64), (f64, f64)) {
        let half_offset = self.channel_offset / 2.0;
        (
            (self.bar1, self.price1 + half_offset),
            (self.bar2, self.price2 + half_offset),
        )
    }
}

impl Primitive for ParallelChannel {
    fn type_id(&self) -> &'static str {
        "parallel_channel"
    }

    fn display_name(&self) -> &str {
        &self.data.display_name
    }

    fn kind(&self) -> PrimitiveKind {
        PrimitiveKind::Channel
    }

    fn click_behavior(&self) -> ClickBehavior {
        ClickBehavior::ThreePoint
    }

    fn data(&self) -> &PrimitiveData {
        &self.data
    }

    fn data_mut(&mut self) -> &mut PrimitiveData {
        &mut self.data
    }

    fn points(&self) -> Vec<(f64, f64)> {
        // Return main line points plus the offset point for the parallel line
        vec![
            (self.bar1, self.price1),
            (self.bar2, self.price2),
            (self.bar1, self.price1 + self.channel_offset), // Third point for channel width
        ]
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        if points.len() >= 2 {
            self.bar1 = points[0].0;
            self.price1 = points[0].1;
            self.bar2 = points[1].0;
            self.price2 = points[1].1;
        }
        if points.len() >= 3 {
            // Third point determines channel offset
            self.channel_offset = points[2].1 - self.price1;
        }
    }

    fn translate(&mut self, bar_delta: f64, price_delta: f64) {
        self.bar1 += bar_delta;
        self.bar2 += bar_delta;
        self.price1 += price_delta;
        self.price2 += price_delta;
        // channel_offset stays the same (relative)
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
            ControlPointType::Point3 => {
                // Point3 controls the channel width (offset)
                // Calculate new offset based on perpendicular distance to main line
                self.channel_offset = price - self.price1;
            }
            ControlPointType::Point4 => {
                // Parallel line point 2
                self.channel_offset = price - self.price2;
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

        // Parallel line screen coordinates
        let py1 = viewport.price_to_y(self.price1 + self.channel_offset, price_scale.price_min, price_scale.price_max);
        let py2 = viewport.price_to_y(self.price2 + self.channel_offset, price_scale.price_min, price_scale.price_max);

        // Check control points
        if check_point_hit(screen_x, screen_y, x1, y1) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        if check_point_hit(screen_x, screen_y, x2, y2) {
            return HitTestResult::ControlPoint(ControlPointType::Point2);
        }
        if check_point_hit(screen_x, screen_y, x1, py1) {
            return HitTestResult::ControlPoint(ControlPointType::Point3);
        }
        if check_point_hit(screen_x, screen_y, x2, py2) {
            return HitTestResult::ControlPoint(ControlPointType::Point4);
        }

        // Check center move point
        let cx = (x1 + x2) / 2.0;
        let cy = (y1 + y2 + py1 + py2) / 4.0;
        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }

        // Check main line
        if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        // Check parallel line
        if point_to_line_distance(screen_x, screen_y, x1, py1, x2, py2) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        // Check if inside filled channel
        if self.fill {
            // Simple check: is point between the two lines?
            let line_y_at_x = |x: f64, lx1: f64, ly1: f64, lx2: f64, ly2: f64| -> f64 {
                if (lx2 - lx1).abs() < 0.001 {
                    (ly1 + ly2) / 2.0
                } else {
                    ly1 + (ly2 - ly1) * (x - lx1) / (lx2 - lx1)
                }
            };

            let main_y = line_y_at_x(screen_x, x1, y1, x2, y2);
            let parallel_y = line_y_at_x(screen_x, x1, py1, x2, py2);
            let min_y = main_y.min(parallel_y);
            let max_y = main_y.max(parallel_y);

            if screen_y >= min_y && screen_y <= max_y && screen_x >= x1.min(x2) && screen_x <= x1.max(x2) {
                return HitTestResult::Body;
            }
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

        let py1 = viewport.price_to_y(self.price1 + self.channel_offset, price_scale.price_min, price_scale.price_max);
        let py2 = viewport.price_to_y(self.price2 + self.channel_offset, price_scale.price_min, price_scale.price_max);

        let mut points = vec![
            ControlPoint::point1(x1, y1),
            ControlPoint::point2(x2, y2),
            ControlPoint::point3(x1, py1),
            ControlPoint::point4(x2, py2),
        ];

        // Center point for move
        let cx = (x1 + x2) / 2.0;
        let cy = (y1 + y2 + py1 + py2) / 4.0;
        let dist = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
        if dist > CONTROL_POINT_RADIUS * 4.0 {
            points.push(ControlPoint::move_point(cx, cy));
        }

        points
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.bar_to_x(self.bar1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.bar_to_x(self.bar2);
        let y2 = ctx.price_to_y(self.price2);
        let py1 = ctx.price_to_y(self.price1 + self.channel_offset);
        let py2 = ctx.price_to_y(self.price2 + self.channel_offset);

        // Fill if enabled
        if self.fill {
            let fill_color = format!("{}{}",
                &self.data.color.stroke[..7],
                format!("{:02x}", (self.fill_opacity * 255.0) as u8)
            );
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.move_to(x1, y1);
            ctx.line_to(x2, y2);
            ctx.line_to(x2, py2);
            ctx.line_to(x1, py1);
            ctx.close_path();
            ctx.fill();
        }

        // Draw lines
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Main line
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
        ctx.stroke();

        // Parallel line
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(py1, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(py2, dpr));
        ctx.stroke();
        ctx.set_line_dash(&[]);

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            for (px, py) in [(x1, y1), (x2, y2), (x1, py1), (x2, py2)] {
                ctx.begin_path();
                ctx.arc(px, py, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }

            // Center move point
            let cx = (x1 + x2) / 2.0;
            let cy = (y1 + y2 + py1 + py2) / 4.0;
            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present (rotated along channel line)
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Calculate text position based on v_align:
                // - Center: on median line (middle of channel) with gap
                // - Start: above upper boundary
                // - End: below lower boundary
                let (text_y1, text_y2) = match text.v_align {
                    super::super::TextAlign::Start => {
                        // Above upper boundary (use the higher of main/parallel lines)
                        let upper_y1 = y1.min(py1);
                        let upper_y2 = y2.min(py2);
                        (upper_y1, upper_y2)
                    }
                    super::super::TextAlign::End => {
                        // Below lower boundary (use the lower of main/parallel lines)
                        let lower_y1 = y1.max(py1);
                        let lower_y2 = y2.max(py2);
                        (lower_y1, lower_y2)
                    }
                    super::super::TextAlign::Center => {
                        // On median line (middle of channel)
                        let mid_y1 = (y1 + py1) / 2.0;
                        let mid_y2 = (y2 + py2) / 2.0;
                        (mid_y1, mid_y2)
                    }
                };

                let params = calculate_line_text_params(x1, text_y1, x2, text_y2, text);
                render_primitive_text_rotated(ctx, text, params.x, params.y, &self.data.color.stroke, params.rotation);
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
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2) as f64
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_parallel_channel(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar1, price1) = points.get(0).copied().unwrap_or((0.0, 0.0));
    let (bar2, price2) = points.get(1).copied().unwrap_or((bar1 + 10.0, price1));
    let channel_offset = if points.len() >= 3 {
        points[2].1 - price1
    } else {
        // Default offset: 5% of price
        price1 * 0.05
    };
    Box::new(ParallelChannel::new(bar1, price1, bar2, price2, channel_offset, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "parallel_channel",
        display_name: "Parallel Channel",
        kind: PrimitiveKind::Channel,
        click_behavior: ClickBehavior::ThreePoint,
        tooltip: "Draw a channel with two parallel trend lines",
        icon: "parallel_channel",
        default_color: "#2196F3",
        factory: create_parallel_channel,
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
