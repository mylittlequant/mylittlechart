//! Liquidity Sweep — the CLAIM half of the smart-money liquidity-sweep
//! markup primitive (`docs/mlc/plans/markup-engines-2026-08-25.md`, Appendix
//! A, "Liquidity sweep / stop hunt").
//!
//! ONE point: the bar the analyst claims wicked through a prior swing and
//! recovered back inside it. The resolved sweep (swept level, direction,
//! bars to recovery) is DERIVED — `mli::smc::liquidity_sweeps` run over a
//! bounded judgement window (`crate::state::smc_engine::SmcEngineConfig::
//! judge_lookback_bars`, resolved via [`super::smc_judge::judge_window`]) —
//! never stored.
//!
//! Appendix A: "This is the wick counterpart of BOS: same level, opposite
//! verdict." `mli::smc::liquidity_sweeps` already draws that line (a wick
//! that never recovers settles as a genuine break, left to
//! `mli::smc::structure`, and produces NO sweep at all) — this claim's own
//! single guard rule (`"liquidity_sweep_not_formed"`) fires whenever no
//! sweep resolves at the anchor, covering BOTH of Appendix A's own failure
//! cases (no wick beyond a prior confirmed swing, or a wick whose close
//! never recovered within the configured window) with one rule id, since
//! `mli` itself reports both as the identical "nothing here" outcome.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64, find_bar_for_timestamp_ms};
use crate::i18n::{current_language, ConceptKey};
use crate::state::smc_engine::smc_engine_config;
use mli::smc as mli_smc;
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, render_primitive_text, TextAlign,
    point_to_line_distance,
    config::{ConfigProperty, PropertyValue, StatRow},
    elliott::guard::{self, Violation},
};

fn default_true() -> bool {
    true
}

fn violations_for(detected: Option<&mli_smc::LiquiditySweep>) -> Vec<Violation> {
    if detected.is_some() {
        Vec::new()
    } else {
        vec![Violation::whole("liquidity_sweep_not_formed", ConceptKey::RuleLiquiditySweepNotFormed)]
    }
}

const LIQUIDITY_SWEEP_RESULT_STATS_ID: &str = "liquidity_sweep_result_stats";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiquiditySweep {
    pub data: PrimitiveData,
    /// Timestamp (ms) of the bar the analyst claims wicked through a prior
    /// swing.
    #[serde(default)]
    pub anchor_ts: i64,
    /// Whether the resolved level's own localized direction label draws
    /// under the marker — the patterns-family's own toggle convention.
    #[serde(default = "default_true")]
    pub show_labels: bool,

    /// This claim's own resolved sweep, with every bar index already
    /// converted back to GLOBAL — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    #[serde(skip)]
    pub cached_sweep: Option<mli_smc::LiquiditySweep>,
    /// This claim's own guard rule — see [`Primitive::context_violations`]'s
    /// own contract.
    #[serde(skip)]
    pub cached_violations: Vec<Violation>,
}

impl LiquiditySweep {
    pub fn new(anchor_ts: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "liquidity_sweep".to_string(),
                display_name: "Liquidity Sweep".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            anchor_ts,
            show_labels: true,
            cached_sweep: None,
            cached_violations: Vec::new(),
        }
    }

    fn anchor_bar_idx(&self, bars: &[Bar]) -> Option<usize> {
        let idx = find_bar_for_timestamp_ms(bars, self.anchor_ts)?;
        (idx < bars.len()).then_some(idx)
    }

    /// This claim's own resolved sweep — `mli::smc::liquidity_sweeps` run
    /// over the bounded judgement window ([`super::smc_judge::judge_window`]),
    /// filtered to the one sweep whose own `index` lands exactly on the
    /// anchor bar. Every bar index on the returned value is already GLOBAL.
    pub fn detect(&self, bars: &[Bar]) -> Option<mli_smc::LiquiditySweep> {
        let anchor_idx = self.anchor_bar_idx(bars)?;
        let engine_cfg = smc_engine_config();
        let (window, offset) = super::smc_judge::judge_window(bars, anchor_idx, engine_cfg.judge_lookback_bars);
        let cfg = engine_cfg.to_smc_config();
        let local_target = anchor_idx.checked_sub(offset)?;
        mli_smc::liquidity_sweeps(&window, &cfg)
            .into_iter()
            .find(|s| s.index == local_target)
            .map(|s| mli_smc::LiquiditySweep {
                index: s.index + offset,
                swept_swing_index: s.swept_swing_index + offset,
                level: s.level,
                direction: s.direction,
                recovered_within: s.recovered_within,
            })
    }

    /// The read-only Stat block's own rows — the swept level and how many
    /// bars separated the wick from the close that recovered back inside
    /// it, both read from [`Self::cached_sweep`].
    pub fn result_stat_rows(&self) -> Vec<StatRow> {
        let lang = current_language();
        let Some(sweep) = &self.cached_sweep else { return Vec::new() };
        let level_value = crate::drawing::primitives_v2::fmt_price(sweep.level);
        vec![
            StatRow::new(ConceptKey::SweptLevelLabel.get(lang), &level_value, None),
            StatRow::new(ConceptKey::RecoveryBarsLabel.get(lang), &sweep.recovered_within.to_string(), None),
        ]
    }
}

impl Primitive for LiquiditySweep {
    fn type_id(&self) -> &'static str { "liquidity_sweep" }
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

    /// The anchor handle, then the resolved level line from the swept swing
    /// bar to the anchor bar.
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let anchor_x = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.anchor_ts));
        if (sx - anchor_x).abs() < HIT_TOLERANCE {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        let Some(sweep) = self.detect(bars) else { return HitTestResult::Miss };
        let level_y = vp.price_to_y(sweep.level, ps.price_min, ps.price_max);
        let x1 = vp.bar_to_x_f64(sweep.swept_swing_index as f64);
        if point_to_line_distance(sx, sy, x1, level_y, anchor_x, level_y) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let x = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.anchor_ts));
        let cy = match self.detect(bars) {
            Some(sweep) => vp.price_to_y(sweep.level, ps.price_min, ps.price_max),
            None => vp.price_to_y((ps.price_min + ps.price_max) / 2.0, ps.price_min, ps.price_max),
        };
        vec![ControlPoint::point1(x, cy)]
    }

    /// A horizontal level line from the swept swing bar to the anchor bar,
    /// plus a marker dot at the anchor, tinted `guard::VIOLATION_COLOR` when
    /// nothing resolves AND `SmcEngineConfig::judge_claims` is on.
    /// Recomputed FRESH from `bars` here, matching
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let bars = ctx.bars();
        let detected = self.detect(bars);
        let anchor_span = super::super::anchor_mark::bar_span_at(bars, self.anchor_ts);
        let judge_claims = smc_engine_config().judge_claims;
        let violated = judge_claims && detected.is_none();
        let stroke = if violated { guard::VIOLATION_COLOR.to_string() } else { self.data.color.stroke.clone() };
        let chart_height = ctx.chart_height();
        let anchor_x = ctx.ts_to_x_ms(self.anchor_ts);

        let Some(sweep) = detected else {
            super::super::anchor_mark::draw_unresolved_anchor(
                ctx, anchor_x, anchor_span, &stroke, self.data.width,
            );
            return;
        };

        let x1 = ctx.bar_to_x(sweep.swept_swing_index as f64);
        let level_y = ctx.price_to_y(sweep.level);

        ctx.set_stroke_color(&stroke);
        ctx.set_stroke_width(self.data.width);
        ctx.set_line_dash(&[6.0 * dpr, 3.0 * dpr]);
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(level_y, dpr));
        ctx.line_to(crisp(anchor_x, dpr), crisp(level_y, dpr));
        ctx.stroke();
        ctx.set_line_dash(&[]);

        ctx.set_fill_color(&stroke);
        ctx.begin_path();
        ctx.arc(anchor_x, level_y, 3.0 * dpr, 0.0, std::f64::consts::TAU);
        ctx.fill();

        if self.show_labels {
            let lang = current_language();
            let label = match sweep.direction {
                mli_smc::Direction::Up => ConceptKey::PatternBiasBearish.get(lang),
                mli_smc::Direction::Down => ConceptKey::PatternBiasBullish.get(lang),
            };
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(crate::render::TextAlign::Center);
            ctx.set_text_baseline(crate::render::TextBaseline::Top);
            ctx.fill_text(label, anchor_x, level_y + 6.0 * dpr);
        }

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let text_offset = 8.0 + text.font_size / 2.0;
                let text_x = match text.h_align {
                    TextAlign::Start => x1.min(anchor_x),
                    TextAlign::Center => (x1 + anchor_x) / 2.0,
                    TextAlign::End => x1.max(anchor_x),
                };
                let text_y = match text.v_align {
                    TextAlign::Start => level_y - text_offset,
                    TextAlign::Center => chart_height / 2.0,
                    TextAlign::End => level_y + text_offset,
                };
                render_primitive_text(ctx, text, text_x, text_y, &stroke);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    fn refresh_bar_derived_cache(&mut self, bars: &[Bar]) {
        let detected = self.detect(bars);
        self.cached_violations = if smc_engine_config().judge_claims {
            violations_for(detected.as_ref())
        } else {
            Vec::new()
        };
        self.cached_sweep = detected;
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

    fn context_violations(&self) -> &[Violation] { &self.cached_violations }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::show_labels(self.show_labels).with_order(10),
            ConfigProperty::stat_block(LIQUIDITY_SWEEP_RESULT_STATS_ID, self.result_stat_rows()).with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            LIQUIDITY_SWEEP_RESULT_STATS_ID => {}
            "show_labels" => { if let Some(v) = value.as_bool() { self.show_labels = v; return true; } }
            _ => {}
        }
        false
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "liquidity_sweep",
        display_name: "Liquidity Sweep",
        kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::SingleClick,
        tooltip: "Claim a liquidity sweep completes on this bar",
        icon: "liquidity_sweep",
        default_color: "#EC407A",
        factory: |points, color| {
            let (t, _) = points.first().copied().unwrap_or((0, 0.0));
            Box::new(LiquiditySweep::new(t, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reproduces `mli::smc::liquidity`'s own `sweep_fixture`: an uptrend
    /// with a confirmed swing high H2=40 at bar 7, wicked (45) but recovered
    /// (close 35) at bar 9.
    fn sweep_fixture_bars() -> Vec<Bar> {
        vec![
            Bar::new(0, 20.0, 22.0, 20.0, 21.0),
            Bar::new(1, 10.0, 12.0, 10.0, 11.0),
            Bar::new(2, 20.0, 22.0, 20.0, 21.0),
            Bar::new(3, 30.0, 30.0, 28.0, 29.0),
            Bar::new(4, 21.0, 22.0, 21.0, 21.0),
            Bar::new(5, 20.0, 22.0, 20.0, 21.0),
            Bar::new(6, 25.0, 27.0, 25.0, 26.0),
            Bar::new(7, 40.0, 40.0, 38.0, 39.0),
            Bar::new(8, 25.0, 27.0, 25.0, 26.0),
            Bar::new(9, 25.0, 45.0, 24.0, 35.0),
            Bar::new(10, 25.0, 27.0, 22.0, 26.0),
        ]
    }

    fn reset_engine_config() -> std::sync::MutexGuard<'static, ()> {
        use crate::state::smc_engine::{engine_config_test_lock, set_smc_engine_config, SmcEngineConfig};
        let guard = engine_config_test_lock();
        set_smc_engine_config(SmcEngineConfig { swing_n: 1, ..SmcEngineConfig::default() });
        guard
    }

    #[test]
    fn claim_on_the_real_sweep_bar_reports_no_violation() {
        let _guard = reset_engine_config();
        let bars = sweep_fixture_bars();
        let mut claim = LiquiditySweep::new(9_000, "#EC407A");
        claim.refresh_bar_derived_cache(&bars);
        let sweep = claim.cached_sweep.expect("expected a resolved sweep");
        assert_eq!(sweep.swept_swing_index, 7);
        assert_eq!(sweep.level, 40.0);
        assert_eq!(sweep.direction, mli_smc::Direction::Up);
        assert_eq!(sweep.recovered_within, 0);
        assert!(claim.context_violations().is_empty(), "{:?}", claim.context_violations());
    }

    #[test]
    fn claim_on_a_bar_with_no_sweep_reports_not_formed() {
        let _guard = reset_engine_config();
        let bars = sweep_fixture_bars();
        let mut claim = LiquiditySweep::new(4_000, "#EC407A"); // no wick here
        claim.refresh_bar_derived_cache(&bars);
        assert!(claim.cached_sweep.is_none());
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "liquidity_sweep_not_formed");
    }

    #[test]
    fn render_cache_is_absent_from_to_json() {
        let _guard = reset_engine_config();
        let bars = sweep_fixture_bars();
        let mut claim = LiquiditySweep::new(9_000, "#EC407A");
        claim.refresh_bar_derived_cache(&bars);
        let json = claim.to_json();
        assert!(!json.contains("cached_sweep"), "{json}");
        assert!(!json.contains("cached_violations"), "{json}");

        let mut registry = super::super::super::registry::PrimitiveRegistry::new();
        registry.register_typed::<LiquiditySweep>(metadata());
        let restored = registry.from_json("liquidity_sweep", &json).expect("round trip");
        assert_eq!(restored.points(), claim.points());
    }

    #[test]
    fn judge_claims_off_silences_the_guard() {
        use crate::state::smc_engine::{set_smc_engine_config, SmcEngineConfig};
        let _guard = reset_engine_config();
        let bars = sweep_fixture_bars();
        let mut claim = LiquiditySweep::new(4_000, "#EC407A"); // no sweep here
        set_smc_engine_config(SmcEngineConfig { swing_n: 1, judge_claims: false, ..SmcEngineConfig::default() });
        claim.refresh_bar_derived_cache(&bars);
        assert!(claim.cached_sweep.is_none());
        assert!(claim.context_violations().is_empty(), "judging itself is off");
    }
}
