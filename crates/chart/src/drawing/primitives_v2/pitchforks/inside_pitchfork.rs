//! Inside Pitchfork primitive
//!
//! A pitchfork that draws channels inward from the outer points
//! rather than from the handle point outward.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    HIT_TOLERANCE, CONTROL_POINT_RADIUS,
    RenderContext, crisp, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text_rotated, calculate_line_text_params,
    config::FibLevelConfig,
};

use super::pitchfork::default_level_configs;

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
                if value.is_object() {
                    let config: FibLevelConfig = serde_json::from_value(value)
                        .map_err(de::Error::custom)?;
                    configs.push(config);
                } else if let Some(level) = value.as_f64() {
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

fn default_true() -> bool { true }
fn default_label_position() -> String { "left".to_string() }

/// Inside Pitchfork
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InsidePitchfork {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Point 1 - the handle
    #[serde(default)]
    pub ts1: i64,
    pub price1: f64,
    /// Point 2 - first swing
    #[serde(default)]
    pub ts2: i64,
    pub price2: f64,
    /// Point 3 - second swing
    #[serde(default)]
    pub ts3: i64,
    pub price3: f64,
    /// Pitchfork level configurations
    #[serde(default = "default_level_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    /// Extend lines
    #[serde(default = "default_true")]
    pub extend: bool,
    /// Show level labels
    #[serde(default = "default_true")]
    pub show_labels: bool,
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

impl InsidePitchfork {
    /// Create a new Inside pitchfork
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, ts3: i64, price3: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "inside_pitchfork".to_string(),
                display_name: "Inside Pitchfork".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            ts1,
            price1,
            ts2,
            price2,
            ts3,
            price3,
            level_configs: default_level_configs(),
            extend: true,
            show_labels: true,
            show_percentages: true,
            label_position: "left".to_string(),
            show_as_percent: true,
        }
    }

    /// Get the midpoint between points 2 and 3 (ts in ms, price)
    pub fn midpoint(&self) -> (i64, f64) {
        (
            (self.ts2 + self.ts3) / 2,
            (self.price2 + self.price3) / 2.0,
        )
    }

    /// Get the channel half-offset inverted (ts_delta in ms, price_delta)
    pub fn channel_offset(&self) -> (i64, f64) {
        (
            (self.ts2 - self.ts3) / 2,
            (self.price2 - self.price3) / 2.0,
        )
    }

    /// Get effective levels for rendering: median (0) + symmetric ±levels
    /// Returns tuples of (level_value, config) for each line to draw
    pub fn effective_levels(&self) -> Vec<(f64, &FibLevelConfig)> {
        let mut result = Vec::new();
        for config in &self.level_configs {
            if config.visible {
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
}

impl Primitive for InsidePitchfork {
    fn type_id(&self) -> &'static str {
        "inside_pitchfork"
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

    fn points(&self) -> Vec<(i64, f64)> {
        vec![
            (self.ts1, self.price1),
            (self.ts2, self.price2),
            (self.ts3, self.price3),
        ]
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
        if let Some(&(ts, price)) = points.get(2) {
            self.ts3 = ts;
            self.price3 = price;
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        self.ts1 += ts_delta_ms;
        self.ts2 += ts_delta_ms;
        self.ts3 += ts_delta_ms;
        self.price1 += price_delta;
        self.price2 += price_delta;
        self.price3 += price_delta;
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
            ControlPointType::Point3 => {
                self.ts3 = ts_ms;
                self.price3 = price;
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
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let b3 = timestamp_ms_to_bar_f64(bars, self.ts3);
        let x1 = viewport.bar_to_x_f64(b1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(b2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);
        let x3 = viewport.bar_to_x_f64(b3);
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

        let (mid_ts, mid_price) = self.midpoint();
        let mid_b = timestamp_ms_to_bar_f64(bars, mid_ts);
        let mid_x = viewport.bar_to_x_f64(mid_b);
        let mid_y = viewport.price_to_y(mid_price, price_scale.price_min, price_scale.price_max);

        let (offset_ts, offset_price) = self.channel_offset();
        let offset_b = timestamp_ms_to_bar_f64(bars, mid_ts + offset_ts);
        let offset_x = viewport.bar_to_x_f64(offset_b) - mid_x;
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

        // Check each pitchfork line (inside variant inverts the direction, symmetric ±levels)
        for (level, _config) in self.effective_levels() {
            let start_x = x1 - offset_x * level;
            let start_y = y1 - offset_y * level;
            let end_x = mid_x - offset_x * level;
            let end_y = mid_y - offset_y * level;

            let dist = if self.extend {
                point_to_ray_distance(screen_x, screen_y, start_x, start_y, end_x, end_y)
            } else {
                point_to_line_distance(screen_x, screen_y, start_x, start_y, end_x, end_y)
            };

            if dist < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }

        // Check connecting lines
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
        bars: &[Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Vec<ControlPoint> {
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let b3 = timestamp_ms_to_bar_f64(bars, self.ts3);
        let x1 = viewport.bar_to_x_f64(b1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(b2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);
        let x3 = viewport.bar_to_x_f64(b3);
        let y3 = viewport.price_to_y(self.price3, price_scale.price_min, price_scale.price_max);

        vec![
            ControlPoint::point1(x1, y1),
            ControlPoint::point2(x2, y2),
            ControlPoint::point3(x3, y3),
        ]
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

        let (mid_ts, mid_price) = self.midpoint();
        let mid_x = ctx.ts_to_x_ms(mid_ts);
        let mid_y = ctx.price_to_y(mid_price);

        // Inside pitchfork: inverted channel offset
        let (offset_ts, offset_price) = self.channel_offset();
        let offset_x = ctx.ts_to_x_ms(mid_ts + offset_ts) - mid_x;
        let offset_y = ctx.price_to_y(mid_price + offset_price) - mid_y;

        // Helper function to draw a pitchfork line with optional gap for label
        fn draw_line_with_gap(
            ctx: &mut dyn RenderContext, dpr: f64, chart_width: f64, extend: bool,
            start_x: f64, start_y: f64, end_x: f64, end_y: f64,
            with_gap: bool, gap_t_start: f64, gap_t_end: f64,
        ) {
            ctx.begin_path();
            if with_gap && gap_t_end > gap_t_start {
                let dx = end_x - start_x;
                let dy = end_y - start_y;
                let base_len = (dx * dx + dy * dy).sqrt();

                // Draw segment before gap (if gap doesn't start at 0)
                if gap_t_start > 0.001 {
                    let gap_x1 = start_x + dx * gap_t_start;
                    let gap_y1 = start_y + dy * gap_t_start;
                    ctx.move_to(crisp(start_x, dpr), crisp(start_y, dpr));
                    ctx.line_to(crisp(gap_x1, dpr), crisp(gap_y1, dpr));
                }

                // Draw segment after gap
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

        // Wrapper for draw_line without gap (used for median line)
        fn draw_line(
            ctx: &mut dyn RenderContext, dpr: f64, chart_width: f64, extend: bool,
            start_x: f64, start_y: f64, end_x: f64, end_y: f64,
        ) {
            draw_line_with_gap(ctx, dpr, chart_width, extend, start_x, start_y, end_x, end_y, false, 0.0, 0.0);
        }

        // === FILL RENDERING (before lines so lines are on top) ===
        // For inside pitchfork, fill is drawn between +level and -level (symmetric channel)
        // Note: inside pitchfork uses subtraction for offset direction
        for config in &self.level_configs {
            if config.visible && config.fill_enabled && config.level.abs() > 0.001 {
                let level = config.level;
                let fill_color = config.fill_color.as_deref()
                    .or(config.color.as_deref())
                    .unwrap_or(&self.data.color.stroke);

                // Calculate corners of the fill quad between +level and -level
                // Start points (at handle, using subtraction for inside variant)
                let pos_start_x = x1 - offset_x * level;
                let pos_start_y = y1 - offset_y * level;
                let neg_start_x = x1 - offset_x * (-level);
                let neg_start_y = y1 - offset_y * (-level);

                // End points (extended or at midpoint)
                let (pos_end_x, pos_end_y, neg_end_x, neg_end_y) = if self.extend {
                    let pos_dx = (mid_x - offset_x * level) - pos_start_x;
                    let pos_dy = (mid_y - offset_y * level) - pos_start_y;
                    let pos_len = (pos_dx * pos_dx + pos_dy * pos_dy).sqrt();

                    let neg_dx = (mid_x - offset_x * (-level)) - neg_start_x;
                    let neg_dy = (mid_y - offset_y * (-level)) - neg_start_y;
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
                    (mid_x - offset_x * level, mid_y - offset_y * level,
                     mid_x - offset_x * (-level), mid_y - offset_y * (-level))
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
        draw_line(ctx, dpr, chart_width, self.extend, x1, y1, mid_x, mid_y);

        // 2. Draw symmetric ±levels from effective_levels() - inside variant inverts the direction
        for (level, config) in self.effective_levels() {
            let start_x = x1 - offset_x * level;
            let start_y = y1 - offset_y * level;
            let end_x = mid_x - offset_x * level;
            let end_y = mid_y - offset_y * level;

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
            draw_line_with_gap(ctx, dpr, chart_width, self.extend, start_x, start_y, end_x, end_y,
                has_label_gap, label_gap_t_start, label_gap_t_end);

            // Draw label in the gap
            if let Some(label) = label {
                ctx.set_font("11px sans-serif");
                ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                ctx.set_fill_color(stroke_color);

                match self.label_position.as_str() {
                    "right" => {
                        ctx.set_text_align(crate::render::TextAlign::Right);
                        ctx.fill_text(&label, end_x, end_y);
                    }
                    "center" => {
                        ctx.set_text_align(crate::render::TextAlign::Center);
                        ctx.fill_text(&label, (start_x + end_x) / 2.0, (start_y + end_y) / 2.0);
                    }
                    _ => { // "left"
                        ctx.set_text_align(crate::render::TextAlign::Left);
                        ctx.fill_text(&label, start_x, start_y);
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
                // - Start: above upper boundary (highest level, but inverted for inside)
                // - End: below lower boundary (lowest level, but inverted for inside)
                let (text_start_x, text_start_y, text_end_x, text_end_y) = match text.v_align {
                    super::super::TextAlign::Start | super::super::TextAlign::End => {
                        // Find visible boundary levels and determine which is visually upper/lower
                        // by comparing actual screen Y coordinates (smaller Y = higher on screen)
                        // Note: inside pitchfork uses subtraction for offset
                        let visible_levels: Vec<f64> = self.level_configs.iter()
                            .filter(|c| c.visible)
                            .map(|c| c.level)
                            .collect();

                        let (upper_level, lower_level) = if visible_levels.is_empty() {
                            (-1.0, 1.0)
                        } else {
                            let max_level = visible_levels.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
                            let min_level = visible_levels.iter().fold(f64::INFINITY, |a, &b| a.min(b));

                            // Calculate actual screen Y positions (note: inside uses subtraction)
                            let max_level_y = y1 - offset_y * max_level;
                            let min_level_y = y1 - offset_y * min_level;

                            // In screen coords, smaller Y is visually higher (upper)
                            if max_level_y < min_level_y {
                                (max_level, min_level)
                            } else {
                                (min_level, max_level)
                            }
                        };

                        let level = if matches!(text.v_align, super::super::TextAlign::Start) {
                            upper_level
                        } else {
                            lower_level
                        };

                        (x1 - offset_x * level, y1 - offset_y * level,
                         mid_x - offset_x * level, mid_y - offset_y * level)
                    }
                    super::super::TextAlign::Center => {
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
            ConfigProperty::show_labels(self.show_labels).with_order(10),
            ConfigProperty::levels(self.show_percentages).with_order(11),
            ConfigProperty::show_as_percent(self.show_as_percent).with_order(12),
            ConfigProperty::label_position(&self.label_position).with_order(13),
            ConfigProperty::extend(self.extend).with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &super::super::config::PropertyValue) -> bool {
        use super::super::config::PropertyValue;
        match id {
            "show_labels" => {
                if let PropertyValue::Boolean(v) = value {
                    self.show_labels = *v;
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
            "extend" => {
                if let PropertyValue::Boolean(v) = value {
                    self.extend = *v;
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

fn create_inside_pitchfork(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
    let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 3_600_000, price1 + 10.0));
    let (ts3, price3) = points.get(2).copied().unwrap_or((ts1 + 3_600_000, price1 - 10.0));
    Box::new(InsidePitchfork::new(ts1, price1, ts2, price2, ts3, price3, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "inside_pitchfork",
        display_name: "Inside Pitchfork",
        kind: PrimitiveKind::Channel,
        click_behavior: ClickBehavior::ThreePoint,
        tooltip: "Inside Pitchfork - inward channels",
        icon: "inside_pitchfork",
        default_color: "#F7B93E",
        factory: create_inside_pitchfork,
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
