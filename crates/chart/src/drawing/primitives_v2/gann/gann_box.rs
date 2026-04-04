//! Gann Box primitive
//!
//! A rectangular box divided by Gann angles.
//! Shows price/time relationships with diagonal lines.

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

/// All Gann Box division levels (fraction of box: 0.0 = edge, 0.5 = center, 1.0 = opposite edge)
/// These represent horizontal and vertical grid lines within the box
pub const ALL_GANN_BOX_LEVELS: &[f64] = &[
    0.0,    // Edge (always drawn as box outline)
    0.25,   // 1/4
    0.333,  // 1/3
    0.5,    // Center (1/2)
    0.667,  // 2/3
    0.75,   // 3/4
    1.0,    // Opposite edge
];

/// Main levels visible by default (quarters + center)
pub const MAIN_GANN_BOX_VISIBLE: &[f64] = &[0.25, 0.5, 0.75];

/// Create default level configs for Gann Box
pub fn default_gann_box_configs() -> Vec<FibLevelConfig> {
    ALL_GANN_BOX_LEVELS.iter().map(|&level| {
        let mut config = FibLevelConfig::new(level);
        config.visible = MAIN_GANN_BOX_VISIBLE.contains(&level);
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
            formatter.write_str("a sequence of FibLevelConfig objects or f64 values")
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

/// Gann Box
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GannBox {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Top-left corner bar
    pub bar1: f64,
    /// Top-left corner price
    pub price1: f64,
    /// Bottom-right corner bar
    pub bar2: f64,
    /// Bottom-right corner price
    pub price2: f64,
    /// Grid level configurations (0.0-1.0 fraction of box dimensions)
    #[serde(default = "default_gann_box_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    /// Show angle labels
    #[serde(default = "default_true")]
    pub show_labels: bool,
    /// Show horizontal/vertical grid
    #[serde(default = "default_true")]
    pub show_grid: bool,
    /// Show diagonals (1x1 and anti-diagonal)
    #[serde(default = "default_true")]
    pub show_diagonals: bool,
}

impl GannBox {
    /// Create a new Gann box
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "gann_box".to_string(),
                display_name: "Gann Box".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            bar1,
            price1,
            bar2,
            price2,
            level_configs: default_gann_box_configs(),
            show_labels: true,
            show_grid: true,
            show_diagonals: true,
        }
    }
}

impl Primitive for GannBox {
    fn type_id(&self) -> &'static str {
        "gann_box"
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

        let min_x = x1.min(x2);
        let max_x = x1.max(x2);
        let min_y = y1.min(y2);
        let max_y = y1.max(y2);

        // Check box edges
        if (screen_x - min_x).abs() < HIT_TOLERANCE && screen_y >= min_y && screen_y <= max_y {
            return HitTestResult::Body;
        }
        if (screen_x - max_x).abs() < HIT_TOLERANCE && screen_y >= min_y && screen_y <= max_y {
            return HitTestResult::Body;
        }
        if (screen_y - min_y).abs() < HIT_TOLERANCE && screen_x >= min_x && screen_x <= max_x {
            return HitTestResult::Body;
        }
        if (screen_y - max_y).abs() < HIT_TOLERANCE && screen_x >= min_x && screen_x <= max_x {
            return HitTestResult::Body;
        }

        // Check diagonal 1x1 line
        if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        // Check anti-diagonal
        if point_to_line_distance(screen_x, screen_y, x1, y2, x2, y1) < HIT_TOLERANCE {
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

        let min_x = x1.min(x2);
        let max_x = x1.max(x2);
        let min_y = y1.min(y2);
        let max_y = y1.max(y2);
        let width = max_x - min_x;
        let height = max_y - min_y;

        // === FILL RENDERING (before lines so lines are on top) ===
        // For GannBox, fill is drawn as horizontal bands between adjacent grid levels
        // Sort levels for proper adjacent fill
        let mut sorted_configs: Vec<_> = self.level_configs.iter()
            .filter(|c| c.visible)
            .collect();
        sorted_configs.sort_by(|a, b| a.level.partial_cmp(&b.level).unwrap_or(std::cmp::Ordering::Equal));

        for i in 0..sorted_configs.len() {
            let config = sorted_configs[i];
            if !config.fill_enabled {
                continue;
            }

            let level1 = config.level;
            let level2 = if i + 1 < sorted_configs.len() {
                sorted_configs[i + 1].level
            } else {
                1.0 // Fill to bottom edge
            };

            let fill_color = config.fill_color.as_deref()
                .or(config.color.as_deref())
                .unwrap_or(&self.data.color.stroke);

            // Horizontal band between level1 and level2
            let y_top = min_y + height * level1;
            let y_bottom = min_y + height * level2;

            ctx.set_fill_color_alpha(fill_color, config.fill_opacity);
            ctx.begin_path();
            ctx.rect(min_x, y_top, width, y_bottom - y_top);
            ctx.fill();
            ctx.reset_alpha();
        }

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Draw box outline
        ctx.begin_path();
        ctx.rect(crisp(min_x, dpr), crisp(min_y, dpr), max_x - min_x, max_y - min_y);
        ctx.stroke();

        // Draw grid lines if enabled - using level_configs
        if self.show_grid {
            for config in &self.level_configs {
                if !config.visible {
                    continue;
                }
                let level = config.level;
                // Skip edges (0.0 and 1.0) - they're drawn as box outline
                if !(0.01..=0.99).contains(&level) {
                    continue;
                }

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

                // Vertical grid line at this level
                let gx = min_x + width * level;
                ctx.begin_path();
                ctx.move_to(crisp(gx, dpr), crisp(min_y, dpr));
                ctx.line_to(crisp(gx, dpr), crisp(max_y, dpr));
                ctx.stroke();

                // Horizontal grid line at this level
                let gy = min_y + height * level;
                ctx.begin_path();
                ctx.move_to(crisp(min_x, dpr), crisp(gy, dpr));
                ctx.line_to(crisp(max_x, dpr), crisp(gy, dpr));
                ctx.stroke();
            }
        }

        // Reset to default style for diagonals
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Draw diagonals if enabled
        if self.show_diagonals {
            // Draw main diagonal (1x1)
            ctx.begin_path();
            ctx.move_to(crisp(min_x, dpr), crisp(min_y, dpr));
            ctx.line_to(crisp(max_x, dpr), crisp(max_y, dpr));
            ctx.stroke();

            // Draw anti-diagonal
            ctx.begin_path();
            ctx.move_to(crisp(min_x, dpr), crisp(max_y, dpr));
            ctx.line_to(crisp(max_x, dpr), crisp(min_y, dpr));
            ctx.stroke();
        }

        ctx.set_line_dash(&[]);

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
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
                    super::super::TextAlign::Center => (min_y + max_y) / 2.0,
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

fn create_gann_box(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar1, price1) = points.first().copied().unwrap_or((0.0, 0.0));
    let (bar2, price2) = points.get(1).copied().unwrap_or((bar1 + 20.0, price1 - 10.0));
    Box::new(GannBox::new(bar1, price1, bar2, price2, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "gann_box",
        display_name: "Gann Box",
        kind: PrimitiveKind::Gann,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Gann Box with angle divisions",
        icon: "gann_box",
        default_color: "#FF9800",
        factory: create_gann_box,
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
