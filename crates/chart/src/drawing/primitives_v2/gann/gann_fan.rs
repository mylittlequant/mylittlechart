//! Gann Fan primitive
//!
//! Fan lines radiating from a single point at Gann angles.
//! Standard angles: 1x8, 1x4, 1x3, 1x2, 1x1, 2x1, 3x1, 4x1, 8x1

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    HIT_TOLERANCE, CONTROL_POINT_RADIUS,
    RenderContext, crisp, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
    config::FibLevelConfig,
};

/// All Gann angle ratios (price units per time unit)
/// Values represent slope: 8.0 = 8 price units per 1 time unit (steep)
/// 1.0 = 45 degrees, 0.125 = 1 price unit per 8 time units (shallow)
pub const ALL_GANN_ANGLES: &[f64] = &[
    8.0,    // 8x1 - Very steep
    4.0,    // 4x1
    3.0,    // 3x1
    2.0,    // 2x1
    1.0,    // 1x1 - 45 degrees (main)
    0.5,    // 1x2
    0.333,  // 1x3
    0.25,   // 1x4
    0.125,  // 1x8 - Very shallow
];

/// Main angles visible by default
pub const MAIN_GANN_VISIBLE: &[f64] = &[2.0, 1.0, 0.5];

/// Get label for Gann angle ratio
pub fn gann_angle_label(ratio: f64) -> String {
    if ratio >= 1.0 {
        let r = ratio.round() as i32;
        format!("{}x1", r)
    } else {
        let inv = (1.0 / ratio).round() as i32;
        format!("1x{}", inv)
    }
}

/// Create default level configs for Gann Fan
pub fn default_gann_fan_configs() -> Vec<FibLevelConfig> {
    ALL_GANN_ANGLES.iter().map(|&ratio| {
        let mut config = FibLevelConfig::new(ratio);
        config.visible = MAIN_GANN_VISIBLE.contains(&ratio);
        config
    }).collect()
}

/// Deserialize level configs with backward compatibility
fn deserialize_level_configs<'de, D>(deserializer: D) -> Result<Vec<FibLevelConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, SeqAccess, Visitor};

    struct LevelConfigsVisitor;

    impl<'de> Visitor<'de> for LevelConfigsVisitor {
        type Value = Vec<FibLevelConfig>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a sequence of FibLevelConfig objects or f64 ratio values")
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
                } else if let Some(ratio) = value.as_f64() {
                    configs.push(FibLevelConfig::new(ratio));
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

/// Gann Fan
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GannFan {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Origin bar
    pub bar1: f64,
    /// Origin price
    pub price1: f64,
    /// Target bar (defines scale)
    pub bar2: f64,
    /// Target price
    pub price2: f64,
    /// Gann angle configurations (ratio values: 8.0=8x1, 1.0=1x1, 0.5=1x2, etc.)
    #[serde(default = "default_gann_fan_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    /// Show labels
    #[serde(default = "default_true")]
    pub show_labels: bool,
    /// Extend lines to chart edge
    #[serde(default = "default_true")]
    pub extend: bool,
    /// Direction: true = upward fan, false = downward fan
    #[serde(default = "default_true")]
    pub upward: bool,
}

impl GannFan {
    /// Create a new Gann fan
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "gann_fan".to_string(),
                display_name: "Gann Fan".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            bar1,
            price1,
            bar2,
            price2,
            level_configs: default_gann_fan_configs(),
            show_labels: true,
            extend: true,
            upward: true,
        }
    }

    /// Get the price scale (price per bar) based on the two points
    pub fn price_per_bar(&self) -> f64 {
        let bar_diff = (self.bar2 - self.bar1).abs();
        let price_diff = (self.price2 - self.price1).abs();
        if bar_diff == 0.0 { 1.0 } else { price_diff / bar_diff }
    }

    /// Get visible levels sorted by ratio (for fill rendering between adjacent angles)
    pub fn visible_levels_sorted(&self) -> Vec<&FibLevelConfig> {
        let mut visible: Vec<_> = self.level_configs.iter()
            .filter(|c| c.visible)
            .collect();
        visible.sort_by(|a, b| b.level.partial_cmp(&a.level).unwrap_or(std::cmp::Ordering::Equal));
        visible
    }
}

impl Primitive for GannFan {
    fn type_id(&self) -> &'static str {
        "gann_fan"
    }

    fn display_name(&self) -> &str {
        &self.data.display_name
    }

    fn kind(&self) -> PrimitiveKind {
        PrimitiveKind::Gann
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

        let ppb = self.price_per_bar();
        let direction = if self.upward { 1.0 } else { -1.0 };

        // Check each visible fan line
        for config in &self.level_configs {
            if !config.visible {
                continue;
            }
            let ratio = config.level;

            // Calculate a point along this angle
            let bar_delta = 100.0; // Fixed bar distance for the line
            let price_delta = bar_delta * ppb * ratio * direction;

            let end_bar = self.bar1 + bar_delta;
            let end_price = self.price1 + price_delta;

            let end_x = viewport.bar_to_x_f64(end_bar);
            let end_y = viewport.price_to_y(end_price, price_scale.price_min, price_scale.price_max);

            let dist = if self.extend {
                point_to_ray_distance(screen_x, screen_y, x1, y1, end_x, end_y)
            } else {
                point_to_line_distance(screen_x, screen_y, x1, y1, end_x, end_y)
            };

            if dist < HIT_TOLERANCE {
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
        let chart_width = ctx.chart_width();
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

        let ppb = self.price_per_bar();
        let direction = if self.upward { 1.0 } else { -1.0 };

        // === FILL RENDERING (before lines so lines are on top) ===
        // For GannFan, fill is drawn between adjacent angle lines (sectors)
        // Get visible levels sorted by ratio (descending: steepest first)
        let visible = self.visible_levels_sorted();

        for i in 0..visible.len() {
            let config = visible[i];
            if !config.fill_enabled {
                continue;
            }

            // Fill goes from this angle to the next one (or to horizontal if last)
            let ratio1 = config.level;
            let ratio2 = if i + 1 < visible.len() {
                visible[i + 1].level
            } else {
                // Last visible angle - fill to horizontal (ratio 0)
                0.0
            };

            let fill_color = config.fill_color.as_deref()
                .or(config.color.as_deref())
                .unwrap_or(&self.data.color.stroke);

            // Calculate end points for both angles
            let ext = if self.extend {
                (chart_width + chart_height) * 2.0
            } else {
                100.0 * ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt() / 100.0
            };

            // Angle 1 (steeper)
            let bar_delta1 = 100.0;
            let price_delta1 = bar_delta1 * ppb * ratio1 * direction;
            let end_x1_raw = ctx.bar_to_x(self.bar1 + bar_delta1);
            let end_y1_raw = ctx.price_to_y(self.price1 + price_delta1);
            let dx1 = end_x1_raw - x1;
            let dy1 = end_y1_raw - y1;
            let len1 = (dx1 * dx1 + dy1 * dy1).sqrt();
            let (end_x1, end_y1) = if len1 > 0.0 && self.extend {
                (x1 + dx1 / len1 * ext, y1 + dy1 / len1 * ext)
            } else {
                (end_x1_raw, end_y1_raw)
            };

            // Angle 2 (shallower)
            let bar_delta2 = 100.0;
            let price_delta2 = bar_delta2 * ppb * ratio2 * direction;
            let end_x2_raw = ctx.bar_to_x(self.bar1 + bar_delta2);
            let end_y2_raw = ctx.price_to_y(self.price1 + price_delta2);
            let dx2 = end_x2_raw - x1;
            let dy2 = end_y2_raw - y1;
            let len2 = (dx2 * dx2 + dy2 * dy2).sqrt();
            let (end_x2, end_y2) = if len2 > 0.0 && self.extend {
                (x1 + dx2 / len2 * ext, y1 + dy2 / len2 * ext)
            } else {
                (end_x2_raw, end_y2_raw)
            };

            // Draw fill triangle/sector: origin -> end1 -> end2 -> origin
            ctx.set_fill_color_alpha(fill_color, config.fill_opacity);
            ctx.begin_path();
            ctx.move_to(x1, y1);
            ctx.line_to(end_x1, end_y1);
            ctx.line_to(end_x2, end_y2);
            ctx.close_path();
            ctx.fill();
            ctx.reset_alpha();
        }

        // Draw each visible Gann angle line
        for config in &self.level_configs {
            if !config.visible {
                continue;
            }
            let ratio = config.level;

            // Use per-level color/width/style if set
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

            let bar_delta = 100.0;
            let price_delta = bar_delta * ppb * ratio * direction;

            let end_bar = self.bar1 + bar_delta;
            let end_price = self.price1 + price_delta;

            let end_x = ctx.bar_to_x(end_bar);
            let end_y = ctx.price_to_y(end_price);

            ctx.begin_path();
            ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));

            if self.extend {
                // Extend ray to chart edge
                let dx = end_x - x1;
                let dy = end_y - y1;
                let len = (dx * dx + dy * dy).sqrt();
                if len > 0.0 {
                    let ext = (chart_width + chart_height) * 2.0;
                    let nx = dx / len;
                    let ny = dy / len;
                    ctx.line_to(crisp(x1 + nx * ext, dpr), crisp(y1 + ny * ext, dpr));
                }
            } else {
                ctx.line_to(crisp(end_x, dpr), crisp(end_y, dpr));
            }
            ctx.stroke();
        }
        ctx.set_line_dash(&[]);

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // For fan shapes, calculate bounding box from the rays
                // The fan originates from (x1, y1) and spreads out
                // Use the outermost rays (8x1 and 1x8) to determine the bounding area
                let bar_delta = 50.0; // Use a fixed distance for text positioning
                let top_price = self.price1 + bar_delta * ppb * 8.0 * direction;
                let bottom_price = self.price1 + bar_delta * ppb * 0.125 * direction;
                let end_bar = self.bar1 + bar_delta;

                let end_x = ctx.bar_to_x(end_bar);
                let top_y = ctx.price_to_y(top_price);
                let bottom_y = ctx.price_to_y(bottom_price);
                let mid_price = self.price1 + bar_delta * ppb * 1.0 * direction;
                let mid_y = ctx.price_to_y(mid_price);

                let min_x = x1.min(end_x);
                let max_x = x1.max(end_x);
                let min_y = top_y.min(bottom_y);
                let max_y = top_y.max(bottom_y);

                let text_offset = 8.0 + text.font_size / 2.0;
                // Calculate X based on h_align
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => (min_x + max_x) / 2.0,
                    super::super::TextAlign::End => max_x,
                };
                // Start = ABOVE upper boundary, Center = middle, End = BELOW lower boundary
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => min_y - text_offset,
                    super::super::TextAlign::Center => mid_y,
                    super::super::TextAlign::End => max_y + text_offset,
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
                ctx.arc(px, py, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
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

fn create_gann_fan(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar1, price1) = points.first().copied().unwrap_or((0.0, 0.0));
    let (bar2, price2) = points.get(1).copied().unwrap_or((bar1 + 20.0, price1 + 20.0));
    Box::new(GannFan::new(bar1, price1, bar2, price2, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "gann_fan",
        display_name: "Gann Fan",
        kind: PrimitiveKind::Gann,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Fan lines at Gann angles",
        icon: "gann_fan",
        default_color: "#FF9800",
        factory: create_gann_fan,
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
