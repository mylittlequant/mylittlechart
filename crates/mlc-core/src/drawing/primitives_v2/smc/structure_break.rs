//! Structure Break — the CLAIM half of the smart-money BOS/CHoCH markup
//! primitive (`docs/mlc/plans/markup-engines-2026-08-25.md`, Appendix A,
//! "BOS — break of structure" and "CHoCH — change of character").
//!
//! ONE point: the bar the analyst claims closed beyond structure. Unlike
//! every other claim in this family, this one also CLAIMS WHICH KIND it is
//! (`claimed_kind`, a `Pills` row: BOS or CHoCH) — Appendix A's own
//! insistence that "CHoCH is not a stronger BOS; it is the event that ends a
//! trend, and the two must be separate rule ids" is why this primitive
//! carries TWO guard rules instead of one:
//! - `"structure_break_not_formed"` — `mli::smc::structure` reports no event
//!   at this bar at all.
//! - `"structure_break_kind_mismatch"` — an event IS there, but it is the
//!   OTHER kind than what was claimed.
//!
//! The resolved event (level, broken swing, direction) and the prevailing
//! trend at that bar are both DERIVED — `mli::smc::structure`/`mli::smc::
//! trend_at` run over a bounded judgement window
//! (`crate::state::smc_engine::SmcEngineConfig::judge_lookback_bars`,
//! resolved via [`super::smc_judge::judge_window`]) — never stored.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64, find_bar_for_timestamp_ms};
use crate::i18n::{current_language, ConceptKey};
use crate::state::smc_engine::smc_engine_config;
use mli::smc as mli_smc;
use super::super::markup;
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, render_primitive_text, TextAlign,
    point_to_line_distance,
    config::{ConfigProperty, PropertyValue, StatRow, SelectOption},
    elliott::guard::{self, Violation},
};

fn default_true() -> bool {
    true
}

fn default_claimed_kind() -> String {
    "bos".to_string()
}

/// This claim's own two guard rules — see the module's own doc for why they
/// must stay separate rule ids.
fn violations_for(claimed_kind: &str, detected: Option<&mli_smc::StructureEvent>) -> Vec<Violation> {
    match detected {
        None => vec![Violation::whole("structure_break_not_formed", ConceptKey::RuleStructureBreakNotFormed)],
        Some(event) => {
            let matches = matches!(
                (claimed_kind, event.kind),
                ("bos", mli_smc::StructureEventKind::Bos) | ("choch", mli_smc::StructureEventKind::Choch)
            );
            if matches {
                Vec::new()
            } else {
                vec![Violation::whole("structure_break_kind_mismatch", ConceptKey::RuleStructureBreakKindMismatch)]
            }
        }
    }
}

const STRUCTURE_BREAK_RESULT_STATS_ID: &str = "structure_break_result_stats";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StructureBreak {
    pub data: PrimitiveData,
    /// Timestamp (ms) of the bar the analyst claims closed beyond structure.
    #[serde(default)]
    pub anchor_ts: i64,
    /// Which kind the analyst claims this break is — `"bos"` or `"choch"`.
    /// Defaults to `"bos"` (continuation is the more common claim; either
    /// way the guard judges it against `mli::smc::structure`'s own verdict,
    /// so a wrong default costs nothing but an initial orange tint).
    #[serde(default = "default_claimed_kind")]
    pub claimed_kind: String,
    /// Whether the claimed-kind label draws on the level line.
    #[serde(default = "default_true")]
    pub show_labels: bool,

    /// This claim's own containment edge — which dealing range or AMD
    /// cycle it sits inside, and in which of that parent's slots. Persisted
    /// (unlike the derived caches below) and maintained wholesale by
    /// `mlc_backend::bubble::markup_guards`; see
    /// [`markup::MarkupLink`]'s own module doc for why it is stored rather
    /// than recomputed on every read.
    #[serde(default)]
    pub markup_link: Option<markup::MarkupLink>,

    /// This claim's own succession edge — the claim it continues in its
    /// family's own narrative. Persisted and maintained wholesale by
    /// `mlc_backend::bubble::markup_guards`, exactly like
    /// [`Self::markup_link`]; `markup::chain`'s own module doc explains why
    /// this edge is derived where Elliott's is declared.
    #[serde(default)]
    pub markup_follows: Option<markup::MarkupFollows>,

    /// This claim's own resolved event, with every bar index already
    /// converted back to GLOBAL — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    #[serde(skip)]
    pub cached_event: Option<mli_smc::StructureEvent>,
    /// The prevailing trend at the anchor bar, from the SAME judgement
    /// window [`Self::cached_event`] was resolved against — Appendix A:
    /// "anything else is unresolved and must be reported as unresolved, not
    /// defaulted to a direction," reported verbatim by
    /// [`Self::result_stat_rows`].
    #[serde(skip)]
    pub cached_trend: Option<mli_smc::Trend>,
    /// This claim's own guard rule(s) — see
    /// [`Primitive::context_violations`]'s own contract.
    #[serde(skip)]
    pub cached_violations: Vec<Violation>,
}

impl StructureBreak {
    pub fn new(anchor_ts: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "structure_break".to_string(),
                display_name: "Structure Break".to_string(),
                color: PrimitiveColor::new(color),
                width: 2.0,
                ..Default::default()
            },
            anchor_ts,
            claimed_kind: default_claimed_kind(),
            show_labels: true,
            cached_event: None,
            cached_trend: None,
            markup_link: None,
            markup_follows: None,
            cached_violations: Vec::new(),
        }
    }

    fn anchor_bar_idx(&self, bars: &[Bar]) -> Option<usize> {
        let idx = find_bar_for_timestamp_ms(bars, self.anchor_ts)?;
        (idx < bars.len()).then_some(idx)
    }

    /// This claim's own resolved event AND the prevailing trend at the same
    /// bar — a SINGLE window resolution ([`super::smc_judge::judge_window`])
    /// backs both, via `mli::smc::structure`/`mli::smc::trend_at`, so the
    /// two can never disagree about which bars they were judged against.
    /// Every bar index on the returned event is already GLOBAL.
    pub fn resolve(&self, bars: &[Bar]) -> (Option<mli_smc::StructureEvent>, Option<mli_smc::Trend>) {
        let Some(anchor_idx) = self.anchor_bar_idx(bars) else { return (None, None) };
        let engine_cfg = smc_engine_config();
        let (window, offset) = super::smc_judge::judge_window(bars, anchor_idx, engine_cfg.judge_lookback_bars);
        let cfg = engine_cfg.to_smc_config();
        let Some(local_target) = anchor_idx.checked_sub(offset) else { return (None, None) };
        let event = mli_smc::structure(&window, &cfg)
            .into_iter()
            .find(|e| e.index == local_target)
            .map(|e| mli_smc::StructureEvent {
                index: e.index + offset,
                kind: e.kind,
                direction: e.direction,
                broken_swing_index: e.broken_swing_index + offset,
                level: e.level,
            });
        let trend = Some(mli_smc::trend_at(&window, &cfg, local_target));
        (event, trend)
    }

    /// The read-only Stat block's own rows — the broken level, the broken
    /// swing's own bar index, and the prevailing trend at the anchor bar
    /// (printed via [`ConceptKey::TrendValueUp`]/`Down`/`Unresolved` —
    /// `Unresolved` is a real printed value, never defaulted to a
    /// direction). Both read from [`Self::cached_event`]/[`Self::cached_trend`].
    pub fn result_stat_rows(&self) -> Vec<StatRow> {
        let lang = current_language();
        let Some(event) = &self.cached_event else { return Vec::new() };
        let level_value = crate::drawing::primitives_v2::fmt_price(event.level);
        let trend_value = match self.cached_trend {
            Some(mli_smc::Trend::Up) => ConceptKey::TrendValueUp.get(lang),
            Some(mli_smc::Trend::Down) => ConceptKey::TrendValueDown.get(lang),
            Some(mli_smc::Trend::Unresolved) | None => ConceptKey::TrendValueUnresolved.get(lang),
        };
        vec![
            StatRow::new(ConceptKey::StructureBreakLevelLabel.get(lang), &level_value, None),
            StatRow::new(ConceptKey::StructureBreakSwingLabel.get(lang), &event.broken_swing_index.to_string(), None),
            StatRow::new(ConceptKey::TrendAtBarLabel.get(lang), trend_value, None),
        ]
    }
}

impl Primitive for StructureBreak {
    fn type_id(&self) -> &'static str { "structure_break" }
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

    /// The anchor handle, then the resolved level line from the broken
    /// swing bar to the anchor bar.
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let anchor_x = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.anchor_ts));
        if (sx - anchor_x).abs() < HIT_TOLERANCE {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        let (Some(event), _) = self.resolve(bars) else { return HitTestResult::Miss };
        let level_y = vp.price_to_y(event.level, ps.price_min, ps.price_max);
        let x1 = vp.bar_to_x_f64(event.broken_swing_index as f64);
        if point_to_line_distance(sx, sy, x1, level_y, anchor_x, level_y) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let x = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.anchor_ts));
        let cy = match self.resolve(bars) {
            (Some(event), _) => vp.price_to_y(event.level, ps.price_min, ps.price_max),
            _ => super::super::anchor_mark::anchor_control_y(bars, self.anchor_ts, vp, ps),
        };
        vec![ControlPoint::point1(x, cy)]
    }

    /// A horizontal level line from the broken swing bar to the anchor bar,
    /// tinted `guard::VIOLATION_COLOR` when the claim disagrees with `mli`
    /// AND `SmcEngineConfig::judge_claims` is on. The on-chart label always
    /// shows the CLAIMED kind (BOS/CHoCH), never the detected one — the
    /// guard tint is what tells the analyst the two disagree. Recomputed
    /// FRESH from `bars` here, matching
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let bars = ctx.bars();
        let (detected, _trend) = self.resolve(bars);
        let anchor_span = super::super::anchor_mark::bar_span_at(bars, self.anchor_ts);
        let judge_claims = smc_engine_config().judge_claims;
        let violated = judge_claims && !violations_for(&self.claimed_kind, detected.as_ref()).is_empty();
        let stroke = if violated { guard::VIOLATION_COLOR.to_string() } else { self.data.color.stroke.clone() };
        let chart_height = ctx.chart_height();
        let anchor_x = ctx.ts_to_x_ms(self.anchor_ts);

        let Some(event) = detected else {
            super::super::anchor_mark::draw_unresolved_anchor(
                ctx, anchor_x, anchor_span, &stroke, self.data.width,
            );
            return;
        };

        let x1 = ctx.bar_to_x(event.broken_swing_index as f64);
        let level_y = ctx.price_to_y(event.level);

        ctx.set_stroke_color(&stroke);
        ctx.set_stroke_width(self.data.width);
        ctx.set_line_dash(&[]);
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(level_y, dpr));
        ctx.line_to(crisp(anchor_x, dpr), crisp(level_y, dpr));
        ctx.stroke();

        if self.show_labels {
            let lang = current_language();
            let label = match self.claimed_kind.as_str() {
                "choch" => ConceptKey::StructureBreakKindChoch.get(lang),
                _ => ConceptKey::StructureBreakKindBos.get(lang),
            };
            ctx.set_fill_color(&stroke);
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(crate::render::TextAlign::Center);
            ctx.set_text_baseline(crate::render::TextBaseline::Bottom);
            ctx.fill_text(label, anchor_x, level_y - 4.0 * dpr);
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

    /// Recompute [`Self::cached_event`]/[`Self::cached_trend`]/
    /// [`Self::cached_violations`] from `bars` through the SAME
    /// [`Self::resolve`]/[`violations_for`] calls `render` makes directly —
    /// see [`Primitive::refresh_bar_derived_cache`]'s own contract.
    fn refresh_bar_derived_cache(&mut self, bars: &[Bar]) {
        let (event, trend) = self.resolve(bars);
        self.cached_violations = if smc_engine_config().judge_claims {
            violations_for(&self.claimed_kind, event.as_ref())
        } else {
            Vec::new()
        };
        self.cached_event = event;
        self.cached_trend = trend;
    }

    /// Snap this claim onto the bar its own detector names, within
    /// `claim_snap_bars` — see [`Primitive::snap_to_detected`] for the
    /// contract and why it exists. Searches outward from the placed anchor,
    /// nearest first, and restores the original anchor when nothing inside
    /// the radius resolves.
    fn snap_to_detected(&mut self, bars: &[Bar]) {
        let radius = smc_engine_config().claim_snap_bars;
        if radius == 0 || self.resolve(bars).0.is_some() {
            return;
        }
        let Some(idx) = self.anchor_bar_idx(bars) else { return };
        let placed = self.anchor_ts;
        for step in 1..=radius {
            for candidate in [idx.checked_sub(step), idx.checked_add(step).filter(|i| *i < bars.len())] {
                let Some(candidate) = candidate else { continue };
                self.anchor_ts = bars[candidate].timestamp * 1000;
                if self.resolve(bars).0.is_some() {
                    return;
                }
            }
        }
        self.anchor_ts = placed;
    }

    fn markup_link(&self) -> Option<&markup::MarkupLink> {
        self.markup_link.as_ref()
    }

    fn set_markup_link(&mut self, link: Option<markup::MarkupLink>) {
        self.markup_link = link;
    }

    /// The middle of what this claim RESOLVED to, on its own anchor bar —
    /// the point a candidate parent is asked to contain
    /// ([`Primitive::markup_anchor`]).
    ///
    /// Falls back to the anchor bar's close when nothing resolved, so a
    /// claim the engine rejects still reports where the analyst put it: a
    /// claim carrying a guard violation is exactly the one whose placement
    /// is worth framing.
    ///
    /// The bar's own timestamp is returned rather than `anchor_ts`, so a
    /// claim anchored mid-bar and one anchored on the open resolve to the
    /// same containment answer.
    fn markup_anchor(&self, bars: &[Bar]) -> Option<(i64, f64)> {
        let bar = bars.get(self.anchor_bar_idx(bars)?)?;
        Some((bar.timestamp * 1000, self.cached_event.as_ref().map(|e| e.level).unwrap_or(bar.close)))
    }

    fn markup_follows(&self) -> Option<&markup::MarkupFollows> {
        self.markup_follows.as_ref()
    }

    fn set_markup_follows(&mut self, follows: Option<markup::MarkupFollows>) {
        self.markup_follows = follows;
    }

    /// What this claim contributes to its family's own chain rules — see
    /// [`Primitive::chain_fact`].
    fn chain_fact(&self, bars: &[Bar]) -> Option<markup::ChainFact> {
        let bar = self.anchor_bar_idx(bars)?;
        // The CLAIMED kind, never the detected one: `choch_after_same_
        // direction_break` is about what the analyst asserted, and the
        // claim's own guard already reports a mismatch with `mli`.
        let variant = match self.claimed_kind.as_str() {
            "choch" => Some(markup::chain::kinds::CHOCH),
            _ => Some(markup::chain::kinds::BOS),
        };
        Some(markup::ChainFact {
            bar,
            bias: self.cached_event.as_ref().map(|e| e.direction.into()),
            variant,
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
        let lang = current_language();
        vec![
            ConfigProperty::pills(
                "break_kind",
                ConceptKey::StructureBreakKindLabel.get(lang),
                &self.claimed_kind,
                vec![
                    SelectOption::new("bos", ConceptKey::StructureBreakKindBos.get(lang)),
                    SelectOption::new("choch", ConceptKey::StructureBreakKindChoch.get(lang)),
                ],
            ).with_order(10),
            ConfigProperty::show_labels(self.show_labels).with_order(11),
            ConfigProperty::stat_block(STRUCTURE_BREAK_RESULT_STATS_ID, markup::with_link_row(self.markup_link.as_ref(), markup::with_follows_row(self.markup_follows.as_ref(), self.result_stat_rows()))).with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            STRUCTURE_BREAK_RESULT_STATS_ID => {}
            "break_kind" => {
                if let Some(s) = value.as_string() {
                    if s == "bos" || s == "choch" {
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
        type_id: "structure_break",
        display_name: "Structure Break",
        kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::SingleClick,
        tooltip: "Claim a break of structure (BOS) or change of character (CHoCH) on this bar",
        icon: "structure_break",
        default_color: "#FFA726",
        factory: |points, color| {
            let (t, _) = points.first().copied().unwrap_or((0, 0.0));
            Box::new(StructureBreak::new(t, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reproduces `mli::smc::market_structure`'s own `structure_fixture`: a
    /// confirmed uptrend, a BOS Up at bar 9 (close 45 > H2 40), a CHoCH Down
    /// at bar 10 (close 5 < L2 20).
    fn structure_fixture_bars() -> Vec<Bar> {
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
            Bar::new(9, 25.0, 45.0, 24.0, 45.0),
            Bar::new(10, 25.0, 50.0, 5.0, 5.0),
        ]
    }

    fn reset_engine_config() -> std::sync::MutexGuard<'static, ()> {
        use crate::state::smc_engine::{engine_config_test_lock, set_smc_engine_config, SmcEngineConfig};
        let guard = engine_config_test_lock();
        set_smc_engine_config(SmcEngineConfig { swing_n: 1, ..SmcEngineConfig::default() });
        guard
    }

    #[test]
    fn a_bos_claim_on_the_real_bos_bar_reports_no_violation() {
        let _guard = reset_engine_config();
        let bars = structure_fixture_bars();
        let mut claim = StructureBreak::new(9_000, "#FFA726");
        claim.claimed_kind = "bos".to_string();
        claim.refresh_bar_derived_cache(&bars);
        let event = claim.cached_event.expect("expected a resolved event");
        assert_eq!(event.kind, mli_smc::StructureEventKind::Bos);
        assert_eq!(event.direction, mli_smc::Direction::Up);
        assert_eq!(event.level, 40.0);
        assert_eq!(event.broken_swing_index, 7);
        assert_eq!(claim.cached_trend, Some(mli_smc::Trend::Up));
        assert!(claim.context_violations().is_empty(), "{:?}", claim.context_violations());
    }

    #[test]
    fn a_choch_claim_on_the_real_choch_bar_reports_no_violation() {
        let _guard = reset_engine_config();
        let bars = structure_fixture_bars();
        let mut claim = StructureBreak::new(10_000, "#FFA726");
        claim.claimed_kind = "choch".to_string();
        claim.refresh_bar_derived_cache(&bars);
        let event = claim.cached_event.expect("expected a resolved event");
        assert_eq!(event.kind, mli_smc::StructureEventKind::Choch);
        assert_eq!(event.direction, mli_smc::Direction::Down);
        assert_eq!(event.level, 20.0);
        assert!(claim.context_violations().is_empty(), "{:?}", claim.context_violations());
    }

    /// A BOS claim on the bar the detector calls CHoCH reports
    /// `structure_break_kind_mismatch`, NOT `structure_break_not_formed` —
    /// the module's own doc's whole point.
    #[test]
    fn a_bos_claim_on_a_choch_bar_reports_kind_mismatch_not_not_formed() {
        let _guard = reset_engine_config();
        let bars = structure_fixture_bars();
        let mut claim = StructureBreak::new(10_000, "#FFA726"); // bar 10 is a CHoCH
        claim.claimed_kind = "bos".to_string();
        claim.refresh_bar_derived_cache(&bars);
        assert!(claim.cached_event.is_some(), "an event DOES exist at this bar");
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "structure_break_kind_mismatch");
    }

    #[test]
    fn a_claim_on_a_bar_with_no_event_reports_not_formed() {
        let _guard = reset_engine_config();
        let bars = structure_fixture_bars();
        let mut claim = StructureBreak::new(4_000, "#FFA726"); // no event here
        claim.refresh_bar_derived_cache(&bars);
        assert!(claim.cached_event.is_none());
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "structure_break_not_formed");
    }

    #[test]
    fn render_cache_is_absent_from_to_json() {
        let _guard = reset_engine_config();
        let bars = structure_fixture_bars();
        let mut claim = StructureBreak::new(9_000, "#FFA726");
        claim.refresh_bar_derived_cache(&bars);
        let json = claim.to_json();
        assert!(!json.contains("cached_event"), "{json}");
        assert!(!json.contains("cached_trend"), "{json}");
        assert!(!json.contains("cached_violations"), "{json}");

        let mut registry = super::super::super::registry::PrimitiveRegistry::new();
        registry.register_typed::<StructureBreak>(metadata());
        let restored = registry.from_json("structure_break", &json).expect("round trip");
        assert_eq!(restored.points(), claim.points());
    }

    #[test]
    fn judge_claims_off_silences_the_guard() {
        use crate::state::smc_engine::{set_smc_engine_config, SmcEngineConfig};
        let _guard = reset_engine_config();
        let bars = structure_fixture_bars();
        let mut claim = StructureBreak::new(10_000, "#FFA726"); // CHoCH bar
        claim.claimed_kind = "bos".to_string(); // wrong claim
        set_smc_engine_config(SmcEngineConfig { swing_n: 1, judge_claims: false, ..SmcEngineConfig::default() });
        claim.refresh_bar_derived_cache(&bars);
        assert!(claim.cached_event.is_some(), "the resolution still runs");
        assert!(claim.context_violations().is_empty(), "judging itself is off");
    }
}
