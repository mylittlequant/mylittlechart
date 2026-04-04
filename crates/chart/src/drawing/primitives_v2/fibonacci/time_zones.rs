//! Fibonacci Time Zones primitive
//!
//! Vertical lines at Fibonacci intervals from a starting point.
//! Shows time-based projections: 1, 2, 3, 5, 8, 13, 21, 34, 55, 89 bars...

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

/// Main time zone levels (Fibonacci sequence - visible by default)
pub const MAIN_TIME_ZONES: &[f64] = &[1.0, 2.0, 3.0, 5.0, 8.0, 13.0, 21.0, 34.0, 55.0, 89.0, 144.0, 233.0];

/// Extended time zone levels (longer projections)
pub const ALL_TIME_ZONES: &[f64] = &[
    // Standard Fibonacci sequence
    1.0, 2.0, 3.0, 5.0, 8.0, 13.0, 21.0, 34.0, 55.0, 89.0, 144.0, 233.0,
    // Extended projections
    377.0, 610.0, 987.0, 1597.0, 2584.0, 4181.0,
];

/// Create default level configurations for time zones
/// Uses Fibonacci sequence for bar offsets (1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233)
pub fn default_time_zone_configs() -> Vec<FibLevelConfig> {
    ALL_TIME_ZONES.iter().map(|&level| {
        let mut config = FibLevelConfig::new(level);
        // Only enable main time zones by default
        config.visible = MAIN_TIME_ZONES.contains(&level);
        config
    }).collect()
}

fn default_true() -> bool { true }

fn default_label_position() -> String { "left".to_string() }

/// Fibonacci Time Zones - vertical lines at Fib intervals
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibTimeZones {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Starting bar index
    pub start_bar: f64,
    /// Starting price (for anchor display)
    pub start_price: f64,
    /// Fibonacci level configurations (with individual colors/widths)
    #[serde(default = "default_time_zone_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    /// Show zone labels
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

impl FibTimeZones {
    /// Create new Fibonacci time zones
    pub fn new(start_bar: f64, start_price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_time_zones".to_string(),
                display_name: "Fib Time Zones".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            start_bar,
            start_price,
            level_configs: default_time_zone_configs(),
            show_labels: true,
            show_percentages: true,
            label_position: "left".to_string(),
            show_as_percent: true,
        }
    }

    /// Get bar positions for all visible zones based on level configs
    pub fn zone_bars(&self) -> Vec<(f64, &FibLevelConfig)> {
        self.level_configs
            .iter()
            .filter(|cfg| cfg.visible)
            .map(|cfg| {
                // Use level value as the bar offset multiplier
                // For time zones, level represents the bar offset from start
                let bar_offset = cfg.level;
                (self.start_bar + bar_offset, cfg)
            })
            .collect()
    }
}

impl Primitive for FibTimeZones {
    fn type_id(&self) -> &'static str {
        "fib_time_zones"
    }

    fn display_name(&self) -> &str {
        &self.data.display_name
    }

    fn kind(&self) -> PrimitiveKind {
        PrimitiveKind::Fibonacci
    }

    fn click_behavior(&self) -> ClickBehavior {
        ClickBehavior::SingleClick
    }

    fn data(&self) -> &PrimitiveData {
        &self.data
    }

    fn data_mut(&mut self) -> &mut PrimitiveData {
        &mut self.data
    }

    fn points(&self) -> Vec<(f64, f64)> {
        vec![(self.start_bar, self.start_price)]
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        if let Some(&(bar, price)) = points.first() {
            self.start_bar = bar;
            self.start_price = price;
        }
    }

    fn translate(&mut self, bar_delta: f64, price_delta: f64) {
        self.start_bar += bar_delta;
        self.start_price += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, bar: f64, price: f64) {
        match point_type {
            ControlPointType::Point1 | ControlPointType::Move => {
                self.start_bar = bar;
                self.start_price = price;
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
        let start_x = viewport.bar_to_x_f64(self.start_bar);
        let start_y = viewport.price_to_y(self.start_price, price_scale.price_min, price_scale.price_max);

        // Check anchor point
        if check_point_hit(screen_x, screen_y, start_x, start_y) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }

        // Check each vertical zone line (only visible ones)
        for (zone_bar, _cfg) in self.zone_bars() {
            let zone_x = viewport.bar_to_x_f64(zone_bar);
            if (screen_x - zone_x).abs() < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }

        // Check starting vertical line
        if (screen_x - start_x).abs() < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        HitTestResult::Miss
    }

    fn control_points(
        &self,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Vec<ControlPoint> {
        let x = viewport.bar_to_x_f64(self.start_bar);
        let y = viewport.price_to_y(self.start_price, price_scale.price_min, price_scale.price_max);

        vec![ControlPoint::point1(x, y)]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let start_x = ctx.bar_to_x(self.start_bar);
        let start_y = ctx.price_to_y(self.start_price);
        let chart_height = ctx.chart_height();

        // Draw starting vertical line with primitive style
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

        // === FILL RENDERING (before lines so lines are on top) ===
        // Collect visible zones with their X coordinates
        let zone_data: Vec<(f64, &FibLevelConfig)> = self.zone_bars();
        let mut visible_zones: Vec<(usize, f64, f64)> = zone_data
            .iter()
            .enumerate()
            .filter(|(_, (_, cfg))| cfg.visible)
            .map(|(idx, (zone_bar, _))| {
                let zone_x = ctx.bar_to_x(*zone_bar);
                (idx, *zone_bar, zone_x)
            })
            .collect();
        visible_zones.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Draw fills between adjacent visible vertical lines
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

        // Draw vertical lines at each visible Fibonacci zone with individual colors/widths
        for (zone_bar, cfg) in self.zone_bars() {
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

            let zone_x = ctx.bar_to_x(zone_bar);

            // Build label and calculate gap
            let (label, label_y, gap_half_height) = if self.show_labels || self.show_percentages {
                let lbl = {
                    let mut label_parts = Vec::new();
                    if self.show_percentages {
                        // For time zones, show the bar offset directly (no percentage conversion)
                        let lvl = cfg.level;
                        if (lvl - lvl.round()).abs() < 0.0001 {
                            label_parts.push(format!("{}", lvl as i32));
                        } else {
                            label_parts.push(format!("{:.1}", lvl));
                        }
                    }
                    label_parts.join(" ")
                };

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
                    ctx.move_to(crisp(zone_x, dpr), 0.0);
                    ctx.line_to(crisp(zone_x, dpr), gap_y_start);
                }

                // Draw line from gap to bottom
                if gap_y_end < chart_height {
                    ctx.move_to(crisp(zone_x, dpr), gap_y_end);
                    ctx.line_to(crisp(zone_x, dpr), chart_height);
                }
            } else {
                ctx.move_to(crisp(zone_x, dpr), 0.0);
                ctx.line_to(crisp(zone_x, dpr), chart_height);
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
                        ctx.fill_text(lbl, zone_x, chart_height / 2.0);
                    }
                    _ => { // "left" = top
                        ctx.set_text_baseline(crate::render::TextBaseline::Top);
                        ctx.fill_text(lbl, zone_x, 4.0);
                    }
                }
            }
        }
        ctx.set_line_dash(&[]);

        // Render text if present with proper v_align positioning
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // For vertical lines, h_align determines which zone line the text is on
                // v_align determines Y position: Start=top, Center=middle, End=bottom
                let zone_bars: Vec<f64> = self.zone_bars().into_iter().map(|(bar, _)| bar).collect();
                let zone_count = zone_bars.len();

                // Calculate text X position based on h_align (which zone)
                let text_x = if zone_count == 0 {
                    start_x
                } else {
                    match text.h_align {
                        TextAlign::Start => ctx.bar_to_x(zone_bars[0]),
                        TextAlign::Center => {
                            let mid_idx = zone_count / 2;
                            ctx.bar_to_x(zone_bars[mid_idx])
                        }
                        TextAlign::End => ctx.bar_to_x(*zone_bars.last().unwrap()),
                    }
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

            ctx.begin_path();
            ctx.arc(start_x, start_y, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
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
            ConfigProperty::show_labels(self.show_labels).with_order(10),
            ConfigProperty::levels(self.show_percentages).with_order(11),
            ConfigProperty::show_as_percent(self.show_as_percent).with_order(12),
            ConfigProperty::label_position(&self.label_position).with_order(13),
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

// =============================================================================
// Factory Registration
// =============================================================================

fn create_fib_time_zones(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar, price) = points.first().copied().unwrap_or((0.0, 0.0));
    Box::new(FibTimeZones::new(bar, price, color))
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
        factory: create_fib_time_zones,
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
