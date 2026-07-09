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
    /// No parameters (dom_heatmap, liquidation_heatmap).
    None,
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

/// Data-plane engine ops for a chart type. Composes `DataBridge`
/// (subscribe/abort) with `DataBubble` (slot clear/bookkeeping stamp). Kept
/// next to `mlc-backend`'s `BackendCore` since only this crate can legally
/// name both types.
///
/// Not `Clone`-derived on purpose — the ops table is `&'static` and shared.
pub struct SubscribeOps {
    /// Fire the appropriate `bridge.request_*` for this chart type.
    /// Takes `&DataBridge` + instrument coords + `SubscribeParams`.
    /// Returns `()` — subscribe methods are fire-and-forget.
    pub subscribe: fn(&DataBridge, ExchangeId, &str, Dig3AccountType, &Timeframe, SubscribeParams),

    /// Clear the bubble slot(s) that this chart type writes into (renko_bars,
    /// pnf_columns, kagi_segments, three_line_break, tpo_sessions,
    /// footprint_bars, dom_heatmap, liq_heatmap, or `bubble.bars` for kline /
    /// bar family). Also stamps `bars_source` and `mark_footprint_pending`
    /// where relevant.
    pub clear_slot: fn(&mut DataBubble),

    /// Abort any live subscription for this chart type (idempotent). Called
    /// before switching AWAY from this type. Wraps `bridge.abort_*` calls.
    pub abort: fn(&DataBridge, ExchangeId, &str, Dig3AccountType, &Timeframe),
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

/// Unified-data-plane Stage Э3: range_bar/volume_bar/dollar_bar/renko/pnf/
/// kagi/three_line_break no longer open a station-side derived subscription
/// through the `SubscribeOps` table — they are client-side folds over the
/// bubble's own "1m" kline layer, installed and reseeded by
/// `BackendCore::subscribe_fold_chart_type` (called alongside `ops.subscribe`
/// at every one of this table's 3 call sites — see
/// `chart_out_events.rs`/`ignite_derived_windows`). This fn-pointer slot is
/// kept as a documented no-op (not removed) so `ops_by_id` stays a complete
/// table for the 24 registered chart types and the `every_registry_id_has_ops`
/// test keeps passing; the REAL network trigger for these 7 types is
/// `subscribe_fold_chart_type`'s "ensure 1m kline layer" step. The old
/// `bridge.request_range_bars`/etc. station-derived paths are left intact on
/// `DataBridge` (Э5 demolition, not this stage) — simply uncalled from here.
fn subscribe_fold_noop(
    _bridge: &DataBridge,
    _eid: ExchangeId,
    _symbol: &str,
    _at: Dig3AccountType,
    _tf: &Timeframe,
    _params: SubscribeParams,
) {
}

/// Unified-data-plane Stage Э4: tick_bar/tick_imbalance_bar/
/// volume_imbalance_bar/run_bar/footprint/tpo no longer open a station-side
/// derived subscription through this table — they are client-side folds
/// over the bubble's own trade layer, installed and reseeded by
/// `BackendCore::subscribe_trade_fold_chart_type` (called alongside
/// `ops.subscribe` at every one of this table's 3 call sites — see
/// `chart_out_events.rs`). This fn-pointer slot is kept as a documented
/// no-op (not removed) so `ops_by_id` stays a complete table for every
/// registered chart type and the `every_registry_id_has_ops` test keeps
/// passing; the REAL network trigger for these 6 types is
/// `subscribe_trade_fold_chart_type`'s "ensure trade layer feed" step.
/// Mirrors [`subscribe_fold_noop`]'s role for the kline-sufficient family.
/// The old `bridge.request_tick_bars`/`request_footprint`/etc. station-
/// derived paths are left intact on `DataBridge` (Э5 demolition, not this
/// stage) — simply uncalled from here.
fn subscribe_trade_fold_noop(
    _bridge: &DataBridge,
    _eid: ExchangeId,
    _symbol: &str,
    _at: Dig3AccountType,
    _tf: &Timeframe,
    _params: SubscribeParams,
) {
}

/// Whether footprint on `(exchange_id, account_type)` must run hybrid
/// (kline backdrop + live-built clusters) instead of the full-takeover
/// path — true exactly when the venue's trade-history capability is the
/// `RecentOnly` tier (no REST pagination exists at all, so a `DeepTrades`
/// chart_type has no history to page in once `bubble.bars` is cleared and
/// the kline sub is aborted).
///
/// `pub` (not crate-private) — `mlc-app`'s `ChangeChartType`/
/// `UpdateChartTypeInterval` handlers (`chart_out_events.rs`) call this
/// directly to decide `clear_slot_footprint`'s hybrid branch and the
/// backdrop-kline-resubscribe-at-bucket-interval step, both of which need
/// the SAME capability check the footprint trade-fold install uses (Stage
/// Э4 — `BackendCore::subscribe_trade_fold_chart_type`'s caller resolves
/// this to decide `bars_source`) — see
/// `nemo/docs/mlc/plans/hybrid-data-modes-plan-2026-07-08.md` Step 3.
///
/// `None` capability (exchange not connected yet) is treated as NOT
/// hybrid — "unknown" must not force hybrid on a venue that may in fact be
/// deep; the ordinary full-takeover path is the safer default until the
/// capability is actually known.
pub fn is_footprint_hybrid(bridge: &DataBridge, eid: ExchangeId, at: Dig3AccountType) -> bool {
    bridge.trade_history_capabilities(eid)
        .map(|caps| matches!(
            caps.tier_for(at),
            digdigdig3::core::types::TradeHistoryTier::RecentOnly { .. },
        ))
        .unwrap_or(false)
}

fn subscribe_dom_heatmap(
    bridge: &DataBridge,
    eid: ExchangeId,
    symbol: &str,
    at: Dig3AccountType,
    _tf: &Timeframe,
    _params: SubscribeParams,
) {
    bridge.request_dom_heatmap(eid, symbol, at);
}

fn subscribe_liq_heatmap(
    bridge: &DataBridge,
    eid: ExchangeId,
    symbol: &str,
    at: Dig3AccountType,
    _tf: &Timeframe,
    _params: SubscribeParams,
) {
    bridge.request_liquidation_heatmap(eid, symbol, at);
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

fn clear_slot_dom_heatmap(bubble: &mut DataBubble) {
    bubble.clear_dom_heatmap();
    bubble.clear_dom_trade_dots();
    bubble.bars_source = BarsSource::Kline;
}

fn clear_slot_liq_heatmap(bubble: &mut DataBubble) {
    bubble.clear_liq_heatmap();
    bubble.clear_liq_dots();
    bubble.bars_source = BarsSource::Kline;
}

// ---------------------------------------------------------------------------
// Abort fns
// ---------------------------------------------------------------------------

/// Kline/render-family abort — releases the live `Stream::Kline` forwarder
/// for the given TF.
fn abort_kline(bridge: &DataBridge, eid: ExchangeId, symbol: &str, at: Dig3AccountType, tf: &Timeframe) {
    bridge.abort_kline_bars(eid, symbol, at, &tf.name);
}

/// Every derived chart type (trade-agg bars, price-action transforms, tpo,
/// the two heatmaps) shares one `derived_subs` slot in `DataBridge` — a
/// single `abort_derived_chart` call releases whichever stream was live.
fn abort_derived(bridge: &DataBridge, eid: ExchangeId, symbol: &str, at: Dig3AccountType, _tf: &Timeframe) {
    bridge.abort_derived_chart(eid, symbol, at);
}

/// Unified-data-plane Stage Э3: the 7 fold types hold no station-side
/// derived subscription to abort (see [`subscribe_fold_noop`]) — their "1m"
/// kline feed is the SAME continuous per-bubble layer feed every chart type
/// shares (Stage Э1), never torn down on a chart-type switch. Dropping
/// `bubble.active_fold` (bubble-mutating, so it happens at the call sites
/// alongside `ops.clear_slot` — this fn-pointer has no bubble access) is
/// the only per-switch cleanup these types need.
fn abort_fold_noop(_bridge: &DataBridge, _eid: ExchangeId, _symbol: &str, _at: Dig3AccountType, _tf: &Timeframe) {
}

/// Unified-data-plane Stage Э4: the 6 trade-fed fold types (tick_bar/
/// tick_imbalance_bar/volume_imbalance_bar/run_bar/footprint/tpo) hold no
/// station-side derived subscription to abort either — their trade-layer
/// feed is the SAME continuous per-bubble layer feed every chart type
/// shares (Stage Э1), never torn down on a chart-type switch. Footprint's
/// (possibly hybrid) kline backdrop is likewise left running — same
/// rationale [`subscribe_trade_fold_noop`]'s doc comment gives for not
/// calling `abort_kline_bars` here. Dropping `bubble.active_trade_fold`
/// (bubble-mutating, so it happens at the call sites alongside
/// `ops.clear_slot` — this fn-pointer has no bubble access) is the only
/// per-switch cleanup these types need. Mirrors [`abort_fold_noop`]'s role
/// for the kline-sufficient family.
fn abort_trade_fold_noop(_bridge: &DataBridge, _eid: ExchangeId, _symbol: &str, _at: Dig3AccountType, _tf: &Timeframe) {
}

// ---------------------------------------------------------------------------
// Static table
// ---------------------------------------------------------------------------

static OPS: &[(&str, SubscribeOps)] = &[
    ("candles", SubscribeOps { subscribe: subscribe_kline_bars, clear_slot: clear_slot_kline_bars, abort: abort_kline }),
    ("bars", SubscribeOps { subscribe: subscribe_kline_bars, clear_slot: clear_slot_kline_bars, abort: abort_kline }),
    ("hollow_candles", SubscribeOps { subscribe: subscribe_kline_bars, clear_slot: clear_slot_kline_bars, abort: abort_kline }),
    ("heikin_ashi", SubscribeOps { subscribe: subscribe_kline_bars, clear_slot: clear_slot_kline_bars, abort: abort_kline }),
    ("line", SubscribeOps { subscribe: subscribe_kline_bars, clear_slot: clear_slot_kline_bars, abort: abort_kline }),
    ("step_line", SubscribeOps { subscribe: subscribe_kline_bars, clear_slot: clear_slot_kline_bars, abort: abort_kline }),
    ("line_markers", SubscribeOps { subscribe: subscribe_kline_bars, clear_slot: clear_slot_kline_bars, abort: abort_kline }),
    ("area", SubscribeOps { subscribe: subscribe_kline_bars, clear_slot: clear_slot_kline_bars, abort: abort_kline }),
    ("hlc_area", SubscribeOps { subscribe: subscribe_kline_bars, clear_slot: clear_slot_kline_bars, abort: abort_kline }),
    ("baseline", SubscribeOps { subscribe: subscribe_kline_bars, clear_slot: clear_slot_kline_bars, abort: abort_kline }),
    ("range_bar", SubscribeOps { subscribe: subscribe_fold_noop, clear_slot: clear_slot_trade_agg_bars, abort: abort_fold_noop }),
    ("volume_bar", SubscribeOps { subscribe: subscribe_fold_noop, clear_slot: clear_slot_trade_agg_bars, abort: abort_fold_noop }),
    // Unified-data-plane Stage Э4: tick_bar/tick_imbalance_bar/volume_imbalance_bar/
    // run_bar/footprint/tpo no longer open a station-side derived subscription
    // through this table either — client-side folds over the bubble's own
    // trade layer (`BackendCore::subscribe_trade_fold_chart_type`), installed
    // at the same 3 `chart_out_events.rs` call sites the Э3 cutover uses. See
    // `subscribe_trade_fold_noop`'s doc comment (mirrors `subscribe_fold_noop`).
    ("tick_bar", SubscribeOps { subscribe: subscribe_trade_fold_noop, clear_slot: clear_slot_trade_agg_bars, abort: abort_trade_fold_noop }),
    ("dollar_bar", SubscribeOps { subscribe: subscribe_fold_noop, clear_slot: clear_slot_trade_agg_bars, abort: abort_fold_noop }),
    ("tick_imbalance_bar", SubscribeOps { subscribe: subscribe_trade_fold_noop, clear_slot: clear_slot_trade_agg_bars, abort: abort_trade_fold_noop }),
    ("volume_imbalance_bar", SubscribeOps { subscribe: subscribe_trade_fold_noop, clear_slot: clear_slot_trade_agg_bars, abort: abort_trade_fold_noop }),
    ("run_bar", SubscribeOps { subscribe: subscribe_trade_fold_noop, clear_slot: clear_slot_trade_agg_bars, abort: abort_trade_fold_noop }),
    ("renko", SubscribeOps { subscribe: subscribe_fold_noop, clear_slot: clear_slot_renko, abort: abort_fold_noop }),
    ("pnf", SubscribeOps { subscribe: subscribe_fold_noop, clear_slot: clear_slot_pnf, abort: abort_fold_noop }),
    ("kagi", SubscribeOps { subscribe: subscribe_fold_noop, clear_slot: clear_slot_kagi, abort: abort_fold_noop }),
    ("three_line_break", SubscribeOps { subscribe: subscribe_fold_noop, clear_slot: clear_slot_three_line_break, abort: abort_fold_noop }),
    ("footprint", SubscribeOps { subscribe: subscribe_trade_fold_noop, clear_slot: clear_slot_footprint, abort: abort_trade_fold_noop }),
    ("tpo", SubscribeOps { subscribe: subscribe_trade_fold_noop, clear_slot: clear_slot_tpo, abort: abort_trade_fold_noop }),
    ("dom_heatmap", SubscribeOps { subscribe: subscribe_dom_heatmap, clear_slot: clear_slot_dom_heatmap, abort: abort_derived }),
    ("liquidation_heatmap", SubscribeOps { subscribe: subscribe_liq_heatmap, clear_slot: clear_slot_liq_heatmap, abort: abort_derived }),
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
/// subscription (see [`subscribe_fold_noop`]/[`abort_fold_noop`] above).
pub const FOLD_CHART_TYPES: &[&str] = &[
    "range_bar", "volume_bar", "dollar_bar", "renko", "pnf", "kagi", "three_line_break",
];

/// Whether `chart_type` is one of the 7 client-side fold types.
pub fn is_fold_chart_type(chart_type: &str) -> bool {
    FOLD_CHART_TYPES.contains(&chart_type)
}

/// Narrow a resolved `SubscribeParams` (ticks × tick_size already applied at
/// the `chart_out_events.rs` UI seam — see `build_subscribe_params`) into
/// the scalar [`FoldParams`] shape [`crate::bubble::FoldRuntime`] needs.
/// Returns `None` for any chart_type outside the 7 fold types, or when
/// `params` doesn't carry the shape that chart_type expects (defensive —
/// mirrors the `_ => default` fallback every `subscribe_*` fn used before
/// this cutover).
pub fn fold_params_from_subscribe_params(chart_type: &str, params: SubscribeParams) -> Option<FoldParams> {
    match chart_type {
        "range_bar" => {
            let scaled_units = match params {
                SubscribeParams::ScaledUnits { scaled_u64 } => scaled_u64 as f64 / 1e8,
                _ => 10.0,
            };
            Some(FoldParams { scaled_units, ..Default::default() })
        }
        "volume_bar" => {
            let scaled_units = match params {
                SubscribeParams::ScaledUnits { scaled_u64 } => scaled_u64 as f64 / 1e8,
                _ => 1000.0,
            };
            Some(FoldParams { scaled_units, ..Default::default() })
        }
        "dollar_bar" => {
            let threshold = match params {
                SubscribeParams::Threshold { threshold_u64 } => threshold_u64 as f64,
                _ => 1_000_000.0,
            };
            Some(FoldParams { threshold, ..Default::default() })
        }
        "renko" => {
            let (scaled_units, reversal) = match params {
                SubscribeParams::ScaledUnitsWithReversal { scaled_u64, reversal } => (scaled_u64 as f64 / 1e8, reversal),
                _ => (10.0, 2),
            };
            Some(FoldParams { scaled_units, reversal, ..Default::default() })
        }
        "pnf" => {
            let (scaled_units, reversal) = match params {
                SubscribeParams::ScaledUnitsWithReversal { scaled_u64, reversal } => (scaled_u64 as f64 / 1e8, reversal),
                _ => (10.0, 3),
            };
            Some(FoldParams { scaled_units, reversal, box_size: scaled_units, ..Default::default() })
        }
        "kagi" => {
            let scaled_units = match params {
                SubscribeParams::ScaledUnits { scaled_u64 } => scaled_u64 as f64 / 1e8,
                _ => 10.0,
            };
            Some(FoldParams { scaled_units, ..Default::default() })
        }
        "three_line_break" => {
            let lines_back = match params {
                SubscribeParams::LinesBack { lines_back } => lines_back,
                _ => 3,
            };
            Some(FoldParams { lines_back, ..Default::default() })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Trade-fed fold cutover (unified-data-plane Stage Э4)
// ---------------------------------------------------------------------------

/// The 6 trade-fed chart types that fold client-side over the bubble's own
/// trade layer instead of opening a station-side derived subscription (see
/// [`subscribe_trade_fold_noop`]/[`abort_trade_fold_noop`] above).
pub const TRADE_FOLD_CHART_TYPES: &[&str] = &[
    "tick_bar", "tick_imbalance_bar", "volume_imbalance_bar", "run_bar", "footprint", "tpo",
];

/// Whether `chart_type` is one of the 6 client-side trade-fold types.
pub fn is_trade_fold_chart_type(chart_type: &str) -> bool {
    TRADE_FOLD_CHART_TYPES.contains(&chart_type)
}

/// Narrow a resolved `SubscribeParams` (see `build_subscribe_params`) into
/// the scalar [`FoldParams`] shape [`crate::bubble::TradeFoldRuntime`] needs.
/// Returns `None` for any chart_type outside the 6 trade-fold types, or when
/// `params` doesn't carry the shape that chart_type expects (defensive —
/// mirrors [`fold_params_from_subscribe_params`]'s same fallback discipline,
/// and the `_ => default` fallback every dead `subscribe_*` fn used before
/// this cutover).
pub fn trade_fold_params_from_subscribe_params(chart_type: &str, params: SubscribeParams) -> Option<FoldParams> {
    match chart_type {
        "tick_bar" => {
            let count = match params {
                SubscribeParams::Count { count_u32 } => count_u32,
                _ => 100, // default 100 trades
            };
            Some(FoldParams { count, ..Default::default() })
        }
        "tick_imbalance_bar" | "volume_imbalance_bar" | "run_bar" => {
            let (alpha_x100, min_ticks) = match params {
                SubscribeParams::MinTicks { alpha_x100, min_ticks } => (alpha_x100, min_ticks),
                _ => (20, 50), // default alpha=20 (0.20), min_ticks=50
            };
            Some(FoldParams { alpha: alpha_x100 as f64 / 100.0, min_ticks, ..Default::default() })
        }
        "footprint" => {
            let interval = match params {
                SubscribeParams::Interval { interval } => interval,
                _ => "1m",
            };
            let interval_ms = digdigdig3_station::folds::interval_to_ms(interval).unwrap_or(60_000);
            Some(FoldParams { interval_ms, ..Default::default() })
        }
        "tpo" => {
            let freq_u16 = match params {
                SubscribeParams::Freq { freq_u16 } => freq_u16,
                _ => 30, // default 30-min letter buckets
            };
            Some(FoldParams { freq_minutes: freq_u16, ..Default::default() })
        }
        _ => None,
    }
}

/// Default [`TpoSource`] the trade-fold cutover installs for `"tpo"` — the
/// UI has no source selector today (the pre-cutover `subscribe_tpo` always
/// hardcoded `TpoSource::TradeBucket`), so this constant preserves that
/// exact behavior. `mlc-app` passes this through
/// `BackendCore::subscribe_trade_fold_chart_type`'s `tpo_source` parameter —
/// change this single constant (or thread a real per-bubble selector
/// through the call sites) to switch the default without touching the fold
/// machinery itself.
pub const DEFAULT_TPO_SOURCE: TpoSource = TpoSource::TradeBucket;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registry_id_has_ops() {
        for def in mlc_core::chart_type::all() {
            assert!(ops_by_id(def.id).is_some(), "no SubscribeOps for chart_type {}", def.id);
        }
    }

    #[test]
    fn ops_table_ids_unique() {
        let ids: Vec<&str> = OPS.iter().map(|(k, _)| *k).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len());
    }
}
