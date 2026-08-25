//! Candle Pattern — the CLAIM half of the candle-pattern markup engine
//! (`docs/mlc/plans/markup-engines-2026-08-25.md`, "the hand-marked
//! primitive... is a CLAIM the guard judges").
//!
//! ONE point: the bar the analyst says the pattern COMPLETES on. The span
//! is DERIVED — `kind.bars_needed()` bars ending at that bar — never
//! stored, so a stored span can never disagree with the chosen pattern
//! (dragging the anchor, or switching `kind_id`, both move the whole span
//! for free). The vocabulary is never re-invented here — every check below
//! calls straight into `mli::events::candle_pattern` (`CandlePatternKind`,
//! `detect_at`), the same discipline `elliott::guard`'s own opening
//! paragraph states for `mli::ewa::rules`.
//!
//! See [`super::candle_pattern_scan::CandlePatternScan`] for the SURVEY
//! half — the bars say what they contain, over a range, with no claim of
//! their own to judge.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64, find_bar_for_timestamp_ms};
use crate::i18n::{current_language, CandlePatternKey, ConceptKey};
use mli::events::candle_pattern::{CandlePatternKind, PatternBias, detect_at};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, render_primitive_text, TextAlign,
    config::{ConfigProperty, PropertyValue, StatRow, Tone, SelectOption},
    elliott::guard::{self, Violation},
};

/// The 34 catalogue ids, ordered by `bars_needed()` (1/2/3/5) then by
/// `mli`'s own [`CandlePatternKind::all`] order — the ordering both
/// [`CandlePattern::style_properties`]'s own `Select` and
/// [`super::candle_pattern_scan::CandlePatternScan`]'s own `Table` present
/// their 34 rows in. `sort_by_key` is STABLE, so the secondary "`all()`
/// order" criterion falls out of leaving ties in their original relative
/// order — no explicit tiebreaker needed.
pub(super) fn sorted_kinds() -> Vec<CandlePatternKind> {
    let mut kinds: Vec<CandlePatternKind> = CandlePatternKind::all().to_vec();
    kinds.sort_by_key(|k| k.bars_needed());
    kinds
}

/// This pattern's own localized display name — `CandlePatternKey::from_id`
/// when the catalogue's own i18n table names it (every id `mli` currently
/// ships does), falling back to `mli`'s own English `label()` for an id a
/// future catalogue grew that this table has not caught up with yet (see
/// `CandlePatternKey::from_id`'s own doc for why that is `None` rather than
/// a placeholder).
pub(super) fn pattern_display_name(kind: CandlePatternKind) -> &'static str {
    match CandlePatternKey::from_id(kind.id()) {
        Some(key) => key.get(current_language()),
        None => kind.label(),
    }
}

/// This claim's own two guard rules, derived from the SAME
/// `resolved_window`/`detect` a fresh `render` call would compute — the one
/// function both [`CandlePattern::refresh_bar_derived_cache`] (fills the
/// cache [`Primitive::context_violations`] returns) and
/// [`CandlePattern::render`] (recolors the marker, never reading the cache
/// — see that method's own doc for why) call, so the two can never
/// disagree about what counts as a violation.
fn violations_for(window: Option<(usize, usize)>, detected: Option<mli::Direction>) -> Vec<Violation> {
    match window {
        None => vec![Violation::whole(
            "candle_pattern_window_incomplete",
            ConceptKey::RuleCandlePatternWindowIncomplete,
        )],
        Some(_) => match detected {
            Some(_) => Vec::new(),
            None => vec![Violation::whole(
                "candle_pattern_not_formed",
                ConceptKey::RuleCandlePatternNotFormed,
            )],
        },
    }
}

/// The read-only Stat block's own id — shared between
/// [`CandlePattern::style_properties`] (which declares it) and
/// [`CandlePattern::apply_style_property`]'s own explicit reject arm, same
/// role `REGRESSION_RESULT_STATS_ID` plays for `RegressionTrend`.
const CANDLE_PATTERN_RESULT_STATS_ID: &str = "candle_pattern_result_stats";

fn default_kind_id() -> String {
    // Doji — the one-bar pattern, so a freshly placed mark is never in an
    // incomplete-window state (see the struct's own field doc).
    "doji".to_string()
}
fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CandlePattern {
    pub data: PrimitiveData,
    /// Timestamp (ms) of the bar the claimed pattern COMPLETES on.
    #[serde(default)]
    pub anchor_ts: i64,
    /// `mli::events::candle_pattern::CandlePatternKind::id()` — the pattern
    /// this mark claims. Defaults to `"doji"`: the one-bar pattern, so a
    /// freshly placed mark is never in an incomplete-window state.
    #[serde(default = "default_kind_id")]
    pub kind_id: String,
    /// Whether the resolved pattern's own localized name draws under the
    /// bracket — the patterns-family's own toggle convention
    /// (`ConfigProperty::show_labels`, every harmonic-pattern type in this
    /// same directory carries the identical field).
    #[serde(default = "default_true")]
    pub show_labels: bool,

    /// The claim's own resolved `[start, end]` bar-index window — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract. Filled by
    /// that hook from the SAME [`CandlePattern::resolved_window`] call
    /// `render`/`hit_test` make directly; read back by
    /// [`CandlePattern::result_stat_rows`] (a `&self`-only reader with no
    /// `bars` of its own).
    ///
    /// `#[serde(skip)]`: a stale window surviving a reload/undo would
    /// misreport a span that no longer matches this primitive's own current
    /// `anchor_ts`/`kind_id` — a freshly deserialized primitive starts at
    /// `None` until the owning bubble's own bar-derived-cache pass runs.
    #[serde(skip)]
    pub cached_window: Option<(usize, usize)>,
    /// The claim's own resolved outcome — `Some(direction)` when the
    /// window's bars form the claimed pattern, `None` otherwise (including
    /// when [`Self::cached_window`] itself is `None`). Same cache contract
    /// as [`Self::cached_window`], filled by the SAME
    /// [`CandlePattern::detect`] call.
    #[serde(skip)]
    pub cached_detected: Option<mli::Direction>,
    /// This claim's own two guard rules — see [`Primitive::context_violations`]'s
    /// own contract. Derived from [`Self::cached_window`]/
    /// [`Self::cached_detected`] via [`violations_for`] at the SAME
    /// `refresh_bar_derived_cache` call that fills those two fields, never
    /// a second, independently-computed pass.
    #[serde(skip)]
    pub cached_violations: Vec<Violation>,
}

impl CandlePattern {
    pub fn new(anchor_ts: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "candle_pattern".to_string(),
                display_name: "Candle Pattern".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            anchor_ts,
            kind_id: default_kind_id(),
            show_labels: true,
            cached_window: None,
            cached_detected: None,
            cached_violations: Vec::new(),
        }
    }

    /// This claim's own resolved pattern — `None` for a `kind_id` no
    /// current catalogue names (see `CandlePatternKind::from_id`'s own
    /// doc: a drawing saved by a catalogue version that no longer exists,
    /// never guessed at).
    pub fn kind(&self) -> Option<CandlePatternKind> {
        CandlePatternKind::from_id(&self.kind_id)
    }

    /// The anchor timestamp resolved to a REAL bar index — `None` when
    /// `bars` is empty or the anchor lands strictly beyond the last loaded
    /// bar (an extrapolated future index has no real bar to judge, so
    /// there is nothing here for [`Self::resolved_window`] to anchor a
    /// completed pattern to).
    fn anchor_bar_idx(&self, bars: &[Bar]) -> Option<usize> {
        let idx = find_bar_for_timestamp_ms(bars, self.anchor_ts)?;
        (idx < bars.len()).then_some(idx)
    }

    /// The `[start, end]` bar-index window this claim covers —
    /// `kind.bars_needed()` bars ending at the anchor bar. `None` when the
    /// anchor cannot resolve to a real bar, the `kind_id` cannot resolve to
    /// a catalogue pattern, or fewer than `bars_needed()` bars exist up to
    /// and including the anchor — the exact case
    /// `"candle_pattern_window_incomplete"` reports.
    ///
    /// `end.checked_add(1).and_then(|e| e.checked_sub(need))` mirrors
    /// `mli::events::candle_pattern::scan`'s own start-index derivation for
    /// one completion bar byte for byte, so this primitive's own claimed
    /// window and the window a survey would report for the identical
    /// completion bar can never disagree.
    pub fn resolved_window(&self, bars: &[Bar]) -> Option<(usize, usize)> {
        let kind = self.kind()?;
        let end = self.anchor_bar_idx(bars)?;
        let start = end.checked_add(1)?.checked_sub(kind.bars_needed())?;
        Some((start, end))
    }

    /// This claim's own outcome — `detect_at` run over the SAME window
    /// [`Self::resolved_window`] resolves, through the SAME OHLC tuples
    /// `mli::events::candle_pattern::scan` itself builds. `None` when the
    /// window is not yet resolvable, or the window's bars do not form the
    /// claimed pattern.
    pub fn detect(&self, bars: &[Bar]) -> Option<mli::Direction> {
        let kind = self.kind()?;
        let (start, end) = self.resolved_window(bars)?;
        let window: Vec<(f64, f64, f64, f64)> =
            bars[start..=end].iter().map(|b| (b.open, b.high, b.low, b.close)).collect();
        detect_at(kind, &window).map(|(_, direction)| direction)
    }

    /// The read-only Stat block's own rows — bars needed, catalogue bias
    /// (both pure facts of [`Self::kind`], not bar-derived), and whether
    /// the bars currently agree with the claim, read from the SAME
    /// [`Self::cached_window`]/[`Self::cached_violations`]
    /// [`Primitive::context_violations`] itself returns — never a second,
    /// independently-computed check. `cached_window.is_some()` gates the
    /// third row so a primitive [`Primitive::refresh_bar_derived_cache`]
    /// has never touched (both fields at their own `Default`, i.e. empty
    /// violations) reads as "not yet confirmed", never a false "agrees".
    pub fn result_stat_rows(&self) -> Vec<StatRow> {
        let lang = current_language();
        let Some(kind) = self.kind() else { return Vec::new() };
        let bias_value = match kind.bias() {
            PatternBias::Bullish => ConceptKey::PatternBiasBullish.get(lang),
            PatternBias::Bearish => ConceptKey::PatternBiasBearish.get(lang),
            PatternBias::Indecision => ConceptKey::PatternBiasIndecision.get(lang),
        };
        let agrees = self.cached_window.is_some() && self.cached_violations.is_empty();
        vec![
            StatRow::new(ConceptKey::BarCount.get(lang), &kind.bars_needed().to_string(), None),
            StatRow::new(ConceptKey::PatternBiasLabel.get(lang), bias_value, None),
            StatRow::new(
                ConceptKey::PatternMatchesBarsLabel.get(lang),
                if agrees { ConceptKey::YesValue.get(lang) } else { ConceptKey::NoValue.get(lang) },
                Some(if agrees { Tone::Good } else { Tone::Bad }),
            ),
        ]
    }
}

impl Primitive for CandlePattern {
    fn type_id(&self) -> &'static str { "candle_pattern" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Pattern }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::SingleClick }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.anchor_ts, 0.0)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(t, _)) = pts.first() { self.anchor_ts = t; }
    }
    fn translate(&mut self, td: i64, _pd: f64) { self.anchor_ts += td; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, _price: f64) {
        match pt {
            ControlPointType::Point1 | ControlPointType::Move => self.anchor_ts = ts_ms,
            _ => {}
        }
    }

    /// The anchor handle, then the drawn bracket's own x-span (any `sy`
    /// across the chart, matching `BarsPattern::hit_test`'s own full-height
    /// column test — this marker's body is a TIME range, not a price
    /// level).
    fn hit_test(&self, sx: f64, _sy: f64, bars: &[Bar], vp: &Viewport, _ps: &PriceScale) -> HitTestResult {
        let anchor_x = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.anchor_ts));
        if (sx - anchor_x).abs() < HIT_TOLERANCE {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        let (min_x, max_x) = match self.resolved_window(bars) {
            Some((start, end)) => (vp.bar_to_x_f64(start as f64), vp.bar_to_x_f64(end as f64)),
            None => (anchor_x, anchor_x),
        };
        let (min_x, max_x) = (min_x.min(max_x), min_x.max(max_x));
        if sx >= min_x - HIT_TOLERANCE && sx <= max_x + HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let x = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.anchor_ts));
        let cy = vp.price_to_y((ps.price_min + ps.price_max) / 2.0, ps.price_min, ps.price_max);
        vec![ControlPoint::point1(x, cy)]
    }

    /// A bracket spanning the claimed window (an underline plus two end
    /// ticks) beneath the window's own lowest low, tinted
    /// `guard::VIOLATION_COLOR` when the bars disagree with the claim AND
    /// `EngineConfig::judge_claims` is on — same gate
    /// [`Self::refresh_bar_derived_cache`] reads, so the on-chart tint and
    /// [`Self::context_violations`]'s own Stat-block row can never disagree
    /// about whether a violation is CURRENTLY active. Window/outcome are
    /// computed FRESH from `bars` here (never read off
    /// [`Self::cached_window`]/[`Self::cached_detected`]), matching
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract: this must
    /// still draw (and recolor) correctly even if that hook is never
    /// called. The pattern's own localized name draws under the bracket
    /// when [`Self::show_labels`] is on.
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let bars = ctx.bars();
        let window = self.resolved_window(bars);
        let detected = self.detect(bars);
        let judge_claims = crate::state::candle_pattern_engine::candle_pattern_engine_config().judge_claims;
        let violated = judge_claims && !violations_for(window, detected).is_empty();
        let stroke = if violated { guard::VIOLATION_COLOR.to_string() } else { self.data.color.stroke.clone() };

        let chart_height = ctx.chart_height();
        let anchor_x = ctx.ts_to_x_ms(self.anchor_ts);
        // Half a bar of spacing on each side. Twelve of the 34 kinds are
        // single-bar (Doji, Hammer, Marubozu, Spinning Top, ...), and
        // without this their bracket degenerates to `x1 == x2`: a lone
        // tick with the pattern's name under it, anchored to nothing the
        // eye can connect it back to. Derived from `bar_to_x` itself
        // because `RenderContext` exposes no bar width of its own.
        let half_bar = ((ctx.bar_to_x(1.0) - ctx.bar_to_x(0.0)) / 2.0).abs();

        let (x1, x2, bracket_y) = match window {
            Some((start, end)) => {
                let x1 = ctx.bar_to_x(start as f64) - half_bar;
                let x2 = ctx.bar_to_x(end as f64) + half_bar;
                let low = bars[start..=end].iter().fold(f64::INFINITY, |acc, b| acc.min(b.low));
                let y = if low.is_finite() { ctx.price_to_y(low) + 14.0 * dpr } else { chart_height / 2.0 };
                (x1, x2, y)
            }
            // The claimed window is not fully loaded (the claim sits too
            // close to the start of the series). Bracket the anchor bar
            // itself while that bar is still on screen — only a claim
            // anchored outside the series entirely falls back to the
            // chart's middle, and a label there describes no price at all.
            None => match super::super::anchor_mark::bar_span_at(bars, self.anchor_ts) {
                Some((_, low)) => (
                    anchor_x - half_bar,
                    anchor_x + half_bar,
                    ctx.price_to_y(low) + 14.0 * dpr,
                ),
                None => (anchor_x, anchor_x, chart_height / 2.0),
            },
        };
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));

        ctx.set_stroke_color(&stroke);
        ctx.set_stroke_width(self.data.width);
        ctx.set_line_dash(&[]);
        ctx.begin_path();
        ctx.move_to(crisp(min_x, dpr), crisp(bracket_y, dpr));
        ctx.line_to(crisp(max_x, dpr), crisp(bracket_y, dpr));
        ctx.stroke();
        let tick = 5.0 * dpr;
        for x in [min_x, max_x] {
            ctx.begin_path();
            ctx.move_to(crisp(x, dpr), crisp(bracket_y - tick, dpr));
            ctx.line_to(crisp(x, dpr), crisp(bracket_y + tick, dpr));
            ctx.stroke();
        }

        if self.show_labels {
            if let Some(kind) = self.kind() {
                ctx.set_fill_color(&stroke);
                ctx.set_font("11px sans-serif");
                ctx.set_text_align(crate::render::TextAlign::Center);
                ctx.set_text_baseline(crate::render::TextBaseline::Top);
                ctx.fill_text(pattern_display_name(kind), (min_x + max_x) / 2.0, bracket_y + tick + 2.0 * dpr);
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
                render_primitive_text(ctx, text, text_x, text_y, &stroke);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    /// Recompute [`Self::cached_window`]/[`Self::cached_detected`]/
    /// [`Self::cached_violations`] from `bars` through the SAME
    /// [`Self::resolved_window`]/[`Self::detect`]/[`violations_for`] calls
    /// `render` makes directly — see [`Primitive::refresh_bar_derived_cache`]'s
    /// own contract. Gated on `EngineConfig::judge_claims`
    /// (`crate::state::candle_pattern_engine`): when the analyst has opted
    /// out of guard reporting, the cache still carries the resolved window/
    /// outcome (the Stat block's own bars-needed/bias rows do not depend on
    /// judging), only [`Self::cached_violations`] stays empty.
    fn refresh_bar_derived_cache(&mut self, bars: &[Bar]) {
        let window = self.resolved_window(bars);
        let detected = self.detect(bars);
        self.cached_window = window;
        self.cached_detected = detected;
        self.cached_violations = if crate::state::candle_pattern_engine::candle_pattern_engine_config().judge_claims {
            violations_for(window, detected)
        } else {
            Vec::new()
        };
    }

    /// Snap this claim onto the bar its own detector names, within
    /// `claim_snap_bars` — see [`Primitive::snap_to_detected`] for the
    /// contract and why it exists. Searches outward from the placed anchor,
    /// nearest first, and restores the original anchor when nothing inside
    /// the radius resolves.
    fn snap_to_detected(&mut self, bars: &[Bar]) {
        let radius = crate::state::candle_pattern_engine::candle_pattern_engine_config().claim_snap_bars;
        if radius == 0 || self.detect(bars).is_some() {
            return;
        }
        let Some(idx) = self.anchor_bar_idx(bars) else { return };
        let placed = self.anchor_ts;
        for step in 1..=radius {
            for candidate in [idx.checked_sub(step), idx.checked_add(step).filter(|i| *i < bars.len())] {
                let Some(candidate) = candidate else { continue };
                self.anchor_ts = bars[candidate].timestamp * 1000;
                if self.detect(bars).is_some() {
                    return;
                }
            }
        }
        self.anchor_ts = placed;
    }

    fn markup_family(&self) -> Option<super::super::markup::MarkupFamily> {
        Some(super::super::markup::MarkupFamily::CandlePattern)
    }

    /// Append-only, by [`Primitive::push_context_violations`]'s own
    /// contract: the cross-primitive pass runs right after this claim's
    /// own cache refresh has rebuilt the self-derived list.
    fn push_context_violations(&mut self, violations: Vec<Violation>) {
        self.cached_violations.extend(violations);
    }

    fn context_violations(&self) -> &[Violation] { &self.cached_violations }

    /// No design-approved archetype names a candle-pattern-claim subject
    /// tab yet (this whole engine is new, 2026-08-25) — measured the same
    /// way `BarsPattern`/`FixedVolumeProfile` already did for their own
    /// ungrouped rows: every property below stays in its constructor's own
    /// default category/run rather than reaching for a `PropertyGroup`
    /// whose own doc names a DIFFERENT subject.
    fn style_properties(&self) -> Vec<ConfigProperty> {
        let lang = current_language();
        let options: Vec<SelectOption> = sorted_kinds()
            .into_iter()
            .map(|k| SelectOption::new(k.id(), pattern_display_name(k)))
            .collect();
        vec![
            ConfigProperty::select(
                "kind_id",
                ConceptKey::PatternDropdownLabel.get(lang),
                &self.kind_id,
                options,
            ).with_order(10),
            ConfigProperty::show_labels(self.show_labels).with_order(11),
            ConfigProperty::stat_block(CANDLE_PATTERN_RESULT_STATS_ID, self.result_stat_rows())
                .with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            // The computed Stat block carries no field of its own to write
            // — explicit rejection, not merely the absence of a matching
            // arm, mirrors `RegressionTrend`'s own
            // `REGRESSION_RESULT_STATS_ID` arm.
            CANDLE_PATTERN_RESULT_STATS_ID => {}
            "kind_id" => {
                if let Some(s) = value.as_string() {
                    // Unknown id: leave the field unchanged rather than
                    // storing a value `CandlePatternKind::from_id` cannot
                    // resolve.
                    if CandlePatternKind::from_id(s).is_some() {
                        self.kind_id = s.to_string();
                        return true;
                    }
                }
            }
            "show_labels" => { if let Some(v) = value.as_bool() { self.show_labels = v; return true; } }
            _ => {}
        }
        false
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "candle_pattern",
        display_name: "Candle Pattern",
        kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::SingleClick,
        tooltip: "Claim a candlestick pattern completes on this bar",
        icon: "candle_pattern",
        default_color: "#00BCD4",
        factory: |points, color| {
            let (t, _) = points.first().copied().unwrap_or((0, 0.0));
            Box::new(CandlePattern::new(t, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Six bars: a textbook bullish engulfing at indices 4..5 (a large
    /// bearish candle at 4 fully engulfed by a large bullish candle at 5),
    /// padded with plain flat candles so `bars_needed() == 1` claims (Doji)
    /// have somewhere harmless to anchor too.
    fn bars_with_engulfing() -> Vec<Bar> {
        vec![
            Bar::new(0, 100.0, 101.0, 99.0, 100.5),
            Bar::new(1, 100.5, 101.5, 99.5, 100.8),
            Bar::new(2, 100.8, 101.8, 99.8, 101.0),
            Bar::new(3, 101.0, 102.0, 100.0, 101.2),
            Bar::new(4, 105.0, 106.0, 99.0, 100.0), // bearish, open 105 close 100
            Bar::new(5, 99.0, 110.0, 98.0, 108.0),  // bullish, engulfs bar 4
        ]
    }

    /// Every test below reads or (for the last one) writes the shared
    /// `candle_pattern_engine_config()` global via `refresh_bar_derived_cache`/
    /// `CandlePattern::new` — `crate::state::candle_pattern_engine::
    /// engine_config_test_lock` serializes them against every other test in
    /// this crate that touches the same global (`cargo test`'s default
    /// parallel runner otherwise interleaves two such tests on separate OS
    /// threads with no isolation between them), and the explicit reset
    /// resets to the documented default REGARDLESS of what a previous test
    /// left behind, guard held or not.
    fn reset_engine_config() -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::state::candle_pattern_engine::engine_config_test_lock();
        crate::state::candle_pattern_engine::set_candle_pattern_engine_config(
            crate::state::candle_pattern_engine::CandlePatternEngineConfig::default(),
        );
        guard
    }

    /// #1: a claim over bars that DO form the pattern reports no violation.
    #[test]
    fn claim_matching_the_bars_reports_no_violation() {
        let _guard = reset_engine_config();
        let bars = bars_with_engulfing();
        let mut claim = CandlePattern::new(5_000, "#00BCD4");
        claim.kind_id = "bullish_engulfing".to_string();
        claim.refresh_bar_derived_cache(&bars);
        assert_eq!(claim.cached_window, Some((4, 5)));
        assert_eq!(claim.cached_detected, Some(mli::Direction::Up));
        assert!(claim.context_violations().is_empty(), "{:?}", claim.context_violations());
    }

    /// #1 (continued): the SAME claim over bars that do not form it reports
    /// `candle_pattern_not_formed`.
    #[test]
    fn claim_not_matching_the_bars_reports_not_formed() {
        let _guard = reset_engine_config();
        let bars = bars_with_engulfing();
        let mut claim = CandlePattern::new(2_000, "#00BCD4"); // bars 1..2 — no engulfing there
        claim.kind_id = "bullish_engulfing".to_string();
        claim.refresh_bar_derived_cache(&bars);
        assert_eq!(claim.cached_window, Some((1, 2)));
        assert_eq!(claim.cached_detected, None);
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "candle_pattern_not_formed");
    }

    /// #2: a claim placed too close to the start of the series reports
    /// `candle_pattern_window_incomplete`, not a panic.
    #[test]
    fn claim_too_close_to_the_start_reports_window_incomplete_not_a_panic() {
        let _guard = reset_engine_config();
        let bars = bars_with_engulfing();
        let mut claim = CandlePattern::new(0, "#00BCD4"); // bar 0 — a 2-bar pattern needs a predecessor
        claim.kind_id = "bullish_engulfing".to_string();
        claim.refresh_bar_derived_cache(&bars);
        assert_eq!(claim.cached_window, None);
        assert_eq!(claim.cached_detected, None);
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "candle_pattern_window_incomplete");
    }

    /// Empty bars: never panics, resolves to the incomplete case.
    #[test]
    fn empty_bars_never_panics() {
        let _guard = reset_engine_config();
        let mut claim = CandlePattern::new(0, "#00BCD4");
        claim.refresh_bar_derived_cache(&[]);
        assert_eq!(claim.cached_window, None);
        assert!(!claim.context_violations().is_empty());
    }

    /// `CandlePatternKey::from_id` resolves every one of `mli`'s 34 ids —
    /// catches the two catalogues (`mli::events::candle_pattern::
    /// CandlePatternKind`'s own wire ids, this crate's `CandlePatternKey`
    /// i18n table) drifting apart.
    #[test]
    fn candle_pattern_key_resolves_every_mli_id() {
        for kind in CandlePatternKind::all() {
            assert!(CandlePatternKey::from_id(kind.id()).is_some(), "missing i18n key for {}", kind.id());
        }
    }

    /// #6 (round trip): the render cache is ABSENT from `to_json` — none of
    /// `cached_window`/`cached_detected`/`cached_violations`' own field
    /// names appear in the serialized string.
    #[test]
    fn render_cache_is_absent_from_to_json() {
        let _guard = reset_engine_config();
        let bars = bars_with_engulfing();
        let mut claim = CandlePattern::new(5_000, "#00BCD4");
        claim.kind_id = "bullish_engulfing".to_string();
        claim.refresh_bar_derived_cache(&bars);
        let json = claim.to_json();
        assert!(!json.contains("cached_window"), "{json}");
        assert!(!json.contains("cached_detected"), "{json}");
        assert!(!json.contains("cached_violations"), "{json}");

        let mut registry = super::super::super::registry::PrimitiveRegistry::new();
        registry.register_typed::<CandlePattern>(metadata());
        let restored = registry.from_json("candle_pattern", &json).expect("round trip");
        assert_eq!(restored.points(), claim.points());
    }

    /// `judge_claims = false` (engine config opt-out) silences the guard
    /// rules while still resolving the window/outcome — a claim placed
    /// where the bars do NOT form the pattern (see
    /// `claim_not_matching_the_bars_reports_not_formed`) would normally
    /// report `"candle_pattern_not_formed"`; with the opt-out set it
    /// reports nothing, even though `cached_window`/`cached_detected`
    /// still resolve exactly as before.
    #[test]
    fn judge_claims_off_silences_the_guard() {
        use crate::state::candle_pattern_engine::{set_candle_pattern_engine_config, CandlePatternEngineConfig};
        let _guard = reset_engine_config();
        let bars = bars_with_engulfing();
        let mut claim = CandlePattern::new(2_000, "#00BCD4"); // bars 1..2 — no engulfing there
        claim.kind_id = "bullish_engulfing".to_string();

        let mut cfg = CandlePatternEngineConfig::default();
        cfg.judge_claims = false;
        set_candle_pattern_engine_config(cfg);
        claim.refresh_bar_derived_cache(&bars);
        assert_eq!(claim.cached_window, Some((1, 2)), "the window still resolves");
        assert_eq!(claim.cached_detected, None, "the outcome still resolves — the bars still don't match");
        assert!(claim.context_violations().is_empty(), "judging itself is off");
    }
}
