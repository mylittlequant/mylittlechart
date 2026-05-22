//! Fibonacci Trend Time primitive
//!
//! Vertical lines projected at Fibonacci ratios from a trend.
//! Uses two points to define a time range, then projects Fib levels.

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

/// Fibonacci Trend Time - vertical lines at Fib ratios of time range
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibTrendTime {
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
    #[serde(default = "default_true")]
    pub show_trend_line: bool,
}

fn default_true() -> bool { true }
fn default_label_position() -> String { "left".to_string() }

impl FibTrendTime {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_trend_time".to_string(),
                display_name: "Fib Trend Time".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            ts1, price1, ts2, price2,
            level_configs: default_level_configs(),
            show_labels: true,
            show_percentages: true,
            label_position: "left".to_string(),
            show_as_percent: true,
            show_trend_line: true,
        }
    }

    /// Get timestamp for a level (interpolated between ts1 and ts2)
    pub fn ts_at_level(&self, level: f64) -> i64 {
        self.ts1 + ((self.ts2 - self.ts1) as f64 * level) as i64
    }
}

impl Primitive for FibTrendTime {
    fn type_id(&self) -> &'static str { "fib_trend_time" }
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

        if point_to_line_distance(sx, sy, x1, y1, x2, y2) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        for cfg in &self.level_configs {
            if !cfg.visible { continue; }
            let level_ts = self.ts_at_level(cfg.level);
            let level_b = timestamp_ms_to_bar_f64(bars, level_ts);
            let level_x = vp.bar_to_x_f64(level_b);
            if (sx - level_x).abs() < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
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
        let chart_height = ctx.chart_height();

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
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.begin_path();
            ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
            ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
            ctx.stroke();
        }

        // === FILL ===
        let mut visible_levels: Vec<(usize, f64, f64)> = self.level_configs
            .iter()
            .enumerate()
            .filter(|(_, cfg)| cfg.visible)
            .map(|(idx, cfg)| {
                let level_ts = self.ts_at_level(cfg.level);
                let level_x = ctx.ts_to_x_ms(level_ts);
                (idx, cfg.level, level_x)
            })
            .collect();
        visible_levels.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        for i in 0..visible_levels.len().saturating_sub(1) {
            let (idx, _, lx1) = visible_levels[i];
            let (_, _, lx2) = visible_levels[i + 1];
            let cfg = &self.level_configs[idx];
            if cfg.fill_enabled {
                let fill_color = cfg.fill_color.as_deref()
                    .or(cfg.color.as_deref())
                    .unwrap_or(&self.data.color.stroke);
                ctx.set_fill_color_alpha(fill_color, cfg.fill_opacity);
                ctx.begin_path();
                ctx.move_to(lx1, 0.0);
                ctx.line_to(lx2, 0.0);
                ctx.line_to(lx2, chart_height);
                ctx.line_to(lx1, chart_height);
                ctx.close_path();
                ctx.fill();
                ctx.reset_alpha();
            }
        }

        for cfg in &self.level_configs {
            if !cfg.visible { continue; }

            let level_ts = self.ts_at_level(cfg.level);
            let level_x = ctx.ts_to_x_ms(level_ts);

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

            let (label, label_y, gap_half_height) = if self.show_labels || self.show_percentages {
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
                let lbl = label_parts.join(" ");
                if !lbl.is_empty() {
                    let (ly, gap_h) = match self.label_position.as_str() {
                        "right" | "center" => (chart_height / 2.0, 8.0),
                        _ => (4.0 + 6.0, 8.0),
                    };
                    (Some(lbl), ly, gap_h)
                } else { (None, 0.0, 0.0) }
            } else { (None, 0.0, 0.0) };

            ctx.begin_path();
            if label.is_some() && gap_half_height > 0.0 {
                let gap_y_start = label_y - gap_half_height;
                let gap_y_end = label_y + gap_half_height;
                if gap_y_start > 0.0 {
                    ctx.move_to(crisp(level_x, dpr), 0.0);
                    ctx.line_to(crisp(level_x, dpr), gap_y_start);
                }
                if gap_y_end < chart_height {
                    ctx.move_to(crisp(level_x, dpr), gap_y_end);
                    ctx.line_to(crisp(level_x, dpr), chart_height);
                }
            } else {
                ctx.move_to(crisp(level_x, dpr), 0.0);
                ctx.line_to(crisp(level_x, dpr), chart_height);
            }
            ctx.stroke();

            if let Some(ref lbl) = label {
                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(color);
                ctx.set_text_align(crate::render::TextAlign::Center);
                match self.label_position.as_str() {
                    "right" | "center" => {
                        ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                        ctx.fill_text(lbl, level_x, chart_height / 2.0);
                    }
                    _ => {
                        ctx.set_text_baseline(crate::render::TextBaseline::Top);
                        ctx.fill_text(lbl, level_x, 4.0);
                    }
                }
            }
        }
        ctx.set_line_dash(&[]);

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_x = match text.h_align {
                    TextAlign::Start => x1,
                    TextAlign::Center => {
                        let median_ts = self.ts_at_level(0.5);
                        ctx.ts_to_x_ms(median_ts)
                    }
                    TextAlign::End => x2,
                };
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_y = match text.v_align {
                    TextAlign::Start => text_offset,
                    TextAlign::Center => chart_height / 2.0,
                    TextAlign::End => chart_height - text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
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
            ConfigProperty::show_trend_line(self.show_trend_line).with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &super::super::config::PropertyValue) -> bool {
        use super::super::config::PropertyValue;
        match id {
            "show_labels" => { if let PropertyValue::Boolean(v) = value { self.show_labels = *v; return true; } }
            "show_percentages" => { if let PropertyValue::Boolean(v) = value { self.show_percentages = *v; return true; } }
            "show_as_percent" => { if let PropertyValue::Boolean(v) = value { self.show_as_percent = *v; return true; } }
            "label_position" => { if let PropertyValue::String(v) = value { self.label_position = v.clone(); return true; } }
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
        type_id: "fib_trend_time",
        display_name: "Fib Trend Time",
        kind: PrimitiveKind::Fibonacci,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Fibonacci trend-based time projection",
        icon: "fib_trend_time",
        default_color: "#F7B93E",
        factory: |points, color| {
            let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
            let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, price1));
            Box::new(FibTrendTime::new(ts1, price1, ts2, price2, color))
        },
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
