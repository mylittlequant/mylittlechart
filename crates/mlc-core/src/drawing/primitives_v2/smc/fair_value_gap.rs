//! Fair Value Gap — the CLAIM half of the smart-money FVG/imbalance markup
//! primitive (`docs/mlc/plans/markup-engines-2026-08-25.md`, Appendix A,
//! "FVG — fair value gap / imbalance").
//!
//! ONE point: the MIDDLE bar of the three-bar triplet — Appendix A's own
//! wording, "the MIDDLE bar is the displacement candle and its own low/high
//! take no part in the test." The resolved gap (zone, direction,
//! mitigation, fill) is DERIVED — `mli::smc::fair_value_gaps` run over a
//! bounded judgement window (`crate::state::smc_engine::SmcEngineConfig::
//! judge_lookback_bars`, resolved via [`super::smc_judge::judge_window`]) —
//! never stored, so a resolved zone can never disagree with what the bars
//! either side of the anchor actually show.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64, find_bar_for_timestamp_ms};
use crate::i18n::{current_language, ConceptKey};
use crate::state::smc_engine::smc_engine_config;
use mli::smc as mli_smc;
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, render_primitive_text, TextAlign,
    config::{ConfigProperty, PropertyValue, StatRow},
    elliott::guard::{self, Violation},
    AREA_FILL_OPACITY, default_area_fill_opacity,
};
use super::smc_judge::judge_window;

fn default_true() -> bool {
    true
}

/// This claim's own guard rule — see [`FairValueGap::refresh_bar_derived_cache`]'s
/// own doc for the SAME `detect` call this reads.
fn violations_for(detected: Option<&mli_smc::FvgZone>) -> Vec<Violation> {
    if detected.is_some() {
        Vec::new()
    } else {
        vec![Violation::whole("fvg_not_formed", ConceptKey::RuleFvgNotFormed)]
    }
}

const FVG_RESULT_STATS_ID: &str = "fair_value_gap_result_stats";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FairValueGap {
    pub data: PrimitiveData,
    /// Timestamp (ms) of the MIDDLE bar of the claimed triplet — see the
    /// module's own doc.
    #[serde(default)]
    pub anchor_ts: i64,
    /// Whether the drawn zone extends to the LAST loaded bar regardless of
    /// invalidation (`true`), or stops extending the instant `mli` reports
    /// this gap filled (falling back to mitigated, then the last bar) when
    /// `false` — see [`FairValueGap::right_edge_bar`]'s own doc. Default
    /// `true`.
    #[serde(default = "default_true")]
    pub extend_right: bool,
    /// Zone fill opacity, 0.0-1.0 — same default every zone-shaped primitive
    /// in this codebase uses (`fill_defaults::AREA_FILL_OPACITY`).
    #[serde(default = "default_area_fill_opacity")]
    pub fill_opacity: f64,
    /// Whether the resolved direction label draws on the zone.
    #[serde(default = "default_true")]
    pub show_labels: bool,

    /// This claim's own resolved gap, with every bar index already
    /// converted back to GLOBAL — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    #[serde(skip)]
    pub cached_gap: Option<mli_smc::FvgZone>,
    /// This claim's own guard rule — see [`Primitive::context_violations`]'s
    /// own contract.
    #[serde(skip)]
    pub cached_violations: Vec<Violation>,
}

impl FairValueGap {
    pub fn new(anchor_ts: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fair_value_gap".to_string(),
                display_name: "Fair Value Gap".to_string(),
                color: PrimitiveColor::with_fill(color, color),
                width: 2.0,
                ..Default::default()
            },
            anchor_ts,
            extend_right: true,
            fill_opacity: AREA_FILL_OPACITY,
            show_labels: true,
            cached_gap: None,
            cached_violations: Vec::new(),
        }
    }

    fn anchor_bar_idx(&self, bars: &[Bar]) -> Option<usize> {
        let idx = find_bar_for_timestamp_ms(bars, self.anchor_ts)?;
        (idx < bars.len()).then_some(idx)
    }

    /// This claim's own resolved gap — `mli::smc::fair_value_gaps` run over
    /// the bounded judgement window ([`judge_window`]), filtered to the one
    /// gap whose own `start_index` sits exactly one bar before the anchor
    /// (the anchor is the MIDDLE bar of the triplet; `fair_value_gaps`'s own
    /// `start_index` is the OLDER bar — see that function's own doc). Every
    /// bar index on the returned value is already GLOBAL.
    pub fn detect(&self, bars: &[Bar]) -> Option<mli_smc::FvgZone> {
        let anchor_idx = self.anchor_bar_idx(bars)?;
        let engine_cfg = smc_engine_config();
        let (window, offset) = judge_window(bars, anchor_idx, engine_cfg.judge_lookback_bars);
        let cfg = engine_cfg.to_smc_config();
        let local_middle = anchor_idx.checked_sub(offset)?;
        let local_start = local_middle.checked_sub(1)?;
        mli_smc::fair_value_gaps(&window, &cfg)
            .into_iter()
            .find(|g| g.start_index == local_start)
            .map(|g| mli_smc::FvgZone {
                start_index: g.start_index + offset,
                end_index: g.end_index + offset,
                low: g.low,
                high: g.high,
                direction: g.direction,
                mitigated_at: g.mitigated_at.map(|i| i + offset),
                filled_at: g.filled_at.map(|i| i + offset),
            })
    }

    /// The right edge bar index the drawn/hit-tested zone extends to.
    /// `extend_right == true`: always the last loaded bar. `extend_right ==
    /// false`: stops at the fill bar if the gap has been filled, else the
    /// mitigation bar if only mitigated, else also the last loaded bar.
    fn right_edge_bar(&self, gap: &mli_smc::FvgZone, bars: &[Bar]) -> usize {
        let last = bars.len().saturating_sub(1);
        if self.extend_right {
            last
        } else {
            gap.filled_at.or(gap.mitigated_at).unwrap_or(last).min(last)
        }
    }

    /// The read-only Stat block's own rows — direction, zone bounds,
    /// mitigation bar, and fill bar (each [`ConceptKey::NoneValue`] when it
    /// has not happened) — all read from [`Self::cached_gap`].
    pub fn result_stat_rows(&self) -> Vec<StatRow> {
        let lang = current_language();
        let Some(gap) = &self.cached_gap else { return Vec::new() };
        let direction_value = match gap.direction {
            mli_smc::Direction::Up => ConceptKey::PatternBiasBullish.get(lang),
            mli_smc::Direction::Down => ConceptKey::PatternBiasBearish.get(lang),
        };
        let zone_value = format!(
            "{} - {}",
            crate::drawing::primitives_v2::fmt_price(gap.low),
            crate::drawing::primitives_v2::fmt_price(gap.high),
        );
        let none_value = ConceptKey::NoneValue.get(lang).to_string();
        let mitigated_value = gap.mitigated_at.map(|i| i.to_string()).unwrap_or_else(|| none_value.clone());
        let filled_value = gap.filled_at.map(|i| i.to_string()).unwrap_or(none_value);
        vec![
            StatRow::new(ConceptKey::SmcDirectionLabel.get(lang), direction_value, None),
            StatRow::new(ConceptKey::SmcZoneLabel.get(lang), &zone_value, None),
            StatRow::new(ConceptKey::ZoneMitigatedLabel.get(lang), &mitigated_value, None),
            StatRow::new(ConceptKey::ZoneFilledLabel.get(lang), &filled_value, None),
        ]
    }
}

impl Primitive for FairValueGap {
    fn type_id(&self) -> &'static str { "fair_value_gap" }
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
        let Some(gap) = self.detect(bars) else { return HitTestResult::Miss };
        let right_bar = self.right_edge_bar(&gap, bars);
        let x1 = vp.bar_to_x_f64(gap.start_index as f64);
        let x2 = vp.bar_to_x_f64(right_bar as f64);
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        let y1 = vp.price_to_y(gap.low, ps.price_min, ps.price_max);
        let y2 = vp.price_to_y(gap.high, ps.price_min, ps.price_max);
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
            Some(gap) => vp.price_to_y((gap.low + gap.high) / 2.0, ps.price_min, ps.price_max),
            None => vp.price_to_y((ps.price_min + ps.price_max) / 2.0, ps.price_min, ps.price_max),
        };
        vec![ControlPoint::point1(x, cy)]
    }

    /// A filled/stroked box over the resolved gap, tinted
    /// `guard::VIOLATION_COLOR` when nothing resolves AND
    /// `SmcEngineConfig::judge_claims` is on. Recomputed FRESH from `bars`
    /// here, matching [`Primitive::refresh_bar_derived_cache`]'s own
    /// contract.
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let bars = ctx.bars();
        let detected = self.detect(bars);
        let anchor_span = super::super::anchor_mark::bar_span_at(bars, self.anchor_ts);
        let judge_claims = smc_engine_config().judge_claims;
        let violated = judge_claims && detected.is_none();
        let stroke = if violated { guard::VIOLATION_COLOR.to_string() } else { self.data.color.stroke.clone() };

        let Some(gap) = detected else {
            let anchor_x = ctx.ts_to_x_ms(self.anchor_ts);
            super::super::anchor_mark::draw_unresolved_anchor(
                ctx, anchor_x, anchor_span, &stroke, self.data.width,
            );
            return;
        };

        let right_bar = self.right_edge_bar(&gap, bars);
        let x1 = ctx.bar_to_x(gap.start_index as f64);
        let x2 = ctx.bar_to_x(right_bar as f64);
        let y1 = ctx.price_to_y(gap.low);
        let y2 = ctx.price_to_y(gap.high);
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
            let label = match gap.direction {
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

    fn refresh_bar_derived_cache(&mut self, bars: &[Bar]) {
        let detected = self.detect(bars);
        self.cached_violations = if smc_engine_config().judge_claims {
            violations_for(detected.as_ref())
        } else {
            Vec::new()
        };
        self.cached_gap = detected;
    }

    fn context_violations(&self) -> &[Violation] { &self.cached_violations }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::extend_right(self.extend_right).with_order(10),
            ConfigProperty::fill_opacity(self.fill_opacity * 100.0).with_order(11),
            ConfigProperty::show_labels(self.show_labels).with_order(12),
            ConfigProperty::stat_block(FVG_RESULT_STATS_ID, self.result_stat_rows()).with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            FVG_RESULT_STATS_ID => {}
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
        type_id: "fair_value_gap",
        display_name: "Fair Value Gap",
        kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::SingleClick,
        tooltip: "Claim a fair value gap centred on this candle",
        icon: "fair_value_gap",
        default_color: "#26C6DA",
        factory: |points, color| {
            let (t, _) = points.first().copied().unwrap_or((0, 0.0));
            Box::new(FairValueGap::new(t, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reproduces `mli::smc::zones`'s own `bullish_fvg_mitigation_then_fill`
    /// fixture: a bullish gap across bars 0/1/2 (zone `[100, 103]`),
    /// mitigated at bar 4 and filled at bar 5.
    fn fvg_fixture_bars() -> Vec<Bar> {
        vec![
            Bar::new(0, 99.0, 100.0, 98.0, 99.5),
            Bar::new(1, 100.0, 108.0, 105.0, 107.0),
            Bar::new(2, 105.0, 112.0, 103.0, 110.0),
            Bar::new(3, 110.0, 111.0, 107.5, 109.5),
            Bar::new(4, 109.0, 109.5, 101.0, 108.0),
            Bar::new(5, 108.0, 108.5, 95.0, 96.0),
        ]
    }

    fn reset_engine_config() -> std::sync::MutexGuard<'static, ()> {
        use crate::state::smc_engine::{engine_config_test_lock, set_smc_engine_config, SmcEngineConfig};
        let guard = engine_config_test_lock();
        set_smc_engine_config(SmcEngineConfig::default());
        guard
    }

    #[test]
    fn claim_on_the_real_middle_bar_reports_no_violation() {
        let _guard = reset_engine_config();
        let bars = fvg_fixture_bars();
        let mut claim = FairValueGap::new(1_000, "#26C6DA"); // bar 1 is the middle bar
        claim.refresh_bar_derived_cache(&bars);
        let gap = claim.cached_gap.expect("expected a resolved gap");
        assert_eq!(gap.start_index, 0);
        assert_eq!(gap.end_index, 2);
        assert_eq!(gap.low, 100.0);
        assert_eq!(gap.high, 103.0);
        assert_eq!(gap.direction, mli_smc::Direction::Up);
        assert_eq!(gap.mitigated_at, Some(4));
        assert_eq!(gap.filled_at, Some(5));
        assert!(claim.context_violations().is_empty(), "{:?}", claim.context_violations());
    }

    #[test]
    fn claim_on_a_bar_with_no_gap_reports_not_formed() {
        let _guard = reset_engine_config();
        let bars = fvg_fixture_bars();
        let mut claim = FairValueGap::new(3_000, "#26C6DA"); // bars 2/3/4 form no gap
        claim.refresh_bar_derived_cache(&bars);
        assert!(claim.cached_gap.is_none());
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "fvg_not_formed");
    }

    #[test]
    fn render_cache_is_absent_from_to_json() {
        let _guard = reset_engine_config();
        let bars = fvg_fixture_bars();
        let mut claim = FairValueGap::new(1_000, "#26C6DA");
        claim.refresh_bar_derived_cache(&bars);
        let json = claim.to_json();
        assert!(!json.contains("cached_gap"), "{json}");
        assert!(!json.contains("cached_violations"), "{json}");

        let mut registry = super::super::super::registry::PrimitiveRegistry::new();
        registry.register_typed::<FairValueGap>(metadata());
        let restored = registry.from_json("fair_value_gap", &json).expect("round trip");
        assert_eq!(restored.points(), claim.points());
    }

    #[test]
    fn judge_claims_off_silences_the_guard() {
        use crate::state::smc_engine::{set_smc_engine_config, SmcEngineConfig};
        let _guard = reset_engine_config();
        let bars = fvg_fixture_bars();
        let mut claim = FairValueGap::new(3_000, "#26C6DA"); // no gap here
        set_smc_engine_config(SmcEngineConfig { judge_claims: false, ..SmcEngineConfig::default() });
        claim.refresh_bar_derived_cache(&bars);
        assert!(claim.cached_gap.is_none());
        assert!(claim.context_violations().is_empty(), "judging itself is off");
    }
}
