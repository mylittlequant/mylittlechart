//! Flat Top/Bottom primitive
//!
//! A channel with one sloped line and one horizontal line.
//! Useful for patterns like ascending/descending triangles.

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

/// Type of flat line
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[derive(Default)]
pub enum FlatType {
    /// Flat line on top (descending triangle)
    #[default]
    Top,
    /// Flat line on bottom (ascending triangle)
    Bottom,
}


/// Flat Top/Bottom - channel with one horizontal line
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FlatTopBottom {
    /// Common primitive data
    pub data: PrimitiveData,
    /// First point of sloped line (bar)
    pub bar1: f64,
    /// First point of sloped line (price)
    pub price1: f64,
    /// Second point of sloped line (bar)
    pub bar2: f64,
    /// Second point of sloped line (price)
    pub price2: f64,
    /// Price level of the flat (horizontal) line
    pub flat_price: f64,
    /// Whether the flat line is on top or bottom
    #[serde(default)]
    pub flat_type: FlatType,
    /// Fill the channel
    #[serde(default = "default_true")]
    pub fill: bool,
    /// Fill opacity
    #[serde(default = "default_fill_opacity")]
    pub fill_opacity: f64,
}

fn default_true() -> bool {
    true
}

fn default_fill_opacity() -> f64 {
    0.2
}

impl FlatTopBottom {
    /// Create a new flat top/bottom channel
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, flat_price: f64, color: &str) -> Self {
        // Determine if flat line is on top or bottom
        let flat_type = if flat_price > (price1 + price2) / 2.0 {
            FlatType::Top
        } else {
            FlatType::Bottom
        };

        Self {
            data: PrimitiveData {
                type_id: "flat_top_bottom".to_string(),
                display_name: if flat_type == FlatType::Top { "Flat Top" } else { "Flat Bottom" }.to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            bar1,
            price1,
            bar2,
            price2,
            flat_price,
            flat_type,
            fill: true,
            fill_opacity: 0.2,
        }
    }
}

impl Primitive for FlatTopBottom {
    fn type_id(&self) -> &'static str {
        "flat_top_bottom"
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
        vec![
            (self.bar1, self.price1),
            (self.bar2, self.price2),
            (self.bar1, self.flat_price), // Third point defines the flat line level
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
            self.flat_price = points[2].1;
            // Update flat type based on position
            self.flat_type = if self.flat_price > (self.price1 + self.price2) / 2.0 {
                FlatType::Top
            } else {
                FlatType::Bottom
            };
            self.data.display_name = if self.flat_type == FlatType::Top {
                "Flat Top".to_string()
            } else {
                "Flat Bottom".to_string()
            };
        }
    }

    fn translate(&mut self, bar_delta: f64, price_delta: f64) {
        self.bar1 += bar_delta;
        self.bar2 += bar_delta;
        self.price1 += price_delta;
        self.price2 += price_delta;
        self.flat_price += price_delta;
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
                // Point3 controls the flat line level (only price, bar stays at bar1)
                self.flat_price = price;
            }
            ControlPointType::Point4 => {
                // Point4 also controls flat line level (at bar2)
                self.flat_price = price;
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
        let x2 = viewport.bar_to_x_f64(self.bar2);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);
        let flat_y = viewport.price_to_y(self.flat_price, price_scale.price_min, price_scale.price_max);

        // Check control points
        if check_point_hit(screen_x, screen_y, x1, y1) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        if check_point_hit(screen_x, screen_y, x2, y2) {
            return HitTestResult::ControlPoint(ControlPointType::Point2);
        }
        if check_point_hit(screen_x, screen_y, x1, flat_y) {
            return HitTestResult::ControlPoint(ControlPointType::Point3);
        }
        if check_point_hit(screen_x, screen_y, x2, flat_y) {
            return HitTestResult::ControlPoint(ControlPointType::Point4);
        }

        // Check center move point
        let cx = (x1 + x2) / 2.0;
        let cy = (y1 + y2 + flat_y + flat_y) / 4.0;
        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }

        // Check sloped line
        if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        // Check flat (horizontal) line
        if point_to_line_distance(screen_x, screen_y, x1, flat_y, x2, flat_y) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        // Check if inside filled area
        if self.fill {
            let min_x = x1.min(x2);
            let max_x = x1.max(x2);

            if screen_x >= min_x && screen_x <= max_x {
                // Interpolate sloped line y at screen_x
                let t = if (x2 - x1).abs() > 0.001 {
                    (screen_x - x1) / (x2 - x1)
                } else {
                    0.5
                };
                let sloped_y = y1 + t * (y2 - y1);

                let min_y = sloped_y.min(flat_y);
                let max_y = sloped_y.max(flat_y);

                if screen_y >= min_y && screen_y <= max_y {
                    return HitTestResult::Body;
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
        let x1 = viewport.bar_to_x_f64(self.bar1);
        let x2 = viewport.bar_to_x_f64(self.bar2);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);
        let flat_y = viewport.price_to_y(self.flat_price, price_scale.price_min, price_scale.price_max);

        let mut points = vec![
            ControlPoint::point1(x1, y1),
            ControlPoint::point2(x2, y2),
            ControlPoint::point3(x1, flat_y),
            ControlPoint::point4(x2, flat_y),
        ];

        // Center point for move
        let cx = (x1 + x2) / 2.0;
        let cy = (y1 + y2 + flat_y + flat_y) / 4.0;
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
        let flat_y = ctx.price_to_y(self.flat_price);

        // Fill if enabled
        if self.fill {
            let fill_color = format!("{}{:02x}", &self.data.color.stroke[..7], (self.fill_opacity * 255.0) as u8);
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.move_to(x1, y1);
            ctx.line_to(x2, y2);
            ctx.line_to(x2, flat_y);
            ctx.line_to(x1, flat_y);
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

        // Sloped line
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
        ctx.stroke();

        // Flat (horizontal) line
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(flat_y, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(flat_y, dpr));
        ctx.stroke();
        ctx.set_line_dash(&[]);

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            for (px, py) in [(x1, y1), (x2, y2), (x1, flat_y), (x2, flat_y)] {
                ctx.begin_path();
                ctx.arc(px, py, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }

            // Center move point
            let cx = (x1 + x2) / 2.0;
            let cy = (y1 + y2 + flat_y + flat_y) / 4.0;
            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present (rotated along sloped line)
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Calculate text position based on v_align:
                // - Center: on the median (between sloped and flat lines)
                // - Start: on upper boundary (min of y1,y2 and flat_y)
                // - End: on lower boundary (max of y1,y2 and flat_y)
                let mid_y1 = (y1 + flat_y) / 2.0;
                let mid_y2 = (y2 + flat_y) / 2.0;

                let (text_y1, text_y2) = match text.v_align {
                    super::super::TextAlign::Start => {
                        // Upper boundary
                        (y1.min(flat_y), y2.min(flat_y))
                    }
                    super::super::TextAlign::End => {
                        // Lower boundary
                        (y1.max(flat_y), y2.max(flat_y))
                    }
                    super::super::TextAlign::Center => {
                        // Median line
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
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2)
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_flat_top_bottom(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar1, price1) = points.first().copied().unwrap_or((0.0, 100.0));
    let (bar2, price2) = points.get(1).copied().unwrap_or((bar1 + 20.0, price1 * 1.05));
    let flat_price = if points.len() >= 3 {
        points[2].1
    } else {
        price1 * 1.1 // Default flat line above
    };
    Box::new(FlatTopBottom::new(bar1, price1, bar2, price2, flat_price, color))
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
        has_points_config: false,
    }
}
