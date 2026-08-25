//! Dealing Range / Premium-Discount — pure geometry over a user-drawn range
//! (`docs/mlc/plans/markup-engines-2026-08-25.md`, Appendix A, "Dealing
//! range / premium-discount").
//!
//! TWO points: the range's own two ends, in either order. Appendix A: "the
//! upper half is premium, the lower half discount, the midpoint
//! equilibrium... no detection, no parameters, and therefore no excuse for
//! it to disagree with the Fibonacci tool already in the chart." Checked
//! against `fibonacci::retracement::FibRetracement::price_at_level` before
//! writing this module's own maths: that method is INLINED on
//! `FibRetracement` itself (`self.price1 + (self.price2 - self.price1) *
//! level`), not exposed as a free function shareable without editing that
//! committed file, so [`ote_bounds`] below writes the exact same two-line
//! formula locally rather than refactoring it.
//!
//! A `Pills` row (`source`) picks where the range's own bounds come from:
//! `"manual"` (default) — exactly the two drawn points, so "a freshly drawn
//! tool must do exactly what the user drew" — or `"from_structure"` — the
//! range `mli::smc::market_structure::dealing_range` reports over the
//! bounded judgement window ENDING AT the later of the two drawn points
//! (see [`DealingRange::from_structure`]'s own doc for why the forward
//! bound differs from every other claim in this family). In `"manual"`
//! there is nothing to be wrong about, so this primitive reports no
//! violation at all in that mode — never a rule invented for a tool that
//! makes no claim.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64, find_bar_for_timestamp_ms};
use crate::i18n::{current_language, ConceptKey};
use crate::state::smc_engine::smc_engine_config;
use mli::smc as mli_smc;
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp,
    config::{ConfigProperty, PropertyValue, StatRow, SelectOption},
    elliott::guard::{self, Violation},
    default_area_fill_opacity,
};
use super::smc_judge::judge_window;

fn default_source() -> String {
    "manual".to_string()
}

/// Premium-half fill tint — the same "sell zone" literal the up/down pair
/// in this family already uses for the bearish side
/// (`patterns::candle_pattern_scan::DOWN_COLOR`).
const PREMIUM_COLOR: &str = "#ef5350";
/// Discount-half fill tint — the bullish-side counterpart
/// (`patterns::candle_pattern_scan::UP_COLOR`).
const DISCOUNT_COLOR: &str = "#26a69a";
/// OTE-band tint — neutral so it reads as a reference band, not a third
/// directional zone.
const OTE_COLOR: &str = "#FFD54F";

const DEALING_RANGE_RESULT_STATS_ID: &str = "dealing_range_result_stats";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DealingRange {
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64,
    #[serde(default)]
    pub price1: f64,
    #[serde(default)]
    pub ts2: i64,
    #[serde(default)]
    pub price2: f64,
    /// `"manual"` or `"from_structure"` — see the module's own doc. Default
    /// `"manual"`.
    #[serde(default = "default_source")]
    pub source: String,
    /// Whether the 0.618-0.79 OTE band draws behind the range. Default
    /// `false` — Appendix A names it "optionally".
    #[serde(default)]
    pub show_ote: bool,
    /// Premium/discount zone fill opacity, 0.0-1.0.
    #[serde(default = "default_area_fill_opacity")]
    pub fill_opacity: f64,

    /// This tool's own resolved `(low, high)` — `None` only in
    /// `"from_structure"` mode when no confirmed swing pair exists in the
    /// window; always `Some` in `"manual"` mode. See
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    #[serde(skip)]
    pub cached_bounds: Option<(f64, f64)>,
    /// This claim's own guard rule — see [`Primitive::context_violations`]'s
    /// own contract. Always empty in `"manual"` mode.
    #[serde(skip)]
    pub cached_violations: Vec<Violation>,
}

impl DealingRange {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "dealing_range".to_string(),
                display_name: "Dealing Range".to_string(),
                color: PrimitiveColor::with_fill(color, color),
                width: 2.0,
                ..Default::default()
            },
            ts1,
            price1,
            ts2,
            price2,
            source: default_source(),
            show_ote: false,
            fill_opacity: default_area_fill_opacity(),
            cached_bounds: None,
            cached_violations: Vec::new(),
        }
    }

    /// `mli::smc::market_structure::dealing_range`'s own resolved range, run
    /// over the judgement window bounded to END at the later of the two
    /// drawn points — unlike every other SMC claim in this family (whose
    /// window always extends forward to the last loaded bar, because their
    /// own mitigation/breaker/recovery state looks forward from the event
    /// that creates it), this tool's own range must NOT keep looking past
    /// the analyst's own later anchor: Appendix A's own range is "taken
    /// from the last confirmed swing pair" as of where the tool was
    /// drawn, and a window that kept extending would let a swing that
    /// formed AFTER the tool was placed silently redefine it on every
    /// later refresh.
    fn from_structure(&self, bars: &[Bar]) -> Option<mli_smc::DealingRange> {
        let anchor_ts = self.ts1.max(self.ts2);
        let anchor_idx = find_bar_for_timestamp_ms(bars, anchor_ts)?;
        if anchor_idx >= bars.len() {
            return None;
        }
        let engine_cfg = smc_engine_config();
        let (window, offset) = judge_window(bars, anchor_idx, engine_cfg.judge_lookback_bars);
        let local_anchor = anchor_idx.checked_sub(offset)?;
        let bounded = window.get(..=local_anchor)?;
        let cfg = engine_cfg.to_smc_config();
        mli_smc::dealing_range(bounded, &cfg).map(|r| mli_smc::DealingRange {
            low: r.low,
            high: r.high,
            low_index: r.low_index + offset,
            high_index: r.high_index + offset,
        })
    }

    /// The resolved `(low, high)` this tool currently claims. `"manual"`:
    /// exactly the two drawn points, always `Some`. `"from_structure"`:
    /// `mli`'s own resolved range, `None` when no confirmed swing pair
    /// exists in the window.
    pub fn resolved_bounds(&self, bars: &[Bar]) -> Option<(f64, f64)> {
        if self.source == "from_structure" {
            self.from_structure(bars).map(|r| (r.low, r.high))
        } else {
            Some((self.price1.min(self.price2), self.price1.max(self.price2)))
        }
    }

    /// Strictly above the resolved equilibrium — `false` when nothing has
    /// resolved.
    pub fn is_premium(&self, price: f64) -> bool {
        self.cached_bounds.map(|(low, high)| price > (low + high) / 2.0).unwrap_or(false)
    }

    /// Strictly below the resolved equilibrium — `false` when nothing has
    /// resolved.
    pub fn is_discount(&self, price: f64) -> bool {
        self.cached_bounds.map(|(low, high)| price < (low + high) / 2.0).unwrap_or(false)
    }

    /// The read-only Stat block's own rows — high, low, equilibrium, and
    /// range size — all read from [`Self::cached_bounds`]. Empty when
    /// nothing has resolved.
    pub fn result_stat_rows(&self) -> Vec<StatRow> {
        let lang = current_language();
        let Some((low, high)) = self.cached_bounds else { return Vec::new() };
        let equilibrium = (low + high) / 2.0;
        vec![
            StatRow::new(ConceptKey::DealingRangeHighLabel.get(lang), &crate::drawing::primitives_v2::fmt_price(high), None),
            StatRow::new(ConceptKey::DealingRangeLowLabel.get(lang), &crate::drawing::primitives_v2::fmt_price(low), None),
            StatRow::new(ConceptKey::DealingRangeEquilibriumLabel.get(lang), &crate::drawing::primitives_v2::fmt_price(equilibrium), None),
            StatRow::new(ConceptKey::DealingRangeSizeLabel.get(lang), &crate::drawing::primitives_v2::fmt_price(high - low), None),
        ]
    }
}

impl Primitive for DealingRange {
    fn type_id(&self) -> &'static str { "dealing_range" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Pattern }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, self.price1), (self.ts2, self.price2)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(t, p)) = pts.first() { self.ts1 = t; self.price1 = p; }
        if let Some(&(t, p)) = pts.get(1) { self.ts2 = t; self.price2 = p; }
    }
    fn translate(&mut self, td: i64, pd: f64) {
        self.ts1 += td; self.ts2 += td;
        self.price1 += pd; self.price2 += pd;
    }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.ts1 = ts_ms; self.price1 = price; }
            ControlPointType::Point2 => { self.ts2 = ts_ms; self.price2 = price; }
            ControlPointType::Move => {
                let td = ts_ms - self.ts1;
                let pd = price - self.price1;
                self.translate(td, pd);
            }
            _ => {}
        }
    }

    /// The two anchor handles, then the resolved box's own bounding area.
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2));
        let y1 = vp.price_to_y(self.price1, ps.price_min, ps.price_max);
        let y2 = vp.price_to_y(self.price2, ps.price_min, ps.price_max);
        if (sx - x1).abs() < HIT_TOLERANCE && (sy - y1).abs() < HIT_TOLERANCE {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        if (sx - x2).abs() < HIT_TOLERANCE && (sy - y2).abs() < HIT_TOLERANCE {
            return HitTestResult::ControlPoint(ControlPointType::Point2);
        }
        let Some((low, high)) = self.resolved_bounds(bars) else { return HitTestResult::Miss };
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        let y_low = vp.price_to_y(low, ps.price_min, ps.price_max);
        let y_high = vp.price_to_y(high, ps.price_min, ps.price_max);
        let (min_y, max_y) = (y_high.min(y_low), y_high.max(y_low));
        if sx >= min_x - HIT_TOLERANCE && sx <= max_x + HIT_TOLERANCE
            && sy >= min_y - HIT_TOLERANCE && sy <= max_y + HIT_TOLERANCE
        {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2));
        let y1 = vp.price_to_y(self.price1, ps.price_min, ps.price_max);
        let y2 = vp.price_to_y(self.price2, ps.price_min, ps.price_max);
        vec![ControlPoint::point1(x1, y1), ControlPoint::point2(x2, y2)]
    }

    /// The range box, the equilibrium line, premium/discount shading, and
    /// — behind [`Self::show_ote`] — the OTE band. Tinted
    /// `guard::VIOLATION_COLOR` when `"from_structure"` resolves nothing
    /// AND `SmcEngineConfig::judge_claims` is on. Recomputed FRESH from
    /// `bars` here (never reads [`Self::cached_bounds`]), matching
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let bars = ctx.bars();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        let bounds = self.resolved_bounds(bars);
        let judge_claims = smc_engine_config().judge_claims;
        let violated = judge_claims && self.source == "from_structure" && bounds.is_none();
        let stroke = if violated { guard::VIOLATION_COLOR.to_string() } else { self.data.color.stroke.clone() };

        let Some((low, high)) = bounds else {
            // Nothing resolved (`"from_structure"` with no confirmed pair):
            // draw just the two anchor ticks, still recolored when judging
            // is on — same "still draws, still recolors" shape every claim
            // in this family uses.
            let chart_height = ctx.chart_height();
            ctx.set_stroke_color(&stroke);
            ctx.set_stroke_width(self.data.width);
            ctx.set_line_dash(&[]);
            for x in [x1, x2] {
                ctx.begin_path();
                ctx.move_to(crisp(x, dpr), 0.0);
                ctx.line_to(crisp(x, dpr), chart_height);
                ctx.stroke();
            }
            return;
        };

        let equilibrium = (low + high) / 2.0;
        let y_high = ctx.price_to_y(high);
        let y_low = ctx.price_to_y(low);
        let y_eq = ctx.price_to_y(equilibrium);

        // Premium (upper) half, then discount (lower) half.
        ctx.set_fill_color_alpha(PREMIUM_COLOR, self.fill_opacity);
        ctx.fill_rect(min_x, y_high, max_x - min_x, y_eq - y_high);
        ctx.set_fill_color_alpha(DISCOUNT_COLOR, self.fill_opacity);
        ctx.fill_rect(min_x, y_eq, max_x - min_x, y_low - y_eq);
        ctx.reset_alpha();

        if self.show_ote {
            let (ote_low, ote_high) = ote_bounds(low, high);
            let y_ote_top = ctx.price_to_y(ote_high);
            let y_ote_bottom = ctx.price_to_y(ote_low);
            ctx.set_fill_color_alpha(OTE_COLOR, (self.fill_opacity * 2.0).min(1.0));
            ctx.fill_rect(min_x, y_ote_top, max_x - min_x, y_ote_bottom - y_ote_top);
            ctx.reset_alpha();
        }

        ctx.set_stroke_color(&stroke);
        ctx.set_stroke_width(self.data.width);
        ctx.set_line_dash(&[]);
        ctx.stroke_rect(crisp(min_x, dpr), crisp(y_high, dpr), max_x - min_x, y_low - y_high);

        ctx.set_line_dash(&[4.0 * dpr, 4.0 * dpr]);
        ctx.begin_path();
        ctx.move_to(crisp(min_x, dpr), crisp(y_eq, dpr));
        ctx.line_to(crisp(max_x, dpr), crisp(y_eq, dpr));
        ctx.stroke();
        ctx.set_line_dash(&[]);
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    /// Recompute [`Self::cached_bounds`]/[`Self::cached_violations`] from
    /// `bars` through the SAME [`Self::resolved_bounds`] call `render`
    /// makes directly. `"manual"` mode never populates
    /// [`Self::cached_violations`] regardless of `judge_claims` — Appendix
    /// A: "in Manual there is nothing to be wrong about."
    fn refresh_bar_derived_cache(&mut self, bars: &[Bar]) {
        let bounds = self.resolved_bounds(bars);
        self.cached_violations = if smc_engine_config().judge_claims && self.source == "from_structure" && bounds.is_none() {
            vec![Violation::whole("dealing_range_no_structure", ConceptKey::RuleDealingRangeNoStructure)]
        } else {
            Vec::new()
        };
        self.cached_bounds = bounds;
    }

    fn context_violations(&self) -> &[Violation] { &self.cached_violations }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        let lang = current_language();
        vec![
            ConfigProperty::pills(
                "source",
                ConceptKey::DealingRangeSourceLabel.get(lang),
                &self.source,
                vec![
                    SelectOption::new("manual", ConceptKey::DealingRangeSourceManual.get(lang)),
                    SelectOption::new("from_structure", ConceptKey::DealingRangeSourceFromStructure.get(lang)),
                ],
            ).with_order(10),
            ConfigProperty::boolean("show_ote", ConceptKey::DealingRangeShowOteLabel.get(lang), self.show_ote).with_order(11),
            ConfigProperty::fill_opacity(self.fill_opacity * 100.0).with_order(12),
            ConfigProperty::stat_block(DEALING_RANGE_RESULT_STATS_ID, self.result_stat_rows()).with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            DEALING_RANGE_RESULT_STATS_ID => {}
            "source" => {
                if let Some(s) = value.as_string() {
                    if s == "manual" || s == "from_structure" {
                        self.source = s.to_string();
                        return true;
                    }
                }
            }
            "show_ote" => { if let Some(v) = value.as_bool() { self.show_ote = v; return true; } }
            "fill_opacity" => { if let Some(v) = value.as_number() { self.fill_opacity = (v / 100.0).clamp(0.0, 1.0); return true; } }
            _ => {}
        }
        false
    }
}

/// The same two-anchor linear interpolation `FibRetracement::price_at_level`
/// uses (`a + (b - a) * level`, written locally per the module's own doc),
/// applied to this range's own resolved `high` → `low` anchors: level 0 =
/// `high`, level 1 = `low` — the ICT convention under which the 0.618-0.79
/// OTE band sits inside the discount half. Free function rather than a
/// method: it depends only on the resolved bounds, not on which of
/// `"manual"`/`"from_structure"` produced them.
fn ote_bounds(low: f64, high: f64) -> (f64, f64) {
    let a = high + (low - high) * 0.618;
    let b = high + (low - high) * 0.79;
    (a.min(b), a.max(b))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "dealing_range",
        display_name: "Dealing Range",
        kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Mark a premium/discount dealing range",
        icon: "dealing_range",
        default_color: "#78909C",
        factory: |points, color| {
            let (t1, p1) = points.first().copied().unwrap_or((0, 0.0));
            let (t2, p2) = points.get(1).copied().unwrap_or((t1 + 1_200_000, p1));
            Box::new(DealingRange::new(t1, p1, t2, p2, color))
        },
        supports_text: false,
        has_levels: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reproduces `mli::smc::market_structure`'s own `structure_fixture`: a
    /// confirmed uptrend, last swing low 20 at bar 5, last swing high 40 at
    /// bar 7 (confirmed at bar 8 with `swing_n = 1`).
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

    /// Same `engine_config_test_lock`/reset idiom every SMC test module in
    /// this crate uses.
    fn reset_engine_config() -> std::sync::MutexGuard<'static, ()> {
        use crate::state::smc_engine::{engine_config_test_lock, set_smc_engine_config, SmcEngineConfig};
        let guard = engine_config_test_lock();
        set_smc_engine_config(SmcEngineConfig::default());
        guard
    }

    fn set_fixture_config() {
        use crate::state::smc_engine::{set_smc_engine_config, SmcEngineConfig};
        set_smc_engine_config(SmcEngineConfig { swing_n: 1, ..SmcEngineConfig::default() });
    }

    #[test]
    fn equilibrium_is_the_midpoint() {
        let _guard = reset_engine_config();
        let bars = structure_fixture_bars();
        let mut dr = DealingRange::new(0, 20.0, 1_000, 40.0, "#78909C");
        dr.refresh_bar_derived_cache(&bars);
        let (low, high) = dr.cached_bounds.expect("manual mode always resolves");
        assert_eq!((low + high) / 2.0, 30.0);
    }

    #[test]
    fn premium_and_discount_classify_either_side_of_equilibrium() {
        let _guard = reset_engine_config();
        let bars = structure_fixture_bars();
        let mut dr = DealingRange::new(0, 20.0, 1_000, 40.0, "#78909C");
        dr.refresh_bar_derived_cache(&bars);
        assert!(dr.is_premium(35.0));
        assert!(!dr.is_discount(35.0));
        assert!(dr.is_discount(25.0));
        assert!(!dr.is_premium(25.0));
    }

    #[test]
    fn from_structure_over_a_confirmed_pair_resolves_to_that_pairs_bounds() {
        let _guard = reset_engine_config();
        set_fixture_config();
        let bars = structure_fixture_bars();
        // ts1/ts2 pick WHERE the tool sits; ts2 lands on the last bar (10),
        // so the judgement window includes bar 8 — the bar H2 (index 7)
        // confirms on with `swing_n = 1`.
        let mut dr = DealingRange::new(0, 0.0, 10_000, 0.0, "#78909C");
        dr.source = "from_structure".to_string();
        dr.refresh_bar_derived_cache(&bars);
        let (low, high) = dr.cached_bounds.expect("expected a resolved structural range");
        assert_eq!(low, 20.0);
        assert_eq!(high, 40.0);
        assert!(dr.context_violations().is_empty(), "{:?}", dr.context_violations());
    }

    #[test]
    fn from_structure_over_a_flat_fixture_reports_no_structure() {
        let _guard = reset_engine_config();
        // Default `swing_n = 2` needs `2*n+1 = 5` bars minimum; two bars
        // can never confirm a swing pair.
        let bars = vec![
            Bar::new(0, 20.0, 21.0, 19.0, 20.0),
            Bar::new(1, 20.0, 21.0, 19.0, 20.0),
        ];
        let mut dr = DealingRange::new(0, 0.0, 1_000, 0.0, "#78909C");
        dr.source = "from_structure".to_string();
        dr.refresh_bar_derived_cache(&bars);
        assert!(dr.cached_bounds.is_none());
        let violations = dr.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "dealing_range_no_structure");
    }

    #[test]
    fn manual_mode_never_reports_a_violation() {
        let _guard = reset_engine_config();
        // Same starved bars `from_structure` fails on — manual mode still
        // reports nothing wrong, because there is nothing to be wrong about.
        let bars = vec![
            Bar::new(0, 20.0, 21.0, 19.0, 20.0),
            Bar::new(1, 20.0, 21.0, 19.0, 20.0),
        ];
        let mut dr = DealingRange::new(0, 15.0, 1_000, 25.0, "#78909C");
        dr.refresh_bar_derived_cache(&bars);
        assert!(dr.cached_bounds.is_some());
        assert!(dr.context_violations().is_empty());
    }

    #[test]
    fn judge_claims_off_silences_the_guard() {
        use crate::state::smc_engine::{set_smc_engine_config, SmcEngineConfig};
        let _guard = reset_engine_config();
        let bars = vec![
            Bar::new(0, 20.0, 21.0, 19.0, 20.0),
            Bar::new(1, 20.0, 21.0, 19.0, 20.0),
        ];
        let mut dr = DealingRange::new(0, 0.0, 1_000, 0.0, "#78909C");
        dr.source = "from_structure".to_string();
        set_smc_engine_config(SmcEngineConfig { judge_claims: false, ..SmcEngineConfig::default() });
        dr.refresh_bar_derived_cache(&bars);
        assert!(dr.cached_bounds.is_none(), "the resolution still runs — the bars still don't qualify");
        assert!(dr.context_violations().is_empty(), "judging itself is off");
    }

    #[test]
    fn render_cache_is_absent_from_to_json() {
        let _guard = reset_engine_config();
        let bars = structure_fixture_bars();
        let mut dr = DealingRange::new(0, 20.0, 1_000, 40.0, "#78909C");
        dr.refresh_bar_derived_cache(&bars);
        let json = dr.to_json();
        assert!(!json.contains("cached_bounds"), "{json}");
        assert!(!json.contains("cached_violations"), "{json}");

        let mut registry = super::super::super::registry::PrimitiveRegistry::new();
        registry.register_typed::<DealingRange>(metadata());
        let restored = registry.from_json("dealing_range", &json).expect("round trip");
        assert_eq!(restored.points(), dr.points());
    }
}
