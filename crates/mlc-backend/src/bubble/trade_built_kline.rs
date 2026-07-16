//! `TradeBuiltKlineRuntime` — per-bubble client-side fold that builds a
//! TRADE-BUILT kline timeframe entirely from the shared per-instrument
//! `TradeStore`, for target TFs the active venue cannot serve natively and
//! cannot aggregate from any native divisor (owner report 2026-07-16:
//! Bybit 15s silently fell back to hourly bars mislabeled "15s" — see
//! `mlc_backend_data_bridge::kline_agg::is_trade_built`'s doc comment for
//! the exact classification).
//!
//! Structurally this mirrors [`super::view_fold::FoldRuntime`] /
//! [`super::view_fold::TradeFoldRuntime`] (install → reseed → live-fold),
//! but the fold itself is the pure, stateless
//! [`mlc_backend_data_bridge::fold_trades_into_bars`] bucket fold rather
//! than a `digdigdig3_station` derived-stream machine — there is no
//! incremental machine state to carry between calls, so [`Self::reseed`]
//! and [`Self::on_live_trade`] both just re-run the fold over a widened
//! trade slice and diff the result against what was last emitted.
//!
//! Output lands in the SAME places an ordinary native kline subscription
//! would write: `bubble.kline_layers[tf_name]` (the raw layer every
//! viewport/reseed path already reads) and, when the bubble's active
//! `chart_type` is `ChartTypeKind::RenderOnly` (candles/bars/line/... —
//! the pure kline-rendered family) on this exact `tf_name`, `bubble.bars`
//! (the display array) — mirroring the ownership gate `tick_writers`'
//! `BarsLoaded`/`BarUpdate` arms already enforce for `BarsSource::Kline`.

use mlc_backend_data_bridge::RawTradeWire;
use mlc_core::Bar;

use super::raw_layer::RawTrade;

fn raw_trade_to_wire(t: &RawTrade) -> RawTradeWire {
    RawTradeWire { ts_ms: t.ts_ms, price: t.price, qty: t.qty, is_buyer_maker: t.is_buyer_maker }
}

/// Per-bubble TRADE-BUILT kline fold state. One instance per (bubble,
/// active TF) — installed when [`mlc_backend_data_bridge::is_trade_built`]
/// (via the bridge facade) says the bubble's active timeframe has no
/// native/aggregated source, dropped on any TF/chart-type/instrument
/// switch away from that state (same lifecycle as
/// [`super::view_fold::FoldRuntime`] — see [`super::data_bubble::DataBubble::active_fold`]'s
/// doc comment).
#[derive(Clone, Debug)]
pub struct TradeBuiltKlineRuntime {
    /// Target timeframe name (e.g. `"15s"`, `"1s"`) — the kline-layer key
    /// this runtime writes into.
    tf_name: String,
    /// Target bucket duration in ms.
    target_ms: i64,
    /// Last folded output, oldest-first — kept so [`Self::on_live_trade`]
    /// can diff against it (in-place update vs append) without re-running
    /// the full fold's allocation for every single trade tick.
    last_bars: Vec<Bar>,
}

impl TradeBuiltKlineRuntime {
    /// Install a fresh runtime for `tf_name`/`target_ms`. Does not fold
    /// anything yet — call [`Self::reseed`] with the current `TradeStore`
    /// snapshot right after construction (mirrors
    /// `TradeFoldRuntime::new` + immediate `reseed` in
    /// `chart_type/lifecycle.rs`).
    pub fn new(tf_name: String, target_ms: i64) -> Self {
        Self { tf_name, target_ms, last_bars: Vec::new() }
    }

    /// Target timeframe name this runtime folds into.
    pub fn tf_name(&self) -> &str {
        &self.tf_name
    }

    /// Target bucket duration in ms.
    pub fn target_ms(&self) -> i64 {
        self.target_ms
    }

    /// Fold a full trade-store snapshot (oldest-first) into target-TF
    /// buckets, replacing [`Self::last_bars`] wholesale. Returns the fresh
    /// bar series — the caller writes it into `kline_layers[tf_name]`
    /// (`seed_block`, mirroring a fresh Phase-A page) and, when
    /// display-writable, `bubble.bars`.
    ///
    /// Used for: cold install, and DEEPENING (`LayerBackfilled`
    /// layer="trades" / `TradeGapFilled` — a full reseed over the widened
    /// store snapshot is v1-acceptable per the task contract: trades in
    /// the store are bounded by the existing 4-day raw window, so this is
    /// never an unbounded replay).
    pub fn reseed(&mut self, trades: &[RawTrade]) -> Vec<Bar> {
        let wire: Vec<RawTradeWire> = trades.iter().map(raw_trade_to_wire).collect();
        let bars = mlc_backend_data_bridge::fold_trades_into_bars(&wire, self.target_ms);
        self.last_bars = bars.clone();
        bars
    }

    /// Fold one live trade into the current (possibly in-progress) bucket.
    ///
    /// Returns `Some(bar)` — the single bar the live bucket now holds
    /// (either the mutated in-progress last bar, or a freshly opened one
    /// on a bucket-boundary cross) — for the caller to upsert into
    /// `kline_layers[tf_name]` (`push_live`, same call the ordinary kline
    /// `BarUpdate` writer uses) and, when display-writable, `bubble.bars`.
    /// Never returns `None` — every live trade produces or updates exactly
    /// one bar.
    pub fn on_live_trade(&mut self, trade: RawTrade) -> Bar {
        let bucket_start = mlc_backend_data_bridge::bucket_start_ms(trade.ts_ms, self.target_ms);
        match self.last_bars.last_mut() {
            Some(last) if last.derived_ts_ms == bucket_start => {
                last.high = last.high.max(trade.price);
                last.low = last.low.min(trade.price);
                last.close = trade.price;
                last.volume += trade.qty;
                *last
            }
            _ => {
                let fresh = Bar {
                    timestamp: bucket_start / 1000,
                    open: trade.price,
                    high: trade.price,
                    low: trade.price,
                    close: trade.price,
                    volume: trade.qty,
                    derived_ts_ms: bucket_start,
                };
                self.last_bars.push(fresh);
                fresh
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trade(ts_ms: i64, price: f64, qty: f64) -> RawTrade {
        RawTrade { ts_ms, price, qty, is_buyer_maker: false }
    }

    #[test]
    fn reseed_folds_the_whole_snapshot() {
        let mut rt = TradeBuiltKlineRuntime::new("15s".to_string(), 15_000);
        let trades = vec![trade(0, 100.0, 1.0), trade(5_000, 101.0, 2.0), trade(16_000, 102.0, 1.0)];
        let bars = rt.reseed(&trades);
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].derived_ts_ms, 0);
        assert_eq!(bars[1].derived_ts_ms, 15_000);
    }

    #[test]
    fn live_trade_in_same_bucket_updates_last_bar_in_place() {
        let mut rt = TradeBuiltKlineRuntime::new("15s".to_string(), 15_000);
        rt.reseed(&[trade(0, 100.0, 1.0)]);
        let updated = rt.on_live_trade(trade(5_000, 105.0, 2.0));
        assert_eq!(updated.derived_ts_ms, 0);
        assert_eq!(updated.close, 105.0);
        assert_eq!(updated.high, 105.0);
        assert_eq!(updated.volume, 3.0);
        assert_eq!(rt.last_bars.len(), 1, "same-bucket trade must not append a new bar");
    }

    #[test]
    fn live_trade_crossing_boundary_appends_a_new_bar() {
        let mut rt = TradeBuiltKlineRuntime::new("15s".to_string(), 15_000);
        rt.reseed(&[trade(0, 100.0, 1.0)]);
        let next = rt.on_live_trade(trade(15_500, 200.0, 1.0));
        assert_eq!(next.derived_ts_ms, 15_000);
        assert_eq!(rt.last_bars.len(), 2);
        assert_eq!(rt.last_bars[0].derived_ts_ms, 0, "prior bucket stays untouched");
    }

    #[test]
    fn live_trade_on_empty_runtime_opens_first_bucket() {
        let mut rt = TradeBuiltKlineRuntime::new("1s".to_string(), 1_000);
        let bar = rt.on_live_trade(trade(2_500, 50.0, 0.5));
        assert_eq!(bar.derived_ts_ms, 2_000);
        assert_eq!(bar.open, 50.0);
        assert_eq!(bar.volume, 0.5);
    }

    #[test]
    fn reseed_after_live_trades_replaces_state_wholesale() {
        let mut rt = TradeBuiltKlineRuntime::new("15s".to_string(), 15_000);
        rt.reseed(&[trade(0, 100.0, 1.0)]);
        rt.on_live_trade(trade(5_000, 999.0, 5.0));
        // A deepening reseed with an entirely different (wider) snapshot
        // must wholesale-replace, not merge with the stale live state.
        let bars = rt.reseed(&[trade(0, 10.0, 1.0), trade(15_000, 20.0, 1.0)]);
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].close, 10.0, "old live-mutated value must not leak through reseed");
    }
}
