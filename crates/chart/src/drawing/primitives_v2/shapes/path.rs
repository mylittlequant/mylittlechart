//! Path primitive
//!
//! A freeform path that can contain straight and curved segments.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    point_to_line_distance, HIT_TOLERANCE, CONTROL_POINT_HIT_RADIUS, CONTROL_POINT_RADIUS,
    RenderContext, crisp, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

/// Path - freeform drawing path
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Path {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Path points as (bar, price) pairs
    pub points_data: Vec<(f64, f64)>,
    /// Smooth the path (bezier interpolation)
    #[serde(default)]
    pub smooth: bool,
    /// Close the path
    #[serde(default)]
    pub closed: bool,
}

impl Path {
    /// Create a new path
    pub fn new(points: Vec<(f64, f64)>, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "path".to_string(),
                display_name: "Path".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            points_data: points,
            smooth: false,
            closed: false,
        }
    }

    /// Add a point
    pub fn add_point(&mut self, bar: f64, price: f64) {
        self.points_data.push((bar, price));
    }

    /// Get center point
    pub fn center(&self) -> (f64, f64) {
        if self.points_data.is_empty() {
            return (0.0, 0.0);
        }
        let sum: (f64, f64) = self.points_data.iter().fold((0.0, 0.0), |acc, p| (acc.0 + p.0, acc.1 + p.1));
        let n = self.points_data.len() as f64;
        (sum.0 / n, sum.1 / n)
    }
}

impl Primitive for Path {
    fn type_id(&self) -> &'static str {
        "path"
    }

    fn display_name(&self) -> &str {
        &self.data.display_name
    }

    fn kind(&self) -> PrimitiveKind {
        PrimitiveKind::Shape
    }

    fn click_behavior(&self) -> ClickBehavior {
        ClickBehavior::MultiPoint(2)
    }

    fn data(&self) -> &PrimitiveData {
        &self.data
    }

    fn data_mut(&mut self) -> &mut PrimitiveData {
        &mut self.data
    }

    fn points(&self) -> Vec<(f64, f64)> {
        self.points_data.clone()
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        self.points_data = points.to_vec();
    }

    fn translate(&mut self, bar_delta: f64, price_delta: f64) {
        for p in &mut self.points_data {
            p.0 += bar_delta;
            p.1 += price_delta;
        }
    }

    fn move_control_point(&mut self, point_type: ControlPointType, bar: f64, price: f64) {
        match point_type {
            ControlPointType::Index(i) => {
                if let Some(p) = self.points_data.get_mut(i as usize) {
                    p.0 = bar;
                    p.1 = price;
                }
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
        if self.points_data.is_empty() {
            return HitTestResult::Miss;
        }

        let screen_points: Vec<(f64, f64)> = self.points_data.iter()
            .map(|(b, p)| {
                (viewport.bar_to_x_f64(*b), viewport.price_to_y(*p, price_scale.price_min, price_scale.price_max))
            })
            .collect();

        // Check control points
        for (i, (sx, sy)) in screen_points.iter().enumerate() {
            if check_point_hit(screen_x, screen_y, *sx, *sy) {
                return HitTestResult::ControlPoint(ControlPointType::Index(i as u8));
            }
        }

        // Check center/move point
        let center = self.center();
        let cx = viewport.bar_to_x_f64(center.0);
        let cy = viewport.price_to_y(center.1, price_scale.price_min, price_scale.price_max);
        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }

        // Check line segments
        for i in 0..screen_points.len().saturating_sub(1) {
            let (x1, y1) = screen_points[i];
            let (x2, y2) = screen_points[i + 1];
            if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }

        // Check closing segment if closed
        if self.closed && screen_points.len() > 2 {
            let (x1, y1) = screen_points[screen_points.len() - 1];
            let (x2, y2) = screen_points[0];
            if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
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
        let mut cps: Vec<ControlPoint> = self.points_data.iter()
            .enumerate()
            .map(|(i, (b, p))| {
                let x = viewport.bar_to_x_f64(*b);
                let y = viewport.price_to_y(*p, price_scale.price_min, price_scale.price_max);
                ControlPoint::with_type(ControlPointType::Index(i as u8), x, y)
            })
            .collect();

        // Add center move point
        if !self.points_data.is_empty() {
            let center = self.center();
            let cx = viewport.bar_to_x_f64(center.0);
            let cy = viewport.price_to_y(center.1, price_scale.price_min, price_scale.price_max);
            cps.push(ControlPoint::move_point(cx, cy));
        }

        cps
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        if self.points_data.len() < 2 {
            return;
        }

        let dpr = ctx.dpr();
        let screen_points: Vec<(f64, f64)> = self.points_data.iter()
            .map(|(b, p)| (ctx.bar_to_x(*b), ctx.price_to_y(*p)))
            .collect();

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
        if self.smooth && screen_points.len() >= 3 {
            // Smooth path using quadratic curves through points
            ctx.move_to(screen_points[0].0, screen_points[0].1);
            for i in 1..screen_points.len() - 1 {
                let (x0, y0) = screen_points[i - 1];
                let (x1, y1) = screen_points[i];
                let (x2, y2) = screen_points[i + 1];
                let cp_x = x1;
                let cp_y = y1;
                let end_x = (x1 + x2) / 2.0;
                let end_y = (y1 + y2) / 2.0;
                if i == 1 {
                    ctx.line_to((x0 + x1) / 2.0, (y0 + y1) / 2.0);
                }
                ctx.quadratic_curve_to(cp_x, cp_y, end_x, end_y);
            }
            let last = screen_points.last().unwrap();
            ctx.line_to(last.0, last.1);
        } else {
            // Straight lines
            ctx.move_to(crisp(screen_points[0].0, dpr), crisp(screen_points[0].1, dpr));
            for (x, y) in screen_points.iter().skip(1) {
                ctx.line_to(crisp(*x, dpr), crisp(*y, dpr));
            }
        }
        if self.closed {
            ctx.close_path();
        }
        ctx.stroke();
        ctx.set_line_dash(&[]);

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            for (x, y) in &screen_points {
                ctx.begin_path();
                ctx.arc(*x, *y, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }

            // Center move point
            let center = self.center();
            let cx = ctx.bar_to_x(center.0);
            let cy = ctx.price_to_y(center.1);
            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                // Calculate bounding box from screen points
                let min_x = screen_points.iter().map(|(x, _)| *x).fold(f64::INFINITY, f64::min);
                let max_x = screen_points.iter().map(|(x, _)| *x).fold(f64::NEG_INFINITY, f64::max);
                let min_y = screen_points.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
                let max_y = screen_points.iter().map(|(_, y)| *y).fold(f64::NEG_INFINITY, f64::max);
                // Calculate centroid for center position
                let (sum_x, sum_y) = screen_points.iter().fold((0.0, 0.0), |(sx, sy), (x, y)| (sx + x, sy + y));
                let n = screen_points.len() as f64;
                let (cx, cy) = (sum_x / n, sum_y / n);
                // Calculate X based on h_align
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => cx,
                    super::super::TextAlign::End => max_x,
                };
                // Calculate Y based on v_align:
                // Start = ABOVE upper boundary, Center = middle, End = BELOW lower boundary
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => min_y - text_offset,
                    super::super::TextAlign::Center => cy,
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
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2) as f64
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_path(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    Box::new(Path::new(points.to_vec(), color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "path",
        display_name: "Path",
        kind: PrimitiveKind::Shape,
        click_behavior: ClickBehavior::MultiPoint(2),
        tooltip: "Draw a freeform path (double-click to finish)",
        icon: "path",
        default_color: "#009688",
        factory: create_path,
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
