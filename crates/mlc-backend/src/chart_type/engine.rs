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

use crate::bubble::{BarsSource, DataBubble};

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

fn subscribe_range_bar(
    bridge: &DataBridge,
    eid: ExchangeId,
    symbol: &str,
    at: Dig3AccountType,
    _tf: &Timeframe,
    params: SubscribeParams,
) {
    let scaled = match params {
        SubscribeParams::ScaledUnits { scaled_u64 } => scaled_u64,
        _ => (10.0_f64 * 1e8) as u64, // default 10.0 units
    };
    bridge.request_range_bars(eid, symbol, at, scaled);
}

fn subscribe_volume_bar(
    bridge: &DataBridge,
    eid: ExchangeId,
    symbol: &str,
    at: Dig3AccountType,
    _tf: &Timeframe,
    params: SubscribeParams,
) {
    let scaled = match params {
        SubscribeParams::ScaledUnits { scaled_u64 } => scaled_u64,
        _ => (1000.0_f64 * 1e8) as u64, // default 1000.0 units
    };
    bridge.request_volume_bars(eid, symbol, at, scaled);
}

fn subscribe_tick_bar(
    bridge: &DataBridge,
    eid: ExchangeId,
    symbol: &str,
    at: Dig3AccountType,
    _tf: &Timeframe,
    params: SubscribeParams,
) {
    let count = match params {
        SubscribeParams::Count { count_u32 } => count_u32,
        _ => 100, // default 100 trades
    };
    bridge.request_tick_bars(eid, symbol, at, count);
}

fn subscribe_dollar_bar(
    bridge: &DataBridge,
    eid: ExchangeId,
    symbol: &str,
    at: Dig3AccountType,
    _tf: &Timeframe,
    params: SubscribeParams,
) {
    let threshold = match params {
        SubscribeParams::Threshold { threshold_u64 } => threshold_u64,
        _ => 1_000_000, // default $1,000,000
    };
    bridge.request_dollar_bars(eid, symbol, at, threshold);
}

fn subscribe_tick_imbalance_bar(
    bridge: &DataBridge,
    eid: ExchangeId,
    symbol: &str,
    at: Dig3AccountType,
    _tf: &Timeframe,
    params: SubscribeParams,
) {
    let (alpha_x100, min_ticks) = match params {
        SubscribeParams::MinTicks { alpha_x100, min_ticks } => (alpha_x100, min_ticks),
        _ => (20, 50), // default alpha=20 (0.20), min_ticks=50
    };
    bridge.request_tick_imbalance_bars(eid, symbol, at, alpha_x100, min_ticks);
}

fn subscribe_volume_imbalance_bar(
    bridge: &DataBridge,
    eid: ExchangeId,
    symbol: &str,
    at: Dig3AccountType,
    _tf: &Timeframe,
    params: SubscribeParams,
) {
    let (alpha_x100, min_ticks) = match params {
        SubscribeParams::MinTicks { alpha_x100, min_ticks } => (alpha_x100, min_ticks),
        _ => (20, 50), // default alpha=20 (0.20), min_ticks=50
    };
    bridge.request_volume_imbalance_bars(eid, symbol, at, alpha_x100, min_ticks);
}

fn subscribe_run_bar(
    bridge: &DataBridge,
    eid: ExchangeId,
    symbol: &str,
    at: Dig3AccountType,
    _tf: &Timeframe,
    params: SubscribeParams,
) {
    let (alpha_x100, min_ticks) = match params {
        SubscribeParams::MinTicks { alpha_x100, min_ticks } => (alpha_x100, min_ticks),
        _ => (20, 50), // default alpha=20 (0.20), min_ticks=50
    };
    bridge.request_run_bars(eid, symbol, at, alpha_x100, min_ticks);
}

fn subscribe_renko(
    bridge: &DataBridge,
    eid: ExchangeId,
    symbol: &str,
    at: Dig3AccountType,
    _tf: &Timeframe,
    params: SubscribeParams,
) {
    let (scaled, reversal) = match params {
        SubscribeParams::ScaledUnitsWithReversal { scaled_u64, reversal } => (scaled_u64, reversal),
        _ => ((10.0_f64 * 1e8) as u64, 2), // default 10.0 units, reversal=2
    };
    bridge.request_renko_bars(eid, symbol, at, scaled, reversal);
}

fn subscribe_pnf(
    bridge: &DataBridge,
    eid: ExchangeId,
    symbol: &str,
    at: Dig3AccountType,
    _tf: &Timeframe,
    params: SubscribeParams,
) {
    let (scaled, reversal) = match params {
        SubscribeParams::ScaledUnitsWithReversal { scaled_u64, reversal } => (scaled_u64, reversal),
        _ => ((10.0_f64 * 1e8) as u64, 3), // default 10.0 units, reversal=3
    };
    bridge.request_pnf_bars(eid, symbol, at, scaled, reversal);
}

fn subscribe_kagi(
    bridge: &DataBridge,
    eid: ExchangeId,
    symbol: &str,
    at: Dig3AccountType,
    _tf: &Timeframe,
    params: SubscribeParams,
) {
    let scaled = match params {
        SubscribeParams::ScaledUnits { scaled_u64 } => scaled_u64,
        _ => (10.0_f64 * 1e8) as u64, // default 10.0 units
    };
    bridge.request_kagi_bars(eid, symbol, at, scaled);
}

fn subscribe_three_line_break(
    bridge: &DataBridge,
    eid: ExchangeId,
    symbol: &str,
    at: Dig3AccountType,
    _tf: &Timeframe,
    params: SubscribeParams,
) {
    let lines_back = match params {
        SubscribeParams::LinesBack { lines_back } => lines_back,
        _ => 3, // default lookback=3
    };
    bridge.request_three_line_break(eid, symbol, at, lines_back);
}

fn subscribe_tpo(
    bridge: &DataBridge,
    eid: ExchangeId,
    symbol: &str,
    at: Dig3AccountType,
    _tf: &Timeframe,
    params: SubscribeParams,
) {
    let freq_u16 = match params {
        SubscribeParams::Freq { freq_u16 } => freq_u16,
        _ => 30, // default 30-min letter buckets
    };
    bridge.request_tpo(eid, symbol, at, freq_u16, TpoSource::TradeBucket);
}

/// Footprint's subscribe is NOT a bare wrapper — it's the network half of a
/// 5-step atomic op (see prior audit). Order: abort the OLD kline sub before
/// flipping to footprint OHLC, then request the footprint stream. The bubble
/// mutations (clear_bars / mark_footprint_pending / bars_source /
/// set_timeframe) live in `clear_slot_footprint` and the caller's
/// `set_timeframe` call — dispatch order is `clear_slot()` then
/// `subscribe()`, same as today's `ChangeChartType` footprint arm.
fn subscribe_footprint(
    bridge: &DataBridge,
    eid: ExchangeId,
    symbol: &str,
    at: Dig3AccountType,
    tf: &Timeframe,
    params: SubscribeParams,
) {
    let interval = match params {
        SubscribeParams::Interval { interval } => interval,
        _ => "1m",
    };
    bridge.abort_kline_bars(eid, symbol, at, &tf.name);
    bridge.request_footprint(eid, symbol, at, interval);
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

/// Render-family (10 types) and trade-agg-bar family (7 types) share this —
/// both write into `bubble.bars` and reset `bars_source` back to `Kline`.
fn clear_slot_kline_bars(bubble: &mut DataBubble) {
    bubble.clear_bars();
    bubble.bars_source = BarsSource::Kline;
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
/// (matches `ChangeChartType`'s footprint arm at chart_out_events.rs:657-661)
/// — `subscribe_footprint` only touches `DataBridge`.
fn clear_slot_footprint(bubble: &mut DataBubble) {
    bubble.clear_bars();
    bubble.mark_footprint_pending();
    bubble.bars_source = BarsSource::FootprintOhlc;
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

/// Footprint holds both a (possibly still-live) kline forwarder AND a
/// derived-subs slot — release both unconditionally.
fn abort_footprint(bridge: &DataBridge, eid: ExchangeId, symbol: &str, at: Dig3AccountType, tf: &Timeframe) {
    bridge.abort_kline_bars(eid, symbol, at, &tf.name);
    bridge.abort_derived_chart(eid, symbol, at);
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
    ("range_bar", SubscribeOps { subscribe: subscribe_range_bar, clear_slot: clear_slot_kline_bars, abort: abort_derived }),
    ("volume_bar", SubscribeOps { subscribe: subscribe_volume_bar, clear_slot: clear_slot_kline_bars, abort: abort_derived }),
    ("tick_bar", SubscribeOps { subscribe: subscribe_tick_bar, clear_slot: clear_slot_kline_bars, abort: abort_derived }),
    ("dollar_bar", SubscribeOps { subscribe: subscribe_dollar_bar, clear_slot: clear_slot_kline_bars, abort: abort_derived }),
    ("tick_imbalance_bar", SubscribeOps { subscribe: subscribe_tick_imbalance_bar, clear_slot: clear_slot_kline_bars, abort: abort_derived }),
    ("volume_imbalance_bar", SubscribeOps { subscribe: subscribe_volume_imbalance_bar, clear_slot: clear_slot_kline_bars, abort: abort_derived }),
    ("run_bar", SubscribeOps { subscribe: subscribe_run_bar, clear_slot: clear_slot_kline_bars, abort: abort_derived }),
    ("renko", SubscribeOps { subscribe: subscribe_renko, clear_slot: clear_slot_renko, abort: abort_derived }),
    ("pnf", SubscribeOps { subscribe: subscribe_pnf, clear_slot: clear_slot_pnf, abort: abort_derived }),
    ("kagi", SubscribeOps { subscribe: subscribe_kagi, clear_slot: clear_slot_kagi, abort: abort_derived }),
    ("three_line_break", SubscribeOps { subscribe: subscribe_three_line_break, clear_slot: clear_slot_three_line_break, abort: abort_derived }),
    ("footprint", SubscribeOps { subscribe: subscribe_footprint, clear_slot: clear_slot_footprint, abort: abort_footprint }),
    ("tpo", SubscribeOps { subscribe: subscribe_tpo, clear_slot: clear_slot_tpo, abort: abort_derived }),
    ("dom_heatmap", SubscribeOps { subscribe: subscribe_dom_heatmap, clear_slot: clear_slot_dom_heatmap, abort: abort_derived }),
    ("liquidation_heatmap", SubscribeOps { subscribe: subscribe_liq_heatmap, clear_slot: clear_slot_liq_heatmap, abort: abort_derived }),
];

/// Look up the `SubscribeOps` for a chart_type id. `None` for unknown ids.
pub fn ops_by_id(id: &str) -> Option<&'static SubscribeOps> {
    OPS.iter().find(|(k, _)| *k == id).map(|(_, v)| v)
}

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
