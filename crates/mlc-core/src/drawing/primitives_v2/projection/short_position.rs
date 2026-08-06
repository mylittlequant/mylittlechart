//! Short Position - sell trade visualization

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, LineStyle,
    render_primitive_text, fmt_price,
    config::{LabelStyle, ConfigProperty, PropertyValue, PropertyCategory},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShortPosition {
    pub data: PrimitiveData,
    #[serde(default)] pub ts_ms: i64,
    pub entry_price: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
    #[serde(default)] pub quantity: f64,
    #[serde(default = "default_true")] pub show_pnl: bool,
    #[serde(default = "default_stop_color")] pub stop_color: String,
    #[serde(default = "default_target_color")] pub target_color: String,
    #[serde(default = "default_zone_opacity")] pub zone_opacity: f64,
    #[serde(default)] pub label_style: LabelStyle,
}
fn default_true() -> bool { true }
fn default_stop_color() -> String { "#F23645".to_string() }
fn default_target_color() -> String { "#089981".to_string() }
fn default_zone_opacity() -> f64 { 0.15 }

/// Muted neutral — deliberately not the entry/stop/target colors, so the
/// live marker reads as subordinate to them rather than a fourth level.
const LIVE_MARKER_COLOR: &str = "#9598A1";

/// Format a signed price delta with an explicit `+`/`-` — `fmt_price` alone
/// has no sign, and a P&L or distance-from-entry reads wrong without one.
fn signed_price(v: f64) -> String {
    if v >= 0.0 { format!("+{}", fmt_price(v)) } else { format!("-{}", fmt_price(-v)) }
}

impl ShortPosition {
    pub fn new(ts_ms: i64, entry: f64, stop: f64, target: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "short_position".to_string(), display_name: "Short Position".to_string(), color: PrimitiveColor::new(color), width: 1.0, ..Default::default() },
            ts_ms, entry_price: entry, stop_loss: stop, take_profit: target, quantity: 1.0, show_pnl: true,
            stop_color: default_stop_color(), target_color: default_target_color(), zone_opacity: default_zone_opacity(),
            label_style: LabelStyle::default(),
        }
    }
    pub fn risk_reward(&self) -> f64 {
        let risk = (self.stop_loss - self.entry_price).abs();
        let reward = (self.entry_price - self.take_profit).abs();
        if risk > 0.0 { reward / risk } else { 0.0 }
    }
}

impl Primitive for ShortPosition {
    fn type_id(&self) -> &'static str { "short_position" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Trading }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::ThreePoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts_ms, self.entry_price), (self.ts_ms, self.stop_loss), (self.ts_ms, self.take_profit)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(t, p)) = pts.first() { self.ts_ms = t; self.entry_price = p; }
        if let Some(&(_, p)) = pts.get(1) { self.stop_loss = p; }
        if let Some(&(_, p)) = pts.get(2) { self.take_profit = p; }
    }
    fn translate(&mut self, td: i64, pd: f64) { self.ts_ms += td; self.entry_price += pd; self.stop_loss += pd; self.take_profit += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.ts_ms = ts_ms; self.entry_price = price; }
            ControlPointType::Point2 => self.stop_loss = price,
            ControlPointType::Point3 => self.take_profit = price,
            ControlPointType::Move => { let td = ts_ms - self.ts_ms; let pd = price - self.entry_price; self.translate(td, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let b = timestamp_ms_to_bar_f64(bars, self.ts_ms);
        let x = vp.bar_to_x_f64(b);
        let ye = vp.price_to_y(self.entry_price, ps.price_min, ps.price_max);
        let ys = vp.price_to_y(self.stop_loss, ps.price_min, ps.price_max);
        let yt = vp.price_to_y(self.take_profit, ps.price_min, ps.price_max);
        let r = 8.0;
        if (sx - x).powi(2) + (sy - ye).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x).powi(2) + (sy - ys).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        if (sx - x).powi(2) + (sy - yt).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point3); }
        let w = 100.0;
        if (sy - ye).abs() < HIT_TOLERANCE && sx >= x && sx <= x + w { return HitTestResult::Body; }
        if (sy - ys).abs() < HIT_TOLERANCE && sx >= x && sx <= x + w { return HitTestResult::Body; }
        if (sy - yt).abs() < HIT_TOLERANCE && sx >= x && sx <= x + w { return HitTestResult::Body; }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let b = timestamp_ms_to_bar_f64(bars, self.ts_ms);
        let x = vp.bar_to_x_f64(b);
        vec![
            ControlPoint::point1(x, vp.price_to_y(self.entry_price, ps.price_min, ps.price_max)),
            ControlPoint::point2(x, vp.price_to_y(self.stop_loss, ps.price_min, ps.price_max)),
            ControlPoint::point3(x, vp.price_to_y(self.take_profit, ps.price_min, ps.price_max)),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts_ms);
        let entry_y = ctx.price_to_y(self.entry_price);
        let stop_y = ctx.price_to_y(self.stop_loss);
        let target_y = ctx.price_to_y(self.take_profit);
        let chart_width = ctx.chart_width();
        let alpha = ((self.zone_opacity.clamp(0.0, 1.0)) * 255.0) as u8;

        // Draw stop loss zone (above entry for shorts), coloured by the
        // configurable stop-zone colour
        let stop_base = if self.stop_color.len() >= 7 { &self.stop_color[..7] } else { &self.stop_color[..] };
        ctx.set_fill_color(&format!("{}{:02x}", stop_base, alpha));
        ctx.fill_rect(crisp(x1, dpr), stop_y.min(entry_y), chart_width - x1, (stop_y - entry_y).abs());

        // Draw take profit zone (below entry for shorts), coloured by the
        // configurable target-zone colour
        let target_base = if self.target_color.len() >= 7 { &self.target_color[..7] } else { &self.target_color[..] };
        ctx.set_fill_color(&format!("{}{:02x}", target_base, alpha));
        ctx.fill_rect(crisp(x1, dpr), target_y.min(entry_y), chart_width - x1, (target_y - entry_y).abs());

        // Set line dash based on style
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Draw entry line (the primitive's own stroke color, themable)
        ctx.set_stroke_width(self.data.width);
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(entry_y, dpr));
        ctx.line_to(crisp(chart_width, dpr), crisp(entry_y, dpr));
        ctx.stroke();

        // Draw stop loss line
        ctx.set_stroke_color(&self.stop_color);
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(stop_y, dpr));
        ctx.line_to(crisp(chart_width, dpr), crisp(stop_y, dpr));
        ctx.stroke();

        // Draw take profit line
        ctx.set_stroke_color(&self.target_color);
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(target_y, dpr));
        ctx.line_to(crisp(chart_width, dpr), crisp(target_y, dpr));
        ctx.stroke();

        // Reset line dash
        ctx.set_line_dash(&[]);

        // Entry/stop/target price labels, R:R ratio, quantity and (optionally)
        // the resulting P&L — the whole point of a position tool, and
        // previously nowhere on screen at all.
        let risk_per_unit = (self.stop_loss - self.entry_price).abs();
        let reward_per_unit = (self.entry_price - self.take_profit).abs();
        let qty_suffix = if self.quantity > 0.0 { format!("  x{}", fmt_price(self.quantity)) } else { String::new() };

        let entry_text = format!("Entry {}{}", fmt_price(self.entry_price), qty_suffix);
        draw_line_label(ctx, x1 + 8.0, entry_y, &entry_text, &self.label_style, &self.data.color.stroke);

        let mut stop_text = format!("Stop {}", fmt_price(self.stop_loss));
        if self.show_pnl {
            stop_text.push_str(&format!("  -{}", fmt_price(risk_per_unit * self.quantity)));
        }
        draw_line_label(ctx, x1 + 8.0, stop_y, &stop_text, &self.label_style, &self.stop_color);

        let mut target_text = format!("Target {}  R:R {:.2}", fmt_price(self.take_profit), self.risk_reward());
        if self.show_pnl {
            target_text.push_str(&format!("  +{}", fmt_price(reward_per_unit * self.quantity)));
        }
        draw_line_label(ctx, x1 + 8.0, target_y, &target_text, &self.label_style, &self.target_color);

        // Live price marker — a thin, visually subordinate line at wherever
        // the current price actually is, tracking the position's running
        // distance/P&L from entry. Reads `ctx.bars()` the same way the
        // chart's own last-price line does (`draw_last_price_line` in
        // mlc-app-layout), so this moves every frame on its own — nothing
        // here is a value frozen at draw time.
        if let Some(last_bar) = ctx.bars().last() {
            let current_price = last_bar.close;
            let current_y = ctx.price_to_y(current_price);

            ctx.set_stroke_color(LIVE_MARKER_COLOR);
            ctx.set_stroke_width(1.0);
            ctx.set_line_dash(&[3.0, 3.0]);
            ctx.begin_path();
            ctx.move_to(crisp(x1, dpr), crisp(current_y, dpr));
            ctx.line_to(crisp(x1 + 40.0, dpr), crisp(current_y, dpr));
            ctx.stroke();
            ctx.set_line_dash(&[]);

            // Short: price BELOW entry is profit — the opposite sign from long.
            let signed_distance = self.entry_price - current_price;
            let pct = if self.entry_price != 0.0 { signed_distance / self.entry_price * 100.0 } else { 0.0 };
            let mut live_text = format!("Live {}  {} ({:+.2}%)", fmt_price(current_price), signed_price(signed_distance), pct);
            if self.show_pnl {
                live_text.push_str(&format!("  {}", signed_price(signed_distance * self.quantity)));
            }

            // Clear of the entry label when the live price sits close enough
            // to collide with it — the common case right after placing the
            // tool, before price has moved away from entry.
            let collision_band = self.label_style.font_size + self.label_style.background_padding * 2.0 + 4.0;
            let live_label_y = if (current_y - entry_y).abs() < collision_band {
                entry_y + collision_band + 2.0
            } else {
                current_y
            };
            draw_line_label(ctx, x1 + 8.0, live_label_y, &live_text, &self.label_style, LIVE_MARKER_COLOR);
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

    fn style_properties(&self) -> Vec<ConfigProperty> {
        let mut props = vec![
            ConfigProperty::show_pnl(self.show_pnl)
                .with_category(PropertyCategory::Style)
                .with_order(10),
            ConfigProperty::quantity(self.quantity)
                .with_category(PropertyCategory::Style)
                .with_order(11),
            ConfigProperty::stop_zone_color(&self.stop_color)
                .with_category(PropertyCategory::Style)
                .with_order(12),
            ConfigProperty::target_zone_color(&self.target_color)
                .with_category(PropertyCategory::Style)
                .with_order(13),
            ConfigProperty::fill_opacity(self.zone_opacity * 100.0)
                .with_category(PropertyCategory::Style)
                .with_order(14),
            ConfigProperty::label_font_size(self.label_style.font_size)
                .with_category(PropertyCategory::Style)
                .with_order(15),
            ConfigProperty::label_color(self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke))
                .with_category(PropertyCategory::Style)
                .with_order(16),
        ];
        props.extend(self.label_style.style_properties(&self.data.color.stroke, 30));
        props
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "show_pnl" => {
                if let Some(v) = value.as_bool() { self.show_pnl = v; return true; }
            }
            "quantity" => {
                if let Some(v) = value.as_number() { self.quantity = v.max(0.0); return true; }
            }
            "stop_color" => {
                if let Some(c) = value.as_color() { self.stop_color = c.to_string(); return true; }
            }
            "target_color" => {
                if let Some(c) = value.as_color() { self.target_color = c.to_string(); return true; }
            }
            "fill_opacity" => {
                if let Some(v) = value.as_number() { self.zone_opacity = (v / 100.0).clamp(0.0, 1.0); return true; }
            }
            "label_font_size" => {
                if let Some(v) = value.as_number() { self.label_style.font_size = v; return true; }
            }
            "label_color" => {
                if let Some(c) = value.as_color() { self.label_style.color = Some(c.to_string()); return true; }
            }
            _ => {}
        }
        if self.label_style.apply_property(id, value) { return true; }
        false
    }
}

/// Draw a single price-line label (small text plate) anchored just right of
/// `(x, y)` on its own line.
fn draw_line_label(ctx: &mut dyn RenderContext, x: f64, y: f64, text: &str, label_style: &LabelStyle, fallback_color: &str) {
    ctx.set_font(&label_style.font_string());
    let text_width = ctx.measure_text(text);
    let padding = label_style.background_padding;
    let text_color = label_style.color.as_deref().unwrap_or(fallback_color);

    if let Some(ref bg) = label_style.background_color {
        let bg_y = y - label_style.font_size / 2.0 - padding;
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(x, bg_y, text_width + padding * 2.0, label_style.font_size + padding * 2.0, label_style.background_radius);
        ctx.fill();

        if let Some(ref border) = label_style.border_color {
            ctx.set_stroke_color(border);
            ctx.set_stroke_width(label_style.border_width);
            ctx.stroke();
        }
    }

    ctx.set_fill_color(text_color);
    ctx.set_text_align(crate::render::TextAlign::Left);
    ctx.set_text_baseline(crate::render::TextBaseline::Middle);
    ctx.fill_text(text, x + padding, y);
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "short_position", display_name: "Short Position", kind: PrimitiveKind::Trading,
        click_behavior: ClickBehavior::ThreePoint, tooltip: "Sell trade with stop/target", icon: "short_position", default_color: "#F44336",
        factory: |points, color| {
            let (t, entry) = points.first().copied().unwrap_or((0, 100.0));
            let (_, stop) = points.get(1).copied().unwrap_or((t, entry + 5.0));
            let (_, target) = points.get(2).copied().unwrap_or((t, entry - 10.0));
            Box::new(ShortPosition::new(t, entry, stop, target, color))
        },
        supports_text: true,
        has_levels: false,
    }
}
