//! Fibonacci Circles primitive
//!
//! Concentric circles at Fibonacci ratios from a center point.
//! The radius is defined by a second point.

use serde::{Deserialize, Serialize};
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

/// Backward compatibility: deserialize old `levels: Vec<f64>` format
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
        LevelFormat::Levels(levels) => {
            Ok(levels.iter().map(|&level| FibLevelConfig::new(level)).collect())
        }
    }
}

fn default_true() -> bool { true }

fn default_label_position() -> String { "center".to_string() }

/// Fibonacci Circles - concentric circles at Fib ratios
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibCircles {
    pub data: PrimitiveData,
    /// Center timestamp (ms)
    pub center_ts: i64,
    pub center_price: f64,
    /// Edge timestamp (ms, defines radius)
    pub edge_ts: i64,
    pub edge_price: f64,
    #[serde(default = "default_level_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    #[serde(default = "default_true")]
    pub show_labels: bool,
    #[serde(default = "default_true")]
    pub show_percentages: bool,
    #[serde(default = "default_label_position")]
    pub label_position: String,
    #[serde(default = "default_true")]
    pub show_as_percent: bool,
}

impl FibCircles {
    pub fn new(center_ts: i64, center_price: f64, edge_ts: i64, edge_price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_circles".to_string(),
                display_name: "Fib Circles".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            center_ts,
            center_price,
            edge_ts,
            edge_price,
            level_configs: default_level_configs(),
            show_labels: true,
            show_percentages: true,
            label_position: "center".to_string(),
            show_as_percent: true,
        }
    }
}

impl Primitive for FibCircles {
    fn type_id(&self) -> &'static str { "fib_circles" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Fibonacci }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> {
        vec![
            (self.center_ts, self.center_price),
            (self.edge_ts, self.edge_price),
        ]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if let Some(&(ts, price)) = points.first() {
            self.center_ts = ts;
            self.center_price = price;
        }
        if let Some(&(ts, price)) = points.get(1) {
            self.edge_ts = ts;
            self.edge_price = price;
        }
    }

    fn translate(&mut self, td: i64, pd: f64) {
        self.center_ts += td;
        self.edge_ts += td;
        self.center_price += pd;
        self.edge_price += pd;
    }

    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => {
                let td = ts_ms - self.center_ts;
                let pd = price - self.center_price;
                self.center_ts = ts_ms;
                self.center_price = price;
                self.edge_ts += td;
                self.edge_price += pd;
            }
            ControlPointType::Point2 => {
                self.edge_ts = ts_ms;
                self.edge_price = price;
            }
            ControlPointType::Move => {
                let td = ts_ms - self.center_ts;
                let pd = price - self.center_price;
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

        let base_rx = (ex - cx).abs().max(1.0);
        let base_ry = (ey - cy).abs().max(1.0);

        for cfg in &self.level_configs {
            if !cfg.visible { continue; }
            let rx = base_rx * cfg.level;
            let ry = base_ry * cfg.level;
            if rx > 0.001 && ry > 0.001 {
                let nx = (sx - cx) / rx;
                let ny = (sy - cy) / ry;
                let dist = (nx * nx + ny * ny).sqrt();
                if (dist - 1.0).abs() < HIT_TOLERANCE / rx.min(ry) {
                    return HitTestResult::Body;
                }
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

        let base_rx = (ex - cx).abs();
        let base_ry = (ey - cy).abs();

        // === FILL RENDERING ===
        let mut visible_levels: Vec<(usize, f64, f64, f64)> = self.level_configs
            .iter()
            .enumerate()
            .filter(|(_, cfg)| cfg.visible)
            .map(|(idx, cfg)| {
                let rx = base_rx * cfg.level;
                let ry = base_ry * cfg.level;
                (idx, cfg.level, rx, ry)
            })
            .collect();
        visible_levels.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        for i in 0..visible_levels.len().saturating_sub(1) {
            let (idx, _, rx1, ry1) = visible_levels[i];
            let (_, _, rx2, ry2) = visible_levels[i + 1];
            let cfg = &self.level_configs[idx];
            if cfg.fill_enabled {
                let fill_color = cfg.fill_color.as_deref()
                    .or(cfg.color.as_deref())
                    .unwrap_or(&self.data.color.stroke);
                ctx.set_fill_color_alpha(fill_color, cfg.fill_opacity);
                ctx.begin_path();
                ctx.ellipse(cx, cy, rx2, ry2, 0.0, 0.0, std::f64::consts::TAU);
                ctx.ellipse(cx, cy, rx1, ry1, 0.0, std::f64::consts::TAU, 0.0);
                ctx.fill();
                ctx.reset_alpha();
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
                _ => LineStyle::Solid,
            };
            match line_style {
                LineStyle::Solid => ctx.set_line_dash(&[]),
                LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
                LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
                LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
                LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
            }

            let rx = base_rx * cfg.level;
            let ry = base_ry * cfg.level;

            let (label, gap_angle, gap_half_angle) = if self.show_labels || self.show_percentages {
                let label = {
                    let mut label_parts = Vec::new();
                    if self.show_percentages {
                        if self.show_as_percent {
                            let pct = cfg.level * 100.0;
                            if (pct - pct.round()).abs() < 0.01 {
                                label_parts.push(format!("{}%", pct as i32));
                            } else {
                                label_parts.push(format!("{:.1}%", pct));
                            }
                        } else {
                            let lvl = cfg.level;
                            if (lvl - lvl.round()).abs() < 0.0001 {
                                label_parts.push(format!("{}", lvl as i32));
                            } else if (lvl * 10.0 - (lvl * 10.0).round()).abs() < 0.001 {
                                label_parts.push(format!("{:.1}", lvl));
                            } else {
                                label_parts.push(format!("{:.3}", lvl));
                            }
                        }
                    }
                    label_parts.join(" ")
                };

                if !label.is_empty() {
                    let center_angle = match self.label_position.as_str() {
                        "right" => 0.0,
                        "center" => -std::f64::consts::FRAC_PI_2,
                        _ => std::f64::consts::PI,
                    };
                    let char_width = 6.5;
                    let text_width = label.len() as f64 * char_width;
                    let avg_radius = (rx + ry) / 2.0;
                    let half_gap = if avg_radius > 0.001 {
                        (text_width / 2.0 / avg_radius).min(0.5)
                    } else {
                        0.0
                    };
                    (Some(label), center_angle, half_gap)
                } else {
                    (None, 0.0, 0.0)
                }
            } else {
                (None, 0.0, 0.0)
            };

            ctx.begin_path();
            if label.is_some() && gap_half_angle > 0.001 {
                let gap_start = gap_angle - gap_half_angle;
                let gap_end = gap_angle + gap_half_angle;
                ctx.ellipse(cx, cy, rx, ry, 0.0, gap_end, gap_start + std::f64::consts::TAU);
            } else {
                ctx.ellipse(cx, cy, rx, ry, 0.0, 0.0, std::f64::consts::TAU);
            }
            ctx.stroke();

            if let Some(ref lbl) = label {
                ctx.set_font("11px sans-serif");
                ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                ctx.set_fill_color(color);
                let (label_x, label_y) = match self.label_position.as_str() {
                    "right" => (cx + rx, cy),
                    "center" => (cx, cy - ry),
                    _ => (cx - rx, cy),
                };
                ctx.set_text_align(crate::render::TextAlign::Center);
                ctx.fill_text(lbl, label_x, label_y);
            }
        }
        ctx.set_line_dash(&[]);

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_x = match text.h_align {
                    TextAlign::Start => cx - base_rx,
                    TextAlign::Center => cx,
                    TextAlign::End => cx + base_rx,
                };
                let text_y = match text.v_align {
                    TextAlign::Start | TextAlign::End => {
                        let max_level = self.level_configs.iter()
                            .filter(|cfg| cfg.visible)
                            .map(|cfg| cfg.level)
                            .fold(0.0_f64, |a, b| a.max(b));
                        let outer_ry = base_ry * max_level;
                        let text_offset = 8.0 + text.font_size / 2.0;
                        if matches!(text.v_align, TextAlign::Start) {
                            cy - outer_ry - text_offset
                        } else {
                            cy + outer_ry + text_offset
                        }
                    }
                    TextAlign::Center => {
                        let median_ry = base_ry * 0.5;
                        cy - median_ry
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
            ConfigProperty::show_labels(self.show_labels).with_order(10),
            ConfigProperty::levels(self.show_percentages).with_order(11),
            ConfigProperty::show_as_percent(self.show_as_percent).with_order(12),
            ConfigProperty::label_position(&self.label_position).with_order(13),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &super::super::config::PropertyValue) -> bool {
        use super::super::config::PropertyValue;
        match id {
            "show_labels" => { if let PropertyValue::Boolean(v) = value { self.show_labels = *v; return true; } }
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

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "fib_circles",
        display_name: "Fib Circles",
        kind: PrimitiveKind::Fibonacci,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Concentric circles at Fibonacci ratios",
        icon: "fib_circles",
        default_color: "#F7B93E",
        factory: |points, color| {
            let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
            let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, price1 + 10.0));
            Box::new(FibCircles::new(ts1, price1, ts2, price2, color))
        },
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
