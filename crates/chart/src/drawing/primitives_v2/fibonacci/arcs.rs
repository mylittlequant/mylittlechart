//! Fibonacci Arcs primitive
//!
//! Curved arcs at Fibonacci ratios from a baseline.
//! Arcs emanate from the second point at Fib ratios of the distance.

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
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

/// Fibonacci Arcs
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibArcs {
    /// Common primitive data
    pub data: PrimitiveData,
    /// First point bar
    pub bar1: f64,
    /// First point price
    pub price1: f64,
    /// Second point bar (arc center)
    pub bar2: f64,
    /// Second point price
    pub price2: f64,
    /// Fibonacci level configurations (with individual colors/widths)
    #[serde(default = "default_level_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    /// Show labels
    #[serde(default = "default_true")]
    pub show_labels: bool,
    /// Full circle (360°) or semi-circle
    #[serde(default)]
    pub full_circle: bool,
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

impl FibArcs {
    /// Create new Fibonacci arcs
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_arcs".to_string(),
                display_name: "Fib Arcs".to_string(),
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
            full_circle: false,
            show_percentages: true,
            label_position: "center".to_string(),
            show_as_percent: true,
        }
    }

    /// Get the base radius (distance between points)
    pub fn base_distance(&self, viewport: &Viewport, price_scale: &PriceScale) -> f64 {
        let x1 = viewport.bar_to_x_f64(self.bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(self.bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt()
    }

    /// Get the angle of the baseline
    pub fn baseline_angle(&self, viewport: &Viewport, price_scale: &PriceScale) -> f64 {
        let x1 = viewport.bar_to_x_f64(self.bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(self.bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        (y2 - y1).atan2(x2 - x1)
    }
}

impl Primitive for FibArcs {
    fn type_id(&self) -> &'static str {
        "fib_arcs"
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

        // Check baseline
        if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        // Calculate base radii for ellipse hit testing
        let base_rx = (x2 - x1).abs().max(1.0);
        let base_ry = (y2 - y1).abs().max(1.0);

        // Check each arc level using ellipse equation (only visible levels)
        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            let rx = base_rx * cfg.level;
            let ry = base_ry * cfg.level;

            // Normalized distance for ellipse
            if rx > 0.001 && ry > 0.001 {
                let nx = (screen_x - x2) / rx;
                let ny = (screen_y - y2) / ry;
                let dist = (nx * nx + ny * ny).sqrt();

                if (dist - 1.0).abs() < HIT_TOLERANCE / rx.min(ry) {
                    // For semi-ellipse, check if angle is valid
                    if !self.full_circle {
                        let angle_to_point = (screen_y - y2).atan2(screen_x - x2);
                        let baseline_angle = self.baseline_angle(viewport, price_scale);
                        let relative_angle = angle_to_point - baseline_angle;

                        // Semi-ellipse faces away from point 1
                        if relative_angle.abs() <= PI / 2.0 || (relative_angle.abs() - PI).abs() <= PI / 2.0 {
                            return HitTestResult::Body;
                        }
                    } else {
                        return HitTestResult::Body;
                    }
                }
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

        // Calculate base radii from data coordinates for ellipse behavior
        let base_rx = (x2 - x1).abs().max(1.0);
        let base_ry = (y2 - y1).abs().max(1.0);

        let baseline_angle = (y2 - y1).atan2(x2 - x1);

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

        // === FILL RENDERING (before lines so lines are on top) ===
        // Collect visible levels sorted by level value for fill rendering
        let mut visible_levels: Vec<(usize, f64, f64, f64)> = self.level_configs
            .iter()
            .enumerate()
            .filter(|(_, cfg)| cfg.visible)
            .map(|(idx, cfg)| {
                let rx = base_rx * cfg.level;
                let ry = base_ry * cfg.level;
                (idx, cfg.level, rx, ry)
            })
            .collect();
        visible_levels.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Draw fills between adjacent visible arcs
        for i in 0..visible_levels.len().saturating_sub(1) {
            let (idx, _, rx1, ry1) = visible_levels[i];
            let (_, _, rx2, ry2) = visible_levels[i + 1];

            let cfg = &self.level_configs[idx];
            if cfg.fill_enabled {
                let fill_color = cfg.fill_color.as_deref()
                    .or(cfg.color.as_deref())
                    .unwrap_or(&self.data.color.stroke);

                ctx.set_fill_color_alpha(fill_color, cfg.fill_opacity);
                ctx.begin_path();

                if self.full_circle {
                    // Ring fill between two full circles
                    ctx.ellipse(x2, y2, rx2, ry2, 0.0, 0.0, std::f64::consts::TAU);
                    ctx.ellipse(x2, y2, rx1, ry1, 0.0, std::f64::consts::TAU, 0.0);
                } else {
                    // Semi-arc fill (wedge shape between two arcs)
                    let arc_start = baseline_angle - PI / 2.0;
                    let arc_end = baseline_angle + PI / 2.0;

                    // Outer arc
                    ctx.ellipse(x2, y2, rx2, ry2, 0.0, arc_start, arc_end);
                    // Inner arc (reverse direction)
                    ctx.ellipse(x2, y2, rx1, ry1, 0.0, arc_end, arc_start);
                    ctx.close_path();
                }
                ctx.fill();
                ctx.reset_alpha();
            }
        }

        // Draw elliptical arcs at each level, centered at point 2 (only visible levels)
        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            let rx = base_rx * cfg.level;
            let ry = base_ry * cfg.level;

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
                _ => self.data.style.clone(),
            };

            match line_style {
                LineStyle::Solid => ctx.set_line_dash(&[]),
                LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
                LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
                LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
                LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
            }

            // Build label and calculate gap angle
            let (label, gap_angle, gap_half_angle) = if self.show_labels || self.show_percentages {
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
                    // Calculate gap angle based on label position
                    let center_angle = match self.label_position.as_str() {
                        "right" => 0.0,                        // Right edge
                        "center" => -std::f64::consts::FRAC_PI_2, // Top
                        _ => std::f64::consts::PI,             // Left edge
                    };

                    // Calculate gap size based on label width and arc radius
                    let char_width = 6.5;
                    let text_width = label.len() as f64 * char_width;
                    let avg_radius = (rx + ry) / 2.0;
                    let half_gap = if avg_radius > 0.001 {
                        (text_width / 2.0 / avg_radius).min(0.5)
                    } else {
                        0.0
                    };

                    (Some(label), center_angle, half_gap)
                } else {
                    (None, 0.0, 0.0)
                }
            } else {
                (None, 0.0, 0.0)
            };

            // Draw arc with or without gap
            ctx.begin_path();
            if self.full_circle {
                if label.is_some() && gap_half_angle > 0.001 {
                    let gap_start = gap_angle - gap_half_angle;
                    let gap_end = gap_angle + gap_half_angle;
                    ctx.ellipse(x2, y2, rx, ry, 0.0, gap_end, gap_start + std::f64::consts::TAU);
                } else {
                    ctx.ellipse(x2, y2, rx, ry, 0.0, 0.0, std::f64::consts::TAU);
                }
            } else {
                // Semi-ellipse facing away from point 1
                let arc_start = baseline_angle - PI / 2.0;
                let arc_end = baseline_angle + PI / 2.0;

                if label.is_some() && gap_half_angle > 0.001 {
                    let gap_start = gap_angle - gap_half_angle;
                    let gap_end = gap_angle + gap_half_angle;

                    // Check if gap is within the semi-arc range
                    // Draw two arcs with gap in between
                    if gap_start > arc_start && gap_end < arc_end {
                        ctx.ellipse(x2, y2, rx, ry, 0.0, arc_start, gap_start);
                        ctx.stroke();
                        ctx.begin_path();
                        ctx.ellipse(x2, y2, rx, ry, 0.0, gap_end, arc_end);
                    } else {
                        ctx.ellipse(x2, y2, rx, ry, 0.0, arc_start, arc_end);
                    }
                } else {
                    ctx.ellipse(x2, y2, rx, ry, 0.0, arc_start, arc_end);
                }
            }
            ctx.stroke();

            // Draw label in the gap
            if let Some(ref lbl) = label {
                ctx.set_font("11px sans-serif");
                ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                ctx.set_fill_color(color);

                // Position label at the edge of the arc based on label_position
                let (label_x, label_y) = match self.label_position.as_str() {
                    "right" => (x2 + rx, y2),  // Right edge of arc
                    "center" => (x2, y2 - ry), // Top of arc
                    _ => (x2 - rx, y2),        // "left" - left edge of arc
                };

                ctx.set_text_align(crate::render::TextAlign::Center);
                ctx.fill_text(lbl, label_x, label_y);
            }
        }
        ctx.set_line_dash(&[]);

        // Render text if present with proper v_align positioning
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Calculate text X position based on h_align along baseline
                let text_x = match text.h_align {
                    TextAlign::Start => x1,
                    TextAlign::Center => (x1 + x2) / 2.0,
                    TextAlign::End => x2,
                };

                // Calculate text Y position based on v_align:
                // - Start: above the outermost arc (smallest screen Y at top)
                // - Center: at the center point (on the 0.5 level arc)
                // - End: below the outermost arc (largest screen Y at bottom)
                let text_y = match text.v_align {
                    TextAlign::Start | TextAlign::End => {
                        // Find outermost visible arc level (max level)
                        let max_level = self.level_configs.iter()
                            .filter(|cfg| cfg.visible)
                            .fold(0.0_f64, |a, cfg| a.max(cfg.level));
                        let outer_ry = base_ry * max_level;

                        let text_offset = 8.0 + text.font_size / 2.0;
                        if matches!(text.v_align, TextAlign::Start) {
                            y2 - outer_ry - text_offset  // Above outermost arc
                        } else {
                            y2 + outer_ry + text_offset  // Below outermost arc
                        }
                    }
                    TextAlign::Center => {
                        // On the 0.5 level arc at the apex (along baseline direction)
                        let median_ry = base_ry * 0.5;
                        // The midpoint on the arc is at baseline_angle direction
                        y2 - median_ry * baseline_angle.sin()
                    }
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
            ConfigProperty::full_circle(self.full_circle).with_order(20),
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
            "full_circle" => {
                if let PropertyValue::Boolean(v) = value {
                    self.full_circle = *v;
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

fn create_fib_arcs(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar1, price1) = points.first().copied().unwrap_or((0.0, 0.0));
    let (bar2, price2) = points.get(1).copied().unwrap_or((bar1 + 20.0, price1 + 10.0));
    Box::new(FibArcs::new(bar1, price1, bar2, price2, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "fib_arcs",
        display_name: "Fib Arcs",
        kind: PrimitiveKind::Fibonacci,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Curved arcs at Fibonacci ratios",
        icon: "fib_arcs",
        default_color: "#F7B93E",
        factory: create_fib_arcs,
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
