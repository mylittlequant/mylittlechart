//! Gann Fan primitive
//!
//! Fan lines radiating from a single point at Gann angles.
//! Standard angles: 1x8, 1x4, 1x3, 1x2, 1x1, 2x1, 3x1, 4x1, 8x1

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{ConfigKey, StyleKey};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle,
    HIT_TOLERANCE, point_to_line_distance,
    RenderContext, crisp,
    render_primitive_text, draw_level_label_plate, level_ramp_color, level_rank, stagger_t,
    config::{FibLevelConfig, LabelStyle, LevelSetPreset},
};
use super::super::types::point_to_ray_distance;

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

/// Gann Fan's own three level-set presets, built strictly from
/// [`ALL_GANN_ANGLES`] — this type's own angle catalogue, never a foreign
/// family's ratio list (see `Primitive::level_set_presets`'s own doc on the
/// defect narrowing toward one would reintroduce).
///
/// - `standard` = [`MAIN_GANN_VISIBLE`] — the exact three angles
///   [`default_gann_fan_configs`] marks visible out of the box (2x1, 1x1,
///   1x2), so a freshly drawn fan already reads as "standard".
/// - `key_only` = the 1x1 angle alone. Every other angle in this catalogue
///   is expressed AS a ratio against 1x1 (the 45° line) — it is the one
///   angle a Gann fan cannot be drawn without still reading as "a Gann
///   fan", the smallest meaningful ladder this tool has.
/// - `all` = every angle [`ALL_GANN_ANGLES`] declares (9). No `extended`
///   tier: `ALL_GANN_ANGLES` already IS the traditional full Gann angle set
///   (1x8 through 8x1) — there is no further named tier between
///   `standard`'s 3 and `all`'s 9 to carve out of this catalogue, so a
///   fourth preset here would be invented, not read off it.
pub const LEVEL_SET_PRESETS: &[LevelSetPreset] = &[
    LevelSetPreset { id: "standard", name: |lang| StyleKey::Standard.get(lang), values: MAIN_GANN_VISIBLE },
    LevelSetPreset { id: "key_only", name: |lang| ConfigKey::LevelSetKeyOnly.get(lang), values: &[1.0] },
    LevelSetPreset { id: "all", name: |lang| ConfigKey::LevelSetAll.get(lang), values: ALL_GANN_ANGLES },
];

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

/// Parse a Gann angle back into its slope ratio — the inverse of
/// [`gann_angle_label`]. Accepts both the `"NxM"` notation that function
/// produces (`"2x1"` -> `2.0`, `"1x4"` -> `0.25`, case-insensitive on the
/// separator) AND a bare number typed directly (`"2"` -> `2.0`), since a
/// user editing the settings-modal value field can always retype it from
/// scratch ignoring the notation shown.
pub fn parse_gann_angle_label(text: &str) -> Option<f64> {
    let t = text.trim();
    if let Some((a, b)) = t.split_once(['x', 'X']) {
        let a: f64 = a.trim().parse().ok()?;
        let b: f64 = b.trim().parse().ok()?;
        if b == 0.0 {
            return None;
        }
        return Some(a / b);
    }
    t.parse::<f64>().ok()
}

/// Create default level configs for Gann Fan
pub fn default_gann_fan_configs() -> Vec<FibLevelConfig> {
    let mut sorted: Vec<f64> = ALL_GANN_ANGLES.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ALL_GANN_ANGLES.iter().map(|&ratio| {
        let mut config = FibLevelConfig::new(ratio);
        config.visible = MAIN_GANN_VISIBLE.contains(&ratio);
        // Ramp colour by rank instead of every angle falling back to the same stroke color.
        config.color = Some(level_ramp_color(level_rank(ratio, &sorted), sorted.len()));
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
    /// Origin timestamp (ms)
    #[serde(default)]
    pub ts1: i64,
    /// Origin price
    pub price1: f64,
    /// Target timestamp (ms) — defines the time scale of the fan
    #[serde(default)]
    pub ts2: i64,
    /// Target price — defines the price scale of the fan
    pub price2: f64,
    /// Gann angle configurations (ratio values: 8.0=8x1, 1.0=1x1, 0.5=1x2, etc.)
    #[serde(default = "default_gann_fan_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    /// Show labels
    #[serde(default = "default_true")]
    pub show_labels: bool,
    #[serde(default)]
    pub extend_left: bool,
    #[serde(default = "default_true")]
    pub extend_right: bool,
    /// Direction: true = upward fan, false = downward fan
    #[serde(default = "default_true")]
    pub upward: bool,
    /// Style for angle labels (font, background plate, offset, border)
    #[serde(default)]
    pub label_style: LabelStyle,
}

impl GannFan {
    /// Create a new Gann fan
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "gann_fan".to_string(),
                display_name: "Gann Fan".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            ts1,
            price1,
            ts2,
            price2,
            level_configs: default_gann_fan_configs(),
            show_labels: true,
            extend_left: false,
            extend_right: true,
            upward: true,
            label_style: LabelStyle::default(),
        }
    }

    /// Get the price scale (price per ms) based on the two points
    pub fn price_per_ms(&self) -> f64 {
        let ts_diff = (self.ts2 - self.ts1).abs();
        let price_diff = (self.price2 - self.price1).abs();
        if ts_diff == 0 { 1.0 } else { price_diff / ts_diff as f64 }
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

        let ppms = self.price_per_ms();
        let direction = if self.upward { 1.0 } else { -1.0 };
        // Fixed time delta for computing fan ray direction: use ts2 - ts1
        let ts_delta = (self.ts2 - self.ts1).abs();
        let ts_delta = if ts_delta == 0 { 1_200_000 } else { ts_delta };

        // Check each visible fan line
        for config in &self.level_configs {
            if !config.visible {
                continue;
            }
            let ratio = config.level;

            let end_ts = self.ts1 + ts_delta;
            let end_price = self.price1 + ts_delta as f64 * ppms * ratio * direction;

            let end_b = timestamp_ms_to_bar_f64(bars, end_ts);
            let end_x = viewport.bar_to_x_f64(end_b);
            let end_y = viewport.price_to_y(end_price, price_scale.price_min, price_scale.price_max);

            let dist = if (self.extend_left || self.extend_right) {
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

        let ppms = self.price_per_ms();
        let direction = if self.upward { 1.0 } else { -1.0 };
        // Fixed time delta for ray direction
        let ts_delta = (self.ts2 - self.ts1).abs();
        let ts_delta = if ts_delta == 0 { 1_200_000 } else { ts_delta };

        // === FILL RENDERING (before lines so lines are on top) ===
        let visible = self.visible_levels_sorted();

        for i in 0..visible.len() {
            let config = visible[i];
            if !config.fill_enabled {
                continue;
            }

            let ratio1 = config.level;
            let ratio2 = if i + 1 < visible.len() {
                visible[i + 1].level
            } else {
                0.0
            };

            let fill_color = config.fill_color.as_deref()
                .or(config.color.as_deref())
                .unwrap_or(&self.data.color.stroke);

            let ext = if (self.extend_left || self.extend_right) {
                (chart_width + chart_height) * 2.0
            } else {
                ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt()
            };

            // Angle 1 (steeper)
            let end_ts1 = self.ts1 + ts_delta;
            let end_price1 = self.price1 + ts_delta as f64 * ppms * ratio1 * direction;
            let end_x1_raw = ctx.ts_to_x_ms(end_ts1);
            let end_y1_raw = ctx.price_to_y(end_price1);
            let dx1 = end_x1_raw - x1;
            let dy1 = end_y1_raw - y1;
            let len1 = (dx1 * dx1 + dy1 * dy1).sqrt();
            let (end_x1, end_y1) = if len1 > 0.0 && (self.extend_left || self.extend_right) {
                (x1 + dx1 / len1 * ext, y1 + dy1 / len1 * ext)
            } else {
                (end_x1_raw, end_y1_raw)
            };

            // Angle 2 (shallower)
            let end_ts2 = self.ts1 + ts_delta;
            let end_price2 = self.price1 + ts_delta as f64 * ppms * ratio2 * direction;
            let end_x2_raw = ctx.ts_to_x_ms(end_ts2);
            let end_y2_raw = ctx.price_to_y(end_price2);
            let dx2 = end_x2_raw - x1;
            let dy2 = end_y2_raw - y1;
            let len2 = (dx2 * dx2 + dy2 * dy2).sqrt();
            let (end_x2, end_y2) = if len2 > 0.0 && (self.extend_left || self.extend_right) {
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

        // Rank each visible angle (ascending) so labels stagger along their
        // own ray instead of colliding at the shared origin (x1, y1) — every
        // angle ray starts from the same point.
        let mut sorted_visible: Vec<f64> = self.level_configs.iter().filter(|c| c.visible).map(|c| c.level).collect();
        sorted_visible.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let visible_count = sorted_visible.len();

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

            let end_ts = self.ts1 + ts_delta;
            let end_price = self.price1 + ts_delta as f64 * ppms * ratio * direction;

            let end_x = ctx.ts_to_x_ms(end_ts);
            let end_y = ctx.price_to_y(end_price);

            ctx.begin_path();
            ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));

            if (self.extend_left || self.extend_right) {
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

            // Angle ratio label (e.g. "2x1", "1x4"), staggered along the ray
            // so closely angled rays don't stack their labels on the origin.
            if self.show_labels {
                let dx = end_x - x1;
                let dy = end_y - y1;
                if dx != 0.0 || dy != 0.0 {
                    let rank = level_rank(ratio, &sorted_visible);
                    let label_t = stagger_t(0.3, rank, visible_count, 0.5);
                    let label_x = x1 + dx * label_t;
                    let label_y = y1 + dy * label_t;
                    let label = gann_angle_label(ratio);
                    let label_color = self.label_style.color.as_deref().unwrap_or(stroke_color);
                    ctx.set_font(&self.label_style.font_string());
                    ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                    ctx.set_text_align(crate::render::TextAlign::Center);
                    ctx.set_fill_color(label_color);
                    draw_level_label_plate(ctx, label_x, label_y, &label, self.label_style.font_size, &self.label_style, label_color, ratio);
                }
            }
        }
        ctx.set_line_dash(&[]);

        // Render text if present
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let ppms = self.price_per_ms();
                let ts_half = ts_delta / 2;
                let top_price = self.price1 + ts_half as f64 * ppms * 8.0 * direction;
                let bottom_price = self.price1 + ts_half as f64 * ppms * 0.125 * direction;
                let end_ts_half = self.ts1 + ts_half;

                let end_x = ctx.ts_to_x_ms(end_ts_half);
                let top_y = ctx.price_to_y(top_price);
                let bottom_y = ctx.price_to_y(bottom_price);
                let mid_price = self.price1 + ts_half as f64 * ppms * 1.0 * direction;
                let mid_y = ctx.price_to_y(mid_price);

                let min_x = x1.min(end_x);
                let max_x = x1.max(end_x);
                let min_y = top_y.min(bottom_y);
                let max_y = top_y.max(bottom_y);

                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    super::super::TextAlign::Start => min_x,
                    super::super::TextAlign::Center => (min_x + max_x) / 2.0,
                    super::super::TextAlign::End => max_x,
                };
                let text_y = match text.v_align {
                    super::super::TextAlign::Start => min_y - text_offset,
                    super::super::TextAlign::Center => mid_y,
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
        gann_angle_label(level)
    }

    fn parse_level_value(&self, text: &str) -> Option<f64> {
        parse_gann_angle_label(text)
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    fn clone_box(&self) -> Box<dyn Primitive> {
        Box::new(self.clone())
    }
    fn extend_mode_raw(&self) -> u8 {
        super::super::extend_mode_lr(self.extend_left, self.extend_right)
    }
    fn unbounded_in_price(&self) -> bool { self.extend_left || self.extend_right }

    /// `extend`/`upward` are real fields `render`/`hit_test` already read
    /// (whether the fan rays reach the panel edge, and which direction the
    /// fan points), with no `ConfigProperty` anywhere to reach them from
    /// the UI — a defect pass fix (2026-08-16). The approved ladder
    /// archetype's own design has no dedicated Gann-toggle slot, so
    /// `PropertyGroup::General` (`ladder_tabs()`'s own General group) is
    /// the closest honest home, mirroring `pitchfork.rs`'s own `extend`/
    /// `show_labels` placement — `extend` reuses the SAME `"extend"` id
    /// pitchfork and its three siblings use for their own equivalent.
    fn level_properties(&self) -> Vec<super::super::config::ConfigProperty> {
        use super::super::config::{ConfigProperty, PropertyCategory, PropertyGroup};
        vec![
            ConfigProperty::extend_left(self.extend_left)
                .with_category(PropertyCategory::LevelsBase).with_order(0).in_group(PropertyGroup::General),
            ConfigProperty::extend_right(self.extend_right)
                .with_category(PropertyCategory::LevelsBase).with_order(0).in_group(PropertyGroup::General),
            ConfigProperty::upward(self.upward)
                .with_category(PropertyCategory::LevelsBase).with_order(1).in_group(PropertyGroup::General),
        ]
    }

    fn apply_level_property(&mut self, id: &str, value: &super::super::config::PropertyValue) -> bool {
        match id {
            "extend_left" => { if let Some(v) = value.as_bool() { self.extend_left = v; return true; } }
            "extend_right" => { if let Some(v) = value.as_bool() { self.extend_right = v; return true; } }
            "upward" => { if let Some(v) = value.as_bool() { self.upward = v; return true; } }
            _ => {}
        }
        false
    }

    fn style_properties(&self) -> Vec<super::super::config::ConfigProperty> {
        use super::super::config::{ConfigProperty, PropertyCategory, PropertyGroup};
        // `show_labels` stays ungrouped (W2): it renders on the Subject tab
        // (`AppearanceLine`, no level-ladder base row declared for this
        // type), while the approved ladder archetype's own "General" group
        // lives on the SAME tab but is addressed via `PropertyCategory::
        // LevelsBase` — a category this type declares nothing into. Giving
        // it a group here would tag it with a header that lands in the
        // wrong section (see `mlc-app-layout`'s `primitive_settings_vid`
        // doc on why `AppearanceLine` and `LevelsBase` are two different
        // render locations even on the same tab).
        let mut props = vec![
            ConfigProperty::show_labels(self.show_labels).with_order(10),
            // `PropertyGroup::LevelLabels` (W2) — the approved design's own
            // `LevelLabelsGroupTitle` (ladder archetype's Labels tab).
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

// =============================================================================
// Factory Registration
// =============================================================================

fn create_gann_fan(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
    let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, price1 + 20.0));
    Box::new(GannFan::new(ts1, price1, ts2, price2, color))
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
    }
}

#[cfg(test)]
mod declared_toggles_tests {
    use super::*;

    /// `extend`/`upward` are real fields `render`/`hit_test` already read
    /// — this asserts they are reachable from the UI at all, not just that
    /// the field exists.
    #[test]
    fn property_list_contains_extend_and_upward() {
        let gann = GannFan::new(0, 100.0, 3_600_000, 120.0, "#FF9800");
        let ids: Vec<String> = gann.level_properties().into_iter().map(|p| p.id).collect();
        assert!(ids.iter().any(|id| id == "extend_left"), "level_properties() must declare extend_left: {ids:?}");
        assert!(ids.iter().any(|id| id == "extend_right"), "level_properties() must declare extend_right: {ids:?}");
        assert!(ids.iter().any(|id| id == "upward"), "level_properties() must declare upward: {ids:?}");
    }
}
