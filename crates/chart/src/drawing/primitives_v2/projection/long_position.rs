//! Long Position - buy trade visualization

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LongPosition {
    pub data: PrimitiveData,
    pub bar: f64,
    pub entry_price: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
    #[serde(default)] pub quantity: f64,
    #[serde(default = "default_true")] pub show_pnl: bool,
}
fn default_true() -> bool { true }

impl LongPosition {
    pub fn new(bar: f64, entry: f64, stop: f64, target: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "long_position".to_string(), display_name: "Long Position".to_string(), color: PrimitiveColor::new(color), width: 1.0, ..Default::default() },
            bar, entry_price: entry, stop_loss: stop, take_profit: target, quantity: 1.0, show_pnl: true,
        }
    }
    pub fn risk_reward(&self) -> f64 {
        let risk = (self.entry_price - self.stop_loss).abs();
        let reward = (self.take_profit - self.entry_price).abs();
        if risk > 0.0 { reward / risk } else { 0.0 }
    }
}

impl Primitive for LongPosition {
    fn type_id(&self) -> &'static str { "long_position" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Trading }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::ThreePoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(f64, f64)> { vec![(self.bar, self.entry_price), (self.bar, self.stop_loss), (self.bar, self.take_profit)] }
    fn set_points(&mut self, pts: &[(f64, f64)]) {
        if let Some(&(b, p)) = pts.first() { self.bar = b; self.entry_price = p; }
        if let Some(&(_, p)) = pts.get(1) { self.stop_loss = p; }
        if let Some(&(_, p)) = pts.get(2) { self.take_profit = p; }
    }
    fn translate(&mut self, bd: f64, pd: f64) { self.bar += bd; self.entry_price += pd; self.stop_loss += pd; self.take_profit += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, bar: f64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.bar = bar; self.entry_price = price; }
            ControlPointType::Point2 => self.stop_loss = price,
            ControlPointType::Point3 => self.take_profit = price,
            ControlPointType::Move => { let bd = bar - self.bar; let pd = price - self.entry_price; self.translate(bd, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let x = vp.bar_to_x_f64(self.bar);
        let ye = vp.price_to_y(self.entry_price, ps.price_min, ps.price_max);
        let ys = vp.price_to_y(self.stop_loss, ps.price_min, ps.price_max);
        let yt = vp.price_to_y(self.take_profit, ps.price_min, ps.price_max);
        let r = 8.0;
        if (sx - x).powi(2) + (sy - ye).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x).powi(2) + (sy - ys).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        if (sx - x).powi(2) + (sy - yt).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point3); }
        // Check horizontal lines
        let w = 100.0;
        if (sy - ye).abs() < HIT_TOLERANCE && sx >= x && sx <= x + w { return HitTestResult::Body; }
        if (sy - ys).abs() < HIT_TOLERANCE && sx >= x && sx <= x + w { return HitTestResult::Body; }
        if (sy - yt).abs() < HIT_TOLERANCE && sx >= x && sx <= x + w { return HitTestResult::Body; }
        HitTestResult::Miss
    }
    fn control_points(&self, vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let x = vp.bar_to_x_f64(self.bar);
        vec![
            ControlPoint::point1(x, vp.price_to_y(self.entry_price, ps.price_min, ps.price_max)),
            ControlPoint::point2(x, vp.price_to_y(self.stop_loss, ps.price_min, ps.price_max)),
            ControlPoint::point3(x, vp.price_to_y(self.take_profit, ps.price_min, ps.price_max)),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.bar_to_x(self.bar);
        let entry_y = ctx.price_to_y(self.entry_price);
        let stop_y = ctx.price_to_y(self.stop_loss);
        let target_y = ctx.price_to_y(self.take_profit);
        let chart_width = ctx.chart_width();

        // Draw stop loss zone (red fill)
        ctx.set_fill_color("#FF000030");
        ctx.fill_rect(crisp(x1, dpr), stop_y.min(entry_y), chart_width - x1, (stop_y - entry_y).abs());

        // Draw take profit zone (green fill)
        ctx.set_fill_color("#00FF0030");
        ctx.fill_rect(crisp(x1, dpr), target_y.min(entry_y), chart_width - x1, (target_y - entry_y).abs());

        // Set line dash based on style
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Draw entry line (white)
        ctx.set_stroke_width(self.data.width);
        ctx.set_stroke_color("#FFFFFF");
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(entry_y, dpr));
        ctx.line_to(crisp(chart_width, dpr), crisp(entry_y, dpr));
        ctx.stroke();

        // Draw stop loss line (red)
        ctx.set_stroke_color("#FF0000");
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(stop_y, dpr));
        ctx.line_to(crisp(chart_width, dpr), crisp(stop_y, dpr));
        ctx.stroke();

        // Draw take profit line (green)
        ctx.set_stroke_color("#00FF00");
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(target_y, dpr));
        ctx.line_to(crisp(chart_width, dpr), crisp(target_y, dpr));
        ctx.stroke();

        // Reset line dash
        ctx.set_line_dash(&[]);

        // Draw control points if selected
        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            // Entry point
            ctx.begin_path();
            ctx.arc(x1, entry_y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();

            // Stop loss point
            ctx.begin_path();
            ctx.arc(x1, stop_y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();

            // Take profit point
            ctx.begin_path();
            ctx.arc(x1, target_y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Bounding box from stop loss to take profit, x1 to chart_width
                let min_x = x1;
                let max_x = chart_width;
                let min_y = stop_y.min(target_y).min(entry_y);
                let max_y = stop_y.max(target_y).max(entry_y);

                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => (min_x + max_x) / 2.0,
                    super::super::TextAlign::End => max_x,
                };
                // Start = ABOVE upper boundary, Center = middle, End = BELOW lower boundary
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => min_y - text_offset,
                    super::super::TextAlign::Center => (min_y + max_y) / 2.0,
                    super::super::TextAlign::End => max_y + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "long_position", display_name: "Long Position", kind: PrimitiveKind::Trading,
        click_behavior: ClickBehavior::ThreePoint, tooltip: "Buy trade with stop/target", icon: "long_position", default_color: "#4CAF50",
        factory: |points, color| {
            let (b, entry) = points.first().copied().unwrap_or((0.0, 100.0));
            let (_, stop) = points.get(1).copied().unwrap_or((b, entry - 5.0));
            let (_, target) = points.get(2).copied().unwrap_or((b, entry + 10.0));
            Box::new(LongPosition::new(b, entry, stop, target, color))
        },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
