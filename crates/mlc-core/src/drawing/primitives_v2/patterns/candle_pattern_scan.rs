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
    RenderContext, render_primitive_text, TextAlign,
    config::{ConfigProperty, SelectOption, PropertyCategory, PropertyValue, StatRow, TableCell, parse_table_cell_id},
};
use super::candle_pattern::{sorted_kinds, pattern_display_name};
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
#[derive(Clone, Copy)]
enum MarkerTint { Up, Down, Neutral }

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
    /// claim never read as the same instrument), then one marker per
    /// completion, tinted by `CandlePatternHit::direction`. Computed FRESH
    /// from `bars` here via [`Self::resolved_scan`] — never reads
    /// [`Self::cached_hits`] — matching
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    ///
    /// Overlap handling: consecutive completion markers closer than
    /// [`MIN_MARKER_GAP_PX`] collapse into ONE marker carrying a small
    /// count badge, rather than drawing every one and letting them
    /// overprint — see the module's own doc for why a proximity collapse
    /// (not a bar-spacing threshold) is what actually prevents overprinting
    /// at any zoom level.
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

        render_scan_bracket(ctx, self.shape, x1, x2, self.p1, self.p2, &self.data.color.stroke, self.data.width);

        // Collect every cluster's own (x, y, tint, count) FIRST, while
        // `ctx.bars()`'s own borrow is still live (every call here is
        // `&self` — `bars`/`bar_to_x`/`price_to_y` all are) — then draw
        // from that owned list in a second pass, once the borrow has
        // ended, so the mutable calls below (`set_fill_color`, `fill`, …)
        // never conflict with it.
        let clusters: Vec<(f64, f64, MarkerTint, usize)> = {
            let bars = ctx.bars();
            let hits = self.resolved_scan(bars);

            // Collapse consecutive completion markers sitting closer than
            // `MIN_MARKER_GAP_PX` — `hits` is already sorted by `end`
            // (`scan`'s own doc), so this is a single forward pass.
            let mut clusters: Vec<(f64, f64, MarkerTint, usize)> = Vec::new();
            for hit in &hits {
                let x = ctx.bar_to_x(hit.end as f64);
                let bar = bars.get(hit.end);
                let (y, tint) = match hit.direction {
                    mli::Direction::Up => (
                        bar.map(|b| ctx.price_to_y(b.low) + 10.0 * dpr).unwrap_or(chart_height / 2.0),
                        MarkerTint::Up,
                    ),
                    mli::Direction::Down => (
                        bar.map(|b| ctx.price_to_y(b.high) - 10.0 * dpr).unwrap_or(chart_height / 2.0),
                        MarkerTint::Down,
                    ),
                    mli::Direction::Neutral => (
                        bar.map(|b| ctx.price_to_y((b.high + b.low) / 2.0)).unwrap_or(chart_height / 2.0),
                        MarkerTint::Neutral,
                    ),
                };
                match clusters.last_mut() {
                    Some((last_x, _, _, count)) if (x - *last_x).abs() < MIN_MARKER_GAP_PX * dpr => {
                        *count += 1;
                    }
                    _ => clusters.push((x, y, tint, 1)),
                }
            }
            clusters
        };

        let stroke_color = self.data.color.stroke.clone();
        for (x, y, tint, count) in clusters {
            let color: &str = match tint {
                MarkerTint::Up => UP_COLOR,
                MarkerTint::Down => DOWN_COLOR,
                MarkerTint::Neutral => &stroke_color,
            };
            ctx.set_fill_color(color);
            ctx.begin_path();
            ctx.arc(x, y, 3.0 * dpr, 0.0, std::f64::consts::TAU);
            ctx.fill();

            if count > 1 {
                ctx.set_font(&format!("{}px sans-serif", (9.0 * dpr) as i32));
                ctx.set_text_align(crate::render::TextAlign::Center);
                ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                ctx.set_fill_color(&stroke_color);
                ctx.fill_text(&count.to_string(), x, y - 8.0 * dpr);
            }
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
}
