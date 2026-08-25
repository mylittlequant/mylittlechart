//! Order Block — the CLAIM half of the smart-money order-block markup
//! primitive (`docs/mlc/plans/markup-engines-2026-08-25.md`, Appendix A,
//! "Order block").
//!
//! ONE point: the bar the analyst claims is the order-block candle. The
//! resolved order block (zone, direction, structure-break bar, breaker
//! state) is DERIVED — `mli::smc::order_blocks` run over a bounded
//! judgement window (`crate::state::smc_engine::SmcEngineConfig::
//! judge_lookback_bars`, resolved via [`super::smc_judge::judge_window`]) —
//! never stored, so a resolved zone can never disagree with what the bars
//! around the anchor actually show.
//!
//! Appendix A: "An order block with no structure break behind it is not an
//! order block." `mli::smc::order_blocks` already enforces exactly that (it
//! never emits a block without the structure break behind it — see that
//! function's own doc), so this claim's own guard rule
//! (`"order_block_no_structure_break"`) fires whenever no qualifying order
//! block resolves at the anchor, for precisely that reason — never a second,
//! independently-computed break check.
//!
//! Direction convention (verified against `mli::smc::zones::order_blocks`'s
//! own fixture, `order_block_has_structure_break_and_later_flips_to_breaker`):
//! an order block's own `direction` is the direction of the displacement leg
//! that follows it, so a BULLISH block (`Direction::Up`) is the last
//! DOWN-close candle before an up leg — Appendix A's own wording, and
//! `mli::smc::zones::is_opposing_close`'s own predicate for `Direction::Up`
//! (`bar.close < bar.open`).

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64, find_bar_for_timestamp_ms};
use crate::i18n::{current_language, ConceptKey};
use crate::state::smc_engine::smc_engine_config;
use mli::smc as mli_smc;
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, render_primitive_text, TextAlign,
    config::{ConfigProperty, PropertyValue, StatRow, Tone},
    elliott::guard::{self, Violation},
    AREA_FILL_OPACITY, default_area_fill_opacity,
};
use super::smc_judge::judge_window;

fn default_true() -> bool {
    true
}

/// This claim's own guard rule — see [`OrderBlock::refresh_bar_derived_cache`]'s
/// own doc for the SAME `detect` call this reads.
fn violations_for(detected: Option<&mli_smc::OrderBlock>) -> Vec<Violation> {
    if detected.is_some() {
        Vec::new()
    } else {
        vec![Violation::whole("order_block_no_structure_break", ConceptKey::RuleOrderBlockNoStructureBreak)]
    }
}

const ORDER_BLOCK_RESULT_STATS_ID: &str = "order_block_result_stats";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderBlock {
    pub data: PrimitiveData,
    /// Timestamp (ms) of the candle the analyst claims is the order block.
    #[serde(default)]
    pub anchor_ts: i64,
    /// Whether the drawn zone extends to the LAST loaded bar regardless of
    /// invalidation (`true`), or stops extending the instant `mli` reports a
    /// breaker transition on this block (`false`) — see
    /// [`OrderBlock::right_edge_bar`]'s own doc. Default `true`.
    #[serde(default = "default_true")]
    pub extend_right: bool,
    /// Zone fill opacity, 0.0-1.0 — the same "barely-there over chart area"
    /// default every zone-shaped primitive in this codebase uses
    /// (`fill_defaults::AREA_FILL_OPACITY`, see `shapes::rectangle::Rectangle`).
    #[serde(default = "default_area_fill_opacity")]
    pub fill_opacity: f64,
    /// Whether the resolved direction label draws on the zone — the
    /// patterns-family's own toggle convention (see
    /// `patterns::candle_pattern::CandlePattern::show_labels`).
    #[serde(default = "default_true")]
    pub show_labels: bool,

    /// This claim's own resolved order block, with every bar index already
    /// converted back to GLOBAL — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    /// `#[serde(skip)]`: a stale resolution surviving a reload/undo would
    /// misreport a zone that no longer matches this primitive's own current
    /// `anchor_ts`.
    #[serde(skip)]
    pub cached_block: Option<mli_smc::OrderBlock>,
    /// This claim's own guard rule — see [`Primitive::context_violations`]'s
    /// own contract.
    #[serde(skip)]
    pub cached_violations: Vec<Violation>,
}

impl OrderBlock {
    pub fn new(anchor_ts: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "order_block".to_string(),
                display_name: "Order Block".to_string(),
                color: PrimitiveColor::with_fill(color, color),
                width: 2.0,
                ..Default::default()
            },
            anchor_ts,
            extend_right: true,
            fill_opacity: AREA_FILL_OPACITY,
            show_labels: true,
            cached_block: None,
            cached_violations: Vec::new(),
        }
    }

    /// The anchor timestamp resolved to a REAL bar index — `None` when
    /// `bars` is empty or the anchor lands strictly beyond the last loaded
    /// bar.
    fn anchor_bar_idx(&self, bars: &[Bar]) -> Option<usize> {
        let idx = find_bar_for_timestamp_ms(bars, self.anchor_ts)?;
        (idx < bars.len()).then_some(idx)
    }

    /// This claim's own resolved order block — `mli::smc::order_blocks` run
    /// over the bounded judgement window ([`judge_window`]), filtered to the
    /// one block whose own `index` lands exactly on the anchor bar. Every
    /// bar index on the returned value is already GLOBAL — the window's own
    /// local indices never leak past this method.
    pub fn detect(&self, bars: &[Bar]) -> Option<mli_smc::OrderBlock> {
        let anchor_idx = self.anchor_bar_idx(bars)?;
        let engine_cfg = smc_engine_config();
        let (window, offset) = judge_window(bars, anchor_idx, engine_cfg.judge_lookback_bars);
        let cfg = engine_cfg.to_smc_config();
        let local_target = anchor_idx.checked_sub(offset)?;
        mli_smc::order_blocks(&window, &cfg)
            .into_iter()
            .find(|b| b.index == local_target)
            .map(|b| mli_smc::OrderBlock {
                index: b.index + offset,
                low: b.low,
                high: b.high,
                direction: b.direction,
                broke_structure_at: b.broke_structure_at + offset,
                breaker_at: b.breaker_at.map(|i| i + offset),
            })
    }

    /// The right edge bar index the drawn/hit-tested zone extends to.
    /// `extend_right == true`: always the last loaded bar, regardless of
    /// whether `mli` has since flipped this block into a breaker.
    /// `extend_right == false`: stops at the breaker bar the instant one
    /// exists, else also the last loaded bar (nothing else would stop it
    /// early). Reusing [`mli_smc::OrderBlock::breaker_at`] rather than a
    /// fixed bar span means the toggle reads directly off the SAME state
    /// this claim's own Stat block already reports, never a second,
    /// independently-chosen cutoff.
    fn right_edge_bar(&self, block: &mli_smc::OrderBlock, bars: &[Bar]) -> usize {
        let last = bars.len().saturating_sub(1);
        if self.extend_right {
            last
        } else {
            block.breaker_at.unwrap_or(last).min(last)
        }
    }

    /// The read-only Stat block's own rows — direction, zone bounds, the
    /// structure-break bar, and the breaker bar (or [`ConceptKey::NoneValue`]
    /// when the block has not flipped) — all read from
    /// [`Self::cached_block`], never a second, independently-computed
    /// resolution. Empty when nothing has resolved (no design point in
    /// showing zone bounds for a zone that does not exist).
    pub fn result_stat_rows(&self) -> Vec<StatRow> {
        let lang = current_language();
        let Some(block) = &self.cached_block else { return Vec::new() };
        let direction_value = match block.direction {
            mli_smc::Direction::Up => ConceptKey::PatternBiasBullish.get(lang),
            mli_smc::Direction::Down => ConceptKey::PatternBiasBearish.get(lang),
        };
        let zone_value = format!(
            "{} - {}",
            crate::drawing::primitives_v2::fmt_price(block.low),
            crate::drawing::primitives_v2::fmt_price(block.high),
        );
        let breaker_value = match block.breaker_at {
            Some(i) => i.to_string(),
            None => ConceptKey::NoneValue.get(lang).to_string(),
        };
        vec![
            StatRow::new(ConceptKey::SmcDirectionLabel.get(lang), direction_value, None),
            StatRow::new(ConceptKey::SmcZoneLabel.get(lang), &zone_value, None),
            StatRow::new(ConceptKey::OrderBlockBreakFollowsLabel.get(lang), &block.broke_structure_at.to_string(), None),
            StatRow::new(
                ConceptKey::BreakerBarLabel.get(lang),
                &breaker_value,
                Some(if block.breaker_at.is_some() { Tone::Bad } else { Tone::Good }),
            ),
        ]
    }
}

impl Primitive for OrderBlock {
    fn type_id(&self) -> &'static str { "order_block" }
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

    /// The anchor handle (any Y, matching `CandlePattern::hit_test`'s own
    /// column test), then the resolved zone's own bounding box when one
    /// resolves.
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let anchor_x = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.anchor_ts));
        if (sx - anchor_x).abs() < HIT_TOLERANCE {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        let Some(block) = self.detect(bars) else { return HitTestResult::Miss };
        let anchor_idx = self.anchor_bar_idx(bars).unwrap_or(0);
        let right_bar = self.right_edge_bar(&block, bars);
        let x1 = vp.bar_to_x_f64(anchor_idx as f64);
        let x2 = vp.bar_to_x_f64(right_bar as f64);
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        let y1 = vp.price_to_y(block.low, ps.price_min, ps.price_max);
        let y2 = vp.price_to_y(block.high, ps.price_min, ps.price_max);
        let (min_y, max_y) = (y1.min(y2), y1.max(y2));
        if sx >= min_x - HIT_TOLERANCE && sx <= max_x + HIT_TOLERANCE
            && sy >= min_y - HIT_TOLERANCE && sy <= max_y + HIT_TOLERANCE
        {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let x = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.anchor_ts));
        let cy = match self.detect(bars) {
            Some(block) => vp.price_to_y((block.low + block.high) / 2.0, ps.price_min, ps.price_max),
            None => vp.price_to_y((ps.price_min + ps.price_max) / 2.0, ps.price_min, ps.price_max),
        };
        vec![ControlPoint::point1(x, cy)]
    }

    /// A filled/stroked box over the resolved zone, tinted
    /// `guard::VIOLATION_COLOR` when nothing resolves AND
    /// `SmcEngineConfig::judge_claims` is on — same gate
    /// [`Self::refresh_bar_derived_cache`] reads. Recomputed FRESH from
    /// `bars` here (never reads [`Self::cached_block`]), matching
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let bars = ctx.bars();
        let detected = self.detect(bars);
        let anchor_span = super::super::anchor_mark::bar_span_at(bars, self.anchor_ts);
        let judge_claims = smc_engine_config().judge_claims;
        let violated = judge_claims && detected.is_none();
        let stroke = if violated { guard::VIOLATION_COLOR.to_string() } else { self.data.color.stroke.clone() };

        let Some(block) = detected else {
            // Nothing resolved: draw just the anchor tick, still recolored
            // when judging is on — the same "still draws, still recolors,
            // even with no resolved geometry" shape
            // `CandlePattern::render`'s own doc describes.
            let anchor_x = ctx.ts_to_x_ms(self.anchor_ts);
            super::super::anchor_mark::draw_unresolved_anchor(
                ctx, anchor_x, anchor_span, &stroke, self.data.width,
            );
            return;
        };

        let anchor_idx = self.anchor_bar_idx(bars).unwrap_or(0);
        let right_bar = self.right_edge_bar(&block, bars);
        let x1 = ctx.bar_to_x(anchor_idx as f64);
        let x2 = ctx.bar_to_x(right_bar as f64);
        let y1 = ctx.price_to_y(block.low);
        let y2 = ctx.price_to_y(block.high);
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        let (min_y, max_y) = (y1.min(y2), y1.max(y2));
        let width = max_x - min_x;
        let height = max_y - min_y;

        let fill_color = format!("{}{:02x}", &stroke[..7], (self.fill_opacity * 255.0) as u8);
        ctx.set_fill_color(&fill_color);
        ctx.fill_rect(min_x, min_y, width, height);

        ctx.set_stroke_color(&stroke);
        ctx.set_stroke_width(self.data.width);
        ctx.set_line_dash(&[]);
        ctx.stroke_rect(crisp(min_x, dpr), crisp(min_y, dpr), width, height);

        if self.show_labels {
            let lang = current_language();
            let label = match block.direction {
                mli_smc::Direction::Up => ConceptKey::PatternBiasBullish.get(lang),
                mli_smc::Direction::Down => ConceptKey::PatternBiasBearish.get(lang),
            };
            ctx.set_fill_color(&stroke);
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(crate::render::TextAlign::Left);
            ctx.set_text_baseline(crate::render::TextBaseline::Bottom);
            ctx.fill_text(label, min_x + 2.0 * dpr, min_y - 2.0 * dpr);
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
                    TextAlign::Start => min_y - text_offset,
                    TextAlign::Center => (min_y + max_y) / 2.0,
                    TextAlign::End => max_y + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &stroke);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    /// Recompute [`Self::cached_block`]/[`Self::cached_violations`] from
    /// `bars` through the SAME [`Self::detect`] call `render` makes
    /// directly — see [`Primitive::refresh_bar_derived_cache`]'s own
    /// contract. Gated on `SmcEngineConfig::judge_claims`: when the analyst
    /// has opted out of guard reporting, the cache still carries the
    /// resolved block (the Stat block does not depend on judging), only
    /// [`Self::cached_violations`] stays empty.
    fn refresh_bar_derived_cache(&mut self, bars: &[Bar]) {
        let detected = self.detect(bars);
        self.cached_violations = if smc_engine_config().judge_claims {
            violations_for(detected.as_ref())
        } else {
            Vec::new()
        };
        self.cached_block = detected;
    }

    /// Snap this claim onto the bar its own detector names, within
    /// `claim_snap_bars` — see [`Primitive::snap_to_detected`] for the
    /// contract and why it exists. Searches outward from the placed anchor,
    /// nearest first, and restores the original anchor when nothing inside
    /// the radius resolves.
    fn snap_to_detected(&mut self, bars: &[Bar]) {
        let radius = smc_engine_config().claim_snap_bars;
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
        Some(super::super::markup::MarkupFamily::SmartMoney)
    }

    /// Append-only, by [`Primitive::push_context_violations`]'s own
    /// contract: the cross-primitive pass runs right after this claim's
    /// own cache refresh has rebuilt the self-derived list.
    fn push_context_violations(&mut self, violations: Vec<Violation>) {
        self.cached_violations.extend(violations);
    }

    fn context_violations(&self) -> &[Violation] { &self.cached_violations }

    /// Same measured verdict `CandlePattern::style_properties` already
    /// records: no design-approved archetype names a smart-money-claim
    /// subject tab yet, so every property below stays in its constructor's
    /// own default category/run. Outline (colour/width/line style) needs no
    /// declaration — the universal base-stroke row every primitive without
    /// a level ladder already gets (see `shapes::rectangle::Rectangle`'s own
    /// doc for the identical call on "border").
    fn style_properties(&self) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::extend_right(self.extend_right).with_order(10),
            ConfigProperty::fill_opacity(self.fill_opacity * 100.0).with_order(11),
            ConfigProperty::show_labels(self.show_labels).with_order(12),
            ConfigProperty::stat_block(ORDER_BLOCK_RESULT_STATS_ID, self.result_stat_rows()).with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            ORDER_BLOCK_RESULT_STATS_ID => {}
            "extend_right" => { if let Some(v) = value.as_bool() { self.extend_right = v; return true; } }
            "fill_opacity" => { if let Some(v) = value.as_number() { self.fill_opacity = (v / 100.0).clamp(0.0, 1.0); return true; } }
            "show_labels" => { if let Some(v) = value.as_bool() { self.show_labels = v; return true; } }
            _ => {}
        }
        false
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "order_block",
        display_name: "Order Block",
        kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::SingleClick,
        tooltip: "Claim an order block on this candle",
        icon: "order_block",
        default_color: "#7E57C2",
        factory: |points, color| {
            let (t, _) = points.first().copied().unwrap_or((0, 0.0));
            Box::new(OrderBlock::new(t, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bars reproducing `mli::smc::zones`'s own
    /// `order_block_fixture` (see that module's test for the full layout):
    /// a confirmed uptrend, a down-close candle at bar 9 (the order-block
    /// candidate), and an up displacement leg (bars 10..12) that leaves a
    /// bullish FVG and closes above the prior swing high at bar 12 — the
    /// structure break the order block requires behind it. Bar 13 later
    /// closes back below the block's own low, flipping it into a breaker.
    fn order_block_fixture_bars() -> Vec<Bar> {
        vec![
            Bar::new(0, 20.0, 22.0, 20.0, 21.0),
            Bar::new(1, 18.0, 19.0, 17.0, 18.0),
            Bar::new(2, 20.0, 22.0, 20.0, 21.0),
            Bar::new(3, 23.0, 24.0, 22.0, 23.0),
            Bar::new(4, 21.0, 22.0, 20.0, 21.0),
            Bar::new(5, 19.0, 20.0, 18.0, 19.0),
            Bar::new(6, 21.0, 22.0, 20.0, 21.0),
            Bar::new(7, 25.0, 26.0, 24.0, 25.0),
            Bar::new(8, 21.0, 22.0, 20.0, 21.0),
            Bar::new(9, 21.0, 22.0, 19.0, 20.0),
            Bar::new(10, 20.0, 25.0, 19.0, 24.0),
            Bar::new(11, 24.0, 25.8, 24.0, 25.5),
            Bar::new(12, 30.0, 155.0, 30.0, 150.0),
            Bar::new(13, 15.0, 16.0, 8.0, 9.0),
        ]
    }

    /// Every test below reads or writes the shared `smc_engine_config()`
    /// global via `refresh_bar_derived_cache` — `crate::state::smc_engine::
    /// engine_config_test_lock` serializes them against every other test in
    /// this crate that touches the same global (`cargo test`'s default
    /// parallel runner otherwise interleaves two such tests on separate OS
    /// threads with no isolation between them), and the explicit reset
    /// resets to the documented default REGARDLESS of what a previous test
    /// left behind, guard held or not — same idiom as
    /// `patterns::candle_pattern`'s own `reset_engine_config`.
    fn reset_engine_config() -> std::sync::MutexGuard<'static, ()> {
        use crate::state::smc_engine::{engine_config_test_lock, set_smc_engine_config, SmcEngineConfig};
        let guard = engine_config_test_lock();
        set_smc_engine_config(SmcEngineConfig::default());
        guard
    }

    /// `swing_n = 1` (one bar of margin either side, matching `mli::smc`'s
    /// own fixtures) so the swing structure the fixture bars encode
    /// confirms without needing the default `swing_n = 2` margin; `atr_period
    /// = 3` so the short fixture series has enough bars to warm up ATR for
    /// the displacement-leg check `order_blocks` runs internally — the SAME
    /// override `mli::smc::zones`'s own `order_block_*` tests use.
    fn set_fixture_config() {
        use crate::state::smc_engine::{set_smc_engine_config, SmcEngineConfig};
        set_smc_engine_config(SmcEngineConfig { swing_n: 1, atr_period: 3, ..SmcEngineConfig::default() });
    }

    #[test]
    fn claim_on_the_real_order_block_candle_reports_no_violation() {
        let _guard = reset_engine_config();
        set_fixture_config();
        let bars = order_block_fixture_bars();
        let mut claim = OrderBlock::new(9_000, "#7E57C2");
        claim.refresh_bar_derived_cache(&bars);
        let block = claim.cached_block.expect("expected a resolved order block");
        assert_eq!(block.low, 19.0);
        assert_eq!(block.high, 22.0);
        assert_eq!(block.direction, mli_smc::Direction::Up);
        assert_eq!(block.broke_structure_at, 12);
        assert_eq!(block.breaker_at, Some(13));
        assert!(claim.context_violations().is_empty(), "{:?}", claim.context_violations());
    }

    #[test]
    fn a_claim_placed_one_bar_off_snaps_onto_the_real_block() {
        let _guard = reset_engine_config();
        set_fixture_config();
        let bars = order_block_fixture_bars();
        // Bar 9 is the order block; the analyst clicked bar 8.
        let mut claim = OrderBlock::new(8_000, "#7E57C2");
        assert!(claim.detect(&bars).is_none(), "bar 8 must not resolve on its own");

        claim.snap_to_detected(&bars);

        assert_eq!(claim.anchor_ts, 9_000, "the anchor moves onto the bar the detector names");
        claim.refresh_bar_derived_cache(&bars);
        assert!(claim.context_violations().is_empty());
    }

    #[test]
    fn a_claim_with_nothing_near_it_stays_exactly_where_it_was_placed() {
        let _guard = reset_engine_config();
        set_fixture_config();
        // No structure break anywhere, so no bar in the fixture resolves.
        let mut bars = order_block_fixture_bars();
        bars[12] = Bar::new(12, 30.0, 25.9, 24.0, 25.9);
        let mut claim = OrderBlock::new(8_000, "#7E57C2");

        claim.snap_to_detected(&bars);

        assert_eq!(claim.anchor_ts, 8_000, "nothing inside the radius resolves — the claim keeps its own bar");
    }

    #[test]
    fn snapping_off_leaves_the_anchor_alone() {
        let _guard = reset_engine_config();
        set_fixture_config();
        use crate::state::smc_engine::{set_smc_engine_config, smc_engine_config, SmcEngineConfig};
        set_smc_engine_config(SmcEngineConfig { claim_snap_bars: 0, ..smc_engine_config() });
        let bars = order_block_fixture_bars();
        let mut claim = OrderBlock::new(8_000, "#7E57C2");

        claim.snap_to_detected(&bars);

        assert_eq!(claim.anchor_ts, 8_000);
    }

    #[test]
    fn claim_with_no_structure_break_behind_it_reports_the_rule() {
        let _guard = reset_engine_config();
        set_fixture_config();
        // Same opposing candle (bar 9), but bar 12 never closes above the
        // prior swing high — no structure break, so `mli::smc::order_blocks`
        // never emits a block here (see `mli`'s own
        // `order_block_requires_a_structure_break_behind_it` test).
        let mut bars = order_block_fixture_bars();
        bars[12] = Bar::new(12, 30.0, 25.9, 24.0, 25.9);
        let mut claim = OrderBlock::new(9_000, "#7E57C2");
        claim.refresh_bar_derived_cache(&bars);
        assert!(claim.cached_block.is_none());
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "order_block_no_structure_break");
    }

    #[test]
    fn render_cache_is_absent_from_to_json() {
        let _guard = reset_engine_config();
        set_fixture_config();
        let bars = order_block_fixture_bars();
        let mut claim = OrderBlock::new(9_000, "#7E57C2");
        claim.refresh_bar_derived_cache(&bars);
        let json = claim.to_json();
        assert!(!json.contains("cached_block"), "{json}");
        assert!(!json.contains("cached_violations"), "{json}");

        let mut registry = super::super::super::registry::PrimitiveRegistry::new();
        registry.register_typed::<OrderBlock>(metadata());
        let restored = registry.from_json("order_block", &json).expect("round trip");
        assert_eq!(restored.points(), claim.points());
    }

    #[test]
    fn judge_claims_off_silences_the_guard() {
        use crate::state::smc_engine::{set_smc_engine_config, SmcEngineConfig};
        let _guard = reset_engine_config();
        let mut bars = order_block_fixture_bars();
        bars[12] = Bar::new(12, 30.0, 25.9, 24.0, 25.9); // no structure break
        let mut claim = OrderBlock::new(9_000, "#7E57C2");

        set_smc_engine_config(SmcEngineConfig { swing_n: 1, atr_period: 3, judge_claims: false, ..SmcEngineConfig::default() });
        claim.refresh_bar_derived_cache(&bars);
        assert!(claim.cached_block.is_none(), "the resolution still runs — the bars still don't qualify");
        assert!(claim.context_violations().is_empty(), "judging itself is off");
    }
}
