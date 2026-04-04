//! Disjoint Channel primitive
//!
//! A channel with two non-parallel lines (widening or narrowing).
//! Each line has independent endpoints.

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

/// Disjoint Channel - non-parallel channel (widening/narrowing)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DisjointChannel {
    /// Common primitive data
    pub data: PrimitiveData,
    /// First line - start bar
    pub line1_bar1: f64,
    /// First line - start price
    pub line1_price1: f64,
    /// First line - end bar
    pub line1_bar2: f64,
    /// First line - end price
    pub line1_price2: f64,
    /// Second line - start bar
    pub line2_bar1: f64,
    /// Second line - start price
    pub line2_price1: f64,
    /// Second line - end bar
    pub line2_bar2: f64,
    /// Second line - end price
    pub line2_price2: f64,
    /// Fill the channel
    #[serde(default = "default_true")]
    pub fill: bool,
    /// Fill opacity
    #[serde(default = "default_fill_opacity")]
    pub fill_opacity: f64,
    /// Extend lines to the right
    #[serde(default)]
    pub extend_right: bool,
}

fn default_true() -> bool {
    true
}

fn default_fill_opacity() -> f64 {
    0.2
}

impl DisjointChannel {
    /// Create a new disjoint channel
    /// Points define the first line, second line is offset initially
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, offset: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "disjoint_channel".to_string(),
                display_name: "Disjoint Channel".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            line1_bar1: bar1,
            line1_price1: price1,
            line1_bar2: bar2,
            line1_price2: price2,
            line2_bar1: bar1,
            line2_price1: price1 + offset,
            line2_bar2: bar2,
            line2_price2: price2 + offset,
            fill: true,
            fill_opacity: 0.2,
            extend_right: false,
        }
    }

    /// Create with all 4 points specified
    pub fn with_points(
        l1_bar1: f64, l1_price1: f64, l1_bar2: f64, l1_price2: f64,
        l2_bar1: f64, l2_price1: f64, l2_bar2: f64, l2_price2: f64,
        color: &str,
    ) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "disjoint_channel".to_string(),
                display_name: "Disjoint Channel".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            line1_bar1: l1_bar1,
            line1_price1: l1_price1,
            line1_bar2: l1_bar2,
            line1_price2: l1_price2,
            line2_bar1: l2_bar1,
            line2_price1: l2_price1,
            line2_bar2: l2_bar2,
            line2_price2: l2_price2,
            fill: true,
            fill_opacity: 0.2,
            extend_right: false,
        }
    }
}

impl Primitive for DisjointChannel {
    fn type_id(&self) -> &'static str {
        "disjoint_channel"
    }

    fn display_name(&self) -> &str {
        &self.data.display_name
    }

    fn kind(&self) -> PrimitiveKind {
        PrimitiveKind::Channel
    }

    fn click_behavior(&self) -> ClickBehavior {
        // 4 clicks: 2 for first line, 2 for second line
        ClickBehavior::FourPoint
    }

    fn data(&self) -> &PrimitiveData {
        &self.data
    }

    fn data_mut(&mut self) -> &mut PrimitiveData {
        &mut self.data
    }

    fn points(&self) -> Vec<(f64, f64)> {
        vec![
            (self.line1_bar1, self.line1_price1),
            (self.line1_bar2, self.line1_price2),
            (self.line2_bar1, self.line2_price1),
            (self.line2_bar2, self.line2_price2),
        ]
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        if points.len() >= 1 {
            self.line1_bar1 = points[0].0;
            self.line1_price1 = points[0].1;
        }
        if points.len() >= 2 {
            self.line1_bar2 = points[1].0;
            self.line1_price2 = points[1].1;
        }
        if points.len() >= 3 {
            self.line2_bar1 = points[2].0;
            self.line2_price1 = points[2].1;
        }
        if points.len() >= 4 {
            self.line2_bar2 = points[3].0;
            self.line2_price2 = points[3].1;
        } else if points.len() == 3 {
            // If only 3 points, make second line parallel offset
            let offset = self.line2_price1 - self.line1_price1;
            self.line2_bar2 = self.line1_bar2;
            self.line2_price2 = self.line1_price2 + offset;
        }
    }

    fn translate(&mut self, bar_delta: f64, price_delta: f64) {
        self.line1_bar1 += bar_delta;
        self.line1_bar2 += bar_delta;
        self.line1_price1 += price_delta;
        self.line1_price2 += price_delta;
        self.line2_bar1 += bar_delta;
        self.line2_bar2 += bar_delta;
        self.line2_price1 += price_delta;
        self.line2_price2 += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, bar: f64, price: f64) {
        match point_type {
            ControlPointType::Point1 => {
                self.line1_bar1 = bar;
                self.line1_price1 = price;
            }
            ControlPointType::Point2 => {
                self.line1_bar2 = bar;
                self.line1_price2 = price;
            }
            ControlPointType::Point3 => {
                self.line2_bar1 = bar;
                self.line2_price1 = price;
            }
            ControlPointType::Point4 => {
                self.line2_bar2 = bar;
                self.line2_price2 = price;
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
        // Line 1 screen coordinates
        let l1_x1 = viewport.bar_to_x_f64(self.line1_bar1);
        let l1_y1 = viewport.price_to_y(self.line1_price1, price_scale.price_min, price_scale.price_max);
        let l1_x2 = viewport.bar_to_x_f64(self.line1_bar2);
        let l1_y2 = viewport.price_to_y(self.line1_price2, price_scale.price_min, price_scale.price_max);

        // Line 2 screen coordinates
        let l2_x1 = viewport.bar_to_x_f64(self.line2_bar1);
        let l2_y1 = viewport.price_to_y(self.line2_price1, price_scale.price_min, price_scale.price_max);
        let l2_x2 = viewport.bar_to_x_f64(self.line2_bar2);
        let l2_y2 = viewport.price_to_y(self.line2_price2, price_scale.price_min, price_scale.price_max);

        // Check control points
        if check_point_hit(screen_x, screen_y, l1_x1, l1_y1) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        if check_point_hit(screen_x, screen_y, l1_x2, l1_y2) {
            return HitTestResult::ControlPoint(ControlPointType::Point2);
        }
        if check_point_hit(screen_x, screen_y, l2_x1, l2_y1) {
            return HitTestResult::ControlPoint(ControlPointType::Point3);
        }
        if check_point_hit(screen_x, screen_y, l2_x2, l2_y2) {
            return HitTestResult::ControlPoint(ControlPointType::Point4);
        }

        // Check center move point
        let cx = (l1_x1 + l1_x2 + l2_x1 + l2_x2) / 4.0;
        let cy = (l1_y1 + l1_y2 + l2_y1 + l2_y2) / 4.0;
        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }

        // Check line 1
        if point_to_line_distance(screen_x, screen_y, l1_x1, l1_y1, l1_x2, l1_y2) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        // Check line 2
        if point_to_line_distance(screen_x, screen_y, l2_x1, l2_y1, l2_x2, l2_y2) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        // Check if inside filled area (simplified quad check)
        if self.fill {
            // Check if point is within the bounding box first
            let min_x = l1_x1.min(l1_x2).min(l2_x1).min(l2_x2);
            let max_x = l1_x1.max(l1_x2).max(l2_x1).max(l2_x2);
            let min_y = l1_y1.min(l1_y2).min(l2_y1).min(l2_y2);
            let max_y = l1_y1.max(l1_y2).max(l2_y1).max(l2_y2);

            if screen_x >= min_x && screen_x <= max_x && screen_y >= min_y && screen_y <= max_y {
                // More detailed check: interpolate both lines at screen_x
                let line_y = |x: f64, x1: f64, y1: f64, x2: f64, y2: f64| -> Option<f64> {
                    if x < x1.min(x2) || x > x1.max(x2) {
                        return None;
                    }
                    if (x2 - x1).abs() < 0.001 {
                        return Some((y1 + y2) / 2.0);
                    }
                    Some(y1 + (y2 - y1) * (x - x1) / (x2 - x1))
                };

                if let (Some(y1_at_x), Some(y2_at_x)) = (
                    line_y(screen_x, l1_x1, l1_y1, l1_x2, l1_y2),
                    line_y(screen_x, l2_x1, l2_y1, l2_x2, l2_y2),
                ) {
                    let min_line_y = y1_at_x.min(y2_at_x);
                    let max_line_y = y1_at_x.max(y2_at_x);
                    if screen_y >= min_line_y && screen_y <= max_line_y {
                        return HitTestResult::Body;
                    }
                }
            }
        }

        HitTestResult::Miss
    }

    fn control_points(
        &self,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Vec<ControlPoint> {
        let l1_x1 = viewport.bar_to_x_f64(self.line1_bar1);
        let l1_y1 = viewport.price_to_y(self.line1_price1, price_scale.price_min, price_scale.price_max);
        let l1_x2 = viewport.bar_to_x_f64(self.line1_bar2);
        let l1_y2 = viewport.price_to_y(self.line1_price2, price_scale.price_min, price_scale.price_max);
        let l2_x1 = viewport.bar_to_x_f64(self.line2_bar1);
        let l2_y1 = viewport.price_to_y(self.line2_price1, price_scale.price_min, price_scale.price_max);
        let l2_x2 = viewport.bar_to_x_f64(self.line2_bar2);
        let l2_y2 = viewport.price_to_y(self.line2_price2, price_scale.price_min, price_scale.price_max);

        let mut points = vec![
            ControlPoint::point1(l1_x1, l1_y1),
            ControlPoint::point2(l1_x2, l1_y2),
            ControlPoint::point3(l2_x1, l2_y1),
            ControlPoint::point4(l2_x2, l2_y2),
        ];

        // Center point for move
        let cx = (l1_x1 + l1_x2 + l2_x1 + l2_x2) / 4.0;
        let cy = (l1_y1 + l1_y2 + l2_y1 + l2_y2) / 4.0;
        let dist = ((l1_x2 - l1_x1).powi(2) + (l1_y2 - l1_y1).powi(2)).sqrt();
        if dist > CONTROL_POINT_RADIUS * 4.0 {
            points.push(ControlPoint::move_point(cx, cy));
        }

        points
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let l1_x1 = ctx.bar_to_x(self.line1_bar1);
        let l1_y1 = ctx.price_to_y(self.line1_price1);
        let l1_x2 = ctx.bar_to_x(self.line1_bar2);
        let l1_y2 = ctx.price_to_y(self.line1_price2);
        let l2_x1 = ctx.bar_to_x(self.line2_bar1);
        let l2_y1 = ctx.price_to_y(self.line2_price1);
        let l2_x2 = ctx.bar_to_x(self.line2_bar2);
        let l2_y2 = ctx.price_to_y(self.line2_price2);

        // Fill if enabled
        if self.fill {
            let fill_color = format!("{}{}",
                &self.data.color.stroke[..7],
                format!("{:02x}", (self.fill_opacity * 255.0) as u8)
            );
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.move_to(l1_x1, l1_y1);
            ctx.line_to(l1_x2, l1_y2);
            ctx.line_to(l2_x2, l2_y2);
            ctx.line_to(l2_x1, l2_y1);
            ctx.close_path();
            ctx.fill();
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

        // Line 1
        ctx.begin_path();
        ctx.move_to(crisp(l1_x1, dpr), crisp(l1_y1, dpr));
        ctx.line_to(crisp(l1_x2, dpr), crisp(l1_y2, dpr));
        ctx.stroke();

        // Line 2
        ctx.begin_path();
        ctx.move_to(crisp(l2_x1, dpr), crisp(l2_y1, dpr));
        ctx.line_to(crisp(l2_x2, dpr), crisp(l2_y2, dpr));
        ctx.stroke();
        ctx.set_line_dash(&[]);

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            for (px, py) in [(l1_x1, l1_y1), (l1_x2, l1_y2), (l2_x1, l2_y1), (l2_x2, l2_y2)] {
                ctx.begin_path();
                ctx.arc(px, py, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }

            // Center move point
            let cx = (l1_x1 + l1_x2 + l2_x1 + l2_x2) / 4.0;
            let cy = (l1_y1 + l1_y2 + l2_y1 + l2_y2) / 4.0;
            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present (rotated along channel)
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Calculate text position based on v_align:
                // - Center: on median line (between line1 and line2)
                // - Start: on upper boundary (min of line1 and line2)
                // - End: on lower boundary (max of line1 and line2)
                let mid_y1 = (l1_y1 + l2_y1) / 2.0;
                let mid_y2 = (l1_y2 + l2_y2) / 2.0;
                let mid_x1 = (l1_x1 + l2_x1) / 2.0;
                let mid_x2 = (l1_x2 + l2_x2) / 2.0;

                let (text_x1, text_y1, text_x2, text_y2) = match text.v_align {
                    super::super::TextAlign::Start => {
                        // Upper boundary (line with lower y values since y increases downward)
                        if l1_y1 < l2_y1 {
                            (l1_x1, l1_y1, l1_x2, l1_y2)
                        } else {
                            (l2_x1, l2_y1, l2_x2, l2_y2)
                        }
                    }
                    super::super::TextAlign::End => {
                        // Lower boundary (line with higher y values)
                        if l1_y1 > l2_y1 {
                            (l1_x1, l1_y1, l1_x2, l1_y2)
                        } else {
                            (l2_x1, l2_y1, l2_x2, l2_y2)
                        }
                    }
                    super::super::TextAlign::Center => {
                        // Median line
                        (mid_x1, mid_y1, mid_x2, mid_y2)
                    }
                };

                let params = calculate_line_text_params(text_x1, text_y1, text_x2, text_y2, text);
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
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2)
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_disjoint_channel(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar1, price1) = points.first().copied().unwrap_or((0.0, 100.0));
    let (bar2, price2) = points.get(1).copied().unwrap_or((bar1 + 20.0, price1 * 1.05));

    if points.len() >= 4 {
        Box::new(DisjointChannel::with_points(
            bar1, price1, bar2, price2,
            points[2].0, points[2].1, points[3].0, points[3].1,
            color,
        ))
    } else if points.len() >= 3 {
        // Use third point for initial offset
        let offset = points[2].1 - price1;
        Box::new(DisjointChannel::new(bar1, price1, bar2, price2, offset, color))
    } else {
        // Default offset
        let offset = price1 * 0.05;
        Box::new(DisjointChannel::new(bar1, price1, bar2, price2, offset, color))
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "disjoint_channel",
        display_name: "Disjoint Channel",
        kind: PrimitiveKind::Channel,
        click_behavior: ClickBehavior::FourPoint,
        tooltip: "Draw a non-parallel (widening/narrowing) channel",
        icon: "disjoint_channel",
        default_color: "#4CAF50",
        factory: create_disjoint_channel,
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
