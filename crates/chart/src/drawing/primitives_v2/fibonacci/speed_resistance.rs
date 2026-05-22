//! Fibonacci Speed Resistance Fan primitive

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle, TextAlign,
    HIT_TOLERANCE, CONTROL_POINT_RADIUS,
    RenderContext, crisp, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text_rotated, calculate_line_text_params,
    config::FibLevelConfig,
};
use super::retracement::default_level_configs;

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

/// Fibonacci Speed Resistance Fan
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibSpeedResistance {
    pub data: PrimitiveData,
    pub ts1: i64,
    pub price1: f64,
    pub ts2: i64,
    pub price2: f64,
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
    #[serde(default)]
    pub reverse: bool,
}

fn default_true() -> bool { true }
fn default_label_position() -> String { "center".to_string() }

impl FibSpeedResistance {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_speed_resistance".to_string(),
                display_name: "Speed Resistance".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            ts1, price1, ts2, price2,
            level_configs: default_level_configs(),
            show_labels: true,
            show_percentages: true,
            label_position: "center".to_string(),
            show_as_percent: true,
            reverse: false,
        }
    }
}

impl Primitive for FibSpeedResistance {
    fn type_id(&self) -> &'static str { "fib_speed_resistance" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Fibonacci }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> {
        vec![(self.ts1, self.price1), (self.ts2, self.price2)]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if let Some(&(ts, price)) = points.first() { self.ts1 = ts; self.price1 = price; }
        if let Some(&(ts, price)) = points.get(1) { self.ts2 = ts; self.price2 = price; }
    }

    fn translate(&mut self, td: i64, pd: f64) {
        self.ts1 += td; self.ts2 += td;
        self.price1 += pd; self.price2 += pd;
    }

    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.ts1 = ts_ms; self.price1 = price; }
            ControlPointType::Point2 => { self.ts2 = ts_ms; self.price2 = price; }
            ControlPointType::Move => {
                let td = ts_ms - self.ts1; let pd = price - self.price1;
                self.translate(td, pd);
            }
            _ => {}
        }
    }

    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = vp.bar_to_x_f64(b1);
        let y1 = vp.price_to_y(self.price1, ps.price_min, ps.price_max);
        let x2 = vp.bar_to_x_f64(b2);
        let y2 = vp.price_to_y(self.price2, ps.price_min, ps.price_max);

        if check_point_hit(sx, sy, x1, y1) { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if check_point_hit(sx, sy, x2, y2) { return HitTestResult::ControlPoint(ControlPointType::Point2); }

        let price_range = self.price2 - self.price1;

        for cfg in &self.level_configs {
            if !cfg.visible { continue; }
            let level_price = if self.reverse {
                self.price1 + price_range * (1.0 - cfg.level)
            } else {
                self.price1 + price_range * cfg.level
            };
            let fan_y2 = vp.price_to_y(level_price, ps.price_min, ps.price_max);
            if point_to_ray_distance(sx, sy, x1, y1, x2, fan_y2) < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }

        if point_to_line_distance(sx, sy, x1, y1, x2, y2) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = vp.bar_to_x_f64(b1);
        let y1 = vp.price_to_y(self.price1, ps.price_min, ps.price_max);
        let x2 = vp.bar_to_x_f64(b2);
        let y2 = vp.price_to_y(self.price2, ps.price_min, ps.price_max);
        vec![ControlPoint::point1(x1, y1), ControlPoint::point2(x2, y2)]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y2 = ctx.price_to_y(self.price2);
        let chart_width = ctx.chart_width();

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
        ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
        ctx.stroke();

        let price_range = self.price2 - self.price1;

        // === FILL RENDERING ===
        let mut visible_levels: Vec<(usize, f64, f64)> = self.level_configs
            .iter()
            .enumerate()
            .filter(|(_, cfg)| cfg.visible)
            .map(|(idx, cfg)| {
                let level_price = if self.reverse {
                    self.price1 + price_range * (1.0 - cfg.level)
                } else {
                    self.price1 + price_range * cfg.level
                };
                let fan_y = ctx.price_to_y(level_price);
                (idx, cfg.level, fan_y)
            })
            .collect();
        visible_levels.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        for i in 0..visible_levels.len().saturating_sub(1) {
            let (idx, _, fy1) = visible_levels[i];
            let (_, _, fy2) = visible_levels[i + 1];
            let cfg = &self.level_configs[idx];
            if cfg.fill_enabled {
                let fill_color = cfg.fill_color.as_deref()
                    .or(cfg.color.as_deref())
                    .unwrap_or(&self.data.color.stroke);
                let dx1 = x2 - x1; let dy1 = fy1 - y1;
                let len1 = (dx1 * dx1 + dy1 * dy1).sqrt();
                let dx2 = x2 - x1; let dy2 = fy2 - y1;
                let len2 = (dx2 * dx2 + dy2 * dy2).sqrt();
                let ext = chart_width * 2.0;
                let (end_x1, end_y1) = if len1 > 0.0 { (x1 + dx1 / len1 * ext, y1 + dy1 / len1 * ext) } else { (x2, fy1) };
                let (end_x2, end_y2) = if len2 > 0.0 { (x1 + dx2 / len2 * ext, y1 + dy2 / len2 * ext) } else { (x2, fy2) };
                ctx.set_fill_color_alpha(fill_color, cfg.fill_opacity);
                ctx.begin_path();
                ctx.move_to(x1, y1);
                ctx.line_to(end_x1, end_y1);
                ctx.line_to(end_x2, end_y2);
                ctx.close_path();
                ctx.fill();
                ctx.reset_alpha();
            }
        }

        let needs_gap = self.data.text.as_ref()
            .map(|t| !t.content.is_empty() && matches!(t.v_align, TextAlign::Center))
            .unwrap_or(false);

        for cfg in &self.level_configs {
            if !cfg.visible { continue; }

            let level_price = if self.reverse {
                self.price1 + price_range * (1.0 - cfg.level)
            } else {
                self.price1 + price_range * cfg.level
            };
            let fan_y = ctx.price_to_y(level_price);

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

            let dx = x2 - x1;
            let dy = fan_y - y1;
            let len = (dx * dx + dy * dy).sqrt();

            let (label, label_gap_t_start, label_gap_t_end) = if self.show_labels || self.show_percentages {
                let label = {
                    let mut label_parts = Vec::new();
                    if self.show_percentages {
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
                    }
                    label_parts.join(" ")
                };
                if !label.is_empty() && len > 0.001 {
                    let char_width = 6.5;
                    let text_width = label.len() as f64 * char_width;
                    let half_gap_t = (text_width / 2.0) / len;
                    let (gap_start, gap_end) = match self.label_position.as_str() {
                        "right" => ((1.0 - half_gap_t * 2.0).max(0.0), 1.0),
                        "center" => ((0.5 - half_gap_t).max(0.0), (0.5 + half_gap_t).min(1.0)),
                        _ => (0.0, (half_gap_t * 2.0).min(1.0)),
                    };
                    (Some(label), gap_start, gap_end)
                } else {
                    (if label.is_empty() { None } else { Some(label) }, 0.0, 0.0)
                }
            } else {
                (None, 0.0, 0.0)
            };

            let is_median = (cfg.level - 0.5).abs() < 0.001;
            let has_label_gap = label.is_some() && label_gap_t_end > label_gap_t_start;

            let (use_gap, gap_t_start, gap_t_end) = if has_label_gap {
                (true, label_gap_t_start, label_gap_t_end)
            } else if is_median && needs_gap && len > 0.001 {
                let text = self.data.text.as_ref().unwrap();
                let t_center = match text.h_align {
                    TextAlign::Start => 0.0, TextAlign::Center => 0.5, TextAlign::End => 1.0,
                };
                let char_count = text.content.len() as f64;
                let text_width = char_count * text.font_size * 0.6 + 8.0;
                let half_gap_t = (text_width / 2.0) / len;
                (true, (t_center - half_gap_t).max(0.0), (t_center + half_gap_t).min(1.0))
            } else {
                (false, 0.0, 0.0)
            };

            ctx.begin_path();
            if use_gap && gap_t_end > gap_t_start && len > 0.001 {
                if gap_t_start > 0.001 {
                    let gap_x1 = x1 + dx * gap_t_start;
                    let gap_y1 = y1 + dy * gap_t_start;
                    ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
                    ctx.line_to(crisp(gap_x1, dpr), crisp(gap_y1, dpr));
                }
                let gap_x2 = x1 + dx * gap_t_end;
                let gap_y2 = y1 + dy * gap_t_end;
                ctx.move_to(crisp(gap_x2, dpr), crisp(gap_y2, dpr));
                let ext = chart_width * 2.0;
                let nx = dx / len; let ny = dy / len;
                ctx.line_to(crisp(x1 + nx * ext, dpr), crisp(y1 + ny * ext, dpr));
            } else {
                ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
                if len > 0.0 {
                    let ext = chart_width * 2.0;
                    let nx = dx / len; let ny = dy / len;
                    ctx.line_to(crisp(x1 + nx * ext, dpr), crisp(y1 + ny * ext, dpr));
                }
            }
            ctx.stroke();

            if let Some(lbl) = label {
                ctx.set_font("11px sans-serif");
                ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                ctx.set_fill_color(color);
                match self.label_position.as_str() {
                    "right" => { ctx.set_text_align(crate::render::TextAlign::Right); ctx.fill_text(&lbl, x2, fan_y); }
                    "center" => { ctx.set_text_align(crate::render::TextAlign::Center); ctx.fill_text(&lbl, (x1 + x2) / 2.0, (y1 + fan_y) / 2.0); }
                    _ => { ctx.set_text_align(crate::render::TextAlign::Left); ctx.fill_text(&lbl, x1, y1); }
                }
            }
        }
        ctx.set_line_dash(&[]);

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let (text_start_x, text_start_y, text_end_x, text_end_y) = match text.v_align {
                    TextAlign::Start | TextAlign::End => {
                        let mut fan_endpoints: Vec<(f64, f64, f64)> = self.level_configs.iter()
                            .filter(|cfg| cfg.visible)
                            .map(|cfg| {
                                let level_price = if self.reverse {
                                    self.price1 + price_range * (1.0 - cfg.level)
                                } else {
                                    self.price1 + price_range * cfg.level
                                };
                                let fy = ctx.price_to_y(level_price);
                                (cfg.level, fy, level_price)
                            }).collect();
                        if fan_endpoints.is_empty() {
                            (x1, y1, x2, y2)
                        } else {
                            fan_endpoints.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
                            let (_, _, level_price) = if matches!(text.v_align, TextAlign::Start) {
                                *fan_endpoints.first().unwrap()
                            } else {
                                *fan_endpoints.last().unwrap()
                            };
                            let fy = ctx.price_to_y(level_price);
                            (x1, y1, x2, fy)
                        }
                    }
                    TextAlign::Center => {
                        let median_price = self.price1 + price_range * 0.5;
                        let fy = ctx.price_to_y(median_price);
                        (x1, y1, x2, fy)
                    }
                };
                let params = calculate_line_text_params(text_start_x, text_start_y, text_end_x, text_end_y, text);
                render_primitive_text_rotated(ctx, text, params.x, params.y, &self.data.color.stroke, params.rotation);
            }
        }

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (px, py) in [(x1, y1), (x2, y2)] {
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
            ConfigProperty::reverse(self.reverse).with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &super::super::config::PropertyValue) -> bool {
        use super::super::config::PropertyValue;
        match id {
            "show_labels" => { if let PropertyValue::Boolean(v) = value { self.show_labels = *v; return true; } }
            "show_percentages" => { if let PropertyValue::Boolean(v) = value { self.show_percentages = *v; return true; } }
            "show_as_percent" => { if let PropertyValue::Boolean(v) = value { self.show_as_percent = *v; return true; } }
            "label_position" => { if let PropertyValue::String(v) = value { self.label_position = v.clone(); return true; } }
            "reverse" => { if let PropertyValue::Boolean(v) = value { self.reverse = *v; return true; } }
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

fn point_to_ray_distance(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1; let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 { return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt(); }
    let t = (((px - x1) * dx + (py - y1) * dy) / len_sq).max(0.0);
    let proj_x = x1 + t * dx; let proj_y = y1 + t * dy;
    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "fib_speed_resistance",
        display_name: "Speed Resistance",
        kind: PrimitiveKind::Fibonacci,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Fibonacci speed resistance fan lines",
        icon: "fib_speed_resistance",
        default_color: "#F7B93E",
        factory: |points, color| {
            let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
            let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, price1 + 10.0));
            Box::new(FibSpeedResistance::new(ts1, price1, ts2, price2, color))
        },
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
