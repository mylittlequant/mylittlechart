//! Fibonacci Time Zones primitive
//!
//! Vertical lines at Fibonacci intervals from a starting point.
//! Shows time-based projections: 1, 2, 3, 5, 8, 13, 21, 34, 55, 89 bars...

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64, bar_interval_seconds};
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

pub const MAIN_TIME_ZONES: &[f64] = &[1.0, 2.0, 3.0, 5.0, 8.0, 13.0, 21.0, 34.0, 55.0, 89.0, 144.0, 233.0];

pub const ALL_TIME_ZONES: &[f64] = &[
    1.0, 2.0, 3.0, 5.0, 8.0, 13.0, 21.0, 34.0, 55.0, 89.0, 144.0, 233.0,
    377.0, 610.0, 987.0, 1597.0, 2584.0, 4181.0,
];

pub fn default_time_zone_configs() -> Vec<FibLevelConfig> {
    ALL_TIME_ZONES.iter().map(|&level| {
        let mut config = FibLevelConfig::new(level);
        config.visible = MAIN_TIME_ZONES.contains(&level);
        config
    }).collect()
}

fn default_true() -> bool { true }
fn default_label_position() -> String { "left".to_string() }

/// Fibonacci Time Zones - vertical lines at Fib intervals
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibTimeZones {
    pub data: PrimitiveData,
    /// Starting timestamp (ms)
    pub start_ts: i64,
    pub start_price: f64,
    /// level_configs: level = bar-count offset from start (e.g. 1.0, 2.0, 3.0, 5.0...)
    #[serde(default = "default_time_zone_configs", deserialize_with = "deserialize_level_configs")]
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

impl FibTimeZones {
    pub fn new(start_ts: i64, start_price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_time_zones".to_string(),
                display_name: "Fib Time Zones".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            start_ts, start_price,
            level_configs: default_time_zone_configs(),
            show_labels: true,
            show_percentages: true,
            label_position: "left".to_string(),
            show_as_percent: true,
        }
    }

    /// Get timestamps for visible zones. `bar_interval_ms` = milliseconds per bar.
    pub fn zone_timestamps(&self, bar_interval_ms: i64) -> Vec<(i64, &FibLevelConfig)> {
        self.level_configs
            .iter()
            .filter(|cfg| cfg.visible)
            .map(|cfg| {
                let ts = self.start_ts + (cfg.level * bar_interval_ms as f64) as i64;
                (ts, cfg)
            })
            .collect()
    }
}

impl Primitive for FibTimeZones {
    fn type_id(&self) -> &'static str { "fib_time_zones" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Fibonacci }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::SingleClick }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> {
        vec![(self.start_ts, self.start_price)]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if let Some(&(ts, price)) = points.first() {
            self.start_ts = ts;
            self.start_price = price;
        }
    }

    fn translate(&mut self, td: i64, pd: f64) {
        self.start_ts += td;
        self.start_price += pd;
    }

    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 | ControlPointType::Move => {
                self.start_ts = ts_ms;
                self.start_price = price;
            }
            _ => {}
        }
    }

    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let start_b = timestamp_ms_to_bar_f64(bars, self.start_ts);
        let start_x = vp.bar_to_x_f64(start_b);
        let start_y = vp.price_to_y(self.start_price, ps.price_min, ps.price_max);

        if check_point_hit(sx, sy, start_x, start_y) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }

        let bar_interval_ms = bar_interval_seconds(bars) * 1000;

        for (zone_ts, _cfg) in self.zone_timestamps(bar_interval_ms) {
            let zone_b = timestamp_ms_to_bar_f64(bars, zone_ts);
            let zone_x = vp.bar_to_x_f64(zone_b);
            if (sx - zone_x).abs() < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }

        if (sx - start_x).abs() < HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let start_b = timestamp_ms_to_bar_f64(bars, self.start_ts);
        let x = vp.bar_to_x_f64(start_b);
        let y = vp.price_to_y(self.start_price, ps.price_min, ps.price_max);
        vec![ControlPoint::point1(x, y)]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let start_x = ctx.ts_to_x_ms(self.start_ts);
        let start_y = ctx.price_to_y(self.start_price);
        let chart_height = ctx.chart_height();

        // Compute bar interval from ctx.bars()
        let bar_interval_ms = bar_interval_seconds(ctx.bars()) * 1000;

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
        ctx.move_to(crisp(start_x, dpr), 0.0);
        ctx.line_to(crisp(start_x, dpr), chart_height);
        ctx.stroke();

        let zone_data: Vec<(i64, &FibLevelConfig)> = self.zone_timestamps(bar_interval_ms);

        let mut visible_zones: Vec<(usize, i64, f64)> = zone_data
            .iter()
            .enumerate()
            .filter(|(_, (_, cfg))| cfg.visible)
            .map(|(idx, (zone_ts, _))| {
                let zone_x = ctx.ts_to_x_ms(*zone_ts);
                (idx, *zone_ts, zone_x)
            })
            .collect();
        visible_zones.sort_by(|a, b| a.1.cmp(&b.1));

        // Fill between adjacent zones
        for i in 0..visible_zones.len().saturating_sub(1) {
            let (idx, _, x1) = visible_zones[i];
            let (_, _, x2) = visible_zones[i + 1];
            if idx < zone_data.len() {
                let cfg = zone_data[idx].1;
                if cfg.fill_enabled {
                    let fill_color = cfg.fill_color.as_deref()
                        .or(cfg.color.as_deref())
                        .unwrap_or(&self.data.color.stroke);
                    ctx.set_fill_color_alpha(fill_color, cfg.fill_opacity);
                    ctx.begin_path();
                    ctx.move_to(x1, 0.0);
                    ctx.line_to(x2, 0.0);
                    ctx.line_to(x2, chart_height);
                    ctx.line_to(x1, chart_height);
                    ctx.close_path();
                    ctx.fill();
                    ctx.reset_alpha();
                }
            }
        }

        for (zone_ts, cfg) in self.zone_timestamps(bar_interval_ms) {
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

            let zone_x = ctx.ts_to_x_ms(zone_ts);

            let (label, label_y, gap_half_height) = if self.show_labels || self.show_percentages {
                let lbl = {
                    let mut label_parts = Vec::new();
                    if self.show_percentages {
                        let lvl = cfg.level;
                        if (lvl - lvl.round()).abs() < 0.0001 { label_parts.push(format!("{}", lvl as i32)); }
                        else { label_parts.push(format!("{:.1}", lvl)); }
                    }
                    label_parts.join(" ")
                };
                if !lbl.is_empty() {
                    let (ly, gap_h) = match self.label_position.as_str() {
                        "right" | "center" => (chart_height / 2.0, 8.0),
                        _ => (4.0 + 6.0, 8.0),
                    };
                    (Some(lbl), ly, gap_h)
                } else {
                    (None, 0.0, 0.0)
                }
            } else {
                (None, 0.0, 0.0)
            };

            ctx.begin_path();
            if label.is_some() && gap_half_height > 0.0 {
                let gap_y_start = label_y - gap_half_height;
                let gap_y_end = label_y + gap_half_height;
                if gap_y_start > 0.0 {
                    ctx.move_to(crisp(zone_x, dpr), 0.0);
                    ctx.line_to(crisp(zone_x, dpr), gap_y_start);
                }
                if gap_y_end < chart_height {
                    ctx.move_to(crisp(zone_x, dpr), gap_y_end);
                    ctx.line_to(crisp(zone_x, dpr), chart_height);
                }
            } else {
                ctx.move_to(crisp(zone_x, dpr), 0.0);
                ctx.line_to(crisp(zone_x, dpr), chart_height);
            }
            ctx.stroke();

            if let Some(ref lbl) = label {
                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(color);
                ctx.set_text_align(crate::render::TextAlign::Center);
                match self.label_position.as_str() {
                    "right" | "center" => {
                        ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                        ctx.fill_text(lbl, zone_x, chart_height / 2.0);
                    }
                    _ => {
                        ctx.set_text_baseline(crate::render::TextBaseline::Top);
                        ctx.fill_text(lbl, zone_x, 4.0);
                    }
                }
            }
        }
        ctx.set_line_dash(&[]);

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let zone_tss: Vec<i64> = self.zone_timestamps(bar_interval_ms).into_iter().map(|(ts, _)| ts).collect();
                let zone_count = zone_tss.len();
                let text_x = if zone_count == 0 {
                    start_x
                } else {
                    match text.h_align {
                        TextAlign::Start => ctx.ts_to_x_ms(zone_tss[0]),
                        TextAlign::Center => {
                            let mid_idx = zone_count / 2;
                            ctx.ts_to_x_ms(zone_tss[mid_idx])
                        }
                        TextAlign::End => ctx.ts_to_x_ms(*zone_tss.last().unwrap()),
                    }
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
            ctx.begin_path();
            ctx.arc(start_x, start_y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
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
        type_id: "fib_time_zones",
        display_name: "Fib Time Zones",
        kind: PrimitiveKind::Fibonacci,
        click_behavior: ClickBehavior::SingleClick,
        tooltip: "Vertical lines at Fibonacci time intervals",
        icon: "fib_time_zones",
        default_color: "#F7B93E",
        factory: |points, color| {
            let (ts, price) = points.first().copied().unwrap_or((0, 0.0));
            Box::new(FibTimeZones::new(ts, price, color))
        },
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
