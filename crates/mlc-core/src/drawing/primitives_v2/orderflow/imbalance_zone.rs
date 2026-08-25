//! Imbalance Zone — the CLAIM half of the order-flow stacked-imbalance
//! markup primitive (`docs/mlc/plans/markup-engines-2026-08-25.md`,
//! Appendix B).
//!
//! ONE point: the bar the analyst claims carries a stacked-imbalance run.
//! Reads that bar's own `FootprintBar::stacked_ask_start`/`stacked_ask_len`
//! (or the bid pair) — never recomputes an imbalance ratio or a stacked-run
//! length of its own; both are already computed upstream by
//! `FootprintBar::compute_extras` from the user's own configured
//! thresholds. See [`super::orderflow_judge::resolve_imbalance_run`] for the
//! ONE place this crate turns that flag into a price range.
//!
//! **Honest degradation (Appendix B).** `RenderContext` carries no
//! footprint accessor — only `bars()` — so this primitive's drawn geometry
//! is NOT recomputed fresh every frame the way `smc::order_block` recomputes
//! from `ctx.bars()`. It is filled by
//! [`Primitive::refresh_footprint_derived_cache`] and read back by
//! `render`/`hit_test`/`control_points`; the anchor HANDLE's own screen X is
//! still resolved fresh from `ctx.bars()` every frame (cheap, and correct
//! even before any sweep has ever run), matching every other claim in this
//! crate for that one coordinate.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::types::FootprintBar;
use crate::i18n::{current_language, ConceptKey};
use crate::state::orderflow_engine::orderflow_engine_config;
use crate::find_bar_for_timestamp_ms;
use super::super::markup;
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, render_primitive_text, TextAlign,
    config::{ConfigProperty, PropertyValue, StatRow},
    elliott::guard::{self, Violation},
    AREA_FILL_OPACITY, default_area_fill_opacity,
};
use super::orderflow_judge::{ImbalanceRun, ImbalanceSide, find_footprint_bar_for_ts, resolve_imbalance_run};

fn default_true() -> bool {
    true
}

/// This claim's own guard rule — called only once real footprint data is
/// known to be present (see [`Primitive::context_violations`]'s own
/// override below for the "no data at all" case, which never reaches this
/// function).
fn violations_for(run: Option<&ImbalanceRun>) -> Vec<Violation> {
    if run.is_none() {
        vec![Violation::whole("imbalance_zone_not_present", ConceptKey::RuleImbalanceZoneNotPresent)]
    } else {
        Vec::new()
    }
}

const IMBALANCE_ZONE_RESULT_STATS_ID: &str = "imbalance_zone_result_stats";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImbalanceZone {
    pub data: PrimitiveData,
    /// Timestamp (ms) of the candle the analyst claims carries a stacked
    /// imbalance run.
    #[serde(default)]
    pub anchor_ts: i64,
    /// Zone fill opacity, 0.0-1.0 — the same "barely-there over chart area"
    /// default every zone-shaped primitive in this crate uses.
    #[serde(default = "default_area_fill_opacity")]
    pub fill_opacity: f64,
    /// Whether the resolved side label (Bullish/Bearish, matching the
    /// existing footprint renderer's own ask=▲/bid=▼ reading) draws on the
    /// zone.
    #[serde(default = "default_true")]
    pub show_labels: bool,

    /// This claim's own resolved run — see
    /// [`Primitive::refresh_footprint_derived_cache`]'s own contract.
    /// `#[serde(skip)]`: a stale resolution surviving a reload/undo would
    /// misreport a run that no longer matches this primitive's own current
    /// `anchor_ts`.
    #[serde(skip)]
    pub cached_run: Option<ImbalanceRun>,
    /// Whether the last sweep this primitive saw was handed REAL footprint
    /// data (`footprint_bars == Some(_)`), vs. having never been swept at
    /// all (the "missing" state a freshly constructed primitive, or one on
    /// a series with no footprint data, starts in and stays in). See the
    /// module's own doc — this is what lets `order_flow_data_missing` be
    /// reported even though `refresh_footprint_derived_cache` is never
    /// CALLED at all when the bubble's own `footprint_bars` is `None`.
    #[serde(skip)]
    pub cached_footprint_present: bool,
    /// This claim's own guard rule — see
    /// [`Primitive::context_violations`]'s own contract.
    #[serde(skip)]
    pub cached_violations: Vec<Violation>,
    /// The footprint length [`Self::refresh_footprint_derived_cache`] last
    /// resolved against — the short-circuit this method's own contract
    /// requires (stay cheap when nothing meaningful changed).
    #[serde(skip)]
    last_seen_footprint_len: Option<usize>,
    /// The `anchor_ts` [`Self::refresh_footprint_derived_cache`] last
    /// resolved against — the other half of the short-circuit.
    #[serde(skip)]
    last_seen_anchor_ts: Option<i64>,
}

impl ImbalanceZone {
    pub fn new(anchor_ts: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "imbalance_zone".to_string(),
                display_name: "Imbalance Zone".to_string(),
                color: PrimitiveColor::with_fill(color, color),
                width: 2.0,
                ..Default::default()
            },
            anchor_ts,
            fill_opacity: AREA_FILL_OPACITY,
            show_labels: true,
            cached_run: None,
            cached_footprint_present: false,
            cached_violations: Vec::new(),
            last_seen_footprint_len: None,
            last_seen_anchor_ts: None,
        }
    }

    /// This claim's own resolved run, computed directly from `footprint` —
    /// the SAME call [`Self::refresh_footprint_derived_cache`] makes to fill
    /// [`Self::cached_run`]. Exposed for tests; `render`/`hit_test` read the
    /// cache instead (see the module's own doc for why they must).
    pub fn resolve(&self, footprint: &[FootprintBar]) -> Option<ImbalanceRun> {
        let idx = find_footprint_bar_for_ts(footprint, self.anchor_ts)?;
        resolve_imbalance_run(&footprint[idx])
    }

    /// The read-only Stat block's own rows — side, run length, and the
    /// price levels spanned. Empty when nothing has resolved.
    pub fn result_stat_rows(&self) -> Vec<StatRow> {
        let lang = current_language();
        let Some(run) = &self.cached_run else { return Vec::new() };
        let side_value = match run.side {
            ImbalanceSide::Ask => ConceptKey::PatternBiasBullish.get(lang),
            ImbalanceSide::Bid => ConceptKey::PatternBiasBearish.get(lang),
        };
        let zone_value = format!(
            "{} - {}",
            crate::drawing::primitives_v2::fmt_price(run.low),
            crate::drawing::primitives_v2::fmt_price(run.high),
        );
        vec![
            StatRow::new(ConceptKey::SmcDirectionLabel.get(lang), side_value, None),
            StatRow::new(ConceptKey::ImbalanceZoneRunLengthLabel.get(lang), &run.len.to_string(), None),
            StatRow::new(ConceptKey::SmcZoneLabel.get(lang), &zone_value, None),
        ]
    }
}

impl Primitive for ImbalanceZone {
    fn type_id(&self) -> &'static str { "imbalance_zone" }
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
    /// column test), then the cached zone's own bounding box when one is
    /// resolved. Reads [`Self::cached_run`] — see the module's own doc for
    /// why (no footprint accessor on `RenderContext`).
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let anchor_x = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.anchor_ts));
        if (sx - anchor_x).abs() < HIT_TOLERANCE {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        let Some(run) = &self.cached_run else { return HitTestResult::Miss };
        let bar_idx = timestamp_ms_to_bar_f64(bars, self.anchor_ts);
        let x1 = vp.bar_to_x_f64(bar_idx);
        let x2 = vp.bar_to_x_f64(bar_idx + 1.0);
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        let y1 = vp.price_to_y(run.low, ps.price_min, ps.price_max);
        let y2 = vp.price_to_y(run.high, ps.price_min, ps.price_max);
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
        let cy = match &self.cached_run {
            Some(run) => vp.price_to_y((run.low + run.high) / 2.0, ps.price_min, ps.price_max),
            None => super::super::anchor_mark::anchor_control_y(bars, self.anchor_ts, vp, ps),
        };
        vec![ControlPoint::point1(x, cy)]
    }

    /// A filled/stroked box over the cached run's own one-bar width, tinted
    /// `guard::VIOLATION_COLOR` when nothing is cached AND
    /// `OrderFlowEngineConfig::judge_claims` is on. Reads [`Self::cached_run`]
    /// — see the module's own doc for why `render` cannot recompute this
    /// fresh the way `smc::order_block::OrderBlock::render` recomputes from
    /// `ctx.bars()`.
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let bars = ctx.bars();
        let anchor_span = super::super::anchor_mark::bar_span_at(bars, self.anchor_ts);
        let judge_claims = orderflow_engine_config().judge_claims;
        // Missing data recolours whatever `judge_claims` says — see
        // `context_violations`'s own doc for why that rule is not a claim.
        let violated = !self.cached_footprint_present
            || judge_claims && self.cached_run.is_none();
        let stroke = if violated { guard::VIOLATION_COLOR.to_string() } else { self.data.color.stroke.clone() };

        let Some(run) = &self.cached_run else {
            let anchor_x = ctx.ts_to_x_ms(self.anchor_ts);
            super::super::anchor_mark::draw_unresolved_anchor(
                ctx, anchor_x, anchor_span, &stroke, self.data.width,
            );
            return;
        };

        let bar_idx = timestamp_ms_to_bar_f64(bars, self.anchor_ts);
        let x1 = ctx.bar_to_x(bar_idx);
        let x2 = ctx.bar_to_x(bar_idx + 1.0);
        let y1 = ctx.price_to_y(run.low);
        let y2 = ctx.price_to_y(run.high);
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
            let label = match run.side {
                ImbalanceSide::Ask => ConceptKey::PatternBiasBullish.get(lang),
                ImbalanceSide::Bid => ConceptKey::PatternBiasBearish.get(lang),
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

    /// Recompute [`Self::cached_run`]/[`Self::cached_violations`] from
    /// `footprint` — see [`Primitive::refresh_footprint_derived_cache`]'s
    /// own contract. Short-circuits when neither the footprint length nor
    /// this claim's own `anchor_ts` has changed since the last call, per
    /// that method's own "stay cheap" obligation.
    fn refresh_footprint_derived_cache(&mut self, footprint: &[FootprintBar]) {
        if self.cached_footprint_present
            && self.last_seen_footprint_len == Some(footprint.len())
            && self.last_seen_anchor_ts == Some(self.anchor_ts)
        {
            return;
        }
        let run = self.resolve(footprint);
        self.cached_violations = if orderflow_engine_config().judge_claims {
            violations_for(run.as_ref())
        } else {
            Vec::new()
        };
        self.cached_run = run;
        self.cached_footprint_present = true;
        self.last_seen_footprint_len = Some(footprint.len());
        self.last_seen_anchor_ts = Some(self.anchor_ts);
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

    /// Gated on `OrderFlowEngineConfig::judge_claims` first — an analyst
    /// who has opted out of guard reporting sees no rule at all, including
    /// the missing-data one. Then the shared `order_flow_data_missing` rule
    /// when no sweep has ever handed this claim real footprint data
    /// (Appendix B's own honest-degradation rule — see
    /// [`super::orderflow_judge::MISSING_VIOLATION`]'s own doc for why this
    /// needs no special-cased default anywhere else); [`Self::cached_violations`]
    /// otherwise.
    /// The missing-data rule is checked BEFORE
    /// `OrderFlowEngineConfig::judge_claims`, and deliberately so: turning
    /// judging off says "stop telling me my claims disagree with the bars",
    /// not "stop telling me there are no bars". A series with no per-price
    /// bid/ask carries nothing this family can read, and silencing that
    /// leaves the tool drawing an empty shape while answering a different
    /// question than the one it was asked — the exact degradation Appendix B
    /// of `docs/mlc/plans/markup-engines-2026-08-25.md` forbids.
    /// What this claim contributes to its family's own chain rules — see
    /// [`Primitive::chain_fact`].
    fn chain_fact(&self, bars: &[Bar]) -> Option<markup::ChainFact> {
        let bar = find_bar_for_timestamp_ms(bars, self.anchor_ts)?;
        Some(markup::ChainFact { bar, bias: None, variant: None, origin_event_bar: None })
    }

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
        vec![
            ConfigProperty::fill_opacity(self.fill_opacity * 100.0).with_order(11),
            ConfigProperty::show_labels(self.show_labels).with_order(12),
            ConfigProperty::stat_block(IMBALANCE_ZONE_RESULT_STATS_ID, self.result_stat_rows()).with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            IMBALANCE_ZONE_RESULT_STATS_ID => {}
            "fill_opacity" => { if let Some(v) = value.as_number() { self.fill_opacity = (v / 100.0).clamp(0.0, 1.0); return true; } }
            "show_labels" => { if let Some(v) = value.as_bool() { self.show_labels = v; return true; } }
            _ => {}
        }
        false
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "imbalance_zone",
        display_name: "Imbalance Zone",
        kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::SingleClick,
        tooltip: "Claim a stacked imbalance run on this candle",
        icon: "imbalance_zone",
        default_color: "#26A69A",
        factory: |points, color| {
            let (t, _) = points.first().copied().unwrap_or((0, 0.0));
            Box::new(ImbalanceZone::new(t, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FootprintLevel;

    fn reset_engine_config() -> std::sync::MutexGuard<'static, ()> {
        use crate::state::orderflow_engine::{engine_config_test_lock, set_orderflow_engine_config, OrderFlowEngineConfig};
        let guard = engine_config_test_lock();
        set_orderflow_engine_config(OrderFlowEngineConfig::default());
        guard
    }

    fn footprint_with_run(open_time: i64) -> FootprintBar {
        let levels = vec![
            FootprintLevel { price: 99.0, buy_vol: 1.0, sell_vol: 1.0, is_single_print: false, is_unfinished: false },
            FootprintLevel { price: 100.0, buy_vol: 5.0, sell_vol: 1.0, is_single_print: false, is_unfinished: false },
            FootprintLevel { price: 101.0, buy_vol: 5.0, sell_vol: 1.0, is_single_print: false, is_unfinished: false },
        ];
        let mut bar = FootprintBar { open_time, open: 99.0, high: 101.0, low: 99.0, close: 100.5, volume: 12.0, levels, ..Default::default() };
        bar.stacked_ask_start = Some(1);
        bar.stacked_ask_len = Some(2);
        bar
    }

    fn footprint_without_run(open_time: i64) -> FootprintBar {
        FootprintBar { open_time, open: 99.0, high: 101.0, low: 99.0, close: 100.5, volume: 12.0, ..Default::default() }
    }

    #[test]
    fn no_footprint_data_reports_exactly_order_flow_data_missing() {
        let _guard = reset_engine_config();
        let claim = ImbalanceZone::new(1_000, "#26A69A");
        // Never call refresh_footprint_derived_cache — this is the "series
        // with no footprint at all" case, and the hook is contractually
        // never called then (see `bar_derived_cache::refresh_footprint_derived_caches`'s
        // own doc: it runs only when `footprint_bars` is `Some`).
        assert!(!claim.cached_footprint_present);
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "order_flow_data_missing");
    }

    #[test]
    fn a_bar_carrying_the_flag_reports_nothing() {
        let _guard = reset_engine_config();
        let footprint = vec![footprint_with_run(1_000)];
        let mut claim = ImbalanceZone::new(1_000, "#26A69A");
        claim.refresh_footprint_derived_cache(&footprint);
        assert!(claim.cached_run.is_some());
        assert!(claim.context_violations().is_empty(), "{:?}", claim.context_violations());
    }

    #[test]
    fn a_bar_without_the_flag_reports_its_own_rule() {
        let _guard = reset_engine_config();
        let footprint = vec![footprint_without_run(1_000)];
        let mut claim = ImbalanceZone::new(1_000, "#26A69A");
        claim.refresh_footprint_derived_cache(&footprint);
        assert!(claim.cached_run.is_none());
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "imbalance_zone_not_present");
    }

    #[test]
    fn render_cache_is_absent_from_to_json() {
        let _guard = reset_engine_config();
        let footprint = vec![footprint_with_run(1_000)];
        let mut claim = ImbalanceZone::new(1_000, "#26A69A");
        claim.refresh_footprint_derived_cache(&footprint);
        let json = claim.to_json();
        assert!(!json.contains("cached_run"), "{json}");
        assert!(!json.contains("cached_violations"), "{json}");
        assert!(!json.contains("cached_footprint_present"), "{json}");

        let mut registry = super::super::super::registry::PrimitiveRegistry::new();
        registry.register_typed::<ImbalanceZone>(metadata());
        let restored = registry.from_json("imbalance_zone", &json).expect("round trip");
        assert_eq!(restored.points(), claim.points());
    }

    #[test]
    fn judge_claims_off_silences_the_guard() {
        use crate::state::orderflow_engine::{set_orderflow_engine_config, OrderFlowEngineConfig};
        let _guard = reset_engine_config();
        let footprint = vec![footprint_without_run(1_000)];
        let mut claim = ImbalanceZone::new(1_000, "#26A69A");
        set_orderflow_engine_config(OrderFlowEngineConfig { judge_claims: false, ..OrderFlowEngineConfig::default() });
        claim.refresh_footprint_derived_cache(&footprint);
        assert!(claim.cached_run.is_none(), "the resolution still runs — the bar still doesn't qualify");
        assert!(claim.context_violations().is_empty(), "judging itself is off");
    }

    #[test]
    fn the_sweep_short_circuits_on_an_unchanged_slice() {
        let _guard = reset_engine_config();
        let footprint = vec![footprint_with_run(1_000)];
        let mut claim = ImbalanceZone::new(1_000, "#26A69A");
        claim.refresh_footprint_derived_cache(&footprint);
        let first = claim.cached_run;
        // Call again with an unchanged slice and unchanged anchor — the
        // short-circuit must leave the cache byte-for-byte identical.
        claim.refresh_footprint_derived_cache(&footprint);
        assert_eq!(claim.cached_run, first);
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
        let claim = ImbalanceZone::new(1_000, "#EF5350");
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1, "the missing-data rule survives judging being off: {violations:?}");
        assert_eq!(violations[0].rule_id, "order_flow_data_missing");
    }
}
