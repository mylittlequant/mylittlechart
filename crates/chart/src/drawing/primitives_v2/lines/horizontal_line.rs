//! Horizontal Line primitive
//!
//! A horizontal line at a specific price level.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType, ControlPointCursor,
    PrimitiveColor, LineStyle, TextAlign, HIT_TOLERANCE,
    RenderContext, crisp, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

/// Horizontal Line at a price level
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HorizontalLine {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Price level
    pub price: f64,
    /// Show price label on scale
    #[serde(default = "default_true")]
    pub show_price_label: bool,
}

fn default_true() -> bool { true }

impl HorizontalLine {
    /// Create a new horizontal line
    pub fn new(price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "horizontal_line".to_string(),
                display_name: "Horizontal Line".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            price,
            show_price_label: true,
        }
    }
}

impl Primitive for HorizontalLine {
    fn type_id(&self) -> &'static str {
        "horizontal_line"
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
        // Return (0, price) - bar doesn't matter for horizontal line
        vec![(0.0, self.price)]
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        if let Some((_, price)) = points.first() {
            self.price = *price;
        }
    }

    fn translate(&mut self, _bar_delta: f64, price_delta: f64) {
        self.price += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, _bar: f64, price: f64) {
        if matches!(point_type, ControlPointType::Move) {
            self.price = price;
        }
    }

    fn hit_test(
        &self,
        _screen_x: f64,
        screen_y: f64,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> HitTestResult {
        let line_y = viewport.price_to_y(self.price, price_scale.price_min, price_scale.price_max);

        if (screen_y - line_y).abs() < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        HitTestResult::Miss
    }

    fn control_points(
        &self,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Vec<ControlPoint> {
        let y = viewport.price_to_y(self.price, price_scale.price_min, price_scale.price_max);
        // Position control point at chart center X
        let x = viewport.chart_width / 2.0;

        vec![ControlPoint::new(
            ControlPointType::Move,
            x,
            y,
            ControlPointCursor::ResizeNS,
        )]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
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

        // Check if we need line gap (only for v_align == Center)
        let needs_gap = self.data.text.as_ref()
            .map(|t| !t.content.is_empty() && matches!(t.v_align, TextAlign::Center))
            .unwrap_or(false);

        // Calculate text position for gap
        let (text_x, text_width) = if needs_gap {
            let text = self.data.text.as_ref().unwrap();
            let x = match text.h_align {
                TextAlign::Start => 20.0,
                TextAlign::Center => ctx.chart_width() / 2.0,
                TextAlign::End => ctx.chart_width() - 20.0,
            };
            let char_count = text.content.len() as f64;
            let width = char_count * text.font_size * 0.6 + 16.0; // +padding
            (x, width)
        } else {
            (0.0, 0.0)
        };

        ctx.begin_path();

        if needs_gap {
            // Draw line with gap around text
            let gap_start = text_x - text_width / 2.0;
            let gap_end = text_x + text_width / 2.0;

            // Left segment
            if gap_start > 0.0 {
                ctx.move_to(0.0, crisp_y);
                ctx.line_to(crisp(gap_start, dpr), crisp_y);
            }

            // Right segment
            if gap_end < ctx.chart_width() {
                ctx.move_to(crisp(gap_end, dpr), crisp_y);
                ctx.line_to(ctx.chart_width(), crisp_y);
            }
        } else {
            // No gap - draw full horizontal line
            ctx.move_to(0.0, crisp_y);
            ctx.line_to(ctx.chart_width(), crisp_y);
        }

        ctx.stroke();

        // Reset line dash
        ctx.set_line_dash(&[]);

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // For horizontal line, position based on h_align along the line
                let text_x = match text.h_align {
                    TextAlign::Start => 20.0,
                    TextAlign::Center => ctx.chart_width() / 2.0,
                    TextAlign::End => ctx.chart_width() - 20.0,
                };
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_y = match text.v_align {
                    TextAlign::Start => y - text_offset,   // above
                    TextAlign::Center => y,                 // on line
                    TextAlign::End => y + text_offset,     // below
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }

        // Draw control point if selected
        if is_selected {
            let cx = ctx.chart_width() / 2.0;
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            ctx.begin_path();
            ctx.arc(cx, y, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
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

fn create_horizontal_line(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let price = points.first().map(|(_, p)| *p).unwrap_or(0.0);
    Box::new(HorizontalLine::new(price, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "horizontal_line",
        display_name: "Horizontal Line",
        kind: PrimitiveKind::Line,
        click_behavior: ClickBehavior::SingleClick,
        tooltip: "Draw a horizontal line at a price level",
        icon: "horizontal_line",
        default_color: "#FF9800",
        factory: create_horizontal_line,
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
