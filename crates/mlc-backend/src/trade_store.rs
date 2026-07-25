//! `TradeStore` — per-instrument shared raw trade store.
//!
//! `docs/mlc/plans/trade-data-plane-2026-07-13.md` §1.1, implementation
//! order step 1 ("TradeStore extraction"). PURE EXTRACTION of
//! `DataBubble.trade_layer` into one shared, per-instrument
//! `RwLock<HashMap<InstrumentAddress, RawLayer<RawTrade>>>` — no behavior
//! change (same dedup semantics, same ring capacity, same event flow). The
//! hot-time-horizon + cold-spill design in §1.1 is a LATER step; this brick
//! only moves the existing ring out from under every bubble into one shared
//! entry per instrument.
//!
//! Keyed by [`mlc_core::instrument::InstrumentAddress`] — the same
//! `(exchange, symbol, account_type)` identity `DataBubble.address` already
//! carries, and the same triplet `BubbleStore::bubbles_for_instrument`
//! resolves against. No new identity format.
//!
//! Owned by [`crate::BackendCore`] as a plain field beside `mini_ticker`
//! (rung-4 keyed exception, doctrine §6 in the crate root doc comment —
//! keyed state with independent per-key invariants, edge/data-plane only).
//!
//! **Day-window retention** (owner rule 2026-07-15): raw trades are kept no
//! deeper than `mlc_backend_data_bridge::trade_raw_window_ms()` (profile
//! `data_load.trade_raw_window_days`, default 4) behind the entry's OWN
//! newest stored timestamp — not wall clock: deterministic, replay-safe,
//! and honest during backfill (a batch of old trades prepended while the
//! live edge is fresh must not evict against a stale wall-clock read).
//! This window is THE single raw-trade memory bound. It replaced three
//! uncoordinated limits that wedged against each other in the 2026-07-15
//! live incident (engine's 1.5M cumulative point cap latching before the
//! store's 1M count ring, both blind to the demand-widened per-entry time
//! horizon): the demand-widened `horizon_ms`/`widen_horizon` machinery and
//! the count capacity are GONE. Depth beyond the window is fold-then-drop:
//! the fold consumers process `TradeHistoryPrepended` payloads from the
//! broadcast independently of store retention, so a zip day deeper than
//! the window still folds into clusters — its raw rows are simply evicted
//! by this same rule right after the write, and a later refold re-pulls
//! the immutable archive day instead of holding 21MB/day of raw forever.
//!
//! **Single-writer discipline**: writers are the `tick_writers` arms in
//! `crate::lib` (`TradeUpdate`'s live push, `TradeHistoryPrepended`'s
//! backfill prepend) — mirrors the bubble-store SWC contract. Readers are
//! the fold/reseed paths in `crate::chart_type::lifecycle` and the demand
//! dispatch in `mlc-app`'s `tick.rs` (via the narrow
//! [`crate::BackendCore::trade_oldest_ts`] facade method — `mlc-app` never
//! sees [`TradeStore`] itself).
//!
//! **wasm lock discipline**: every method below acquires the lock, does its
//! work, and drops the guard before returning — no method ever returns a
//! guard or a borrow tied to one. Callers must never call a
//! `bubbles()`/`bubbles_mut()` facade method while holding a `TradeStore`
//! read/write borrow, and must never read `TradeStore` from inside a
//! `with_bubble`/`with_bubble_mut` closure — bind `TradeStore` reads BEFORE
//! entering such a closure, exactly like the existing bubble-read-before-
//! bubble-write shape in `tick_writers`.
//!
//! **Instrument-switch semantics (v1, documented choice)**: unlike
//! `DataBubble::reset_raw_layers` (which used to clear a bubble's OWN
//! `trade_layer` on a genuine symbol/exchange/account_type reassignment),
//! [`TradeStore`] entries are NEVER cleared on a bubble's instrument switch.
//! The store is per-INSTRUMENT, not per-bubble — the departing bubble simply
//! stops reading that entry, and the data stays valid for any OTHER bubble
//! still tracking the same instrument. The day window bounds memory.

use std::collections::HashMap;
use std::sync::RwLock;

use mlc_core::instrument::InstrumentAddress;

use crate::bubble::{RawLayer, RawTrade};

/// One per-instrument entry: the raw trade ring, unbounded in count
/// (capacity `0`) — the day-window eviction in [`StoreEntry::evict_expired`]
/// is the memory bound (see module doc).
struct StoreEntry {
    layer: RawLayer<RawTrade>,
}

impl StoreEntry {
    fn new() -> Self {
        Self { layer: RawLayer::new(0) }
    }

    /// Evict trades older than the raw-trade day window (module doc),
    /// measured from this entry's OWN newest stored timestamp (not wall
    /// clock — see module doc). A no-op on an empty layer (nothing to
    /// measure "now" from yet).
    fn evict_expired(&mut self) {
        if let Some(newest) = self.layer.newest_ts() {
            self.layer.evict_older_than(newest - mlc_backend_data_bridge::trade_raw_window_ms());
        }
    }
}

/// Per-instrument shared raw trade store. See module doc.
pub struct TradeStore {
    inner: RwLock<HashMap<InstrumentAddress, StoreEntry>>,
}

impl TradeStore {
    /// Empty store — no instruments touched yet.
    pub fn new() -> Self {
        Self { inner: RwLock::new(HashMap::new()) }
    }

    /// Backfill-batch write: prepend `trades` in front of the existing ring
    /// for `key`, seam-deduped exactly like [`RawLayer::prepend_batch`].
    /// After the write, evicts anything older than the raw-trade day window
    /// (measured from the entry's own newest ts, see module doc) — a batch
    /// entirely below the window is therefore fold-then-drop by
    /// construction: the fold consumers already received it from the
    /// broadcast; the store simply declines to retain the raw rows.
    pub fn prepend_batch(&self, key: &InstrumentAddress, trades: &[RawTrade]) {
        if trades.is_empty() {
            return;
        }
        let mut guard = self.inner.write().expect("TradeStore poisoned (write)");
        let entry = guard.entry(key.clone()).or_insert_with(StoreEntry::new);
        entry.layer.prepend_batch(trades.to_vec());
        entry.evict_expired();
    }

    /// Live-tick write: append `trade`, or in-place replace the current last
    /// entry if its chronological key matches (see [`RawLayer::push_live`]).
    /// After the write, evicts anything older than the raw-trade day window
    /// (measured from the entry's own newest ts, see module doc).
    pub fn push_live(&self, key: &InstrumentAddress, trade: RawTrade) {
        let mut guard = self.inner.write().expect("TradeStore poisoned (write)");
        let entry = guard.entry(key.clone()).or_insert_with(StoreEntry::new);
        entry.layer.push_live(trade);
        entry.evict_expired();
    }

    /// Oldest stored trade's timestamp for `key`, if the instrument has ever
    /// been touched and the ring is non-empty.
    pub fn oldest_ts(&self, key: &InstrumentAddress) -> Option<i64> {
        let guard = self.inner.read().expect("TradeStore poisoned (read)");
        guard.get(key).and_then(|e| e.layer.oldest_ts())
    }

    /// Number of trades currently stored for `key`. `0` if the instrument
    /// has never been touched.
    pub fn len(&self, key: &InstrumentAddress) -> usize {
        let guard = self.inner.read().expect("TradeStore poisoned (read)");
        guard.get(key).map(|e| e.layer.len()).unwrap_or(0)
    }

    /// `true` if `key` has no stored trades (including "never touched").
    pub fn is_empty(&self, key: &InstrumentAddress) -> bool {
        self.len(key) == 0
    }

    /// Total stored trades across every instrument + instrument count —
    /// memory diagnostics (wasm-leak differential, 2026-07-26).
    pub fn totals(&self) -> (usize, usize) {
        let guard = self.inner.read().expect("TradeStore poisoned (read)");
        let total = guard.values().map(|e| e.layer.len()).sum();
        (total, guard.len())
    }

    /// Copy of every stored trade for `key` with `ts_from <= ts_ms < ts_to`,
    /// oldest-first. Used by seam-bucket refolds (small windows — a bounded
    /// linear scan over the ring is cheap at that scale).
    pub fn slice_window(&self, key: &InstrumentAddress, ts_from: i64, ts_to: i64) -> Vec<RawTrade> {
        let guard = self.inner.read().expect("TradeStore poisoned (read)");
        guard.get(key)
            .map(|e| e.layer.iter().filter(|t| t.ts_ms >= ts_from && t.ts_ms < ts_to).copied().collect())
            .unwrap_or_default()
    }

    /// Full chronological copy of the stored ring for `key`, oldest-first.
    /// Empty if the instrument has never been touched. Used by the rare
    /// whole-window reads: exhausted-reseed / param-change refold / initial
    /// fold-install reseed.
    pub fn snapshot_all(&self, key: &InstrumentAddress) -> Vec<RawTrade> {
        let guard = self.inner.read().expect("TradeStore poisoned (read)");
        guard.get(key).map(|e| e.layer.snapshot()).unwrap_or_default()
    }

    /// [`RawLayer`]-shaped full snapshot for `key` — same data as
    /// [`Self::snapshot_all`], but as a fresh `RawLayer` (capacity `0`,
    /// unbounded) for call sites that pass the snapshot straight into
    /// [`crate::bubble::TradeFoldRuntime::reseed`], which takes
    /// `&RawLayer<RawTrade>`. Empty (capacity-0) layer if the instrument has
    /// never been touched.
    pub fn snapshot_layer(&self, key: &InstrumentAddress) -> RawLayer<RawTrade> {
        let guard = self.inner.read().expect("TradeStore poisoned (read)");
        match guard.get(key) {
            Some(e) => e.layer.clone(),
            None => RawLayer::new(0),
        }
    }
}

impl Default for TradeStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr() -> InstrumentAddress {
        InstrumentAddress::new("binance", "BTCUSDT", mlc_core::instrument::AccountType::Spot)
    }

    fn trade(ts_ms: i64, price: f64) -> RawTrade {
        RawTrade { ts_ms, price, qty: 1.0, is_buyer_maker: false }
    }

    #[test]
    fn fresh_store_reads_empty_for_untouched_instrument() {
        let store = TradeStore::new();
        let key = addr();
        assert_eq!(store.len(&key), 0);
        assert!(store.is_empty(&key));
        assert_eq!(store.oldest_ts(&key), None);
        assert!(store.snapshot_all(&key).is_empty());
    }

    #[test]
    fn push_live_creates_entry_and_grows() {
        let store = TradeStore::new();
        let key = addr();
        store.push_live(&key, trade(100, 1.0));
        store.push_live(&key, trade(200, 2.0));
        assert_eq!(store.len(&key), 2);
        assert_eq!(store.oldest_ts(&key), Some(100));
        assert_eq!(store.snapshot_all(&key), vec![trade(100, 1.0), trade(200, 2.0)]);
    }

    #[test]
    fn prepend_batch_seam_dedups_like_raw_layer() {
        let store = TradeStore::new();
        let key = addr();
        store.push_live(&key, trade(300, 3.0));
        store.push_live(&key, trade(400, 4.0));
        store.prepend_batch(&key, &[trade(300, 3.0), trade(200, 2.0), trade(100, 1.0)]);
        assert_eq!(store.len(&key), 4);
        assert_eq!(
            store.snapshot_all(&key),
            vec![trade(100, 1.0), trade(200, 2.0), trade(300, 3.0), trade(400, 4.0)]
        );
    }

    #[test]
    fn different_instruments_are_isolated() {
        let store = TradeStore::new();
        let btc = addr();
        let eth = InstrumentAddress::new("binance", "ETHUSDT", mlc_core::instrument::AccountType::Spot);
        store.push_live(&btc, trade(100, 1.0));
        assert_eq!(store.len(&btc), 1);
        assert_eq!(store.len(&eth), 0);
    }

    #[test]
    fn slice_window_returns_only_points_in_range() {
        let store = TradeStore::new();
        let key = addr();
        for ts in [100, 200, 300, 400, 500] {
            store.push_live(&key, trade(ts, ts as f64));
        }
        let window = store.slice_window(&key, 200, 400);
        assert_eq!(window, vec![trade(200, 200.0), trade(300, 300.0)]);
    }

    #[test]
    fn snapshot_layer_is_usable_as_reseed_input() {
        let store = TradeStore::new();
        let key = addr();
        store.push_live(&key, trade(100, 1.0));
        store.push_live(&key, trade(200, 2.0));
        let layer = store.snapshot_layer(&key);
        assert_eq!(layer.len(), 2);
        assert_eq!(layer.oldest_ts(), Some(100));
    }

    /// The raw-trade day window as the tests see it — the bridge global's
    /// untouched default (4 days; see module doc). Tests use relative
    /// timestamps against this value and never mutate the global (it is
    /// process-wide — mutating it would race parallel tests).
    const WINDOW_MS: i64 = 4 * 86_400_000;

    #[test]
    fn push_live_evicts_trades_older_than_the_day_window() {
        let store = TradeStore::new();
        let key = addr();

        store.push_live(&key, trade(0, 1.0));
        store.push_live(&key, trade(WINDOW_MS / 2, 2.0));
        // Newest so far is half the window -> cutoff is negative, nothing
        // expired yet.
        assert_eq!(store.len(&key), 2);

        // Newest becomes 2x the window -> cutoff = 2w - w = w; both earlier
        // points (0 and w/2) are older than the cutoff and must be evicted.
        store.push_live(&key, trade(WINDOW_MS * 2, 3.0));
        assert_eq!(store.len(&key), 1);
        assert_eq!(store.oldest_ts(&key), Some(WINDOW_MS * 2));
    }

    #[test]
    fn prepend_batch_below_the_window_is_fold_then_drop() {
        // A backfill batch entirely below the day window lands (the fold
        // consumers get it from the broadcast) but the store declines to
        // RETAIN the raw rows — fold-then-drop by construction (module doc).
        let store = TradeStore::new();
        let key = addr();
        let live_ts = WINDOW_MS * 10;
        store.push_live(&key, trade(live_ts, 5.0));

        store.prepend_batch(&key, &[trade(100, 1.0), trade(200, 2.0), trade(300, 3.0)]);
        assert_eq!(store.len(&key), 1);
        assert_eq!(store.oldest_ts(&key), Some(live_ts));
    }

    #[test]
    fn prepend_batch_keeps_points_inside_the_window() {
        let store = TradeStore::new();
        let key = addr();
        let live_ts = WINDOW_MS * 10;
        store.push_live(&key, trade(live_ts, 5.0));

        // This page sits just inside the window (cutoff = live_ts - window),
        // so it survives eviction — a whole archive day within the window is
        // retained for refolds.
        let inside_ts = live_ts - WINDOW_MS + 1;
        store.prepend_batch(&key, &[trade(inside_ts, 1.0)]);
        assert_eq!(store.len(&key), 2);
        assert_eq!(store.oldest_ts(&key), Some(inside_ts));
    }

    #[test]
    fn no_count_capacity_clip_within_the_window() {
        // The count ring is GONE (owner rule 2026-07-15) — everything
        // inside the day window is retained regardless of count.
        let store = TradeStore::new();
        let key = addr();
        for ts in 0..2_000i64 {
            store.push_live(&key, trade(ts, ts as f64));
        }
        assert_eq!(store.len(&key), 2_000, "no count clip inside the window");
        assert_eq!(store.oldest_ts(&key), Some(0));
    }
}
