//! Vertical Line primitive
//!
//! A vertical line at a specific bar/time.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType, ControlPointCursor,
    PrimitiveColor, LineStyle, HIT_TOLERANCE,
    RenderContext, crisp, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text_rotated,
};

/// Vertical Line at a bar index
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerticalLine {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Bar index (f64 for sub-bar precision)
    pub bar_idx: f64,
    /// Show time label on scale
    #[serde(default = "default_true")]
    pub show_time_label: bool,
}

fn default_true() -> bool { true }

impl VerticalLine {
    /// Create a new vertical line
    pub fn new(bar_idx: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "vertical_line".to_string(),
                display_name: "Vertical Line".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            bar_idx,
            show_time_label: true,
        }
    }

    /// Create from integer bar index
    pub fn from_bar(bar_idx: usize, color: &str) -> Self {
        Self::new(bar_idx as f64, color)
    }
}

impl Primitive for VerticalLine {
    fn type_id(&self) -> &'static str {
        "vertical_line"
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
        // Return (bar, 0) - price doesn't matter for vertical line
        vec![(self.bar_idx, 0.0)]
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        if let Some((bar, _)) = points.first() {
            self.bar_idx = *bar;
        }
    }

    fn translate(&mut self, bar_delta: f64, _price_delta: f64) {
        self.bar_idx += bar_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, bar: f64, _price: f64) {
        if matches!(point_type, ControlPointType::Move) {
            self.bar_idx = bar;
        }
    }

    fn hit_test(
        &self,
        screen_x: f64,
        _screen_y: f64,
        viewport: &Viewport,
        _price_scale: &PriceScale,
    ) -> HitTestResult {
        let line_x = viewport.bar_to_x_f64(self.bar_idx);

        if (screen_x - line_x).abs() < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        HitTestResult::Miss
    }

    fn control_points(
        &self,
        viewport: &Viewport,
        _price_scale: &PriceScale,
    ) -> Vec<ControlPoint> {
        let x = viewport.bar_to_x_f64(self.bar_idx);
        // Position control point at chart center Y
        let y = viewport.chart_height / 2.0;

        vec![ControlPoint::new(
            ControlPointType::Move,
            x,
            y,
            ControlPointCursor::ResizeEW,
        )]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x = ctx.bar_to_x(self.bar_idx);
        let crisp_x = crisp(x, dpr);

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

        // Draw vertical line across entire chart
        ctx.begin_path();
        ctx.move_to(crisp_x, 0.0);
        ctx.line_to(crisp_x, ctx.chart_height());
        ctx.stroke();

        // Reset line dash
        ctx.set_line_dash(&[]);

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // For vertical line, position based on v_align along the line
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => text.font_size / 2.0 + 4.0,
                    super::super::TextAlign::Center => ctx.chart_height() / 2.0,
                    super::super::TextAlign::End => ctx.chart_height() - text.font_size / 2.0 - 4.0,
                };
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => x - text_offset,   // left
                    super::super::TextAlign::Center => x,                 // on line
                    super::super::TextAlign::End => x + text_offset,     // right
                };
                render_primitive_text_rotated(ctx, text, text_x, text_y, &self.data.color.stroke, -std::f64::consts::FRAC_PI_2);
            }
        }

        // Draw control point if selected
        if is_selected {
            let cy = ctx.chart_height() / 2.0;
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            ctx.begin_path();
            ctx.arc(x, cy, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
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

// =============================================================================
// Factory Registration
// =============================================================================

fn create_vertical_line(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let bar_idx = points.first().map(|(b, _)| *b).unwrap_or(0.0);
    Box::new(VerticalLine::new(bar_idx, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "vertical_line",
        display_name: "Vertical Line",
        kind: PrimitiveKind::Line,
        click_behavior: ClickBehavior::SingleClick,
        tooltip: "Draw a vertical line at a bar/time",
        icon: "vertical_line",
        default_color: "#9C27B0",
        factory: create_vertical_line,
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
