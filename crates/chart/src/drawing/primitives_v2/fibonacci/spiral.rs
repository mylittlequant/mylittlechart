//! Fibonacci Spiral primitive
//!
//! A logarithmic spiral based on the golden ratio (phi = 1.618).

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle, TextAlign,
    HIT_TOLERANCE, CONTROL_POINT_RADIUS,
    RenderContext, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
    config::FibLevelConfig,
};
use super::retracement::default_level_configs;

pub const PHI: f64 = 1.618033988749895;

fn deserialize_level_configs<'de, D>(deserializer: D) -> Result<Vec<FibLevelConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum LevelFormat {
        Configs(Vec<FibLevelConfig>),
        Levels(Vec<f64>),
    }
    match LevelFormat::deserialize(deserializer)? {
        LevelFormat::Configs(configs) => Ok(configs),
        LevelFormat::Levels(levels) => Ok(levels.iter().map(|&level| FibLevelConfig::new(level)).collect()),
    }
}

/// Fibonacci Spiral
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibSpiral {
    pub data: PrimitiveData,
    /// Center timestamp (ms)
    pub center_ts: i64,
    pub center_price: f64,
    /// Edge timestamp (ms, defines initial radius)
    pub edge_ts: i64,
    pub edge_price: f64,
    #[serde(default = "default_level_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    #[serde(default = "default_rotations")]
    pub rotations: f64,
    #[serde(default = "default_true")]
    pub clockwise: bool,
    #[serde(default)]
    pub flip_horizontal: bool,
    #[serde(default)]
    pub flip_vertical: bool,
    #[serde(default = "default_true")]
    pub show_percentages: bool,
    #[serde(default = "default_label_position")]
    pub label_position: String,
    #[serde(default = "default_true")]
    pub show_as_percent: bool,
}

fn default_true() -> bool { true }
fn default_rotations() -> f64 { 3.0 }
fn default_label_position() -> String { "left".to_string() }

impl FibSpiral {
    pub fn new(center_ts: i64, center_price: f64, edge_ts: i64, edge_price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_spiral".to_string(),
                display_name: "Fib Spiral".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            center_ts, center_price, edge_ts, edge_price,
            level_configs: default_level_configs(),
            rotations: 3.0,
            clockwise: true,
            flip_horizontal: false,
            flip_vertical: false,
            show_percentages: true,
            label_position: "left".to_string(),
            show_as_percent: true,
        }
    }

    /// Returns spiral points as (timestamp_ms, price)
    pub fn spiral_points(&self, num_points: usize) -> Vec<(i64, f64)> {
        let initial_radius_ts = (self.edge_ts - self.center_ts).abs() as f64;
        let initial_radius_price = (self.edge_price - self.center_price).abs();

        let b = PHI.ln() / (PI / 2.0);
        let mut points = Vec::with_capacity(num_points);
        let max_angle = self.rotations * 2.0 * PI;

        for i in 0..num_points {
            let t = i as f64 / (num_points - 1) as f64;
            let theta = t * max_angle;
            let r = (-b * theta).exp();
            let angle = if self.clockwise { theta } else { -theta };
            let mut dx = r * angle.cos();
            let mut dy = r * angle.sin();
            if self.flip_horizontal { dx = -dx; }
            if self.flip_vertical { dy = -dy; }
            let ts = self.center_ts + (dx * initial_radius_ts) as i64;
            let price = self.center_price + dy * initial_radius_price;
            points.push((ts, price));
        }
        points
    }
}

impl Primitive for FibSpiral {
    fn type_id(&self) -> &'static str { "fib_spiral" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Fibonacci }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> {
        vec![(self.center_ts, self.center_price), (self.edge_ts, self.edge_price)]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if let Some(&(ts, price)) = points.first() { self.center_ts = ts; self.center_price = price; }
        if let Some(&(ts, price)) = points.get(1) { self.edge_ts = ts; self.edge_price = price; }
    }

    fn translate(&mut self, td: i64, pd: f64) {
        self.center_ts += td; self.edge_ts += td;
        self.center_price += pd; self.edge_price += pd;
    }

    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => {
                let td = ts_ms - self.center_ts;
                let pd = price - self.center_price;
                self.center_ts = ts_ms; self.center_price = price;
                self.edge_ts += td; self.edge_price += pd;
            }
            ControlPointType::Point2 => { self.edge_ts = ts_ms; self.edge_price = price; }
            ControlPointType::Move => {
                let td = ts_ms - self.center_ts; let pd = price - self.center_price;
                self.translate(td, pd);
            }
            _ => {}
        }
    }

    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let center_b = timestamp_ms_to_bar_f64(bars, self.center_ts);
        let edge_b = timestamp_ms_to_bar_f64(bars, self.edge_ts);
        let cx = vp.bar_to_x_f64(center_b);
        let cy = vp.price_to_y(self.center_price, ps.price_min, ps.price_max);
        let ex = vp.bar_to_x_f64(edge_b);
        let ey = vp.price_to_y(self.edge_price, ps.price_min, ps.price_max);

        if check_point_hit(sx, sy, cx, cy) { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if check_point_hit(sx, sy, ex, ey) { return HitTestResult::ControlPoint(ControlPointType::Point2); }

        let spiral = self.spiral_points(200);
        for window in spiral.windows(2) {
            let (ts1, price1) = window[0];
            let (ts2, price2) = window[1];
            let b1 = timestamp_ms_to_bar_f64(bars, ts1);
            let b2 = timestamp_ms_to_bar_f64(bars, ts2);
            let x1 = vp.bar_to_x_f64(b1);
            let y1 = vp.price_to_y(price1, ps.price_min, ps.price_max);
            let x2 = vp.bar_to_x_f64(b2);
            let y2 = vp.price_to_y(price2, ps.price_min, ps.price_max);
            if point_to_line_distance(sx, sy, x1, y1, x2, y2) < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let center_b = timestamp_ms_to_bar_f64(bars, self.center_ts);
        let edge_b = timestamp_ms_to_bar_f64(bars, self.edge_ts);
        let cx = vp.bar_to_x_f64(center_b);
        let cy = vp.price_to_y(self.center_price, ps.price_min, ps.price_max);
        let ex = vp.bar_to_x_f64(edge_b);
        let ey = vp.price_to_y(self.edge_price, ps.price_min, ps.price_max);
        vec![ControlPoint::point1(cx, cy), ControlPoint::point2(ex, ey)]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let cx = ctx.ts_to_x_ms(self.center_ts);
        let cy = ctx.price_to_y(self.center_price);
        let ex = ctx.ts_to_x_ms(self.edge_ts);
        let ey = ctx.price_to_y(self.edge_price);

        let spiral_data = self.spiral_points(200);

        // === FILL RENDERING ===
        let total_points = spiral_data.len();
        if total_points >= 2 {
            let mut visible_levels: Vec<(usize, f64, usize, usize)> = self.level_configs
                .iter()
                .enumerate()
                .filter(|(_, cfg)| cfg.visible)
                .map(|(idx, cfg)| {
                    let start_idx = (cfg.level * total_points as f64 / self.rotations).floor() as usize;
                    let end_idx = ((cfg.level + 0.1) * total_points as f64 / self.rotations).ceil() as usize;
                    let start_idx = start_idx.min(total_points - 1);
                    let end_idx = end_idx.min(total_points).max(start_idx + 1);
                    (idx, cfg.level, start_idx, end_idx)
                })
                .collect();
            visible_levels.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            for i in 0..visible_levels.len().saturating_sub(1) {
                let (idx, _, start1, end1) = visible_levels[i];
                let (_, _, start2, _end2) = visible_levels[i + 1];
                let cfg = &self.level_configs[idx];
                if cfg.fill_enabled && start1 < total_points && start2 < total_points {
                    let fill_color = cfg.fill_color.as_deref()
                        .or(cfg.color.as_deref())
                        .unwrap_or(&self.data.color.stroke);
                    ctx.set_fill_color_alpha(fill_color, cfg.fill_opacity);
                    ctx.begin_path();
                    ctx.move_to(cx, cy);
                    for &(ts, price) in &spiral_data[start1..end1.min(total_points)] {
                        ctx.line_to(ctx.ts_to_x_ms(ts), ctx.price_to_y(price));
                    }
                    for j in (start2.._end2.min(total_points)).rev() {
                        let (ts, price) = spiral_data[j];
                        ctx.line_to(ctx.ts_to_x_ms(ts), ctx.price_to_y(price));
                    }
                    ctx.close_path();
                    ctx.fill();
                    ctx.reset_alpha();
                }
            }
        }

        for cfg in &self.level_configs {
            if !cfg.visible { continue; }

            let color = cfg.color.as_deref().unwrap_or(&self.data.color.stroke);
            ctx.set_stroke_color(color);
            let width = cfg.width.unwrap_or(self.data.width);
            ctx.set_stroke_width(width);

            let line_style = match cfg.style.as_str() {
                "dashed" => LineStyle::Dashed,
                "dotted" => LineStyle::Dotted,
                "large_dashed" => LineStyle::LargeDashed,
                "sparse_dotted" => LineStyle::SparseDotted,
                _ => self.data.style,
            };
            match line_style {
                LineStyle::Solid => ctx.set_line_dash(&[]),
                LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
                LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
                LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
                LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
            }

            let total_points = spiral_data.len();
            if total_points < 2 { continue; }

            let start_idx = (cfg.level * total_points as f64 / self.rotations).floor() as usize;
            let end_idx = ((cfg.level + 0.1) * total_points as f64 / self.rotations).ceil() as usize;
            let start_idx = start_idx.min(total_points - 1);
            let end_idx = end_idx.min(total_points).max(start_idx + 1);

            if start_idx >= total_points - 1 { continue; }

            ctx.begin_path();
            let (ts, price) = spiral_data[start_idx];
            ctx.move_to(ctx.ts_to_x_ms(ts), ctx.price_to_y(price));
            for &(ts, price) in &spiral_data[(start_idx + 1)..end_idx] {
                ctx.line_to(ctx.ts_to_x_ms(ts), ctx.price_to_y(price));
            }
            ctx.stroke();

            if self.show_percentages {
                let label = {
                    let mut label_parts = Vec::new();
                    if self.show_as_percent {
                        let pct = cfg.level * 100.0;
                        if (pct - pct.round()).abs() < 0.01 { label_parts.push(format!("{}%", pct as i32)); }
                        else { label_parts.push(format!("{:.1}%", pct)); }
                    } else {
                        let lvl = cfg.level;
                        if (lvl - lvl.round()).abs() < 0.0001 { label_parts.push(format!("{}", lvl as i32)); }
                        else if (lvl * 10.0 - (lvl * 10.0).round()).abs() < 0.001 { label_parts.push(format!("{:.1}", lvl)); }
                        else { label_parts.push(format!("{:.3}", lvl)); }
                    }
                    label_parts.join(" ")
                };

                if !label.is_empty() && start_idx < spiral_data.len() {
                    let (ts, price) = spiral_data[start_idx];
                    let lx = ctx.ts_to_x_ms(ts);
                    let ly = ctx.price_to_y(price);
                    ctx.set_font("11px sans-serif");
                    ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                    ctx.set_fill_color(color);
                    ctx.set_text_align(crate::render::TextAlign::Left);
                    ctx.fill_text(&label, lx, ly);
                }
            }
        }
        ctx.set_line_dash(&[]);

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let spiral_screen_points: Vec<(f64, f64)> = spiral_data.iter()
                    .map(|&(ts, price)| (ctx.ts_to_x_ms(ts), ctx.price_to_y(price)))
                    .collect();

                let (min_x, max_x, min_y, max_y) = if spiral_screen_points.is_empty() {
                    (cx, ex, cy, ey)
                } else {
                    let min_x = spiral_screen_points.iter().fold(f64::INFINITY, |a, &(x, _)| a.min(x));
                    let max_x = spiral_screen_points.iter().fold(f64::NEG_INFINITY, |a, &(x, _)| a.max(x));
                    let min_y = spiral_screen_points.iter().fold(f64::INFINITY, |a, &(_, y)| a.min(y));
                    let max_y = spiral_screen_points.iter().fold(f64::NEG_INFINITY, |a, &(_, y)| a.max(y));
                    (min_x, max_x, min_y, max_y)
                };

                let text_x = match text.h_align {
                    TextAlign::Start => min_x,
                    TextAlign::Center => (min_x + max_x) / 2.0,
                    TextAlign::End => max_x,
                };
                let text_y = match text.v_align {
                    TextAlign::Start => {
                        let text_offset = 8.0 + text.font_size / 2.0;
                        min_y - text_offset
                    }
                    TextAlign::Center => (min_y + max_y) / 2.0,
                    TextAlign::End => {
                        let text_offset = 8.0 + text.font_size / 2.0;
                        max_y + text_offset
                    }
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (px, py) in [(cx, cy), (ex, ey)] {
                ctx.begin_path();
                ctx.arc(px, py, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }
        }
    }

    fn level_configs(&self) -> Option<Vec<FibLevelConfig>> { Some(self.level_configs.clone()) }
    fn set_level_configs(&mut self, configs: Vec<FibLevelConfig>) -> bool { self.level_configs = configs; true }

    fn style_properties(&self) -> Vec<super::super::config::ConfigProperty> {
        use super::super::config::ConfigProperty;
        vec![
            ConfigProperty::levels(self.show_percentages).with_order(10),
            ConfigProperty::show_as_percent(self.show_as_percent).with_order(11),
            ConfigProperty::label_position(&self.label_position).with_order(12),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &super::super::config::PropertyValue) -> bool {
        use super::super::config::PropertyValue;
        match id {
            "show_percentages" => { if let PropertyValue::Boolean(v) = value { self.show_percentages = *v; return true; } }
            "show_as_percent" => { if let PropertyValue::Boolean(v) = value { self.show_as_percent = *v; return true; } }
            "label_position" => { if let PropertyValue::String(v) = value { self.label_position = v.clone(); return true; } }
            _ => {}
        }
        false
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    let radius = 8.0;
    (sx - px).powi(2) + (sy - py).powi(2) <= radius * radius
}

fn point_to_line_distance(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1; let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 { return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt(); }
    let t = (((px - x1) * dx + (py - y1) * dy) / len_sq).clamp(0.0, 1.0);
    let proj_x = x1 + t * dx; let proj_y = y1 + t * dy;
    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "fib_spiral",
        display_name: "Fib Spiral",
        kind: PrimitiveKind::Fibonacci,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Golden ratio logarithmic spiral",
        icon: "fib_spiral",
        default_color: "#F7B93E",
        factory: |points, color| {
            let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
            let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, price1 + 10.0));
            Box::new(FibSpiral::new(ts1, price1, ts2, price2, color))
        },
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
