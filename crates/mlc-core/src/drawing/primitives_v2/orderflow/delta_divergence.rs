//! Delta Divergence — the CLAIM half of the order-flow delta-divergence
//! markup primitive (`docs/mlc/plans/markup-engines-2026-08-25.md`,
//! Appendix B).
//!
//! ONE point: the LATER of the two swing bars the analyst claims a
//! divergence spans. Unlike the other three primitives in this family,
//! this one CLAIMS WHICH KIND it is (`claimed_kind`, a `Pills` row: Regular
//! or Hidden) — the same shape `smc::structure_break` claims BOS vs CHoCH,
//! and for the same reason this primitive carries TWO guard rules instead
//! of one:
//! - `"delta_divergence_not_formed"` — `mli::orderflow::delta_divergences`
//!   reports no divergence at all at this bar.
//! - `"delta_divergence_kind_mismatch"` — a divergence IS resolved there,
//!   but it is the OTHER kind than what was claimed.
//!
//! The only COMPUTED thing in the whole order-flow family: everywhere else
//! reads a flag `FootprintBar::compute_extras` already set. Here, the
//! series fed to `mli::orderflow::delta_divergences` is each footprint
//! bar's own `cumulative_delta` — already computed upstream, never
//! recomputed here — over a bounded judgement window
//! (`OrderFlowEngineConfig::judge_lookback_bars`, resolved via
//! [`super::orderflow_judge::footprint_judge_window`]).
//!
//! `mli::orderflow::delta_divergences` expects `mli::smc::SmcBar`; `smc::
//! smc_judge::judge_window` is `pub(super)` to `smc` and unreachable from
//! here, so this crate's OWN conversion lives in
//! `orderflow_judge::footprint_judge_window` — see that module's own doc.
//! It converts from each FOOTPRINT bar's own OHLC (not the displayed candle
//! series), so `bars`/`series` stay aligned 1:1 by construction regardless
//! of whether the footprint's own bucketing matches the candles on screen.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::types::FootprintBar;
use crate::i18n::{current_language, ConceptKey};
use crate::state::orderflow_engine::orderflow_engine_config;
use mli::events::divergence::DivergenceKind;
use mli::orderflow::DeltaDivergenceHit;
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, render_primitive_text, TextAlign,
    point_to_line_distance,
    config::{ConfigProperty, PropertyValue, StatRow, SelectOption},
    elliott::guard::{self, Violation},
};
use super::orderflow_judge::{find_footprint_bar_for_ts, footprint_judge_window};

fn default_true() -> bool {
    true
}

fn default_claimed_kind() -> String {
    "regular".to_string()
}

/// This claim's own two guard rules — see the module's own doc for why they
/// must stay separate rule ids. Called only once real footprint data is
/// known to be present (see [`Primitive::context_violations`]'s own
/// override for the "no data at all" case).
fn violations_for(claimed_kind: &str, hit: Option<&DeltaDivergenceHit>) -> Vec<Violation> {
    match hit {
        None => vec![Violation::whole("delta_divergence_not_formed", ConceptKey::RuleDeltaDivergenceNotFormed)],
        Some(h) => {
            let matches = matches!(
                (claimed_kind, h.kind),
                ("regular", DivergenceKind::Regular) | ("hidden", DivergenceKind::Hidden)
            );
            if matches {
                Vec::new()
            } else {
                vec![Violation::whole("delta_divergence_kind_mismatch", ConceptKey::RuleDeltaDivergenceKindMismatch)]
            }
        }
    }
}

const DELTA_DIVERGENCE_RESULT_STATS_ID: &str = "delta_divergence_result_stats";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeltaDivergence {
    pub data: PrimitiveData,
    /// Timestamp (ms) of the LATER swing bar the divergence is claimed on.
    #[serde(default)]
    pub anchor_ts: i64,
    /// Which kind the analyst claims — `"regular"` or `"hidden"`.
    #[serde(default = "default_claimed_kind")]
    pub claimed_kind: String,
    /// Whether the claimed-kind label draws on the anchor point.
    #[serde(default = "default_true")]
    pub show_labels: bool,

    /// This claim's own resolved hit, with both bar indices already
    /// converted back to GLOBAL footprint-bar indices — see
    /// [`Primitive::refresh_footprint_derived_cache`]'s own contract.
    #[serde(skip)]
    pub cached_hit: Option<DeltaDivergenceHit>,
    /// The EARLIER swing bar's own `FootprintBar::open_time` — cached
    /// alongside [`Self::cached_hit`] because `render`/`hit_test` have no
    /// footprint of their own to look this timestamp up in (see the
    /// family's own module doc: `RenderContext` carries no footprint
    /// accessor).
    #[serde(skip)]
    pub cached_from_ts: Option<i64>,
    /// See `imbalance_zone::ImbalanceZone::cached_footprint_present`'s own
    /// doc — identical contract, identical reason.
    #[serde(skip)]
    pub cached_footprint_present: bool,
    #[serde(skip)]
    pub cached_violations: Vec<Violation>,
    #[serde(skip)]
    last_seen_footprint_len: Option<usize>,
    #[serde(skip)]
    last_seen_anchor_ts: Option<i64>,
    #[serde(skip)]
    last_seen_claimed_kind: Option<String>,
}

impl DeltaDivergence {
    pub fn new(anchor_ts: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "delta_divergence".to_string(),
                display_name: "Delta Divergence".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            anchor_ts,
            claimed_kind: default_claimed_kind(),
            show_labels: true,
            cached_hit: None,
            cached_from_ts: None,
            cached_footprint_present: false,
            cached_violations: Vec::new(),
            last_seen_footprint_len: None,
            last_seen_anchor_ts: None,
            last_seen_claimed_kind: None,
        }
    }

    /// This claim's own resolved hit plus the earlier swing's own
    /// timestamp — the SAME resolution
    /// [`Self::refresh_footprint_derived_cache`] performs.
    pub fn resolve(&self, footprint: &[FootprintBar]) -> Option<(DeltaDivergenceHit, i64)> {
        let anchor_idx = find_footprint_bar_for_ts(footprint, self.anchor_ts)?;
        let cfg = orderflow_engine_config();
        let (window_bars, window_series, offset) =
            footprint_judge_window(footprint, anchor_idx, cfg.judge_lookback_bars);
        let local_target = anchor_idx.checked_sub(offset)?;
        let dd_cfg = cfg.to_delta_divergence_config();
        let hit = mli::orderflow::delta_divergences(&window_bars, &window_series, &dd_cfg)
            .into_iter()
            .find(|h| h.to_index == local_target)?;
        let global_hit = DeltaDivergenceHit { from_index: hit.from_index + offset, to_index: hit.to_index + offset, ..hit };
        let from_ts = footprint.get(global_hit.from_index)?.open_time;
        Some((global_hit, from_ts))
    }

    /// The read-only Stat block's own rows — the two prices compared, the
    /// two delta (series) values compared, and the earlier swing's own bar.
    pub fn result_stat_rows(&self) -> Vec<StatRow> {
        let lang = current_language();
        let Some(hit) = &self.cached_hit else { return Vec::new() };
        vec![
            StatRow::new(ConceptKey::DeltaDivergencePriceFromLabel.get(lang), &crate::drawing::primitives_v2::fmt_price(hit.price_from), None),
            StatRow::new(ConceptKey::DeltaDivergencePriceToLabel.get(lang), &crate::drawing::primitives_v2::fmt_price(hit.price_to), None),
            StatRow::new(ConceptKey::DeltaDivergenceValueFromLabel.get(lang), &crate::drawing::primitives_v2::fmt_price(hit.series_from), None),
            StatRow::new(ConceptKey::DeltaDivergenceValueToLabel.get(lang), &crate::drawing::primitives_v2::fmt_price(hit.series_to), None),
            StatRow::new(ConceptKey::DeltaDivergenceEarlierSwingLabel.get(lang), &hit.from_index.to_string(), None),
        ]
    }
}

impl Primitive for DeltaDivergence {
    fn type_id(&self) -> &'static str { "delta_divergence" }
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

    /// The anchor handle, then the cached hit's own level segment from the
    /// earlier swing's screen X to the anchor's screen X. Reads
    /// [`Self::cached_hit`]/[`Self::cached_from_ts`] — see the module's own
    /// doc for why (no footprint accessor on `RenderContext`).
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let anchor_x = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.anchor_ts));
        if (sx - anchor_x).abs() < HIT_TOLERANCE {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        let (Some(hit), Some(from_ts)) = (&self.cached_hit, self.cached_from_ts) else { return HitTestResult::Miss };
        let from_x = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, from_ts));
        let from_y = vp.price_to_y(hit.price_from, ps.price_min, ps.price_max);
        let to_y = vp.price_to_y(hit.price_to, ps.price_min, ps.price_max);
        if point_to_line_distance(sx, sy, from_x, from_y, anchor_x, to_y) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let x = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.anchor_ts));
        let cy = match &self.cached_hit {
            Some(hit) => vp.price_to_y(hit.price_to, ps.price_min, ps.price_max),
            None => super::super::anchor_mark::anchor_control_y(bars, self.anchor_ts, vp, ps),
        };
        vec![ControlPoint::point1(x, cy)]
    }

    /// A diagonal from the earlier swing to the anchor, tinted
    /// `guard::VIOLATION_COLOR` when nothing resolves or the claimed kind
    /// disagrees AND `OrderFlowEngineConfig::judge_claims` is on. Reads
    /// [`Self::cached_hit`]/[`Self::cached_from_ts`] — see the module's own
    /// doc for why `render` cannot recompute this fresh.
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        // Read only the claimed bar's own high/low out of the borrow, so the
        // unresolved mark can bracket that bar instead of ruling the chart.
        let anchor_span = super::super::anchor_mark::bar_span_at(ctx.bars(), self.anchor_ts);
        let judge_claims = orderflow_engine_config().judge_claims;
        // Missing data recolours whatever `judge_claims` says — see
        // `context_violations`'s own doc for why that rule is not a claim.
        let violated = !self.cached_footprint_present
            || judge_claims && !violations_for(&self.claimed_kind, self.cached_hit.as_ref()).is_empty();
        let stroke = if violated { guard::VIOLATION_COLOR.to_string() } else { self.data.color.stroke.clone() };
        let anchor_x = ctx.ts_to_x_ms(self.anchor_ts);

        let (Some(hit), Some(from_ts)) = (&self.cached_hit, self.cached_from_ts) else {
            super::super::anchor_mark::draw_unresolved_anchor(
                ctx, anchor_x, anchor_span, &stroke, self.data.width,
            );
            return;
        };

        let from_x = ctx.ts_to_x_ms(from_ts);
        let from_y = ctx.price_to_y(hit.price_from);
        let to_y = ctx.price_to_y(hit.price_to);

        ctx.set_stroke_color(&stroke);
        ctx.set_stroke_width(self.data.width);
        ctx.set_line_dash(&[]);
        ctx.begin_path();
        ctx.move_to(crisp(from_x, dpr), crisp(from_y, dpr));
        ctx.line_to(crisp(anchor_x, dpr), crisp(to_y, dpr));
        ctx.stroke();

        if self.show_labels {
            let lang = current_language();
            let label = match self.claimed_kind.as_str() {
                "hidden" => ConceptKey::DeltaDivergenceKindHidden.get(lang),
                _ => ConceptKey::DeltaDivergenceKindRegular.get(lang),
            };
            ctx.set_fill_color(&stroke);
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(crate::render::TextAlign::Center);
            ctx.set_text_baseline(crate::render::TextBaseline::Bottom);
            ctx.fill_text(label, anchor_x, to_y - 4.0 * dpr);
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => from_x.min(anchor_x),
                    TextAlign::Center => (from_x + anchor_x) / 2.0,
                    TextAlign::End => from_x.max(anchor_x),
                };
                let text_y = match text.v_align {
                    TextAlign::Start => to_y - text_offset,
                    TextAlign::Center => (from_y + to_y) / 2.0,
                    TextAlign::End => to_y + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &stroke);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    /// Recompute [`Self::cached_hit`]/[`Self::cached_from_ts`]/
    /// [`Self::cached_violations`] from `footprint`. Short-circuits when
    /// neither the footprint length, this claim's own `anchor_ts`, NOR its
    /// own `claimed_kind` has changed since the last call — the kind alone
    /// can flip the guard verdict without the resolved hit itself changing,
    /// so it is part of the short-circuit key too.
    fn refresh_footprint_derived_cache(&mut self, footprint: &[FootprintBar]) {
        if self.cached_footprint_present
            && self.last_seen_footprint_len == Some(footprint.len())
            && self.last_seen_anchor_ts == Some(self.anchor_ts)
            && self.last_seen_claimed_kind.as_deref() == Some(self.claimed_kind.as_str())
        {
            return;
        }
        let resolved = self.resolve(footprint);
        let hit = resolved.as_ref().map(|(h, _)| *h);
        self.cached_violations = if orderflow_engine_config().judge_claims {
            violations_for(&self.claimed_kind, hit.as_ref())
        } else {
            Vec::new()
        };
        self.cached_hit = hit;
        self.cached_from_ts = resolved.map(|(_, ts)| ts);
        self.cached_footprint_present = true;
        self.last_seen_footprint_len = Some(footprint.len());
        self.last_seen_anchor_ts = Some(self.anchor_ts);
        self.last_seen_claimed_kind = Some(self.claimed_kind.clone());
    }

    /// Snap this claim onto the footprint bucket its own detector names,
    /// within `claim_snap_bars` — see [`Primitive::snap_to_detected`] for
    /// the contract.
    fn snap_to_detected_footprint(&mut self, footprint: &[crate::types::FootprintBar]) {
        let radius = orderflow_engine_config().claim_snap_bars;
        if radius == 0 || self.resolve(footprint).is_some() {
            return;
        }
        let Some(idx) = super::orderflow_judge::find_footprint_bar_for_ts(footprint, self.anchor_ts) else { return };
        let placed = self.anchor_ts;
        for step in 1..=radius {
            for candidate in [idx.checked_sub(step), idx.checked_add(step).filter(|i| *i < footprint.len())] {
                let Some(candidate) = candidate else { continue };
                self.anchor_ts = footprint[candidate].open_time;
                if self.resolve(footprint).is_some() {
                    return;
                }
            }
        }
        self.anchor_ts = placed;
    }

    /// Gated on `OrderFlowEngineConfig::judge_claims` first — see
    /// `imbalance_zone::ImbalanceZone::context_violations`'s own doc for the
    /// identical rationale.
    /// The missing-data rule is checked BEFORE
    /// `OrderFlowEngineConfig::judge_claims`, and deliberately so: turning
    /// judging off says "stop telling me my claims disagree with the bars",
    /// not "stop telling me there are no bars". A series with no per-price
    /// bid/ask carries nothing this family can read, and silencing that
    /// leaves the tool drawing an empty shape while answering a different
    /// question than the one it was asked — the exact degradation Appendix B
    /// of `docs/mlc/plans/markup-engines-2026-08-25.md` forbids.
    fn markup_family(&self) -> Option<super::super::markup::MarkupFamily> {
        Some(super::super::markup::MarkupFamily::OrderFlow)
    }

    /// Append-only, by [`Primitive::push_context_violations`]'s own
    /// contract: the cross-primitive pass runs right after this claim's
    /// own cache refresh has rebuilt the self-derived list.
    fn push_context_violations(&mut self, violations: Vec<Violation>) {
        self.cached_violations.extend(violations);
    }

    fn context_violations(&self) -> &[Violation] {
        if !self.cached_footprint_present {
            return &super::orderflow_judge::MISSING_VIOLATION;
        }
        if !orderflow_engine_config().judge_claims {
            return &[];
        }
        &self.cached_violations
    }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        let lang = current_language();
        vec![
            ConfigProperty::pills(
                "divergence_kind",
                ConceptKey::DeltaDivergenceKindLabel.get(lang),
                &self.claimed_kind,
                vec![
                    SelectOption::new("regular", ConceptKey::DeltaDivergenceKindRegular.get(lang)),
                    SelectOption::new("hidden", ConceptKey::DeltaDivergenceKindHidden.get(lang)),
                ],
            ).with_order(10),
            ConfigProperty::show_labels(self.show_labels).with_order(11),
            ConfigProperty::stat_block(DELTA_DIVERGENCE_RESULT_STATS_ID, self.result_stat_rows()).with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            DELTA_DIVERGENCE_RESULT_STATS_ID => {}
            "divergence_kind" => {
                if let Some(s) = value.as_string() {
                    if s == "regular" || s == "hidden" {
                        self.claimed_kind = s.to_string();
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
        type_id: "delta_divergence",
        display_name: "Delta Divergence",
        kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::SingleClick,
        tooltip: "Claim a delta divergence on the later of two swing bars",
        icon: "delta_divergence",
        default_color: "#AB47BC",
        factory: |points, color| {
            let (t, _) = points.first().copied().unwrap_or((0, 0.0));
            Box::new(DeltaDivergence::new(t, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset_engine_config() -> std::sync::MutexGuard<'static, ()> {
        use crate::state::orderflow_engine::{engine_config_test_lock, set_orderflow_engine_config, OrderFlowEngineConfig};
        let guard = engine_config_test_lock();
        set_orderflow_engine_config(OrderFlowEngineConfig::default());
        guard
    }

    fn bar(open_time: i64, o: f64, h: f64, l: f64, c: f64) -> FootprintBar {
        FootprintBar { open_time, open: o, high: h, low: l, close: c, volume: 1.0, ..Default::default() }
    }

    /// Reproduces `mli::orderflow::delta_divergence`'s own
    /// `rising_highs_fixture`: two confirmed swing highs (idx1=20, idx3=25,
    /// rising) with one confirmed swing low (idx2=5) between them —
    /// `swing_n = 1`. Series falls while price's swing highs rise → regular
    /// bearish, reported on bar 3 (`to_index`).
    fn regular_bearish_fixture() -> (Vec<FootprintBar>, Vec<f64>) {
        let bars = vec![
            bar(0, 10.0, 10.0, 8.0, 9.0),
            bar(1_000, 15.0, 20.0, 15.0, 18.0),
            bar(2_000, 8.0, 10.0, 5.0, 7.0),
            bar(3_000, 15.0, 25.0, 15.0, 20.0),
            bar(4_000, 10.0, 10.0, 8.0, 9.0),
        ];
        let series = vec![0.0, 100.0, 0.0, 50.0, 0.0];
        (bars, series)
    }

    fn footprint_with_delta(fixture: &(Vec<FootprintBar>, Vec<f64>)) -> Vec<FootprintBar> {
        fixture.0.iter().zip(fixture.1.iter()).map(|(b, &d)| {
            let mut b = b.clone();
            b.cumulative_delta = d;
            b
        }).collect()
    }

    /// Reproduces `mli::orderflow::delta_divergence`'s own
    /// `rising_lows_fixture`: two confirmed swing lows (idx1=2, idx3=5,
    /// RISING) — the shape a hidden bullish divergence is read on when the
    /// series makes the LOWER low instead. `swing_n = 1`, reported on bar 3.
    fn hidden_bullish_fixture() -> (Vec<FootprintBar>, Vec<f64>) {
        let bars = vec![
            bar(0, 20.0, 20.0, 10.0, 15.0),
            bar(1_000, 15.0, 15.0, 2.0, 10.0),
            bar(2_000, 20.0, 25.0, 10.0, 22.0),
            bar(3_000, 15.0, 15.0, 5.0, 10.0),
            bar(4_000, 20.0, 20.0, 10.0, 15.0),
        ];
        let series = vec![0.0, 100.0, 0.0, 50.0, 0.0];
        (bars, series)
    }

    fn set_fixture_config() {
        use crate::state::orderflow_engine::{set_orderflow_engine_config, OrderFlowEngineConfig};
        set_orderflow_engine_config(OrderFlowEngineConfig { delta_divergence_swing_n: 1, ..OrderFlowEngineConfig::default() });
    }

    #[test]
    fn no_footprint_data_reports_exactly_order_flow_data_missing() {
        let _guard = reset_engine_config();
        let claim = DeltaDivergence::new(3_000, "#AB47BC");
        assert!(!claim.cached_footprint_present);
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "order_flow_data_missing");
    }

    /// Hits equal a direct `mli::orderflow::delta_divergences` call over the
    /// same window (full footprint, offset 0 for this fixture's own size).
    #[test]
    fn resolved_hit_matches_a_direct_mli_call_over_the_same_window() {
        let _guard = reset_engine_config();
        set_fixture_config();
        let footprint = footprint_with_delta(&regular_bearish_fixture());
        let claim = DeltaDivergence::new(3_000, "#AB47BC");
        let (hit, from_ts) = claim.resolve(&footprint).expect("expected a resolved hit");

        let cfg = crate::state::orderflow_engine::orderflow_engine_config();
        let smc_bars: Vec<mli::smc::SmcBar> =
            footprint.iter().map(|fb| mli::smc::SmcBar { open: fb.open, high: fb.high, low: fb.low, close: fb.close }).collect();
        let series: Vec<f64> = footprint.iter().map(|fb| fb.cumulative_delta).collect();
        let direct = mli::orderflow::delta_divergences(&smc_bars, &series, &cfg.to_delta_divergence_config());
        let direct_hit = direct.into_iter().find(|h| h.to_index == 3).expect("direct call must also resolve bar 3");

        assert_eq!(hit, direct_hit);
        assert_eq!(hit.kind, DivergenceKind::Regular);
        assert_eq!(from_ts, footprint[1].open_time);
    }

    #[test]
    fn a_regular_claim_on_the_real_regular_bar_reports_no_violation() {
        let _guard = reset_engine_config();
        set_fixture_config();
        let footprint = footprint_with_delta(&regular_bearish_fixture());
        let mut claim = DeltaDivergence::new(3_000, "#AB47BC");
        claim.claimed_kind = "regular".to_string();
        claim.refresh_footprint_derived_cache(&footprint);
        assert!(claim.cached_hit.is_some());
        assert!(claim.context_violations().is_empty(), "{:?}", claim.context_violations());
    }

    /// A `hidden` claim on a bar `mli` reports as `regular` reports
    /// `delta_divergence_kind_mismatch`, NOT `delta_divergence_not_formed`.
    #[test]
    fn a_hidden_claim_on_a_regular_bar_reports_kind_mismatch_not_not_formed() {
        let _guard = reset_engine_config();
        set_fixture_config();
        let footprint = footprint_with_delta(&regular_bearish_fixture());
        let mut claim = DeltaDivergence::new(3_000, "#AB47BC");
        claim.claimed_kind = "hidden".to_string();
        claim.refresh_footprint_derived_cache(&footprint);
        assert!(claim.cached_hit.is_some(), "a divergence DOES exist at this bar");
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "delta_divergence_kind_mismatch");
    }

    #[test]
    fn a_claim_on_a_bar_with_no_divergence_reports_not_formed() {
        let _guard = reset_engine_config();
        set_fixture_config();
        let footprint = footprint_with_delta(&regular_bearish_fixture());
        let mut claim = DeltaDivergence::new(2_000, "#AB47BC"); // no divergence here
        claim.refresh_footprint_derived_cache(&footprint);
        assert!(claim.cached_hit.is_none());
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "delta_divergence_not_formed");
    }

    /// The task's own literal scenario: "a Regular claim on a Hidden
    /// divergence reports `delta_divergence_kind_mismatch`" — NOT
    /// `delta_divergence_not_formed`.
    #[test]
    fn a_regular_claim_on_a_hidden_divergence_reports_kind_mismatch_not_not_formed() {
        let _guard = reset_engine_config();
        set_fixture_config();
        let footprint = footprint_with_delta(&hidden_bullish_fixture());
        let mut claim = DeltaDivergence::new(3_000, "#AB47BC");
        claim.claimed_kind = "regular".to_string();
        claim.refresh_footprint_derived_cache(&footprint);
        let hit = claim.cached_hit.expect("a hidden divergence DOES exist at this bar");
        assert_eq!(hit.kind, DivergenceKind::Hidden);
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "delta_divergence_kind_mismatch");
    }

    #[test]
    fn render_cache_is_absent_from_to_json() {
        let _guard = reset_engine_config();
        set_fixture_config();
        let footprint = footprint_with_delta(&regular_bearish_fixture());
        let mut claim = DeltaDivergence::new(3_000, "#AB47BC");
        claim.refresh_footprint_derived_cache(&footprint);
        let json = claim.to_json();
        assert!(!json.contains("cached_hit"), "{json}");
        assert!(!json.contains("cached_from_ts"), "{json}");
        assert!(!json.contains("cached_violations"), "{json}");

        let mut registry = super::super::super::registry::PrimitiveRegistry::new();
        registry.register_typed::<DeltaDivergence>(metadata());
        let restored = registry.from_json("delta_divergence", &json).expect("round trip");
        assert_eq!(restored.points(), claim.points());
    }

    #[test]
    fn judge_claims_off_silences_the_guard() {
        use crate::state::orderflow_engine::{set_orderflow_engine_config, OrderFlowEngineConfig};
        let _guard = reset_engine_config();
        let footprint = footprint_with_delta(&regular_bearish_fixture());
        let mut claim = DeltaDivergence::new(3_000, "#AB47BC");
        claim.claimed_kind = "hidden".to_string(); // wrong claim
        set_orderflow_engine_config(OrderFlowEngineConfig { delta_divergence_swing_n: 1, judge_claims: false, ..OrderFlowEngineConfig::default() });
        claim.refresh_footprint_derived_cache(&footprint);
        assert!(claim.cached_hit.is_some(), "the resolution still runs");
        assert!(claim.context_violations().is_empty(), "judging itself is off");
    }

    #[test]
    fn the_sweep_short_circuits_on_an_unchanged_slice() {
        let _guard = reset_engine_config();
        set_fixture_config();
        let footprint = footprint_with_delta(&regular_bearish_fixture());
        let mut claim = DeltaDivergence::new(3_000, "#AB47BC");
        claim.refresh_footprint_derived_cache(&footprint);
        let first = claim.cached_hit;
        claim.refresh_footprint_derived_cache(&footprint);
        assert_eq!(claim.cached_hit, first);
    }

    /// Turning judging off must not hide the fact that there is nothing to
    /// judge. `judge_claims` silences "your claim disagrees with the bars";
    /// it does not silence "this series carries no per-price bid/ask", which
    /// is the difference between a tool that is quiet and a tool that is
    /// blank while pretending otherwise.
    #[test]
    fn judge_claims_off_still_reports_missing_data() {
        use crate::state::orderflow_engine::{set_orderflow_engine_config, OrderFlowEngineConfig};
        let _guard = reset_engine_config();
        set_orderflow_engine_config(OrderFlowEngineConfig { judge_claims: false, ..OrderFlowEngineConfig::default() });
        let claim = DeltaDivergence::new(1_000, "#EF5350");
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1, "the missing-data rule survives judging being off: {violations:?}");
        assert_eq!(violations[0].rule_id, "order_flow_data_missing");
    }
}
