//! Absorption Mark — the CLAIM half of the order-flow absorption markup
//! primitive (`docs/mlc/plans/markup-engines-2026-08-25.md`, Appendix B).
//!
//! ONE point: the bar the analyst claims is absorption. Reads
//! `FootprintBar::is_absorption` verbatim — never recomputes the
//! `|delta| / volume` threshold that flag is built from; that arithmetic
//! already lives in `FootprintBar::compute_extras`, driven by the user's own
//! configured threshold. See [`super::orderflow_judge::resolve_absorption`].
//!
//! Honest degradation and the render-reads-the-cache shape are shared with
//! [`super::imbalance_zone`] — see that module's own doc for the full
//! rationale (`RenderContext` carries no footprint accessor).

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64, find_bar_for_timestamp_ms};
use crate::types::FootprintBar;
use crate::i18n::{current_language, ConceptKey, PrimitiveNameKey};
use crate::state::orderflow_engine::orderflow_engine_config;
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, render_primitive_text, TextAlign,
    config::{ConfigProperty, PropertyValue, StatRow},
    elliott::guard::{self, Violation},
};
use super::orderflow_judge::{AbsorptionSnapshot, find_footprint_bar_for_ts, resolve_absorption};

fn default_true() -> bool {
    true
}

/// This claim's own guard rule — called only once real footprint data is
/// known to be present (see [`Primitive::context_violations`]'s own
/// override below for the "no data at all" case).
fn violations_for(snapshot: Option<&AbsorptionSnapshot>) -> Vec<Violation> {
    if snapshot.is_none() {
        vec![Violation::whole("absorption_not_present", ConceptKey::RuleAbsorptionNotPresent)]
    } else {
        Vec::new()
    }
}

const ABSORPTION_MARK_RESULT_STATS_ID: &str = "absorption_mark_result_stats";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AbsorptionMark {
    pub data: PrimitiveData,
    /// Timestamp (ms) of the candle the analyst claims is absorption.
    #[serde(default)]
    pub anchor_ts: i64,
    /// Whether the "Absorption" badge draws on the mark.
    #[serde(default = "default_true")]
    pub show_labels: bool,

    /// This claim's own resolved snapshot — see
    /// [`Primitive::refresh_footprint_derived_cache`]'s own contract.
    #[serde(skip)]
    pub cached_snapshot: Option<AbsorptionSnapshot>,
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
}

impl AbsorptionMark {
    pub fn new(anchor_ts: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "absorption_mark".to_string(),
                display_name: "Absorption".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            anchor_ts,
            show_labels: true,
            cached_snapshot: None,
            cached_footprint_present: false,
            cached_violations: Vec::new(),
            last_seen_footprint_len: None,
            last_seen_anchor_ts: None,
        }
    }

    fn anchor_bar_idx(&self, bars: &[Bar]) -> Option<usize> {
        let idx = find_bar_for_timestamp_ms(bars, self.anchor_ts)?;
        (idx < bars.len()).then_some(idx)
    }

    /// This claim's own resolved snapshot, computed directly from
    /// `footprint` — the SAME call
    /// [`Self::refresh_footprint_derived_cache`] makes.
    pub fn resolve(&self, footprint: &[FootprintBar]) -> Option<AbsorptionSnapshot> {
        let idx = find_footprint_bar_for_ts(footprint, self.anchor_ts)?;
        resolve_absorption(&footprint[idx])
    }

    /// The read-only Stat block's own rows — bar delta, bar volume, and the
    /// ratio between them, all read off [`Self::cached_snapshot`] (never a
    /// recomputed threshold).
    pub fn result_stat_rows(&self) -> Vec<StatRow> {
        let lang = current_language();
        let Some(snap) = &self.cached_snapshot else { return Vec::new() };
        let ratio = if snap.volume > 0.0 { snap.delta.abs() / snap.volume } else { 0.0 };
        vec![
            StatRow::new(ConceptKey::AbsorptionDeltaLabel.get(lang), &crate::drawing::primitives_v2::fmt_price(snap.delta), None),
            StatRow::new(ConceptKey::AbsorptionVolumeLabel.get(lang), &crate::drawing::primitives_v2::fmt_price(snap.volume), None),
            StatRow::new(ConceptKey::AbsorptionRatioLabel.get(lang), &format!("{ratio:.3}"), None),
        ]
    }
}

impl Primitive for AbsorptionMark {
    fn type_id(&self) -> &'static str { "absorption_mark" }
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

    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let anchor_x = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.anchor_ts));
        if (sx - anchor_x).abs() < HIT_TOLERANCE {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        let Some(idx) = self.anchor_bar_idx(bars) else { return HitTestResult::Miss };
        if self.cached_snapshot.is_none() {
            return HitTestResult::Miss;
        }
        let bar = &bars[idx];
        let bar_idx = timestamp_ms_to_bar_f64(bars, self.anchor_ts);
        let x1 = vp.bar_to_x_f64(bar_idx);
        let x2 = vp.bar_to_x_f64(bar_idx + 1.0);
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        let y1 = vp.price_to_y(bar.high, ps.price_min, ps.price_max);
        let y2 = vp.price_to_y(bar.low, ps.price_min, ps.price_max);
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
        let cy = match self.anchor_bar_idx(bars) {
            Some(idx) => vp.price_to_y((bars[idx].high + bars[idx].low) / 2.0, ps.price_min, ps.price_max),
            None => super::super::anchor_mark::anchor_control_y(bars, self.anchor_ts, vp, ps),
        };
        vec![ControlPoint::point1(x, cy)]
    }

    /// A highlighted band over the anchor bar's own OHLC high/low (fresh
    /// from `ctx.bars()` — screen position only, never footprint-derived
    /// itself), tinted `guard::VIOLATION_COLOR` when nothing is cached AND
    /// `OrderFlowEngineConfig::judge_claims` is on. Reads
    /// [`Self::cached_snapshot`] for whether to draw the band at all — see
    /// the module's own doc for why `render` cannot recompute this fresh.
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let bars = ctx.bars();
        let anchor_span = super::super::anchor_mark::bar_span_at(bars, self.anchor_ts);
        let judge_claims = orderflow_engine_config().judge_claims;
        // Missing data recolours whatever `judge_claims` says — see
        // `context_violations`'s own doc for why that rule is not a claim.
        let violated = !self.cached_footprint_present
            || judge_claims && self.cached_snapshot.is_none();
        let stroke = if violated { guard::VIOLATION_COLOR.to_string() } else { self.data.color.stroke.clone() };
        let anchor_x = ctx.ts_to_x_ms(self.anchor_ts);

        let Some(idx) = self.anchor_bar_idx(bars) else {
            super::super::anchor_mark::draw_unresolved_anchor(
                ctx, anchor_x, anchor_span, &stroke, self.data.width,
            );
            return;
        };

        if self.cached_snapshot.is_none() {
            super::super::anchor_mark::draw_unresolved_anchor(
                ctx, anchor_x, anchor_span, &stroke, self.data.width,
            );
            return;
        }

        let bar = &bars[idx];
        let bar_idx = idx as f64;
        let x1 = ctx.bar_to_x(bar_idx);
        let x2 = ctx.bar_to_x(bar_idx + 1.0);
        let y1 = ctx.price_to_y(bar.high);
        let y2 = ctx.price_to_y(bar.low);
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        let (min_y, max_y) = (y1.min(y2), y1.max(y2));
        let width = max_x - min_x;
        let height = max_y - min_y;

        let fill_color = format!("{}{:02x}", &stroke[..7], 0x33_u8);
        ctx.set_fill_color(&fill_color);
        ctx.fill_rect(min_x, min_y, width, height);

        ctx.set_stroke_color(&stroke);
        ctx.set_stroke_width(self.data.width);
        ctx.set_line_dash(&[]);
        ctx.stroke_rect(crisp(min_x, dpr), crisp(min_y, dpr), width, height);

        if self.show_labels {
            let lang = current_language();
            let label = PrimitiveNameKey::AbsorptionMark.get(lang);
            ctx.set_fill_color(&stroke);
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(crate::render::TextAlign::Center);
            ctx.set_text_baseline(crate::render::TextBaseline::Bottom);
            ctx.fill_text(label, (min_x + max_x) / 2.0, min_y - 2.0 * dpr);
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

    /// Recompute [`Self::cached_snapshot`]/[`Self::cached_violations`] from
    /// `footprint`. Short-circuits when neither the footprint length nor
    /// this claim's own `anchor_ts` has changed since the last call — see
    /// [`Primitive::refresh_footprint_derived_cache`]'s own "stay cheap"
    /// contract.
    fn refresh_footprint_derived_cache(&mut self, footprint: &[FootprintBar]) {
        if self.cached_footprint_present
            && self.last_seen_footprint_len == Some(footprint.len())
            && self.last_seen_anchor_ts == Some(self.anchor_ts)
        {
            return;
        }
        let snapshot = self.resolve(footprint);
        self.cached_violations = if orderflow_engine_config().judge_claims {
            violations_for(snapshot.as_ref())
        } else {
            Vec::new()
        };
        self.cached_snapshot = snapshot;
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
        vec![
            ConfigProperty::show_labels(self.show_labels).with_order(12),
            ConfigProperty::stat_block(ABSORPTION_MARK_RESULT_STATS_ID, self.result_stat_rows()).with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            ABSORPTION_MARK_RESULT_STATS_ID => {}
            "show_labels" => { if let Some(v) = value.as_bool() { self.show_labels = v; return true; } }
            _ => {}
        }
        false
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "absorption_mark",
        display_name: "Absorption",
        kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::SingleClick,
        tooltip: "Claim absorption on this candle",
        icon: "absorption_mark",
        default_color: "#EF5350",
        factory: |points, color| {
            let (t, _) = points.first().copied().unwrap_or((0, 0.0));
            Box::new(AbsorptionMark::new(t, color))
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

    fn absorbing_bar(open_time: i64) -> FootprintBar {
        FootprintBar { open_time, open: 100.0, high: 101.0, low: 99.0, close: 100.0, volume: 20.0, is_absorption: true, ..Default::default() }
    }

    fn non_absorbing_bar(open_time: i64) -> FootprintBar {
        FootprintBar { open_time, open: 100.0, high: 101.0, low: 99.0, close: 100.0, volume: 20.0, is_absorption: false, ..Default::default() }
    }

    #[test]
    fn no_footprint_data_reports_exactly_order_flow_data_missing() {
        let _guard = reset_engine_config();
        let claim = AbsorptionMark::new(1_000, "#EF5350");
        assert!(!claim.cached_footprint_present);
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "order_flow_data_missing");
    }

    #[test]
    fn a_bar_carrying_the_flag_reports_nothing() {
        let _guard = reset_engine_config();
        let footprint = vec![absorbing_bar(1_000)];
        let mut claim = AbsorptionMark::new(1_000, "#EF5350");
        claim.refresh_footprint_derived_cache(&footprint);
        assert!(claim.cached_snapshot.is_some());
        assert!(claim.context_violations().is_empty(), "{:?}", claim.context_violations());
    }

    #[test]
    fn a_bar_without_the_flag_reports_its_own_rule() {
        let _guard = reset_engine_config();
        let footprint = vec![non_absorbing_bar(1_000)];
        let mut claim = AbsorptionMark::new(1_000, "#EF5350");
        claim.refresh_footprint_derived_cache(&footprint);
        assert!(claim.cached_snapshot.is_none());
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "absorption_not_present");
    }

    #[test]
    fn render_cache_is_absent_from_to_json() {
        let _guard = reset_engine_config();
        let footprint = vec![absorbing_bar(1_000)];
        let mut claim = AbsorptionMark::new(1_000, "#EF5350");
        claim.refresh_footprint_derived_cache(&footprint);
        let json = claim.to_json();
        assert!(!json.contains("cached_snapshot"), "{json}");
        assert!(!json.contains("cached_violations"), "{json}");

        let mut registry = super::super::super::registry::PrimitiveRegistry::new();
        registry.register_typed::<AbsorptionMark>(metadata());
        let restored = registry.from_json("absorption_mark", &json).expect("round trip");
        assert_eq!(restored.points(), claim.points());
    }

    #[test]
    fn judge_claims_off_silences_the_guard() {
        use crate::state::orderflow_engine::{set_orderflow_engine_config, OrderFlowEngineConfig};
        let _guard = reset_engine_config();
        let footprint = vec![non_absorbing_bar(1_000)];
        let mut claim = AbsorptionMark::new(1_000, "#EF5350");
        set_orderflow_engine_config(OrderFlowEngineConfig { judge_claims: false, ..OrderFlowEngineConfig::default() });
        claim.refresh_footprint_derived_cache(&footprint);
        assert!(claim.cached_snapshot.is_none());
        assert!(claim.context_violations().is_empty(), "judging itself is off");
    }

    #[test]
    fn the_sweep_short_circuits_on_an_unchanged_slice() {
        let _guard = reset_engine_config();
        let footprint = vec![absorbing_bar(1_000)];
        let mut claim = AbsorptionMark::new(1_000, "#EF5350");
        claim.refresh_footprint_derived_cache(&footprint);
        let first = claim.cached_snapshot;
        claim.refresh_footprint_derived_cache(&footprint);
        assert_eq!(claim.cached_snapshot, first);
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
        let claim = AbsorptionMark::new(1_000, "#EF5350");
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1, "the missing-data rule survives judging being off: {violations:?}");
        assert_eq!(violations[0].rule_id, "order_flow_data_missing");
    }
}
