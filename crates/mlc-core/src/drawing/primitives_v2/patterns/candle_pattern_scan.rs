//! Candle Pattern Scan — the SURVEY half of the candle-pattern markup
//! engine (`docs/mlc/plans/markup-engines-2026-08-25.md`, "the scan is a
//! SURVEY: the bars say what they contain").
//!
//! Two points bracket a bar RANGE. `refresh_bar_derived_cache` resolves
//! that range to bar indices via the shared
//! [`super::super::bar_window::window_indices`] helper and hands the
//! enabled kinds to `mli::events::candle_pattern::scan` — the SAME free
//! function [`super::candle_pattern::CandlePattern`]'s own single-bar claim
//! calls through `detect_at`. The hits are a RENDER CACHE: never
//! persisted, never in `to_json`, recomputed from bars on load — see
//! [`Primitive::refresh_bar_derived_cache`]'s own contract.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{current_language, ConceptKey, SettingsKey};
use mli::events::candle_pattern::{CandlePatternKind, CandlePatternHit, scan};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, render_primitive_text, TextAlign,
    config::{ConfigProperty, SelectOption, PropertyCategory, PropertyValue, StatRow, TableCell, parse_table_cell_id},
};
use super::candle_pattern::{sorted_kinds, pattern_display_name, pattern_short_display_name};
use super::super::scan_shape::{ScanShape, render_scan_bracket, price_band_filter};

/// Default up/down marker colours, matching the chart's own default theme
/// preset (`#26a69a` / `#ef5350`) — the same literal pair
/// `projection::bars_pattern::PROJECTED_UP_COLOR`/`PROJECTED_DOWN_COLOR`
/// use, for the same reason (no live `ThemeManager` reachable from a
/// primitive's own `render`).
const UP_COLOR: &str = "#26a69a";
const DOWN_COLOR: &str = "#ef5350";

/// The scanner family's shared collapse spacing — see
/// [`super::super::SCAN_MARKER_MIN_GAP_PX`], which every scanner reads so
/// that "degrade honestly rather than overprint" means one behaviour on the
/// chart rather than one per file.
use super::super::SCAN_MARKER_MIN_GAP_PX as MIN_MARKER_GAP_PX;

/// The `candle_pattern_scan_kinds` table's own id — shared between
/// [`CandlePatternScan::style_properties`] (which declares it) and
/// [`CandlePatternScan::apply_style_property`] (which parses a cell click
/// back via [`parse_table_cell_id`]).
const CANDLE_PATTERN_SCAN_TABLE_ID: &str = "candle_pattern_scan_kinds";
/// The read-only totals Stat block's own id — same reject-arm role
/// `CANDLE_PATTERN_RESULT_STATS_ID` plays for `CandlePattern`.
const CANDLE_PATTERN_SCAN_STATS_ID: &str = "candle_pattern_scan_stats";

/// A cluster's own resolved colour source — kept as a tag rather than a
/// borrowed `&str` while `bars`/`hits` are still borrowed from `ctx` (see
/// [`CandlePatternScan::render`]'s own doc for why the two-pass split
/// exists at all), resolved to an actual colour string only in the second,
/// borrow-free pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MarkerTint { Up, Down, Neutral }

/// This tint's own resolved colour — `stroke` is the scan's own line
/// colour, used for a `Neutral` hit (no live `ThemeManager` reachable from
/// a primitive's own `render`, same reasoning [`UP_COLOR`]/[`DOWN_COLOR`]
/// carry).
fn tint_color<'a>(tint: MarkerTint, stroke: &'a str) -> &'a str {
    match tint {
        MarkerTint::Up => UP_COLOR,
        MarkerTint::Down => DOWN_COLOR,
        MarkerTint::Neutral => stroke,
    }
}

/// Which side of the highlighted bar range a hit's own label sits on — the
/// charting convention: a bullish pattern forms at a low, so its label sits
/// BELOW the bars; bearish and neutral ones sit ABOVE (coordinator
/// correction of the 0.5.615 brief, which had it inverted).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LabelSide { Above, Below }

fn label_side_for(tint: MarkerTint) -> LabelSide {
    match tint {
        MarkerTint::Up => LabelSide::Below,
        MarkerTint::Down | MarkerTint::Neutral => LabelSide::Above,
    }
}

/// Below this many device pixels of bar spacing, [`render_hit_marks`] stops
/// drawing label TEXT altogether — a zoomed-out chart has no room for it
/// and every hit would just overprint its neighbours. The highlight
/// rectangle and dot marker still draw at any zoom (the "small marker"
/// the owner's own spec names as the degrade). CSS px, scaled by `dpr` the
/// same way [`MIN_MARKER_GAP_PX`] already is.
const MIN_BAR_SPACING_FOR_LABELS_PX: f64 = 14.0;
/// The dot marker's own distance from the highlighted range's outer edge
/// (top edge for `Above`, bottom edge for `Below`). CSS px, scaled by
/// `dpr`.
const DOT_GAP_PX: f64 = 6.0;
/// The label text's own distance from the highlighted range's outer edge —
/// deliberately beyond [`DOT_GAP_PX`] so the label never sits on top of
/// the dot. CSS px, scaled by `dpr`.
const LABEL_GAP_PX: f64 = 15.0;
/// Horizontal breathing room folded into a label's own collision box on
/// each side of its measured text width. CSS px, scaled by `dpr`.
const LABEL_PAD_PX: f64 = 4.0;
/// The label text's own font size. CSS px, scaled by `dpr`.
const LABEL_FONT_PX: f64 = 10.0;

/// `true` once there is enough room per bar to draw label text at all —
/// see [`MIN_BAR_SPACING_FOR_LABELS_PX`]'s own doc.
fn labels_visible_at(bar_spacing_px: f64, dpr: f64) -> bool {
    bar_spacing_px >= MIN_BAR_SPACING_FOR_LABELS_PX * dpr
}

/// One completion hit reduced to pure screen geometry — no `RenderContext`
/// of its own, so every pass below this point is unit-testable without a
/// render backend. Built by `render()` from a `CandlePatternHit` plus
/// already-resolved `ctx.bar_to_x`/`ctx.price_to_y` calls.
#[derive(Clone, Copy, Debug, PartialEq)]
struct HitMark {
    /// Screen X of the completion bar (`ctx.bar_to_x(hit.end)`) — hits
    /// arrive already ascending by `end` ([`CandlePatternScan::resolved_scan`]'s
    /// own doc), so every clustering pass below is a single forward walk.
    x: f64,
    /// The highlight rectangle's own screen extent — the window's bars'
    /// own high/low, already converted to Y, with the window's own
    /// `[start, end]` bar span (plus half a bar either side) already
    /// converted to X.
    hi_x1: f64,
    hi_x2: f64,
    hi_y1: f64,
    hi_y2: f64,
    tint: MarkerTint,
    kind: CandlePatternKind,
}

/// One Stage-1 cluster: every [`HitMark`] within [`MIN_MARKER_GAP_PX`] of
/// the cluster's own first member, reduced to the union of their own
/// highlight extents — see [`build_clusters`].
#[derive(Clone, Copy, Debug, PartialEq)]
struct ScanCluster {
    anchor_x: f64,
    hi_x1: f64,
    hi_x2: f64,
    hi_y1: f64,
    hi_y2: f64,
    tint: MarkerTint,
    /// The cluster's own FIRST member's kind — meaningful only while
    /// `count == 1`; a cluster with more members never reads this (see
    /// [`render_hit_marks`]'s own draw step).
    kind: CandlePatternKind,
    count: usize,
}

/// One label candidate for the Stage-2 width-aware collision pass — see
/// [`merge_overlapping_label_boxes`].
#[derive(Clone, Copy, Debug, PartialEq)]
struct LabelBox {
    center_x: f64,
    half_width: f64,
    label_y: f64,
    tint: MarkerTint,
    kind: CandlePatternKind,
    count: usize,
}

/// Collapse a run of consecutive `items` (already ascending by `x(item)`)
/// into ONE cluster wherever consecutive members sit within `gap_px` of
/// the run's own FIRST member — the anchored-to-first rule
/// `smc::smc_scan::for_each_cluster` already applies to its own marker
/// dots (shared discipline: one collapse rule for "too dense, group it",
/// not a second one invented here). Returns index ranges into `items`
/// (`start..start+len`) so the caller decides what a cluster's own
/// representative value is.
fn cluster_indices_by_gap<T>(items: &[T], x: impl Fn(&T) -> f64, gap_px: f64) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < items.len() {
        let start_x = x(&items[i]);
        let mut j = i + 1;
        while j < items.len() && (x(&items[j]) - start_x).abs() < gap_px {
            j += 1;
        }
        out.push((i, j - i));
        i = j;
    }
    out
}

/// [`HitMark`]s (already ascending by `x`) reduced to [`ScanCluster`]s via
/// [`cluster_indices_by_gap`] — pure geometry, no `RenderContext`, so this
/// is the same function both `render()` (real hits) and this module's own
/// tests (synthetic hits) call.
fn build_clusters(marks: &[HitMark], gap_px: f64) -> Vec<ScanCluster> {
    cluster_indices_by_gap(marks, |m| m.x, gap_px)
        .into_iter()
        .map(|(start, len)| {
            let group = &marks[start..start + len];
            ScanCluster {
                anchor_x: group[0].x,
                hi_x1: group.iter().fold(f64::INFINITY, |m, g| m.min(g.hi_x1)),
                hi_x2: group.iter().fold(f64::NEG_INFINITY, |m, g| m.max(g.hi_x2)),
                hi_y1: group.iter().fold(f64::INFINITY, |m, g| m.min(g.hi_y1)),
                hi_y2: group.iter().fold(f64::NEG_INFINITY, |m, g| m.max(g.hi_y2)),
                tint: group[0].tint,
                kind: group[0].kind,
                count: len,
            }
        })
        .collect()
}

/// Merge adjacent same-side label boxes whose measured screen extents
/// overlap, guaranteeing the RETURNED boxes never overlap on X — a
/// classic left-to-right interval merge (running max of the right edge).
/// Deliberately NOT [`cluster_indices_by_gap`]'s own "anchored to the
/// run's first member" rule: that rule exists to stop a long line of
/// *tight* dot clusters drifting away from their own true position: here
/// the only goal is the hard "labels never overlap" contract, which a
/// running-max interval merge gives for free. `boxes` must already be
/// sorted ascending by `center_x` (every caller builds them from
/// [`ScanCluster`]s, themselves built from hits ascending by completion
/// bar index).
///
/// Measuring collision against each box's own SPECIFIC-name width (rather
/// than the shorter generic "N patterns" text a merged box ends up
/// drawing) is deliberately conservative, not a bug: shrinking the drawn
/// text after a merge can only shrink the box, never grow it, so a merge
/// decision made on the wider pre-merge width can never UNDER-detect a
/// real overlap.
fn merge_overlapping_label_boxes(boxes: &[LabelBox], side: LabelSide) -> Vec<LabelBox> {
    let mut out: Vec<LabelBox> = Vec::new();
    for b in boxes {
        let (left, right) = (b.center_x - b.half_width, b.center_x + b.half_width);
        if let Some(last) = out.last_mut() {
            let last_right = last.center_x + last.half_width;
            if left < last_right {
                let last_left = last.center_x - last.half_width;
                let new_left = last_left.min(left);
                let new_right = last_right.max(right);
                last.center_x = (new_left + new_right) / 2.0;
                last.half_width = (new_right - new_left) / 2.0;
                last.count += b.count;
                last.label_y = match side {
                    // Extend outward (away from the bars) to clear every
                    // absorbed member's own highlight, never inward.
                    LabelSide::Above => last.label_y.min(b.label_y),
                    LabelSide::Below => last.label_y.max(b.label_y),
                };
                continue;
            }
        }
        out.push(*b);
    }
    out
}

/// Shift `center` only as far as needed so `[center - half_extent, center
/// + half_extent]` fits inside `[min, max]` — the "never overlap the price
/// axis" clamp (`max` is `ctx.chart_width()`, which already excludes the
/// axis gutter — see [`RenderContext::chart_width`]'s own doc) and the
/// chart's own top/bottom edges. A box wider than `[min, max]` itself
/// cannot be made to fit either side at once; this clamps to the `max`
/// edge in that case (arbitrary but deterministic — there is no correct
/// answer when the box is simply too wide for the chart).
fn clamp_within(center: f64, half_extent: f64, min: f64, max: f64) -> f64 {
    let mut c = center;
    if c + half_extent > max {
        c = max - half_extent;
    }
    if c - half_extent < min {
        c = min + half_extent;
    }
    c
}

/// Cluster + draw every hit on ONE side (`Above` for bullish hits, `Below`
/// for bearish/neutral — [`label_side_for`]). The highlight rectangle and
/// dot marker draw once per Stage-1 cluster ([`build_clusters`], the SAME
/// proximity collapse the marker dots always used) at ANY zoom — the
/// "small marker" [`MIN_BAR_SPACING_FOR_LABELS_PX`]'s own doc names as the
/// degrade. The label text on top additionally collapses same-side
/// clusters whose own MEASURED width would overlap
/// ([`merge_overlapping_label_boxes`]), and is skipped entirely below
/// that same bar-spacing floor.
fn render_hit_marks(
    ctx: &mut dyn RenderContext,
    marks: &[HitMark],
    side: LabelSide,
    stroke_color: &str,
    show_labels: bool,
    chart_width: f64,
    dpr: f64,
) {
    if marks.is_empty() {
        return;
    }
    let clusters = build_clusters(marks, MIN_MARKER_GAP_PX * dpr);

    for c in &clusters {
        let color = tint_color(c.tint, stroke_color);
        ctx.set_fill_color(&format!("{color}22"));
        ctx.fill_rect(c.hi_x1, c.hi_y1, c.hi_x2 - c.hi_x1, c.hi_y2 - c.hi_y1);
        ctx.set_stroke_color(color);
        ctx.set_stroke_width(1.0);
        ctx.set_line_dash(&[]);
        ctx.stroke_rect(crisp(c.hi_x1, dpr), crisp(c.hi_y1, dpr), c.hi_x2 - c.hi_x1, c.hi_y2 - c.hi_y1);

        let dot_y = match side {
            LabelSide::Above => c.hi_y1 - DOT_GAP_PX * dpr,
            LabelSide::Below => c.hi_y2 + DOT_GAP_PX * dpr,
        };
        ctx.set_fill_color(color);
        ctx.begin_path();
        ctx.arc(c.anchor_x, dot_y, 2.5 * dpr, 0.0, std::f64::consts::TAU);
        ctx.fill();
    }

    if !show_labels {
        return;
    }

    ctx.set_font(&format!("{}px sans-serif", (LABEL_FONT_PX * dpr) as i32));
    let boxes: Vec<LabelBox> = clusters
        .iter()
        .map(|c| {
            let label_y = match side {
                LabelSide::Above => c.hi_y1 - LABEL_GAP_PX * dpr,
                LabelSide::Below => c.hi_y2 + LABEL_GAP_PX * dpr,
            };
            let text = pattern_short_display_name(c.kind);
            let half_width = ctx.measure_text(text) / 2.0 + LABEL_PAD_PX * dpr;
            LabelBox { center_x: c.anchor_x, half_width, label_y, tint: c.tint, kind: c.kind, count: c.count }
        })
        .collect();
    let merged = merge_overlapping_label_boxes(&boxes, side);

    let lang = current_language();
    ctx.set_text_align(crate::render::TextAlign::Center);
    ctx.set_text_baseline(match side {
        LabelSide::Above => crate::render::TextBaseline::Bottom,
        LabelSide::Below => crate::render::TextBaseline::Top,
    });
    for b in &merged {
        let x = clamp_within(b.center_x, b.half_width, 0.0, chart_width);
        let color = tint_color(b.tint, stroke_color);
        let text = if b.count == 1 {
            pattern_short_display_name(b.kind).to_string()
        } else {
            format!("{} {}", b.count, ConceptKey::ScanClusterPatternsSuffix.get(lang))
        };
        ctx.set_fill_color(color);
        ctx.fill_text(&text, x, b.label_y);
    }
}

fn default_all_kind_ids() -> Vec<String> {
    CandlePatternKind::all().iter().map(|k| k.id().to_string()).collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CandlePatternScan {
    pub data: PrimitiveData,
    #[serde(default)] pub ts1: i64,
    #[serde(default)] pub ts2: i64,
    /// Two full-height vertical brackets (default) or a bounded rectangle
    /// over `[ts1, ts2] x [p1, p2]` — see
    /// [`crate::drawing::primitives_v2::scan_shape::ScanShape`]'s own doc.
    /// `#[serde(default)]` so a scan saved before this field existed loads
    /// as `TwoLines`, byte-for-byte the shape it always drew.
    #[serde(default)]
    pub shape: ScanShape,
    /// The price half of this survey's own two control points — see
    /// `smc::smc_scan::SmcScan::p1`'s own doc for the identical contract.
    #[serde(default)] pub p1: f64,
    #[serde(default)] pub p2: f64,
    /// Whether this survey covers the range its own two points bracket, or
    /// every loaded bar. `"range"` (default) or `"all_bars"`.
    ///
    /// `"all_bars"` is the automatic mode: the two points stay as the
    /// primitive's own handles and still mark the series' ends on screen,
    /// but the detectors are handed the whole loaded series and the survey
    /// re-resolves as bars arrive, so it keeps marking new structure
    /// without being redrawn.
    #[serde(default = "default_coverage")]
    pub coverage: String,
    /// `mli::events::candle_pattern::CandlePatternKind::id()` wire ids —
    /// serialized as ids (never as an enum/index) so a saved drawing
    /// survives a catalogue that grows. `#[serde(default)]` (all 34,
    /// [`default_all_kind_ids`]) covers a JSON missing this field entirely
    /// — never read from the live engine config at deserialize time, which
    /// would make an old saved scan silently start tracking a moving
    /// target; only [`CandlePatternScan::new`] reads that config, and only
    /// once, at construction (see that method's own doc).
    #[serde(default = "default_all_kind_ids")]
    pub enabled_kinds: Vec<String>,

    /// This scan's own resolved `[start, end]` bar-index window — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract. Filled by
    /// that hook from the SAME [`super::super::bar_window::window_indices`]
    /// call [`CandlePatternScan::resolved_scan`] makes internally; read
    /// back only by [`CandlePatternScan::style_properties`]'s own "bars
    /// surveyed" Stat row (a `&self`-only reader with no `bars` of its
    /// own).
    ///
    /// `#[serde(skip)]`: a stale window surviving a reload/undo would
    /// misreport a range that no longer matches this primitive's own
    /// current `[ts1, ts2]`.
    #[serde(skip)]
    pub cached_window: Option<(usize, usize)>,
    /// The hits `mli::events::candle_pattern::scan` finds inside
    /// [`Self::cached_window`], for the enabled kinds — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract and this
    /// module's own doc. `render` recomputes this fresh via
    /// [`CandlePatternScan::resolved_scan`] and never reads this field —
    /// this cache exists only for [`CandlePatternScan::style_properties`]'s
    /// own per-kind hit-count column and total-hits Stat row.
    #[serde(skip)]
    pub cached_hits: Vec<CandlePatternHit>,
}


/// Every survey's own default coverage — the range its two points bracket.
/// A survey placed by dragging a range should survey that range; automatic
/// coverage is a deliberate switch, not what a drag silently means.
fn default_coverage() -> String {
    "range".to_string()
}

impl CandlePatternScan {
    pub fn new(ts1: i64, ts2: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "candle_pattern_scan".to_string(),
                display_name: "Candle Pattern Scan".to_string(),
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
            // Read ONCE at construction — see `CandlePatternEngineConfig::
            // default_enabled_kinds`'s own doc; never retro-applied to a
            // scan that already exists.
            enabled_kinds: crate::state::candle_pattern_engine::candle_pattern_engine_config()
                .default_enabled_kinds,
            cached_window: None,
            cached_hits: Vec::new(),
        }
    }

    /// The `[start, end]` bar-index window this survey covers — the shared
    /// two-endpoint span resolver every sibling in this family
    /// (`FixedVwap`, `FixedVolumeProfile`, `BarsPattern`) now shares. `None`
    /// when the span holds no bar.
    fn resolved_window(&self, bars: &[Bar]) -> Option<(usize, usize)> {
        // Automatic coverage: every loaded bar, so the survey keeps
        // marking new structure as bars arrive instead of staying inside
        // whatever range was dragged once.
        if self.coverage == "all_bars" {
            return (!bars.is_empty()).then(|| (0, bars.len() - 1));
        }
        super::super::bar_window::window_indices(bars, self.ts1, self.ts2)
    }

    /// This survey's own hits — `mli::events::candle_pattern::scan` run
    /// over the WHOLE `bars` array (never just the window's own slice: a
    /// 5-bar pattern completing at the window's own start legitimately
    /// reads four bars before it — see `scan`'s own doc), bounded to
    /// [`Self::resolved_window`]'s own completion-index range. `render`
    /// and [`Primitive::refresh_bar_derived_cache`] both call this directly
    /// — the ONE place this crate calls `scan`, so a rendered marker and a
    /// cached hit-count can never drift apart.
    pub fn resolved_scan(&self, bars: &[Bar]) -> Vec<CandlePatternHit> {
        let Some((start, end)) = self.resolved_window(bars) else { return Vec::new() };
        let kinds: Vec<CandlePatternKind> =
            self.enabled_kinds.iter().filter_map(|id| CandlePatternKind::from_id(id)).collect();
        if kinds.is_empty() {
            return Vec::new();
        }
        let ohlc: Vec<(f64, f64, f64, f64)> = bars.iter().map(|b| (b.open, b.high, b.low, b.close)).collect();
        scan(&kinds, &ohlc, start..end + 1)
            .into_iter()
            // `Rectangle` mode only (see `price_band_filter`'s own doc) —
            // the completion bar's own midpoint is this survey's ONE
            // representative price per hit; `TwoLines` is a no-op
            // passthrough, so this changes nothing for a scan that never
            // opted into a price band.
            .filter(|h| {
                let mid = bars.get(h.end).map(|b| (b.high + b.low) / 2.0).unwrap_or(f64::NAN);
                price_band_filter(self.shape, self.p1, self.p2, mid)
            })
            .collect()
    }

    /// The `candle_pattern_scan_kinds` table's own rows — one per catalogue
    /// pattern, in [`sorted_kinds`]'s own order (the SAME order
    /// [`CandlePatternScan::apply_style_property`] indexes back into via
    /// `row`). Hit counts read [`Self::cached_hits`] — never a second
    /// `resolved_scan` call from a method with no `bars` of its own.
    fn kind_table_rows(&self) -> Vec<Vec<TableCell>> {
        sorted_kinds()
            .into_iter()
            .map(|k| {
                let enabled = self.enabled_kinds.iter().any(|id| id == k.id());
                let count = self.cached_hits.iter().filter(|h| h.kind == k).count();
                vec![
                    TableCell::EditableCheck(enabled),
                    TableCell::Text(pattern_display_name(k).to_string()),
                    TableCell::Text(count.to_string()),
                ]
            })
            .collect()
    }
}

impl Primitive for CandlePatternScan {
    fn type_id(&self) -> &'static str { "candle_pattern_scan" }
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

    /// The two boundaries, then a full-height body column between them —
    /// same "the body is a TIME range" shape `BarsPattern::hit_test` and
    /// `CandlePattern::hit_test` both already use.
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

    /// The two dashed boundaries (dashed, not solid — visually distinct
    /// from `CandlePattern`'s own solid claim bracket, so a survey and a
    /// claim never read as the same instrument), then every hit drawn as a
    /// soft, tinted highlight over its own bars plus a short readable
    /// label — the owner's own 0.5.615 spec ("just marks bars with some
    /// unclear dots" → make hits readable without opening settings).
    /// Computed FRESH from `bars` here via [`Self::resolved_scan`] — never
    /// reads [`Self::cached_hits`] — matching
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    ///
    /// Layout: bullish hits label BELOW their own bars, bearish/neutral
    /// hits ABOVE ([`label_side_for`]); dense neighbours collapse first by
    /// dot proximity ([`build_clusters`], any zoom) and then, additionally,
    /// by measured label width ([`merge_overlapping_label_boxes`]) — a
    /// collapsed cluster shows a generic "N patterns" count rather than one
    /// member's own name, since (unlike `SmcScan`'s same-structure
    /// clusters) a run of candlestick hits can mix different pattern
    /// kinds. Label text itself is skipped below
    /// [`MIN_BAR_SPACING_FOR_LABELS_PX`] — the highlight + dot still draw.
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        // In automatic coverage the two markers sit on the series' own
        // ends rather than where the range was dragged — they still mark
        // exactly what is being surveyed.
        let (x1, x2) = match self.resolved_window(ctx.bars()) {
            Some((s, e)) if self.coverage == "all_bars" => (ctx.bar_to_x(s as f64), ctx.bar_to_x(e as f64)),
            _ => (ctx.ts_to_x_ms(self.ts1), ctx.ts_to_x_ms(self.ts2)),
        };
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        let chart_height = ctx.chart_height();
        let chart_width = ctx.chart_width();

        render_scan_bracket(ctx, self.shape, x1, x2, self.p1, self.p2, &self.data.color.stroke, self.data.width);

        let half_bar = ((ctx.bar_to_x(1.0) - ctx.bar_to_x(0.0)) / 2.0).abs();
        let bar_spacing = half_bar * 2.0;

        // Collect every hit's own resolved screen geometry FIRST, while
        // `ctx.bars()`'s own borrow is still live (every call here is
        // `&self`) — then cluster/draw from that owned list in a second,
        // borrow-free pass, so the mutable calls below (`set_fill_color`,
        // `fill_rect`, …) never conflict with it.
        let (above, below): (Vec<HitMark>, Vec<HitMark>) = {
            let bars = ctx.bars();
            let hits = self.resolved_scan(bars);
            let pad = 3.0 * dpr;
            let mut above = Vec::new();
            let mut below = Vec::new();
            for hit in &hits {
                let Some(window) = bars.get(hit.start..=hit.end).filter(|w| !w.is_empty()) else { continue };
                let hi = window.iter().fold(f64::NEG_INFINITY, |m, b| m.max(b.high));
                let lo = window.iter().fold(f64::INFINITY, |m, b| m.min(b.low));
                let tint = match hit.direction {
                    mli::Direction::Up => MarkerTint::Up,
                    mli::Direction::Down => MarkerTint::Down,
                    mli::Direction::Neutral => MarkerTint::Neutral,
                };
                let mark = HitMark {
                    x: ctx.bar_to_x(hit.end as f64),
                    hi_x1: ctx.bar_to_x(hit.start as f64) - half_bar,
                    hi_x2: ctx.bar_to_x(hit.end as f64) + half_bar,
                    hi_y1: ctx.price_to_y(hi) - pad,
                    hi_y2: ctx.price_to_y(lo) + pad,
                    tint,
                    kind: hit.kind,
                };
                match label_side_for(tint) {
                    LabelSide::Above => above.push(mark),
                    LabelSide::Below => below.push(mark),
                }
            }
            (above, below)
        };

        let stroke_color = self.data.color.stroke.clone();
        let show_labels = labels_visible_at(bar_spacing, dpr);
        render_hit_marks(ctx, &above, LabelSide::Above, &stroke_color, show_labels, chart_width, dpr);
        render_hit_marks(ctx, &below, LabelSide::Below, &stroke_color, show_labels, chart_width, dpr);

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
    /// A survey makes no claim of its own (see this module's own doc), so
    /// it has nothing a neighbour could contradict — it declares its
    /// family for grouping but keeps the default no-op
    /// `push_context_violations`.
    fn markup_family(&self) -> Option<super::super::markup::MarkupFamily> {
        Some(super::super::markup::MarkupFamily::CandlePattern)
    }

    fn refresh_bar_derived_cache(&mut self, bars: &[Bar]) {
        self.cached_window = self.resolved_window(bars);
        self.cached_hits = self.resolved_scan(bars);
    }

    /// Same measured verdict `CandlePattern::style_properties` already
    /// records: no design-approved archetype names a candle-pattern-scan
    /// subject tab yet, so the table and the Stat block below stay in
    /// their own constructors' default category/run.
    fn style_properties(&self) -> Vec<ConfigProperty> {
        let lang = current_language();
        let head = vec![
            SettingsKey::Show.get(lang).to_string(),
            ConceptKey::Name.get(lang).to_string(),
            ConceptKey::ScanHitsColumnHeader.get(lang).to_string(),
        ];
        let bars_surveyed = self.cached_window.map(|(s, e)| e - s + 1).unwrap_or(0);
        vec![
            // Coverage first: it decides what the table below is counting.
            ConfigProperty::pills(
                "coverage",
                ConceptKey::ScanCoverageLabel.get(lang),
                &self.coverage,
                vec![
                    SelectOption::new("range", ConceptKey::ScanCoverageRange.get(lang)),
                    SelectOption::new("all_bars", ConceptKey::ScanCoverageAllBars.get(lang)),
                ],
            ).with_category(PropertyCategory::AppearanceStats).with_order(5),
            // The shape a placed survey is bracketed by — see
            // `smc::smc_scan::SmcScan::style_properties`'s own identical row
            // for the full rationale.
            ConfigProperty::pills(
                "shape",
                ConceptKey::ScanShapeLabel.get(lang),
                self.shape.id(),
                vec![
                    SelectOption::new(ScanShape::TwoLines.id(), ConceptKey::ScanShapeTwoLines.get(lang)),
                    SelectOption::new(ScanShape::Rectangle.id(), ConceptKey::ScanShapeRectangle.get(lang)),
                ],
            ).with_category(PropertyCategory::AppearanceStats).with_order(7),
            // The structure table is this survey's PRIMARY control, not a
            // label setting. `ConfigProperty::table`'s own default category
            // is `LabelsText`, which files it under the Labels tab: measured
            // on the live binary, the first tab then carried nothing but
            // colour/width/style while the list of what to scan for was a
            // tab away. `AppearanceStats` puts it in its own headed group on
            // the main tab, ahead of the computed results below it (the
            // category decides placement only — the renderer checks
            // `PropertyType::Table` before the `readonly` flag this
            // constructor already sets, so the checkboxes stay editable).
            ConfigProperty::table(
                CANDLE_PATTERN_SCAN_TABLE_ID,
                ConceptKey::PatternDropdownLabel.get(lang),
                head,
                self.kind_table_rows(),
            ).with_category(PropertyCategory::AppearanceStats).with_order(10),
            ConfigProperty::stat_block(
                CANDLE_PATTERN_SCAN_STATS_ID,
                vec![
                    StatRow::new(ConceptKey::ScanTotalHitsLabel.get(lang), &self.cached_hits.len().to_string(), None),
                    StatRow::new(ConceptKey::BarCount.get(lang), &bars_surveyed.to_string(), None),
                ],
            ).with_order(20),
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
        if let Some((base, row, col)) = parse_table_cell_id(id) {
            if base != CANDLE_PATTERN_SCAN_TABLE_ID || col != 0 {
                return false;
            }
            let Some(enabled) = value.as_bool() else { return false };
            let Some(kind) = sorted_kinds().get(row).copied() else { return false };
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
        // The read-only totals Stat block carries no field of its own to
        // write — explicit rejection, mirroring `CandlePattern`'s own
        // `CANDLE_PATTERN_RESULT_STATS_ID` arm.
        if id == CANDLE_PATTERN_SCAN_STATS_ID {
            return false;
        }
        false
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "candle_pattern_scan",
        display_name: "Candle Pattern Scan",
        kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Survey a bar range for every enabled candlestick pattern",
        icon: "candle_pattern_scan",
        default_color: "#8BC34A",
        factory: |points, color| {
            let (t1, _) = points.first().copied().unwrap_or((0, 0.0));
            let (t2, _) = points.get(1).copied().unwrap_or((t1 + 72_000_000, 0.0));
            Box::new(CandlePatternScan::new(t1, t2, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mli::events::candle_pattern::scan as mli_scan;

    /// Six bars: a textbook bullish engulfing at indices 4..5, same fixture
    /// shape `candle_pattern`'s own tests use.
    fn bars_with_engulfing() -> Vec<Bar> {
        vec![
            Bar::new(0, 100.0, 101.0, 99.0, 100.5),
            Bar::new(1, 100.5, 101.5, 99.5, 100.8),
            Bar::new(2, 100.8, 101.8, 99.8, 101.0),
            Bar::new(3, 101.0, 102.0, 100.0, 101.2),
            Bar::new(4, 105.0, 106.0, 99.0, 100.0),
            Bar::new(5, 99.0, 110.0, 98.0, 108.0),
        ]
    }

    /// `CandlePatternScan::new` reads `default_enabled_kinds` off the
    /// shared `candle_pattern_engine_config()` global (see that field's own
    /// doc) — every test below that constructs one first serializes
    /// against every other test in this crate touching the same global via
    /// `crate::state::candle_pattern_engine::engine_config_test_lock`, and
    /// resets it to the documented default first, so a concurrently-running
    /// test elsewhere (e.g. `candle_pattern_engine`'s own `set_then_read_
    /// round_trips`) can never leave `enabled_kinds` narrowed to something
    /// this test did not ask for.
    fn reset_engine_config() -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::state::candle_pattern_engine::engine_config_test_lock();
        crate::state::candle_pattern_engine::set_candle_pattern_engine_config(
            crate::state::candle_pattern_engine::CandlePatternEngineConfig::default(),
        );
        guard
    }

    /// #4: a scan over a range returns EXACTLY the hits `mli`'s own `scan`
    /// returns for that range — asserted by direct identity against a
    /// separate, direct `mli::events::candle_pattern::scan` call, so this
    /// primitive can never drift from the vocabulary.
    #[test]
    fn scan_matches_a_direct_mli_scan_call_identically() {
        let _guard = reset_engine_config();
        let bars = bars_with_engulfing();
        let scan_prim = CandlePatternScan::new(0, 5_000, "#8BC34A");
        let hits = scan_prim.resolved_scan(&bars);

        let kinds: Vec<CandlePatternKind> =
            scan_prim.enabled_kinds.iter().filter_map(|id| CandlePatternKind::from_id(id)).collect();
        let ohlc: Vec<(f64, f64, f64, f64)> = bars.iter().map(|b| (b.open, b.high, b.low, b.close)).collect();
        let direct = mli_scan(&kinds, &ohlc, 0..6);

        assert_eq!(hits, direct);
        assert!(hits.iter().any(|h| h.kind == CandlePatternKind::BullishEngulfing && h.end == 5));
    }

    /// Piece 7: `TwoLines` keeps the engulfing hit whatever `p1`/`p2` hold
    /// (captured but unused); flipping to `Rectangle` with a band that
    /// excludes bar 5's own midpoint (high 110.0, low 98.0, midpoint 104.0)
    /// drops it, and a band that includes it keeps it.
    #[test]
    fn rectangle_shape_keeps_only_hits_inside_the_price_band() {
        let _guard = reset_engine_config();
        let bars = bars_with_engulfing();

        let mut scan_prim = CandlePatternScan::new(0, 5_000, "#8BC34A");
        scan_prim.p1 = 1.0;
        scan_prim.p2 = 2.0;
        let two_lines_hits = scan_prim.resolved_scan(&bars);
        assert!(two_lines_hits.iter().any(|h| h.kind == CandlePatternKind::BullishEngulfing && h.end == 5));

        scan_prim.shape = ScanShape::Rectangle;
        let out_of_band = scan_prim.resolved_scan(&bars);
        assert!(!out_of_band.iter().any(|h| h.end == 5), "Rectangle drops a hit outside [p1, p2]");

        scan_prim.p1 = 100.0;
        scan_prim.p2 = 108.0;
        let in_band = scan_prim.resolved_scan(&bars);
        assert!(in_band.iter().any(|h| h.kind == CandlePatternKind::BullishEngulfing && h.end == 5), "Rectangle keeps a hit inside [p1, p2]");
    }

    /// A scan saved before piece 7 carries no `shape`/`p1`/`p2` fields at
    /// all — its JSON must still load, landing on `TwoLines` (the shape it
    /// always drew). Mirrors `smc_scan`'s own identical test.
    #[test]
    fn a_pre_piece_7_saved_scan_loads_as_two_lines() {
        let data = PrimitiveData {
            type_id: "candle_pattern_scan".to_string(),
            display_name: "Candle Pattern Scan".to_string(),
            color: PrimitiveColor::new("#8BC34A"),
            width: 1.0,
            ..Default::default()
        };
        let old_json = serde_json::json!({ "data": data, "ts1": 0, "ts2": 5_000 }).to_string();
        assert!(!old_json.contains("shape") && !old_json.contains("\"p1\"") && !old_json.contains("\"p2\""));

        let restored: CandlePatternScan = serde_json::from_str(&old_json).expect("old scan JSON still loads");
        assert_eq!(restored.shape, ScanShape::TwoLines);
        assert_eq!(restored.p1, 0.0);
        assert_eq!(restored.p2, 0.0);
    }

    /// #5: unchecking a kind removes its hits.
    #[test]
    fn unchecking_a_kind_removes_its_hits() {
        let _guard = reset_engine_config();
        let bars = bars_with_engulfing();
        let mut scan_prim = CandlePatternScan::new(0, 5_000, "#8BC34A");
        scan_prim.refresh_bar_derived_cache(&bars);
        assert!(scan_prim.cached_hits.iter().any(|h| h.kind == CandlePatternKind::BullishEngulfing));

        scan_prim.enabled_kinds.retain(|id| id != "bullish_engulfing");
        scan_prim.refresh_bar_derived_cache(&bars);
        assert!(!scan_prim.cached_hits.iter().any(|h| h.kind == CandlePatternKind::BullishEngulfing));
    }

    /// #6 (round trip): the render cache is ABSENT from `to_json`.
    #[test]
    fn render_cache_is_absent_from_to_json() {
        let _guard = reset_engine_config();
        let bars = bars_with_engulfing();
        let mut scan_prim = CandlePatternScan::new(0, 5_000, "#8BC34A");
        scan_prim.refresh_bar_derived_cache(&bars);
        let json = scan_prim.to_json();
        assert!(!json.contains("cached_window"), "{json}");
        assert!(!json.contains("cached_hits"), "{json}");

        let mut registry = super::super::super::registry::PrimitiveRegistry::new();
        registry.register_typed::<CandlePatternScan>(metadata());
        let restored = registry.from_json("candle_pattern_scan", &json).expect("round trip");
        assert_eq!(restored.points(), scan_prim.points());
    }

    /// Piece 8's own write path: the Coverage tab's Shape row applies
    /// through this exact generic `apply_style_property("shape", ...)`.
    #[test]
    fn shape_property_round_trips() {
        let mut scan_prim = CandlePatternScan::new(0, 5_000, "#8BC34A");
        assert_eq!(scan_prim.shape, ScanShape::TwoLines);

        assert!(scan_prim.apply_style_property("shape", &PropertyValue::String("rectangle".to_string())));
        assert_eq!(scan_prim.shape, ScanShape::Rectangle);

        assert!(!scan_prim.apply_style_property("shape", &PropertyValue::String("bogus".to_string())));
    }

    /// Unchecking a kind through the declared table cell (the actual click
    /// path a settings-modal user drives) reaches the same
    /// `enabled_kinds` mutation the direct-field test above exercises.
    #[test]
    fn table_cell_click_toggles_enabled_kinds() {
        let _guard = reset_engine_config();
        let mut scan_prim = CandlePatternScan::new(0, 5_000, "#8BC34A");
        let row = sorted_kinds().iter().position(|k| k.id() == "bullish_engulfing").expect("catalogue has it");
        let cell_id = format!("{CANDLE_PATTERN_SCAN_TABLE_ID}::cell:{row}:0");

        assert!(scan_prim.apply_style_property(&cell_id, &PropertyValue::Boolean(false)));
        assert!(!scan_prim.enabled_kinds.iter().any(|k| k == "bullish_engulfing"));

        assert!(scan_prim.apply_style_property(&cell_id, &PropertyValue::Boolean(true)));
        assert!(scan_prim.enabled_kinds.iter().any(|k| k == "bullish_engulfing"));
    }

    // =========================================================================
    // Pure layout functions — label placement, collision/cluster collapse
    // (owner complaint 0.5.615, readable-hits arc). Every function under
    // test here takes plain geometry and returns plain geometry: no
    // `RenderContext`, no mock render backend needed.
    // =========================================================================

    #[test]
    fn cluster_indices_by_gap_groups_only_points_within_gap_of_the_runs_first_member() {
        // 0.0 and 2.0 are both within 3.0 of 0.0 (the run's own first
        // member) — one cluster of 2; 10.0 starts a new run on its own.
        let xs = [0.0_f64, 2.0, 10.0];
        let ranges = cluster_indices_by_gap(&xs, |x| *x, 3.0);
        assert_eq!(ranges, vec![(0, 2), (2, 1)]);
    }

    #[test]
    fn cluster_indices_by_gap_is_anchored_to_the_runs_first_member_not_the_previous_item() {
        // Each successive point is 2.0 from its own predecessor (so a naive
        // pairwise-consecutive rule would chain all four into one run), but
        // 6.0 is already 6.0 from the run's own first member (0.0), past a
        // gap of 5.0 — the anchored-to-first rule must split there, the
        // SAME rule `smc_scan::for_each_cluster` already applies to its own
        // marker dots.
        let xs = [0.0_f64, 2.0, 4.0, 6.0, 8.0];
        let ranges = cluster_indices_by_gap(&xs, |x| *x, 5.0);
        assert_eq!(ranges, vec![(0, 3), (3, 2)]);
    }

    #[test]
    fn cluster_indices_by_gap_on_empty_input_returns_empty() {
        let xs: [f64; 0] = [];
        assert_eq!(cluster_indices_by_gap(&xs, |x| *x, 5.0), Vec::<(usize, usize)>::new());
    }

    fn mark(x: f64, tint: MarkerTint) -> HitMark {
        HitMark { x, hi_x1: x - 1.0, hi_x2: x + 1.0, hi_y1: 100.0, hi_y2: 110.0, tint, kind: CandlePatternKind::Doji }
    }

    #[test]
    fn build_clusters_unions_the_highlight_extent_of_every_absorbed_member() {
        let marks = [
            HitMark { x: 0.0, hi_x1: -1.0, hi_x2: 1.0, hi_y1: 90.0, hi_y2: 95.0, tint: MarkerTint::Up, kind: CandlePatternKind::Doji },
            HitMark { x: 2.0, hi_x1: 1.0, hi_x2: 3.0, hi_y1: 80.0, hi_y2: 100.0, tint: MarkerTint::Up, kind: CandlePatternKind::Hammer },
        ];
        let clusters = build_clusters(&marks, 5.0);
        assert_eq!(clusters.len(), 1);
        let c = clusters[0];
        assert_eq!(c.count, 2);
        assert_eq!((c.hi_x1, c.hi_x2), (-1.0, 3.0));
        assert_eq!((c.hi_y1, c.hi_y2), (80.0, 100.0));
        // The FIRST member's own kind represents the cluster while `count`
        // is still available for the caller to notice it is > 1.
        assert_eq!(c.kind, CandlePatternKind::Doji);
    }

    #[test]
    fn build_clusters_keeps_far_apart_hits_separate() {
        let marks = [mark(0.0, MarkerTint::Up), mark(100.0, MarkerTint::Up)];
        let clusters = build_clusters(&marks, 5.0);
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].count, 1);
        assert_eq!(clusters[1].count, 1);
    }

    fn label_box(center_x: f64, half_width: f64, count: usize) -> LabelBox {
        LabelBox { center_x, half_width, label_y: 0.0, tint: MarkerTint::Up, kind: CandlePatternKind::Doji, count }
    }

    #[test]
    fn merge_overlapping_label_boxes_leaves_non_overlapping_boxes_untouched() {
        let boxes = [label_box(0.0, 5.0, 1), label_box(20.0, 5.0, 1)];
        let merged = merge_overlapping_label_boxes(&boxes, LabelSide::Above);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].count, 1);
        assert_eq!(merged[1].count, 1);
    }

    #[test]
    fn merge_overlapping_label_boxes_merges_two_overlapping_boxes_into_one_wider_box() {
        // [−5, 5] and [3, 13] overlap on [3, 5] — must merge into one box
        // spanning the union, [−5, 13].
        let boxes = [label_box(0.0, 5.0, 1), label_box(8.0, 5.0, 2)];
        let merged = merge_overlapping_label_boxes(&boxes, LabelSide::Above);
        assert_eq!(merged.len(), 1);
        let m = merged[0];
        assert_eq!(m.count, 3);
        assert!((m.center_x - m.half_width - (-5.0)).abs() < 1e-9, "left edge");
        assert!((m.center_x + m.half_width - 13.0).abs() < 1e-9, "right edge");
    }

    #[test]
    fn merge_overlapping_label_boxes_chains_a_three_way_overlap_into_one_box() {
        let boxes = [label_box(0.0, 5.0, 1), label_box(8.0, 5.0, 1), label_box(16.0, 5.0, 1)];
        let merged = merge_overlapping_label_boxes(&boxes, LabelSide::Above);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].count, 3);
    }

    #[test]
    fn merge_overlapping_label_boxes_never_returns_overlapping_boxes() {
        // A denser run than any single gap constant here would collapse on
        // its own — the invariant under test is the OUTPUT, not any one
        // input shape: no two returned boxes may overlap.
        let boxes: Vec<LabelBox> = (0..12).map(|i| label_box(i as f64 * 3.0, 4.0, 1)).collect();
        let merged = merge_overlapping_label_boxes(&boxes, LabelSide::Above);
        for pair in merged.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(
                a.center_x + a.half_width <= b.center_x - b.half_width + 1e-9,
                "boxes at {} and {} overlap",
                a.center_x, b.center_x
            );
        }
    }

    #[test]
    fn merge_overlapping_label_boxes_extends_label_y_outward_on_each_side() {
        // `Above`: smaller Y is further from the bars — the merged box must
        // keep the SMALLEST (furthest-out) label_y of its members.
        let boxes = [
            LabelBox { center_x: 0.0, half_width: 5.0, label_y: -10.0, tint: MarkerTint::Up, kind: CandlePatternKind::Doji, count: 1 },
            LabelBox { center_x: 8.0, half_width: 5.0, label_y: -20.0, tint: MarkerTint::Up, kind: CandlePatternKind::Doji, count: 1 },
        ];
        let merged = merge_overlapping_label_boxes(&boxes, LabelSide::Above);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].label_y, -20.0);

        // `Below`: larger Y is further from the bars — the merged box must
        // keep the LARGEST label_y.
        let boxes = [
            LabelBox { center_x: 0.0, half_width: 5.0, label_y: 10.0, tint: MarkerTint::Down, kind: CandlePatternKind::Doji, count: 1 },
            LabelBox { center_x: 8.0, half_width: 5.0, label_y: 20.0, tint: MarkerTint::Down, kind: CandlePatternKind::Doji, count: 1 },
        ];
        let merged = merge_overlapping_label_boxes(&boxes, LabelSide::Below);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].label_y, 20.0);
    }

    #[test]
    fn clamp_within_leaves_an_already_fitting_box_untouched() {
        assert_eq!(clamp_within(50.0, 5.0, 0.0, 100.0), 50.0);
    }

    #[test]
    fn clamp_within_pulls_a_box_back_from_the_right_edge() {
        // Centre 98, half-width 5 → right edge at 103, past max 100; the
        // clamped centre must put the right edge exactly on `max`.
        let c = clamp_within(98.0, 5.0, 0.0, 100.0);
        assert_eq!(c + 5.0, 100.0);
    }

    #[test]
    fn clamp_within_pulls_a_box_back_from_the_left_edge() {
        let c = clamp_within(2.0, 5.0, 0.0, 100.0);
        assert_eq!(c - 5.0, 0.0);
    }

    #[test]
    fn clamp_within_does_not_panic_when_the_box_is_wider_than_the_available_range() {
        // No assertion on the exact result (there is no "correct" answer
        // when the box cannot fit either edge at once) — only that this
        // never panics.
        let _ = clamp_within(50.0, 1000.0, 0.0, 100.0);
    }

    #[test]
    fn label_side_for_routes_bullish_up_and_everything_else_down() {
        assert_eq!(label_side_for(MarkerTint::Up), LabelSide::Below);
        assert_eq!(label_side_for(MarkerTint::Down), LabelSide::Above);
        assert_eq!(label_side_for(MarkerTint::Neutral), LabelSide::Above);
    }

    #[test]
    fn labels_visible_at_gates_on_the_dpr_scaled_bar_spacing_floor() {
        assert!(!labels_visible_at(MIN_BAR_SPACING_FOR_LABELS_PX - 1.0, 1.0));
        assert!(labels_visible_at(MIN_BAR_SPACING_FOR_LABELS_PX, 1.0));
        assert!(labels_visible_at(MIN_BAR_SPACING_FOR_LABELS_PX + 1.0, 1.0));
        // dpr = 2.0 doubles the floor in device pixels.
        assert!(!labels_visible_at(MIN_BAR_SPACING_FOR_LABELS_PX * 2.0 - 1.0, 2.0));
        assert!(labels_visible_at(MIN_BAR_SPACING_FOR_LABELS_PX * 2.0, 2.0));
    }

    #[test]
    fn tint_color_uses_the_fixed_up_down_colours_and_stroke_for_neutral() {
        assert_eq!(tint_color(MarkerTint::Up, "#123456"), UP_COLOR);
        assert_eq!(tint_color(MarkerTint::Down, "#123456"), DOWN_COLOR);
        assert_eq!(tint_color(MarkerTint::Neutral, "#123456"), "#123456");
    }
}
