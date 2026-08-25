//! SMC Scan — the SURVEY half of the smart-money markup engine
//! (`docs/mlc/plans/markup-engines-2026-08-25.md`, "the scan is a SURVEY:
//! the bars say what they contain"). Sibling of
//! `patterns::candle_pattern_scan::CandlePatternScan` — same shape, same
//! contract, deliberately not diverged from: two points bracket a bar
//! RANGE, `refresh_bar_derived_cache` resolves it and stores every enabled
//! detector's own hits as a RENDER CACHE (`#[serde(skip)]`, rebuilt from
//! bars, never persisted — see [`Primitive::refresh_bar_derived_cache`]'s
//! own contract), and a table + Stat block report it.
//!
//! Seven structures are surveyable, over the SAME bounded judgement window
//! (`crate::state::smc_engine::SmcEngineConfig::judge_lookback_bars`,
//! resolved via [`super::smc_judge::judge_window`], anchored at the survey
//! range's own RIGHT edge) every hand-marked SMC claim reads: `order_block`,
//! `fair_value_gap`, `liquidity_sweep`, `structure_break` (the four
//! committed claims' own vocabulary), plus `equal_levels`,
//! `displacement_leg`, `amd_cycle` (no standalone claim primitive of their
//! own — a survey is the only place this chart names them at all, besides
//! `amd_cycle`'s own claim above). [`SurveyStructure`] is the ONE place the
//! seven wire ids map to a detector call and a display label — declared
//! once, per the plan's own rule that a persisted wire id is never
//! scattered across string literals.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
use crate::i18n::{current_language, ConceptKey, Language, PrimitiveNameKey, SettingsKey};
use crate::state::smc_engine::smc_engine_config;
use mli::smc as mli_smc;
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor, HIT_TOLERANCE,
    RenderContext, crisp, render_primitive_text, TextAlign,
    config::{ConfigProperty, PropertyValue, StatRow, TableCell, parse_table_cell_id},
};
use super::smc_judge::judge_window;
use super::render_shapes::{draw_zone, draw_level, draw_leg};
use super::amd_cycle::{offset_sweep, offset_leg, leg_endpoints};

/// The scanner family's shared collapse spacing — see
/// [`super::super::SCAN_MARKER_MIN_GAP_PX`]. Read, not re-declared: two
/// scanners are the same gesture, and a chart where one collapses its hits
/// at a different spacing than the other is a chart that answers the same
/// question two ways.
use super::super::SCAN_MARKER_MIN_GAP_PX as MIN_MARKER_GAP_PX;

const SMC_SCAN_TABLE_ID: &str = "smc_scan_structures";
const SMC_SCAN_STATS_ID: &str = "smc_scan_stats";

/// The seven structures `SmcScan` can survey — stable snake_case WIRE ids,
/// persisted on [`SmcScan::enabled_structures`]. The ONE place id, display
/// label, and detector colour map for every one of the seven, so no other
/// module scatters a `"order_block"`/`"equal_levels"`/... string literal of
/// its own for this table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurveyStructure {
    OrderBlock,
    FairValueGap,
    LiquiditySweep,
    StructureBreak,
    EqualLevels,
    DisplacementLeg,
    AmdCycle,
}

impl SurveyStructure {
    pub const ALL: [Self; 7] = [
        Self::OrderBlock,
        Self::FairValueGap,
        Self::LiquiditySweep,
        Self::StructureBreak,
        Self::EqualLevels,
        Self::DisplacementLeg,
        Self::AmdCycle,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::OrderBlock => "order_block",
            Self::FairValueGap => "fair_value_gap",
            Self::LiquiditySweep => "liquidity_sweep",
            Self::StructureBreak => "structure_break",
            Self::EqualLevels => "equal_levels",
            Self::DisplacementLeg => "displacement_leg",
            Self::AmdCycle => "amd_cycle",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }

    /// The claim primitives among the seven reuse their own
    /// `PrimitiveNameKey` row rather than a second, independently-typed
    /// label; `equal_levels`/`displacement_leg` carry no claim primitive of
    /// their own, so they read the two [`ConceptKey`] rows this survey
    /// declares for them.
    pub fn label(self, lang: Language) -> &'static str {
        match self {
            Self::OrderBlock => PrimitiveNameKey::OrderBlock.get(lang),
            Self::FairValueGap => PrimitiveNameKey::FairValueGap.get(lang),
            Self::LiquiditySweep => PrimitiveNameKey::LiquiditySweep.get(lang),
            Self::StructureBreak => PrimitiveNameKey::StructureBreak.get(lang),
            Self::EqualLevels => ConceptKey::EqualLevelsLabel.get(lang),
            Self::DisplacementLeg => ConceptKey::DisplacementLegLabel.get(lang),
            Self::AmdCycle => PrimitiveNameKey::AmdCycle.get(lang),
        }
    }

    /// The colour this structure's own hits draw in — the SAME
    /// `default_color` its own claim primitive's `metadata()` uses where one
    /// exists, so a survey hit and a hand-placed claim of the same kind
    /// read as the same instrument.
    fn color(self) -> &'static str {
        match self {
            Self::OrderBlock => "#7E57C2",
            Self::FairValueGap => "#26C6DA",
            Self::LiquiditySweep => "#EC407A",
            Self::StructureBreak => "#FFA726",
            Self::EqualLevels => "#8D6E63",
            Self::DisplacementLeg => "#42A5F5",
            Self::AmdCycle => "#FFCA28",
        }
    }
}

fn default_all_structure_ids() -> Vec<String> {
    SurveyStructure::ALL.iter().map(|s| s.id().to_string()).collect()
}

/// This survey's own resolved hits, one field per structure — a RENDER
/// CACHE, never persisted (see the module's own doc).
#[derive(Clone, Debug, Default)]
pub struct ScanHits {
    pub order_blocks: Vec<mli_smc::OrderBlock>,
    pub fair_value_gaps: Vec<mli_smc::FvgZone>,
    pub liquidity_sweeps: Vec<mli_smc::LiquiditySweep>,
    pub structure_breaks: Vec<mli_smc::StructureEvent>,
    pub equal_levels: Vec<mli_smc::EqualLevels>,
    pub displacement_legs: Vec<mli_smc::DisplacementLeg>,
    pub amd_cycles: Vec<mli_smc::AmdCycle>,
}

impl ScanHits {
    pub fn total(&self) -> usize {
        self.order_blocks.len()
            + self.fair_value_gaps.len()
            + self.liquidity_sweeps.len()
            + self.structure_breaks.len()
            + self.equal_levels.len()
            + self.displacement_legs.len()
            + self.amd_cycles.len()
    }

    fn count_for(&self, kind: SurveyStructure) -> usize {
        match kind {
            SurveyStructure::OrderBlock => self.order_blocks.len(),
            SurveyStructure::FairValueGap => self.fair_value_gaps.len(),
            SurveyStructure::LiquiditySweep => self.liquidity_sweeps.len(),
            SurveyStructure::StructureBreak => self.structure_breaks.len(),
            SurveyStructure::EqualLevels => self.equal_levels.len(),
            SurveyStructure::DisplacementLeg => self.displacement_legs.len(),
            SurveyStructure::AmdCycle => self.amd_cycles.len(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SmcScan {
    pub data: PrimitiveData,
    #[serde(default)] pub ts1: i64,
    #[serde(default)] pub ts2: i64,
    /// [`SurveyStructure::id`] wire ids — serialized as ids, never as an
    /// enum/index, so a saved scan survives a catalogue that grows (same
    /// convention `CandlePatternScan::enabled_kinds` uses). Default: all
    /// seven.
    #[serde(default = "default_all_structure_ids")]
    pub enabled_structures: Vec<String>,

    /// This scan's own resolved `[start, end]` bar-index window — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    #[serde(skip)]
    pub cached_window: Option<(usize, usize)>,
    /// This scan's own resolved hits, for the enabled structures — see the
    /// module's own doc and [`Primitive::refresh_bar_derived_cache`]'s own
    /// contract. `render` recomputes this fresh via
    /// [`SmcScan::resolved_scan`] and never reads this field; it exists only
    /// for [`SmcScan::style_properties`]'s own per-structure hit-count
    /// column and total-hits Stat row.
    #[serde(skip)]
    pub cached_hits: ScanHits,
}

impl SmcScan {
    pub fn new(ts1: i64, ts2: i64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "smc_scan".to_string(),
                display_name: "Smart Money Scan".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            ts1,
            ts2,
            enabled_structures: default_all_structure_ids(),
            cached_window: None,
            cached_hits: ScanHits::default(),
        }
    }

    fn resolved_window(&self, bars: &[Bar]) -> Option<(usize, usize)> {
        super::super::bar_window::window_indices(bars, self.ts1, self.ts2)
    }

    fn enabled(&self, kind: SurveyStructure) -> bool {
        self.enabled_structures.iter().any(|id| id == kind.id())
    }

    /// This survey's own hits — every enabled detector run over the SAME
    /// bounded judgement window ([`judge_window`], anchored at the survey
    /// range's own right edge, exactly like every hand-marked SMC claim in
    /// this crate resolves its own single anchor), filtered to the hit
    /// whose own defining bar index falls inside [`Self::resolved_window`].
    /// Every returned bar index is already GLOBAL. `render` and
    /// [`Primitive::refresh_bar_derived_cache`] both call this directly —
    /// the ONE place this primitive calls into `mli::smc`, so a rendered
    /// hit and a cached hit-count can never drift apart.
    pub fn resolved_scan(&self, bars: &[Bar]) -> ScanHits {
        let mut hits = ScanHits::default();
        let Some((start, end)) = self.resolved_window(bars) else { return hits };
        let engine_cfg = smc_engine_config();
        let (window, offset) = judge_window(bars, end, engine_cfg.judge_lookback_bars);
        let cfg = engine_cfg.to_smc_config();
        let in_range = |global_idx: usize| global_idx >= start && global_idx <= end;

        if self.enabled(SurveyStructure::OrderBlock) {
            hits.order_blocks = mli_smc::order_blocks(&window, &cfg)
                .into_iter()
                .filter(|b| in_range(b.index + offset))
                .map(|b| mli_smc::OrderBlock {
                    index: b.index + offset,
                    low: b.low,
                    high: b.high,
                    direction: b.direction,
                    broke_structure_at: b.broke_structure_at + offset,
                    breaker_at: b.breaker_at.map(|i| i + offset),
                })
                .collect();
        }
        if self.enabled(SurveyStructure::FairValueGap) {
            hits.fair_value_gaps = mli_smc::fair_value_gaps(&window, &cfg)
                .into_iter()
                .filter(|g| in_range(g.start_index + 1 + offset)) // middle bar, matching `FairValueGap`'s own anchor
                .map(|g| mli_smc::FvgZone {
                    start_index: g.start_index + offset,
                    end_index: g.end_index + offset,
                    low: g.low,
                    high: g.high,
                    direction: g.direction,
                    mitigated_at: g.mitigated_at.map(|i| i + offset),
                    filled_at: g.filled_at.map(|i| i + offset),
                })
                .collect();
        }
        if self.enabled(SurveyStructure::LiquiditySweep) {
            hits.liquidity_sweeps = mli_smc::liquidity_sweeps(&window, &cfg)
                .into_iter()
                .filter(|s| in_range(s.index + offset))
                .map(|s| offset_sweep(&s, offset))
                .collect();
        }
        if self.enabled(SurveyStructure::StructureBreak) {
            hits.structure_breaks = mli_smc::structure(&window, &cfg)
                .into_iter()
                .filter(|e| in_range(e.index + offset))
                .map(|e| mli_smc::StructureEvent {
                    index: e.index + offset,
                    kind: e.kind,
                    direction: e.direction,
                    broken_swing_index: e.broken_swing_index + offset,
                    level: e.level,
                })
                .collect();
        }
        if self.enabled(SurveyStructure::EqualLevels) {
            hits.equal_levels = mli_smc::equal_levels(&window, &cfg)
                .into_iter()
                .filter(|e| e.indices.iter().copied().min().is_some_and(|i| in_range(i + offset)))
                .map(|e| mli_smc::EqualLevels {
                    indices: e.indices.iter().map(|&i| i + offset).collect(),
                    level: e.level,
                    kind: e.kind,
                })
                .collect();
        }
        if self.enabled(SurveyStructure::DisplacementLeg) {
            hits.displacement_legs = mli_smc::displacement_legs(&window, &cfg)
                .into_iter()
                .filter(|l| in_range(l.start_index + offset))
                .map(|l| offset_leg(&l, offset))
                .collect();
        }
        if self.enabled(SurveyStructure::AmdCycle) {
            hits.amd_cycles = mli_smc::amd_cycles(&window, &cfg)
                .into_iter()
                .filter(|c| in_range(c.manipulation.index + offset))
                .map(|c| mli_smc::AmdCycle {
                    accumulation: (c.accumulation.start + offset)..(c.accumulation.end + offset),
                    manipulation: offset_sweep(&c.manipulation, offset),
                    distribution: offset_leg(&c.distribution, offset),
                })
                .collect();
        }
        hits
    }

    /// The `smc_scan_structures` table's own rows — one per structure, in
    /// [`SurveyStructure::ALL`]'s own order (the SAME order
    /// [`SmcScan::apply_style_property`] indexes back into via `row`). Hit
    /// counts read [`Self::cached_hits`] — never a second `resolved_scan`
    /// call from a method with no `bars` of its own.
    fn structure_table_rows(&self) -> Vec<Vec<TableCell>> {
        let lang = current_language();
        SurveyStructure::ALL
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

impl Primitive for SmcScan {
    fn type_id(&self) -> &'static str { "smc_scan" }
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

    /// The two dashed boundaries, then every enabled structure's own hits —
    /// each drawn in the SAME shape its own hand-marked claim uses (a zone
    /// for order blocks/gaps, a level for sweeps/breaks, a diagonal for
    /// displacement legs, all three combined for AMD cycles) via
    /// [`super::render_shapes`]. Consecutive same-kind hits closer than
    /// [`MIN_MARKER_GAP_PX`] collapse: only the first of the run draws its
    /// full geometry, carrying a small count badge — the same proximity
    /// degrade [`patterns::candle_pattern_scan::CandlePatternScan::render`]
    /// chose, read there and matched here rather than inventing a second
    /// behaviour. Computed FRESH from `bars` here via
    /// [`Self::resolved_scan`] — never reads [`Self::cached_hits`] —
    /// matching [`Primitive::refresh_bar_derived_cache`]'s own contract.
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

        let hits = self.resolved_scan(ctx.bars());
        let window_end = self.resolved_window(ctx.bars()).map(|(_, e)| e);

        // Order blocks / fair value gaps: zones, extended to the survey's
        // own right edge (mirroring `extend_right == true`, the default
        // every zone-shaped claim in this family ships with).
        for_each_cluster(ctx, &hits.order_blocks, |b| b.index, MIN_MARKER_GAP_PX, |ctx, b, count| {
            let right = window_end.unwrap_or(b.index).max(b.index);
            let sx1 = ctx.bar_to_x(b.index as f64);
            let sx2 = ctx.bar_to_x(right as f64);
            let sy1 = ctx.price_to_y(b.high);
            let sy2 = ctx.price_to_y(b.low);
            draw_zone(ctx, sx1, sx2, sy1, sy2, SurveyStructure::OrderBlock.color(), 0.16, false);
            draw_count_badge(ctx, sx1, sy1, count, SurveyStructure::OrderBlock.color());
        });
        for_each_cluster(ctx, &hits.fair_value_gaps, |g| g.start_index + 1, MIN_MARKER_GAP_PX, |ctx, g, count| {
            let right = window_end.unwrap_or(g.end_index).max(g.end_index);
            let sx1 = ctx.bar_to_x(g.start_index as f64);
            let sx2 = ctx.bar_to_x(right as f64);
            let sy1 = ctx.price_to_y(g.high);
            let sy2 = ctx.price_to_y(g.low);
            draw_zone(ctx, sx1, sx2, sy1, sy2, SurveyStructure::FairValueGap.color(), 0.16, false);
            draw_count_badge(ctx, sx1, sy1, count, SurveyStructure::FairValueGap.color());
        });

        // Liquidity sweeps / structure breaks: level segments.
        for_each_cluster(ctx, &hits.liquidity_sweeps, |s| s.index, MIN_MARKER_GAP_PX, |ctx, s, count| {
            let sx1 = ctx.bar_to_x(s.swept_swing_index as f64);
            let sx2 = ctx.bar_to_x(s.index as f64);
            let sy = ctx.price_to_y(s.level);
            draw_level(ctx, sx1, sx2, sy, SurveyStructure::LiquiditySweep.color(), true);
            draw_count_badge(ctx, sx2, sy, count, SurveyStructure::LiquiditySweep.color());
        });
        for_each_cluster(ctx, &hits.structure_breaks, |e| e.index, MIN_MARKER_GAP_PX, |ctx, e, count| {
            let sx1 = ctx.bar_to_x(e.broken_swing_index as f64);
            let sx2 = ctx.bar_to_x(e.index as f64);
            let sy = ctx.price_to_y(e.level);
            draw_level(ctx, sx1, sx2, sy, SurveyStructure::StructureBreak.color(), false);
            draw_count_badge(ctx, sx2, sy, count, SurveyStructure::StructureBreak.color());
        });

        // Equal levels: a dashed level spanning the cluster's own first and
        // last swing.
        for_each_cluster(
            ctx,
            &hits.equal_levels,
            |e| e.indices.iter().copied().min().unwrap_or(0),
            MIN_MARKER_GAP_PX,
            |ctx, e, count| {
                let lo = e.indices.iter().copied().min().unwrap_or(0);
                let hi = e.indices.iter().copied().max().unwrap_or(lo);
                let sx1 = ctx.bar_to_x(lo as f64);
                let sx2 = ctx.bar_to_x(hi as f64);
                let sy = ctx.price_to_y(e.level);
                draw_level(ctx, sx1, sx2, sy, SurveyStructure::EqualLevels.color(), true);
                draw_count_badge(ctx, sx2, sy, count, SurveyStructure::EqualLevels.color());
            },
        );

        // Displacement legs: a diagonal.
        for_each_cluster(ctx, &hits.displacement_legs, |l| l.start_index, MIN_MARKER_GAP_PX, |ctx, l, count| {
            if let Some((bx1, py1, bx2, py2)) = leg_endpoints(ctx, l) {
                let sx1 = ctx.bar_to_x(bx1);
                let sy1 = ctx.price_to_y(py1);
                let sx2 = ctx.bar_to_x(bx2);
                let sy2 = ctx.price_to_y(py2);
                draw_leg(ctx, sx1, sy1, sx2, sy2, SurveyStructure::DisplacementLeg.color());
                draw_count_badge(ctx, sx2, sy2, count, SurveyStructure::DisplacementLeg.color());
            }
        });

        // AMD cycles: accumulation zone + manipulation level + distribution
        // leg, the same three-shape combination `AmdCycle::render` draws.
        for_each_cluster(ctx, &hits.amd_cycles, |c| c.manipulation.index, MIN_MARKER_GAP_PX, |ctx, c, count| {
            let color = SurveyStructure::AmdCycle.color();
            let acc_bars = ctx.bars();
            let acc_bounds = acc_bars
                .get(c.accumulation.start..=c.accumulation.end.min(acc_bars.len().saturating_sub(1)))
                .filter(|s| !s.is_empty())
                .map(|s| (s.iter().fold(f64::INFINITY, |m, b| m.min(b.low)), s.iter().fold(f64::NEG_INFINITY, |m, b| m.max(b.high))));
            if let Some((lo, hi)) = acc_bounds {
                let sx1 = ctx.bar_to_x(c.accumulation.start as f64);
                let sx2 = ctx.bar_to_x(c.accumulation.end as f64);
                let sy1 = ctx.price_to_y(hi);
                let sy2 = ctx.price_to_y(lo);
                draw_zone(ctx, sx1, sx2, sy1, sy2, color, 0.12, true);
            }
            let sx1 = ctx.bar_to_x(c.manipulation.swept_swing_index as f64);
            let sx2 = ctx.bar_to_x(c.manipulation.index as f64);
            let sy = ctx.price_to_y(c.manipulation.level);
            draw_level(ctx, sx1, sx2, sy, color, true);
            if let Some((lx1, ly1, lx2, ly2)) = leg_endpoints(ctx, &c.distribution) {
                let dx1 = ctx.bar_to_x(lx1);
                let dy1 = ctx.price_to_y(ly1);
                let dx2 = ctx.bar_to_x(lx2);
                let dy2 = ctx.price_to_y(ly2);
                draw_leg(ctx, dx1, dy1, dx2, dy2, color);
            }
            draw_count_badge(ctx, sx2, sy, count, color);
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

    /// Recompute [`Self::cached_window`]/[`Self::cached_hits`] from `bars`
    /// through the SAME [`Self::resolved_window`]/[`Self::resolved_scan`]
    /// calls `render` makes directly — see
    /// [`Primitive::refresh_bar_derived_cache`]'s own contract.
    fn refresh_bar_derived_cache(&mut self, bars: &[Bar]) {
        self.cached_window = self.resolved_window(bars);
        self.cached_hits = self.resolved_scan(bars);
    }

    // No `context_violations` override: a SURVEY reports what the bars
    // contain, never a claim, so it has no rule to fire — the same posture
    // `CandlePatternScan` already ships (no rule id, no `judge_claims`
    // read anywhere in that file). Inventing one here would be exactly the
    // "never invent a rule for a tool that makes no claim" defect Appendix
    // A calls out for `dealing_range`'s own Manual mode, generalized.

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
                SMC_SCAN_TABLE_ID,
                ConceptKey::SmcScanStructuresLabel.get(lang),
                head,
                self.structure_table_rows(),
            ).with_order(10),
            ConfigProperty::stat_block(
                SMC_SCAN_STATS_ID,
                vec![
                    StatRow::new(ConceptKey::ScanTotalHitsLabel.get(lang), &self.cached_hits.total().to_string(), None),
                    StatRow::new(ConceptKey::BarCount.get(lang), &bars_surveyed.to_string(), None),
                ],
            ).with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        if let Some((base, row, col)) = parse_table_cell_id(id) {
            if base != SMC_SCAN_TABLE_ID || col != 0 {
                return false;
            }
            let Some(enabled) = value.as_bool() else { return false };
            let Some(kind) = SurveyStructure::ALL.get(row).copied() else { return false };
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
        if id == SMC_SCAN_STATS_ID {
            return false;
        }
        false
    }
}

/// Draw a small `"×{count}"` badge above a marker — the count-badge half of
/// the proximity degrade, matching
/// `patterns::candle_pattern_scan::CandlePatternScan::render`'s own choice
/// to print a count rather than stack markers. No-op for `count <= 1`.
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

/// Walk `items` (already sorted ascending by `anchor`, as every `mli::smc`
/// detector list in this crate is documented to be), collapsing a run of
/// consecutive items whose own `anchor`-derived screen X sits within
/// `gap_px` device pixels of the run's own first member. `draw` is called
/// once per run, on the run's own FIRST item, with the run's own length as
/// `count` — the representative-plus-count-badge shape [`draw_count_badge`]
/// renders.
fn for_each_cluster<T>(
    ctx: &mut dyn RenderContext,
    items: &[T],
    anchor: impl Fn(&T) -> usize,
    gap_px: f64,
    mut draw: impl FnMut(&mut dyn RenderContext, &T, usize),
) {
    let dpr = ctx.dpr();
    let mut i = 0;
    while i < items.len() {
        let start_x = ctx.bar_to_x(anchor(&items[i]) as f64);
        let mut j = i + 1;
        while j < items.len() {
            let x = ctx.bar_to_x(anchor(&items[j]) as f64);
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
        type_id: "smc_scan",
        display_name: "Smart Money Scan",
        kind: PrimitiveKind::Pattern,
        click_behavior: ClickBehavior::TwoPoint,
        tooltip: "Survey a bar range for every enabled smart-money structure",
        icon: "smc_scan",
        default_color: "#5C6BC0",
        factory: |points, color| {
            let (t1, _) = points.first().copied().unwrap_or((0, 0.0));
            let (t2, _) = points.get(1).copied().unwrap_or((t1 + 72_000_000, 0.0));
            Box::new(SmcScan::new(t1, t2, color))
        },
        supports_text: true,
        has_levels: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reproduces `smc::order_block`'s own fixture (borrowed, per the
    /// task's own instruction to reuse `mli`'s test series rather than
    /// invent one that may not clear the thresholds): a confirmed uptrend,
    /// a down-close order-block candle at bar 9, an up displacement leg
    /// (10..12) that breaks structure, and a breaker close at bar 13.
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

    /// Hits equal a direct `mli` call over the same range: the SAME window
    /// this primitive's own `resolved_scan` builds (full bars, offset 0 for
    /// this fixture's own size), asserted by identity.
    #[test]
    fn order_block_hits_match_a_direct_mli_call_over_the_same_range() {
        let _guard = reset_engine_config();
        set_fixture_config();
        let bars = order_block_fixture_bars();
        let scan = SmcScan::new(0, 13_000, "#5C6BC0");
        let hits = scan.resolved_scan(&bars);

        let cfg = smc_engine_config().to_smc_config();
        let smc_bars: Vec<mli_smc::SmcBar> =
            bars.iter().map(|b| mli_smc::SmcBar { open: b.open, high: b.high, low: b.low, close: b.close }).collect();
        let direct = mli_smc::order_blocks(&smc_bars, &cfg);

        assert_eq!(hits.order_blocks, direct);
        assert!(hits.order_blocks.iter().any(|b| b.index == 9));
    }

    #[test]
    fn unchecking_a_structure_removes_its_hits() {
        let _guard = reset_engine_config();
        set_fixture_config();
        let bars = order_block_fixture_bars();
        let mut scan = SmcScan::new(0, 13_000, "#5C6BC0");
        scan.refresh_bar_derived_cache(&bars);
        assert!(!scan.cached_hits.order_blocks.is_empty());

        scan.enabled_structures.retain(|id| id != "order_block");
        scan.refresh_bar_derived_cache(&bars);
        assert!(scan.cached_hits.order_blocks.is_empty());
    }

    #[test]
    fn render_cache_is_absent_from_to_json() {
        let _guard = reset_engine_config();
        set_fixture_config();
        let bars = order_block_fixture_bars();
        let mut scan = SmcScan::new(0, 13_000, "#5C6BC0");
        scan.refresh_bar_derived_cache(&bars);
        let json = scan.to_json();
        assert!(!json.contains("cached_window"), "{json}");
        assert!(!json.contains("cached_hits"), "{json}");
        assert!(!json.contains("order_blocks"), "{json}");

        let mut registry = super::super::super::registry::PrimitiveRegistry::new();
        registry.register_typed::<SmcScan>(metadata());
        let restored = registry.from_json("smc_scan", &json).expect("round trip");
        assert_eq!(restored.points(), scan.points());
    }

    #[test]
    fn table_cell_click_toggles_enabled_structures() {
        let _guard = reset_engine_config();
        let mut scan = SmcScan::new(0, 13_000, "#5C6BC0");
        let row = SurveyStructure::ALL.iter().position(|k| k.id() == "order_block").expect("catalogue has it");
        let cell_id = format!("{SMC_SCAN_TABLE_ID}::cell:{row}:0");

        assert!(scan.apply_style_property(&cell_id, &PropertyValue::Boolean(false)));
        assert!(!scan.enabled_structures.iter().any(|k| k == "order_block"));

        assert!(scan.apply_style_property(&cell_id, &PropertyValue::Boolean(true)));
        assert!(scan.enabled_structures.iter().any(|k| k == "order_block"));
    }
}
