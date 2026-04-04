//! Rectangle primitive
//!
//! A rectangular box defined by two corner points (drag to create).

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType, ControlPointCursor,
    PrimitiveColor, LineStyle,
    point_to_line_distance, HIT_TOLERANCE, CONTROL_POINT_HIT_RADIUS, CONTROL_POINT_RADIUS,
    RenderContext, crisp, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

/// Rectangle - box defined by two corners
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rectangle {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Top-left corner bar index
    pub bar1: f64,
    /// Top-left corner price
    pub price1: f64,
    /// Bottom-right corner bar index
    pub bar2: f64,
    /// Bottom-right corner price
    pub price2: f64,
    /// Fill the rectangle
    #[serde(default = "default_true")]
    pub fill: bool,
    /// Fill opacity (0.0 - 1.0)
    #[serde(default = "default_fill_opacity")]
    pub fill_opacity: f64,
    /// Border radius for rounded corners (0 = sharp)
    #[serde(default)]
    pub border_radius: f64,
}

fn default_true() -> bool {
    true
}

fn default_fill_opacity() -> f64 {
    0.2
}

impl Rectangle {
    /// Create a new rectangle
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "rectangle".to_string(),
                display_name: "Rectangle".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            bar1,
            price1,
            bar2,
            price2,
            fill: true,
            fill_opacity: 0.2,
            border_radius: 0.0,
        }
    }

    /// Get normalized corners (min/max)
    pub fn normalized(&self) -> (f64, f64, f64, f64) {
        let min_bar = self.bar1.min(self.bar2);
        let max_bar = self.bar1.max(self.bar2);
        let min_price = self.price1.min(self.price2);
        let max_price = self.price1.max(self.price2);
        (min_bar, min_price, max_bar, max_price)
    }

    /// Get center point
    pub fn center(&self) -> (f64, f64) {
        ((self.bar1 + self.bar2) / 2.0, (self.price1 + self.price2) / 2.0)
    }

    /// Get width in bars
    pub fn width_bars(&self) -> f64 {
        (self.bar2 - self.bar1).abs()
    }

    /// Get height in price
    pub fn height_price(&self) -> f64 {
        (self.price2 - self.price1).abs()
    }
}

impl Primitive for Rectangle {
    fn type_id(&self) -> &'static str {
        "rectangle"
    }

    fn display_name(&self) -> &str {
        &self.data.display_name
    }

    fn kind(&self) -> PrimitiveKind {
        PrimitiveKind::Shape
    }

    fn click_behavior(&self) -> ClickBehavior {
        ClickBehavior::ClickDrag
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
            ControlPointType::Corner(0) => {
                // Top-left
                self.bar1 = bar;
                self.price1 = price;
            }
            ControlPointType::Corner(1) => {
                // Top-right
                self.bar2 = bar;
                self.price1 = price;
            }
            ControlPointType::Corner(2) => {
                // Bottom-right
                self.bar2 = bar;
                self.price2 = price;
            }
            ControlPointType::Corner(3) => {
                // Bottom-left
                self.bar1 = bar;
                self.price2 = price;
            }
            ControlPointType::Edge(0) => {
                // Top edge - adjust price1
                self.price1 = price;
            }
            ControlPointType::Edge(1) => {
                // Right edge - adjust bar2
                self.bar2 = bar;
            }
            ControlPointType::Edge(2) => {
                // Bottom edge - adjust price2
                self.price2 = price;
            }
            ControlPointType::Edge(3) => {
                // Left edge - adjust bar1
                self.bar1 = bar;
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

        let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
        let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };

        // Check corner control points (4 corners)
        let corners = [
            (x1, y1, ControlPointType::Corner(0)), // TL
            (x2, y1, ControlPointType::Corner(1)), // TR
            (x2, y2, ControlPointType::Corner(2)), // BR
            (x1, y2, ControlPointType::Corner(3)), // BL
        ];
        for (cx, cy, cp_type) in corners {
            if check_point_hit(screen_x, screen_y, cx, cy) {
                return HitTestResult::ControlPoint(cp_type);
            }
        }

        // Check edge midpoint control points
        let edges = [
            ((x1 + x2) / 2.0, y1, ControlPointType::Edge(0)), // Top
            (x2, (y1 + y2) / 2.0, ControlPointType::Edge(1)), // Right
            ((x1 + x2) / 2.0, y2, ControlPointType::Edge(2)), // Bottom
            (x1, (y1 + y2) / 2.0, ControlPointType::Edge(3)), // Left
        ];
        for (ex, ey, cp_type) in edges {
            if check_point_hit(screen_x, screen_y, ex, ey) {
                return HitTestResult::ControlPoint(cp_type);
            }
        }

        // Check move point at center
        let cx = (x1 + x2) / 2.0;
        let cy = (y1 + y2) / 2.0;
        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }

        // Check if on edges (border)
        let edges_lines = [
            (x1, y1, x2, y1), // Top
            (x2, y1, x2, y2), // Right
            (x2, y2, x1, y2), // Bottom
            (x1, y2, x1, y1), // Left
        ];
        for (lx1, ly1, lx2, ly2) in edges_lines {
            if point_to_line_distance(screen_x, screen_y, lx1, ly1, lx2, ly2) < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }

        // Check if inside filled rectangle
        if self.fill && screen_x >= min_x && screen_x <= max_x && screen_y >= min_y && screen_y <= max_y {
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

        vec![
            // Corners with diagonal resize cursors
            ControlPoint::new(ControlPointType::Corner(0), x1, y1, ControlPointCursor::ResizeNWSE),
            ControlPoint::new(ControlPointType::Corner(1), x2, y1, ControlPointCursor::ResizeNESW),
            ControlPoint::new(ControlPointType::Corner(2), x2, y2, ControlPointCursor::ResizeNWSE),
            ControlPoint::new(ControlPointType::Corner(3), x1, y2, ControlPointCursor::ResizeNESW),
            // Edge midpoints
            ControlPoint::new(ControlPointType::Edge(0), (x1 + x2) / 2.0, y1, ControlPointCursor::ResizeNS),
            ControlPoint::new(ControlPointType::Edge(1), x2, (y1 + y2) / 2.0, ControlPointCursor::ResizeEW),
            ControlPoint::new(ControlPointType::Edge(2), (x1 + x2) / 2.0, y2, ControlPointCursor::ResizeNS),
            ControlPoint::new(ControlPointType::Edge(3), x1, (y1 + y2) / 2.0, ControlPointCursor::ResizeEW),
            // Center move point
            ControlPoint::move_point((x1 + x2) / 2.0, (y1 + y2) / 2.0),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();

        // Convert to screen coordinates
        let x1 = ctx.bar_to_x(self.bar1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.bar_to_x(self.bar2);
        let y2 = ctx.price_to_y(self.price2);

        let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
        let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };
        let width = max_x - min_x;
        let height = max_y - min_y;

        // Fill if enabled
        if self.fill {
            let fill_color = format!("{}{}",
                &self.data.color.stroke[..7],
                format!("{:02x}", (self.fill_opacity * 255.0) as u8)
            );
            ctx.set_fill_color(&fill_color);
            ctx.fill_rect(min_x, min_y, width, height);
        }

        // Set stroke style
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);

        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Draw rectangle border
        ctx.stroke_rect(crisp(min_x, dpr), crisp(min_y, dpr), width, height);
        ctx.set_line_dash(&[]);

        // Draw control points if selected
        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            let corners = [(x1, y1), (x2, y1), (x2, y2), (x1, y2)];
            for (cx, cy) in corners {
                ctx.begin_path();
                ctx.arc(cx, cy, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }

            let edges = [((x1 + x2) / 2.0, y1), (x2, (y1 + y2) / 2.0), ((x1 + x2) / 2.0, y2), (x1, (y1 + y2) / 2.0)];
            for (ex, ey) in edges {
                ctx.begin_path();
                ctx.arc(ex, ey, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }

            let cx = (x1 + x2) / 2.0;
            let cy = (y1 + y2) / 2.0;
            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                // Calculate X based on h_align
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => (x1 + x2) / 2.0,
                    super::super::TextAlign::End => max_x,
                };
                // Calculate Y based on v_align:
                // Start = ABOVE upper boundary, Center = middle, End = BELOW lower boundary
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => min_y - text_offset,
                    super::super::TextAlign::Center => (y1 + y2) / 2.0,
                    super::super::TextAlign::End => max_y + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
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

fn create_rectangle(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar1, price1) = points.first().copied().unwrap_or((0.0, 0.0));
    let (bar2, price2) = points.get(1).copied().unwrap_or((bar1 + 10.0, price1 * 1.05));
    Box::new(Rectangle::new(bar1, price1, bar2, price2, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "rectangle",
        display_name: "Rectangle",
        kind: PrimitiveKind::Shape,
        click_behavior: ClickBehavior::ClickDrag,
        tooltip: "Draw a rectangle by dragging",
        icon: "rectangle",
        default_color: "#2196F3",
        factory: create_rectangle,
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
