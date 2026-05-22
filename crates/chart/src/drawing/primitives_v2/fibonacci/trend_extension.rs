//! Fibonacci Trend Extension primitive
//!
//! Uses three points to project Fibonacci extension levels.
//! Point 1 and 2 define the trend, Point 3 is the retracement.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle, TextAlign,
    HIT_TOLERANCE, CONTROL_POINT_RADIUS,
    RenderContext, crisp, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
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

/// Fibonacci Trend Extension - three-point projection
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibTrendExtension {
    pub data: PrimitiveData,
    pub ts1: i64,
    pub price1: f64,
    pub ts2: i64,
    pub price2: f64,
    pub ts3: i64,
    pub price3: f64,
    #[serde(default = "default_level_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    #[serde(default = "default_true")]
    pub show_prices: bool,
    #[serde(default = "default_true")]
    pub show_percentages: bool,
    #[serde(default = "default_label_position")]
    pub label_position: String,
    #[serde(default = "default_true")]
    pub show_as_percent: bool,
    #[serde(default = "default_true")]
    pub show_trend_line: bool,
    #[serde(default = "default_true")]
    pub extend_right: bool,
}

fn default_true() -> bool { true }
fn default_label_position() -> String { "left".to_string() }

impl FibTrendExtension {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, ts3: i64, price3: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_trend_extension".to_string(),
                display_name: "Fib Extension".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            ts1, price1, ts2, price2, ts3, price3,
            level_configs: default_level_configs(),
            show_prices: true,
            show_percentages: true,
            label_position: "left".to_string(),
            show_as_percent: true,
            show_trend_line: true,
            extend_right: true,
        }
    }

    pub fn price_at_level(&self, level: f64) -> f64 {
        let range = self.price2 - self.price1;
        self.price3 + range * level
    }
}

impl Primitive for FibTrendExtension {
    fn type_id(&self) -> &'static str { "fib_trend_extension" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Fibonacci }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::ThreePoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> {
        vec![(self.ts1, self.price1), (self.ts2, self.price2), (self.ts3, self.price3)]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if let Some(&(ts, price)) = points.first() { self.ts1 = ts; self.price1 = price; }
        if let Some(&(ts, price)) = points.get(1) { self.ts2 = ts; self.price2 = price; }
        if let Some(&(ts, price)) = points.get(2) { self.ts3 = ts; self.price3 = price; }
    }

    fn translate(&mut self, td: i64, pd: f64) {
        self.ts1 += td; self.ts2 += td; self.ts3 += td;
        self.price1 += pd; self.price2 += pd; self.price3 += pd;
    }

    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.ts1 = ts_ms; self.price1 = price; }
            ControlPointType::Point2 => { self.ts2 = ts_ms; self.price2 = price; }
            ControlPointType::Point3 => { self.ts3 = ts_ms; self.price3 = price; }
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
        let b3 = timestamp_ms_to_bar_f64(bars, self.ts3);
        let x1 = vp.bar_to_x_f64(b1);
        let y1 = vp.price_to_y(self.price1, ps.price_min, ps.price_max);
        let x2 = vp.bar_to_x_f64(b2);
        let y2 = vp.price_to_y(self.price2, ps.price_min, ps.price_max);
        let x3 = vp.bar_to_x_f64(b3);
        let y3 = vp.price_to_y(self.price3, ps.price_min, ps.price_max);

        if check_point_hit(sx, sy, x1, y1) { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if check_point_hit(sx, sy, x2, y2) { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        if check_point_hit(sx, sy, x3, y3) { return HitTestResult::ControlPoint(ControlPointType::Point3); }

        let min_x = x3;

        for cfg in &self.level_configs {
            if !cfg.visible { continue; }
            let level_price = self.price_at_level(cfg.level);
            let level_y = vp.price_to_y(level_price, ps.price_min, ps.price_max);
            let in_bounds = if self.extend_right { sx >= min_x } else { sx >= min_x && sx <= vp.chart_width };
            if in_bounds && (sy - level_y).abs() < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }

        if point_to_line_distance(sx, sy, x1, y1, x2, y2) < HIT_TOLERANCE { return HitTestResult::Body; }
        if point_to_line_distance(sx, sy, x2, y2, x3, y3) < HIT_TOLERANCE { return HitTestResult::Body; }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let b3 = timestamp_ms_to_bar_f64(bars, self.ts3);
        let x1 = vp.bar_to_x_f64(b1);
        let y1 = vp.price_to_y(self.price1, ps.price_min, ps.price_max);
        let x2 = vp.bar_to_x_f64(b2);
        let y2 = vp.price_to_y(self.price2, ps.price_min, ps.price_max);
        let x3 = vp.bar_to_x_f64(b3);
        let y3 = vp.price_to_y(self.price3, ps.price_min, ps.price_max);
        vec![ControlPoint::point1(x1, y1), ControlPoint::point2(x2, y2), ControlPoint::point3(x3, y3)]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y2 = ctx.price_to_y(self.price2);
        let x3 = ctx.ts_to_x_ms(self.ts3);
        let y3 = ctx.price_to_y(self.price3);
        let chart_width = ctx.chart_width();
        let right_x = chart_width;

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        if self.show_trend_line {
            ctx.begin_path();
            ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
            ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
            ctx.line_to(crisp(x3, dpr), crisp(y3, dpr));
            ctx.stroke();
        }

        // === FILL ===
        let mut visible_levels: Vec<(usize, f64, f64)> = self.level_configs
            .iter()
            .enumerate()
            .filter(|(_, cfg)| cfg.visible)
            .map(|(idx, cfg)| {
                let y = ctx.price_to_y(self.price_at_level(cfg.level));
                (idx, cfg.level, y)
            })
            .collect();
        visible_levels.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        for i in 0..visible_levels.len().saturating_sub(1) {
            let (idx, _, y_top) = visible_levels[i];
            let (_, _, y_bottom) = visible_levels[i + 1];
            let cfg = &self.level_configs[idx];
            if cfg.fill_enabled {
                let fill_color = cfg.fill_color.as_deref()
                    .or(cfg.color.as_deref())
                    .unwrap_or(&self.data.color.stroke);
                ctx.set_fill_color_alpha(fill_color, cfg.fill_opacity);
                ctx.begin_path();
                ctx.move_to(x3, y_top);
                ctx.line_to(right_x, y_top);
                ctx.line_to(right_x, y_bottom);
                ctx.line_to(x3, y_bottom);
                ctx.close_path();
                ctx.fill();
                ctx.reset_alpha();
            }
        }

        let needs_gap = self.data.text.as_ref()
            .map(|t| !t.content.is_empty() && matches!(t.v_align, TextAlign::Center))
            .unwrap_or(false);

        let (gap_x_start, gap_x_end) = if needs_gap {
            let text = self.data.text.as_ref().unwrap();
            let line_len = right_x - x3;
            if line_len > 0.001 {
                let t_center = match text.h_align {
                    TextAlign::Start => 0.0, TextAlign::Center => 0.5, TextAlign::End => 1.0,
                };
                let char_count = text.content.len() as f64;
                let text_width = char_count * text.font_size * 0.6 + 8.0;
                let half_gap = text_width / 2.0;
                let text_x = x3 + line_len * t_center;
                ((text_x - half_gap).max(x3), (text_x + half_gap).min(right_x))
            } else { (0.0, 0.0) }
        } else { (0.0, 0.0) };

        for cfg in &self.level_configs {
            if !cfg.visible { continue; }

            let level_price = self.price_at_level(cfg.level);
            let y = ctx.price_to_y(level_price);

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

            let is_median = (cfg.level - 0.5).abs() < 0.001;

            let label = if self.show_prices || self.show_percentages {
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
                if self.show_prices { label_parts.push(super::super::fmt_price(level_price)); }
                Some(label_parts.join(" "))
            } else { None };

            let label_gap = if let Some(ref lbl) = label {
                let char_width = 6.5;
                let text_width = lbl.len() as f64 * char_width;
                let line_len = right_x - x3;
                if line_len > 0.001 {
                    let half_gap = text_width / 2.0;
                    match self.label_position.as_str() {
                        "right" => Some((right_x - text_width, right_x)),
                        "center" => {
                            let center_x = (x3 + right_x) / 2.0;
                            Some((center_x - half_gap, center_x + half_gap))
                        }
                        _ => Some((x3, x3 + text_width)),
                    }
                } else { None }
            } else { None };

            if is_median && needs_gap && gap_x_end > gap_x_start {
                ctx.begin_path();
                if gap_x_start > x3 + 0.001 {
                    ctx.move_to(crisp(x3, dpr), crisp(y, dpr));
                    ctx.line_to(crisp(gap_x_start, dpr), crisp(y, dpr));
                }
                ctx.stroke();
                ctx.begin_path();
                if gap_x_end < right_x - 0.001 {
                    ctx.move_to(crisp(gap_x_end, dpr), crisp(y, dpr));
                    ctx.line_to(crisp(right_x, dpr), crisp(y, dpr));
                }
                ctx.stroke();
            } else if let Some((gap_start, gap_end)) = label_gap {
                ctx.begin_path();
                if gap_start > x3 + 0.001 {
                    ctx.move_to(crisp(x3, dpr), crisp(y, dpr));
                    ctx.line_to(crisp(gap_start, dpr), crisp(y, dpr));
                }
                ctx.stroke();
                ctx.begin_path();
                if gap_end < right_x - 0.001 {
                    ctx.move_to(crisp(gap_end, dpr), crisp(y, dpr));
                    ctx.line_to(crisp(right_x, dpr), crisp(y, dpr));
                }
                ctx.stroke();
            } else {
                ctx.begin_path();
                ctx.move_to(crisp(x3, dpr), crisp(y, dpr));
                ctx.line_to(crisp(right_x, dpr), crisp(y, dpr));
                ctx.stroke();
            }

            if let Some(ref lbl) = label {
                ctx.set_font("11px sans-serif");
                ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                ctx.set_fill_color(color);
                match self.label_position.as_str() {
                    "right" => { ctx.set_text_align(crate::render::TextAlign::Right); ctx.fill_text(lbl, right_x, y); }
                    "center" => { ctx.set_text_align(crate::render::TextAlign::Center); ctx.fill_text(lbl, (x3 + right_x) / 2.0, y); }
                    _ => { ctx.set_text_align(crate::render::TextAlign::Left); ctx.fill_text(lbl, x3, y); }
                }
            }
        }
        ctx.set_line_dash(&[]);

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let line_len = right_x - x3;
                let text_x = match text.h_align {
                    TextAlign::Start => x3, TextAlign::Center => x3 + line_len * 0.5, TextAlign::End => right_x,
                };
                let text_y = match text.v_align {
                    TextAlign::Start | TextAlign::End => {
                        let level_ys: Vec<f64> = self.level_configs.iter()
                            .filter(|cfg| cfg.visible)
                            .map(|cfg| ctx.price_to_y(self.price_at_level(cfg.level)))
                            .collect();
                        if level_ys.is_empty() {
                            y3
                        } else {
                            let min_y = level_ys.iter().fold(f64::INFINITY, |a, &b| a.min(b));
                            let max_y = level_ys.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
                            let text_offset = 8.0 + text.font_size / 2.0;
                            if matches!(text.v_align, TextAlign::Start) { min_y - text_offset } else { max_y + text_offset }
                        }
                    }
                    TextAlign::Center => ctx.price_to_y(self.price_at_level(0.5)),
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (px, py) in [(x1, y1), (x2, y2), (x3, y3)] {
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
            ConfigProperty::show_prices(self.show_prices).with_order(10),
            ConfigProperty::levels(self.show_percentages).with_order(11),
            ConfigProperty::show_as_percent(self.show_as_percent).with_order(12),
            ConfigProperty::label_position(&self.label_position).with_order(13),
            ConfigProperty::extend_right(self.extend_right).with_order(20),
            ConfigProperty::show_trend_line(self.show_trend_line).with_order(21),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &super::super::config::PropertyValue) -> bool {
        use super::super::config::PropertyValue;
        match id {
            "show_prices" => { if let PropertyValue::Boolean(v) = value { self.show_prices = *v; return true; } }
            "show_percentages" => { if let PropertyValue::Boolean(v) = value { self.show_percentages = *v; return true; } }
            "show_as_percent" => { if let PropertyValue::Boolean(v) = value { self.show_as_percent = *v; return true; } }
            "label_position" => { if let PropertyValue::String(v) = value { self.label_position = v.clone(); return true; } }
            "extend_right" => { if let PropertyValue::Boolean(v) = value { self.extend_right = *v; return true; } }
            "show_trend_line" => { if let PropertyValue::Boolean(v) = value { self.show_trend_line = *v; return true; } }
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
        type_id: "fib_trend_extension",
        display_name: "Fib Extension",
        kind: PrimitiveKind::Fibonacci,
        click_behavior: ClickBehavior::ThreePoint,
        tooltip: "Fibonacci trend-based extension",
        icon: "fib_trend_extension",
        default_color: "#F7B93E",
        factory: |points, color| {
            let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
            let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 600_000, price1 + 10.0));
            let (ts3, price3) = points.get(2).copied().unwrap_or((ts2 + 300_000, price2 - 5.0));
            Box::new(FibTrendExtension::new(ts1, price1, ts2, price2, ts3, price3, color))
        },
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
