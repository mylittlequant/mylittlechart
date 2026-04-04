//! Andrew's Pitchfork primitive
//!
//! A three-point technical analysis tool consisting of a median line
//! with parallel upper and lower trendlines.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    HIT_TOLERANCE, CONTROL_POINT_RADIUS,
    RenderContext, crisp, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text_rotated, calculate_line_text_params,
    config::{FibLevelConfig, ConfigProperty, PropertyValue, PropertyCategory},
};

/// Level mode for Pitchfork - determines which levels are shown
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum PitchforkLevelMode {
    /// Standard quarter levels only (0.25, 0.5, 0.75, 1.0, etc.)
    Base,
    /// Fibonacci ratio levels only (0.236, 0.382, 0.618, 0.786, etc.)
    Fibonacci,
    /// Both standard and Fibonacci levels
    #[default]
    Both,
}

impl PitchforkLevelMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PitchforkLevelMode::Base => "base",
            PitchforkLevelMode::Fibonacci => "fibonacci",
            PitchforkLevelMode::Both => "both",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "base" => PitchforkLevelMode::Base,
            "fibonacci" => PitchforkLevelMode::Fibonacci,
            _ => PitchforkLevelMode::Both,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            PitchforkLevelMode::Base => "Базовые",
            PitchforkLevelMode::Fibonacci => "Фибоначчи",
            PitchforkLevelMode::Both => "Все",
        }
    }
}

/// All 24 pitchfork levels (positive only, will be mirrored to ± when rendering)
/// Mix of standard quarters and Fibonacci ratios
pub const ALL_PITCHFORK_LEVELS: &[f64] = &[
    // Standard quarters
    0.25, 0.5, 0.75, 1.0,
    // Extensions
    1.25, 1.5, 1.75, 2.0,
    // Fibonacci ratios
    0.236, 0.382, 0.618, 0.786,
    // Fib extensions
    1.272, 1.414, 1.618, 2.618,
    // Outer extensions
    2.5, 3.0, 3.5, 4.0,
    // Additional
    0.146, 0.886, 4.236, 4.618,
];

/// Base (standard quarter) levels
pub const BASE_LEVELS: &[f64] = &[
    0.25, 0.5, 0.75, 1.0,
    1.25, 1.5, 1.75, 2.0,
    2.5, 3.0, 3.5, 4.0,
];

/// Fibonacci ratio levels
pub const FIBONACCI_LEVELS: &[f64] = &[
    0.146, 0.236, 0.382, 0.618, 0.786, 0.886,
    1.272, 1.414, 1.618, 2.618, 4.236, 4.618,
];

/// Check if a level is a base (quarter) level
fn is_base_level(level: f64) -> bool {
    BASE_LEVELS.iter().any(|&l| (l - level).abs() < 0.001)
}

/// Check if a level is a Fibonacci level
fn is_fibonacci_level(level: f64) -> bool {
    FIBONACCI_LEVELS.iter().any(|&l| (l - level).abs() < 0.001)
}

/// Main levels visible by default
pub const MAIN_VISIBLE: &[f64] = &[0.5, 1.0];

/// Backward compatibility alias
pub const DEFAULT_PITCHFORK_LEVELS: &[f64] = &[-1.0, -0.5, 0.0, 0.5, 1.0];
pub const MAIN_PITCHFORK_LEVELS: &[f64] = DEFAULT_PITCHFORK_LEVELS;

/// Create default level configs for pitchfork (24 levels, only main visible)
pub fn default_level_configs() -> Vec<FibLevelConfig> {
    ALL_PITCHFORK_LEVELS.iter().map(|&level| {
        let mut config = FibLevelConfig::new(level);
        config.visible = MAIN_VISIBLE.contains(&level);
        config
    }).collect()
}

/// Deserialize level configs with backward compatibility for old `levels: Vec<f64>` format
fn deserialize_level_configs<'de, D>(deserializer: D) -> Result<Vec<FibLevelConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, SeqAccess, Visitor};

    struct LevelConfigsVisitor;

    impl<'de> Visitor<'de> for LevelConfigsVisitor {
        type Value = Vec<FibLevelConfig>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a sequence of FibLevelConfig objects or f64 level values")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut configs = Vec::new();

            while let Some(value) = seq.next_element::<serde_json::Value>()? {
                // Try to parse as FibLevelConfig first
                if value.is_object() {
                    let config: FibLevelConfig = serde_json::from_value(value)
                        .map_err(de::Error::custom)?;
                    configs.push(config);
                } else if let Some(level) = value.as_f64() {
                    // Backward compatibility: old format was just f64 levels
                    configs.push(FibLevelConfig::new(level));
                } else {
                    return Err(de::Error::custom("expected FibLevelConfig object or f64"));
                }
            }

            Ok(configs)
        }
    }

    deserializer.deserialize_seq(LevelConfigsVisitor)
}

/// Andrew's Pitchfork
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pitchfork {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Point 1 - the handle (starting pivot)
    pub bar1: f64,
    pub price1: f64,
    /// Point 2 - first swing (usually a high/low)
    pub bar2: f64,
    pub price2: f64,
    /// Point 3 - second swing (opposite of point 2)
    pub bar3: f64,
    pub price3: f64,
    /// Pitchfork level configurations (positive values only, rendered as ±)
    /// Median (0) is always rendered automatically
    #[serde(default = "default_level_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    /// Extend lines
    #[serde(default = "default_true")]
    pub extend: bool,
    /// Show level labels
    #[serde(default = "default_true")]
    pub show_labels: bool,
    /// Level mode - determines which type of levels are visible
    #[serde(default)]
    pub level_mode: PitchforkLevelMode,
    /// Show percentage/level labels
    #[serde(default = "default_true")]
    pub show_percentages: bool,
    /// Label position: "left", "right", "center"
    #[serde(default = "default_label_position")]
    pub label_position: String,
    /// Show levels as percentages (true) or coefficients (false)
    #[serde(default = "default_true")]
    pub show_as_percent: bool,
}

fn default_true() -> bool { true }
fn default_label_position() -> String { "left".to_string() }

impl Pitchfork {
    /// Create a new pitchfork
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, bar3: f64, price3: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "pitchfork".to_string(),
                display_name: "Pitchfork".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            bar1,
            price1,
            bar2,
            price2,
            bar3,
            price3,
            level_configs: default_level_configs(),
            extend: true,
            show_labels: true,
            level_mode: PitchforkLevelMode::Both,
            show_percentages: true,
            label_position: "left".to_string(),
            show_as_percent: true,
        }
    }

    /// Check if a level should be shown based on current level_mode
    fn should_show_level(&self, level: f64) -> bool {
        match self.level_mode {
            PitchforkLevelMode::Base => is_base_level(level),
            PitchforkLevelMode::Fibonacci => is_fibonacci_level(level),
            PitchforkLevelMode::Both => true,
        }
    }

    /// Get effective levels for rendering: median (0) + symmetric ±levels
    /// Returns tuples of (level_value, config) for each line to draw
    /// Filters by level_mode setting
    pub fn effective_levels(&self) -> Vec<(f64, &FibLevelConfig)> {
        let mut result = Vec::new();

        for config in &self.level_configs {
            if config.visible && self.should_show_level(config.level) {
                // Positive level
                result.push((config.level, config));
                // Mirror to negative (unless it's 0)
                if config.level.abs() > 0.001 {
                    result.push((-config.level, config));
                }
            }
        }

        result
    }

    /// Get visible levels as f64 values (for hit testing)
    /// Includes median (0) and symmetric ±levels
    /// Filters by level_mode setting
    pub fn visible_levels(&self) -> Vec<f64> {
        let mut levels = vec![0.0]; // Median always included
        for config in &self.level_configs {
            if config.visible && self.should_show_level(config.level) {
                levels.push(config.level);
                if config.level.abs() > 0.001 {
                    levels.push(-config.level);
                }
            }
        }
        levels
    }

    /// Get the midpoint between points 2 and 3
    pub fn midpoint(&self) -> (f64, f64) {
        (
            (self.bar2 + self.bar3) / 2.0,
            (self.price2 + self.price3) / 2.0,
        )
    }

    /// Get the channel width (half distance from point 2 to point 3)
    pub fn channel_offset(&self) -> (f64, f64) {
        (
            (self.bar3 - self.bar2) / 2.0,
            (self.price3 - self.price2) / 2.0,
        )
    }

    /// Get median line direction
    pub fn median_direction(&self) -> (f64, f64) {
        let (mid_bar, mid_price) = self.midpoint();
        (mid_bar - self.bar1, mid_price - self.price1)
    }
}

impl Primitive for Pitchfork {
    fn type_id(&self) -> &'static str {
        "pitchfork"
    }

    fn display_name(&self) -> &str {
        &self.data.display_name
    }

    fn kind(&self) -> PrimitiveKind {
        PrimitiveKind::Channel
    }

    fn click_behavior(&self) -> ClickBehavior {
        ClickBehavior::ThreePoint
    }

    fn data(&self) -> &PrimitiveData {
        &self.data
    }

    fn data_mut(&mut self) -> &mut PrimitiveData {
        &mut self.data
    }

    fn points(&self) -> Vec<(f64, f64)> {
        vec![
            (self.bar1, self.price1),
            (self.bar2, self.price2),
            (self.bar3, self.price3),
        ]
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        if let Some(&(bar, price)) = points.first() {
            self.bar1 = bar;
            self.price1 = price;
        }
        if let Some(&(bar, price)) = points.get(1) {
            self.bar2 = bar;
            self.price2 = price;
        }
        if let Some(&(bar, price)) = points.get(2) {
            self.bar3 = bar;
            self.price3 = price;
        }
    }

    fn translate(&mut self, bar_delta: f64, price_delta: f64) {
        self.bar1 += bar_delta;
        self.bar2 += bar_delta;
        self.bar3 += bar_delta;
        self.price1 += price_delta;
        self.price2 += price_delta;
        self.price3 += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, bar: f64, price: f64) {
        match point_type {
            ControlPointType::Point1 => {
                self.bar1 = bar;
                self.price1 = price;
            }
            ControlPointType::Point2 => {
                self.bar2 = bar;
                self.price2 = price;
            }
            ControlPointType::Point3 => {
                self.bar3 = bar;
                self.price3 = price;
            }
            ControlPointType::Move => {
                let bar_delta = bar - self.bar1;
                let price_delta = price - self.price1;
                self.translate(bar_delta, price_delta);
            }
            _ => {}
        }
    }

    fn hit_test(
        &self,
        screen_x: f64,
        screen_y: f64,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> HitTestResult {
        let x1 = viewport.bar_to_x_f64(self.bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(self.bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);
        let x3 = viewport.bar_to_x_f64(self.bar3);
        let y3 = viewport.price_to_y(self.price3, price_scale.price_min, price_scale.price_max);

        // Check control points
        if check_point_hit(screen_x, screen_y, x1, y1) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        if check_point_hit(screen_x, screen_y, x2, y2) {
            return HitTestResult::ControlPoint(ControlPointType::Point2);
        }
        if check_point_hit(screen_x, screen_y, x3, y3) {
            return HitTestResult::ControlPoint(ControlPointType::Point3);
        }

        let (mid_bar, mid_price) = self.midpoint();
        let mid_x = viewport.bar_to_x_f64(mid_bar);
        let mid_y = viewport.price_to_y(mid_price, price_scale.price_min, price_scale.price_max);

        let (offset_bar, offset_price) = self.channel_offset();
        let offset_x = viewport.bar_to_x_f64(mid_bar + offset_bar) - mid_x;
        let offset_y = viewport.price_to_y(mid_price + offset_price, price_scale.price_min, price_scale.price_max) - mid_y;

        // Check median line (always present)
        {
            let dist = if self.extend {
                point_to_ray_distance(screen_x, screen_y, x1, y1, mid_x, mid_y)
            } else {
                point_to_line_distance(screen_x, screen_y, x1, y1, mid_x, mid_y)
            };
            if dist < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }

        // Check each visible pitchfork line (symmetric ±levels)
        for (level, _config) in self.effective_levels() {
            let start_x = x1 + offset_x * level;
            let start_y = y1 + offset_y * level;
            let end_x = mid_x + offset_x * level;
            let end_y = mid_y + offset_y * level;

            let dist = if self.extend {
                point_to_ray_distance(screen_x, screen_y, start_x, start_y, end_x, end_y)
            } else {
                point_to_line_distance(screen_x, screen_y, start_x, start_y, end_x, end_y)
            };

            if dist < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }

        // Check connecting lines from handle to points 2 and 3
        if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        if point_to_line_distance(screen_x, screen_y, x1, y1, x3, y3) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        HitTestResult::Miss
    }

    fn control_points(
        &self,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Vec<ControlPoint> {
        let x1 = viewport.bar_to_x_f64(self.bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(self.bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);
        let x3 = viewport.bar_to_x_f64(self.bar3);
        let y3 = viewport.price_to_y(self.price3, price_scale.price_min, price_scale.price_max);

        vec![
            ControlPoint::point1(x1, y1),
            ControlPoint::point2(x2, y2),
            ControlPoint::point3(x3, y3),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.bar_to_x(self.bar1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.bar_to_x(self.bar2);
        let y2 = ctx.price_to_y(self.price2);
        let x3 = ctx.bar_to_x(self.bar3);
        let y3 = ctx.price_to_y(self.price3);
        let chart_width = ctx.chart_width();

        let (mid_bar, mid_price) = self.midpoint();
        let mid_x = ctx.bar_to_x(mid_bar);
        let mid_y = ctx.price_to_y(mid_price);

        let (offset_bar, offset_price) = self.channel_offset();
        let offset_x = ctx.bar_to_x(mid_bar + offset_bar) - mid_x;
        let offset_y = ctx.price_to_y(mid_price + offset_price) - mid_y;

        // === FILL RENDERING (before lines so lines are on top) ===
        // For pitchfork, fill is drawn between +level and -level (symmetric channel)
        for config in &self.level_configs {
            if config.visible && config.fill_enabled && config.level.abs() > 0.001 {
                let level = config.level;
                let fill_color = config.fill_color.as_deref()
                    .or(config.color.as_deref())
                    .unwrap_or(&self.data.color.stroke);

                // Calculate corners of the fill quad between +level and -level
                // Start points (at handle)
                let pos_start_x = x1 + offset_x * level;
                let pos_start_y = y1 + offset_y * level;
                let neg_start_x = x1 + offset_x * (-level);
                let neg_start_y = y1 + offset_y * (-level);

                // End points (extended or at midpoint)
                let (pos_end_x, pos_end_y, neg_end_x, neg_end_y) = if self.extend {
                    // Extend to chart edge
                    let pos_dx = (mid_x + offset_x * level) - pos_start_x;
                    let pos_dy = (mid_y + offset_y * level) - pos_start_y;
                    let pos_len = (pos_dx * pos_dx + pos_dy * pos_dy).sqrt();

                    let neg_dx = (mid_x + offset_x * (-level)) - neg_start_x;
                    let neg_dy = (mid_y + offset_y * (-level)) - neg_start_y;
                    let neg_len = (neg_dx * neg_dx + neg_dy * neg_dy).sqrt();

                    let ext = chart_width * 2.0;
                    let (pex, pey) = if pos_len > 0.0 {
                        (pos_start_x + pos_dx / pos_len * ext, pos_start_y + pos_dy / pos_len * ext)
                    } else {
                        (pos_start_x, pos_start_y)
                    };
                    let (nex, ney) = if neg_len > 0.0 {
                        (neg_start_x + neg_dx / neg_len * ext, neg_start_y + neg_dy / neg_len * ext)
                    } else {
                        (neg_start_x, neg_start_y)
                    };
                    (pex, pey, nex, ney)
                } else {
                    (mid_x + offset_x * level, mid_y + offset_y * level,
                     mid_x + offset_x * (-level), mid_y + offset_y * (-level))
                };

                // Draw fill quad
                ctx.set_fill_color_alpha(fill_color, config.fill_opacity);
                ctx.begin_path();
                ctx.move_to(pos_start_x, pos_start_y);
                ctx.line_to(pos_end_x, pos_end_y);
                ctx.line_to(neg_end_x, neg_end_y);
                ctx.line_to(neg_start_x, neg_start_y);
                ctx.close_path();
                ctx.fill();
                ctx.reset_alpha();
            }
        }

        // Check if we need line gap on median (only for v_align == Center)
        let needs_gap = self.data.text.as_ref()
            .map(|t| !t.content.is_empty() && matches!(t.v_align, super::super::TextAlign::Center))
            .unwrap_or(false);

        // Calculate gap parameters for median line
        let (gap_t_start, gap_t_end) = if needs_gap {
            let text = self.data.text.as_ref().unwrap();
            let dx = mid_x - x1;
            let dy = mid_y - y1;
            let base_len = (dx * dx + dy * dy).sqrt();

            if base_len > 0.001 {
                let t_center = match text.h_align {
                    super::super::TextAlign::Start => 0.0,
                    super::super::TextAlign::Center => 0.5,
                    super::super::TextAlign::End => 1.0,
                };

                let char_count = text.content.len() as f64;
                let text_width = char_count * text.font_size * 0.6 + 8.0;
                let half_gap_t = (text_width / 2.0) / base_len;

                ((t_center - half_gap_t).max(0.0), (t_center + half_gap_t).min(1.0))
            } else {
                (0.0, 0.0)
            }
        } else {
            (0.0, 0.0)
        };

        // Helper macro-like function to draw a line at given level
        fn draw_pitchfork_line(
            ctx: &mut dyn RenderContext, dpr: f64, chart_width: f64, extend: bool,
            x1: f64, y1: f64, mid_x: f64, mid_y: f64,
            offset_x: f64, offset_y: f64, level: f64,
            with_gap: bool, gap_t_start: f64, gap_t_end: f64,
        ) {
            let start_x = x1 + offset_x * level;
            let start_y = y1 + offset_y * level;
            let end_x = mid_x + offset_x * level;
            let end_y = mid_y + offset_y * level;

            ctx.begin_path();

            if with_gap && gap_t_end > gap_t_start {
                let dx = end_x - start_x;
                let dy = end_y - start_y;
                let base_len = (dx * dx + dy * dy).sqrt();

                if gap_t_start > 0.001 {
                    let gap_x1 = start_x + dx * gap_t_start;
                    let gap_y1 = start_y + dy * gap_t_start;
                    ctx.move_to(crisp(start_x, dpr), crisp(start_y, dpr));
                    ctx.line_to(crisp(gap_x1, dpr), crisp(gap_y1, dpr));
                }

                let gap_x2 = start_x + dx * gap_t_end;
                let gap_y2 = start_y + dy * gap_t_end;

                if extend && base_len > 0.0 {
                    let ext = chart_width * 2.0;
                    let nx = dx / base_len;
                    let ny = dy / base_len;
                    ctx.move_to(crisp(gap_x2, dpr), crisp(gap_y2, dpr));
                    ctx.line_to(crisp(start_x + nx * ext, dpr), crisp(start_y + ny * ext, dpr));
                } else if gap_t_end < 0.999 {
                    ctx.move_to(crisp(gap_x2, dpr), crisp(gap_y2, dpr));
                    ctx.line_to(crisp(end_x, dpr), crisp(end_y, dpr));
                }
            } else {
                ctx.move_to(crisp(start_x, dpr), crisp(start_y, dpr));

                if extend {
                    let dx = end_x - start_x;
                    let dy = end_y - start_y;
                    let len = (dx * dx + dy * dy).sqrt();
                    if len > 0.0 {
                        let ext = chart_width * 2.0;
                        let nx = dx / len;
                        let ny = dy / len;
                        ctx.line_to(crisp(start_x + nx * ext, dpr), crisp(start_y + ny * ext, dpr));
                    }
                } else {
                    ctx.line_to(crisp(end_x, dpr), crisp(end_y, dpr));
                }
            }
            ctx.stroke();
        }

        // 1. Draw median line (level 0) - always present
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }
        draw_pitchfork_line(ctx, dpr, chart_width, self.extend,
            x1, y1, mid_x, mid_y, offset_x, offset_y, 0.0,
            needs_gap, gap_t_start, gap_t_end);

        // 2. Draw symmetric ±levels from effective_levels()
        for (level, config) in self.effective_levels() {
            let stroke_color = config.color.as_ref().unwrap_or(&self.data.color.stroke);
            ctx.set_stroke_color(stroke_color);

            let stroke_width = config.width.unwrap_or(self.data.width);
            ctx.set_stroke_width(stroke_width);

            let line_style = match config.style.as_str() {
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

            // Build label and calculate gap for this level
            let (label, label_gap_t_start, label_gap_t_end) = if self.show_labels || self.show_percentages {
                let label = {
                    let mut label_parts = Vec::new();
                    if self.show_percentages {
                        if self.show_as_percent {
                            let pct = level * 100.0;
                            if (pct - pct.round()).abs() < 0.01 {
                                label_parts.push(format!("{}%", pct as i32));
                            } else {
                                label_parts.push(format!("{:.1}%", pct));
                            }
                        } else {
                            let lvl = level;
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
                    let start_x = x1 + offset_x * level;
                    let start_y = y1 + offset_y * level;
                    let end_x = mid_x + offset_x * level;
                    let end_y = mid_y + offset_y * level;
                    let dx = end_x - start_x;
                    let dy = end_y - start_y;
                    let line_len = (dx * dx + dy * dy).sqrt();

                    if line_len > 0.001 {
                        let char_width = 6.5;
                        let text_width = label.len() as f64 * char_width;
                        let half_gap_t = (text_width / 2.0) / line_len;

                        let (gap_start, gap_end) = match self.label_position.as_str() {
                            "right" => ((1.0 - half_gap_t * 2.0).max(0.0), 1.0),
                            "center" => ((0.5 - half_gap_t).max(0.0), (0.5 + half_gap_t).min(1.0)),
                            _ => (0.0, (half_gap_t * 2.0).min(1.0)), // "left"
                        };
                        (Some(label), gap_start, gap_end)
                    } else {
                        (Some(label), 0.0, 0.0)
                    }
                } else {
                    (None, 0.0, 0.0)
                }
            } else {
                (None, 0.0, 0.0)
            };

            let has_label_gap = label.is_some() && label_gap_t_end > label_gap_t_start;
            draw_pitchfork_line(ctx, dpr, chart_width, self.extend,
                x1, y1, mid_x, mid_y, offset_x, offset_y, level,
                has_label_gap, label_gap_t_start, label_gap_t_end);

            // Draw label
            if let Some(ref lbl) = label {
                let start_x = x1 + offset_x * level;
                let start_y = y1 + offset_y * level;
                let end_x = mid_x + offset_x * level;
                let end_y = mid_y + offset_y * level;

                ctx.set_font("11px sans-serif");
                ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                ctx.set_fill_color(stroke_color);

                match self.label_position.as_str() {
                    "right" => {
                        ctx.set_text_align(crate::render::TextAlign::Right);
                        ctx.fill_text(lbl, end_x, end_y);
                    }
                    "center" => {
                        ctx.set_text_align(crate::render::TextAlign::Center);
                        ctx.fill_text(lbl, (start_x + end_x) / 2.0, (start_y + end_y) / 2.0);
                    }
                    _ => { // "left"
                        ctx.set_text_align(crate::render::TextAlign::Left);
                        ctx.fill_text(lbl, start_x, start_y);
                    }
                }
            }
        }
        ctx.set_line_dash(&[]);

        // Render text if present (rotated along median line)
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Calculate text position based on v_align:
                // - Center: on median line (level 0)
                // - Start: above upper boundary (highest level, typically level 1)
                // - End: below lower boundary (lowest level, typically level -1)
                let (text_start_x, text_start_y, text_end_x, text_end_y) = match text.v_align {
                    super::super::TextAlign::Start | super::super::TextAlign::End => {
                        // Find visible boundary levels and determine which is visually upper/lower
                        // by comparing actual screen Y coordinates (smaller Y = higher on screen)
                        let visible_levels: Vec<f64> = self.level_configs.iter()
                            .filter(|c| c.visible)
                            .map(|c| c.level)
                            .collect();

                        let (upper_level, lower_level) = if visible_levels.is_empty() {
                            (1.0, -1.0)
                        } else {
                            // Calculate screen Y for level 1 and level -1 (or first/last visible)
                            let max_level = visible_levels.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
                            let min_level = visible_levels.iter().fold(f64::INFINITY, |a, &b| a.min(b));

                            // Calculate actual screen Y positions
                            let max_level_y = y1 + offset_y * max_level;
                            let min_level_y = y1 + offset_y * min_level;

                            // In screen coords, smaller Y is visually higher (upper)
                            if max_level_y < min_level_y {
                                (max_level, min_level)  // max_level is visually upper
                            } else {
                                (min_level, max_level)  // min_level is visually upper
                            }
                        };

                        let level = if matches!(text.v_align, super::super::TextAlign::Start) {
                            upper_level  // Start = upper boundary (smaller screen Y)
                        } else {
                            lower_level  // End = lower boundary (larger screen Y)
                        };

                        (x1 + offset_x * level, y1 + offset_y * level,
                         mid_x + offset_x * level, mid_y + offset_y * level)
                    }
                    super::super::TextAlign::Center => {
                        // On median line (level 0)
                        (x1, y1, mid_x, mid_y)
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

            for (px, py) in [(x1, y1), (x2, y2), (x3, y3)] {
                ctx.begin_path();
                ctx.arc(px, py, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }
        }
    }

    fn level_properties(&self) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::level_mode(self.level_mode.as_str())
                .with_category(PropertyCategory::Levels).with_order(0),
            ConfigProperty::extend_lines(self.extend)
                .with_category(PropertyCategory::Levels)
                .with_order(1),
            ConfigProperty::show_labels(self.show_labels)
                .with_category(PropertyCategory::Levels)
                .with_order(2),
        ]
    }

    fn apply_level_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "level_mode" => {
                if let Some(s) = value.as_string() {
                    self.level_mode = PitchforkLevelMode::from_str(s);
                    return true;
                }
            }
            "extend" => {
                if let Some(v) = value.as_bool() {
                    self.extend = v;
                    return true;
                }
            }
            "show_labels" => {
                if let Some(v) = value.as_bool() {
                    self.show_labels = v;
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::show_labels(self.show_labels).with_order(10),
            ConfigProperty::levels(self.show_percentages).with_order(11),
            ConfigProperty::show_as_percent(self.show_as_percent).with_order(12),
            ConfigProperty::label_position(&self.label_position).with_order(13),
            ConfigProperty::extend_lines(self.extend).with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "show_labels" => {
                if let Some(v) = value.as_bool() {
                    self.show_labels = v;
                    return true;
                }
            }
            "show_percentages" => {
                if let Some(v) = value.as_bool() {
                    self.show_percentages = v;
                    return true;
                }
            }
            "show_as_percent" => {
                if let Some(v) = value.as_bool() {
                    self.show_as_percent = v;
                    return true;
                }
            }
            "label_position" => {
                if let Some(s) = value.as_string() {
                    self.label_position = s.to_string();
                    return true;
                }
            }
            "extend" => {
                if let Some(v) = value.as_bool() {
                    self.extend = v;
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    fn level_configs(&self) -> Option<Vec<FibLevelConfig>> {
        Some(self.level_configs.clone())
    }

    fn set_level_configs(&mut self, configs: Vec<FibLevelConfig>) -> bool {
        self.level_configs = configs;
        true
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

fn point_to_line_distance(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;

    if len_sq == 0.0 {
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
    }

    let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);

    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;

    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

fn point_to_ray_distance(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;

    if len_sq == 0.0 {
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
    }

    let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
    let t = t.max(0.0);

    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;

    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_pitchfork(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar1, price1) = points.first().copied().unwrap_or((0.0, 0.0));
    let (bar2, price2) = points.get(1).copied().unwrap_or((bar1 + 10.0, price1 + 10.0));
    let (bar3, price3) = points.get(2).copied().unwrap_or((bar1 + 10.0, price1 - 10.0));
    Box::new(Pitchfork::new(bar1, price1, bar2, price2, bar3, price3, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "pitchfork",
        display_name: "Pitchfork",
        kind: PrimitiveKind::Channel,
        click_behavior: ClickBehavior::ThreePoint,
        tooltip: "Andrew's Pitchfork - median line with parallel channels",
        icon: "pitchfork",
        default_color: "#F7B93E",
        factory: create_pitchfork,
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
