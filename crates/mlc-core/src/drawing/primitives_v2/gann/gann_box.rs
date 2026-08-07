//! Gann Box primitive
//!
//! A rectangular box divided by Gann angles.
//! Shows price/time relationships with diagonal lines.

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
    let mut sorted: Vec<f64> = ALL_GANN_BOX_LEVELS.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ALL_GANN_BOX_LEVELS.iter().map(|&level| {
        let mut config = FibLevelConfig::new(level);
        config.visible = MAIN_GANN_BOX_VISIBLE.contains(&level);
        // Ramp colour by rank instead of every grid line falling back to the same stroke color.
        config.color = Some(level_ramp_color(level_rank(level, &sorted), sorted.len()));
        // Band fill on by default — `render` draws it as horizontal bands
        // between adjacent grid levels, a real closed region.
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

/// Gann Box
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GannBox {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Top-left corner timestamp (ms)
    #[serde(default)]
    pub ts1: i64,
    /// Top-left corner price
    pub price1: f64,
    /// Bottom-right corner timestamp (ms)
    #[serde(default)]
    pub ts2: i64,
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
    /// Style for grid fraction labels (font, background plate, offset, border)
    #[serde(default)]
    pub label_style: LabelStyle,
}

impl GannBox {
    /// Create a new Gann box
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "gann_box".to_string(),
                display_name: "Gann Box".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            ts1,
            price1,
            ts2,
            price2,
            level_configs: default_gann_box_configs(),
            show_labels: true,
            show_grid: true,
            show_diagonals: true,
            label_style: LabelStyle::default(),
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

    fn points(&self) -> Vec<(i64, f64)> {
        vec![(self.ts1, self.price1), (self.ts2, self.price2)]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if let Some(&(ts, price)) = points.first() {
            self.ts1 = ts;
            self.price1 = price;
        }
        if let Some(&(ts, price)) = points.get(1) {
            self.ts2 = ts;
            self.price2 = price;
        }
    }

    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64) {
        self.ts1 += ts_delta_ms;
        self.ts2 += ts_delta_ms;
        self.price1 += price_delta;
        self.price2 += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64) {
        match point_type {
            // Corner index convention matches `shapes::rectangle::Rectangle`
            // exactly: 0=(ts1,price1), 1=(ts2,price1), 2=(ts2,price2),
            // 3=(ts1,price2). Every corner writes BOTH axes.
            ControlPointType::Corner(0) => {
                self.ts1 = ts_ms;
                self.price1 = price;
            }
            ControlPointType::Corner(1) => {
                self.ts2 = ts_ms;
                self.price1 = price;
            }
            ControlPointType::Corner(2) => {
                self.ts2 = ts_ms;
                self.price2 = price;
            }
            ControlPointType::Corner(3) => {
                self.ts1 = ts_ms;
                self.price2 = price;
            }
            // Edge midpoints: 0=top(price1), 1=right(ts2), 2=bottom(price2),
            // 3=left(ts1) — same convention as Rectangle.
            ControlPointType::Edge(0) => { self.price1 = price; }
            ControlPointType::Edge(1) => { self.ts2 = ts_ms; }
            ControlPointType::Edge(2) => { self.price2 = price; }
            ControlPointType::Edge(3) => { self.ts1 = ts_ms; }
            ControlPointType::Move => {
                let ts_delta = ts_ms - self.ts1;
                let price_delta = price - self.price1;
                self.translate(ts_delta, price_delta);
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
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(b1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(b2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        // Corners tested before edges before the body stroke, so a click at
        // a shared endpoint always resolves to the corner, never the edge
        // midpoint that sits at the middle of a side.
        let corners = [
            (x1, y1, ControlPointType::Corner(0)),
            (x2, y1, ControlPointType::Corner(1)),
            (x2, y2, ControlPointType::Corner(2)),
            (x1, y2, ControlPointType::Corner(3)),
        ];
        for (cx, cy, cp_type) in corners {
            if check_point_hit(screen_x, screen_y, cx, cy) {
                return HitTestResult::ControlPoint(cp_type);
            }
        }

        let edges = [
            ((x1 + x2) / 2.0, y1, ControlPointType::Edge(0)),
            (x2, (y1 + y2) / 2.0, ControlPointType::Edge(1)),
            ((x1 + x2) / 2.0, y2, ControlPointType::Edge(2)),
            (x1, (y1 + y2) / 2.0, ControlPointType::Edge(3)),
        ];
        for (ex, ey, cp_type) in edges {
            if check_point_hit(screen_x, screen_y, ex, ey) {
                return HitTestResult::ControlPoint(cp_type);
            }
        }

        let min_x = x1.min(x2);
        let max_x = x1.max(x2);
        let min_y = y1.min(y2);
        let max_y = y1.max(y2);
        let width = max_x - min_x;
        let height = max_y - min_y;

        // Every stroke `render` actually draws: the 4 box edges, the inner
        // grid lines (gated by `show_grid` + `grid_level_visible` — the
        // exact filter `render`'s grid loop applies, so a level hidden by
        // its own visibility config can't stay clickable), and the two
        // diagonals (gated by `show_diagonals`, which the old check here
        // ignored).
        let mut segments = vec![
            Segment::new(min_x, min_y, max_x, min_y), // top
            Segment::new(max_x, min_y, max_x, max_y), // right
            Segment::new(min_x, max_y, max_x, max_y), // bottom
            Segment::new(min_x, min_y, min_x, max_y), // left
        ];

        if self.show_grid {
            for config in &self.level_configs {
                if !grid_level_visible(config) {
                    continue;
                }
                let level = config.level;
                let gx = min_x + width * level;
                segments.push(Segment::new(gx, min_y, gx, max_y));
                let gy = min_y + height * level;
                segments.push(Segment::new(min_x, gy, max_x, gy));
            }
        }

        if self.show_diagonals {
            segments.push(Segment::new(x1, y1, x2, y2));
            segments.push(Segment::new(x1, y2, x2, y1));
        }

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
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(b1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(b2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        vec![
            ControlPoint::new(ControlPointType::Corner(0), x1, y1, ControlPointCursor::ResizeNWSE),
            ControlPoint::new(ControlPointType::Corner(1), x2, y1, ControlPointCursor::ResizeNESW),
            ControlPoint::new(ControlPointType::Corner(2), x2, y2, ControlPointCursor::ResizeNWSE),
            ControlPoint::new(ControlPointType::Corner(3), x1, y2, ControlPointCursor::ResizeNESW),
            ControlPoint::new(ControlPointType::Edge(0), (x1 + x2) / 2.0, y1, ControlPointCursor::ResizeNS),
            ControlPoint::new(ControlPointType::Edge(1), x2, (y1 + y2) / 2.0, ControlPointCursor::ResizeEW),
            ControlPoint::new(ControlPointType::Edge(2), (x1 + x2) / 2.0, y2, ControlPointCursor::ResizeNS),
            ControlPoint::new(ControlPointType::Edge(3), x1, (y1 + y2) / 2.0, ControlPointCursor::ResizeEW),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
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
                // Same filter `hit_test` applies via `grid_level_visible` —
                // keeps the drawn and clickable grid lines in agreement.
                if !grid_level_visible(config) {
                    continue;
                }
                let level = config.level;

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

                // Fraction label at the vertical/horizontal line intersection
                if self.show_labels {
                    let pct = level * 100.0;
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
                    let label_y = min_y - self.label_style.font_size / 2.0 - self.label_style.background_padding - 2.0;
                    draw_label_plate(ctx, gx, label_y, &label, self.label_style.font_size, &self.label_style, label_color);
                }
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

/// Whether a grid level should be drawn/hit-tested — shared by `render` and
/// `hit_test` so a level hidden by its own visibility config can never
/// remain clickable. Levels at 0.0/1.0 are the box edge itself, already
/// covered by the outline.
fn grid_level_visible(config: &FibLevelConfig) -> bool {
    config.visible && (0.01..=0.99).contains(&config.level)
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_gann_box(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
    let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, price1 - 10.0));
    Box::new(GannBox::new(ts1, price1, ts2, price2, color))
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
    }
}
