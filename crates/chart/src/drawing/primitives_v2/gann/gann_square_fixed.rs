//! Gann Square Fixed primitive
//!
//! A fixed-size Gann square based on a single point.
//! The square maintains equal price/time units.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType, ControlPointCursor,
    PrimitiveColor, LineStyle,
    HIT_TOLERANCE, CONTROL_POINT_RADIUS,
    RenderContext, crisp, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text,
    config::FibLevelConfig,
};

// Use same levels as GannSquare
use super::gann_square::{ALL_GANN_SQUARE_LEVELS, MAIN_GANN_SQUARE_VISIBLE};

/// Create default level configs for Gann Square Fixed
pub fn default_gann_square_fixed_configs() -> Vec<FibLevelConfig> {
    ALL_GANN_SQUARE_LEVELS.iter().map(|&level| {
        let mut config = FibLevelConfig::new(level);
        config.visible = MAIN_GANN_SQUARE_VISIBLE.contains(&level);
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

/// Gann Square Fixed
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GannSquareFixed {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Center bar
    pub center_bar: f64,
    /// Center price
    pub center_price: f64,
    /// Size in bars (horizontal)
    pub bar_size: f64,
    /// Size in price (vertical) - typically equal to bar_size * price_per_bar
    pub price_size: f64,
    /// Ring level configurations (0.0-1.0 fraction from center to edge)
    #[serde(default = "default_gann_square_fixed_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    /// Show labels
    #[serde(default = "default_true")]
    pub show_labels: bool,
    /// Show spiral numbers
    #[serde(default)]
    pub show_numbers: bool,
}

impl GannSquareFixed {
    /// Create a new fixed Gann square
    pub fn new(center_bar: f64, center_price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "gann_square_fixed".to_string(),
                display_name: "Gann Square Fixed".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            center_bar,
            center_price,
            bar_size: 20.0,
            price_size: 10.0,
            level_configs: default_gann_square_fixed_configs(),
            show_labels: true,
            show_numbers: false,
        }
    }

    /// Get the corners of the square
    pub fn corners(&self) -> [(f64, f64); 4] {
        let half_bar = self.bar_size / 2.0;
        let half_price = self.price_size / 2.0;
        [
            (self.center_bar - half_bar, self.center_price + half_price), // top-left
            (self.center_bar + half_bar, self.center_price + half_price), // top-right
            (self.center_bar + half_bar, self.center_price - half_price), // bottom-right
            (self.center_bar - half_bar, self.center_price - half_price), // bottom-left
        ]
    }
}

impl Primitive for GannSquareFixed {
    fn type_id(&self) -> &'static str {
        "gann_square_fixed"
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
        // Return center and corner (for two-point creation)
        vec![
            (self.center_bar, self.center_price),
            (self.center_bar + self.bar_size / 2.0, self.center_price + self.price_size / 2.0),
        ]
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        if let Some(&(bar, price)) = points.first() {
            self.center_bar = bar;
            self.center_price = price;
        }
        if let Some(&(bar, price)) = points.get(1) {
            // Second point defines the corner, so calculate size
            self.bar_size = ((bar - self.center_bar).abs() * 2.0).max(1.0);
            self.price_size = ((price - self.center_price).abs() * 2.0).max(1.0);
        }
    }

    fn translate(&mut self, bar_delta: f64, price_delta: f64) {
        self.center_bar += bar_delta;
        self.center_price += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, bar: f64, price: f64) {
        match point_type {
            ControlPointType::Point1 | ControlPointType::Move => {
                self.center_bar = bar;
                self.center_price = price;
            }
            ControlPointType::Corner(_) => {
                // Corner resize - proportional
                self.bar_size = ((bar - self.center_bar).abs() * 2.0).max(1.0);
                self.price_size = ((price - self.center_price).abs() * 2.0).max(1.0);
            }
            ControlPointType::Edge(0) => {
                // Top edge - adjust price_size
                self.price_size = ((price - self.center_price).abs() * 2.0).max(1.0);
            }
            ControlPointType::Edge(1) => {
                // Right edge - adjust bar_size
                self.bar_size = ((bar - self.center_bar).abs() * 2.0).max(1.0);
            }
            ControlPointType::Edge(2) => {
                // Bottom edge - adjust price_size
                self.price_size = ((self.center_price - price).abs() * 2.0).max(1.0);
            }
            ControlPointType::Edge(3) => {
                // Left edge - adjust bar_size
                self.bar_size = ((self.center_bar - bar).abs() * 2.0).max(1.0);
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

        let corners = self.corners();
        let screen_corners: Vec<(f64, f64)> = corners.iter()
            .map(|(bar, price)| {
                (
                    viewport.bar_to_x_f64(*bar),
                    viewport.price_to_y(*price, price_scale.price_min, price_scale.price_max),
                )
            })
            .collect();

        // Check corner control points first (top-left, top-right, bottom-right, bottom-left)
        for (i, &(x, y)) in screen_corners.iter().enumerate() {
            if check_point_hit(screen_x, screen_y, x, y) {
                return HitTestResult::ControlPoint(ControlPointType::Corner(i as u8));
            }
        }

        // Check edge control points (midpoints: top, right, bottom, left)
        let half_bar_screen = (screen_corners[1].0 - cx).abs();
        let half_price_screen = (cy - screen_corners[0].1).abs();
        let edges = [
            (cx, cy - half_price_screen, 0), // top
            (cx + half_bar_screen, cy, 1),   // right
            (cx, cy + half_price_screen, 2), // bottom
            (cx - half_bar_screen, cy, 3),   // left
        ];
        for (ex, ey, idx) in edges {
            if check_point_hit(screen_x, screen_y, ex, ey) {
                return HitTestResult::ControlPoint(ControlPointType::Edge(idx));
            }
        }

        // Check center point
        if check_point_hit(screen_x, screen_y, cx, cy) {
            return HitTestResult::ControlPoint(ControlPointType::Move);
        }

        // Check square edges
        for i in 0..4 {
            let (x1, y1) = screen_corners[i];
            let (x2, y2) = screen_corners[(i + 1) % 4];
            if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }

        // Check diagonals
        if point_to_line_distance(screen_x, screen_y, screen_corners[0].0, screen_corners[0].1, screen_corners[2].0, screen_corners[2].1) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        if point_to_line_distance(screen_x, screen_y, screen_corners[1].0, screen_corners[1].1, screen_corners[3].0, screen_corners[3].1) < HIT_TOLERANCE {
            return HitTestResult::Body;
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

        let corners = self.corners();
        let screen_corners: Vec<(f64, f64)> = corners.iter()
            .map(|(bar, price)| {
                (
                    viewport.bar_to_x_f64(*bar),
                    viewport.price_to_y(*price, price_scale.price_min, price_scale.price_max),
                )
            })
            .collect();

        let half_bar_screen = (screen_corners[1].0 - cx).abs();
        let half_price_screen = (cy - screen_corners[0].1).abs();

        vec![
            // Center move point
            ControlPoint::move_point(cx, cy),
            // 4 corner points (NW, NE, SE, SW order matches corners())
            ControlPoint::new(ControlPointType::Corner(0), screen_corners[0].0, screen_corners[0].1, ControlPointCursor::ResizeNWSE),
            ControlPoint::new(ControlPointType::Corner(1), screen_corners[1].0, screen_corners[1].1, ControlPointCursor::ResizeNESW),
            ControlPoint::new(ControlPointType::Corner(2), screen_corners[2].0, screen_corners[2].1, ControlPointCursor::ResizeNWSE),
            ControlPoint::new(ControlPointType::Corner(3), screen_corners[3].0, screen_corners[3].1, ControlPointCursor::ResizeNESW),
            // 4 edge points (top, right, bottom, left)
            ControlPoint::new(ControlPointType::Edge(0), cx, cy - half_price_screen, ControlPointCursor::ResizeNS),
            ControlPoint::new(ControlPointType::Edge(1), cx + half_bar_screen, cy, ControlPointCursor::ResizeEW),
            ControlPoint::new(ControlPointType::Edge(2), cx, cy + half_price_screen, ControlPointCursor::ResizeNS),
            ControlPoint::new(ControlPointType::Edge(3), cx - half_bar_screen, cy, ControlPointCursor::ResizeEW),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let cx = ctx.bar_to_x(self.center_bar);
        let cy = ctx.price_to_y(self.center_price);

        let corners = self.corners();
        let screen_corners: Vec<(f64, f64)> = corners.iter()
            .map(|(bar, price)| (ctx.bar_to_x(*bar), ctx.price_to_y(*price)))
            .collect();

        // Calculate half dimensions in screen space
        let min_x = screen_corners.iter().map(|(x, _)| *x).fold(f64::INFINITY, f64::min);
        let max_x = screen_corners.iter().map(|(x, _)| *x).fold(f64::NEG_INFINITY, f64::max);
        let min_y = screen_corners.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
        let max_y = screen_corners.iter().map(|(_, y)| *y).fold(f64::NEG_INFINITY, f64::max);
        let half_w_full = (max_x - min_x) / 2.0;
        let half_h_full = (max_y - min_y) / 2.0;

        // === FILL RENDERING (before lines so lines are on top) ===
        // For GannSquareFixed, fill is drawn as rings between concentric squares
        let mut sorted_configs: Vec<_> = self.level_configs.iter()
            .filter(|c| c.visible)
            .collect();
        sorted_configs.sort_by(|a, b| a.level.partial_cmp(&b.level).unwrap_or(std::cmp::Ordering::Equal));

        for i in 0..sorted_configs.len() {
            let config = sorted_configs[i];
            if !config.fill_enabled {
                continue;
            }

            let ratio1 = config.level;
            let ratio2 = if i + 1 < sorted_configs.len() {
                sorted_configs[i + 1].level
            } else {
                1.0 // Fill to outer edge
            };

            let fill_color = config.fill_color.as_deref()
                .or(config.color.as_deref())
                .unwrap_or(&self.data.color.stroke);

            // Inner square
            let half_w1 = half_w_full * ratio1;
            let half_h1 = half_h_full * ratio1;
            // Outer square
            let half_w2 = half_w_full * ratio2;
            let half_h2 = half_h_full * ratio2;

            // Draw ring as outer rect minus inner rect
            ctx.set_fill_color_alpha(fill_color, config.fill_opacity);
            ctx.begin_path();
            // Outer rectangle (clockwise)
            ctx.move_to(cx - half_w2, cy - half_h2);
            ctx.line_to(cx + half_w2, cy - half_h2);
            ctx.line_to(cx + half_w2, cy + half_h2);
            ctx.line_to(cx - half_w2, cy + half_h2);
            ctx.close_path();
            // Inner rectangle (counter-clockwise to create hole)
            ctx.move_to(cx - half_w1, cy - half_h1);
            ctx.line_to(cx - half_w1, cy + half_h1);
            ctx.line_to(cx + half_w1, cy + half_h1);
            ctx.line_to(cx + half_w1, cy - half_h1);
            ctx.close_path();
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

        // Draw square outline
        ctx.begin_path();
        ctx.move_to(crisp(screen_corners[0].0, dpr), crisp(screen_corners[0].1, dpr));
        for &(x, y) in &screen_corners[1..] {
            ctx.line_to(crisp(x, dpr), crisp(y, dpr));
        }
        ctx.close_path();
        ctx.stroke();

        // Draw diagonals
        ctx.begin_path();
        ctx.move_to(crisp(screen_corners[0].0, dpr), crisp(screen_corners[0].1, dpr));
        ctx.line_to(crisp(screen_corners[2].0, dpr), crisp(screen_corners[2].1, dpr));
        ctx.stroke();

        ctx.begin_path();
        ctx.move_to(crisp(screen_corners[1].0, dpr), crisp(screen_corners[1].1, dpr));
        ctx.line_to(crisp(screen_corners[3].0, dpr), crisp(screen_corners[3].1, dpr));
        ctx.stroke();

        // Draw cardinal lines through center
        ctx.begin_path();
        ctx.move_to(crisp(min_x, dpr), crisp(cy, dpr));
        ctx.line_to(crisp(max_x, dpr), crisp(cy, dpr));
        ctx.stroke();

        ctx.begin_path();
        ctx.move_to(crisp(cx, dpr), crisp(min_y, dpr));
        ctx.line_to(crisp(cx, dpr), crisp(max_y, dpr));
        ctx.stroke();

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

            // Draw center control point
            ctx.begin_path();
            ctx.arc(cx, cy, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.stroke();

            // Draw corner control points
            for &(x, y) in &screen_corners {
                ctx.begin_path();
                ctx.arc(x, y, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }

            // Draw edge control points
            let half_bar_screen = (screen_corners[1].0 - cx).abs();
            let half_price_screen = (cy - screen_corners[0].1).abs();
            let edges = [
                (cx, cy - half_price_screen), // top
                (cx + half_bar_screen, cy),   // right
                (cx, cy + half_price_screen), // bottom
                (cx - half_bar_screen, cy),   // left
            ];
            for (ex, ey) in edges {
                ctx.begin_path();
                ctx.arc(ex, ey, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
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

fn create_gann_square_fixed(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar, price) = points.first().copied().unwrap_or((0.0, 100.0));
    let mut gann = GannSquareFixed::new(bar, price, color);
    if let Some(&(bar2, price2)) = points.get(1) {
        gann.bar_size = ((bar2 - bar).abs() * 2.0).max(1.0);
        gann.price_size = ((price2 - price).abs() * 2.0).max(1.0);
    }
    Box::new(gann)
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "gann_square_fixed",
        display_name: "Gann Square Fixed",
        kind: PrimitiveKind::Gann,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Fixed-size Gann square",
        icon: "gann_square_fixed",
        default_color: "#FF9800",
        factory: create_gann_square_fixed,
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
