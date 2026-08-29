//! Vertical Line primitive
//!
//! A vertical line at a specific timestamp.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::chart::types::time_scale::format_time_full;
use crate::i18n::{ConfigKey, current_language};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType, ControlPointCursor,
    PrimitiveColor, LineStyle, HIT_TOLERANCE, CONTROL_POINT_HIT_RADIUS, draw_label_plate,
    RenderContext, crisp,
    render_primitive_text_rotated,
    config::{ConfigProperty, PropertyValue, LabelStyle},
};

/// Vertical Line at a timestamp
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerticalLine {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Timestamp in Unix milliseconds
    pub ts_ms: i64,
    /// Show time label on scale
    #[serde(default = "default_true")]
    pub show_time_label: bool,
}

fn default_true() -> bool { true }

impl VerticalLine {
    /// Create a new vertical line
    pub fn new(ts_ms: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "vertical_line".to_string(),
                display_name: "Vertical Line".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            ts_ms,
            show_time_label: true,
        }
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

    fn points(&self) -> Vec<(i64, f64)> {
        vec![(self.ts_ms, 0.0)]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if let Some((ts, _)) = points.first() {
            self.ts_ms = *ts;
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, _price_delta: f64) {
        self.ts_ms += ts_delta_ms;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, _price: f64) {
        if matches!(point_type, ControlPointType::Move) {
            self.ts_ms = ts_ms;
        }
    }

    fn hit_test(
        &self,
        screen_x: f64,
        screen_y: f64,
        bars: &[Bar],
        viewport: &Viewport,
        _price_scale: &PriceScale,
    ) -> HitTestResult {
        let bar = timestamp_ms_to_bar_f64(bars, self.ts_ms);
        let line_x = viewport.bar_to_x_f64(bar);
        let move_y = viewport.chart_height / 2.0;

        // The Move handle (below in `control_points()`) sits ON the line
        // itself, at its midpoint, so it must be tested BEFORE the body's
        // whole-line check — otherwise the body test always wins first and
        // the handle can never be grabbed, only the line under it.
        if check_point_hit(screen_x, screen_y, line_x, move_y) {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }

        if (screen_x - line_x).abs() < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        HitTestResult::Miss
    }

    fn control_points(
        &self,
        bars: &[Bar],
        viewport: &Viewport,
        _price_scale: &PriceScale,
    ) -> Vec<ControlPoint> {
        let bar = timestamp_ms_to_bar_f64(bars, self.ts_ms);
        let x = viewport.bar_to_x_f64(bar);
        let y = viewport.chart_height / 2.0;

        vec![ControlPoint::new(
            ControlPointType::Move,
            x,
            y,
            ControlPointCursor::ResizeEW,
        )]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let x = ctx.ts_to_x_ms(self.ts_ms);
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

        // `show_time_label` used to be a struct field with no reader
        // anywhere — toggling it changed nothing. `ts_ms` is milliseconds,
        // `format_time_full` takes SECONDS, matching `Bar::timestamp`'s unit.
        if self.show_time_label {
            let label_style = LabelStyle::default();
            ctx.set_font(&label_style.font_string());
            ctx.set_text_align(crate::render::TextAlign::Center);
            ctx.set_text_baseline(crate::render::TextBaseline::Middle);
            let label = format_time_full(self.ts_ms / 1000);
            draw_label_plate(ctx, x, 16.0, &label, label_style.font_size, &label_style, &self.data.color.stroke);
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => text.font_size / 2.0 + 4.0,
                    super::super::TextAlign::Center => ctx.chart_height() / 2.0,
                    super::super::TextAlign::End => ctx.chart_height() - text.font_size / 2.0 - 4.0,
                };
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => x - text_offset,
                    super::super::TextAlign::Center => x,
                    super::super::TextAlign::End => x + text_offset,
                };
                render_primitive_text_rotated(ctx, text, text_x, text_y, &self.data.color.stroke, -std::f64::consts::FRAC_PI_2);
            }
        }

        // Draw control point if selected
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    fn clone_box(&self) -> Box<dyn Primitive> {
        Box::new(self.clone())
    }
    fn unbounded_in_price(&self) -> bool { true }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        vec![ConfigProperty::boolean("show_time_label", ConfigKey::ShowTimeLabel.get(current_language()), self.show_time_label).with_order(20)]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        if id == "show_time_label" {
            if let Some(v) = value.as_bool() { self.show_time_label = v; return true; }
        }
        false
    }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    (sx - px).powi(2) + (sy - py).powi(2) <= CONTROL_POINT_HIT_RADIUS.powi(2)
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_vertical_line(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let ts_ms = points.first().map(|(t, _)| *t).unwrap_or(0);
    Box::new(VerticalLine::new(ts_ms, color))
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
    }
}
