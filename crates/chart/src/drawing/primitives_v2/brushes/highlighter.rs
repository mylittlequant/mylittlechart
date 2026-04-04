//! Highlighter - semi-transparent highlight

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport, apply_opacity};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Highlighter {
    pub data: PrimitiveData,
    pub points: Vec<(f64, f64)>,
    #[serde(default = "default_size")] pub brush_size: f64,
    #[serde(default = "default_opacity")] pub opacity: f64,
}
fn default_size() -> f64 { 20.0 }
fn default_opacity() -> f64 { 0.4 }

impl Highlighter {
    pub fn new(points: Vec<(f64, f64)>, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "highlighter".to_string(), display_name: "Highlighter".to_string(), color: PrimitiveColor::new(color), width: 20.0, ..Default::default() },
            points, brush_size: 20.0, opacity: 0.4,
        }
    }
}

impl Primitive for Highlighter {
    fn type_id(&self) -> &'static str { "highlighter" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Annotation }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::FreehandDrag }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(f64, f64)> { self.points.clone() }
    fn set_points(&mut self, pts: &[(f64, f64)]) { self.points = pts.to_vec(); }
    fn translate(&mut self, bd: f64, pd: f64) { for p in &mut self.points { p.0 += bd; p.1 += pd; } }
    fn move_control_point(&mut self, pt: ControlPointType, bar: f64, price: f64) {
        if let ControlPointType::Move = pt {
            if let Some(first) = self.points.first() {
                let bd = bar - first.0;
                let pd = price - first.1;
                self.translate(bd, pd);
            }
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let screen: Vec<_> = self.points.iter().map(|(b, p)| (vp.bar_to_x_f64(*b), vp.price_to_y(*p, ps.price_min, ps.price_max))).collect();
        for i in 0..screen.len().saturating_sub(1) {
            if point_to_line_dist(sx, sy, screen[i].0, screen[i].1, screen[i+1].0, screen[i+1].1) < HIT_TOLERANCE + self.brush_size {
                return HitTestResult::Body;
            }
        }
        HitTestResult::Miss
    }
    fn control_points(&self, _vp: &Viewport, _ps: &PriceScale) -> Vec<ControlPoint> { vec![] }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        if self.points.is_empty() {
            return;
        }

        let _dpr = ctx.dpr();

        // Parse the color and apply opacity
        let color_with_opacity = apply_opacity(&self.data.color.stroke, self.opacity);

        ctx.set_stroke_color(&color_with_opacity);
        ctx.set_stroke_width(self.data.width);
        ctx.set_line_cap("round");
        ctx.set_line_join("round");

        // Convert to screen coordinates
        let screen_pts: Vec<(f64, f64)> = self.points.iter()
            .map(|&(bar, price)| (ctx.bar_to_x(bar), ctx.price_to_y(price)))
            .collect();

        // Draw smooth curve using quadratic bezier interpolation
        ctx.begin_path();
        if screen_pts.len() == 1 {
            // Single point - just draw a dot
            let (x, y) = screen_pts[0];
            ctx.arc(x, y, self.data.width / 2.0, 0.0, std::f64::consts::TAU);
            ctx.fill();
            return;
        } else if screen_pts.len() == 2 {
            // Two points - draw a line
            ctx.move_to(screen_pts[0].0, screen_pts[0].1);
            ctx.line_to(screen_pts[1].0, screen_pts[1].1);
        } else {
            // 3+ points - use quadratic bezier through midpoints for smooth curves
            ctx.move_to(screen_pts[0].0, screen_pts[0].1);

            // First segment: line to midpoint of first two points
            let mid_x = (screen_pts[0].0 + screen_pts[1].0) / 2.0;
            let mid_y = (screen_pts[0].1 + screen_pts[1].1) / 2.0;
            ctx.line_to(mid_x, mid_y);

            // Middle segments: quadratic curves through points, ending at midpoints
            for i in 1..screen_pts.len() - 1 {
                let next_mid_x = (screen_pts[i].0 + screen_pts[i + 1].0) / 2.0;
                let next_mid_y = (screen_pts[i].1 + screen_pts[i + 1].1) / 2.0;
                ctx.quadratic_curve_to(screen_pts[i].0, screen_pts[i].1, next_mid_x, next_mid_y);
            }

            // Last segment: line to final point
            let last = screen_pts.last().unwrap();
            ctx.line_to(last.0, last.1);
        }
        ctx.stroke();

        if is_selected {
            // Draw control points at first and last point
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for &(bar, price) in [self.points.first(), self.points.last()].iter().filter_map(|p| *p) {
                let x = ctx.bar_to_x(bar);
                let y = ctx.price_to_y(price);
                ctx.begin_path();
                ctx.arc(x, y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

fn point_to_line_dist(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 { return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt(); }
    let t = (((px - x1) * dx + (py - y1) * dy) / len_sq).clamp(0.0, 1.0);
    ((px - (x1 + t * dx)).powi(2) + (py - (y1 + t * dy)).powi(2)).sqrt()
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "highlighter", display_name: "Highlighter", kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::FreehandDrag, tooltip: "Semi-transparent highlight", icon: "highlighter", default_color: "#FFEB3B",
        factory: |points, color| Box::new(Highlighter::new(points.to_vec(), color)),
        supports_text: false,
        has_levels: false,
        has_points_config: false,
    }
}
