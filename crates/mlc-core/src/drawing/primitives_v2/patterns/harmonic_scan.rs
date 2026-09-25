//! Harmonic autodetector (`HarmonicScan`) — the SURVEY half of the harmonic
//! family, the same "the bars say what they contain" contract
//! `elliott::elliott_scan::ElliottScan`/`smc::smc_scan::SmcScan` already
//! establish. Sibling of `ElliottScan` (A2): two points bracket a bar RANGE,
//! `refresh_bar_derived_cache` resolves it, [`HarmonicScan::resolved_scan`]
//! is the ONE place this primitive calls into `mli::ewa::{swing, scan}`, and
//! a Hits table + Stat block report the result.
//!
//! Surveys the 11 harmonic kinds `HarmonicScanKind` names — see this crate's
//! own `docs/mlc/plans/autodetectors-a2-a4-implementation-2026-09-24.md`,
//! "A3 — Harmonics autodetector".
//!
//! `swing_n`/`min_score`/`tolerance` are genuinely PER-DRAWING fields (arc
//! §3's own explicit ruling — "mirrors `XabcdPattern::harmonic_tolerance`'s
//! own per-drawing field") — never a `HarmonicEngineConfig` read after
//! construction; see [`HarmonicScan::new`]'s own doc.
//!
//! A hit renders and places through the SAME primitive a hand-drawn
//! structure of its own kind would be
//! ([`super::super::ewa_scan_shared::build_wave_primitive`]) — this file
//! never re-implements `XabcdPattern`/`CypherPattern`/`AbcdPattern`/
//! `ThreeDrives`/`FiveZeroPattern`'s own drawing code. `ResolvedWaveHit`/
//! `resolve_hit` are the SAME shared types `ElliottScan` uses (`super::super::
//! ewa_scan_shared`) — one pivot-resolution arithmetic, not a second copy.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{current_language, ConceptKey, SettingsKey};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, render_primitive_text, TextAlign,
    config::{ConfigProperty, SelectOption, PropertyCategory, PropertyValue, StatRow, TableCell, Tone, WaveDegree, parse_table_cell_id},
};
use super::super::ewa_scan_shared::{build_wave_primitive, ewa_scan_window, resolve_hit, ResolvedWaveHit};
use super::super::scan_shape::{ScanShape, render_scan_bracket};
use super::super::SCAN_MARKER_MIN_GAP_PX as MIN_MARKER_GAP_PX;
use super::harmonic_scan_kind::HarmonicScanKind;

const KINDS_TABLE_ID: &str = "harmonic_scan_kinds";
const HITS_TABLE_ID: &str = "harmonic_scan_hits";
const STATS_ID: &str = "harmonic_scan_stats";
/// The Hits table's own "Place" column — 0-indexed, `[Kind, Pivots, Score,
/// Place]`.
const PLACE_COL: usize = 3;

fn default_coverage() -> String {
    "range".to_string()
}

/// Matches `SmcEngineConfig::swing_n`'s own default (Appendix A), same
/// fallback `elliott_scan::default_swing_n` uses for the same reason: this
/// fn only covers an old saved scan whose JSON is missing the field
/// entirely — a freshly constructed scan reads `HarmonicEngineConfig`
/// instead (see [`HarmonicScan::new`]).
fn default_swing_n() -> usize {
    2
}

/// [`HarmonicScanKind::id`] wire ids — every harmonic kind this scan
/// surveys, the fallback an old saved scan missing the field entirely lands
/// on.
fn default_all_kind_ids() -> Vec<String> {
    super::harmonic_scan_kind::default_all_harmonic_kind_ids()
}

/// The vocabulary's own default harmonic-band tolerance — see
/// `mli::ewa::harmonic::DEFAULT_HARMONIC_TOLERANCE`'s own doc. Only covers
/// an old saved scan missing the field entirely; a freshly constructed scan
/// reads `HarmonicEngineConfig::scan_default_tolerance` instead.
fn default_tolerance() -> f64 {
    mli::ewa::harmonic::DEFAULT_HARMONIC_TOLERANCE
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HarmonicScan {
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
    /// Per-drawing pivot sensitivity fed to `mli::ewa::swing::extract_pivots`
    /// — same per-drawing ruling `ElliottScan::swing_n`'s own doc gives.
    #[serde(default = "default_swing_n")]
    pub swing_n: usize,
    /// Post-filter on `EwaWaveHit::score` — mlc-side only, no `mli` change
    /// needed (score is already returned per-hit).
    #[serde(default)]
    pub min_score: f64,
    /// Widening delta (`mli::ewa::harmonic::widen_band`) applied to every
    /// target leg's own band before `scan_waves_with_harmonic_tolerance`'s
    /// own hard band check (A3.0) — the SAME per-drawing knob
    /// `XabcdPattern`/`FiveZeroPattern`'s own `harmonic_tolerance` field
    /// already applies on a MANUAL drawing, now real for a SURVEY too.
    #[serde(default = "default_tolerance")]
    pub tolerance: f64,
    /// [`HarmonicScanKind::id`] wire ids — the 11 harmonic kinds this scan
    /// surveys.
    #[serde(default = "default_all_kind_ids")]
    pub enabled_kinds: Vec<String>,

    /// This scan's own resolved `[start, end]` bar-index window — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    #[serde(skip)]
    pub cached_window: Option<(usize, usize)>,
    /// This scan's own resolved hits — see [`Self::resolved_scan`]. `render`
    /// recomputes this fresh and never reads this field; it exists for
    /// [`Self::style_properties`]'s own Hits table / Stat block and for
    /// [`Self::take_scan_placements`], which reads it directly rather than
    /// re-scanning (see that method's own doc).
    #[serde(skip)]
    pub cached_hits: Vec<ResolvedWaveHit>,
    /// A Hits-table "Place" cell click's own queue — row indices into
    /// [`Self::cached_hits`], drained by [`Self::take_scan_placements`].
    #[serde(skip)]
    pending_placements: Vec<usize>,
}

impl HarmonicScan {
    /// Reads `HarmonicEngineConfig`'s own `scan_default_*` fields ONCE, at
    /// construction — the exact `CandlePatternEngineConfig::
    /// default_enabled_kinds` → `CandlePatternScan::new()` precedent
    /// [`ElliottScan::new`] already follows for the Elliott side.
    pub fn new(ts1: i64, ts2: i64, color: &str) -> Self {
        let engine_cfg = crate::state::harmonic_engine::harmonic_engine_config();
        Self {
            data: PrimitiveData {
                type_id: "harmonic_scan".to_string(),
                display_name: "Harmonics autodetector".to_string(),
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
            swing_n: default_swing_n(),
            min_score: engine_cfg.scan_default_min_score,
            tolerance: engine_cfg.scan_default_tolerance,
            enabled_kinds: engine_cfg.scan_default_enabled_kinds,
            cached_window: None,
            cached_hits: Vec::new(),
            pending_placements: Vec::new(),
        }
    }

    fn resolved_window(&self, bars: &[Bar]) -> Option<(usize, usize)> {
        if self.coverage == "all_bars" {
            return (!bars.is_empty()).then(|| (0, bars.len() - 1));
        }
        super::super::bar_window::window_indices(bars, self.ts1, self.ts2)
    }

    fn enabled(&self, kind: HarmonicScanKind) -> bool {
        self.enabled_kinds.iter().any(|id| id == kind.id())
    }

    /// This survey's own hits, over [`Self::resolved_window`] — the ONE
    /// place this primitive calls `mli::ewa::{swing, scan}`, so a rendered
    /// hit and a cached hit-count can never drift apart. `render` and
    /// [`Primitive::refresh_bar_derived_cache`] both call this directly.
    /// Calls `scan_waves_with_harmonic_tolerance` (A3.0), never the plain
    /// `scan_waves` `ElliottScan` uses — `self.tolerance` must reach the
    /// band check for the arc's own "control over nothing" defect not to
    /// reappear here.
    ///
    /// [`ewa_scan_window`] bounds the bars handed to `extract_pivots`/
    /// `scan_waves_with_harmonic_tolerance` to a lookback window anchored at
    /// the survey's own right edge — see that fn's own doc for why:
    /// `mli::ewa::scan`'s hard `MAX_PIVOTS_PER_SCAN` cap silently discards
    /// every RECENT pivot once the loaded series accumulates too many, so
    /// feeding it the whole series (as this used to) made a survey dragged
    /// over current price action always return zero hits.
    pub fn resolved_scan(&self, bars: &[Bar]) -> Vec<ResolvedWaveHit> {
        let Some((start, end)) = self.resolved_window(bars) else { return Vec::new() };
        let (window_bars, local_range) = ewa_scan_window(bars, start, end);
        let ohlc: Vec<(f64, f64, f64, f64)> = window_bars.iter().map(|b| (b.open, b.high, b.low, b.close)).collect();
        let pivots = mli::ewa::swing::extract_pivots(&ohlc, self.swing_n.max(1));
        let segments = mli::ewa::swing::build_segments(&ohlc, &pivots);
        let kinds: Vec<mli::ewa::types::EwaPatternKind> =
            self.enabled_kinds.iter().filter_map(|id| HarmonicScanKind::from_id(id)).map(|k| k.as_ewa_kind()).collect();
        mli::ewa::scan::scan_waves_with_harmonic_tolerance(&pivots, &segments, &kinds, local_range, self.tolerance)
            .into_iter()
            .filter(|h| h.score >= self.min_score)
            .filter_map(|h| resolve_hit(h, &pivots, window_bars, self.shape, self.p1, self.p2))
            .collect()
    }

    /// The `harmonic_scan_kinds` table's own rows — one per
    /// [`HarmonicScanKind`], in [`HarmonicScanKind::ALL`]'s own order (the
    /// SAME order [`Self::apply_style_property`] indexes back into via
    /// `row`). Hit counts read [`Self::cached_hits`] — never a second
    /// `resolved_scan` call from a method with no `bars` of its own.
    fn kinds_table_rows(&self) -> Vec<Vec<TableCell>> {
        let lang = current_language();
        HarmonicScanKind::ALL
            .iter()
            .map(|&kind| {
                let hits = self.cached_hits.iter().filter(|h| h.kind == kind.as_ewa_kind()).count();
                vec![
                    TableCell::EditableCheck(self.enabled(kind)),
                    TableCell::Text(kind.label(lang).to_string()),
                    TableCell::Text(hits.to_string()),
                ]
            })
            .collect()
    }

    /// The `harmonic_scan_hits` table's own rows — one per resolved hit,
    /// `[Kind, Pivots, Score, Place]`. `Pivots` shows the pivot COUNT (the
    /// resolved hit carries no raw bar index this method could format as a
    /// range — see [`ResolvedWaveHit`]'s own doc — and this method has no
    /// `bars` of its own to re-derive one from timestamps), the SAME choice
    /// `elliott_scan::ElliottScan::hits_table_rows` already makes; `Place`
    /// is always an unchecked [`TableCell::EditableCheck`] — fire-once, see
    /// the module doc's own "New mechanism" reference.
    fn hits_table_rows(&self) -> Vec<Vec<TableCell>> {
        let lang = current_language();
        self.cached_hits
            .iter()
            .map(|hit| {
                let kind_label = HarmonicScanKind::from_ewa_kind(hit.kind).map(|k| k.label(lang).to_string()).unwrap_or_default();
                let tone = if hit.score >= 0.7 { Tone::Good } else { Tone::Neutral };
                vec![
                    TableCell::Text(kind_label),
                    TableCell::Text(hit.points.len().to_string()),
                    TableCell::Toned(format!("{:.2}", hit.score), tone),
                    TableCell::EditableCheck(false),
                ]
            })
            .collect()
    }
}

impl Primitive for HarmonicScan {
    fn type_id(&self) -> &'static str { "harmonic_scan" }
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

    /// The two dashed boundaries, then every resolved hit rendered through
    /// the SAME primitive a hand-drawn structure of its own kind would be —
    /// see the module's own doc. Consecutive hits whose own last-pivot bar
    /// index sits within [`MIN_MARKER_GAP_PX`] collapse: only the first of
    /// the run draws its full geometry, carrying a small count badge — the
    /// same proximity degrade `ElliottScan::render`/`SmcScan::render`
    /// already choose. Computed FRESH from `bars` here via
    /// [`Self::resolved_scan`] — never reads [`Self::cached_hits`] —
    /// matching [`Primitive::refresh_bar_derived_cache`]'s own contract.
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let (x1, x2) = match self.resolved_window(ctx.bars()) {
            Some((s, e)) if self.coverage == "all_bars" => (ctx.bar_to_x(s as f64), ctx.bar_to_x(e as f64)),
            _ => (ctx.ts_to_x_ms(self.ts1), ctx.ts_to_x_ms(self.ts2)),
        };
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        let chart_height = ctx.chart_height();

        render_scan_bracket(ctx, self.shape, x1, x2, self.p1, self.p2, &self.data.color.stroke, self.data.width);

        let hits = self.resolved_scan(ctx.bars());
        // Cluster key: last pivot's bar index (this module's own doc, same
        // convention `ElliottScan::render` already establishes). Resolved
        // here, once, into a plain `usize` BEFORE `for_each_cluster` runs.
        let mut anchored: Vec<(usize, ResolvedWaveHit)> = {
            let bars_ref = ctx.bars();
            hits.into_iter()
                .map(|h| {
                    let last_ts = h.points.last().map(|&(ts, _)| ts).unwrap_or(self.ts1);
                    let idx = timestamp_ms_to_bar_f64(bars_ref, last_ts).round().max(0.0) as usize;
                    (idx, h)
                })
                .collect()
        };
        // `scan_waves_with_harmonic_tolerance` carries no documented output
        // ordering — sort defensively so the collapse below always walks
        // ascending.
        anchored.sort_by_key(|(idx, _)| *idx);

        // `degree` is inert for every one of the 11 harmonic kinds
        // `build_wave_primitive` ever matches this scan's own hits against
        // (`XabcdPattern`/`CypherPattern`/`AbcdPattern`/`ThreeDrives`/
        // `FiveZeroPattern` carry no `degree` field — only the Elliott
        // motive/corrective primitives do) — `WaveDegree::default()` is
        // passed only to satisfy that shared function's own signature.
        let degree = WaveDegree::default();
        for_each_cluster(ctx, &anchored, |(idx, _)| *idx, MIN_MARKER_GAP_PX, |ctx, (_, hit), count| {
            if let Some(prim) = build_wave_primitive(hit.kind, &hit.points, &self.data.color.stroke, degree) {
                prim.render(ctx, false);
            }
            if count > 1 {
                if let Some(&(ts, price)) = hit.points.last() {
                    let sx = ctx.ts_to_x_ms(ts);
                    let sy = ctx.price_to_y(price);
                    draw_count_badge(ctx, sx, sy, count, &self.data.color.stroke);
                }
            }
        });

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

    /// Recompute [`Self::cached_window`]/[`Self::cached_hits`] from `bars`
    /// through the SAME [`Self::resolved_window`]/[`Self::resolved_scan`]
    /// calls `render` makes directly — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    fn refresh_bar_derived_cache(&mut self, bars: &[Bar]) {
        self.cached_window = self.resolved_window(bars);
        self.cached_hits = self.resolved_scan(bars);
    }

    /// Drains [`Self::pending_placements`] into real, independent
    /// primitives — one per queued row, looked up in [`Self::cached_hits`]
    /// (never re-scanning: this reads the SAME freshly-resolved hit
    /// geometry `refresh_bar_derived_cache` just cached, called immediately
    /// before this by the SAME per-primitive sweep — see
    /// `mlc_backend::bubble::bar_derived_cache::refresh_bar_derived_caches`).
    fn take_scan_placements(&mut self, _bars: &[Bar]) -> Vec<Box<dyn Primitive>> {
        if self.pending_placements.is_empty() {
            return Vec::new();
        }
        let rows = std::mem::take(&mut self.pending_placements);
        let degree = WaveDegree::default(); // inert for every harmonic kind — see `render`'s own note.
        rows.into_iter()
            .filter_map(|row| self.cached_hits.get(row))
            .filter_map(|hit| build_wave_primitive(hit.kind, &hit.points, &self.data.color.stroke, degree))
            .collect()
    }

    // No `markup_family`/`context_violations` override: a SURVEY makes no
    // claim of its own (this module's own doc), matching `ElliottScan`'s
    // own choice — see the arc's own "No `MarkupFamily` change" section.

    fn style_properties(&self) -> Vec<ConfigProperty> {
        let lang = current_language();
        let kinds_head = vec![
            SettingsKey::Show.get(lang).to_string(),
            ConceptKey::Name.get(lang).to_string(),
            ConceptKey::ScanHitsColumnHeader.get(lang).to_string(),
        ];
        let hits_head = vec![
            ConceptKey::ScanHitKindColumnHeader.get(lang).to_string(),
            ConceptKey::ScanHitPivotsColumnHeader.get(lang).to_string(),
            ConceptKey::ScanHitScoreColumnHeader.get(lang).to_string(),
            ConceptKey::ScanHitPlaceColumnHeader.get(lang).to_string(),
        ];
        let bars_surveyed = self.cached_window.map(|(s, e)| e - s + 1).unwrap_or(0);

        vec![
            // Coverage/Shape — ungrouped, `AppearanceStats`, same order
            // convention `SmcScan`/`ElliottScan` already establish (coverage
            // 5, shape 7). No `degree` row here — harmonics carry no
            // `WaveDegree` concept (arc §3 never names one for this family).
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

            // `PropertyGroup::ScanWhatToDetect` — the What-to-detect kinds
            // table, the survey's PRIMARY control (same reasoning
            // `ElliottScan::style_properties` gives for its own kinds table).
            ConfigProperty::table(KINDS_TABLE_ID, ConceptKey::ScanKindsTableLabel.get(lang), kinds_head, self.kinds_table_rows())
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
            // `tolerance` — same numeric-value-is-its-own-label shape
            // `swing_n`'s own pills row uses (no discrete word tiering like
            // `min_score`'s Any/Medium/High/Exact exists for a band-widening
            // delta); `0.0` = mli's own exact default (A3.0).
            ConfigProperty::pills(
                "tolerance",
                ConceptKey::ToleranceColumnHeader.get(lang),
                &format!("{}", self.tolerance),
                vec![
                    SelectOption::new("0", "0"),
                    SelectOption::new("0.005", "0.005"),
                    SelectOption::new("0.01", "0.01"),
                    SelectOption::new("0.02", "0.02"),
                ],
            ).with_category(PropertyCategory::AppearanceStats)
                .with_order(12)
                .in_group(super::super::config::PropertyGroup::ScanThresholds),
            ConfigProperty::pills(
                "min_score",
                ConceptKey::ScanMinScoreLabel.get(lang),
                &self.min_score.to_string(),
                vec![
                    SelectOption::new("0", ConceptKey::ScanMinScoreAny.get(lang)),
                    SelectOption::new("0.5", ConceptKey::ScanMinScoreMedium.get(lang)),
                    SelectOption::new("0.7", ConceptKey::ScanMinScoreHigh.get(lang)),
                    SelectOption::new("0.9", ConceptKey::ScanMinScoreExact.get(lang)),
                ],
            ).with_category(PropertyCategory::AppearanceStats)
                .with_order(13)
                .in_group(super::super::config::PropertyGroup::ScanThresholds),

            ConfigProperty::table(HITS_TABLE_ID, ConceptKey::ScanHitsColumnHeader.get(lang), hits_head, self.hits_table_rows())
                .with_category(PropertyCategory::AppearanceStats)
                .with_order(20),

            ConfigProperty::stat_block(
                STATS_ID,
                vec![
                    StatRow::new(ConceptKey::ScanTotalHitsLabel.get(lang), &self.cached_hits.len().to_string(), None),
                    StatRow::new(ConceptKey::BarCount.get(lang), &bars_surveyed.to_string(), None),
                ],
            ).with_order(30),
        ]
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
        if id == "tolerance" {
            let Some(s) = value.as_string() else { return false };
            let Ok(v) = s.parse::<f64>() else { return false };
            self.tolerance = v.max(0.0);
            return true;
        }
        if id == "min_score" {
            let Some(s) = value.as_string() else { return false };
            let Ok(v) = s.parse::<f64>() else { return false };
            self.min_score = v;
            return true;
        }
        if let Some((base, row, col)) = parse_table_cell_id(id) {
            if base == KINDS_TABLE_ID && col == 0 {
                let Some(enabled) = value.as_bool() else { return false };
                let Some(kind) = HarmonicScanKind::ALL.get(row).copied() else { return false };
                let kind_id = kind.id().to_string();
                if enabled {
                    if !self.enabled_kinds.iter().any(|k| k == &kind_id) {
                        self.enabled_kinds.push(kind_id);
                    }
                } else {
                    self.enabled_kinds.retain(|k| k != &kind_id);
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

/// Draw a small `"×{count}"` badge above a marker — the count-badge half of
/// the proximity degrade, matching `ElliottScan::render`'s own choice to
/// print a count rather than stack markers. No-op for `count <= 1`.
fn draw_count_badge(ctx: &mut dyn RenderContext, x: f64, y: f64, count: usize, color: &str) {
    if count <= 1 {
        return;
    }
    let dpr = ctx.dpr();
    ctx.set_font(&format!("{}px sans-serif", (9.0 * dpr) as i32));
    ctx.set_text_align(crate::render::TextAlign::Center);
    ctx.set_text_baseline(crate::render::TextBaseline::Bottom);
    ctx.set_fill_color(color);
    ctx.fill_text(&format!("×{count}"), x, y - 4.0 * dpr);
}

/// Walk `items` (already sorted ascending by `anchor`), collapsing a run of
/// consecutive items whose own `anchor`-derived screen X sits within
/// `gap_px` device pixels of the run's own first member. `draw` is called
/// once per run, on the run's own FIRST item, with the run's own length as
/// `count` — mirrors `elliott_scan::ElliottScan::render`'s own private
/// `for_each_cluster` (each scanner in this crate carries its own copy,
/// tailored to its own hit shape — see that function's own doc for why
/// there is no single shared implementation to call instead).
fn for_each_cluster<T>(
    ctx: &mut dyn RenderContext,
    items: &[T],
    anchor: impl Fn(&T) -> usize,
    gap_px: f64,
    mut draw: impl FnMut(&mut dyn RenderContext, &T, usize),
) {
    let dpr = ctx.dpr();
    let mut i = 0;
    while i < items.len() {
        let start_x = ctx.bar_to_x(anchor(&items[i]) as f64);
        let mut j = i + 1;
        while j < items.len() {
            let x = ctx.bar_to_x(anchor(&items[j]) as f64);
            if (x - start_x).abs() >= gap_px * dpr {
                break;
            }
            j += 1;
        }
        draw(ctx, &items[i], j - i);
        i = j;
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "harmonic_scan",
        display_name: "Harmonics autodetector",
        kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Survey a bar range for harmonic patterns",
        icon: "harmonic_scan",
        default_color: "#9C27B0",
        factory: |points, color| {
            let (t1, _) = points.first().copied().unwrap_or((0, 0.0));
            let (t2, _) = points.get(1).copied().unwrap_or((t1 + 72_000_000, 0.0));
            Box::new(HarmonicScan::new(t1, t2, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A textbook Gartley — the SAME pivot prices `mli::ewa::scan`'s own
    /// `textbook_gartley_is_found_at_expected_pivots` fixture uses
    /// (`ewa/scan.rs`'s own test module): `X=1000 (Low), A=1100 (High,
    /// XA=100), B=1038.2 (Low, AB=61.8, ratio 0.618 exact), C=1080.2 (High,
    /// BC=42.0, ratio 0.6796, inside [0.382,0.886]), D=1021.4 (Low, CD=58.8,
    /// ratio 1.4 of BC, inside [1.13,1.618]; AD=78.6, ratio 0.786 exact)`.
    /// That helper's own `wave_bars`/`ramp`/`pivot_bar` fns are private to
    /// `mli`'s own `#[cfg(test)]` module and unreachable from this crate, so
    /// this reimplements the SAME ramp-then-pivot-bar shape locally, the
    /// same posture `elliott_scan.rs`'s own test module already takes for
    /// its impulse fixture.
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

    fn gartley_fixture_bars() -> Vec<Bar> {
        let is_high = [false, true, false, true, false];
        let prices = [1000.0, 1100.0, 1038.2, 1080.2, 1021.4];
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

    fn new_scan_over(bars: &[Bar]) -> HarmonicScan {
        let mut scan = HarmonicScan::new(0, (bars.len() as i64 - 1) * 1000, "#9C27B0");
        scan.swing_n = 2;
        scan.enabled_kinds = vec!["gartley".to_string()];
        scan.min_score = 0.0;
        scan.tolerance = 0.0;
        scan
    }

    /// Same OHLC shape [`gartley_fixture_bars`] converts to `Bar`s, kept as
    /// raw tuples so [`a_survey_over_a_long_series_still_finds_a_recent_gartley_despite_thousands_of_older_bars`]
    /// can append it AFTER a long filler series without building `Bar`s
    /// twice.
    fn gartley_fixture_ohlc() -> Vec<(f64, f64, f64, f64)> {
        let is_high = [false, true, false, true, false];
        let prices = [1000.0, 1100.0, 1038.2, 1080.2, 1021.4];
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
        ohlc
    }

    /// A long run of small alternating up/down legs — noise that generates
    /// hundreds of its own swing pivots without ever forming a valid
    /// harmonic pattern, standing in for "years of ordinary price action"
    /// ahead of a genuine pattern. `legs` * 4 bars each; `legs=700` yields
    /// ~2,800 bars — comfortably past the pivot count `mli::ewa::scan::
    /// MAX_PIVOTS_PER_SCAN` (500) caps a single scan call at, at any
    /// `swing_n` this fixture's own caller uses.
    fn filler_zigzag_ohlc(legs: usize) -> Vec<(f64, f64, f64, f64)> {
        let mut ohlc = Vec::new();
        let mut price = 500.0;
        let mut is_high = true;
        for _ in 0..legs {
            let target = if is_high { price + 10.0 } else { price - 10.0 };
            ohlc.extend(ramp(price, target, 3, 0.2));
            ohlc.push(pivot_bar(is_high, target, 0.2));
            price = target;
            is_high = !is_high;
        }
        ohlc
    }

    /// Regression fence for the owner report 2026-09-25 ("Harmonic scan
    /// doesn't work"): a genuine textbook Gartley sitting at the END of a
    /// long series (2,800 filler bars ahead of it — well past `mli::ewa::
    /// scan::MAX_PIVOTS_PER_SCAN`'s 500-pivot cap) must still be found
    /// under `coverage == "all_bars"`. Same rationale as `elliott_scan.rs`'s
    /// own `a_survey_over_a_long_series_still_finds_a_recent_impulse_
    /// despite_thousands_of_older_bars` — before [`ewa_scan_window`]
    /// bounded the input, every pivot belonging to this Gartley (all in the
    /// last ~20 bars) was discarded by `scan_waves_with_harmonic_tolerance`'s
    /// own oldest-500 truncation before `completion_range` ever got a
    /// chance to select it.
    #[test]
    fn a_survey_over_a_long_series_still_finds_a_recent_gartley_despite_thousands_of_older_bars() {
        let mut ohlc = filler_zigzag_ohlc(700);
        let gartley_start_bar = ohlc.len();
        ohlc.extend(gartley_fixture_ohlc());
        let bars: Vec<Bar> = ohlc.into_iter().enumerate().map(|(i, (o, h, l, c))| Bar::new(i as i64, o, h, l, c)).collect();

        let mut scan = HarmonicScan::new(0, 0, "#9C27B0");
        scan.coverage = "all_bars".to_string();
        scan.swing_n = 2;
        scan.enabled_kinds = vec!["gartley".to_string()];
        scan.min_score = 0.0;
        scan.tolerance = 0.0;

        let hits = scan.resolved_scan(&bars);
        assert!(
            !hits.is_empty(),
            "a textbook Gartley at the end of a {}-bar series was not found — \
             the lookback-window fix did not reach HarmonicScan::resolved_scan",
            bars.len()
        );
        let last_pivot_ts = hits[0].points.last().map(|&(ts, _)| ts).unwrap_or(0);
        let last_pivot_bar = (last_pivot_ts / 1000) as usize;
        assert!(
            last_pivot_bar >= gartley_start_bar,
            "the hit found (last pivot at bar {last_pivot_bar}) is not the appended \
             Gartley (which starts at bar {gartley_start_bar}) — filler noise must not \
             score as a false positive here"
        );
    }

    #[test]
    fn resolved_scan_identity_against_a_direct_mli_call_over_a_textbook_gartley() {
        let bars = gartley_fixture_bars();
        let scan = new_scan_over(&bars);
        let hits = scan.resolved_scan(&bars);

        let ohlc: Vec<(f64, f64, f64, f64)> = bars.iter().map(|b| (b.open, b.high, b.low, b.close)).collect();
        let pivots = mli::ewa::swing::extract_pivots(&ohlc, 2);
        let segments = mli::ewa::swing::build_segments(&ohlc, &pivots);
        let direct = mli::ewa::scan::scan_waves_with_harmonic_tolerance(
            &pivots, &segments, &[mli::ewa::types::EwaPatternKind::Gartley], 0..bars.len(), 0.0,
        );

        assert!(!direct.is_empty(), "fixture must actually contain a textbook Gartley");
        assert_eq!(hits.len(), direct.len());
        assert_eq!(hits[0].kind, mli::ewa::types::EwaPatternKind::Gartley);
        assert_eq!(hits[0].points.len(), direct[0].pivots.len());
    }

    /// A2/A3 proof requested by the coordinator: raising `tolerance` finds a
    /// candidate the default (narrower) tolerance misses — the exact "not a
    /// dead field" check A3.0's own mli change exists to make possible. The
    /// SAME pivot prices `mli::ewa::scan`'s own
    /// `wider_harmonic_tolerance_finds_a_near_miss_the_default_rejects`
    /// fixture uses: `AB = 62.8` (ratio 0.628 — just outside Gartley's own
    /// `exact(0.618)` band `[0.613, 0.623]` but inside a `0.02`-widened one
    /// `[0.598, 0.643]`); `AD` stays exactly on `exact(0.786)` so only the
    /// `AB` leg gates this fixture.
    #[test]
    fn tolerance_really_changes_hits() {
        let is_high = [false, true, false, true, false];
        let prices = [1000.0, 1100.0, 1037.2, 1081.16, 1021.4];
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
        let bars: Vec<Bar> = ohlc.into_iter().enumerate().map(|(i, (o, h, l, c))| Bar::new(i as i64, o, h, l, c)).collect();

        let mut narrow = new_scan_over(&bars);
        narrow.tolerance = 0.0;
        assert!(narrow.resolved_scan(&bars).is_empty(), "default-tolerance scan must miss this near-miss Gartley");

        let mut wide = new_scan_over(&bars);
        wide.tolerance = 0.02;
        assert!(!wide.resolved_scan(&bars).is_empty(), "widened-tolerance scan must find the SAME shape the narrow scan misses");
    }

    #[test]
    fn min_score_filter_drops_a_hit_below_threshold() {
        let bars = gartley_fixture_bars();
        let mut scan = new_scan_over(&bars);

        let hits = scan.resolved_scan(&bars);
        assert!(!hits.is_empty(), "textbook Gartley must be found at min_score 0.0");

        scan.min_score = 1.01; // above any achievable score (score is 0..=1)
        let filtered = scan.resolved_scan(&bars);
        assert!(filtered.is_empty(), "min_score above the hit's own score must drop it");
    }

    #[test]
    fn unchecking_a_kind_removes_its_hits() {
        let bars = gartley_fixture_bars();
        let mut scan = new_scan_over(&bars);
        assert!(!scan.resolved_scan(&bars).is_empty(), "Gartley must be found while enabled");

        scan.enabled_kinds.clear();
        assert!(scan.resolved_scan(&bars).is_empty(), "no enabled kinds must survey nothing");
    }

    #[test]
    fn render_cache_is_absent_from_to_json() {
        let bars = gartley_fixture_bars();
        let mut scan = new_scan_over(&bars);
        scan.refresh_bar_derived_cache(&bars);
        assert!(!scan.cached_hits.is_empty());

        let json = scan.to_json();
        assert!(!json.contains("cached_window"), "{json}");
        assert!(!json.contains("cached_hits"), "{json}");
        assert!(!json.contains("pending_placements"), "{json}");

        let mut registry = super::super::super::registry::PrimitiveRegistry::new();
        registry.register_typed::<HarmonicScan>(metadata());
        let restored = registry.from_json("harmonic_scan", &json).expect("round trip");
        assert_eq!(restored.points(), scan.points());
    }

    #[test]
    fn rectangle_shape_price_band_filters_a_hit() {
        let bars = gartley_fixture_bars();
        let mut scan = new_scan_over(&bars);
        let unfiltered = scan.resolved_scan(&bars);
        assert!(!unfiltered.is_empty());
        let representative_price = unfiltered[0].points.last().expect("hit has pivots").1;

        scan.shape = ScanShape::Rectangle;
        scan.p1 = representative_price + 1000.0;
        scan.p2 = representative_price + 2000.0; // band far away from the hit's own price
        assert!(scan.resolved_scan(&bars).is_empty(), "a price band that excludes the hit must drop it");
    }

    #[test]
    fn kinds_table_cell_click_toggles_enabled_kinds() {
        let mut scan = HarmonicScan::new(0, 13_000, "#9C27B0");
        let row = HarmonicScanKind::ALL.iter().position(|k| k.id() == "gartley").expect("catalogue has it");
        let cell_id = format!("{KINDS_TABLE_ID}::cell:{row}:0");

        assert!(scan.apply_style_property(&cell_id, &PropertyValue::Boolean(false)));
        assert!(!scan.enabled_kinds.iter().any(|k| k == "gartley"));

        assert!(scan.apply_style_property(&cell_id, &PropertyValue::Boolean(true)));
        assert!(scan.enabled_kinds.iter().any(|k| k == "gartley"));
    }

    #[test]
    fn place_click_then_take_scan_placements_yields_a_matching_xabcd_pattern() {
        let bars = gartley_fixture_bars();
        let mut scan = new_scan_over(&bars);
        scan.refresh_bar_derived_cache(&bars);
        assert!(!scan.cached_hits.is_empty(), "textbook Gartley must be cached");

        let cell_id = format!("{HITS_TABLE_ID}::cell:0:{PLACE_COL}");
        assert!(scan.apply_style_property(&cell_id, &PropertyValue::Boolean(true)));

        let placed = scan.take_scan_placements(&bars);
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].type_id(), "xabcd_pattern");
        assert_eq!(placed[0].points(), scan.cached_hits[0].points);

        // The queue does not refill itself.
        let placed_again = scan.take_scan_placements(&bars);
        assert!(placed_again.is_empty());
    }

    #[test]
    fn shape_and_coverage_properties_round_trip() {
        let mut scan = HarmonicScan::new(0, 13_000, "#9C27B0");
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
}
