//! AMD Cycle — accumulation / manipulation / distribution
//! (`docs/mlc/plans/markup-engines-2026-08-25.md`, Appendix A, "AMD").
//!
//! ONE point: the MANIPULATION bar — the liquidity sweep. Appendix A: "not
//! a fourth detector but an ASSEMBLY... if the three parts do not come from
//! the three detectors that already found them, the engine has two
//! implementations of the same fact." Nothing about the three phases is
//! stored here either: the whole cycle — accumulation span, the
//! manipulation sweep, and the distribution leg — is DERIVED, every refresh,
//! from `mli::smc::amd::amd_cycles` run over the bounded judgement window
//! (`crate::state::smc_engine::SmcEngineConfig::judge_lookback_bars`,
//! resolved via [`super::smc_judge::judge_window`] — the SAME forward-
//! unbounded window `order_block`/`fair_value_gap`/`liquidity_sweep`/
//! `structure_break` use, unlike `dealing_range`: the distribution phase is
//! a REACTION that happens AFTER the manipulation bar, so the window must
//! keep looking forward from it).
//!
//! Closes a standing defect: `SmcEngineConfig::amd_max_distribution_lag` was
//! carried with no reader through the earlier SMC wave (see that field's
//! own doc in `state::smc_engine`) — this primitive is that reader, via
//! `SmcEngineConfig::to_smc_config`, which now passes the value straight to
//! `mli::smc::amd::amd_cycles` instead of pinning it to `mli`'s default.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64, find_bar_for_timestamp_ms};
use crate::i18n::{current_language, ConceptKey};
use crate::state::smc_engine::smc_engine_config;
use mli::smc as mli_smc;
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, render_primitive_text, TextAlign,
    config::{ConfigProperty, PropertyValue, StatRow},
    elliott::guard::{self, Violation},
};
use super::smc_judge::judge_window;
use super::super::markup;
use super::render_shapes::{draw_zone, draw_level, draw_leg};

fn default_true() -> bool {
    true
}

fn violations_for(detected: Option<&mli_smc::AmdCycle>) -> Vec<Violation> {
    if detected.is_some() {
        Vec::new()
    } else {
        vec![Violation::whole("amd_cycle_not_formed", ConceptKey::RuleAmdCycleNotFormed)]
    }
}

const AMD_CYCLE_RESULT_STATS_ID: &str = "amd_cycle_result_stats";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AmdCycle {
    pub data: PrimitiveData,
    /// Timestamp (ms) of the bar the analyst claims is the manipulation
    /// (liquidity-sweep) bar — see the module's own doc.
    #[serde(default)]
    pub anchor_ts: i64,
    /// Whether the three localized phase labels draw on the chart.
    #[serde(default = "default_true")]
    pub show_labels: bool,

    /// This claim's own resolved cycle, with every bar index already
    /// converted back to GLOBAL — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    #[serde(skip)]
    pub cached_cycle: Option<mli_smc::AmdCycle>,
    /// This claim's own guard rule — see [`Primitive::context_violations`]'s
    /// own contract.
    #[serde(skip)]
    pub cached_violations: Vec<Violation>,
}

impl AmdCycle {
    pub fn new(anchor_ts: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "amd_cycle".to_string(),
                display_name: "AMD Cycle".to_string(),
                color: PrimitiveColor::with_fill(color, color),
                width: 2.0,
                ..Default::default()
            },
            anchor_ts,
            show_labels: true,
            cached_cycle: None,
            cached_violations: Vec::new(),
        }
    }

    fn anchor_bar_idx(&self, bars: &[Bar]) -> Option<usize> {
        let idx = find_bar_for_timestamp_ms(bars, self.anchor_ts)?;
        (idx < bars.len()).then_some(idx)
    }

    /// This claim's own resolved cycle — `mli::smc::amd::amd_cycles` run
    /// over the bounded judgement window ([`judge_window`]), filtered to
    /// the one cycle whose own `manipulation.index` lands exactly on the
    /// anchor bar. Every bar index on the returned value is already
    /// GLOBAL — the window's own local indices never leak past this
    /// method, mirroring every sibling claim in this family.
    pub fn detect(&self, bars: &[Bar]) -> Option<mli_smc::AmdCycle> {
        let anchor_idx = self.anchor_bar_idx(bars)?;
        let engine_cfg = smc_engine_config();
        let (window, offset) = judge_window(bars, anchor_idx, engine_cfg.judge_lookback_bars);
        let cfg = engine_cfg.to_smc_config();
        let local_target = anchor_idx.checked_sub(offset)?;
        mli_smc::amd_cycles(&window, &cfg)
            .into_iter()
            .find(|c| c.manipulation.index == local_target)
            .map(|c| mli_smc::AmdCycle {
                accumulation: (c.accumulation.start + offset)..(c.accumulation.end + offset),
                manipulation: offset_sweep(&c.manipulation, offset),
                distribution: offset_leg(&c.distribution, offset),
            })
    }

    /// The read-only Stat block's own rows — the accumulation span, the
    /// manipulation span (wick bar through its own recovery bar), the
    /// distribution span, and the swept level — all read from
    /// [`Self::cached_cycle`].
    pub fn result_stat_rows(&self) -> Vec<StatRow> {
        let lang = current_language();
        let Some(cycle) = &self.cached_cycle else { return Vec::new() };
        let accumulation_value = format!("{} - {}", cycle.accumulation.start, cycle.accumulation.end);
        let manipulation_value = format!(
            "{} - {}",
            cycle.manipulation.index,
            cycle.manipulation.index + cycle.manipulation.recovered_within,
        );
        let distribution_value = format!("{} - {}", cycle.distribution.start_index, cycle.distribution.end_index);
        let swept_level_value = crate::drawing::primitives_v2::fmt_price(cycle.manipulation.level);
        vec![
            StatRow::new(ConceptKey::AccumulationLabel.get(lang), &accumulation_value, None),
            StatRow::new(ConceptKey::ManipulationLabel.get(lang), &manipulation_value, None),
            StatRow::new(ConceptKey::DistributionLabel.get(lang), &distribution_value, None),
            StatRow::new(ConceptKey::SweptLevelLabel.get(lang), &swept_level_value, None),
        ]
    }
}

/// Shift a resolved sweep's own bar indices from window-local to GLOBAL —
/// shared by [`AmdCycle::detect`] and `smc_scan::SmcScan`'s own
/// `liquidity_sweep`/`amd_cycle` hit resolution, so the two never carry two
/// independent copies of the same offset arithmetic.
pub(super) fn offset_sweep(sweep: &mli_smc::LiquiditySweep, offset: usize) -> mli_smc::LiquiditySweep {
    mli_smc::LiquiditySweep {
        index: sweep.index + offset,
        swept_swing_index: sweep.swept_swing_index + offset,
        level: sweep.level,
        direction: sweep.direction,
        recovered_within: sweep.recovered_within,
    }
}

/// Shift a resolved displacement leg's own bar indices from window-local to
/// GLOBAL — shared the same way [`offset_sweep`] is.
pub(super) fn offset_leg(leg: &mli_smc::DisplacementLeg, offset: usize) -> mli_smc::DisplacementLeg {
    mli_smc::DisplacementLeg {
        start_index: leg.start_index + offset,
        end_index: leg.end_index + offset,
        direction: leg.direction,
        range: leg.range,
        atr_at_start: leg.atr_at_start,
    }
}

/// The two (x, y) endpoints a displacement leg's own line draws between —
/// shared by [`AmdCycle::render`] and `smc_scan::SmcScan`'s own
/// `displacement_leg`/`amd_cycle` hit rendering: from the run's own
/// starting extreme (low for an up leg, high for a down leg) to its ending
/// extreme (the mirror), so the line visually points the direction the leg
/// actually ran.
pub(super) fn leg_endpoints(ctx: &dyn RenderContext, leg: &mli_smc::DisplacementLeg) -> Option<(f64, f64, f64, f64)> {
    let bars = ctx.bars();
    let start_bar = bars.get(leg.start_index)?;
    let end_bar = bars.get(leg.end_index)?;
    let (y1, y2) = match leg.direction {
        mli_smc::Direction::Up => (start_bar.low, end_bar.high),
        mli_smc::Direction::Down => (start_bar.high, end_bar.low),
    };
    Some((leg.start_index as f64, y1, leg.end_index as f64, y2))
}

impl Primitive for AmdCycle {
    fn type_id(&self) -> &'static str { "amd_cycle" }
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

    /// The anchor handle, then the resolved cycle's own combined bounding
    /// box (accumulation range through the distribution leg's own end).
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let anchor_x = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.anchor_ts));
        if (sx - anchor_x).abs() < HIT_TOLERANCE {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        let Some(cycle) = self.detect(bars) else { return HitTestResult::Miss };
        let left = cycle.accumulation.start.min(cycle.distribution.start_index);
        let right = cycle.distribution.end_index.max(cycle.manipulation.index);
        let x1 = vp.bar_to_x_f64(left as f64);
        let x2 = vp.bar_to_x_f64(right as f64);
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        if sx < min_x - HIT_TOLERANCE || sx > max_x + HIT_TOLERANCE {
            return HitTestResult::Miss;
        }
        let prices: Vec<f64> = (left..=right).filter_map(|i| bars.get(i)).flat_map(|b| [b.high, b.low]).collect();
        let (Some(&lo), Some(&hi)) = (
            prices.iter().min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)),
            prices.iter().max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)),
        ) else {
            return HitTestResult::Miss;
        };
        let y1 = vp.price_to_y(lo, ps.price_min, ps.price_max);
        let y2 = vp.price_to_y(hi, ps.price_min, ps.price_max);
        let (min_y, max_y) = (y1.min(y2), y1.max(y2));
        if sy >= min_y - HIT_TOLERANCE && sy <= max_y + HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let x = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.anchor_ts));
        let cy = match self.detect(bars) {
            Some(cycle) => vp.price_to_y(cycle.manipulation.level, ps.price_min, ps.price_max),
            None => super::super::anchor_mark::anchor_control_y(bars, self.anchor_ts, vp, ps),
        };
        vec![ControlPoint::point1(x, cy)]
    }

    /// The three phases, each drawn distinctly: accumulation as a dashed
    /// box, manipulation as the sweep's own level segment + marker,
    /// distribution as the leg's own diagonal — reusing
    /// [`super::render_shapes`]'s shared zone/level/leg helpers (see that
    /// module's own doc for why they exist rather than reaching into the
    /// committed `liquidity_sweep`/`zones` files). Tinted
    /// `guard::VIOLATION_COLOR` when nothing resolves AND
    /// `SmcEngineConfig::judge_claims` is on.
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let bars = ctx.bars();
        let detected = self.detect(bars);
        let anchor_span = super::super::anchor_mark::bar_span_at(bars, self.anchor_ts);
        let judge_claims = smc_engine_config().judge_claims;
        let violated = judge_claims && detected.is_none();
        let stroke = if violated { guard::VIOLATION_COLOR.to_string() } else { self.data.color.stroke.clone() };
        let chart_height = ctx.chart_height();

        let Some(cycle) = detected else {
            let anchor_x = ctx.ts_to_x_ms(self.anchor_ts);
            super::super::anchor_mark::draw_unresolved_anchor(
                ctx, anchor_x, anchor_span, &stroke, self.data.width,
            );
            return;
        };

        let lang = current_language();

        // Accumulation: dashed box over its own bar span, price bounds the
        // extremes of every bar inside it.
        if let (Some(acc_lo), Some(acc_hi)) = accumulation_bounds(bars, &cycle.accumulation) {
            let x1 = ctx.bar_to_x(cycle.accumulation.start as f64);
            let x2 = ctx.bar_to_x(cycle.accumulation.end as f64);
            let y1 = ctx.price_to_y(acc_hi);
            let y2 = ctx.price_to_y(acc_lo);
            draw_zone(ctx, x1, x2, y1, y2, &stroke, 0.12, true);
            if self.show_labels {
                label_at(ctx, (x1 + x2) / 2.0, y1 - 6.0 * dpr, ConceptKey::AccumulationLabel.get(lang), &stroke);
            }
        }

        // Manipulation: the sweep's own level segment + marker.
        let x1 = ctx.bar_to_x(cycle.manipulation.swept_swing_index as f64);
        let x2 = ctx.bar_to_x(cycle.manipulation.index as f64);
        let y = ctx.price_to_y(cycle.manipulation.level);
        draw_level(ctx, x1, x2, y, &stroke, true);
        if self.show_labels {
            // Anchored to the RIGHT of the sweep bar, not centred on it: the
            // sweep that manipulates an accumulation range sits at that
            // range's own edge by definition, so a centred label lands on
            // top of the "Accumulation" one drawn just above the box
            // (observed on the live stand). Left-aligning past the sweep bar
            // keeps the three phase labels apart however tight the cycle is.
            label_right_of(ctx, x2 + 6.0 * dpr, y, ConceptKey::ManipulationLabel.get(lang), &stroke);
        }

        // Distribution: the leg's own diagonal.
        if let Some((lx1, ly1, lx2, ly2)) = leg_endpoints(ctx, &cycle.distribution) {
            let sx1 = ctx.bar_to_x(lx1);
            let sy1 = ctx.price_to_y(ly1);
            let sx2 = ctx.bar_to_x(lx2);
            let sy2 = ctx.price_to_y(ly2);
            draw_leg(ctx, sx1, sy1, sx2, sy2, &stroke);
            if self.show_labels {
                label_at(ctx, (sx1 + sx2) / 2.0, (sy1 + sy2) / 2.0 - 6.0 * dpr, ConceptKey::DistributionLabel.get(lang), &stroke);
            }
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let anchor_x = ctx.ts_to_x_ms(self.anchor_ts);
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => anchor_x - text_offset,
                    TextAlign::Center => anchor_x,
                    TextAlign::End => anchor_x + text_offset,
                };
                let text_y = match text.v_align {
                    TextAlign::Start => 0.0 + text_offset,
                    TextAlign::Center => chart_height / 2.0,
                    TextAlign::End => chart_height - text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &stroke);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    /// Recompute [`Self::cached_cycle`]/[`Self::cached_violations`] from
    /// `bars` through the SAME [`Self::detect`] call `render` makes
    /// directly — see [`Primitive::refresh_bar_derived_cache`]'s own
    /// contract.
    fn refresh_bar_derived_cache(&mut self, bars: &[Bar]) {
        let detected = self.detect(bars);
        self.cached_violations = if smc_engine_config().judge_claims {
            violations_for(detected.as_ref())
        } else {
            Vec::new()
        };
        self.cached_cycle = detected;
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

    /// Which of this cycle's three phases the point `(ts_ms, price)` falls
    /// in ([`Primitive::slot_containing`]).
    ///
    /// Price is ignored: an AMD phase is a stretch of TIME, not a band —
    /// the accumulation is a range of bars, the manipulation is the sweep
    /// bar through its own recovery, the distribution is the displacement
    /// leg. This is the second half of why the shared layer asks the parent
    /// instead of reading a span off it (see `markup::link`'s module doc):
    /// two containers in the same family answer the same question from
    /// entirely different geometry.
    ///
    /// Tested narrowest-first. The phases abut and the manipulation's own
    /// recovery window may reach into the distribution leg, so a bar inside
    /// both is reported as the manipulation — the more specific claim, and
    /// the one a trader would name.
    fn slot_containing(&self, bars: &[Bar], ts_ms: i64, _price: f64) -> Option<markup::SlotId> {
        let cycle = self.cached_cycle.as_ref()?;
        let idx = find_bar_for_timestamp_ms(bars, ts_ms)?;
        let manipulation_end = cycle.manipulation.index + cycle.manipulation.recovered_within;
        if (cycle.manipulation.index..=manipulation_end).contains(&idx) {
            return Some(markup::slots::MANIPULATION);
        }
        if (cycle.distribution.start_index..=cycle.distribution.end_index).contains(&idx) {
            return Some(markup::slots::DISTRIBUTION);
        }
        if (cycle.accumulation.start..=cycle.accumulation.end).contains(&idx) {
            return Some(markup::slots::ACCUMULATION);
        }
        None
    }

    /// The resolved cycle's own first-to-last bar, as timestamps — see
    /// [`Primitive::markup_span`].
    fn markup_span(&self, bars: &[Bar]) -> Option<(i64, i64)> {
        let cycle = self.cached_cycle.as_ref()?;
        let last = cycle
            .distribution
            .end_index
            .max(cycle.manipulation.index + cycle.manipulation.recovered_within);
        let from = bars.get(cycle.accumulation.start)?.timestamp * 1000;
        let to = bars.get(last)?.timestamp * 1000;
        Some((from, to))
    }

    /// What this claim contributes to its family's own chain rules — see
    /// [`Primitive::chain_fact`].
    fn chain_fact(&self, bars: &[Bar]) -> Option<markup::ChainFact> {
        let bar = self.anchor_bar_idx(bars)?;
        // The cycle speaks through its manipulation phase — that IS the
        // sweep, and it is the event the narrative continues from.
        Some(markup::ChainFact {
            bar,
            bias: self.cached_cycle.as_ref().map(|c| c.manipulation.direction.into()),
            variant: None,
            origin_event_bar: None,
        })
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

    fn style_properties(&self) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::show_labels(self.show_labels).with_order(10),
            ConfigProperty::stat_block(AMD_CYCLE_RESULT_STATS_ID, self.result_stat_rows()).with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            AMD_CYCLE_RESULT_STATS_ID => {}
            "show_labels" => { if let Some(v) = value.as_bool() { self.show_labels = v; return true; } }
            _ => {}
        }
        false
    }
}

fn accumulation_bounds(bars: &[Bar], span: &std::ops::Range<usize>) -> (Option<f64>, Option<f64>) {
    let slice = bars.get(span.start..=span.end.min(bars.len().saturating_sub(1)));
    let Some(slice) = slice else { return (None, None) };
    if slice.is_empty() { return (None, None); }
    let lo = slice.iter().fold(f64::INFINITY, |m, b| m.min(b.low));
    let hi = slice.iter().fold(f64::NEG_INFINITY, |m, b| m.max(b.high));
    (Some(lo), Some(hi))
}

/// A phase label anchored by its own LEFT edge and vertically centred on
/// `y` — for a label that has to sit beside a level rather than above or
/// below it. See the manipulation phase's own call for why.
fn label_right_of(ctx: &mut dyn RenderContext, x: f64, y: f64, text: &str, color: &str) {
    ctx.set_fill_color(color);
    ctx.set_font("10px sans-serif");
    ctx.set_text_align(crate::render::TextAlign::Left);
    ctx.set_text_baseline(crate::render::TextBaseline::Middle);
    ctx.fill_text(text, x, y);
}

fn label_at(ctx: &mut dyn RenderContext, x: f64, y: f64, text: &str, color: &str) {
    ctx.set_fill_color(color);
    ctx.set_font("10px sans-serif");
    ctx.set_text_align(crate::render::TextAlign::Center);
    ctx.set_text_baseline(crate::render::TextBaseline::Bottom);
    ctx.fill_text(text, x, y);
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "amd_cycle",
        display_name: "AMD Cycle",
        kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::SingleClick,
        tooltip: "Claim an accumulation/manipulation/distribution cycle at this bar",
        icon: "amd_cycle",
        default_color: "#FFCA28",
        factory: |points, color| {
            let (t, _) = points.first().copied().unwrap_or((0, 0.0));
            Box::new(AmdCycle::new(t, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reproduces `mli::smc::amd`'s own `amd_fixture`: an uptrend, a
    /// confirmed swing high at bar 7, a liquidity sweep of it at bar 9, and
    /// a down displacement leg over bars 10..12.
    fn amd_fixture_bars() -> Vec<Bar> {
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
            Bar::new(9, 25.0, 27.0, 24.0, 25.5),
            Bar::new(10, 25.0, 25.5, 19.0, 20.0),
            Bar::new(11, 20.0, 20.5, 9.0, 10.0),
            Bar::new(12, 10.0, 10.5, 0.5, 1.0),
        ]
    }

    fn reset_engine_config() -> std::sync::MutexGuard<'static, ()> {
        use crate::state::smc_engine::{engine_config_test_lock, set_smc_engine_config, SmcEngineConfig};
        let guard = engine_config_test_lock();
        set_smc_engine_config(SmcEngineConfig::default());
        guard
    }

    fn set_fixture_config() {
        use crate::state::smc_engine::{set_smc_engine_config, SmcEngineConfig};
        set_smc_engine_config(SmcEngineConfig { swing_n: 1, atr_period: 3, ..SmcEngineConfig::default() });
    }

    /// The anchor on the fixture's own sweep bar resolves the three phases,
    /// and they are IDENTICAL — not merely present — to what
    /// `mli::smc::amd::amd_cycles` itself returns over the same bars.
    #[test]
    fn claim_on_the_real_manipulation_bar_matches_mli_identically() {
        let _guard = reset_engine_config();
        set_fixture_config();
        let bars = amd_fixture_bars();
        let mut claim = AmdCycle::new(9_000, "#FFCA28");
        claim.refresh_bar_derived_cache(&bars);
        let cycle = claim.cached_cycle.clone().expect("expected a resolved AMD cycle");

        let cfg = smc_engine_config().to_smc_config();
        let smc_bars: Vec<mli_smc::SmcBar> =
            bars.iter().map(|b| mli_smc::SmcBar { open: b.open, high: b.high, low: b.low, close: b.close }).collect();
        let direct = mli_smc::amd_cycles(&smc_bars, &cfg)
            .into_iter()
            .find(|c| c.manipulation.index == 9)
            .expect("expected mli to resolve the same cycle directly");

        assert_eq!(cycle, direct, "AmdCycle's own resolution must be IDENTICAL to mli's own output, not merely present");
        assert!(claim.context_violations().is_empty(), "{:?}", claim.context_violations());
    }

    #[test]
    fn claim_on_an_ordinary_bar_reports_not_formed() {
        let _guard = reset_engine_config();
        set_fixture_config();
        let bars = amd_fixture_bars();
        let mut claim = AmdCycle::new(4_000, "#FFCA28"); // no sweep here
        claim.refresh_bar_derived_cache(&bars);
        assert!(claim.cached_cycle.is_none());
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "amd_cycle_not_formed");
    }

    /// Mirrors `mli::smc::amd`'s own
    /// `a_late_displacement_leg_does_not_become_this_sweeps_distribution`:
    /// the fixture's distribution leg starts one bar after the sweep, so a
    /// zero-bar lag bound must reject the cycle the default (10) accepts.
    #[test]
    fn changing_amd_max_distribution_lag_changes_the_outcome() {
        use crate::state::smc_engine::{set_smc_engine_config, SmcEngineConfig};
        let _guard = reset_engine_config();
        let bars = amd_fixture_bars();
        let mut claim = AmdCycle::new(9_000, "#FFCA28");

        set_smc_engine_config(SmcEngineConfig { swing_n: 1, atr_period: 3, amd_max_distribution_lag: 0, ..SmcEngineConfig::default() });
        claim.refresh_bar_derived_cache(&bars);
        assert!(claim.cached_cycle.is_none(), "a zero-bar lag bound must reject a leg starting one bar after the sweep");

        set_smc_engine_config(SmcEngineConfig { swing_n: 1, atr_period: 3, ..SmcEngineConfig::default() });
        claim.refresh_bar_derived_cache(&bars);
        assert!(claim.cached_cycle.is_some(), "the default lag bound (10) must accept the same leg");
    }

    #[test]
    fn render_cache_is_absent_from_to_json() {
        let _guard = reset_engine_config();
        set_fixture_config();
        let bars = amd_fixture_bars();
        let mut claim = AmdCycle::new(9_000, "#FFCA28");
        claim.refresh_bar_derived_cache(&bars);
        let json = claim.to_json();
        assert!(!json.contains("cached_cycle"), "{json}");
        assert!(!json.contains("cached_violations"), "{json}");

        let mut registry = super::super::super::registry::PrimitiveRegistry::new();
        registry.register_typed::<AmdCycle>(metadata());
        let restored = registry.from_json("amd_cycle", &json).expect("round trip");
        assert_eq!(restored.points(), claim.points());
    }

    #[test]
    fn judge_claims_off_silences_the_guard() {
        use crate::state::smc_engine::{set_smc_engine_config, SmcEngineConfig};
        let _guard = reset_engine_config();
        let bars = amd_fixture_bars();
        let mut claim = AmdCycle::new(4_000, "#FFCA28"); // no cycle here
        set_smc_engine_config(SmcEngineConfig { swing_n: 1, atr_period: 3, judge_claims: false, ..SmcEngineConfig::default() });
        claim.refresh_bar_derived_cache(&bars);
        assert!(claim.cached_cycle.is_none(), "the resolution still runs — the bars still don't qualify");
        assert!(claim.context_violations().is_empty(), "judging itself is off");
    }
}
