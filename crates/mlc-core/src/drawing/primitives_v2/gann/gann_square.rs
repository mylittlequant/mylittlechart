//! Gann Square primitive
//!
//! A resizable Gann square defined by two points.
//! Shows the classic Gann square with angle divisions and cardinal/ordinal lines.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{ConfigKey, StyleKey};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    HIT_TOLERANCE,
    RenderContext, crisp,
    render_primitive_text, draw_label_plate, level_ramp_color, level_rank,
    config::{FibLevelConfig, LabelStyle, LevelSetPreset},
};
use super::super::types::{Segment, hit_test_strokes};
use super::gann_box::{gann_fraction_label, parse_gann_fraction_label};

/// All Gann Square ring levels (0.0 = center, 1.0 = outer edge)
/// These represent concentric squares from center outward
pub const ALL_GANN_SQUARE_LEVELS: &[f64] = &[
    0.25,   // Inner ring (1/4)
    0.333,  // 1/3 ring
    0.5,    // Middle ring (1/2)
    0.667,  // 2/3 ring
    0.75,   // Outer inner ring (3/4)
    1.0,    // Outer edge (always drawn)
];

/// Main levels visible by default
pub const MAIN_GANN_SQUARE_VISIBLE: &[f64] = &[0.333, 0.667, 1.0];

/// The Gann Square family's own three level-set presets, built strictly
/// from [`ALL_GANN_SQUARE_LEVELS`] — this catalogue is shared by both
/// [`super::GannSquare`] and [`super::GannSquareFixed`] (the fixed variant
/// imports it directly, see `gann_square_fixed.rs`), so both types' rings
/// carry identical named presets.
///
/// - `standard` = [`MAIN_GANN_SQUARE_VISIBLE`] — the exact three rings
///   [`default_gann_square_configs`] marks visible out of the box (1/3,
///   2/3, the outer edge), so a freshly drawn square already reads as
///   "standard".
/// - `key_only` = the middle ring (0.5) alone — the square's own defining
///   concentric division, the smallest meaningful ladder beyond the
///   outer edge [`ring_level_visible`] already excludes from the gated
///   ring loop (the outer square is drawn unconditionally, exactly like
///   `gann_box`'s own always-drawn outline — see that type's own
///   `LEVEL_SET_PRESETS` doc for the identical reasoning).
/// - `all` = every ring [`ALL_GANN_SQUARE_LEVELS`] declares (6). No
///   `extended` tier: the 3 rings beyond `standard` have no further named
///   grouping in this catalogue to carve an in-between tier out of.
pub const LEVEL_SET_PRESETS: &[LevelSetPreset] = &[
    LevelSetPreset { id: "standard", name: |lang| StyleKey::Standard.get(lang), values: MAIN_GANN_SQUARE_VISIBLE },
    LevelSetPreset { id: "key_only", name: |lang| ConfigKey::LevelSetKeyOnly.get(lang), values: &[0.5] },
    LevelSetPreset { id: "all", name: |lang| ConfigKey::LevelSetAll.get(lang), values: ALL_GANN_SQUARE_LEVELS },
];

/// Create default level configs for Gann Square
pub fn default_gann_square_configs() -> Vec<FibLevelConfig> {
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

/// Gann Square
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GannSquare {
    /// Common primitive data
    pub data: PrimitiveData,
    /// First corner timestamp (ms)
    #[serde(default)]
    pub ts1: i64,
    /// First corner price
    pub price1: f64,
    /// Second corner timestamp (ms)
    #[serde(default)]
    pub ts2: i64,
    /// Second corner price
    pub price2: f64,
    /// Ring level configurations (0.0-1.0 fraction from center to edge)
    #[serde(default = "default_gann_square_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    /// Show labels
    #[serde(default = "default_true")]
    pub show_labels: bool,
    /// Show cardinal lines (horizontal/vertical through center)
    #[serde(default = "default_true")]
    pub show_cardinal: bool,
    /// Show ordinal lines (diagonals)
    #[serde(default = "default_true")]
    pub show_ordinal: bool,
    /// Style for ring fraction labels (font, background plate, offset, border)
    #[serde(default)]
    pub label_style: LabelStyle,
}

impl GannSquare {
    /// Create a new Gann square
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "gann_square".to_string(),
                display_name: "Gann Square".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            ts1,
            price1,
            ts2,
            price2,
            level_configs: default_gann_square_configs(),
            show_labels: true,
            show_cardinal: true,
            show_ordinal: true,
            label_style: LabelStyle::default(),
        }
    }
}

impl Primitive for GannSquare {
    fn type_id(&self) -> &'static str {
        "gann_square"
    }

    fn display_name(&self) -> &str {
        &self.data.display_name
    }

    fn kind(&self) -> PrimitiveKind {
        PrimitiveKind::Gann
    }

    // `Ladder`-archetype (via the default `level_configs().is_some()`
    // check), but every Gann tool calls its ladder "Углы"/Angles, not the
    // archetype's own default "Уровни"/Levels.
    fn subject_tab_label(&self) -> &'static str {
        crate::i18n::t_concept(crate::i18n::ConceptKey::Angles)
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
            ControlPointType::Point1 => {
                self.ts1 = ts_ms;
                self.price1 = price;
            }
            ControlPointType::Point2 => {
                self.ts2 = ts_ms;
                self.price2 = price;
            }
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
        let cx = (x1 + x2) / 2.0;
        let cy = (y1 + y2) / 2.0;
        let half_w_full = (max_x - min_x) / 2.0;
        let half_h_full = (max_y - min_y) / 2.0;

        // Every stroke `render` actually draws: the outer square, the
        // concentric inner ring squares (gated by `ring_level_visible` — the
        // same filter `render`'s ring loop applies, so a ring hidden by its
        // own visibility config can't stay clickable), the cardinal lines
        // (gated by `show_cardinal`) and the ordinal diagonals (gated by
        // `show_ordinal`).
        let mut segments = vec![
            Segment::new(min_x, min_y, max_x, min_y), // top
            Segment::new(max_x, min_y, max_x, max_y), // right
            Segment::new(min_x, max_y, max_x, max_y), // bottom
            Segment::new(min_x, min_y, min_x, max_y), // left
        ];

        for config in &self.level_configs {
            if !ring_level_visible(config) {
                continue;
            }
            let half_w = half_w_full * config.level;
            let half_h = half_h_full * config.level;
            segments.push(Segment::new(cx - half_w, cy - half_h, cx + half_w, cy - half_h)); // top
            segments.push(Segment::new(cx + half_w, cy - half_h, cx + half_w, cy + half_h)); // right
            segments.push(Segment::new(cx - half_w, cy + half_h, cx + half_w, cy + half_h)); // bottom
            segments.push(Segment::new(cx - half_w, cy - half_h, cx - half_w, cy + half_h)); // left
        }

        if self.show_cardinal {
            segments.push(Segment::new(min_x, cy, max_x, cy));
            segments.push(Segment::new(cx, min_y, cx, max_y));
        }

        if self.show_ordinal {
            segments.push(Segment::new(min_x, min_y, max_x, max_y));
            segments.push(Segment::new(min_x, max_y, max_x, min_y));
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
            ControlPoint::point1(x1, y1),
            ControlPoint::point2(x2, y2),
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
        let cx = (x1 + x2) / 2.0;
        let cy = (y1 + y2) / 2.0;
        let half_w_full = (max_x - min_x) / 2.0;
        let half_h_full = (max_y - min_y) / 2.0;

        // === FILL RENDERING (before lines so lines are on top) ===
        // For GannSquare, fill is drawn as rings between concentric squares
        // Sort levels for proper ring fill (inner to outer)
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

            // Draw ring as outer rect minus inner rect (using even-odd fill)
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

        // Draw outer square (always)
        ctx.begin_path();
        ctx.rect(crisp(min_x, dpr), crisp(min_y, dpr), max_x - min_x, max_y - min_y);
        ctx.stroke();

        // Draw inner level squares using level_configs
        for config in &self.level_configs {
            // Same filter `hit_test` applies via `ring_level_visible` — keeps
            // the drawn and clickable rings in agreement. Skips the outer
            // edge (1.0), already drawn above.
            if !ring_level_visible(config) {
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

            let half_w = half_w_full * ratio;
            let half_h = half_h_full * ratio;
            ctx.begin_path();
            ctx.rect(crisp(cx - half_w, dpr), crisp(cy - half_h, dpr), half_w * 2.0, half_h * 2.0);
            ctx.stroke();

            // Ring fraction label on the ring's top edge — rings nest around a
            // shared center rather than converging on one point, so each
            // ring's own top edge is already a collision-free anchor.
            if self.show_labels {
                let label = gann_fraction_label(ratio);
                let label_color = self.label_style.color.as_deref().unwrap_or(stroke_color);
                ctx.set_font(&self.label_style.font_string());
                ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                ctx.set_text_align(crate::render::TextAlign::Center);
                ctx.set_fill_color(label_color);
                draw_label_plate(ctx, cx, cy - half_h, &label, self.label_style.font_size, &self.label_style, label_color);
            }
        }

        // Reset to default style
        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Draw cardinal lines if enabled
        if self.show_cardinal {
            // Horizontal center line
            ctx.begin_path();
            ctx.move_to(crisp(min_x, dpr), crisp(cy, dpr));
            ctx.line_to(crisp(max_x, dpr), crisp(cy, dpr));
            ctx.stroke();

            // Vertical center line
            ctx.begin_path();
            ctx.move_to(crisp(cx, dpr), crisp(min_y, dpr));
            ctx.line_to(crisp(cx, dpr), crisp(max_y, dpr));
            ctx.stroke();
        }

        // Draw ordinal lines if enabled
        if self.show_ordinal {
            // Main diagonal
            ctx.begin_path();
            ctx.move_to(crisp(min_x, dpr), crisp(min_y, dpr));
            ctx.line_to(crisp(max_x, dpr), crisp(max_y, dpr));
            ctx.stroke();

            // Anti-diagonal
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

    fn level_set_presets(&self) -> &'static [LevelSetPreset] {
        LEVEL_SET_PRESETS
    }

    fn level_value_label(&self, level: f64) -> String {
        gann_fraction_label(level)
    }

    fn parse_level_value(&self, text: &str) -> Option<f64> {
        parse_gann_fraction_label(text)
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    fn clone_box(&self) -> Box<dyn Primitive> {
        Box::new(self.clone())
    }

    /// `show_cardinal`/`show_ordinal` are real fields `render`/`hit_test`
    /// already read (the cardinal cross and the two ordinal diagonals, both
    /// above), with no `ConfigProperty` anywhere to reach them from the UI
    /// — a defect pass fix (2026-08-16). The approved ladder archetype's
    /// own design has no dedicated Gann-toggle slot, so `PropertyGroup::
    /// General` (`ladder_tabs()`'s own General group) is the closest honest
    /// home, mirroring `pitchfork.rs`'s own `extend`/`show_labels` placement.
    fn level_properties(&self) -> Vec<super::super::config::ConfigProperty> {
        use super::super::config::{ConfigProperty, PropertyCategory, PropertyGroup};
        vec![
            ConfigProperty::show_cardinal(self.show_cardinal)
                .with_category(PropertyCategory::LevelsBase).with_order(0).in_group(PropertyGroup::General),
            ConfigProperty::show_ordinal(self.show_ordinal)
                .with_category(PropertyCategory::LevelsBase).with_order(1).in_group(PropertyGroup::General),
        ]
    }

    fn apply_level_property(&mut self, id: &str, value: &super::super::config::PropertyValue) -> bool {
        match id {
            "show_cardinal" => { if let Some(v) = value.as_bool() { self.show_cardinal = v; return true; } }
            "show_ordinal" => { if let Some(v) = value.as_bool() { self.show_ordinal = v; return true; } }
            _ => {}
        }
        false
    }

    fn style_properties(&self) -> Vec<super::super::config::ConfigProperty> {
        use super::super::config::{ConfigProperty, PropertyCategory, PropertyGroup};
        // `show_labels` stays ungrouped (W2), matching `gann_fan`'s own
        // choice: this type declares nothing under `LevelsBase`, so it
        // renders in the Subject tab's default run. `PropertyGroup::
        // LevelLabels` (W2) — the approved ladder archetype's own
        // `LevelLabelsGroupTitle` — matches `gann_fan`'s grouping for the
        // font/colour pair and the label-style block below it.
        let mut props = vec![
            ConfigProperty::show_labels(self.show_labels).with_order(10),
            ConfigProperty::label_font_size(self.label_style.font_size).with_category(PropertyCategory::LabelsText).with_order(40).in_group(PropertyGroup::LevelLabels),
            ConfigProperty::label_color(self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke)).with_category(PropertyCategory::LabelsText).with_order(41).in_group(PropertyGroup::LevelLabels),
        ];
        props.extend(self.label_style.style_properties_grouped(&self.data.color.stroke, 50, PropertyGroup::LevelLabels));
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
        if self.label_style.apply_property(id, value, &self.data.color.stroke) {
            return true;
        }
        false
    }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    let radius = 8.0;
    (sx - px).powi(2) + (sy - py).powi(2) <= radius * radius
}

/// Whether a ring level should be drawn/hit-tested — shared by `render` and
/// `hit_test` (and by `gann_square_fixed`, which uses the identical
/// concentric-ring filter) so a ring hidden by its own visibility config can
/// never remain clickable. The outer edge (1.0) is already drawn as the
/// square outline.
pub(super) fn ring_level_visible(config: &FibLevelConfig) -> bool {
    config.visible && config.level <= 0.99
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_gann_square(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
    let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, price1 - 20.0));
    Box::new(GannSquare::new(ts1, price1, ts2, price2, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "gann_square",
        display_name: "Gann Square",
        kind: PrimitiveKind::Gann,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Resizable Gann square with divisions",
        icon: "gann_square",
        default_color: "#FF9800",
        factory: create_gann_square,
        supports_text: true,
        has_levels: true,
    }
}

#[cfg(test)]
mod declared_toggles_tests {
    use super::*;

    /// `show_cardinal`/`show_ordinal` are real fields `render` already
    /// reads — this asserts they are reachable from the UI at all, not just
    /// that the field exists.
    #[test]
    fn property_list_contains_show_cardinal_and_show_ordinal() {
        let gann = GannSquare::new(0, 100.0, 3_600_000, 80.0, "#FF9800");
        let ids: Vec<String> = gann.level_properties().into_iter().map(|p| p.id).collect();
        assert!(ids.iter().any(|id| id == "show_cardinal"), "level_properties() must declare show_cardinal: {ids:?}");
        assert!(ids.iter().any(|id| id == "show_ordinal"), "level_properties() must declare show_ordinal: {ids:?}");
    }
}
