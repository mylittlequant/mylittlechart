//! Gann Square Fixed primitive
//!
//! A fixed-size Gann square based on a single point.
//! The square maintains equal price/time units.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType, ControlPointCursor,
    PrimitiveColor, LineStyle,
    HIT_TOLERANCE,
    RenderContext, crisp,
    render_primitive_text, draw_label_plate, level_ramp_color, level_rank,
    config::{FibLevelConfig, LabelStyle},
};
use super::super::types::{Segment, hit_test_strokes};

// Use same levels — and the same ring-visibility filter — as GannSquare
use super::gann_square::{ALL_GANN_SQUARE_LEVELS, MAIN_GANN_SQUARE_VISIBLE, ring_level_visible};

/// Create default level configs for Gann Square Fixed
pub fn default_gann_square_fixed_configs() -> Vec<FibLevelConfig> {
    let mut sorted: Vec<f64> = ALL_GANN_SQUARE_LEVELS.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ALL_GANN_SQUARE_LEVELS.iter().map(|&level| {
        let mut config = FibLevelConfig::new(level);
        config.visible = MAIN_GANN_SQUARE_VISIBLE.contains(&level);
        // Ramp colour by rank instead of every ring falling back to the same stroke color.
        config.color = Some(level_ramp_color(level_rank(level, &sorted), sorted.len()));
        // Band fill on by default — `render` draws it as a square ring
        // (outer rect minus inner rect via nonzero winding), a real closed region.
        config.fill_enabled = true;
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

fn default_bar_size_ms() -> i64 { 1_200_000 }

/// Gann Square Fixed
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GannSquareFixed {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Center timestamp (ms)
    pub center_ts: i64,
    /// Center price
    pub center_price: f64,
    /// Size in milliseconds (horizontal half = bar_size_ms / 2)
    #[serde(default = "default_bar_size_ms")]
    pub bar_size_ms: i64,
    /// Size in price (vertical)
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
    /// Style for ring fraction labels (font, background plate, offset, border)
    #[serde(default)]
    pub label_style: LabelStyle,
}

impl GannSquareFixed {
    /// Create a new fixed Gann square
    pub fn new(center_ts: i64, center_price: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "gann_square_fixed".to_string(),
                display_name: "Gann Square Fixed".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            center_ts,
            center_price,
            bar_size_ms: 1_200_000,
            price_size: 10.0,
            level_configs: default_gann_square_fixed_configs(),
            show_labels: true,
            show_numbers: false,
            label_style: LabelStyle::default(),
        }
    }

    /// Get the corners as (timestamp_ms, price) — top-left, top-right, bottom-right, bottom-left
    pub fn corners(&self) -> [(i64, f64); 4] {
        let half_ts = self.bar_size_ms / 2;
        let half_price = self.price_size / 2.0;
        [
            (self.center_ts - half_ts, self.center_price + half_price), // top-left
            (self.center_ts + half_ts, self.center_price + half_price), // top-right
            (self.center_ts + half_ts, self.center_price - half_price), // bottom-right
            (self.center_ts - half_ts, self.center_price - half_price), // bottom-left
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

    fn points(&self) -> Vec<(i64, f64)> {
        // Return center and corner (for two-point creation)
        vec![
            (self.center_ts, self.center_price),
            (self.center_ts + self.bar_size_ms / 2, self.center_price + self.price_size / 2.0),
        ]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if let Some(&(ts, price)) = points.first() {
            self.center_ts = ts;
            self.center_price = price;
        }
        if let Some(&(ts, price)) = points.get(1) {
            // Second point defines the corner, so calculate size
            let ts_diff = (ts - self.center_ts).abs() * 2;
            self.bar_size_ms = ts_diff.max(60_000);
            self.price_size = ((price - self.center_price).abs() * 2.0).max(1.0);
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        self.center_ts += ts_delta_ms;
        self.center_price += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        match point_type {
            ControlPointType::Point1 | ControlPointType::Move => {
                self.center_ts = ts_ms;
                self.center_price = price;
            }
            ControlPointType::Corner(_) => {
                // Corner resize - proportional
                let ts_diff = (ts_ms - self.center_ts).abs() * 2;
                self.bar_size_ms = ts_diff.max(60_000);
                self.price_size = ((price - self.center_price).abs() * 2.0).max(1.0);
            }
            ControlPointType::Edge(0) => {
                // Top edge - adjust price_size
                self.price_size = ((price - self.center_price).abs() * 2.0).max(1.0);
            }
            ControlPointType::Edge(1) => {
                // Right edge - adjust bar_size_ms
                let ts_diff = (ts_ms - self.center_ts).abs() * 2;
                self.bar_size_ms = ts_diff.max(60_000);
            }
            ControlPointType::Edge(2) => {
                // Bottom edge - adjust price_size
                self.price_size = ((self.center_price - price).abs() * 2.0).max(1.0);
            }
            ControlPointType::Edge(3) => {
                // Left edge - adjust bar_size_ms
                let ts_diff = (self.center_ts - ts_ms).abs() * 2;
                self.bar_size_ms = ts_diff.max(60_000);
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
        let center_b = timestamp_ms_to_bar_f64(bars, self.center_ts);
        let cx = viewport.bar_to_x_f64(center_b);
        let cy = viewport.price_to_y(self.center_price, price_scale.price_min, price_scale.price_max);

        let corners = self.corners();
        let screen_corners: Vec<(f64, f64)> = corners.iter()
            .map(|(ts, price)| {
                let b = timestamp_ms_to_bar_f64(bars, *ts);
                (
                    viewport.bar_to_x_f64(b),
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
            (cx, cy - half_price_screen, 0u8), // top
            (cx + half_bar_screen, cy, 1),      // right
            (cx, cy + half_price_screen, 2),    // bottom
            (cx - half_bar_screen, cy, 3),      // left
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

        // Every stroke `render` actually draws: the outer square, the
        // concentric inner ring squares (gated by `ring_level_visible` — the
        // same filter GannSquare's ring loop applies, so a ring hidden by
        // its own visibility config can't stay clickable), the two
        // diagonals, and the cardinal lines through center.
        let mut segments = Vec::with_capacity(4 + self.level_configs.len() * 4 + 4);
        for i in 0..4 {
            let (x1, y1) = screen_corners[i];
            let (x2, y2) = screen_corners[(i + 1) % 4];
            segments.push(Segment::new(x1, y1, x2, y2));
        }

        for config in &self.level_configs {
            if !ring_level_visible(config) {
                continue;
            }
            let half_w = half_bar_screen * config.level;
            let half_h = half_price_screen * config.level;
            segments.push(Segment::new(cx - half_w, cy - half_h, cx + half_w, cy - half_h)); // top
            segments.push(Segment::new(cx + half_w, cy - half_h, cx + half_w, cy + half_h)); // right
            segments.push(Segment::new(cx - half_w, cy + half_h, cx + half_w, cy + half_h)); // bottom
            segments.push(Segment::new(cx - half_w, cy - half_h, cx - half_w, cy + half_h)); // left
        }

        // Diagonals
        segments.push(Segment::new(screen_corners[0].0, screen_corners[0].1, screen_corners[2].0, screen_corners[2].1));
        segments.push(Segment::new(screen_corners[1].0, screen_corners[1].1, screen_corners[3].0, screen_corners[3].1));

        // Cardinal lines through center
        segments.push(Segment::new(cx - half_bar_screen, cy, cx + half_bar_screen, cy));
        segments.push(Segment::new(cx, cy - half_price_screen, cx, cy + half_price_screen));

        if hit_test_strokes(screen_x, screen_y, &segments, HIT_TOLERANCE) {
            return HitTestResult::Body;
        }

        HitTestResult::Miss
    }

    fn control_points(
        &self,
        bars: &[Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Vec<ControlPoint> {
        let center_b = timestamp_ms_to_bar_f64(bars, self.center_ts);
        let cx = viewport.bar_to_x_f64(center_b);
        let cy = viewport.price_to_y(self.center_price, price_scale.price_min, price_scale.price_max);

        let corners = self.corners();
        let screen_corners: Vec<(f64, f64)> = corners.iter()
            .map(|(ts, price)| {
                let b = timestamp_ms_to_bar_f64(bars, *ts);
                (
                    viewport.bar_to_x_f64(b),
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

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let cx = ctx.ts_to_x_ms(self.center_ts);
        let cy = ctx.price_to_y(self.center_price);

        let corners = self.corners();
        let screen_corners: Vec<(f64, f64)> = corners.iter()
            .map(|(ts, price)| (ctx.ts_to_x_ms(*ts), ctx.price_to_y(*price)))
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

        // Draw inner ring squares using level_configs (previously the fill
        // quads above were the only visible trace of a level — the ring
        // OUTLINE itself, and its label, never rendered).
        for config in &self.level_configs {
            // Same filter `hit_test` applies via `ring_level_visible` — keeps
            // the drawn and clickable rings in agreement.
            if !ring_level_visible(config) {
                continue;
            }
            let ratio = config.level;

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

            let half_w = half_w_full * ratio;
            let half_h = half_h_full * ratio;
            ctx.begin_path();
            ctx.rect(crisp(cx - half_w, dpr), crisp(cy - half_h, dpr), half_w * 2.0, half_h * 2.0);
            ctx.stroke();

            if self.show_labels {
                let pct = ratio * 100.0;
                let label = if (pct - pct.round()).abs() < 0.01 {
                    format!("{}%", pct.round() as i32)
                } else {
                    format!("{:.1}%", pct)
                };
                let label_color = self.label_style.color.as_deref().unwrap_or(stroke_color);
                ctx.set_font(&self.label_style.font_string());
                ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                ctx.set_text_align(crate::render::TextAlign::Center);
                ctx.set_fill_color(label_color);
                draw_label_plate(ctx, cx, cy - half_h, &label, self.label_style.font_size, &self.label_style, label_color);
            }
        }

        // Reset to default style before diagonals/cardinals (the ring loop
        // above may have left a per-level color/dash pattern active)
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

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
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => (min_x + max_x) / 2.0,
                    super::super::TextAlign::End => max_x,
                };
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => min_y - text_offset,
                    super::super::TextAlign::Center => (min_y + max_y) / 2.0,
                    super::super::TextAlign::End => max_y + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
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

    fn style_properties(&self) -> Vec<super::super::config::ConfigProperty> {
        use super::super::config::ConfigProperty;
        let mut props = vec![
            ConfigProperty::show_labels(self.show_labels).with_order(10),
            ConfigProperty::label_font_size(self.label_style.font_size).with_order(40),
            ConfigProperty::label_color(self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke)).with_order(41),
        ];
        props.extend(self.label_style.style_properties(&self.data.color.stroke, 50));
        props
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
            "label_font_size" => {
                if let Some(v) = value.as_number() {
                    self.label_style.font_size = v;
                    return true;
                }
            }
            "label_color" => {
                if let Some(c) = value.as_color() {
                    self.label_style.color = Some(c.to_string());
                    return true;
                }
            }
            _ => {}
        }
        if self.label_style.apply_property(id, value) {
            return true;
        }
        false
    }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    let radius = 8.0;
    (sx - px).powi(2) + (sy - py).powi(2) <= radius * radius
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_gann_square_fixed(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts, price) = points.first().copied().unwrap_or((0, 100.0));
    let mut gann = GannSquareFixed::new(ts, price, color);
    if let Some(&(ts2, price2)) = points.get(1) {
        let ts_diff = (ts2 - ts).abs() * 2;
        gann.bar_size_ms = ts_diff.max(60_000);
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
    }
}
