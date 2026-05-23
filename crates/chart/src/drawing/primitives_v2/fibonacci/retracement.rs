//! Fibonacci Retracement primitive
//!
//! Shows horizontal levels at Fibonacci ratios between two price points.
//! Standard levels: 0%, 23.6%, 38.2%, 50%, 61.8%, 78.6%, 100%

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

/// Main/active Fibonacci levels (visible by default)
pub const MAIN_LEVELS: &[f64] = &[0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0];

/// All 30 Fibonacci levels (main + extensions + uncommon)
pub const ALL_LEVELS: &[f64] = &[
    // Basic retracements
    0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0,
    // Extensions
    1.272, 1.414, 1.618, 2.0, 2.272, 2.414, 2.618, 3.0, 3.272, 3.414, 3.618, 4.0, 4.236, 4.618,
    // Additional uncommon levels
    0.146, 0.292, 0.707, 0.886, 1.13, 1.886, 2.886, 5.0, 6.854
];

/// Standard Fibonacci retracement levels (backward compatibility)
pub const DEFAULT_LEVELS: &[f64] = MAIN_LEVELS;

/// Extended Fibonacci levels (including extensions)
pub const EXTENDED_LEVELS: &[f64] = &[0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0, 1.272, 1.618, 2.0, 2.618];

/// Create default level configurations with 30 levels
/// Main levels (0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0) are visible by default
/// Other levels are pre-added but disabled
pub fn default_level_configs() -> Vec<FibLevelConfig> {
    ALL_LEVELS.iter().map(|&level| {
        let mut config = FibLevelConfig::new(level);
        // Only enable main levels by default
        config.visible = MAIN_LEVELS.contains(&level);
        config
    }).collect()
}

/// Create extended level configurations (with extensions)
pub fn extended_level_configs() -> Vec<FibLevelConfig> {
    EXTENDED_LEVELS.iter().map(|&level| FibLevelConfig::new(level)).collect()
}

/// Create level configurations with fills between zones
/// Different colors for different zones
pub fn filled_level_configs() -> Vec<FibLevelConfig> {
    vec![
        FibLevelConfig::with_fill(0.0, Some("#787b86".to_string()), 0.08),
        FibLevelConfig::with_fill(0.236, Some("#f7525f".to_string()), 0.08),
        FibLevelConfig::with_fill(0.382, Some("#22ab94".to_string()), 0.08),
        FibLevelConfig::with_fill(0.5, Some("#2962ff".to_string()), 0.08),
        FibLevelConfig::with_fill(0.618, Some("#ff9800".to_string()), 0.08),
        FibLevelConfig::with_fill(0.786, Some("#9c27b0".to_string()), 0.08),
        FibLevelConfig::new(1.0), // No fill for last level
    ]
}

/// Fibonacci Retracement - horizontal levels at Fib ratios
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibRetracement {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Start timestamp in ms (point 1)
    #[serde(default)]
    pub ts1: i64,
    /// Start price (point 1 - usually swing high or low)
    pub price1: f64,
    /// End timestamp in ms (point 2)
    #[serde(default)]
    pub ts2: i64,
    /// End price (point 2 - usually swing low or high)
    pub price2: f64,
    /// Fibonacci level configurations (with individual colors/widths)
    #[serde(default = "default_level_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    /// Show price labels
    #[serde(default = "default_true")]
    pub show_prices: bool,
    /// Show percentage labels
    #[serde(default = "default_true")]
    pub show_percentages: bool,
    /// Extend lines to left
    #[serde(default)]
    pub extend_left: bool,
    /// Extend lines to right
    #[serde(default = "default_true")]
    pub extend_right: bool,
    /// Fill between levels
    #[serde(default)]
    pub show_fill: bool,
    /// Fill opacity (0.0 to 1.0)
    #[serde(default = "default_fill_opacity")]
    pub fill_opacity: f64,
    /// Label position: "left", "right", "center"
    #[serde(default = "default_label_position")]
    pub label_position: String,
    /// Show levels as percentages (true) or coefficients (false)
    #[serde(default = "default_true")]
    pub show_as_percent: bool,
    /// Show connecting trend line between point 1 and 2
    #[serde(default = "default_true")]
    pub show_trend_line: bool,
}

fn default_true() -> bool { true }
fn default_fill_opacity() -> f64 { 0.1 }
fn default_label_position() -> String { "left".to_string() }

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
            // Convert old format to new format
            Ok(levels.iter().map(|&level| FibLevelConfig::new(level)).collect())
        }
    }
}

impl FibRetracement {
    /// Create a new Fibonacci retracement
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_retracement".to_string(),
                display_name: "Fib Retracement".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            ts1,
            price1,
            ts2,
            price2,
            level_configs: default_level_configs(),
            show_prices: true,
            show_percentages: true,
            extend_left: false,
            extend_right: true,
            show_fill: false,
            fill_opacity: 0.1,
            label_position: "left".to_string(),
            show_as_percent: true,
            show_trend_line: true,
        }
    }

    /// Get the price at a given Fibonacci level
    pub fn price_at_level(&self, level: f64) -> f64 {
        self.price1 + (self.price2 - self.price1) * level
    }

    /// Get all level prices (only visible levels)
    pub fn level_prices(&self) -> Vec<(f64, f64)> {
        self.level_configs
            .iter()
            .filter(|cfg| cfg.visible)
            .map(|cfg| (cfg.level, self.price_at_level(cfg.level)))
            .collect()
    }
}

impl Primitive for FibRetracement {
    fn type_id(&self) -> &'static str {
        "fib_retracement"
    }

    fn display_name(&self) -> &str {
        &self.data.display_name
    }

    fn kind(&self) -> PrimitiveKind {
        PrimitiveKind::Fibonacci
    }

    fn click_behavior(&self) -> ClickBehavior {
        ClickBehavior::TwoPoint
    }

    fn data(&self) -> &PrimitiveData {
        &self.data
    }

    fn data_mut(&mut self) -> &mut PrimitiveData {
        &mut self.data
    }

    fn points(&self) -> Vec<(i64, f64)> {
        vec![(self.ts1, self.price1), (self.ts2, self.price2)]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if let Some(&(ts, price)) = points.first() {
            self.ts1 = ts;
            self.price1 = price;
        }
        if let Some(&(ts, price)) = points.get(1) {
            self.ts2 = ts;
            self.price2 = price;
        }
    }

    fn translate(&mut self, ts_delta: i64, price_delta: f64) {
        self.ts1 += ts_delta;
        self.ts2 += ts_delta;
        self.price1 += price_delta;
        self.price2 += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        match point_type {
            ControlPointType::Point1 => {
                self.ts1 = ts_ms;
                self.price1 = price;
            }
            ControlPointType::Point2 => {
                self.ts2 = ts_ms;
                self.price2 = price;
            }
            ControlPointType::Move => {
                let ts_delta = ts_ms - self.ts1;
                let price_delta = price - self.price1;
                self.translate(ts_delta, price_delta);
            }
            _ => {}
        }
    }

    fn hit_test(
        &self,
        screen_x: f64,
        screen_y: f64,
        bars: &[Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> HitTestResult {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        // Check control points
        if check_point_hit(screen_x, screen_y, x1, y1) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        if check_point_hit(screen_x, screen_y, x2, y2) {
            return HitTestResult::ControlPoint(ControlPointType::Point2);
        }

        // Check each level line (only visible ones)
        let min_x = x1.min(x2);
        let max_x = x1.max(x2);

        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            let level_price = self.price_at_level(cfg.level);
            let level_y = viewport.price_to_y(level_price, price_scale.price_min, price_scale.price_max);

            // Check if within horizontal bounds (or extended)
            let in_bounds = if self.extend_left && self.extend_right {
                true
            } else if self.extend_left {
                screen_x <= max_x
            } else if self.extend_right {
                screen_x >= min_x
            } else {
                screen_x >= min_x && screen_x <= max_x
            };

            if in_bounds && (screen_y - level_y).abs() < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }

        HitTestResult::Miss
    }

    fn control_points(
        &self,
        bars: &[Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Vec<ControlPoint> {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        vec![
            ControlPoint::point1(x1, y1),
            ControlPoint::point2(x2, y2),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let chart_width = ctx.chart_width();

        let left_x = if self.extend_left { 0.0 } else { x1.min(x2) };
        let right_x = if self.extend_right { chart_width } else { x1.max(x2) };

        // Collect visible levels sorted by level value for fill rendering
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

        // Draw fills between adjacent visible levels (before lines so lines are on top)
        for i in 0..visible_levels.len().saturating_sub(1) {
            let (idx, _, y_top) = visible_levels[i];
            let (_, _, y_bottom) = visible_levels[i + 1];

            let cfg = &self.level_configs[idx];
            if cfg.fill_enabled {
                // Use fill_color or fall back to line color
                let fill_color = cfg.fill_color.as_deref()
                    .or(cfg.color.as_deref())
                    .unwrap_or(&self.data.color.stroke);

                // Apply fill with opacity
                ctx.set_fill_color_alpha(fill_color, cfg.fill_opacity);
                ctx.begin_path();
                ctx.move_to(left_x, y_top);
                ctx.line_to(right_x, y_top);
                ctx.line_to(right_x, y_bottom);
                ctx.line_to(left_x, y_bottom);
                ctx.close_path();
                ctx.fill();
                ctx.reset_alpha();
            }
        }

        // Check if we need line gap on median (0.5 level) for v_align == Center
        let needs_gap = self.data.text.as_ref()
            .map(|t| !t.content.is_empty() && matches!(t.v_align, TextAlign::Center))
            .unwrap_or(false);

        // Calculate gap parameters for median line (0.5 level)
        let (gap_x_start, gap_x_end) = if needs_gap {
            let text = self.data.text.as_ref().unwrap();
            let line_len = right_x - left_x;

            if line_len > 0.001 {
                let t_center = match text.h_align {
                    TextAlign::Start => 0.0,
                    TextAlign::Center => 0.5,
                    TextAlign::End => 1.0,
                };

                let char_count = text.content.len() as f64;
                let text_width = char_count * text.font_size * 0.6 + 8.0;
                let half_gap = text_width / 2.0;

                let text_x = left_x + line_len * t_center;
                ((text_x - half_gap).max(left_x), (text_x + half_gap).min(right_x))
            } else {
                (0.0, 0.0)
            }
        } else {
            (0.0, 0.0)
        };

        // Draw each level line with labels
        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            let level_price = self.price_at_level(cfg.level);
            let y = ctx.price_to_y(level_price);

            // Use level-specific color or fall back to main color
            let color = cfg.color.as_deref().unwrap_or(&self.data.color.stroke);

            // Build label text if needed
            let label = if self.show_prices || self.show_percentages {
                let mut label_parts = Vec::new();
                if self.show_percentages {
                    if self.show_as_percent {
                        // Smart format: 0% instead of 0.0%, 100% instead of 100.0%
                        let pct = cfg.level * 100.0;
                        if (pct - pct.round()).abs() < 0.01 {
                            label_parts.push(format!("{}%", pct as i32));
                        } else {
                            label_parts.push(format!("{:.1}%", pct));
                        }
                    } else {
                        // Smart format: 0, 1, 0.5 instead of 0.000, 1.000, 0.500
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
                if self.show_prices {
                    label_parts.push(super::super::fmt_price(level_price));
                }
                Some(label_parts.join(" "))
            } else {
                None
            };

            // Calculate label gap and position (no extra padding - flush to edge)
            let label_gap = if let Some(ref lbl) = label {
                let char_width = 6.5; // approximate
                let text_width = lbl.len() as f64 * char_width;
                let half_gap = text_width / 2.0;

                match self.label_position.as_str() {
                    "right" => {
                        // Gap at right edge - flush
                        let center_x = right_x - half_gap;
                        Some((half_gap, center_x))
                    }
                    "center" => {
                        // Gap at center
                        let center_x = (left_x + right_x) / 2.0;
                        Some((half_gap, center_x))
                    }
                    _ => { // "left"
                        // Gap at left edge - flush
                        let center_x = left_x + half_gap;
                        Some((half_gap, center_x))
                    }
                }
            } else {
                None
            };

            // Set stroke style
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

            // Check if this is the median line (0.5 level) and needs gap for main text
            let is_median = (cfg.level - 0.5).abs() < 0.001;
            let main_text_gap = if is_median && needs_gap && gap_x_end > gap_x_start {
                Some((gap_x_start, gap_x_end))
            } else {
                None
            };

            // Draw line with gaps (for main text and/or center label)
            if let Some((half_gap, center_x)) = label_gap {
                // Center label - draw line with gap
                let gap_start = (center_x - half_gap).max(left_x);
                let gap_end = (center_x + half_gap).min(right_x);

                ctx.begin_path();
                if gap_start > left_x + 0.001 {
                    ctx.move_to(crisp(left_x, dpr), crisp(y, dpr));
                    ctx.line_to(crisp(gap_start, dpr), crisp(y, dpr));
                }
                ctx.stroke();

                ctx.begin_path();
                if gap_end < right_x - 0.001 {
                    ctx.move_to(crisp(gap_end, dpr), crisp(y, dpr));
                    ctx.line_to(crisp(right_x, dpr), crisp(y, dpr));
                }
                ctx.stroke();
            } else if let Some((gap_start, gap_end)) = main_text_gap {
                // Main text gap on median
                ctx.begin_path();
                if gap_start > left_x + 0.001 {
                    ctx.move_to(crisp(left_x, dpr), crisp(y, dpr));
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
                // Full line
                ctx.begin_path();
                ctx.move_to(crisp(left_x, dpr), crisp(y, dpr));
                ctx.line_to(crisp(right_x, dpr), crisp(y, dpr));
                ctx.stroke();
            }

            // Draw label (flush to edge - no padding)
            if let Some(ref lbl) = label {
                ctx.set_font("11px sans-serif");
                ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                ctx.set_fill_color(color);

                match self.label_position.as_str() {
                    "right" => {
                        ctx.set_text_align(crate::render::TextAlign::Right);
                        ctx.fill_text(lbl, right_x, y);
                    }
                    "center" => {
                        ctx.set_text_align(crate::render::TextAlign::Center);
                        ctx.fill_text(lbl, (left_x + right_x) / 2.0, y);
                    }
                    _ => { // "left"
                        ctx.set_text_align(crate::render::TextAlign::Left);
                        ctx.fill_text(lbl, left_x, y);
                    }
                }
            }
        }
        ctx.set_line_dash(&[]);

        // Calculate y coordinates for points (used for trend line and control points)
        let y1 = ctx.price_to_y(self.price1);
        let y2 = ctx.price_to_y(self.price2);

        // Draw connecting line from point 1 to point 2 (if enabled)
        if self.show_trend_line {
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.begin_path();
            ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
            ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
            ctx.stroke();
            ctx.set_line_dash(&[]);
        }

        // Render text if present with proper v_align positioning
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Calculate text X position based on h_align
                let line_len = right_x - left_x;
                let text_x = match text.h_align {
                    TextAlign::Start => left_x,
                    TextAlign::Center => left_x + line_len * 0.5,
                    TextAlign::End => right_x,
                };

                // Calculate text Y position based on v_align:
                // - Start: above upper boundary (topmost visible level = smallest screen Y)
                // - Center: on median (0.5 level)
                // - End: below lower boundary (bottommost visible level = largest screen Y)
                let text_y = match text.v_align {
                    TextAlign::Start | TextAlign::End => {
                        // Find topmost and bottommost visible level Y positions
                        let visible_ys: Vec<f64> = visible_levels.iter()
                            .map(|(_, _, y)| *y)
                            .collect();

                        if visible_ys.is_empty() {
                            (y1 + y2) / 2.0
                        } else {
                            let min_y = visible_ys.iter().fold(f64::INFINITY, |a, &b| a.min(b));
                            let max_y = visible_ys.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));

                            let text_offset = 8.0 + text.font_size / 2.0;
                            if matches!(text.v_align, TextAlign::Start) {
                                min_y - text_offset  // Above topmost (smaller screen Y = higher price)
                            } else {
                                max_y + text_offset  // Below bottommost (larger screen Y = lower price)
                            }
                        }
                    }
                    TextAlign::Center => {
                        // On median (0.5 level)
                        let median_price = self.price_at_level(0.5);
                        ctx.price_to_y(median_price)
                    }
                };

                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            ctx.begin_path();
            ctx.arc(x1, y1, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();

            ctx.begin_path();
            ctx.arc(x2, y2, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();
        }
    }

    fn level_configs(&self) -> Option<Vec<FibLevelConfig>> {
        Some(self.level_configs.clone())
    }

    fn set_level_configs(&mut self, configs: Vec<FibLevelConfig>) -> bool {
        self.level_configs = configs;
        true
    }

    fn style_properties(&self) -> Vec<super::super::config::ConfigProperty> {
        use super::super::config::ConfigProperty;
        vec![
            ConfigProperty::show_prices(self.show_prices).with_order(10),
            ConfigProperty::show_levels(self.show_percentages).with_order(11),
            ConfigProperty::show_as_percent(self.show_as_percent).with_order(12),
            ConfigProperty::label_position(&self.label_position).with_order(13),
            ConfigProperty::extend_left(self.extend_left).with_order(20),
            ConfigProperty::extend_right(self.extend_right).with_order(21),
            ConfigProperty::show_trend_line(self.show_trend_line).with_order(22),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &super::super::config::PropertyValue) -> bool {
        use super::super::config::PropertyValue;
        match id {
            "show_prices" => {
                if let PropertyValue::Boolean(v) = value {
                    self.show_prices = *v;
                    return true;
                }
            }
            "show_percentages" => {
                if let PropertyValue::Boolean(v) = value {
                    self.show_percentages = *v;
                    return true;
                }
            }
            "show_as_percent" => {
                if let PropertyValue::Boolean(v) = value {
                    self.show_as_percent = *v;
                    return true;
                }
            }
            "label_position" => {
                if let PropertyValue::String(v) = value {
                    self.label_position = v.clone();
                    return true;
                }
            }
            "extend_left" => {
                if let PropertyValue::Boolean(v) = value {
                    self.extend_left = *v;
                    return true;
                }
            }
            "extend_right" => {
                if let PropertyValue::Boolean(v) = value {
                    self.extend_right = *v;
                    return true;
                }
            }
            "show_trend_line" => {
                if let PropertyValue::Boolean(v) = value {
                    self.show_trend_line = *v;
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    fn clone_box(&self) -> Box<dyn Primitive> {
        Box::new(self.clone())
    }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    let radius = 8.0;
    (sx - px).powi(2) + (sy - py).powi(2) <= radius * radius
}

// Note: Configurable is now implemented via blanket impl in config.rs
// This provides base configuration (color, width, style, coordinates) automatically.
// Custom properties (show_prices, extend_left, etc.) could be added via a
// separate trait or by extending the base properties in the future.

// =============================================================================
// Factory Registration
// =============================================================================

fn create_fib_retracement(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
    let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, price1));
    Box::new(FibRetracement::new(ts1, price1, ts2, price2, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "fib_retracement",
        display_name: "Fib Retracement",
        kind: PrimitiveKind::Fibonacci,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Fibonacci retracement levels",
        icon: "fib_retracement",
        default_color: "#F7B93E",
        factory: create_fib_retracement,
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
