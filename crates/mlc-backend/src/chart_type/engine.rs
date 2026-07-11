//! Per-chart-type data-plane engine table (chart-type registry arc, Step 8).
//!
//! [`SubscribeOps`] composes [`DataBridge`] (subscribe/abort network calls)
//! with [`DataBubble`] (slot-clear bookkeeping) into one fn-pointer triple
//! per chart_type id. This is a pure landing: [`ops_by_id`] has no callers
//! yet — the 100+ existing call sites in `mlc-app` keep calling
//! `bridge.request_*` / `bridge.abort_*` / `bubble.clear_*` directly until
//! Steps 10 and 12 migrate them onto this table.
//!
//! Kept inside `mlc-backend` (not `mlc-backend-data-bridge`) because only
//! this crate is allowed to name both `DataBridge` and `DataBubble` — see
//! the crate-contract table in `mylittlechart/CLAUDE.md`.

use digdigdig3::{AccountType as Dig3AccountType, ExchangeId};
use mlc_backend_data_bridge::{DataBridge, TpoSource};
use mlc_core::state::Timeframe;

use crate::bubble::{BarsSource, DataBubble, FoldParams};

/// Type-erased subscribe parameters, one variant per parametric-subscribe
/// shape found in `bridge.request_*` signatures. Populated by callers
/// (`ChangeChartType` handler, `ignite_derived_windows`,
/// `UpdateChartTypeParam` handler) from `chart_type_param` /
/// `chart_type_interval` bubble fields.
#[derive(Clone, Copy, Debug)]
pub enum SubscribeParams {
    /// Kline / render-family: reload plain kline bars at the current TF.
    /// `bar_count` = user profile's bar_count target (currently passed at
    /// call site).
    Kline { bar_count: usize },
    /// range_bar / volume_bar / kagi: units × 1e8 (fixed-point scaled).
    ScaledUnits { scaled_u64: u64 },
    /// renko / pnf: units × 1e8 + reversal count.
    ScaledUnitsWithReversal { scaled_u64: u64, reversal: u8 },
    /// dollar_bar: whole-dollar threshold.
    Threshold { threshold_u64: u64 },
    /// tick_imbalance_bar / volume_imbalance_bar / run_bar: alpha ×100 (e.g.
    /// 20 → 0.20) + min ticks.
    MinTicks { alpha_x100: u16, min_ticks: u32 },
    /// tick_bar: trade count.
    Count { count_u32: u32 },
    /// three_line_break: lookback lines.
    LinesBack { lines_back: u8 },
    /// tpo: frequency (minutes per letter).
    Freq { freq_u16: u16 },
    /// footprint: bucket interval string (`"1m"`/`"3m"`/... — one of the six
    /// values).
    Interval { interval: &'static str },
}

/// Data-plane engine ops for a chart type. Composes `DataBridge` (subscribe)
/// with `DataBubble` (slot clear/bookkeeping stamp). Kept next to
/// `mlc-backend`'s `BackendCore` since only this crate can legally name both
/// types.
///
/// Not `Clone`-derived on purpose — the ops table is `&'static` and shared.
pub struct SubscribeOps {
    /// Fire the appropriate `bridge.request_*` for this chart type, or
    /// `None` for the 13 fold/trade-fold types (unified-data-plane Э3/Э4) —
    /// their real network trigger is `BackendCore::subscribe_fold_chart_type`/
    /// `subscribe_trade_fold_chart_type`, called by every caller of this
    /// table ALONGSIDE (not through) `ops.subscribe`, guarded by
    /// `is_fold_chart_type`/`is_trade_fold_chart_type` before this field is
    /// ever read — an honest `None` reflects that this fn-pointer is never
    /// actually invoked for those 13 rows, rather than a live no-op stub.
    /// Kline family + dom/liq heatmap keep a real subscribe fn.
    pub subscribe: Option<fn(&DataBridge, ExchangeId, &str, Dig3AccountType, &Timeframe, SubscribeParams)>,

    /// Clear the bubble slot(s) that this chart type writes into (renko_bars,
    /// pnf_columns, kagi_segments, three_line_break, tpo_sessions,
    /// footprint_bars, dom_heatmap, liq_heatmap, or `bubble.bars` for kline /
    /// bar family). Also stamps `bars_source` and `mark_footprint_pending`
    /// where relevant.
    pub clear_slot: fn(&mut DataBubble),
}

// ---------------------------------------------------------------------------
// Subscribe fns
// ---------------------------------------------------------------------------

/// Render-family (candles, bars, hollow_candles, heikin_ashi, line,
/// step_line, line_markers, area, hlc_area, baseline) and
/// trade-agg-bar family share this: they all reload plain kline bars at the
/// current TF. Default `bar_count` = 300 when the caller doesn't supply a
/// `Kline` variant.
fn subscribe_kline_bars(
    bridge: &DataBridge,
    eid: ExchangeId,
    symbol: &str,
    at: Dig3AccountType,
    tf: &Timeframe,
    params: SubscribeParams,
) {
    let bar_count = match params {
        SubscribeParams::Kline { bar_count } => bar_count,
        _ => 300,
    };
    bridge.request_bars(eid, symbol, tf, at, None, Some(bar_count), true);
}

/// Pure join: deep trade history is available exactly when `tier` is
/// `RestDeep`/`RestWindow`, **or** `archive` is `Some` (the venue publishes
/// a bulk daily trade-archive the backfill engine can chain into once the
/// live tier bottoms out). `RecentOnly` with `archive == None` is the only
/// case that resolves `false` — the sole tier with no path to deep history
/// at all.
///
/// Split out from [`deep_trade_history_available`] so the decision itself
/// (no `DataBridge`/network dependency) is unit-testable in isolation.
fn deep_from_caps(
    tier: digdigdig3::core::types::TradeHistoryTier,
    archive: Option<digdigdig3::core::types::ArchiveSpec>,
) -> bool {
    let rest_deep = matches!(
        tier,
        digdigdig3::core::types::TradeHistoryTier::RestDeep { .. }
            | digdigdig3::core::types::TradeHistoryTier::RestWindow { .. },
    );
    rest_deep || archive.is_some()
}

/// Whether deep trade history is available for `(eid, at)` on `bridge` —
/// true when the venue's live REST tier already reaches back arbitrarily
/// far (`RestDeep`) or far-but-bounded (`RestWindow`), **or** the venue
/// publishes a bulk daily trade-archive the backfill engine can chain into
/// once the live tier bottoms out (`ArchiveSpec` via
/// `TradeArchiveCapabilities::for_account`). `RecentOnly` with no archive is
/// the only case that resolves `false`.
///
/// Sibling of [`crate::deep_trade_history_available`] (same join, `Dig3AccountType`
/// already resolved here vs `mlc_core::instrument::AccountType` there —
/// callers in this module already hold the dig3 type) — single source of
/// truth for "does this venue have trade depth", consulted by
/// [`is_footprint_hybrid`] below. `None` (exchange not connected yet)
/// propagates capability-unknown.
///
/// `nemo/docs/dig3/plans/filedump-trade-archives-2026-07-09.md` §Ф3.
pub fn deep_trade_history_available(bridge: &DataBridge, eid: ExchangeId, at: Dig3AccountType) -> Option<bool> {
    let caps = bridge.trade_history_capabilities(eid)?;
    let archive = bridge.trade_archive_capabilities(eid)
        .and_then(|archive| archive.for_account(at));
    Some(deep_from_caps(caps.tier_for(at), archive))
}

/// Whether footprint on `(exchange_id, account_type)` must run hybrid
/// (kline backdrop + live-built clusters) instead of the full-takeover
/// path — true exactly when [`deep_trade_history_available`] resolves
/// `false` (no live REST depth AND no archive: a `DeepTrades` chart_type
/// has no history to page in once `bubble.bars` is cleared and the kline
/// sub is aborted).
///
/// `pub` (not crate-private) — `mlc-app`'s `ChangeChartType`/
/// `UpdateChartTypeInterval` handlers (`chart_out_events.rs`) call this
/// directly to decide `clear_slot_footprint`'s hybrid branch and the
/// backdrop-kline-resubscribe-at-bucket-interval step, both of which need
/// the SAME capability check the footprint trade-fold install uses (Stage
/// Э4 — `BackendCore::subscribe_trade_fold_chart_type`'s caller resolves
/// this to decide `bars_source`) — see
/// `nemo/docs/mlc/plans/hybrid-data-modes-plan-2026-07-08.md` Step 3 and
/// `nemo/docs/dig3/plans/filedump-trade-archives-2026-07-09.md` §Ф3.
///
/// `None` capability (exchange not connected yet) is treated as NOT
/// hybrid — "unknown" must not force hybrid on a venue that may in fact be
/// deep; the ordinary full-takeover path is the safer default until the
/// capability is actually known.
pub fn is_footprint_hybrid(bridge: &DataBridge, eid: ExchangeId, at: Dig3AccountType) -> bool {
    deep_trade_history_available(bridge, eid, at)
        .map(|deep| !deep)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Clear-slot fns
// ---------------------------------------------------------------------------

/// Render-family (10 types) share this — writes into `bubble.bars` and
/// resets `bars_source` back to `Kline` (also the reset target every OTHER
/// clear_slot fn in this module uses, including the trade-agg-bar family's
/// own [`clear_slot_trade_agg_bars`] switch-away path via
/// `chart_out_events.rs`'s generic `ops.clear_slot` dispatch).
fn clear_slot_kline_bars(bubble: &mut DataBubble) {
    bubble.clear_bars();
    bubble.bars_source = BarsSource::Kline;
}

/// Trade-agg-bar family (range_bar/volume_bar/tick_bar/dollar_bar/
/// tick_imbalance_bar/volume_imbalance_bar/run_bar — registry
/// `ChartTypeKind::TradeAggBar`, 7 types) — shares `bubble.bars` with kline
/// (see `resolve_active_series`'s mapping) but is a DIFFERENT writer: the
/// station's trade-agg derived pipeline, not the kline REST/WS path. Stamps
/// `bars_source = TradeAggBars` on subscribe so every kline-family writer
/// (`BackfillComplete`/`ScrollBarsLoaded` REST, `TradeUpdate`-built candles)
/// drops its write for this bubble instead of stomping the derived series —
/// mirrors [`clear_slot_footprint_hybrid_aware`]'s stamp for footprint.
fn clear_slot_trade_agg_bars(bubble: &mut DataBubble) {
    bubble.clear_bars();
    bubble.bars_source = BarsSource::TradeAggBars;
}

fn clear_slot_renko(bubble: &mut DataBubble) {
    bubble.clear_renko();
    bubble.bars_source = BarsSource::Kline;
}

fn clear_slot_pnf(bubble: &mut DataBubble) {
    bubble.clear_pnf();
    bubble.bars_source = BarsSource::Kline;
}

fn clear_slot_kagi(bubble: &mut DataBubble) {
    bubble.clear_kagi();
    bubble.bars_source = BarsSource::Kline;
}

fn clear_slot_three_line_break(bubble: &mut DataBubble) {
    bubble.clear_three_line_break();
    bubble.bars_source = BarsSource::Kline;
}

fn clear_slot_tpo(bubble: &mut DataBubble) {
    bubble.clear_tpo();
    bubble.bars_source = BarsSource::Kline;
}

/// Footprint's clear_slot owns ALL the bubble mutations for the switch
/// (matches `ChangeChartType`'s footprint arm at chart_out_events.rs) —
/// `subscribe_footprint` only touches `DataBridge`.
///
/// This is the `SubscribeOps` table entry — its fn-pointer type
/// (`fn(&mut DataBubble)`) carries no capability info, so it always takes
/// the full-takeover branch. The real `ChangeChartType`/
/// `UpdateChartTypeInterval` call sites in `mlc-app` do NOT use this table
/// entry for footprint — they call [`clear_slot_footprint_hybrid_aware`]
/// directly instead, since only they have `(exchange_id, account_type)` in
/// scope to resolve [`is_footprint_hybrid`]. Kept in the table anyway so
/// `ops_by_id("footprint")` stays a complete, self-consistent (if
/// capability-blind) `SubscribeOps` for any other/future caller.
fn clear_slot_footprint(bubble: &mut DataBubble) {
    clear_slot_footprint_hybrid_aware(bubble, false);
}

/// Capability-aware footprint clear_slot (hybrid-data-modes-plan-2026-07-08,
/// Step 3). `hybrid == true` (RecentOnly venue): `bars_source` STAYS
/// `Kline` and `bubble.bars` is NOT cleared — the kline backdrop keeps
/// rendering through the existing candle path while `footprint_bars`
/// accumulates live-only alongside it; clearing `bubble.bars` here would
/// blank the very backdrop hybrid mode exists to keep. `hybrid == false`
/// (deep tiers): unchanged full-takeover behavior.
pub fn clear_slot_footprint_hybrid_aware(bubble: &mut DataBubble, hybrid: bool) {
    bubble.mark_footprint_pending();
    if !hybrid {
        bubble.clear_bars();
        bubble.bars_source = BarsSource::FootprintOhlc;
    }
}

/// Clear the DOM Heatmap overlay's bubble ring-buffer state.
///
/// `pub` (not table-private) — chart-type → overlay migration moved the
/// trigger from `ChangeChartType`'s per-chart-type `SubscribeOps.clear_slot`
/// dispatch to `ChartOutEvent::ToggleDomHeatmapOverlay`'s handler
/// (`mlc-app::chart_out_events`), which calls this directly. Does NOT stamp
/// `bars_source` — the overlay never owns the bubble's kline series.
pub fn clear_slot_dom_heatmap(bubble: &mut DataBubble) {
    bubble.clear_dom_heatmap();
    bubble.clear_dom_trade_dots();
}

/// Clear the Liquidation Heatmap overlay's bubble ring-buffer state.
/// See [`clear_slot_dom_heatmap`] doc comment — same migration, same shape.
pub fn clear_slot_liq_heatmap(bubble: &mut DataBubble) {
    bubble.clear_liq_heatmap();
    bubble.clear_liq_dots();
}

// ---------------------------------------------------------------------------
// Static table
// ---------------------------------------------------------------------------

static OPS: &[(&str, SubscribeOps)] = &[
    ("candles", SubscribeOps { subscribe: Some(subscribe_kline_bars), clear_slot: clear_slot_kline_bars }),
    ("bars", SubscribeOps { subscribe: Some(subscribe_kline_bars), clear_slot: clear_slot_kline_bars }),
    ("hollow_candles", SubscribeOps { subscribe: Some(subscribe_kline_bars), clear_slot: clear_slot_kline_bars }),
    ("heikin_ashi", SubscribeOps { subscribe: Some(subscribe_kline_bars), clear_slot: clear_slot_kline_bars }),
    ("line", SubscribeOps { subscribe: Some(subscribe_kline_bars), clear_slot: clear_slot_kline_bars }),
    ("step_line", SubscribeOps { subscribe: Some(subscribe_kline_bars), clear_slot: clear_slot_kline_bars }),
    ("line_markers", SubscribeOps { subscribe: Some(subscribe_kline_bars), clear_slot: clear_slot_kline_bars }),
    ("area", SubscribeOps { subscribe: Some(subscribe_kline_bars), clear_slot: clear_slot_kline_bars }),
    ("hlc_area", SubscribeOps { subscribe: Some(subscribe_kline_bars), clear_slot: clear_slot_kline_bars }),
    ("baseline", SubscribeOps { subscribe: Some(subscribe_kline_bars), clear_slot: clear_slot_kline_bars }),
    // Unified-data-plane Э3/Э4: the 13 fold/trade-fold types below install/
    // reseed their client-side `FoldRuntime`/`TradeFoldRuntime` through
    // `BackendCore::subscribe_fold_chart_type`/`subscribe_trade_fold_chart_type`
    // instead of `ops.subscribe` — every caller of this table checks
    // `is_fold_chart_type`/`is_trade_fold_chart_type` BEFORE ever reading
    // `ops.subscribe`, so `None` here is never actually dereferenced in
    // practice; it is the honest value for "no station-side subscribe fn".
    ("range_bar", SubscribeOps { subscribe: None, clear_slot: clear_slot_trade_agg_bars }),
    ("volume_bar", SubscribeOps { subscribe: None, clear_slot: clear_slot_trade_agg_bars }),
    ("tick_bar", SubscribeOps { subscribe: None, clear_slot: clear_slot_trade_agg_bars }),
    ("dollar_bar", SubscribeOps { subscribe: None, clear_slot: clear_slot_trade_agg_bars }),
    ("tick_imbalance_bar", SubscribeOps { subscribe: None, clear_slot: clear_slot_trade_agg_bars }),
    ("volume_imbalance_bar", SubscribeOps { subscribe: None, clear_slot: clear_slot_trade_agg_bars }),
    ("run_bar", SubscribeOps { subscribe: None, clear_slot: clear_slot_trade_agg_bars }),
    ("renko", SubscribeOps { subscribe: None, clear_slot: clear_slot_renko }),
    ("pnf", SubscribeOps { subscribe: None, clear_slot: clear_slot_pnf }),
    ("kagi", SubscribeOps { subscribe: None, clear_slot: clear_slot_kagi }),
    ("three_line_break", SubscribeOps { subscribe: None, clear_slot: clear_slot_three_line_break }),
    ("footprint", SubscribeOps { subscribe: None, clear_slot: clear_slot_footprint }),
    ("tpo", SubscribeOps { subscribe: None, clear_slot: clear_slot_tpo }),
    // "dom_heatmap" / "liquidation_heatmap" REMOVED (chart-type → overlay
    // migration) — [`clear_slot_dom_heatmap`]/[`clear_slot_liq_heatmap`] are
    // now called directly by the overlay toggle handler, not through this
    // per-chart-type table. See their doc comments above.
];

/// Look up the `SubscribeOps` for a chart_type id. `None` for unknown ids.
pub fn ops_by_id(id: &str) -> Option<&'static SubscribeOps> {
    OPS.iter().find(|(k, _)| *k == id).map(|(_, v)| v)
}

// ---------------------------------------------------------------------------
// Client-side fold cutover (unified-data-plane Stage Э3)
// ---------------------------------------------------------------------------

/// The 7 kline-sufficient chart types that fold client-side over the
/// bubble's own "1m" kline layer instead of opening a station-side derived
/// subscription. Registry-driven (chart-type-engine-consolidation B3, S1):
/// membership is every row whose `machine` is a kline-fold
/// [`mlc_core::chart_type::MachineKind`] variant (everything the
/// [`MachineKind`](mlc_core::chart_type::MachineKind) doc comment classifies
/// as folding the "1m" layer, not the trade layer — mirrors
/// `crate::bubble::FoldMachine`'s variant set exactly).
pub fn fold_chart_types() -> impl Iterator<Item = &'static str> {
    mlc_core::chart_type::all().iter().filter_map(|def| {
        matches!(
            def.machine,
            mlc_core::chart_type::MachineKind::RangeBar
                | mlc_core::chart_type::MachineKind::VolumeBar
                | mlc_core::chart_type::MachineKind::DollarBar
                | mlc_core::chart_type::MachineKind::Renko
                | mlc_core::chart_type::MachineKind::Pnf
                | mlc_core::chart_type::MachineKind::Kagi
                | mlc_core::chart_type::MachineKind::ThreeLineBreak
        )
        .then_some(def.id)
    })
}

/// Whether `chart_type` is one of the 7 client-side fold types.
pub fn is_fold_chart_type(chart_type: &str) -> bool {
    fold_chart_types().any(|id| id == chart_type)
}

/// Registry-driven default fallback for a chart_type's numeric param
/// (chart-type-engine-consolidation B3, S3) — thin forward to the single
/// canonical resolver [`mlc_core::chart_type::ChartTypeDef::default_param_value`]
/// (canon-fix wave 2, item 1: `mlc-core` is the one crate every reader —
/// `mlc-app`'s UI seam included — already depends on, so the rule now lives
/// there instead of being duplicated per-crate) so [`fold_params_for`]'s
/// defensive fallback branch reads from the SAME single source of truth
/// (the registry row) instead of an independent literal. Only reachable when
/// `params` doesn't carry the `SubscribeParams` shape `def.machine` expects —
/// `chart_out_events.rs`'s `build_subscribe_params` always produces the
/// matching variant in practice, so this is a last-resort default, not a
/// live UI path.
fn registry_default_param(def: &mlc_core::chart_type::ChartTypeDef) -> f64 {
    def.default_param_value().unwrap_or(0.0)
}

/// Narrow a resolved `SubscribeParams` (ticks × tick_size already applied at
/// the `chart_out_events.rs` UI seam — see `build_subscribe_params`) into
/// the scalar [`FoldParams`] shape [`crate::bubble::FoldRuntime`]/
/// [`crate::bubble::TradeFoldRuntime`] needs — ONE registry-driven function
/// (chart-type-engine-consolidation B3, S3) covering both the kline-fold
/// (Э3) and trade-fold (Э4) families, dispatched on `def.machine` instead of
/// two separate `chart_type: &str` forward matches. Returns `None` for any
/// chart_type outside the 13 fold+trade-fold types, or when the registry
/// lookup misses.
///
/// Fixed non-editable constants (renko/pnf reversal, imbalance/run alpha)
/// come from `def.fixed_consts` — no longer independently hardcoded here.
/// The defensive `params` shape-mismatch fallback (see
/// [`registry_default_param`]'s doc comment) reads the registry's
/// `inline_default`/ladder-first value instead of an independent literal —
/// closes the exact class of drift the 2026-07-10 "1T subscribes 100"
/// bug was ([`registry_default_param`]'s doc comment).
pub fn fold_params_for(def: &mlc_core::chart_type::ChartTypeDef, params: SubscribeParams) -> Option<FoldParams> {
    use mlc_core::chart_type::MachineKind;
    match def.machine {
        MachineKind::RangeBar => {
            let scaled_units = match params {
                SubscribeParams::ScaledUnits { scaled_u64 } => scaled_u64 as f64 / 1e8,
                _ => registry_default_param(def),
            };
            Some(FoldParams { scaled_units, ..Default::default() })
        }
        MachineKind::VolumeBar => {
            let scaled_units = match params {
                SubscribeParams::ScaledUnits { scaled_u64 } => scaled_u64 as f64 / 1e8,
                _ => registry_default_param(def),
            };
            Some(FoldParams { scaled_units, ..Default::default() })
        }
        MachineKind::DollarBar => {
            let threshold = match params {
                SubscribeParams::Threshold { threshold_u64 } => threshold_u64 as f64,
                _ => registry_default_param(def),
            };
            Some(FoldParams { threshold, ..Default::default() })
        }
        MachineKind::Renko => {
            let default_reversal = def.fixed_consts.reversal.unwrap_or(2);
            let (scaled_units, reversal) = match params {
                SubscribeParams::ScaledUnitsWithReversal { scaled_u64, reversal } => (scaled_u64 as f64 / 1e8, reversal),
                _ => (registry_default_param(def), default_reversal),
            };
            Some(FoldParams { scaled_units, reversal, ..Default::default() })
        }
        MachineKind::Pnf => {
            let default_reversal = def.fixed_consts.reversal.unwrap_or(3);
            let (scaled_units, reversal) = match params {
                SubscribeParams::ScaledUnitsWithReversal { scaled_u64, reversal } => (scaled_u64 as f64 / 1e8, reversal),
                _ => (registry_default_param(def), default_reversal),
            };
            Some(FoldParams { scaled_units, reversal, box_size: scaled_units, ..Default::default() })
        }
        MachineKind::Kagi => {
            let scaled_units = match params {
                SubscribeParams::ScaledUnits { scaled_u64 } => scaled_u64 as f64 / 1e8,
                _ => registry_default_param(def),
            };
            Some(FoldParams { scaled_units, ..Default::default() })
        }
        MachineKind::ThreeLineBreak => {
            let lines_back = match params {
                SubscribeParams::LinesBack { lines_back } => lines_back,
                _ => registry_default_param(def) as u8,
            };
            Some(FoldParams { lines_back, ..Default::default() })
        }
        MachineKind::TickBar => {
            let count = match params {
                SubscribeParams::Count { count_u32 } => count_u32,
                _ => registry_default_param(def) as u32,
            };
            Some(FoldParams { count, ..Default::default() })
        }
        MachineKind::TickImbalance | MachineKind::VolumeImbalance | MachineKind::RunBar => {
            let default_alpha_x100 = def.fixed_consts.alpha_x100.unwrap_or(20);
            let (alpha_x100, min_ticks) = match params {
                SubscribeParams::MinTicks { alpha_x100, min_ticks } => (alpha_x100, min_ticks),
                _ => (default_alpha_x100, registry_default_param(def) as u32),
            };
            Some(FoldParams { alpha: alpha_x100 as f64 / 100.0, min_ticks, ..Default::default() })
        }
        MachineKind::Footprint => {
            let interval = match params {
                SubscribeParams::Interval { interval } => interval,
                _ => def.inline_default.unwrap_or("1m"),
            };
            let interval_ms = digdigdig3_station::folds::interval_to_ms(interval).unwrap_or(60_000);
            Some(FoldParams { interval_ms, ..Default::default() })
        }
        MachineKind::Tpo => {
            let freq_u16 = match params {
                SubscribeParams::Freq { freq_u16 } => freq_u16,
                _ => registry_default_param(def) as u16,
            };
            Some(FoldParams { freq_minutes: freq_u16, ..Default::default() })
        }
        MachineKind::None => None,
    }
}

/// `&str`-id wrapper over [`fold_params_for`] — resolves `chart_type` via
/// the registry then delegates. Kept for the existing call sites (`mlc-app`)
/// that only have the chart_type string in scope; returns `None` for any
/// chart_type outside the 7 kline-fold types (registry lookup miss, or a
/// `def.machine` outside the kline-fold family).
pub fn fold_params_from_subscribe_params(chart_type: &str, params: SubscribeParams) -> Option<FoldParams> {
    let def = mlc_core::chart_type::by_id(chart_type)?;
    if !fold_chart_types().any(|id| id == chart_type) {
        return None;
    }
    fold_params_for(def, params)
}

// ---------------------------------------------------------------------------
// Trade-fed fold cutover (unified-data-plane Stage Э4)
// ---------------------------------------------------------------------------

/// The 6 trade-fed chart types that fold client-side over the bubble's own
/// trade layer instead of opening a station-side derived subscription.
/// Registry-driven (chart-type-engine-consolidation B3, S1): membership is
/// every row whose `machine` is a trade-fold
/// [`mlc_core::chart_type::MachineKind`] variant — mirrors
/// `crate::bubble::TradeFoldMachine`'s variant set exactly (`Cvd` excluded:
/// the CVD companion is not a registry row, see that module's doc comment).
pub fn trade_fold_chart_types() -> impl Iterator<Item = &'static str> {
    mlc_core::chart_type::all().iter().filter_map(|def| {
        matches!(
            def.machine,
            mlc_core::chart_type::MachineKind::TickBar
                | mlc_core::chart_type::MachineKind::TickImbalance
                | mlc_core::chart_type::MachineKind::VolumeImbalance
                | mlc_core::chart_type::MachineKind::RunBar
                | mlc_core::chart_type::MachineKind::Footprint
                | mlc_core::chart_type::MachineKind::Tpo
        )
        .then_some(def.id)
    })
}

/// Whether `chart_type` is one of the 6 client-side trade-fold types.
pub fn is_trade_fold_chart_type(chart_type: &str) -> bool {
    trade_fold_chart_types().any(|id| id == chart_type)
}

/// `&str`-id wrapper over [`fold_params_for`] — see
/// [`fold_params_from_subscribe_params`]'s doc comment for the same
/// rationale (kept for existing `mlc-app` call sites that only have the
/// chart_type string in scope). Returns `None` for any chart_type outside
/// the 6 trade-fold types.
pub fn trade_fold_params_from_subscribe_params(chart_type: &str, params: SubscribeParams) -> Option<FoldParams> {
    let def = mlc_core::chart_type::by_id(chart_type)?;
    if !trade_fold_chart_types().any(|id| id == chart_type) {
        return None;
    }
    fold_params_for(def, params)
}

/// Default [`TpoSource`] the trade-fold cutover installs for `"tpo"`.
///
/// `Kline1m` — TPO letter buckets are canonically built from intraday bars
/// on every real platform (Sierra Chart, TradingView, Bookmap); 1-minute
/// klines are lossless for the 30-minute letter-period buckets TPO uses by
/// default (`FoldParams::freq_minutes`), and the kline-fed path
/// (`TradeFoldMachine::TpoKline1m` / `TpoFromKline1mDerived`) gets deep
/// history for free via [`crate::chart_type::lifecycle::TPO_KLINE_DEPTH_BARS`]
/// instead of being bounded by however many raw trades the venue/backfill
/// window can supply. The pre-cutover `subscribe_tpo` hardcoded
/// `TpoSource::TradeBucket` (no UI selector existed) — that is now the
/// non-default fallback flavour, still fully wired for symbols/venues where
/// a deep kline history isn't available. `mlc-app` passes this through
/// `BackendCore::subscribe_trade_fold_chart_type`'s `tpo_source` parameter —
/// change this single constant (or thread a real per-bubble selector
/// through the call sites) to switch the default without touching the fold
/// machinery itself.
pub const DEFAULT_TPO_SOURCE: TpoSource = TpoSource::Kline1m;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registry_id_has_ops() {
        for def in mlc_core::chart_type::all() {
            assert!(ops_by_id(def.id).is_some(), "no SubscribeOps for chart_type {}", def.id);
        }
    }

    /// Registry-derived membership (B3, S1) must reproduce the exact 7/6
    /// sets the pre-derivation hand-written `const` slices carried, and the
    /// two families must be disjoint from each other and from `is_fold_chart_type`'s
    /// complement.
    #[test]
    fn fold_and_trade_fold_chart_types_membership_pinned() {
        let fold: Vec<&str> = fold_chart_types().collect();
        assert_eq!(
            fold,
            vec!["range_bar", "volume_bar", "dollar_bar", "renko", "pnf", "kagi", "three_line_break"],
        );
        let trade_fold: Vec<&str> = trade_fold_chart_types().collect();
        assert_eq!(
            trade_fold,
            vec!["tick_bar", "tick_imbalance_bar", "volume_imbalance_bar", "run_bar", "footprint", "tpo"],
        );
        for id in &fold {
            assert!(is_fold_chart_type(id));
            assert!(!is_trade_fold_chart_type(id));
        }
        for id in &trade_fold {
            assert!(is_trade_fold_chart_type(id));
            assert!(!is_fold_chart_type(id));
        }
    }

    /// `fold_params_for`'s fixed-constant arms must read `def.fixed_consts`,
    /// not an independent literal (B3, S3) — renko/pnf reversal,
    /// imbalance/run alpha.
    #[test]
    fn fold_params_for_fixed_consts_match_registry() {
        let renko = mlc_core::chart_type::by_id("renko").unwrap();
        let out = fold_params_for(renko, SubscribeParams::Kline { bar_count: 1 }).unwrap();
        assert_eq!(out.reversal, renko.fixed_consts.reversal.unwrap());

        let pnf = mlc_core::chart_type::by_id("pnf").unwrap();
        let out = fold_params_for(pnf, SubscribeParams::Kline { bar_count: 1 }).unwrap();
        assert_eq!(out.reversal, pnf.fixed_consts.reversal.unwrap());

        for id in ["tick_imbalance_bar", "volume_imbalance_bar", "run_bar"] {
            let def = mlc_core::chart_type::by_id(id).unwrap();
            let out = fold_params_for(def, SubscribeParams::Kline { bar_count: 1 }).unwrap();
            let expected_alpha = def.fixed_consts.alpha_x100.unwrap() as f64 / 100.0;
            assert_eq!(out.alpha, expected_alpha, "{id} alpha mismatch");
        }
    }

    /// Defensive shape-mismatch fallback (`params` not the expected
    /// variant) must read the registry's `inline_default`/ladder-first
    /// value, not an independent literal — closes the volume_bar/dollar_bar
    /// drift that existed before B3, S3 (fallback `1000.0`/`1_000_000.0`
    /// vs registry `inline_default` `"10.0"`/`"100000"`).
    #[test]
    fn fold_params_for_defensive_fallback_matches_registry_inline_default() {
        let mismatched = SubscribeParams::Kline { bar_count: 1 };
        let volume_bar = mlc_core::chart_type::by_id("volume_bar").unwrap();
        let out = fold_params_for(volume_bar, mismatched).unwrap();
        assert_eq!(out.scaled_units, 10.0, "volume_bar fallback must match inline_default \"10.0\"");

        let dollar_bar = mlc_core::chart_type::by_id("dollar_bar").unwrap();
        let out = fold_params_for(dollar_bar, mismatched).unwrap();
        assert_eq!(out.threshold, 100_000.0, "dollar_bar fallback must match inline_default \"100000\"");
    }

    #[test]
    fn ops_table_ids_unique() {
        let ids: Vec<&str> = OPS.iter().map(|(k, _)| *k).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len());
    }

    fn dummy_archive_spec() -> digdigdig3::core::types::ArchiveSpec {
        digdigdig3::core::types::ArchiveSpec {
            host: "public.bybit.com",
            url_pattern: "/spot/{symbol}/{symbol}_{date}.csv.gz",
            format: digdigdig3::core::types::ArchiveFormat::CsvGz,
            schema: digdigdig3::core::types::ArchiveSchema::BybitSpot,
            since_ms: 0,
        }
    }

    // deep_from_caps — the pure join `deep_trade_history_available`/
    // `BackendCore::deep_trade_history_available` both delegate to.

    #[test]
    fn deep_from_caps_rest_deep_is_always_deep() {
        let tier = digdigdig3::core::types::TradeHistoryTier::RestDeep {
            cursor: digdigdig3::core::types::HistoryCursor::FromId,
        };
        assert!(deep_from_caps(tier, None));
        assert!(deep_from_caps(tier, Some(dummy_archive_spec())));
    }

    #[test]
    fn deep_from_caps_rest_window_is_always_deep() {
        let tier = digdigdig3::core::types::TradeHistoryTier::RestWindow {
            cursor: digdigdig3::core::types::HistoryCursor::TsWindow,
            max_back_ms: 86_400_000,
        };
        assert!(deep_from_caps(tier, None));
    }

    #[test]
    fn deep_from_caps_recent_only_with_archive_is_deep() {
        // Bybit spot: RecentOnly{60} live tier, but public.bybit.com day-file
        // archive fills the gap — must resolve deep, not hybrid/live-only.
        let tier = digdigdig3::core::types::TradeHistoryTier::RecentOnly { max_trades: 60 };
        assert!(deep_from_caps(tier, Some(dummy_archive_spec())));
    }

    #[test]
    fn deep_from_caps_recent_only_without_archive_is_shallow() {
        let tier = digdigdig3::core::types::TradeHistoryTier::RecentOnly { max_trades: 1000 };
        assert!(!deep_from_caps(tier, None));
    }
}
