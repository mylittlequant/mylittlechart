//! Fibonacci Spiral primitive
//!
//! A logarithmic spiral based on the golden ratio (phi = 1.618).
//! Commonly used to identify potential support/resistance areas.

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle, TextAlign,
    HIT_TOLERANCE, CONTROL_POINT_RADIUS,
    RenderContext, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
    config::FibLevelConfig,
};
use super::retracement::default_level_configs;

/// Golden ratio
pub const PHI: f64 = 1.618033988749895;

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

/// Fibonacci Spiral
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibSpiral {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Center bar
    pub center_bar: f64,
    /// Center price
    pub center_price: f64,
    /// Edge bar (defines initial radius)
    pub edge_bar: f64,
    /// Edge price
    pub edge_price: f64,
    /// Fibonacci level configurations (with individual colors/widths)
    #[serde(default = "default_level_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    /// Number of rotations
    #[serde(default = "default_rotations")]
    pub rotations: f64,
    /// Clockwise direction
    #[serde(default = "default_true")]
    pub clockwise: bool,
    /// Flip horizontally
    #[serde(default)]
    pub flip_horizontal: bool,
    /// Flip vertically
    #[serde(default)]
    pub flip_vertical: bool,
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
fn default_rotations() -> f64 { 3.0 }
fn default_label_position() -> String { "left".to_string() }

impl FibSpiral {
    /// Create a new Fibonacci spiral
    pub fn new(center_bar: f64, center_price: f64, edge_bar: f64, edge_price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_spiral".to_string(),
                display_name: "Fib Spiral".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            center_bar,
            center_price,
            edge_bar,
            edge_price,
            level_configs: default_level_configs(),
            rotations: 3.0,
            clockwise: true,
            flip_horizontal: false,
            flip_vertical: false,
            show_percentages: true,
            label_position: "left".to_string(),
            show_as_percent: true,
        }
    }

    /// Calculate spiral points for rendering
    /// Returns points in (bar, price) coordinates
    pub fn spiral_points(&self, num_points: usize) -> Vec<(f64, f64)> {
        let initial_radius_bar = (self.edge_bar - self.center_bar).abs();
        let initial_radius_price = (self.edge_price - self.center_price).abs();

        // Logarithmic spiral: r = a * e^(b*theta)
        // For golden spiral: b = ln(phi) / (pi/2)
        let b = PHI.ln() / (PI / 2.0);

        let mut points = Vec::with_capacity(num_points);
        let max_angle = self.rotations * 2.0 * PI;

        for i in 0..num_points {
            let t = i as f64 / (num_points - 1) as f64;
            let theta = t * max_angle;

            let r = (-b * theta).exp(); // Spiral inward
            let angle = if self.clockwise { theta } else { -theta };

            let mut dx = r * angle.cos();
            let mut dy = r * angle.sin();

            if self.flip_horizontal {
                dx = -dx;
            }
            if self.flip_vertical {
                dy = -dy;
            }

            let bar = self.center_bar + dx * initial_radius_bar;
            let price = self.center_price + dy * initial_radius_price;

            points.push((bar, price));
        }

        points
    }
}

impl Primitive for FibSpiral {
    fn type_id(&self) -> &'static str {
        "fib_spiral"
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
        vec![
            (self.center_bar, self.center_price),
            (self.edge_bar, self.edge_price),
        ]
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        if let Some(&(bar, price)) = points.first() {
            self.center_bar = bar;
            self.center_price = price;
        }
        if let Some(&(bar, price)) = points.get(1) {
            self.edge_bar = bar;
            self.edge_price = price;
        }
    }

    fn translate(&mut self, bar_delta: f64, price_delta: f64) {
        self.center_bar += bar_delta;
        self.edge_bar += bar_delta;
        self.center_price += price_delta;
        self.edge_price += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, bar: f64, price: f64) {
        match point_type {
            ControlPointType::Point1 => {
                let bar_delta = bar - self.center_bar;
                let price_delta = price - self.center_price;
                self.center_bar = bar;
                self.center_price = price;
                self.edge_bar += bar_delta;
                self.edge_price += price_delta;
            }
            ControlPointType::Point2 => {
                self.edge_bar = bar;
                self.edge_price = price;
            }
            ControlPointType::Move => {
                let bar_delta = bar - self.center_bar;
                let price_delta = price - self.center_price;
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
        let cx = viewport.bar_to_x_f64(self.center_bar);
        let cy = viewport.price_to_y(self.center_price, price_scale.price_min, price_scale.price_max);
        let ex = viewport.bar_to_x_f64(self.edge_bar);
        let ey = viewport.price_to_y(self.edge_price, price_scale.price_min, price_scale.price_max);

        // Check control points
        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        if check_point_hit(screen_x, screen_y, ex, ey) {
            return HitTestResult::ControlPoint(ControlPointType::Point2);
        }

        // Check spiral path
        let spiral = self.spiral_points(200);
        for window in spiral.windows(2) {
            let (bar1, price1) = window[0];
            let (bar2, price2) = window[1];

            let x1 = viewport.bar_to_x_f64(bar1);
            let y1 = viewport.price_to_y(price1, price_scale.price_min, price_scale.price_max);
            let x2 = viewport.bar_to_x_f64(bar2);
            let y2 = viewport.price_to_y(price2, price_scale.price_min, price_scale.price_max);

            if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
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
        let cx = viewport.bar_to_x_f64(self.center_bar);
        let cy = viewport.price_to_y(self.center_price, price_scale.price_min, price_scale.price_max);
        let ex = viewport.bar_to_x_f64(self.edge_bar);
        let ey = viewport.price_to_y(self.edge_price, price_scale.price_min, price_scale.price_max);

        vec![
            ControlPoint::point1(cx, cy),
            ControlPoint::point2(ex, ey),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let cx = ctx.bar_to_x(self.center_bar);
        let cy = ctx.price_to_y(self.center_price);
        let ex = ctx.bar_to_x(self.edge_bar);
        let ey = ctx.price_to_y(self.edge_price);

        // Generate spiral points
        let spiral_data = self.spiral_points(200);

        // === FILL RENDERING (before lines so lines are on top) ===
        // Collect visible levels sorted by level value for fill rendering
        let total_points = spiral_data.len();
        if total_points >= 2 {
            let mut visible_levels: Vec<(usize, f64, usize, usize)> = self.level_configs
                .iter()
                .enumerate()
                .filter(|(_, cfg)| cfg.visible)
                .map(|(idx, cfg)| {
                    let start_idx = (cfg.level * total_points as f64 / (self.rotations * 1.0)).floor() as usize;
                    let end_idx = ((cfg.level + 0.1) * total_points as f64 / (self.rotations * 1.0)).ceil() as usize;
                    let start_idx = start_idx.min(total_points - 1);
                    let end_idx = end_idx.min(total_points).max(start_idx + 1);
                    (idx, cfg.level, start_idx, end_idx)
                })
                .collect();
            visible_levels.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            // Draw fills between adjacent spiral segments (wedge-like fills through center)
            for i in 0..visible_levels.len().saturating_sub(1) {
                let (idx, _, start1, end1) = visible_levels[i];
                let (_, _, start2, end2) = visible_levels[i + 1];

                let cfg = &self.level_configs[idx];
                if cfg.fill_enabled && start1 < total_points && start2 < total_points {
                    let fill_color = cfg.fill_color.as_deref()
                        .or(cfg.color.as_deref())
                        .unwrap_or(&self.data.color.stroke);

                    ctx.set_fill_color_alpha(fill_color, cfg.fill_opacity);
                    ctx.begin_path();

                    // Draw from center, along first segment, then back along second segment
                    ctx.move_to(cx, cy);

                    // First segment points (outer edge)
                    for &(bar, price) in &spiral_data[start1..end1.min(total_points)] {
                        ctx.line_to(ctx.bar_to_x(bar), ctx.price_to_y(price));
                    }

                    // Second segment points (in reverse for proper fill)
                    for j in (start2..end2.min(total_points)).rev() {
                        let (bar, price) = spiral_data[j];
                        ctx.line_to(ctx.bar_to_x(bar), ctx.price_to_y(price));
                    }

                    ctx.close_path();
                    ctx.fill();
                    ctx.reset_alpha();
                }
            }
        }

        // Render spiral segments based on level configs
        // Each level config represents a segment/arc of the spiral
        // The level value (0.0 to 1.0+) maps to progress along the spiral
        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            // Use level-specific color or fall back to primitive color
            let color = cfg.color.as_deref().unwrap_or(&self.data.color.stroke);
            ctx.set_stroke_color(color);

            // Use level-specific width or fall back to primitive width
            let width = cfg.width.unwrap_or(self.data.width);
            ctx.set_stroke_width(width);

            // Parse style from config or use primitive style
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

            // Calculate segment range based on level
            // Each level represents a quarter turn (90 degrees) of the spiral
            // Level 0.0 = start, 0.236 = 23.6% through, etc.
            let total_points = spiral_data.len();
            if total_points < 2 {
                continue;
            }

            // Map level to point indices
            // The level value maps to the spiral progress (0.0 = start, 1.0 = one full rotation)
            let start_idx = (cfg.level * total_points as f64 / (self.rotations * 1.0)).floor() as usize;
            let end_idx = ((cfg.level + 0.1) * total_points as f64 / (self.rotations * 1.0)).ceil() as usize;

            let start_idx = start_idx.min(total_points - 1);
            let end_idx = end_idx.min(total_points).max(start_idx + 1);

            if start_idx >= total_points - 1 {
                continue;
            }

            // Draw this segment
            ctx.begin_path();
            let (bar, price) = spiral_data[start_idx];
            ctx.move_to(ctx.bar_to_x(bar), ctx.price_to_y(price));

            for &(bar, price) in &spiral_data[(start_idx + 1)..end_idx] {
                ctx.line_to(ctx.bar_to_x(bar), ctx.price_to_y(price));
            }
            ctx.stroke();

            // Draw label for this segment
            if self.show_percentages {
                let label = {
                    let mut label_parts = Vec::new();
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
                    label_parts.join(" ")
                };

                if !label.is_empty() && start_idx < spiral_data.len() {
                    let (bar, price) = spiral_data[start_idx];
                    let lx = ctx.bar_to_x(bar);
                    let ly = ctx.price_to_y(price);

                    ctx.set_font("11px sans-serif");
                    ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                    ctx.set_fill_color(color);
                    ctx.set_text_align(crate::render::TextAlign::Left);
                    ctx.fill_text(&label, lx, ly);
                }
            }
        }
        ctx.set_line_dash(&[]);

        // Render text if present with proper v_align positioning
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Calculate spiral bounding box from spiral points
                let spiral_screen_points: Vec<(f64, f64)> = spiral_data.iter()
                    .map(|&(bar, price)| (ctx.bar_to_x(bar), ctx.price_to_y(price)))
                    .collect();

                let (min_x, max_x, min_y, max_y) = if spiral_screen_points.is_empty() {
                    (cx, ex, cy, ey)
                } else {
                    let min_x = spiral_screen_points.iter().fold(f64::INFINITY, |a, &(x, _)| a.min(x));
                    let max_x = spiral_screen_points.iter().fold(f64::NEG_INFINITY, |a, &(x, _)| a.max(x));
                    let min_y = spiral_screen_points.iter().fold(f64::INFINITY, |a, &(_, y)| a.min(y));
                    let max_y = spiral_screen_points.iter().fold(f64::NEG_INFINITY, |a, &(_, y)| a.max(y));
                    (min_x, max_x, min_y, max_y)
                };

                // Calculate text X position based on h_align
                let text_x = match text.h_align {
                    TextAlign::Start => min_x,           // Left edge of spiral
                    TextAlign::Center => (min_x + max_x) / 2.0, // Center
                    TextAlign::End => max_x,             // Right edge of spiral
                };

                // Calculate text Y position based on v_align:
                // - Start: above the spiral (smallest screen Y at top)
                // - Center: at the center of the spiral
                // - End: below the spiral (largest screen Y at bottom)
                let text_y = match text.v_align {
                    TextAlign::Start => {
                        let text_offset = 8.0 + text.font_size / 2.0;
                        min_y - text_offset  // Above spiral
                    }
                    TextAlign::Center => {
                        (min_y + max_y) / 2.0  // Center of spiral
                    }
                    TextAlign::End => {
                        let text_offset = 8.0 + text.font_size / 2.0;
                        max_y + text_offset  // Below spiral
                    }
                };

                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            for (px, py) in [(cx, cy), (ex, ey)] {
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
            ConfigProperty::levels(self.show_percentages).with_order(10),
            ConfigProperty::show_as_percent(self.show_as_percent).with_order(11),
            ConfigProperty::label_position(&self.label_position).with_order(12),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &super::super::config::PropertyValue) -> bool {
        use super::super::config::PropertyValue;
        match id {
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

fn create_fib_spiral(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar1, price1) = points.first().copied().unwrap_or((0.0, 0.0));
    let (bar2, price2) = points.get(1).copied().unwrap_or((bar1 + 20.0, price1 + 10.0));
    Box::new(FibSpiral::new(bar1, price1, bar2, price2, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "fib_spiral",
        display_name: "Fib Spiral",
        kind: PrimitiveKind::Fibonacci,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Golden ratio logarithmic spiral",
        icon: "fib_spiral",
        default_color: "#F7B93E",
        factory: create_fib_spiral,
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
