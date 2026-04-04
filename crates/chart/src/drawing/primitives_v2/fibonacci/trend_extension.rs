//! Fibonacci Trend Extension primitive
//!
//! Uses three points to project Fibonacci extension levels.
//! Point 1 and 2 define the trend, Point 3 is the retracement.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
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

/// Fibonacci Trend Extension - three-point projection
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibTrendExtension {
    /// Common primitive data
    pub data: PrimitiveData,
    /// First point bar (trend start)
    pub bar1: f64,
    /// First point price
    pub price1: f64,
    /// Second point bar (trend end)
    pub bar2: f64,
    /// Second point price
    pub price2: f64,
    /// Third point bar (retracement point)
    pub bar3: f64,
    /// Third point price
    pub price3: f64,
    /// Fibonacci level configurations (with individual colors/widths)
    #[serde(default = "default_level_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    /// Show price labels
    #[serde(default = "default_true")]
    pub show_prices: bool,
    /// Show percentage labels
    #[serde(default = "default_true")]
    pub show_percentages: bool,
    /// Label position: "left", "right", "center"
    #[serde(default = "default_label_position")]
    pub label_position: String,
    /// Show levels as percentages (true) or coefficients (false)
    #[serde(default = "default_true")]
    pub show_as_percent: bool,
    /// Show connecting trend line between points
    #[serde(default = "default_true")]
    pub show_trend_line: bool,
    /// Extend to right edge
    #[serde(default = "default_true")]
    pub extend_right: bool,
}

fn default_true() -> bool { true }
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

impl FibTrendExtension {
    /// Create a new Fibonacci trend extension
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, bar3: f64, price3: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_trend_extension".to_string(),
                display_name: "Fib Extension".to_string(),
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
            show_prices: true,
            show_percentages: true,
            label_position: "left".to_string(),
            show_as_percent: true,
            show_trend_line: true,
            extend_right: true,
        }
    }

    /// Get the price at a given extension level
    /// Extensions are calculated from point 3 based on the 1-2 range
    pub fn price_at_level(&self, level: f64) -> f64 {
        let range = self.price2 - self.price1;
        self.price3 + range * level
    }
}

impl Primitive for FibTrendExtension {
    fn type_id(&self) -> &'static str {
        "fib_trend_extension"
    }

    fn display_name(&self) -> &str {
        &self.data.display_name
    }

    fn kind(&self) -> PrimitiveKind {
        PrimitiveKind::Fibonacci
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

        // Check each level line (horizontal lines extending from point 3)
        let min_x = x3;

        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            let level_price = self.price_at_level(cfg.level);
            let level_y = viewport.price_to_y(level_price, price_scale.price_min, price_scale.price_max);

            let in_bounds = if self.extend_right {
                screen_x >= min_x
            } else {
                screen_x >= min_x && screen_x <= viewport.chart_width
            };

            if in_bounds && (screen_y - level_y).abs() < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }

        // Check trend lines connecting points
        if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        if point_to_line_distance(screen_x, screen_y, x2, y2, x3, y3) < HIT_TOLERANCE {
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

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Draw trend lines 1-2 and 2-3 (if enabled)
        if self.show_trend_line {
            ctx.begin_path();
            ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
            ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
            ctx.line_to(crisp(x3, dpr), crisp(y3, dpr));
            ctx.stroke();
        }

        // Draw extension levels from point 3
        let right_x = if self.extend_right { chart_width } else { chart_width };

        // === FILL RENDERING (before lines so lines are on top) ===
        // Collect visible levels sorted by level value for fill rendering
        let mut visible_levels: Vec<(usize, f64, f64)> = self.level_configs
            .iter()
            .enumerate()
            .filter(|(_, cfg)| cfg.visible)
            .map(|(idx, cfg)| {
                let level_price = self.price_at_level(cfg.level);
                let y = ctx.price_to_y(level_price);
                (idx, cfg.level, y)
            })
            .collect();
        visible_levels.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Draw fills between adjacent visible levels
        for i in 0..visible_levels.len().saturating_sub(1) {
            let (idx, _, y_top) = visible_levels[i];
            let (_, _, y_bottom) = visible_levels[i + 1];

            let cfg = &self.level_configs[idx];
            if cfg.fill_enabled {
                let fill_color = cfg.fill_color.as_deref()
                    .or(cfg.color.as_deref())
                    .unwrap_or(&self.data.color.stroke);

                // Fill from x3 to right edge
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

        // Check if we need line gap on median (0.5 level) for v_align == Center
        let needs_gap = self.data.text.as_ref()
            .map(|t| !t.content.is_empty() && matches!(t.v_align, TextAlign::Center))
            .unwrap_or(false);

        // Calculate gap parameters for median line (0.5 level)
        let (gap_x_start, gap_x_end) = if needs_gap {
            let text = self.data.text.as_ref().unwrap();
            let line_len = right_x - x3;

            if line_len > 0.001 {
                let t_center = match text.h_align {
                    TextAlign::Start => 0.0,
                    TextAlign::Center => 0.5,
                    TextAlign::End => 1.0,
                };

                let char_count = text.content.len() as f64;
                let text_width = char_count * text.font_size * 0.6 + 8.0;
                let half_gap = text_width / 2.0;

                let text_x = x3 + line_len * t_center;
                ((text_x - half_gap).max(x3), (text_x + half_gap).min(right_x))
            } else {
                (0.0, 0.0)
            }
        } else {
            (0.0, 0.0)
        };
        // Draw each level line with individual colors/widths
        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            let level_price = self.price_at_level(cfg.level);
            let y = ctx.price_to_y(level_price);

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
                _ => LineStyle::Solid,
            };

            match line_style {
                LineStyle::Solid => ctx.set_line_dash(&[]),
                LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
                LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
                LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
                LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
            }

            // Check if this is the median line (0.5 level) and needs gap
            let is_median = (cfg.level - 0.5).abs() < 0.001;

            // Build label text if needed
            let label = if self.show_prices || self.show_percentages {
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
                if self.show_prices {
                    label_parts.push(super::super::fmt_price(level_price));
                }
                Some(label_parts.join(" "))
            } else {
                None
            };

            // Calculate label gap position based on label_position
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
                        _ => Some((x3, x3 + text_width)), // "left"
                    }
                } else {
                    None
                }
            } else {
                None
            };

            if is_median && needs_gap && gap_x_end > gap_x_start {
                // Draw median line with gap
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
                // Draw line with gap for label
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

            // Draw label
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
                        ctx.fill_text(lbl, (x3 + right_x) / 2.0, y);
                    }
                    _ => { // "left"
                        ctx.set_text_align(crate::render::TextAlign::Left);
                        ctx.fill_text(lbl, x3, y);
                    }
                }
            }
        }
        ctx.set_line_dash(&[]);

        // Render text if present with proper v_align positioning
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Calculate text X position based on h_align
                let line_len = right_x - x3;
                let text_x = match text.h_align {
                    TextAlign::Start => x3,
                    TextAlign::Center => x3 + line_len * 0.5,
                    TextAlign::End => right_x,
                };

                // Calculate text Y position based on v_align:
                // - Start: above upper boundary (topmost level)
                // - Center: on median (0.5 level)
                // - End: below lower boundary (bottommost level)
                let text_y = match text.v_align {
                    TextAlign::Start | TextAlign::End => {
                        // Calculate all visible level Y positions
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
                            if matches!(text.v_align, TextAlign::Start) {
                                min_y - text_offset  // Above topmost
                            } else {
                                max_y + text_offset  // Below bottommost
                            }
                        }
                    }
                    TextAlign::Center => {
                        // On median (0.5 level)
                        ctx.price_to_y(self.price_at_level(0.5))
                    }
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

// =============================================================================
// Factory Registration
// =============================================================================

fn create_fib_trend_extension(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar1, price1) = points.first().copied().unwrap_or((0.0, 0.0));
    let (bar2, price2) = points.get(1).copied().unwrap_or((bar1 + 10.0, price1 + 10.0));
    let (bar3, price3) = points.get(2).copied().unwrap_or((bar2 + 5.0, price2 - 5.0));
    Box::new(FibTrendExtension::new(bar1, price1, bar2, price2, bar3, price3, color))
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
        factory: create_fib_trend_extension,
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
