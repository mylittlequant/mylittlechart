//! Fibonacci Fan primitive
//!
//! Fan lines radiating from a point through Fibonacci levels.
//! Similar to speed resistance but with different angle calculations.

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

/// Fibonacci Fan
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibFan {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Origin timestamp in ms
    #[serde(default)]
    pub ts1: i64,
    /// Origin price
    pub price1: f64,
    /// Target timestamp in ms
    #[serde(default)]
    pub ts2: i64,
    /// Target price
    pub price2: f64,
    /// Fibonacci level configurations (with individual colors/widths)
    #[serde(default = "default_level_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    /// Show labels
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
    /// Extend rays to edge of chart
    #[serde(default = "default_true")]
    pub extend: bool,
}

fn default_true() -> bool { true }

fn default_label_position() -> String { "center".to_string() }

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

impl FibFan {
    /// Create a new Fibonacci fan
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_fan".to_string(),
                display_name: "Fib Fan".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            ts1,
            price1,
            ts2,
            price2,
            level_configs: default_level_configs(),
            show_labels: true,
            show_percentages: true,
            label_position: "center".to_string(),
            show_as_percent: true,
            extend: true,
        }
    }

    /// Get the endpoint for a fan line at given level.
    /// Returns (timestamp_ms, price) of the endpoint.
    pub fn fan_endpoint(&self, level: f64) -> (i64, f64) {
        let price_range = self.price2 - self.price1;
        let fan_price = self.price1 + price_range * level;
        (self.ts2, fan_price)
    }
}

impl Primitive for FibFan {
    fn type_id(&self) -> &'static str {
        "fib_fan"
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
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(b1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(b2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        // Check control points
        if check_point_hit(screen_x, screen_y, x1, y1) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        if check_point_hit(screen_x, screen_y, x2, y2) {
            return HitTestResult::ControlPoint(ControlPointType::Point2);
        }

        // Check baseline
        if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        // Check each fan ray (only visible ones)
        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            let (fan_ts, fan_price) = self.fan_endpoint(cfg.level);
            let fan_bar = timestamp_ms_to_bar_f64(bars, fan_ts);
            let fx = viewport.bar_to_x_f64(fan_bar);
            let fy = viewport.price_to_y(fan_price, price_scale.price_min, price_scale.price_max);

            let dist = if self.extend {
                point_to_ray_distance(screen_x, screen_y, x1, y1, fx, fy)
            } else {
                point_to_line_distance(screen_x, screen_y, x1, y1, fx, fy)
            };

            if dist < HIT_TOLERANCE {
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
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(b1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(b2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        vec![
            ControlPoint::point1(x1, y1),
            ControlPoint::point2(x2, y2),
        ]
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

        // Draw baseline from point 1 to point 2
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
        ctx.stroke();

        // === FILL RENDERING (before lines so lines are on top) ===
        // Collect visible levels sorted by level value for fill rendering
        let mut visible_levels: Vec<(usize, f64, f64, f64)> = self.level_configs
            .iter()
            .enumerate()
            .filter(|(_, cfg)| cfg.visible)
            .map(|(idx, cfg)| {
                let (fan_ts, fan_price) = self.fan_endpoint(cfg.level);
                let fx = ctx.ts_to_x_ms(fan_ts);
                let fy = ctx.price_to_y(fan_price);
                (idx, cfg.level, fx, fy)
            })
            .collect();
        visible_levels.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Draw fills between adjacent visible fan rays
        for i in 0..visible_levels.len().saturating_sub(1) {
            let (idx, _, fx1, fy1) = visible_levels[i];
            let (_, _, fx2, fy2) = visible_levels[i + 1];

            let cfg = &self.level_configs[idx];
            if cfg.fill_enabled {
                let fill_color = cfg.fill_color.as_deref()
                    .or(cfg.color.as_deref())
                    .unwrap_or(&self.data.color.stroke);

                // Calculate extended endpoints if extend is enabled
                let (end_x1, end_y1, end_x2, end_y2) = if self.extend {
                    let dx1 = fx1 - x1;
                    let dy1 = fy1 - y1;
                    let len1 = (dx1 * dx1 + dy1 * dy1).sqrt();
                    let dx2 = fx2 - x1;
                    let dy2 = fy2 - y1;
                    let len2 = (dx2 * dx2 + dy2 * dy2).sqrt();
                    let ext = chart_width * 2.0;

                    let (ex1, ey1) = if len1 > 0.0 {
                        (x1 + dx1 / len1 * ext, y1 + dy1 / len1 * ext)
                    } else {
                        (fx1, fy1)
                    };
                    let (ex2, ey2) = if len2 > 0.0 {
                        (x1 + dx2 / len2 * ext, y1 + dy2 / len2 * ext)
                    } else {
                        (fx2, fy2)
                    };
                    (ex1, ey1, ex2, ey2)
                } else {
                    (fx1, fy1, fx2, fy2)
                };

                // Draw fill triangle/quad from origin to both ray endpoints
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

        // Check if we need line gap on median (0.5 level) for v_align == Center
        let needs_gap = self.data.text.as_ref()
            .map(|t| !t.content.is_empty() && matches!(t.v_align, TextAlign::Center))
            .unwrap_or(false);

        // Draw fan lines at each level (only visible ones)
        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            let (fan_ts, fan_price) = self.fan_endpoint(cfg.level);
            let fx = ctx.ts_to_x_ms(fan_ts);
            let fy = ctx.price_to_y(fan_price);

            // Use level-specific color or fall back to main color
            let color = cfg.color.as_deref().unwrap_or(&self.data.color.stroke);
            ctx.set_stroke_color(color);

            // Use level-specific width or fall back to main width
            let width = cfg.width.unwrap_or(self.data.width);
            ctx.set_stroke_width(width);

            // Parse style from string
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

            // Build label and calculate gap for this level
            let (label, label_gap_t_start, label_gap_t_end) = if self.show_labels || self.show_percentages {
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
                    let dx = fx - x1;
                    let dy = fy - y1;
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

            // Check if this is the median line (0.5 level) and needs text gap
            let is_median = (cfg.level - 0.5).abs() < 0.001;
            let has_label_gap = label.is_some() && label_gap_t_end > label_gap_t_start;

            // Determine gap parameters (label gap takes priority, then text gap for median)
            let (use_gap, gap_t_start, gap_t_end) = if has_label_gap {
                (true, label_gap_t_start, label_gap_t_end)
            } else if is_median && needs_gap {
                let text = self.data.text.as_ref().unwrap();
                let dx = fx - x1;
                let dy = fy - y1;
                let base_len = (dx * dx + dy * dy).sqrt();
                if base_len > 0.001 {
                    let t_center = match text.h_align {
                        TextAlign::Start => 0.0,
                        TextAlign::Center => 0.5,
                        TextAlign::End => 1.0,
                    };
                    let char_count = text.content.len() as f64;
                    let text_width = char_count * text.font_size * 0.6 + 8.0;
                    let half_gap_t = (text_width / 2.0) / base_len;
                    (true, (t_center - half_gap_t).max(0.0), (t_center + half_gap_t).min(1.0))
                } else {
                    (false, 0.0, 0.0)
                }
            } else {
                (false, 0.0, 0.0)
            };

            let dx = fx - x1;
            let dy = fy - y1;
            let base_len = (dx * dx + dy * dy).sqrt();

            ctx.begin_path();
            if use_gap && gap_t_end > gap_t_start && base_len > 0.001 {
                // Draw first segment before gap
                if gap_t_start > 0.001 {
                    let gap_x1 = x1 + dx * gap_t_start;
                    let gap_y1 = y1 + dy * gap_t_start;
                    ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
                    ctx.line_to(crisp(gap_x1, dpr), crisp(gap_y1, dpr));
                }

                // Draw second segment after gap
                let gap_x2 = x1 + dx * gap_t_end;
                let gap_y2 = y1 + dy * gap_t_end;
                ctx.move_to(crisp(gap_x2, dpr), crisp(gap_y2, dpr));

                if self.extend {
                    let ext = chart_width * 2.0;
                    let nx = dx / base_len;
                    let ny = dy / base_len;
                    ctx.line_to(crisp(x1 + nx * ext, dpr), crisp(y1 + ny * ext, dpr));
                } else {
                    ctx.line_to(crisp(fx, dpr), crisp(fy, dpr));
                }
            } else {
                ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
                if self.extend && base_len > 0.0 {
                    let ext = chart_width * 2.0;
                    let nx = dx / base_len;
                    let ny = dy / base_len;
                    ctx.line_to(crisp(x1 + nx * ext, dpr), crisp(y1 + ny * ext, dpr));
                } else {
                    ctx.line_to(crisp(fx, dpr), crisp(fy, dpr));
                }
            }
            ctx.stroke();

            // Draw label in the gap
            if let Some(lbl) = label {
                ctx.set_font("11px sans-serif");
                ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                ctx.set_fill_color(color);

                match self.label_position.as_str() {
                    "right" => {
                        ctx.set_text_align(crate::render::TextAlign::Right);
                        ctx.fill_text(&lbl, fx, fy);
                    }
                    "center" => {
                        ctx.set_text_align(crate::render::TextAlign::Center);
                        ctx.fill_text(&lbl, (x1 + fx) / 2.0, (y1 + fy) / 2.0);
                    }
                    _ => { // "left"
                        ctx.set_text_align(crate::render::TextAlign::Left);
                        ctx.fill_text(&lbl, x1, y1);
                    }
                }
            }
        }
        ctx.set_line_dash(&[]);

        // Render text if present (positioned based on v_align along fan lines)
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Calculate text position based on v_align:
                // - Start: above upper boundary (topmost fan line)
                // - Center: on median (0.5 level) line - with line gap
                // - End: below lower boundary (bottommost fan line)
                let (text_start_x, text_start_y, text_end_x, text_end_y) = match text.v_align {
                    TextAlign::Start | TextAlign::End => {
                        // Find topmost and bottommost fan lines by screen Y coordinate (only visible ones)
                        let mut fan_endpoints: Vec<(f64, f64, f64)> = self.level_configs.iter()
                            .filter(|cfg| cfg.visible)
                            .map(|cfg| {
                                let (_, fan_price) = self.fan_endpoint(cfg.level);
                                let fy = ctx.price_to_y(fan_price);
                                (cfg.level, fy, fan_price)
                            }).collect();

                        if fan_endpoints.is_empty() {
                            // Fallback to baseline
                            (x1, y1, x2, y2)
                        } else {
                            // Sort by screen Y (smaller = visually higher)
                            fan_endpoints.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

                            let level = if matches!(text.v_align, TextAlign::Start) {
                                fan_endpoints.first().unwrap().0  // Topmost (smallest screen Y)
                            } else {
                                fan_endpoints.last().unwrap().0   // Bottommost (largest screen Y)
                            };

                            let (_, fan_price) = self.fan_endpoint(level);
                            let fx = ctx.ts_to_x_ms(self.ts2);
                            let fy = ctx.price_to_y(fan_price);
                            (x1, y1, fx, fy)
                        }
                    }
                    TextAlign::Center => {
                        // On median (0.5 level) line
                        let (_, fan_price) = self.fan_endpoint(0.5);
                        let fx = ctx.ts_to_x_ms(self.ts2);
                        let fy = ctx.price_to_y(fan_price);
                        (x1, y1, fx, fy)
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
            ConfigProperty::extend_lines(self.extend).with_order(20),
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
    let t = t.max(0.0); // Ray extends from point 1 through point 2

    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;

    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_fib_fan(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
    let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, price1 + 10.0));
    Box::new(FibFan::new(ts1, price1, ts2, price2, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "fib_fan",
        display_name: "Fib Fan",
        kind: PrimitiveKind::Fibonacci,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Fibonacci fan lines",
        icon: "fib_fan",
        default_color: "#F7B93E",
        factory: create_fib_fan,
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
