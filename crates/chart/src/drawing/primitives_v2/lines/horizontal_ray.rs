//! Horizontal Ray primitive
//!
//! A horizontal line from a single point extending infinitely to the right.
//! Useful for marking support/resistance levels from a specific bar.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    HIT_TOLERANCE, CONTROL_POINT_HIT_RADIUS, CONTROL_POINT_RADIUS,
    RenderContext, crisp, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

/// Horizontal Ray - horizontal line extending right from a point
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HorizontalRay {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Starting bar index
    pub bar: f64,
    /// Price level
    pub price: f64,
    /// Show price label on the right
    #[serde(default = "default_true")]
    pub show_price_label: bool,
}

fn default_true() -> bool {
    true
}

impl HorizontalRay {
    /// Create a new horizontal ray
    pub fn new(bar: f64, price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "horizontal_ray".to_string(),
                display_name: "Horizontal Ray".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            bar,
            price,
            show_price_label: true,
        }
    }
}

impl Primitive for HorizontalRay {
    fn type_id(&self) -> &'static str {
        "horizontal_ray"
    }

    fn display_name(&self) -> &str {
        &self.data.display_name
    }

    fn kind(&self) -> PrimitiveKind {
        PrimitiveKind::Line
    }

    fn click_behavior(&self) -> ClickBehavior {
        ClickBehavior::SingleClick
    }

    fn data(&self) -> &PrimitiveData {
        &self.data
    }

    fn data_mut(&mut self) -> &mut PrimitiveData {
        &mut self.data
    }

    fn points(&self) -> Vec<(f64, f64)> {
        vec![(self.bar, self.price)]
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        if let Some(&(bar, price)) = points.first() {
            self.bar = bar;
            self.price = price;
        }
    }

    fn translate(&mut self, bar_delta: f64, price_delta: f64) {
        self.bar += bar_delta;
        self.price += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, bar: f64, price: f64) {
        match point_type {
            ControlPointType::Point1 | ControlPointType::Move => {
                self.bar = bar;
                self.price = price;
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
        let x1 = viewport.bar_to_x_f64(self.bar);
        let y = viewport.price_to_y(self.price, price_scale.price_min, price_scale.price_max);

        // Check starting point control point
        if check_point_hit(screen_x, screen_y, x1, y) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }

        // Check if on the horizontal ray (from bar to right edge)
        // Only consider points to the right of the starting bar
        if screen_x >= x1 - HIT_TOLERANCE {
            // Check vertical distance from the line
            if (screen_y - y).abs() < HIT_TOLERANCE {
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
        let x = viewport.bar_to_x_f64(self.bar);
        let y = viewport.price_to_y(self.price, price_scale.price_min, price_scale.price_max);

        vec![
            ControlPoint::point1(x, y),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x = ctx.bar_to_x(self.bar);
        let y = ctx.price_to_y(self.price);
        let crisp_y = crisp(y, dpr);

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

        // Draw horizontal ray from point to right edge
        ctx.begin_path();
        ctx.move_to(crisp(x, dpr), crisp_y);
        ctx.line_to(ctx.chart_width(), crisp_y);
        ctx.stroke();

        // Reset line dash
        ctx.set_line_dash(&[]);

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // For horizontal ray, position based on h_align along the line
                let min_x = x;
                let max_x = ctx.chart_width();
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x + 10.0,
                    super::super::TextAlign::Center => (min_x + max_x) / 2.0,
                    super::super::TextAlign::End => max_x - 10.0,
                };
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => y - text_offset,   // above
                    super::super::TextAlign::Center => y,                 // on line
                    super::super::TextAlign::End => y + text_offset,     // below
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }

        // Draw control point if selected
        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            ctx.begin_path();
            ctx.arc(x, y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
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

fn create_horizontal_ray(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar, price) = points.first().copied().unwrap_or((0.0, 0.0));
    Box::new(HorizontalRay::new(bar, price, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "horizontal_ray",
        display_name: "Horizontal Ray",
        kind: PrimitiveKind::Line,
        click_behavior: ClickBehavior::SingleClick,
        tooltip: "Draw a horizontal line extending to the right",
        icon: "horizontal_ray",
        default_color: "#4CAF50",
        factory: create_horizontal_ray,
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
