//! Fibonacci Retracement primitive
//!
//! Shows horizontal levels at Fibonacci ratios between two price points.
//! Standard levels: 0%, 23.6%, 38.2%, 50%, 61.8%, 78.6%, 100%

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{ConfigKey, SettingsKey, current_language};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle, TextAlign,
    HIT_TOLERANCE, point_to_line_distance, control_point_style,
    RenderContext, crisp,
    render_primitive_text, draw_label_plate,
    config::{FibLevelConfig, LabelStyle},
    BAND_FILL_OPACITY, default_band_fill_opacity,
};

/// Main/active Fibonacci levels (visible by default)
pub const MAIN_LEVELS: &[f64] = &[0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0];

/// All 30 Fibonacci levels (main + extensions + uncommon)
pub const ALL_LEVELS: &[f64] = &[
    // Basic retracements
    0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0,
    // Extensions
    1.272, 1.414, 1.618, 2.0, 2.272, 2.414, 2.618, 3.0, 3.272, 3.414, 3.618, 4.0, 4.236, 4.618,
    // Additional uncommon levels
    0.146, 0.292, 0.707, 0.886, 1.13, 1.886, 2.886, 5.0, 6.854
];

/// Standard Fibonacci retracement levels (backward compatibility)
pub const DEFAULT_LEVELS: &[f64] = MAIN_LEVELS;

/// Extended Fibonacci levels (including extensions)
pub const EXTENDED_LEVELS: &[f64] = &[0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0, 1.272, 1.618, 2.0, 2.618];

/// Fibonacci levels singled out as "key" levels for the Levels tab's
/// "Только ключевые" system preset — both retracement endpoints plus the
/// golden zone. **Ambiguous in the approved spec** (`fib_modal.html`'s
/// answer #1 names "Только ключевые" but never lists its values) — needs
/// the owner's sign-off, flagged again here as it was where this constant
/// used to live (`mlc-app-layout::layout::modals::primitive_settings_intent`,
/// carried over unchanged from the ui-lab prototype's own
/// `level_presets::KEY_ONLY_LEVELS`). Moved here 2026-08-09 (primitive-
/// settings "Save as…" reference fix) so both `mlc-backend`
/// (creation-time `StyleReference::System` stamping) and `mlc-app-layout`
/// (the Levels tab UI) read the exact same list — see
/// [`level_set_values`]/[`level_set_id_for_composition`].
pub const KEY_ONLY_LEVELS: &[f64] = &[0.0, 0.382, 0.5, 0.618, 1.0];

/// Floating-point tolerance for comparing a level value against a system
/// level-set preset's own canonical value list.
const LEVEL_SET_MATCH_EPS: f64 = 1e-9;

/// The four system level-set preset ids
/// [`crate::drawing::StyleReference::System`] can name, in display order —
/// also the four ids
/// [`crate::templates::PrimitiveTemplate::system_templates`] mints for any
/// level-having primitive type. Never `"custom"` — see that variant's own
/// doc for why "custom" names no fixed definition of its own.
pub const LEVEL_SET_IDS: [&str; 4] = ["standard", "extended", "key_only", "all"];

/// The canonical value list a system level-set preset id stands for, or
/// `None` for any id that does not name one of the four real presets
/// (including `"custom"`, or a stale/unknown id).
pub fn level_set_values(id: &str) -> Option<&'static [f64]> {
    match id {
        "standard" => Some(MAIN_LEVELS),
        "extended" => Some(EXTENDED_LEVELS),
        "key_only" => Some(KEY_ONLY_LEVELS),
        "all" => Some(ALL_LEVELS),
        _ => None,
    }
}

/// Which system level-set preset id (if any) a raw composition of level
/// values exactly matches: same value set, order-independent, no partial
/// credit. `None` means the composition matches none of the four — a
/// "custom" composition, which has no fixed definition of its own to name
/// it with.
///
/// The single canonical "which preset does this composition match" answer
/// — both `mlc-backend` (stamping a freshly-created primitive's
/// `StyleReference::System` from its seeded composition) and
/// `mlc-app-layout` (the Levels tab's own `current_level_set` display,
/// keyed off [`level_set_values`] the same way) read off this, so the two
/// can never independently redefine "what a preset means" and drift apart.
pub fn level_set_id_for_composition(levels: &[f64]) -> Option<&'static str> {
    LEVEL_SET_IDS.into_iter().find(|id| {
        let target = level_set_values(id).unwrap_or(&[]);
        levels.len() == target.len()
            && target.iter().all(|v| levels.iter().any(|c| (*c - *v).abs() < LEVEL_SET_MATCH_EPS))
    })
}

/// Create default level configurations — the approved Levels-tab ladder
/// design (`nemo/docs/mlc/...` primitive-settings port, 2026-08-08) treats
/// the row list as "what is really enabled, not a catalogue": the ladder
/// shows exactly the entries `level_configs()` holds, with none of them
/// hidden-but-present. Seeding all 30 [`ALL_LEVELS`] with 23 disabled (the
/// previous behaviour) would render a 30-row ladder on every freshly placed
/// primitive instead of the intended list-of-7-you-add-to. [`ALL_LEVELS`]
/// and [`EXTENDED_LEVELS`] stay in place for the ladder's own "+ добавить
/// уровень" quick-add chips and `extended_level_configs()`; only the
/// DEFAULT seed changes. Existing saved drawings keep whatever they were
/// serialized with (30 rows for anything created before this change) — no
/// migration.
pub fn default_level_configs() -> Vec<FibLevelConfig> {
    // No per-level colour: every level draws in the primitive's own stroke.
    // The rank ramp used to be seeded here, which made the BASE look a
    // rainbow ladder and — worse — made it a half-rainbow the moment a
    // preset added values the 7-entry seed never had, since those got their
    // colours from a ramp run over a different length. Per-level hues are
    // what the Levels tab's Палитра MODE applies on demand
    // (`apply_palette_lines`/`apply_palette_fill`); a base look ships
    // without one (owner direction, 2026-08-09).
    MAIN_LEVELS.iter().map(|&level| {
        let mut config = FibLevelConfig::new(level);
        config.visible = true;
        config
    }).collect()
}

/// Create extended level configurations (with extensions)
pub fn extended_level_configs() -> Vec<FibLevelConfig> {
    EXTENDED_LEVELS.iter().map(|&level| FibLevelConfig::new(level)).collect()
}

/// Same 30-level ramp as [`default_level_configs`], with per-level band fill
/// turned on. Shared by every family whose levels bound a real closed region
/// — a stack of parallel/vertical bands or a concentric ring — so the fill
/// reads as a designed look out of the box rather than mud: this primitive's
/// own horizontal bands, `channel` (parallel-ish band), `trend_extension`
/// (horizontal band), `wedge` (bounded trapezoid between converging edges —
/// not the separate whole-shape `fill` toggle), `arcs`/`circles` (annulus /
/// ring via the nonzero-winding two-ellipse trick), `trend_time` (vertical
/// band). Deliberately NOT used by `fan`/`speed_resistance` (their fill is
/// an unbounded angular wedge from a shared apex, extended 2x chart-width —
/// adjacent-ray sector areas can be wildly uneven depending on how bunched
/// the levels are) or `spiral` (its per-level fill path reconnects to the
/// spiral's own center on every band instead of tracing the gap between two
/// consecutive loops, so it does not draw a band at all) — those three keep
/// consuming the ramp-only [`default_level_configs`].
pub fn default_level_configs_filled() -> Vec<FibLevelConfig> {
    default_level_configs()
        .into_iter()
        .map(|mut cfg| {
            cfg.fill_enabled = true;
            cfg
        })
        .collect()
}

/// Create level configurations with fills between zones
/// Different colors for different zones
pub fn filled_level_configs() -> Vec<FibLevelConfig> {
    vec![
        FibLevelConfig::with_fill(0.0, Some("#787b86".to_string()), 0.08),
        FibLevelConfig::with_fill(0.236, Some("#f7525f".to_string()), 0.08),
        FibLevelConfig::with_fill(0.382, Some("#22ab94".to_string()), 0.08),
        FibLevelConfig::with_fill(0.5, Some("#2962ff".to_string()), 0.08),
        FibLevelConfig::with_fill(0.618, Some("#ff9800".to_string()), 0.08),
        FibLevelConfig::with_fill(0.786, Some("#9c27b0".to_string()), 0.08),
        FibLevelConfig::new(1.0), // No fill for last level
    ]
}

/// Fibonacci Retracement - horizontal levels at Fib ratios
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibRetracement {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Start timestamp in ms (point 1)
    #[serde(default)]
    pub ts1: i64,
    /// Start price (point 1 - usually swing high or low)
    pub price1: f64,
    /// End timestamp in ms (point 2)
    #[serde(default)]
    pub ts2: i64,
    /// End price (point 2 - usually swing low or high)
    pub price2: f64,
    /// Fibonacci level configurations (with individual colors/widths)
    #[serde(default = "default_level_configs_filled", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    /// Show price labels
    #[serde(default = "default_true")]
    pub show_prices: bool,
    /// Show percentage labels
    #[serde(default = "default_true")]
    pub show_percentages: bool,
    /// Extend lines to left
    #[serde(default)]
    pub extend_left: bool,
    /// Extend lines to right
    #[serde(default = "default_true")]
    pub extend_right: bool,
    /// Master fill toggle — gates every level's own `fill_enabled` at once.
    /// Defaults true (via `default_true`, not `bool::default()`) so a saved
    /// primitive from before this field was wired — where the per-level
    /// `fill_enabled` toggle was the only thing that mattered — keeps
    /// rendering its fills exactly as before, rather than a `false` default
    /// silently turning every one of them off.
    #[serde(default = "default_true")]
    pub show_fill: bool,
    /// Fill opacity (0.0 to 1.0)
    #[serde(default = "default_band_fill_opacity")]
    pub fill_opacity: f64,
    /// Label position: "left", "right", "center"
    #[serde(default = "default_label_position")]
    pub label_position: String,
    /// Show levels as percentages (true) or coefficients (false)
    #[serde(default = "default_true")]
    pub show_as_percent: bool,
    /// Show connecting trend line between point 1 and 2
    #[serde(default = "default_true")]
    pub show_trend_line: bool,
    /// Style for level labels (font, background plate, offset, border)
    #[serde(default)]
    pub label_style: LabelStyle,
}

fn default_true() -> bool { true }
fn default_label_position() -> String { "left".to_string() }

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

impl FibRetracement {
    /// Create a new Fibonacci retracement
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_retracement".to_string(),
                display_name: "Fib Retracement".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            ts1,
            price1,
            ts2,
            price2,
            level_configs: default_level_configs_filled(),
            show_prices: true,
            show_percentages: true,
            extend_left: false,
            extend_right: true,
            show_fill: true,
            fill_opacity: BAND_FILL_OPACITY,
            label_position: "left".to_string(),
            show_as_percent: true,
            show_trend_line: true,
            label_style: LabelStyle::default(),
        }
    }

    /// Get the price at a given Fibonacci level
    pub fn price_at_level(&self, level: f64) -> f64 {
        self.price1 + (self.price2 - self.price1) * level
    }

    /// Get all level prices (only visible levels)
    pub fn level_prices(&self) -> Vec<(f64, f64)> {
        self.level_configs
            .iter()
            .filter(|cfg| cfg.visible)
            .map(|cfg| (cfg.level, self.price_at_level(cfg.level)))
            .collect()
    }
}

impl Primitive for FibRetracement {
    fn type_id(&self) -> &'static str {
        "fib_retracement"
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

    fn translate(&mut self, ts_delta: i64, price_delta: f64) {
        self.ts1 += ts_delta;
        self.ts2 += ts_delta;
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
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        // Check control points. The handle radius is read ONCE per hit-test,
        // not once per point: `control_point_style()` takes a global
        // `RwLock::read()`, and hit_test runs on every click and every hover
        // move over the chart.
        let handle_r = control_point_style().hit_radius();
        if check_point_hit(screen_x, screen_y, x1, y1, handle_r) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        if check_point_hit(screen_x, screen_y, x2, y2, handle_r) {
            return HitTestResult::ControlPoint(ControlPointType::Point2);
        }

        // The trend line between the two anchors is a drawn stroke, so it is
        // a grab surface like any other edge. It was rendered and never
        // hit-tested — the diagonal was the one part of the tool a click
        // could not reach. `trend_extension` and `trend_time` already test
        // their connecting segments; this brings retracement in line.
        if self.show_trend_line
            && point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE
        {
            return HitTestResult::Body;
        }

        // Check each level line (only visible ones)
        let min_x = x1.min(x2);
        let max_x = x1.max(x2);

        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            let level_price = self.price_at_level(cfg.level);
            let level_y = viewport.price_to_y(level_price, price_scale.price_min, price_scale.price_max);

            // Check if within horizontal bounds (or extended)
            let in_bounds = if self.extend_left && self.extend_right {
                true
            } else if self.extend_left {
                screen_x <= max_x
            } else if self.extend_right {
                screen_x >= min_x
            } else {
                screen_x >= min_x && screen_x <= max_x
            };

            if in_bounds && (screen_y - level_y).abs() < HIT_TOLERANCE {
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
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let x1 = viewport.bar_to_x_f64(bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);

        vec![
            ControlPoint::point1(x1, y1),
            ControlPoint::point2(x2, y2),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let chart_width = ctx.chart_width();

        let left_x = if self.extend_left { 0.0 } else { x1.min(x2) };
        let right_x = if self.extend_right { chart_width } else { x1.max(x2) };

        // Collect visible levels sorted by level value for fill rendering
        let mut visible_levels: Vec<(usize, f64, f64)> = self.level_configs
            .iter()
            .enumerate()
            .filter(|(_, cfg)| cfg.visible)
            .map(|(idx, cfg)| {
                let y = ctx.price_to_y(self.price_at_level(cfg.level));
                (idx, cfg.level, y)
            })
            .collect();
        visible_levels.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Draw fills between adjacent visible levels (before lines so lines are on top).
        // Gated by `show_fill` — previously a struct field with no reader
        // anywhere, so the primitive always fell back to whatever each
        // level's own `fill_enabled`/`fill_opacity` said (only reachable via
        // the per-level editor on the Levels tab), with no single master
        // on/off + opacity control. `fill_opacity` now overrides each
        // level's own opacity uniformly when the master toggle is on.
        if self.show_fill {
            for i in 0..visible_levels.len().saturating_sub(1) {
                let (idx, _, y_top) = visible_levels[i];
                let (_, _, y_bottom) = visible_levels[i + 1];

                let cfg = &self.level_configs[idx];
                if cfg.fill_enabled {
                    // Use fill_color or fall back to line color
                    let fill_color = cfg.fill_color.as_deref()
                        .or(cfg.color.as_deref())
                        .unwrap_or(&self.data.color.stroke);

                    // Apply fill with opacity
                    ctx.set_fill_color_alpha(fill_color, self.fill_opacity);
                    ctx.begin_path();
                    ctx.move_to(left_x, y_top);
                    ctx.line_to(right_x, y_top);
                    ctx.line_to(right_x, y_bottom);
                    ctx.line_to(left_x, y_bottom);
                    ctx.close_path();
                    ctx.fill();
                    ctx.reset_alpha();
                }
            }
        }

        // Check if we need line gap on median (0.5 level) for v_align == Center
        let needs_gap = self.data.text.as_ref()
            .map(|t| !t.content.is_empty() && matches!(t.v_align, TextAlign::Center))
            .unwrap_or(false);

        // Calculate gap parameters for median line (0.5 level)
        let (gap_x_start, gap_x_end) = if needs_gap {
            let text = self.data.text.as_ref().unwrap();
            let line_len = right_x - left_x;

            if line_len > 0.001 {
                let t_center = match text.h_align {
                    TextAlign::Start => 0.0,
                    TextAlign::Center => 0.5,
                    TextAlign::End => 1.0,
                };

                let char_count = text.content.len() as f64;
                let text_width = char_count * text.font_size * 0.6 + 8.0;
                let half_gap = text_width / 2.0;

                let text_x = left_x + line_len * t_center;
                ((text_x - half_gap).max(left_x), (text_x + half_gap).min(right_x))
            } else {
                (0.0, 0.0)
            }
        } else {
            (0.0, 0.0)
        };

        // Draw each level line with labels
        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            let level_price = self.price_at_level(cfg.level);
            let y = ctx.price_to_y(level_price);

            // Use level-specific color or fall back to main color
            let color = cfg.color.as_deref().unwrap_or(&self.data.color.stroke);

            // Build label text if needed
            let label = if self.show_prices || self.show_percentages {
                let mut label_parts = Vec::new();
                if self.show_percentages {
                    if self.show_as_percent {
                        // Smart format: 0% instead of 0.0%, 100% instead of 100.0%
                        let pct = cfg.level * 100.0;
                        if (pct - pct.round()).abs() < 0.01 {
                            label_parts.push(format!("{}%", pct as i32));
                        } else {
                            label_parts.push(format!("{:.1}%", pct));
                        }
                    } else {
                        // Smart format: 0, 1, 0.5 instead of 0.000, 1.000, 0.500
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
                if self.show_prices {
                    label_parts.push(super::super::fmt_price(level_price));
                }
                Some(label_parts.join(" "))
            } else {
                None
            };

            // Calculate label gap and position (no extra padding - flush to edge)
            let label_gap = if let Some(ref lbl) = label {
                let char_width = 6.5; // approximate
                let text_width = lbl.len() as f64 * char_width;
                let half_gap = text_width / 2.0;

                match self.label_position.as_str() {
                    "right" => {
                        // Gap at right edge - flush
                        let center_x = right_x - half_gap;
                        Some((half_gap, center_x))
                    }
                    "center" => {
                        // Gap at center
                        let center_x = (left_x + right_x) / 2.0;
                        Some((half_gap, center_x))
                    }
                    _ => { // "left"
                        // Gap at left edge - flush
                        let center_x = left_x + half_gap;
                        Some((half_gap, center_x))
                    }
                }
            } else {
                None
            };

            // Set stroke style
            ctx.set_stroke_color(color);
            let width = cfg.width.unwrap_or(self.data.width);
            ctx.set_stroke_width(width);

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

            // Check if this is the median line (0.5 level) and needs gap for main text
            let is_median = (cfg.level - 0.5).abs() < 0.001;
            let main_text_gap = if is_median && needs_gap && gap_x_end > gap_x_start {
                Some((gap_x_start, gap_x_end))
            } else {
                None
            };

            // Draw line with gaps (for main text and/or center label)
            if let Some((half_gap, center_x)) = label_gap {
                // Center label - draw line with gap
                let gap_start = (center_x - half_gap).max(left_x);
                let gap_end = (center_x + half_gap).min(right_x);

                ctx.begin_path();
                if gap_start > left_x + 0.001 {
                    ctx.move_to(crisp(left_x, dpr), crisp(y, dpr));
                    ctx.line_to(crisp(gap_start, dpr), crisp(y, dpr));
                }
                ctx.stroke();

                ctx.begin_path();
                if gap_end < right_x - 0.001 {
                    ctx.move_to(crisp(gap_end, dpr), crisp(y, dpr));
                    ctx.line_to(crisp(right_x, dpr), crisp(y, dpr));
                }
                ctx.stroke();
            } else if let Some((gap_start, gap_end)) = main_text_gap {
                // Main text gap on median
                ctx.begin_path();
                if gap_start > left_x + 0.001 {
                    ctx.move_to(crisp(left_x, dpr), crisp(y, dpr));
                    ctx.line_to(crisp(gap_start, dpr), crisp(y, dpr));
                }
                ctx.stroke();

                ctx.begin_path();
                if gap_end < right_x - 0.001 {
                    ctx.move_to(crisp(gap_end, dpr), crisp(y, dpr));
                    ctx.line_to(crisp(right_x, dpr), crisp(y, dpr));
                }
                ctx.stroke();
            } else {
                // Full line
                ctx.begin_path();
                ctx.move_to(crisp(left_x, dpr), crisp(y, dpr));
                ctx.line_to(crisp(right_x, dpr), crisp(y, dpr));
                ctx.stroke();
            }

            // Draw label with readability plate (see 7b4c1064 label-plumbing pass)
            if let Some(ref lbl) = label {
                let label_color = self.label_style.color.as_deref().unwrap_or(color);
                ctx.set_font(&self.label_style.font_string());
                ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                ctx.set_text_align(crate::render::TextAlign::Center);
                ctx.set_fill_color(label_color);

                let label_x = match self.label_position.as_str() {
                    "right" => right_x - ctx.measure_text(lbl) / 2.0 - 4.0,
                    "center" => (left_x + right_x) / 2.0,
                    _ => left_x + ctx.measure_text(lbl) / 2.0 + 4.0, // "left"
                };
                draw_label_plate(ctx, label_x, y, lbl, self.label_style.font_size, &self.label_style, label_color);
            }
        }
        ctx.set_line_dash(&[]);

        // Calculate y coordinates for points (used for trend line and control points)
        let y1 = ctx.price_to_y(self.price1);
        let y2 = ctx.price_to_y(self.price2);

        // Draw connecting line from point 1 to point 2 (if enabled).
        //
        // Sets its OWN stroke colour and width. It used to set neither, so it
        // inherited whatever the last level line in the loop above happened to
        // leave on the context — with the rank colour ramp that is the topmost
        // level, which is why the trend line rendered green on a fib whose
        // base stroke is blue (owner-reported, 2026-08-09). The trend line
        // belongs to the primitive as a whole, so it draws in the primitive's
        // own stroke, not in some level's.
        if self.show_trend_line {
            ctx.set_stroke_color(&self.data.color.stroke);
            ctx.set_stroke_width(self.data.width);
            ctx.set_line_dash(&[4.0, 4.0]);
            ctx.begin_path();
            ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
            ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
            ctx.stroke();
            ctx.set_line_dash(&[]);
        }

        // Render text if present with proper v_align positioning
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Calculate text X position based on h_align
                let line_len = right_x - left_x;
                let text_x = match text.h_align {
                    TextAlign::Start => left_x,
                    TextAlign::Center => left_x + line_len * 0.5,
                    TextAlign::End => right_x,
                };

                // Calculate text Y position based on v_align:
                // - Start: above upper boundary (topmost visible level = smallest screen Y)
                // - Center: on median (0.5 level)
                // - End: below lower boundary (bottommost visible level = largest screen Y)
                let text_y = match text.v_align {
                    TextAlign::Start | TextAlign::End => {
                        // Find topmost and bottommost visible level Y positions
                        let visible_ys: Vec<f64> = visible_levels.iter()
                            .map(|(_, _, y)| *y)
                            .collect();

                        if visible_ys.is_empty() {
                            (y1 + y2) / 2.0
                        } else {
                            let min_y = visible_ys.iter().fold(f64::INFINITY, |a, &b| a.min(b));
                            let max_y = visible_ys.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));

                            let text_offset = 8.0 + text.font_size / 2.0;
                            if matches!(text.v_align, TextAlign::Start) {
                                min_y - text_offset  // Above topmost (smaller screen Y = higher price)
                            } else {
                                max_y + text_offset  // Below bottommost (larger screen Y = lower price)
                            }
                        }
                    }
                    TextAlign::Center => {
                        // On median (0.5 level)
                        let median_price = self.price_at_level(0.5);
                        ctx.price_to_y(median_price)
                    }
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

    fn style_properties(&self) -> Vec<super::super::config::ConfigProperty> {
        use super::super::config::{ConfigProperty, PropertyCategory, PropertyGroup};
        let mut props = vec![
            // Levels/Base — the stroke every level falls back to when it
            // has no colour/width/line-style of its own (`render`, above:
            // `cfg.color.unwrap_or(&self.data.color.stroke)`, and the width
            // equivalent). Ids match the base `PrimitiveData` fields on
            // purpose (`"stroke_color"`/`"stroke_width"`/`"line_style"`) —
            // the click dispatch already special-cases these three ids to
            // write `PrimitiveData` directly (`mlc-app`'s
            // `input/settings.rs`), so declaring them here only makes the
            // Levels tab's base row a real, generic, declared control
            // instead of an assumption the modal hardcoded; it does not
            // change how a click on them is applied. `PropertyGroup::General`
            // (W2) — the approved design's own ladder "General" group
            // (`ladder_tabs()`'s `LadderGeneralGroupTitle`) — covers this
            // row and the extend/trend-line trio below it, the whole of
            // what this tab drew before named groups existed.
            ConfigProperty::color("stroke_color", SettingsKey::Color.get(current_language()), &self.data.color.stroke)
                .with_category(PropertyCategory::LevelsBase).with_order(0).in_group(PropertyGroup::General),
            ConfigProperty::number("stroke_width", ConfigKey::WidthLabel.get(current_language()), self.data.width, Some(1.0), Some(10.0))
                .with_category(PropertyCategory::LevelsBase).with_order(1).in_group(PropertyGroup::General),
            ConfigProperty::line_style("line_style", ConfigKey::Style.get(current_language()), self.data.style.as_str())
                .with_category(PropertyCategory::LevelsBase).with_order(2).in_group(PropertyGroup::General),

            // Levels/Base — how far the level lines reach, and whether the
            // dashed connector between point 1 and point 2 draws. Moved
            // here from Appearance/Line in the 4-tab cutover (2026-08-08):
            // both belong to the SAME drawn object as the ladder, not to a
            // separate "the shape's own line" the way they would on a
            // primitive with no levels — `extend_left`/`extend_right` say
            // how far every level line runs, and the trend line connects
            // the two anchors the whole ladder is built between. This,
            // together with declaring no Appearance/Line or Appearance/Fill
            // property at all (`fill_master`'s own doc below covers why
            // `show_fill`/`fill_opacity` were never declared), is what
            // makes `available_tabs` (`mlc-app-layout`) render
            // Levels · Labels · Placement for this primitive with no
            // Appearance tab at all — nothing keys off this type's id to
            // hide it; it falls out of what this method does and does not
            // declare.
            ConfigProperty::extend_left(self.extend_left).with_category(PropertyCategory::LevelsBase).with_order(10).in_group(PropertyGroup::General),
            ConfigProperty::extend_right(self.extend_right).with_category(PropertyCategory::LevelsBase).with_order(11).in_group(PropertyGroup::General),
            ConfigProperty::show_trend_line(self.show_trend_line).with_category(PropertyCategory::LevelsBase).with_order(12).in_group(PropertyGroup::General),

            // Labels — pills + label position (the ladder's own per-level
            // styling stays on the Levels tab; these are the primitive-wide
            // label toggles). Its own tab since the 4-tab cutover
            // (2026-08-08) — previously shared the Appearance tab under
            // `AppearanceLabels`. Left UNGROUPED (W2): this primitive's own
            // quirks, with no row in the approved design's ladder Labels tab
            // to address them into (`LevelLabelsGroupTitle` is font/colour/
            // inherit/bold only) — see the archetype-modals plan's own rule
            // that a type's quirk keeps its place even where the archetype
            // has no slot for it yet.
            ConfigProperty::show_prices(self.show_prices).with_category(PropertyCategory::LabelsText).in_group(PropertyGroup::LevelLabels).with_order(10),
            ConfigProperty::show_levels(self.show_percentages).with_category(PropertyCategory::LabelsText).in_group(PropertyGroup::LevelLabels).with_order(11),
            ConfigProperty::show_as_percent(self.show_as_percent).with_category(PropertyCategory::LabelsText).in_group(PropertyGroup::LevelLabels).with_order(12),
            ConfigProperty::label_position(&self.label_position).with_category(PropertyCategory::LabelsText).in_group(PropertyGroup::LevelLabels).with_order(13),

            // `show_fill`/`fill_opacity` deliberately NOT declared here —
            // see this impl's own doc above `apply_style_property` for why.

            // Labels — label font/colour + the shared
            // bold/offset/background/border block below.
            // `PropertyGroup::LevelLabels` (W2) — the approved design's own
            // `LevelLabelsGroupTitle`.
            ConfigProperty::label_font_size(self.label_style.font_size).with_category(PropertyCategory::LabelsText).with_order(40).in_group(PropertyGroup::LevelLabels),
            ConfigProperty::label_color(self.label_style.color.as_deref().unwrap_or(&self.data.color.stroke)).with_category(PropertyCategory::LabelsText).with_order(41).in_group(PropertyGroup::LevelLabels),
        ];
        props.extend(self.label_style.style_properties_grouped(&self.data.color.stroke, 50, PropertyGroup::LevelLabels));
        props
    }

    /// `fill_master()` override — see the `Primitive` trait's own doc.
    /// `show_fill`/`fill_opacity` are real fields (`render`, above, gates
    /// every band on them), just not part of the DECLARED list any more:
    /// they used to double as a `style_prop:fill`/`style_prop:fill_opacity`
    /// row that the Appearance tab drew for every level-less primitive and
    /// suppressed for every level-having one via a `has_levels` special
    /// case — the exact
    /// per-type branching the generic tab renderer (`primitive_settings_vid`)
    /// no longer does. Declaring them here would put them back in the
    /// Appearance tab's generic FILL group, right back to two on-screen
    /// controls for the one master field the Levels tab's global row already
    /// owns. `apply_style_property` below keeps handling `"fill"`/
    /// `"fill_opacity"` — that half is not id-listing, it is the write path
    /// this override's sibling (the Levels tab's global row) still uses.
    fn fill_master(&self) -> Option<(bool, f64)> {
        Some((self.show_fill, self.fill_opacity))
    }

    fn apply_style_property(&mut self, id: &str, value: &super::super::config::PropertyValue) -> bool {
        use super::super::config::PropertyValue;
        match id {
            "show_prices" => {
                if let PropertyValue::Boolean(v) = value {
                    self.show_prices = *v;
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
            "extend_left" => {
                if let PropertyValue::Boolean(v) = value {
                    self.extend_left = *v;
                    return true;
                }
            }
            "extend_right" => {
                if let PropertyValue::Boolean(v) = value {
                    self.extend_right = *v;
                    return true;
                }
            }
            "show_trend_line" => {
                if let PropertyValue::Boolean(v) = value {
                    self.show_trend_line = *v;
                    return true;
                }
            }
            "fill" => {
                if let PropertyValue::Boolean(v) = value {
                    self.show_fill = *v;
                    return true;
                }
            }
            "fill_opacity" => {
                if let Some(v) = value.as_number() {
                    self.fill_opacity = (v / 100.0).clamp(0.0, 1.0);
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

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    fn clone_box(&self) -> Box<dyn Primitive> {
        Box::new(self.clone())
    }
}

/// `radius` comes from the shared handle style rather than a local literal,
/// so the grab zone tracks the size the user picked and can never drift from
/// what `draw_control_points` actually painted. The caller resolves it once
/// per hit-test — reading it here would take a global lock per point.
fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64, radius: f64) -> bool {
    (sx - px).powi(2) + (sy - py).powi(2) <= radius * radius
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_fib_retracement(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive> {
    let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
    let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, price1));
    Box::new(FibRetracement::new(ts1, price1, ts2, price2, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "fib_retracement",
        display_name: "Fib Retracement",
        kind: PrimitiveKind::Fibonacci,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Fibonacci retracement levels",
        icon: "fib_retracement",
        default_color: "#F7B93E",
        factory: create_fib_retracement,
        supports_text: true,
        has_levels: true,
    }
}
