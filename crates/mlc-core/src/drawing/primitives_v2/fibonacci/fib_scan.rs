//! Fib autodetector (`FibScan`) — the SURVEY half of the Fibonacci family,
//! the same "the bars say what they contain" contract
//! `elliott::elliott_scan::ElliottScan`/`patterns::harmonic_scan::
//! HarmonicScan` already establish. Sibling of both: two points bracket a
//! bar RANGE, `refresh_bar_derived_cache` resolves it,
//! [`FibScan::resolved_scan`] is the ONE place this primitive calls into
//! `mli::ewa::{swing, fib_swing}`, and a Hits table + Stat block report the
//! result.
//!
//! No mli gap and no manual-primitive companion needed here, unlike A3's
//! `FiveZeroPattern`: `select_fib_swings`/`FibSwingMode`/`FibSwingDirection`
//! are already fully exposed, and `FibScan`'s own "place as primitive"
//! target is the EXISTING [`super::retracement::FibRetracement`] (2-point) —
//! a retracement drawn between a swing's own two endpoints already shows
//! extension-style ratios past 100% via its own `ALL_LEVELS` table, so ONE
//! primitive type covers both regimes. See this crate's own
//! `docs/mlc/plans/autodetectors-a2-a4-implementation-2026-09-24.md`, "A4 —
//! Fib autodetector".
//!
//! `swing_n`/`swing_mode`/`last_swings_n`/`direction`/`enabled_levels` are
//! genuinely PER-DRAWING fields (arc §2's own explicit ruling, applied here
//! the same way `ElliottScan::swing_n`'s own doc already applies it) — never
//! a `FibEngineConfig` read after construction; see [`FibScan::new`]'s own
//! doc.
//!
//! A swing renders and places through the SAME `FibRetracement` a hand-drawn
//! retracement would be — `render()`/`take_scan_placements` both call
//! [`FibScan::build_retracement`], the ONE place a swing becomes a
//! `FibRetracement`, never a hand-copy of its own level-drawing code.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{current_language, ConceptKey, SettingsKey};
use mli::fib::swing::{FibSwingDirection, FibSwingMode, FibSwingPair};
use mli::ewa::types::EwaSegmentDirection;
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, render_primitive_text, TextAlign,
    config::{ConfigProperty, SelectOption, PropertyCategory, PropertyValue, StatRow, TableCell, parse_table_cell_id, FibLevelConfig},
};
use super::super::scan_shape::{ScanShape, render_scan_bracket, price_band_filter};
use super::retracement::{FibRetracement, ALL_LEVELS};

const LEVELS_TABLE_ID: &str = "fib_scan_levels";
const HITS_TABLE_ID: &str = "fib_scan_hits";
const STATS_ID: &str = "fib_scan_stats";
/// The Hits table's own "Place" column — 0-indexed, `[Direction, Range,
/// Place]` (no "Score" column — a swing pair has no fit-quality score,
/// unlike a wave/harmonic hit).
const PLACE_COL: usize = 2;

fn default_coverage() -> String {
    "range".to_string()
}

/// Matches `SmcEngineConfig::swing_n`'s own default (Appendix A), same
/// fallback `elliott_scan::default_swing_n`/`harmonic_scan::default_swing_n`
/// use for the same reason: this fn only covers an old saved scan whose
/// JSON is missing the field entirely — a freshly constructed scan reads
/// `FibEngineConfig` instead (see [`FibScan::new`]).
fn default_swing_n() -> usize {
    2
}

fn default_swing_mode() -> String {
    "largest".to_string()
}

fn default_last_swings_n() -> usize {
    1
}

fn default_direction() -> String {
    "auto".to_string()
}

/// Every id's own fallback for an old saved scan missing `enabled_levels`
/// entirely — the SAME `MAIN_LEVELS` ladder `FibEngineConfig`'s own default
/// seeds a freshly constructed scan with.
fn default_enabled_levels() -> Vec<String> {
    super::retracement::MAIN_LEVELS.iter().map(|v| format!("{v}")).collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibScan {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64,
    #[serde(default)]
    pub ts2: i64,
    /// Two full-height vertical brackets (default) or a bounded rectangle —
    /// see [`ScanShape`]'s own doc.
    #[serde(default)]
    pub shape: ScanShape,
    #[serde(default)]
    pub p1: f64,
    #[serde(default)]
    pub p2: f64,
    #[serde(default = "default_coverage")]
    pub coverage: String,
    /// Per-drawing pivot sensitivity fed to `mli::ewa::chart_swing::extract_pivots`
    /// — same per-drawing ruling `ElliottScan::swing_n`'s own doc gives.
    #[serde(default = "default_swing_n")]
    pub swing_n: usize,
    /// `"largest"` | `"last_n"` wire id, mirrors `mli::fib::swing::
    /// FibSwingMode` — string-serialized per this crate's own "ids, never
    /// enum indices" convention. An unrecognized id (a stale saved drawing
    /// from a future catalogue) falls back to `LargestSwing` in
    /// [`Self::resolved_scan`], never panics.
    #[serde(default = "default_swing_mode")]
    pub swing_mode: String,
    /// Only read when `swing_mode == "last_n"`.
    #[serde(default = "default_last_swings_n")]
    pub last_swings_n: usize,
    /// `"auto"` | `"up"` | `"down"` wire id, mirrors `mli::fib::swing::
    /// FibSwingDirection`.
    #[serde(default = "default_direction")]
    pub direction: String,
    /// Fib ratio ids to draw — `format!("{v}")` of a value in
    /// [`ALL_LEVELS`], the SAME string-id convention every other
    /// enabled-kinds list in this arc uses.
    #[serde(default = "default_enabled_levels")]
    pub enabled_levels: Vec<String>,

    /// This scan's own resolved `[start, end]` bar-index window — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    #[serde(skip)]
    pub cached_window: Option<(usize, usize)>,
    /// This scan's own resolved swings — see [`Self::resolved_scan`].
    /// `render` recomputes this fresh and never reads this field; it exists
    /// for [`Self::style_properties`]'s own Hits table / Stat block and for
    /// [`Self::take_scan_placements`], which reads it directly rather than
    /// re-selecting (see that method's own doc).
    #[serde(skip)]
    pub cached_swings: Vec<FibSwingPair>,
    /// A Hits-table "Place" cell click's own queue — row indices into
    /// [`Self::cached_swings`], drained by [`Self::take_scan_placements`].
    #[serde(skip)]
    pending_placements: Vec<usize>,
}

impl FibScan {
    /// Reads `FibEngineConfig`'s own `scan_default_*` fields ONCE, at
    /// construction — the exact `CandlePatternEngineConfig::
    /// default_enabled_kinds` → `CandlePatternScan::new()` precedent
    /// [`ElliottScan::new`]/[`HarmonicScan::new`] already follow for their
    /// own families.
    pub fn new(ts1: i64, ts2: i64, color: &str) -> Self {
        let engine_cfg = crate::state::fib_engine::fib_engine_config();
        Self {
            data: PrimitiveData {
                type_id: "fib_scan".to_string(),
                display_name: "Fib autodetector".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            ts1,
            ts2,
            shape: ScanShape::default(),
            p1: 0.0,
            p2: 0.0,
            coverage: default_coverage(),
            swing_n: engine_cfg.scan_default_swing_n,
            swing_mode: engine_cfg.scan_default_swing_mode,
            last_swings_n: engine_cfg.scan_default_last_swings_n,
            direction: engine_cfg.scan_default_direction,
            enabled_levels: engine_cfg.scan_default_enabled_levels,
            cached_window: None,
            cached_swings: Vec::new(),
            pending_placements: Vec::new(),
        }
    }

    fn resolved_window(&self, bars: &[Bar]) -> Option<(usize, usize)> {
        if self.coverage == "all_bars" {
            return (!bars.is_empty()).then(|| (0, bars.len() - 1));
        }
        super::super::bar_window::window_indices(bars, self.ts1, self.ts2)
    }

    fn mode(&self) -> FibSwingMode {
        if self.swing_mode == "last_n" {
            FibSwingMode::LastSwings(self.last_swings_n)
        } else {
            FibSwingMode::LargestSwing
        }
    }

    fn direction_filter(&self) -> FibSwingDirection {
        match self.direction.as_str() {
            "up" => FibSwingDirection::Up,
            "down" => FibSwingDirection::Down,
            _ => FibSwingDirection::Auto,
        }
    }

    /// This survey's own swings, over [`Self::resolved_window`] — the ONE
    /// place this primitive calls `mli::ewa::{swing, fib_swing}`, so a
    /// rendered swing and a cached swing-count can never drift apart.
    /// `render` and [`Primitive::refresh_bar_derived_cache`] both call this
    /// directly. `select_fib_swings` itself is O(segments) with no
    /// windowing/enumeration cost — cheaper than `ElliottScan`'s/
    /// `HarmonicScan`'s own `scan_waves` call, no separate perf note needed.
    pub fn resolved_scan(&self, bars: &[Bar]) -> Vec<FibSwingPair> {
        let Some((start, end)) = self.resolved_window(bars) else { return Vec::new() };
        let ohlc: Vec<(f64, f64, f64, f64)> = bars.iter().map(|b| (b.open, b.high, b.low, b.close)).collect();
        let pivots = mli::ewa::chart_swing::extract_pivots(&ohlc, self.swing_n.max(1));
        let segments = mli::ewa::chart_swing::build_segments(&ohlc, &pivots);
        mli::fib::swing::select_fib_swings(&pivots, &segments, start..end + 1, self.mode(), self.direction_filter())
            .into_iter()
            .filter(|sw| price_band_filter(self.shape, self.p1, self.p2, (sw.start_price + sw.end_price) / 2.0))
            .collect()
    }

    /// Builds the SAME primitive type a hand-drawn Fibonacci retracement
    /// would be for `swing` — `render()` calls this to construct an
    /// EPHEMERAL instance and calls its own `.render(ctx, false)` (never
    /// re-implementing `FibRetracement`'s own draw code a second time);
    /// [`Self::take_scan_placements`] calls this to build the REAL instance
    /// inserted into the collection. One function, two callers — a detected
    /// swing and a hand-placed retracement can never visually drift apart.
    ///
    /// `level_configs` is set to exactly [`Self::enabled_levels`]' values,
    /// each [`FibLevelConfig::new`] with `visible = true` — mirrors
    /// `FibRetracement::default_level_configs`'s own construction loop
    /// rather than hand-building the struct a second way. `None` when
    /// `swing`'s own bar indices fall outside `bars` — defensive, unreachable
    /// for a swing [`Self::resolved_scan`] actually produced from the SAME
    /// `bars` slice.
    fn build_retracement(&self, bars: &[Bar], swing: &FibSwingPair) -> Option<FibRetracement> {
        let ts1 = bars.get(swing.start_index)?.timestamp * 1000;
        let ts2 = bars.get(swing.end_index)?.timestamp * 1000;
        let mut fib = FibRetracement::new(ts1, swing.start_price, ts2, swing.end_price, &self.data.color.stroke);
        fib.level_configs = self.enabled_levels.iter().filter_map(|id| id.parse::<f64>().ok()).map(FibLevelConfig::new).collect();
        Some(fib)
    }

    fn direction_label(direction: EwaSegmentDirection, lang: crate::i18n::Language) -> &'static str {
        match direction {
            EwaSegmentDirection::Up => ConceptKey::TrendValueUp.get(lang),
            EwaSegmentDirection::Down => ConceptKey::TrendValueDown.get(lang),
        }
    }

    /// The `fib_scan_levels` table's own rows — one per [`ALL_LEVELS`]
    /// entry, in that slice's own order (the SAME order
    /// [`Self::apply_style_property`] indexes back into via `row`).
    fn levels_table_rows(&self) -> Vec<Vec<TableCell>> {
        ALL_LEVELS
            .iter()
            .map(|&level| {
                let id = format!("{level}");
                let enabled = self.enabled_levels.iter().any(|l| l == &id);
                vec![TableCell::EditableCheck(enabled), TableCell::Text(id)]
            })
            .collect()
    }

    /// The `fib_scan_hits` table's own rows — one per resolved swing,
    /// `[Direction, Range, Place]`. `Range` shows the swing's own
    /// `[start_index, end_index]` bar span — [`FibSwingPair`] carries these
    /// directly, unlike `ElliottScan`'s/`HarmonicScan`'s own resolved hit,
    /// so this needs no `bars` of its own to format one. `Place` is always
    /// an unchecked [`TableCell::EditableCheck`] — fire-once, see the
    /// module doc's own "New mechanism" reference (shared arc doc).
    fn hits_table_rows(&self) -> Vec<Vec<TableCell>> {
        let lang = current_language();
        self.cached_swings
            .iter()
            .map(|swing| {
                vec![
                    TableCell::Text(Self::direction_label(swing.direction, lang).to_string()),
                    TableCell::Text(format!("{}\u{2013}{}", swing.start_index, swing.end_index)),
                    TableCell::EditableCheck(false),
                ]
            })
            .collect()
    }
}

impl Primitive for FibScan {
    fn type_id(&self) -> &'static str { "fib_scan" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Pattern }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, 0.0), (self.ts2, 0.0)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(t, _)) = pts.first() { self.ts1 = t; }
        if let Some(&(t, _)) = pts.get(1) { self.ts2 = t; }
    }
    fn translate(&mut self, td: i64, _pd: f64) { self.ts1 += td; self.ts2 += td; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, _price: f64) {
        match pt {
            ControlPointType::Point1 => self.ts1 = ts_ms,
            ControlPointType::Point2 => self.ts2 = ts_ms,
            ControlPointType::Move => { let td = ts_ms - self.ts1; self.translate(td, 0.0); }
            _ => {}
        }
    }

    fn hit_test(&self, sx: f64, _sy: f64, bars: &[Bar], vp: &Viewport, _ps: &PriceScale) -> HitTestResult {
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2));
        if (sx - x1).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        if sx >= min_x - HIT_TOLERANCE && sx <= max_x + HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2));
        let cy = vp.price_to_y((ps.price_min + ps.price_max) / 2.0, ps.price_min, ps.price_max);
        vec![ControlPoint::point1(x1, cy), ControlPoint::point2(x2, cy)]
    }

    /// The two dashed boundaries, then every resolved swing rendered through
    /// the SAME `FibRetracement` a hand-drawn one would be — see the module's
    /// own doc. Unlike `ElliottScan`/`HarmonicScan`, no proximity-cluster
    /// collapse: `select_fib_swings` returns at most `last_swings_n` swings
    /// (`LargestSwing` returns at most one), never the dense per-bar hit
    /// stream the wave/harmonic scanners can produce, so there is nothing to
    /// collapse. Computed FRESH from `bars` here via [`Self::resolved_scan`]
    /// — never reads [`Self::cached_swings`] — matching
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let (x1, x2) = match self.resolved_window(ctx.bars()) {
            Some((s, e)) if self.coverage == "all_bars" => (ctx.bar_to_x(s as f64), ctx.bar_to_x(e as f64)),
            _ => (ctx.ts_to_x_ms(self.ts1), ctx.ts_to_x_ms(self.ts2)),
        };
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        let chart_height = ctx.chart_height();

        render_scan_bracket(ctx, self.shape, x1, x2, self.p1, self.p2, &self.data.color.stroke, self.data.width);

        // Every swing resolved into a real ephemeral `FibRetracement` BEFORE
        // any of them render — `bars` is borrowed from `ctx` only for this
        // block, never held across the mutating `.render()` calls below
        // (same reason `ElliottScan::render`'s own `anchored` vec is built
        // in its own block before its cluster loop runs).
        let ephemeral: Vec<FibRetracement> = {
            let bars = ctx.bars();
            self.resolved_scan(bars).iter().filter_map(|swing| self.build_retracement(bars, swing)).collect()
        };
        for fib in &ephemeral {
            fib.render(ctx, false);
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => min_x,
                    TextAlign::Center => (min_x + max_x) / 2.0,
                    TextAlign::End => max_x,
                };
                let text_y = match text.v_align {
                    TextAlign::Start => -text_offset,
                    TextAlign::Center => chart_height / 2.0,
                    TextAlign::End => chart_height + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    /// Recompute [`Self::cached_window`]/[`Self::cached_swings`] from `bars`
    /// through the SAME [`Self::resolved_window`]/[`Self::resolved_scan`]
    /// calls `render` makes directly — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    fn refresh_bar_derived_cache(&mut self, bars: &[Bar]) {
        self.cached_window = self.resolved_window(bars);
        self.cached_swings = self.resolved_scan(bars);
    }

    /// Drains [`Self::pending_placements`] into real, independent
    /// `FibRetracement` primitives — one per queued row, looked up in
    /// [`Self::cached_swings`] (never re-selecting: this reads the SAME
    /// freshly-resolved swing geometry `refresh_bar_derived_cache` just
    /// cached, called immediately before this by the SAME per-primitive
    /// sweep — see `mlc_backend::bubble::bar_derived_cache::
    /// refresh_bar_derived_caches`).
    fn take_scan_placements(&mut self, bars: &[Bar]) -> Vec<Box<dyn Primitive>> {
        if self.pending_placements.is_empty() {
            return Vec::new();
        }
        let rows = std::mem::take(&mut self.pending_placements);
        rows.into_iter()
            .filter_map(|row| self.cached_swings.get(row))
            .filter_map(|swing| self.build_retracement(bars, swing))
            .map(|fib| Box::new(fib) as Box<dyn Primitive>)
            .collect()
    }

    // No `markup_family`/`context_violations` override: a SURVEY makes no
    // claim of its own (this module's own doc), matching `ElliottScan`'s/
    // `HarmonicScan`'s own choice — see the arc's own "No `MarkupFamily`
    // change" section.

    fn style_properties(&self) -> Vec<ConfigProperty> {
        let lang = current_language();
        let levels_head = vec![SettingsKey::Show.get(lang).to_string(), ConceptKey::LevelColumnHeader.get(lang).to_string()];
        let hits_head = vec![
            ConceptKey::FibScanDirectionLabel.get(lang).to_string(),
            ConceptKey::FibScanHitRangeColumnHeader.get(lang).to_string(),
            ConceptKey::ScanHitPlaceColumnHeader.get(lang).to_string(),
        ];
        let bars_surveyed = self.cached_window.map(|(s, e)| e - s + 1).unwrap_or(0);

        let mut props = vec![
            // Coverage/Shape — ungrouped, `AppearanceStats`, same order
            // convention `SmcScan`/`ElliottScan`/`HarmonicScan` already
            // establish (coverage 5, shape 7). No `degree` row here — a Fib
            // swing carries no `WaveDegree` concept.
            ConfigProperty::pills(
                "coverage",
                ConceptKey::ScanCoverageLabel.get(lang),
                &self.coverage,
                vec![
                    SelectOption::new("range", ConceptKey::ScanCoverageRange.get(lang)),
                    SelectOption::new("all_bars", ConceptKey::ScanCoverageAllBars.get(lang)),
                ],
            ).with_category(PropertyCategory::AppearanceStats).with_order(5),
            ConfigProperty::pills(
                "shape",
                ConceptKey::ScanShapeLabel.get(lang),
                self.shape.id(),
                vec![
                    SelectOption::new(ScanShape::TwoLines.id(), ConceptKey::ScanShapeTwoLines.get(lang)),
                    SelectOption::new(ScanShape::Rectangle.id(), ConceptKey::ScanShapeRectangle.get(lang)),
                ],
            ).with_category(PropertyCategory::AppearanceStats).with_order(7),

            // `PropertyGroup::ScanWhatToDetect` — the levels table, NOT a
            // kinds-checkbox table (a Fib swing has no "kind" — every swing
            // is the same detector).
            ConfigProperty::table(LEVELS_TABLE_ID, ConceptKey::FibScanLevelsTableLabel.get(lang), levels_head, self.levels_table_rows())
                .with_category(PropertyCategory::AppearanceStats)
                .with_order(10)
                .in_group(super::super::config::PropertyGroup::ScanWhatToDetect),

            // `PropertyGroup::ScanThresholds`.
            ConfigProperty::pills(
                "swing_n",
                ConceptKey::MarkupThresholdSwingNLabel.get(lang),
                &self.swing_n.to_string(),
                vec![
                    SelectOption::new("1", "1"),
                    SelectOption::new("2", "2"),
                    SelectOption::new("3", "3"),
                    SelectOption::new("5", "5"),
                ],
            ).with_category(PropertyCategory::AppearanceStats)
                .with_order(11)
                .in_group(super::super::config::PropertyGroup::ScanThresholds),
            ConfigProperty::pills(
                "swing_mode",
                ConceptKey::FibScanSwingModeLabel.get(lang),
                &self.swing_mode,
                vec![
                    SelectOption::new("largest", ConceptKey::FibScanSwingModeLargest.get(lang)),
                    SelectOption::new("last_n", ConceptKey::FibScanSwingModeLastN.get(lang)),
                ],
            ).with_category(PropertyCategory::AppearanceStats)
                .with_order(12)
                .in_group(super::super::config::PropertyGroup::ScanThresholds),
        ];

        // `last_swings_n` — only meaningful/rendered when `swing_mode ==
        // "last_n"`, mirroring `markup_engine_settings.rs`'s own
        // conditional-row pattern (e.g. `single_print_run` only drawing for
        // `OrderFlow`), done here as a conditional push into this method's
        // own returned `Vec` rather than an imperative renderer.
        if self.swing_mode == "last_n" {
            props.push(
                ConfigProperty::pills(
                    "last_swings_n",
                    ConceptKey::FibScanLastSwingsNLabel.get(lang),
                    &self.last_swings_n.to_string(),
                    vec![
                        SelectOption::new("1", "1"),
                        SelectOption::new("2", "2"),
                        SelectOption::new("3", "3"),
                        SelectOption::new("5", "5"),
                    ],
                ).with_category(PropertyCategory::AppearanceStats)
                    .with_order(13)
                    .in_group(super::super::config::PropertyGroup::ScanThresholds),
            );
        }

        props.push(
            ConfigProperty::pills(
                "direction",
                ConceptKey::FibScanDirectionLabel.get(lang),
                &self.direction,
                vec![
                    SelectOption::new("auto", ConceptKey::FibScanDirectionAuto.get(lang)),
                    SelectOption::new("up", ConceptKey::TrendValueUp.get(lang)),
                    SelectOption::new("down", ConceptKey::TrendValueDown.get(lang)),
                ],
            ).with_category(PropertyCategory::AppearanceStats)
                .with_order(14)
                .in_group(super::super::config::PropertyGroup::ScanThresholds),
        );

        props.push(
            ConfigProperty::table(HITS_TABLE_ID, ConceptKey::ScanHitsColumnHeader.get(lang), hits_head, self.hits_table_rows())
                .with_category(PropertyCategory::AppearanceStats)
                .with_order(20),
        );

        props.push(
            ConfigProperty::stat_block(
                STATS_ID,
                vec![
                    StatRow::new(ConceptKey::ScanTotalHitsLabel.get(lang), &self.cached_swings.len().to_string(), None),
                    StatRow::new(ConceptKey::BarCount.get(lang), &bars_surveyed.to_string(), None),
                ],
            ).with_order(30),
        );

        props
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        if id == "coverage" {
            let Some(s) = value.as_string() else { return false };
            if s != "range" && s != "all_bars" {
                return false;
            }
            self.coverage = s.to_string();
            return true;
        }
        if id == "shape" {
            let Some(s) = value.as_string() else { return false };
            let Some(shape) = ScanShape::from_id(s) else { return false };
            self.shape = shape;
            return true;
        }
        if id == "swing_n" {
            let Some(s) = value.as_string() else { return false };
            let Ok(n) = s.parse::<usize>() else { return false };
            self.swing_n = n.max(1);
            return true;
        }
        if id == "swing_mode" {
            let Some(s) = value.as_string() else { return false };
            if s != "largest" && s != "last_n" {
                return false;
            }
            self.swing_mode = s.to_string();
            return true;
        }
        if id == "last_swings_n" {
            let Some(s) = value.as_string() else { return false };
            let Ok(n) = s.parse::<usize>() else { return false };
            self.last_swings_n = n.max(1);
            return true;
        }
        if id == "direction" {
            let Some(s) = value.as_string() else { return false };
            if s != "auto" && s != "up" && s != "down" {
                return false;
            }
            self.direction = s.to_string();
            return true;
        }
        if let Some((base, row, col)) = parse_table_cell_id(id) {
            if base == LEVELS_TABLE_ID && col == 0 {
                let Some(enabled) = value.as_bool() else { return false };
                let Some(&level) = ALL_LEVELS.get(row) else { return false };
                let id = format!("{level}");
                if enabled {
                    if !self.enabled_levels.iter().any(|l| l == &id) {
                        self.enabled_levels.push(id);
                    }
                } else {
                    self.enabled_levels.retain(|l| l != &id);
                }
                return true;
            }
            if base == HITS_TABLE_ID && col == PLACE_COL {
                if value.as_bool() == Some(true) {
                    self.pending_placements.push(row);
                }
                return true;
            }
            return false;
        }
        if id == STATS_ID {
            return false;
        }
        false
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "fib_scan",
        display_name: "Fib autodetector",
        kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Survey a bar range for Fibonacci swings",
        icon: "fib_scan",
        // Reuses `FibRetracement`'s own default colour (the "no colour
        // literal beyond the family's own token" standing rule).
        default_color: "#F7B93E",
        factory: |points, color| {
            let (t1, _) = points.first().copied().unwrap_or((0, 0.0));
            let (t2, _) = points.get(1).copied().unwrap_or((t1 + 72_000_000, 0.0));
            Box::new(FibScan::new(t1, t2, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three-segment fixture `mli::ewa::fib_swing`'s own
    /// `largest_swing_picks_the_biggest_of_several` test uses
    /// (`mylittleindicators/src/ewa/fib_swing.rs`): four pivots at bar
    /// indices 0/5/10/15, giving three segments Up(+20)/Down(-30)/Up(+60).
    /// That helper's own fixture builder is private to `mli`'s own
    /// `#[cfg(test)]` module and unreachable from this crate, so this
    /// reimplements it locally via bars whose own `swing::extract_pivots`
    /// output reproduces the SAME pivot/segment shape, the same posture
    /// `elliott_scan.rs`'s/`harmonic_scan.rs`'s own test modules already
    /// take for their own fixtures.
    fn ramp(from: f64, to: f64, steps: usize, spread: f64) -> Vec<(f64, f64, f64, f64)> {
        (1..=steps)
            .map(|k| {
                let t = k as f64 / (steps as f64 + 1.0);
                let level = from + (to - from) * t;
                let (high, low) = if to >= from { (level, level - spread) } else { (level + spread, level) };
                (level, high, low, level)
            })
            .collect()
    }

    fn pivot_bar(is_high: bool, price: f64, spread: f64) -> (f64, f64, f64, f64) {
        if is_high {
            (price - spread / 2.0, price, price - spread, price - spread / 2.0)
        } else {
            (price + spread / 2.0, price + spread, price, price + spread / 2.0)
        }
    }

    /// Four pivots — Low 100 / High 120 / Low 90 / High 150 — giving three
    /// segments Up(+20)/Down(-30)/Up(+60), the SAME shape mli's own
    /// `three_segment_fixture` builds directly from `EwaSwingPivot`/
    /// `EwaSwingSegment` values.
    fn three_segment_fixture_bars() -> Vec<Bar> {
        let is_high = [false, true, false, true];
        let prices = [100.0, 120.0, 90.0, 150.0];
        let steps = 3usize;
        let spread = 0.2;
        let mut ohlc = Vec::new();
        let lead_from = if is_high[0] { prices[0] - 8.0 } else { prices[0] + 8.0 };
        ohlc.extend(ramp(lead_from, prices[0], 2, spread));
        for i in 0..prices.len() {
            if i > 0 {
                ohlc.extend(ramp(prices[i - 1], prices[i], steps, spread));
            }
            ohlc.push(pivot_bar(is_high[i], prices[i], spread));
        }
        let last_price = *prices.last().unwrap_or(&0.0);
        let trail_to = if *is_high.last().unwrap_or(&false) { last_price - 8.0 } else { last_price + 8.0 };
        ohlc.extend(ramp(last_price, trail_to, 2, spread));

        ohlc.into_iter().enumerate().map(|(i, (o, h, l, c))| Bar::new(i as i64, o, h, l, c)).collect()
    }

    fn new_scan_over(bars: &[Bar]) -> FibScan {
        let mut scan = FibScan::new(0, (bars.len() as i64 - 1) * 1000, "#F7B93E");
        scan.swing_n = 2;
        scan
    }

    #[test]
    fn resolved_scan_identity_against_a_direct_mli_call() {
        let bars = three_segment_fixture_bars();
        let scan = new_scan_over(&bars);
        let swings = scan.resolved_scan(&bars);

        let ohlc: Vec<(f64, f64, f64, f64)> = bars.iter().map(|b| (b.open, b.high, b.low, b.close)).collect();
        let pivots = mli::ewa::chart_swing::extract_pivots(&ohlc, 2);
        let segments = mli::ewa::chart_swing::build_segments(&ohlc, &pivots);
        let direct = mli::fib::swing::select_fib_swings(
            &pivots, &segments, 0..bars.len(), FibSwingMode::LargestSwing, FibSwingDirection::Auto,
        );

        assert!(!direct.is_empty(), "fixture must actually contain a largest swing");
        assert_eq!(swings.len(), direct.len());
        assert_eq!(swings[0], direct[0]);
        assert_eq!(swings[0].direction, EwaSegmentDirection::Up);
    }

    #[test]
    fn last_n_mode_returns_the_most_recent_swings_in_order() {
        let bars = three_segment_fixture_bars();
        let mut scan = new_scan_over(&bars);
        scan.swing_mode = "last_n".to_string();
        scan.last_swings_n = 2;

        let swings = scan.resolved_scan(&bars);
        assert_eq!(swings.len(), 2, "LastSwings(2) must return exactly two pairs");
        assert_eq!(swings[0].direction, EwaSegmentDirection::Down);
        assert_eq!(swings[1].direction, EwaSegmentDirection::Up);
    }

    #[test]
    fn direction_filter_keeps_only_matching_swings() {
        let bars = three_segment_fixture_bars();
        let mut scan = new_scan_over(&bars);
        scan.direction = "down".to_string();

        let swings = scan.resolved_scan(&bars);
        assert_eq!(swings.len(), 1);
        assert_eq!(swings[0].direction, EwaSegmentDirection::Down);
    }

    #[test]
    fn render_cache_is_absent_from_to_json() {
        let bars = three_segment_fixture_bars();
        let mut scan = new_scan_over(&bars);
        scan.refresh_bar_derived_cache(&bars);
        assert!(!scan.cached_swings.is_empty());

        let json = scan.to_json();
        assert!(!json.contains("cached_window"), "{json}");
        assert!(!json.contains("cached_swings"), "{json}");
        assert!(!json.contains("pending_placements"), "{json}");

        let mut registry = super::super::super::registry::PrimitiveRegistry::new();
        registry.register_typed::<FibScan>(metadata());
        let restored = registry.from_json("fib_scan", &json).expect("round trip");
        assert_eq!(restored.points(), scan.points());
    }

    #[test]
    fn rectangle_shape_price_band_filters_a_swing() {
        let bars = three_segment_fixture_bars();
        let mut scan = new_scan_over(&bars);
        let unfiltered = scan.resolved_scan(&bars);
        assert!(!unfiltered.is_empty());
        let mid = (unfiltered[0].start_price + unfiltered[0].end_price) / 2.0;

        scan.shape = ScanShape::Rectangle;
        scan.p1 = mid + 1000.0;
        scan.p2 = mid + 2000.0; // band far away from the swing's own midpoint
        assert!(scan.resolved_scan(&bars).is_empty(), "a price band that excludes the swing must drop it");
    }

    #[test]
    fn levels_table_cell_click_toggles_enabled_levels() {
        let mut scan = FibScan::new(0, 13_000, "#F7B93E");
        let row = ALL_LEVELS.iter().position(|&l| l == 0.618).expect("catalogue has 0.618");
        let cell_id = format!("{LEVELS_TABLE_ID}::cell:{row}:0");

        assert!(scan.apply_style_property(&cell_id, &PropertyValue::Boolean(false)));
        assert!(!scan.enabled_levels.iter().any(|l| l == "0.618"));

        assert!(scan.apply_style_property(&cell_id, &PropertyValue::Boolean(true)));
        assert!(scan.enabled_levels.iter().any(|l| l == "0.618"));
    }

    #[test]
    fn place_click_then_take_scan_placements_yields_a_matching_retracement() {
        let bars = three_segment_fixture_bars();
        let mut scan = new_scan_over(&bars);
        scan.enabled_levels = vec!["0".to_string(), "0.618".to_string(), "1".to_string()];
        scan.refresh_bar_derived_cache(&bars);
        assert!(!scan.cached_swings.is_empty(), "largest swing must be cached");

        let cell_id = format!("{HITS_TABLE_ID}::cell:0:{PLACE_COL}");
        assert!(scan.apply_style_property(&cell_id, &PropertyValue::Boolean(true)));

        let placed = scan.take_scan_placements(&bars);
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].type_id(), "fib_retracement");
        let swing = scan.cached_swings[0];
        let expected_ts1 = bars[swing.start_index].timestamp * 1000;
        let expected_ts2 = bars[swing.end_index].timestamp * 1000;
        assert_eq!(placed[0].points(), vec![(expected_ts1, swing.start_price), (expected_ts2, swing.end_price)]);
        let levels = placed[0].level_configs().expect("retracement carries levels");
        let mut level_values: Vec<f64> = levels.iter().map(|c| c.level).collect();
        level_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        assert_eq!(level_values, vec![0.0, 0.618, 1.0]);

        // The queue does not refill itself.
        let placed_again = scan.take_scan_placements(&bars);
        assert!(placed_again.is_empty());
    }

    #[test]
    fn shape_and_coverage_properties_round_trip() {
        let mut scan = FibScan::new(0, 13_000, "#F7B93E");
        assert_eq!(scan.shape, ScanShape::TwoLines);
        assert!(scan.apply_style_property("shape", &PropertyValue::String("rectangle".to_string())));
        assert_eq!(scan.shape, ScanShape::Rectangle);
        assert!(scan.apply_style_property("shape", &PropertyValue::String("two_lines".to_string())));
        assert_eq!(scan.shape, ScanShape::TwoLines);
        assert!(!scan.apply_style_property("shape", &PropertyValue::String("bogus".to_string())));
        assert!(!scan.apply_style_property("shape", &PropertyValue::Boolean(true)));

        assert_eq!(scan.coverage, "range");
        assert!(scan.apply_style_property("coverage", &PropertyValue::String("all_bars".to_string())));
        assert_eq!(scan.coverage, "all_bars");
        assert!(scan.apply_style_property("coverage", &PropertyValue::String("range".to_string())));
        assert_eq!(scan.coverage, "range");
        assert!(!scan.apply_style_property("coverage", &PropertyValue::String("bogus".to_string())));
    }

    #[test]
    fn swing_mode_and_direction_properties_round_trip() {
        let mut scan = FibScan::new(0, 13_000, "#F7B93E");
        assert_eq!(scan.swing_mode, "largest");
        assert!(scan.apply_style_property("swing_mode", &PropertyValue::String("last_n".to_string())));
        assert_eq!(scan.swing_mode, "last_n");
        assert!(!scan.apply_style_property("swing_mode", &PropertyValue::String("bogus".to_string())));

        assert!(scan.apply_style_property("last_swings_n", &PropertyValue::String("3".to_string())));
        assert_eq!(scan.last_swings_n, 3);

        assert_eq!(scan.direction, "auto");
        assert!(scan.apply_style_property("direction", &PropertyValue::String("up".to_string())));
        assert_eq!(scan.direction, "up");
        assert!(!scan.apply_style_property("direction", &PropertyValue::String("bogus".to_string())));
    }

    /// `last_swings_n` row only appears in `style_properties()` while
    /// `swing_mode == "last_n"` — the conditional-push shape this file's
    /// own doc mirrors from `markup_engine_settings.rs`.
    #[test]
    fn last_swings_n_row_is_conditional_on_swing_mode() {
        let mut scan = FibScan::new(0, 13_000, "#F7B93E");
        assert!(!scan.style_properties().iter().any(|p| p.id == "last_swings_n"));
        scan.swing_mode = "last_n".to_string();
        assert!(scan.style_properties().iter().any(|p| p.id == "last_swings_n"));
    }
}
