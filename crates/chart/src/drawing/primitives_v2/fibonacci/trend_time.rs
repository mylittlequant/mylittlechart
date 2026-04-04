//! Fibonacci Trend Time primitive
//!
//! Vertical lines projected at Fibonacci ratios from a trend.
//! Uses two points to define a time range, then projects Fib levels.

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

/// Fibonacci Trend Time - vertical lines at Fib ratios of time range
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibTrendTime {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Start bar
    pub bar1: f64,
    /// Start price (for anchor display)
    pub price1: f64,
    /// End bar
    pub bar2: f64,
    /// End price
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
    /// Show connecting trend line between points
    #[serde(default = "default_true")]
    pub show_trend_line: bool,
}

fn default_true() -> bool { true }
fn default_label_position() -> String { "left".to_string() }

impl FibTrendTime {
    /// Create new Fibonacci trend time
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_trend_time".to_string(),
                display_name: "Fib Trend Time".to_string(),
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
            label_position: "left".to_string(),
            show_as_percent: true,
            show_trend_line: true,
        }
    }

    /// Get bar position for a level
    pub fn bar_at_level(&self, level: f64) -> f64 {
        self.bar1 + (self.bar2 - self.bar1) * level
    }
}

impl Primitive for FibTrendTime {
    fn type_id(&self) -> &'static str {
        "fib_trend_time"
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

        // Check baseline connecting points
        if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        // Check each vertical level line (only visible ones)
        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            let level_bar = self.bar_at_level(cfg.level);
            let level_x = viewport.bar_to_x_f64(level_bar);

            if (screen_x - level_x).abs() < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
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

        // Draw baseline connecting points (if enabled)
        if self.show_trend_line {
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.begin_path();
            ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
            ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
            ctx.stroke();
        }

        // === FILL RENDERING (before lines so lines are on top) ===
        // Collect visible levels sorted by level value for fill rendering
        let mut visible_levels: Vec<(usize, f64, f64)> = self.level_configs
            .iter()
            .enumerate()
            .filter(|(_, cfg)| cfg.visible)
            .map(|(idx, cfg)| {
                let level_bar = self.bar_at_level(cfg.level);
                let level_x = ctx.bar_to_x(level_bar);
                (idx, cfg.level, level_x)
            })
            .collect();
        visible_levels.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Draw fills between adjacent visible vertical lines
        for i in 0..visible_levels.len().saturating_sub(1) {
            let (idx, _, x1) = visible_levels[i];
            let (_, _, x2) = visible_levels[i + 1];

            let cfg = &self.level_configs[idx];
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

        // Draw vertical lines at each Fibonacci time level (only visible ones)
        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            let level_bar = self.bar_at_level(cfg.level);
            let level_x = ctx.bar_to_x(level_bar);

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

            // Build label text and calculate gap if needed
            let (label, label_y, gap_half_height) = if self.show_labels || self.show_percentages {
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
                let lbl = label_parts.join(" ");
                if !lbl.is_empty() {
                    // Calculate label Y position and gap
                    let (ly, gap_h) = match self.label_position.as_str() {
                        "right" | "center" => (chart_height / 2.0, 8.0), // middle
                        _ => (4.0 + 6.0, 8.0), // "left" = top, add half font height
                    };
                    (Some(lbl), ly, gap_h)
                } else {
                    (None, 0.0, 0.0)
                }
            } else {
                (None, 0.0, 0.0)
            };

            // Draw vertical line with gap for label
            ctx.begin_path();
            if label.is_some() && gap_half_height > 0.0 {
                let gap_y_start = label_y - gap_half_height;
                let gap_y_end = label_y + gap_half_height;

                // Draw line from top to gap
                if gap_y_start > 0.0 {
                    ctx.move_to(crisp(level_x, dpr), 0.0);
                    ctx.line_to(crisp(level_x, dpr), gap_y_start);
                }

                // Draw line from gap to bottom
                if gap_y_end < chart_height {
                    ctx.move_to(crisp(level_x, dpr), gap_y_end);
                    ctx.line_to(crisp(level_x, dpr), chart_height);
                }
            } else {
                ctx.move_to(crisp(level_x, dpr), 0.0);
                ctx.line_to(crisp(level_x, dpr), chart_height);
            }
            ctx.stroke();

            // Draw label in the gap
            if let Some(ref lbl) = label {
                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(color);
                ctx.set_text_align(crate::render::TextAlign::Center);

                match self.label_position.as_str() {
                    "right" | "center" => {
                        ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                        ctx.fill_text(lbl, level_x, chart_height / 2.0);
                    }
                    _ => { // "left" = top
                        ctx.set_text_baseline(crate::render::TextBaseline::Top);
                        ctx.fill_text(lbl, level_x, 4.0);
                    }
                }
            }
        }
        ctx.set_line_dash(&[]);

        // Render text if present with proper v_align positioning
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // For vertical lines, h_align determines which level line the text is on
                // v_align determines Y position: Start=top, Center=middle, End=bottom

                // Calculate text X position based on h_align
                let text_x = match text.h_align {
                    TextAlign::Start => x1,
                    TextAlign::Center => {
                        // Median (0.5) level line
                        let median_bar = self.bar_at_level(0.5);
                        ctx.bar_to_x(median_bar)
                    }
                    TextAlign::End => x2,
                };

                // Calculate text Y position based on v_align:
                // - Start: at top of chart
                // - Center: at middle of chart
                // - End: at bottom of chart
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_y = match text.v_align {
                    TextAlign::Start => text_offset,              // Near top
                    TextAlign::Center => chart_height / 2.0,      // Middle
                    TextAlign::End => chart_height - text_offset, // Near bottom
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
            ConfigProperty::show_trend_line(self.show_trend_line).with_order(20),
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

fn create_fib_trend_time(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar1, price1) = points.first().copied().unwrap_or((0.0, 0.0));
    let (bar2, price2) = points.get(1).copied().unwrap_or((bar1 + 20.0, price1));
    Box::new(FibTrendTime::new(bar1, price1, bar2, price2, color))
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
        factory: create_fib_trend_time,
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
