//! Fibonacci Speed Resistance Fan primitive
//!
//! Fan lines radiating from a point at Fibonacci-based angles.
//! Also known as speed/resistance arcs - combines price and time analysis.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
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

/// Fibonacci Speed Resistance Fan
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibSpeedResistance {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Origin bar
    pub bar1: f64,
    /// Origin price
    pub price1: f64,
    /// Target bar (defines the base)
    pub bar2: f64,
    /// Target price
    pub price2: f64,
    /// Speed level configurations (with individual colors/widths)
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
    /// Reverse (flip) the fan
    #[serde(default)]
    pub reverse: bool,
}

fn default_true() -> bool { true }

fn default_label_position() -> String { "center".to_string() }

impl FibSpeedResistance {
    /// Create a new speed resistance fan
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_speed_resistance".to_string(),
                display_name: "Speed Resistance".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            bar1,
            price1,
            bar2,
            price2,
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
    fn type_id(&self) -> &'static str {
        "fib_speed_resistance"
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

    fn points(&self) -> Vec<(f64, f64)> {
        vec![(self.bar1, self.price1), (self.bar2, self.price2)]
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
    }

    fn translate(&mut self, bar_delta: f64, price_delta: f64) {
        self.bar1 += bar_delta;
        self.bar2 += bar_delta;
        self.price1 += price_delta;
        self.price2 += price_delta;
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

        // Check control points
        if check_point_hit(screen_x, screen_y, x1, y1) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        if check_point_hit(screen_x, screen_y, x2, y2) {
            return HitTestResult::ControlPoint(ControlPointType::Point2);
        }

        let price_range = self.price2 - self.price1;
        let _bar_range = self.bar2 - self.bar1;

        // Check each fan line (only visible ones)
        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            // Calculate fan line endpoint
            let level_price = if self.reverse {
                self.price1 + price_range * (1.0 - cfg.level)
            } else {
                self.price1 + price_range * cfg.level
            };

            let fan_x2 = viewport.bar_to_x_f64(self.bar2);
            let fan_y2 = viewport.price_to_y(level_price, price_scale.price_min, price_scale.price_max);

            if point_to_ray_distance(screen_x, screen_y, x1, y1, fan_x2, fan_y2) < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }

        // Check baseline
        if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
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

        vec![
            ControlPoint::point1(x1, y1),
            ControlPoint::point2(x2, y2),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.bar_to_x(self.bar1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.bar_to_x(self.bar2);
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

        // Draw baseline
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
        ctx.stroke();

        let price_range = self.price2 - self.price1;

        // === FILL RENDERING (before lines so lines are on top) ===
        // Collect visible levels sorted by level value for fill rendering
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

        // Draw fills between adjacent visible fan rays
        for i in 0..visible_levels.len().saturating_sub(1) {
            let (idx, _, fy1) = visible_levels[i];
            let (_, _, fy2) = visible_levels[i + 1];

            let cfg = &self.level_configs[idx];
            if cfg.fill_enabled {
                let fill_color = cfg.fill_color.as_deref()
                    .or(cfg.color.as_deref())
                    .unwrap_or(&self.data.color.stroke);

                // Calculate extended endpoints (always extended for speed resistance)
                let dx1 = x2 - x1;
                let dy1 = fy1 - y1;
                let len1 = (dx1 * dx1 + dy1 * dy1).sqrt();
                let dx2 = x2 - x1;
                let dy2 = fy2 - y1;
                let len2 = (dx2 * dx2 + dy2 * dy2).sqrt();
                let ext = chart_width * 2.0;

                let (end_x1, end_y1) = if len1 > 0.0 {
                    (x1 + dx1 / len1 * ext, y1 + dy1 / len1 * ext)
                } else {
                    (x2, fy1)
                };
                let (end_x2, end_y2) = if len2 > 0.0 {
                    (x1 + dx2 / len2 * ext, y1 + dy2 / len2 * ext)
                } else {
                    (x2, fy2)
                };

                // Draw fill triangle from origin to both ray endpoints
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

        // Draw fan lines at each speed level (only visible ones)
        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            let level_price = if self.reverse {
                self.price1 + price_range * (1.0 - cfg.level)
            } else {
                self.price1 + price_range * cfg.level
            };

            let fan_y = ctx.price_to_y(level_price);

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

            // Line direction
            let dx = x2 - x1;
            let dy = fan_y - y1;
            let len = (dx * dx + dy * dy).sqrt();

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

                if !label.is_empty() && len > 0.001 {
                    let char_width = 6.5;
                    let text_width = label.len() as f64 * char_width;
                    let half_gap_t = (text_width / 2.0) / len;

                    let (gap_start, gap_end) = match self.label_position.as_str() {
                        "right" => ((1.0 - half_gap_t * 2.0).max(0.0), 1.0),
                        "center" => ((0.5 - half_gap_t).max(0.0), (0.5 + half_gap_t).min(1.0)),
                        _ => (0.0, (half_gap_t * 2.0).min(1.0)), // "left"
                    };
                    (Some(label), gap_start, gap_end)
                } else {
                    (if label.is_empty() { None } else { Some(label) }, 0.0, 0.0)
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
            } else if is_median && needs_gap && len > 0.001 {
                let text = self.data.text.as_ref().unwrap();
                let t_center = match text.h_align {
                    TextAlign::Start => 0.0,
                    TextAlign::Center => 0.5,
                    TextAlign::End => 1.0,
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

                let ext = chart_width * 2.0;
                let nx = dx / len;
                let ny = dy / len;
                ctx.line_to(crisp(x1 + nx * ext, dpr), crisp(y1 + ny * ext, dpr));
            } else {
                ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
                if len > 0.0 {
                    let ext = chart_width * 2.0;
                    let nx = dx / len;
                    let ny = dy / len;
                    ctx.line_to(crisp(x1 + nx * ext, dpr), crisp(y1 + ny * ext, dpr));
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
                        ctx.fill_text(&lbl, x2, fan_y);
                    }
                    "center" => {
                        ctx.set_text_align(crate::render::TextAlign::Center);
                        ctx.fill_text(&lbl, (x1 + x2) / 2.0, (y1 + fan_y) / 2.0);
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
                                let level_price = if self.reverse {
                                    self.price1 + price_range * (1.0 - cfg.level)
                                } else {
                                    self.price1 + price_range * cfg.level
                                };
                                let fy = ctx.price_to_y(level_price);
                                (cfg.level, fy, level_price)
                            }).collect();

                        if fan_endpoints.is_empty() {
                            // Fallback to baseline
                            (x1, y1, x2, y2)
                        } else {
                            // Sort by screen Y (smaller = visually higher)
                            fan_endpoints.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

                            let (_, _, level_price) = if matches!(text.v_align, TextAlign::Start) {
                                *fan_endpoints.first().unwrap()  // Topmost (smallest screen Y)
                            } else {
                                *fan_endpoints.last().unwrap()   // Bottommost (largest screen Y)
                            };

                            let fy = ctx.price_to_y(level_price);
                            (x1, y1, x2, fy)
                        }
                    }
                    TextAlign::Center => {
                        // On median (0.5 level) line
                        let median_price = if self.reverse {
                            self.price1 + price_range * 0.5
                        } else {
                            self.price1 + price_range * 0.5
                        };
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
            ConfigProperty::reverse(self.reverse).with_order(20),
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
            "reverse" => {
                if let PropertyValue::Boolean(v) = value {
                    self.reverse = *v;
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

fn create_fib_speed_resistance(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar1, price1) = points.first().copied().unwrap_or((0.0, 0.0));
    let (bar2, price2) = points.get(1).copied().unwrap_or((bar1 + 20.0, price1 + 10.0));
    Box::new(FibSpeedResistance::new(bar1, price1, bar2, price2, color))
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
        factory: create_fib_speed_resistance,
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
