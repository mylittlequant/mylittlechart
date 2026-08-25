//! Order Flow Scan — the SURVEY half of the order-flow markup engine
//! (`docs/mlc/plans/markup-engines-2026-08-25.md`, Appendix B). Sibling of
//! `smc::smc_scan::SmcScan` — same shape, same contract: two points bracket
//! a bar RANGE, and every enabled structure's own hits inside that range are
//! surveyed and rendered.
//!
//! **Cache-driven render — the one thing that makes every primitive in this
//! family different from `SmcScan`.** `SmcScan::render` recomputes
//! `resolved_scan(ctx.bars())` fresh every frame because `RenderContext`
//! exposes OHLC bars. This survey's own data (`FootprintBar`) has no such
//! accessor, so `render` reads [`OrderFlowScan::cached_hits`] — filled by
//! [`Primitive::refresh_footprint_derived_cache`] — instead of recomputing.
//!
//! Five structures are surveyable, over the survey range's own footprint-bar
//! window: `stacked_imbalance`, `absorption`, `single_print`,
//! `unfinished_business` (all read straight off `FootprintBar`'s own flags —
//! nothing recomputed), and `delta_divergence` (the one computed structure
//! in this whole family, over the SAME bounded judgement window
//! `delta_divergence::DeltaDivergence` resolves its own single claim
//! against, anchored at the survey range's own right edge — exactly how
//! `SmcScan` anchors its own `judge_window` call).
//!
//! Like `SmcScan`/`CandlePatternScan`, this is a SURVEY and carries no claim
//! of its own — EXCEPT the one rule Appendix B names for the whole family:
//! `order_flow_data_missing`, reported when the series carries no footprint
//! data at all. That is not a claim being wrong, it is the tool having
//! nothing to read — see [`Primitive::context_violations`]'s own override
//! below.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::types::FootprintBar;
use crate::i18n::{current_language, ConceptKey, Language, PrimitiveNameKey, SettingsKey};
use crate::state::orderflow_engine::orderflow_engine_config;
use mli::events::divergence::DivergenceKind;
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, render_primitive_text, TextAlign,
    config::{ConfigProperty, PropertyValue, StatRow, TableCell, parse_table_cell_id},
    elliott::guard::Violation,
};
use super::orderflow_judge::{
    AbsorptionSnapshot, ImbalanceRun, LevelHit,
    footprint_judge_window, footprint_window_indices, resolve_absorption, resolve_imbalance_run, resolve_level_flags,
};

/// The scanner family's shared collapse spacing — see
/// [`super::super::SCAN_MARKER_MIN_GAP_PX`]. Read, not re-declared — see
/// `smc::smc_scan`'s own identical import for the same rationale.
use super::super::SCAN_MARKER_MIN_GAP_PX as MIN_MARKER_GAP_PX;

const ORDER_FLOW_SCAN_TABLE_ID: &str = "order_flow_scan_structures";
const ORDER_FLOW_SCAN_STATS_ID: &str = "order_flow_scan_stats";

/// The five structures [`OrderFlowScan`] can survey — stable snake_case
/// WIRE ids, persisted on [`OrderFlowScan::enabled_structures`]. The ONE
/// place id, display label, and marker colour map for every one of the
/// five, so no other module scatters a string literal of its own for this
/// table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderFlowStructure {
    StackedImbalance,
    Absorption,
    SinglePrint,
    UnfinishedBusiness,
    DeltaDivergence,
}

impl OrderFlowStructure {
    pub const ALL: [Self; 5] = [
        Self::StackedImbalance,
        Self::Absorption,
        Self::SinglePrint,
        Self::UnfinishedBusiness,
        Self::DeltaDivergence,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::StackedImbalance => "stacked_imbalance",
            Self::Absorption => "absorption",
            Self::SinglePrint => "single_print",
            Self::UnfinishedBusiness => "unfinished_business",
            Self::DeltaDivergence => "delta_divergence",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }

    /// Three of the five reuse their own claim primitive's
    /// `PrimitiveNameKey` row rather than a second, independently-typed
    /// label; `single_print`/`unfinished_business` carry no claim primitive
    /// of their own, so they read the two [`ConceptKey`] rows this survey
    /// declares for them.
    pub fn label(self, lang: Language) -> &'static str {
        match self {
            Self::StackedImbalance => PrimitiveNameKey::ImbalanceZone.get(lang),
            Self::Absorption => PrimitiveNameKey::AbsorptionMark.get(lang),
            Self::SinglePrint => ConceptKey::SinglePrintLabel.get(lang),
            Self::UnfinishedBusiness => ConceptKey::UnfinishedBusinessLabel.get(lang),
            Self::DeltaDivergence => PrimitiveNameKey::DeltaDivergence.get(lang),
        }
    }

    /// The colour this structure's own hits draw in — the SAME
    /// `default_color` its own claim primitive's `metadata()` uses where
    /// one exists, so a survey hit and a hand-placed claim of the same kind
    /// read as the same instrument.
    fn color(self) -> &'static str {
        match self {
            Self::StackedImbalance => "#26A69A",
            Self::Absorption => "#EF5350",
            Self::SinglePrint => "#FFA726",
            Self::UnfinishedBusiness => "#8D6E63",
            Self::DeltaDivergence => "#AB47BC",
        }
    }
}

fn default_all_structure_ids() -> Vec<String> {
    OrderFlowStructure::ALL.iter().map(|s| s.id().to_string()).collect()
}

/// One delta-divergence hit as this survey caches it — augmented with both
/// endpoints' own timestamps, since `render` has no footprint of its own to
/// look either up in (see the module's own doc).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScanDivergenceHit {
    pub from_ts: i64,
    pub to_ts: i64,
    pub price_from: f64,
    pub price_to: f64,
    pub kind: DivergenceKind,
}

/// This survey's own resolved hits, one field per structure — a RENDER
/// CACHE, never persisted (see the module's own doc).
#[derive(Clone, Debug, Default)]
pub struct OrderFlowScanHits {
    pub stacked_imbalance: Vec<ImbalanceRun>,
    pub absorption: Vec<AbsorptionSnapshot>,
    pub single_print: Vec<LevelHit>,
    pub unfinished_business: Vec<LevelHit>,
    pub delta_divergence: Vec<ScanDivergenceHit>,
}

impl OrderFlowScanHits {
    pub fn total(&self) -> usize {
        self.stacked_imbalance.len()
            + self.absorption.len()
            + self.single_print.len()
            + self.unfinished_business.len()
            + self.delta_divergence.len()
    }

    fn count_for(&self, kind: OrderFlowStructure) -> usize {
        match kind {
            OrderFlowStructure::StackedImbalance => self.stacked_imbalance.len(),
            OrderFlowStructure::Absorption => self.absorption.len(),
            OrderFlowStructure::SinglePrint => self.single_print.len(),
            OrderFlowStructure::UnfinishedBusiness => self.unfinished_business.len(),
            OrderFlowStructure::DeltaDivergence => self.delta_divergence.len(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderFlowScan {
    pub data: PrimitiveData,
    #[serde(default)] pub ts1: i64,
    #[serde(default)] pub ts2: i64,
    /// [`OrderFlowStructure::id`] wire ids — serialized as ids, never as an
    /// enum/index, so a saved scan survives a catalogue that grows (same
    /// convention `SmcScan::enabled_structures` uses). Default: all five.
    #[serde(default = "default_all_structure_ids")]
    pub enabled_structures: Vec<String>,

    /// This scan's own resolved `[start, end]` FOOTPRINT-bar-index window —
    /// see [`Primitive::refresh_footprint_derived_cache`]'s own contract.
    #[serde(skip)]
    pub cached_window: Option<(usize, usize)>,
    /// This scan's own resolved hits — see the module's own doc.
    /// `render` reads this directly (unlike `SmcScan::render`, which
    /// recomputes fresh — see the module's own doc for why this family
    /// cannot).
    #[serde(skip)]
    pub cached_hits: OrderFlowScanHits,
    /// See `imbalance_zone::ImbalanceZone::cached_footprint_present`'s own
    /// doc — identical contract, identical reason.
    #[serde(skip)]
    pub cached_footprint_present: bool,
    #[serde(skip)]
    last_seen_footprint_len: Option<usize>,
    #[serde(skip)]
    last_seen_range: Option<(i64, i64)>,
    #[serde(skip)]
    last_seen_enabled: Option<Vec<String>>,
}

impl OrderFlowScan {
    pub fn new(ts1: i64, ts2: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "order_flow_scan".to_string(),
                display_name: "Order Flow Scan".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            ts1,
            ts2,
            enabled_structures: default_all_structure_ids(),
            cached_window: None,
            cached_hits: OrderFlowScanHits::default(),
            cached_footprint_present: false,
            last_seen_footprint_len: None,
            last_seen_range: None,
            last_seen_enabled: None,
        }
    }

    fn enabled(&self, kind: OrderFlowStructure) -> bool {
        self.enabled_structures.iter().any(|id| id == kind.id())
    }

    /// This survey's own hits — every enabled structure read/resolved over
    /// [`Self::cached_window`]'s own footprint-bar range. `refresh_footprint_derived_cache`
    /// is the ONE place this primitive calls into either
    /// `orderflow_judge`'s flag readers or `mli::orderflow`, so a rendered
    /// hit and a cached hit-count can never drift apart.
    pub fn resolved_scan(&self, footprint: &[FootprintBar]) -> (Option<(usize, usize)>, OrderFlowScanHits) {
        let mut hits = OrderFlowScanHits::default();
        let Some((start, end)) = footprint_window_indices(footprint, self.ts1, self.ts2) else {
            return (None, hits);
        };
        if self.enabled(OrderFlowStructure::StackedImbalance) {
            hits.stacked_imbalance = (start..=end).filter_map(|i| resolve_imbalance_run(&footprint[i])).collect();
        }
        if self.enabled(OrderFlowStructure::Absorption) {
            hits.absorption = (start..=end).filter_map(|i| resolve_absorption(&footprint[i])).collect();
        }
        if self.enabled(OrderFlowStructure::SinglePrint) || self.enabled(OrderFlowStructure::UnfinishedBusiness) {
            for i in start..=end {
                let (sp, uf) = resolve_level_flags(&footprint[i]);
                if self.enabled(OrderFlowStructure::SinglePrint) {
                    hits.single_print.extend(sp);
                }
                if self.enabled(OrderFlowStructure::UnfinishedBusiness) {
                    hits.unfinished_business.extend(uf);
                }
            }
        }
        if self.enabled(OrderFlowStructure::DeltaDivergence) {
            let cfg = orderflow_engine_config();
            let (window_bars, window_series, offset) = footprint_judge_window(footprint, end, cfg.judge_lookback_bars);
            let dd_cfg = cfg.to_delta_divergence_config();
            hits.delta_divergence = mli::orderflow::delta_divergences(&window_bars, &window_series, &dd_cfg)
                .into_iter()
                .filter_map(|h| {
                    let gi_to = h.to_index + offset;
                    let gi_from = h.from_index + offset;
                    if gi_to < start || gi_to > end {
                        return None;
                    }
                    let from_ts = footprint.get(gi_from)?.open_time;
                    let to_ts = footprint.get(gi_to)?.open_time;
                    Some(ScanDivergenceHit { from_ts, to_ts, price_from: h.price_from, price_to: h.price_to, kind: h.kind })
                })
                .collect();
        }
        (Some((start, end)), hits)
    }

    /// The `order_flow_scan_structures` table's own rows — one per
    /// structure, in [`OrderFlowStructure::ALL`]'s own order (the SAME
    /// order [`OrderFlowScan::apply_style_property`] indexes back into via
    /// `row`). Hit counts read [`Self::cached_hits`] — never a second
    /// `resolved_scan` call from a method with no footprint of its own.
    fn structure_table_rows(&self) -> Vec<Vec<TableCell>> {
        let lang = current_language();
        OrderFlowStructure::ALL
            .iter()
            .map(|&kind| {
                vec![
                    TableCell::EditableCheck(self.enabled(kind)),
                    TableCell::Text(kind.label(lang).to_string()),
                    TableCell::Text(self.cached_hits.count_for(kind).to_string()),
                ]
            })
            .collect()
    }
}

impl Primitive for OrderFlowScan {
    fn type_id(&self) -> &'static str { "order_flow_scan" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Pattern }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, 0.0), (self.ts2, 0.0)] }
    fn set_points(&mut self, pts: &[(i64, f64)]) {
        if let Some(&(t, _)) = pts.first() { self.ts1 = t; }
        if let Some(&(t, _)) = pts.get(1) { self.ts2 = t; }
    }
    fn translate(&mut self, td: i64, _pd: f64) { self.ts1 += td; self.ts2 += td; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, _price: f64) {
        match pt {
            ControlPointType::Point1 => self.ts1 = ts_ms,
            ControlPointType::Point2 => self.ts2 = ts_ms,
            ControlPointType::Move => { let td = ts_ms - self.ts1; self.translate(td, 0.0); }
            _ => {}
        }
    }

    fn hit_test(&self, sx: f64, _sy: f64, bars: &[Bar], vp: &Viewport, _ps: &PriceScale) -> HitTestResult {
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2));
        if (sx - x1).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).abs() < HIT_TOLERANCE { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        if sx >= min_x - HIT_TOLERANCE && sx <= max_x + HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let x1 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts1));
        let x2 = vp.bar_to_x_f64(timestamp_ms_to_bar_f64(bars, self.ts2));
        let cy = vp.price_to_y((ps.price_min + ps.price_max) / 2.0, ps.price_min, ps.price_max);
        vec![ControlPoint::point1(x1, cy), ControlPoint::point2(x2, cy)]
    }

    /// The two dashed boundaries (screen X resolved fresh from `ctx.bars()`
    /// — cheap, and correct even before any sweep), then every enabled
    /// structure's own CACHED hits. Consecutive same-kind hits closer than
    /// [`MIN_MARKER_GAP_PX`] collapse — the same proximity degrade
    /// `SmcScan::render` uses, matched here rather than inventing a second
    /// behaviour.
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let (min_x, max_x) = (x1.min(x2), x1.max(x2));
        let chart_height = ctx.chart_height();

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        ctx.set_line_dash(&[4.0 * dpr, 4.0 * dpr]);
        for x in [min_x, max_x] {
            ctx.begin_path();
            ctx.move_to(crisp(x, dpr), 0.0);
            ctx.line_to(crisp(x, dpr), chart_height);
            ctx.stroke();
        }
        ctx.set_line_dash(&[]);

        let hits = &self.cached_hits;

        for_each_cluster(ctx, &hits.stacked_imbalance, |r| r.bar_ts, MIN_MARKER_GAP_PX, |ctx, run, count| {
            // Side is not read for colour — both sides of a stacked run
            // draw in the same structure colour; direction is what the
            // claim's own Stat block (`ImbalanceZone::result_stat_rows`)
            // reports, not a second colour code here.
            let _ = run.side;
            let color = OrderFlowStructure::StackedImbalance.color();
            let cx = ctx.ts_to_x_ms(run.bar_ts);
            let sx1 = cx - 3.0 * dpr;
            let sx2 = cx + 3.0 * dpr;
            let y_top = ctx.price_to_y(run.high);
            let y_bot = ctx.price_to_y(run.low);
            let (min_y, max_y) = (y_top.min(y_bot), y_top.max(y_bot));
            let fill = format!("{color}29");
            ctx.set_fill_color(&fill);
            ctx.fill_rect(sx1, min_y, sx2 - sx1, max_y - min_y);
            ctx.set_stroke_color(color);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rect(sx1, min_y, sx2 - sx1, max_y - min_y);
            draw_count_badge(ctx, sx2, min_y, count, color);
        });

        for_each_cluster(ctx, &hits.absorption, |a| a.bar_ts, MIN_MARKER_GAP_PX, |ctx, snap, count| {
            let sx = ctx.ts_to_x_ms(snap.bar_ts);
            let sy = ctx.price_to_y(snap.mid_price);
            let color = OrderFlowStructure::Absorption.color();
            ctx.set_fill_color(color);
            ctx.begin_path();
            ctx.arc(sx, sy, 3.0 * dpr, 0.0, std::f64::consts::TAU);
            ctx.fill();
            draw_count_badge(ctx, sx, sy, count, color);
        });

        for_each_cluster(ctx, &hits.single_print, |l| l.bar_ts, MIN_MARKER_GAP_PX, |ctx, lvl, count| {
            let sx = ctx.ts_to_x_ms(lvl.bar_ts);
            let sy = ctx.price_to_y(lvl.price);
            let color = OrderFlowStructure::SinglePrint.color();
            ctx.set_stroke_color(color);
            ctx.set_stroke_width(1.5);
            ctx.set_line_dash(&[2.0 * dpr, 2.0 * dpr]);
            ctx.begin_path();
            ctx.move_to(sx - 3.0 * dpr, sy);
            ctx.line_to(sx + 3.0 * dpr, sy);
            ctx.stroke();
            ctx.set_line_dash(&[]);
            draw_count_badge(ctx, sx, sy, count, color);
        });

        for_each_cluster(ctx, &hits.unfinished_business, |l| l.bar_ts, MIN_MARKER_GAP_PX, |ctx, lvl, count| {
            let sx = ctx.ts_to_x_ms(lvl.bar_ts);
            let sy = ctx.price_to_y(lvl.price);
            let color = OrderFlowStructure::UnfinishedBusiness.color();
            ctx.set_fill_color(color);
            ctx.begin_path();
            ctx.arc(sx, sy, 2.0 * dpr, 0.0, std::f64::consts::TAU);
            ctx.fill();
            draw_count_badge(ctx, sx, sy, count, color);
        });

        for_each_cluster(ctx, &hits.delta_divergence, |d| d.to_ts, MIN_MARKER_GAP_PX, |ctx, hit, count| {
            let color = OrderFlowStructure::DeltaDivergence.color();
            let fx = ctx.ts_to_x_ms(hit.from_ts);
            let fy = ctx.price_to_y(hit.price_from);
            let tx = ctx.ts_to_x_ms(hit.to_ts);
            let ty = ctx.price_to_y(hit.price_to);
            ctx.set_stroke_color(color);
            ctx.set_stroke_width(1.5);
            ctx.set_line_dash(&[]);
            ctx.begin_path();
            ctx.move_to(fx, fy);
            ctx.line_to(tx, ty);
            ctx.stroke();
            draw_count_badge(ctx, tx, ty, count, color);
        });

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
                render_primitive_text(ctx, text, text_x, text_y, &self.data.color.stroke);
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    /// Recompute [`Self::cached_window`]/[`Self::cached_hits`] from
    /// `footprint` through [`Self::resolved_scan`] — see
    /// [`Primitive::refresh_footprint_derived_cache`]'s own contract.
    /// Short-circuits when neither the footprint length, this scan's own
    /// `(ts1, ts2)` range, NOR its own `enabled_structures` has changed
    /// since the last call.
    fn refresh_footprint_derived_cache(&mut self, footprint: &[FootprintBar]) {
        if self.cached_footprint_present
            && self.last_seen_footprint_len == Some(footprint.len())
            && self.last_seen_range == Some((self.ts1, self.ts2))
            && self.last_seen_enabled.as_deref() == Some(self.enabled_structures.as_slice())
        {
            return;
        }
        let (window, hits) = self.resolved_scan(footprint);
        self.cached_window = window;
        self.cached_hits = hits;
        self.cached_footprint_present = true;
        self.last_seen_footprint_len = Some(footprint.len());
        self.last_seen_range = Some((self.ts1, self.ts2));
        self.last_seen_enabled = Some(self.enabled_structures.clone());
    }

    /// This survey's own ONE rule — see the module's own doc. No other rule
    /// exists here: a survey reports what the bars contain, never a claim.
    /// Gated on `OrderFlowEngineConfig::judge_claims` first, same as every
    /// claim in this family — see
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
    fn context_violations(&self) -> &[Violation] {
        if !self.cached_footprint_present {
            &super::orderflow_judge::MISSING_VIOLATION
        } else {
            &[]
        }
    }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        let lang = current_language();
        let head = vec![
            SettingsKey::Show.get(lang).to_string(),
            ConceptKey::Name.get(lang).to_string(),
            ConceptKey::ScanHitsColumnHeader.get(lang).to_string(),
        ];
        let bars_surveyed = self.cached_window.map(|(s, e)| e - s + 1).unwrap_or(0);
        vec![
            ConfigProperty::table(
                ORDER_FLOW_SCAN_TABLE_ID,
                ConceptKey::OrderFlowScanStructuresLabel.get(lang),
                head,
                self.structure_table_rows(),
            ).with_order(10),
            ConfigProperty::stat_block(
                ORDER_FLOW_SCAN_STATS_ID,
                vec![
                    StatRow::new(ConceptKey::ScanTotalHitsLabel.get(lang), &self.cached_hits.total().to_string(), None),
                    StatRow::new(ConceptKey::BarCount.get(lang), &bars_surveyed.to_string(), None),
                ],
            ).with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        if let Some((base, row, col)) = parse_table_cell_id(id) {
            if base != ORDER_FLOW_SCAN_TABLE_ID || col != 0 {
                return false;
            }
            let Some(enabled) = value.as_bool() else { return false };
            let Some(kind) = OrderFlowStructure::ALL.get(row).copied() else { return false };
            let kind_id = kind.id().to_string();
            if enabled {
                if !self.enabled_structures.iter().any(|k| k == &kind_id) {
                    self.enabled_structures.push(kind_id);
                }
            } else {
                self.enabled_structures.retain(|k| k != &kind_id);
            }
            return true;
        }
        if id == ORDER_FLOW_SCAN_STATS_ID {
            return false;
        }
        false
    }
}

/// Draw a small `"×{count}"` badge above a marker — see
/// `smc::smc_scan`'s own `draw_count_badge` for the identical shape,
/// matched here rather than inventing a second behaviour. No-op for
/// `count <= 1`.
fn draw_count_badge(ctx: &mut dyn RenderContext, x: f64, y: f64, count: usize, color: &str) {
    if count <= 1 {
        return;
    }
    let dpr = ctx.dpr();
    ctx.set_font(&format!("{}px sans-serif", (9.0 * dpr) as i32));
    ctx.set_text_align(crate::render::TextAlign::Center);
    ctx.set_text_baseline(crate::render::TextBaseline::Bottom);
    ctx.set_fill_color(color);
    ctx.fill_text(&format!("×{count}"), x, y - 4.0 * dpr);
}

/// Walk `items` (already sorted ascending by `ts`, as every resolver this
/// survey calls is documented to return), collapsing a run of consecutive
/// items whose own `ts`-derived screen X sits within `gap_px` device pixels
/// of the run's own first member. `draw` is called once per run, on the
/// run's own FIRST item, with the run's own length as `count` — see
/// `smc::smc_scan::for_each_cluster`'s own doc for the identical shape
/// (keyed on bar index there; keyed on timestamp here, since a footprint
/// hit carries no OHLC bar index of its own).
fn for_each_cluster<T>(
    ctx: &mut dyn RenderContext,
    items: &[T],
    ts_of: impl Fn(&T) -> i64,
    gap_px: f64,
    mut draw: impl FnMut(&mut dyn RenderContext, &T, usize),
) {
    let dpr = ctx.dpr();
    let mut i = 0;
    while i < items.len() {
        let start_x = ctx.ts_to_x_ms(ts_of(&items[i]));
        let mut j = i + 1;
        while j < items.len() {
            let x = ctx.ts_to_x_ms(ts_of(&items[j]));
            if (x - start_x).abs() >= gap_px * dpr {
                break;
            }
            j += 1;
        }
        draw(ctx, &items[i], j - i);
        i = j;
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "order_flow_scan",
        display_name: "Order Flow Scan",
        kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Survey a bar range for every enabled order-flow structure",
        icon: "order_flow_scan",
        default_color: "#5C6BC0",
        factory: |points, color| {
            let (t1, _) = points.first().copied().unwrap_or((0, 0.0));
            let (t2, _) = points.get(1).copied().unwrap_or((t1 + 72_000_000, 0.0));
            Box::new(OrderFlowScan::new(t1, t2, color))
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

    #[test]
    fn no_footprint_data_reports_exactly_order_flow_data_missing() {
        let _guard = reset_engine_config();
        let scan = OrderFlowScan::new(0, 5_000, "#5C6BC0");
        assert!(!scan.cached_footprint_present);
        let violations = scan.context_violations();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "order_flow_data_missing");
    }

    #[test]
    fn unchecking_a_structure_removes_its_hits() {
        let _guard = reset_engine_config();
        let footprint = vec![footprint_with_run(0), footprint_with_run(1_000), footprint_with_run(2_000)];
        let mut scan = OrderFlowScan::new(0, 2_000, "#5C6BC0");
        scan.refresh_footprint_derived_cache(&footprint);
        assert!(!scan.cached_hits.stacked_imbalance.is_empty());

        scan.enabled_structures.retain(|id| id != "stacked_imbalance");
        scan.refresh_footprint_derived_cache(&footprint);
        assert!(scan.cached_hits.stacked_imbalance.is_empty());
    }

    #[test]
    fn render_cache_is_absent_from_to_json() {
        let _guard = reset_engine_config();
        let footprint = vec![footprint_with_run(0)];
        let mut scan = OrderFlowScan::new(0, 1_000, "#5C6BC0");
        scan.refresh_footprint_derived_cache(&footprint);
        let json = scan.to_json();
        assert!(!json.contains("cached_window"), "{json}");
        assert!(!json.contains("cached_hits"), "{json}");
        assert!(!json.contains("cached_footprint_present"), "{json}");

        let mut registry = super::super::super::registry::PrimitiveRegistry::new();
        registry.register_typed::<OrderFlowScan>(metadata());
        let restored = registry.from_json("order_flow_scan", &json).expect("round trip");
        assert_eq!(restored.points(), scan.points());
    }

    #[test]
    fn table_cell_click_toggles_enabled_structures() {
        let _guard = reset_engine_config();
        let mut scan = OrderFlowScan::new(0, 2_000, "#5C6BC0");
        let row = OrderFlowStructure::ALL.iter().position(|k| k.id() == "stacked_imbalance").expect("catalogue has it");
        let cell_id = format!("{ORDER_FLOW_SCAN_TABLE_ID}::cell:{row}:0");

        assert!(scan.apply_style_property(&cell_id, &PropertyValue::Boolean(false)));
        assert!(!scan.enabled_structures.iter().any(|k| k == "stacked_imbalance"));

        assert!(scan.apply_style_property(&cell_id, &PropertyValue::Boolean(true)));
        assert!(scan.enabled_structures.iter().any(|k| k == "stacked_imbalance"));
    }

    #[test]
    fn the_sweep_short_circuits_on_an_unchanged_slice() {
        let _guard = reset_engine_config();
        let footprint = vec![footprint_with_run(0), footprint_with_run(1_000)];
        let mut scan = OrderFlowScan::new(0, 1_000, "#5C6BC0");
        scan.refresh_footprint_derived_cache(&footprint);
        let first_len = scan.cached_hits.total();
        scan.refresh_footprint_derived_cache(&footprint);
        assert_eq!(scan.cached_hits.total(), first_len);
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
        let claim = OrderFlowScan::new(1_000, 2_000, "#EF5350");
        let violations = claim.context_violations();
        assert_eq!(violations.len(), 1, "the missing-data rule survives judging being off: {violations:?}");
        assert_eq!(violations[0].rule_id, "order_flow_data_missing");
    }
}
