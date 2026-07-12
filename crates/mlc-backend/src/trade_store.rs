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
//! still tracking the same instrument. Ring capacity bounds memory the same
//! way it always did. A future step (§1.1's time-horizon + cold-spill work)
//! may add real eviction/expiry; this extraction does not.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;

use mlc_core::instrument::InstrumentAddress;

use crate::bubble::{RawLayer, RawTrade, DEFAULT_TRADE_LAYER_CAPACITY};

/// Per-instrument shared raw trade ring. See module doc.
pub struct TradeStore {
    inner: RwLock<HashMap<InstrumentAddress, RawLayer<RawTrade>>>,
    /// Ring capacity applied to every entry — same role
    /// `DataBubble.trade_layer_capacity` played before this extraction, now
    /// a single store-wide default instead of one value per bubble (every
    /// call site already sourced the same
    /// `UserProfile.domain.data_load.trade_layer_capacity` value for every
    /// bubble it touched, so this is not a behavior change). `AtomicUsize`
    /// so [`Self::set_default_capacity`] can update it without taking the
    /// `inner` write lock for a value-only read elsewhere.
    default_capacity: AtomicUsize,
}

impl TradeStore {
    /// Empty store — no instruments touched yet. Default capacity starts at
    /// [`DEFAULT_TRADE_LAYER_CAPACITY`], same as the old per-bubble default.
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            default_capacity: AtomicUsize::new(DEFAULT_TRADE_LAYER_CAPACITY),
        }
    }

    /// Configure the ring capacity applied to every instrument entry —
    /// mirrors [`crate::bubble::DataBubble::set_layer_capacities`]'s old
    /// `trade_capacity` argument, now store-wide instead of per-bubble
    /// (every caller already passed the same profile-sourced value for
    /// every bubble). Applies immediately to every already-existing entry
    /// (same "set_capacity evicts overflow immediately" behavior
    /// [`RawLayer::set_capacity`] already had) and becomes the capacity new
    /// entries are created with from then on.
    pub fn set_default_capacity(&self, capacity: usize) {
        self.default_capacity.store(capacity, Ordering::Relaxed);
        let mut guard = self.inner.write().expect("TradeStore poisoned (write)");
        for layer in guard.values_mut() {
            layer.set_capacity(capacity);
        }
    }

    /// Backfill-batch write: prepend `trades` in front of the existing ring
    /// for `key`, seam-deduped exactly like [`RawLayer::prepend_batch`].
    /// Creates the entry on first touch at the current default capacity
    /// (see [`Self::set_default_capacity`]).
    pub fn prepend_batch(&self, key: &InstrumentAddress, trades: &[RawTrade]) {
        if trades.is_empty() {
            return;
        }
        let capacity = self.default_capacity.load(Ordering::Relaxed);
        let mut guard = self.inner.write().expect("TradeStore poisoned (write)");
        let layer = guard.entry(key.clone()).or_insert_with(|| RawLayer::new(capacity));
        layer.prepend_batch(trades.to_vec());
    }

    /// Live-tick write: append `trade`, or in-place replace the current last
    /// entry if its chronological key matches (see
    /// [`RawLayer::push_live`]). Creates the entry on first touch at the
    /// current default capacity (see [`Self::set_default_capacity`]).
    pub fn push_live(&self, key: &InstrumentAddress, trade: RawTrade) {
        let capacity = self.default_capacity.load(Ordering::Relaxed);
        let mut guard = self.inner.write().expect("TradeStore poisoned (write)");
        let layer = guard.entry(key.clone()).or_insert_with(|| RawLayer::new(capacity));
        layer.push_live(trade);
    }

    /// Oldest stored trade's timestamp for `key`, if the instrument has ever
    /// been touched and the ring is non-empty.
    pub fn oldest_ts(&self, key: &InstrumentAddress) -> Option<i64> {
        let guard = self.inner.read().expect("TradeStore poisoned (read)");
        guard.get(key).and_then(RawLayer::oldest_ts)
    }

    /// Number of trades currently stored for `key`. `0` if the instrument
    /// has never been touched.
    pub fn len(&self, key: &InstrumentAddress) -> usize {
        let guard = self.inner.read().expect("TradeStore poisoned (read)");
        guard.get(key).map(RawLayer::len).unwrap_or(0)
    }

    /// `true` if `key` has no stored trades (including "never touched").
    pub fn is_empty(&self, key: &InstrumentAddress) -> bool {
        self.len(key) == 0
    }

    /// Copy of every stored trade for `key` with `ts_from <= ts_ms < ts_to`,
    /// oldest-first. Used by seam-bucket refolds (small windows — a bounded
    /// linear scan over the ring is cheap at that scale).
    pub fn slice_window(&self, key: &InstrumentAddress, ts_from: i64, ts_to: i64) -> Vec<RawTrade> {
        let guard = self.inner.read().expect("TradeStore poisoned (read)");
        guard.get(key)
            .map(|l| l.iter().filter(|t| t.ts_ms >= ts_from && t.ts_ms < ts_to).copied().collect())
            .unwrap_or_default()
    }

    /// Full chronological copy of the stored ring for `key`, oldest-first.
    /// Empty if the instrument has never been touched. Used by the rare
    /// whole-window reads: exhausted-reseed / param-change refold / initial
    /// fold-install reseed.
    pub fn snapshot_all(&self, key: &InstrumentAddress) -> Vec<RawTrade> {
        let guard = self.inner.read().expect("TradeStore poisoned (read)");
        guard.get(key).map(RawLayer::snapshot).unwrap_or_default()
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
            Some(l) => l.clone(),
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

    #[test]
    fn capacity_eviction_still_applies_via_the_store() {
        let store = TradeStore::new();
        store.set_default_capacity(3);
        let key = addr();
        for ts in [100, 200, 300, 400] {
            store.push_live(&key, trade(ts, ts as f64));
        }
        assert_eq!(store.len(&key), 3);
        assert_eq!(store.oldest_ts(&key), Some(200), "oldest must be evicted, not newest");
    }

    #[test]
    fn set_default_capacity_evicts_existing_entries_immediately() {
        let store = TradeStore::new();
        let key = addr();
        for ts in [100, 200, 300, 400] {
            store.push_live(&key, trade(ts, ts as f64));
        }
        assert_eq!(store.len(&key), 4);
        store.set_default_capacity(2);
        assert_eq!(store.len(&key), 2, "shrinking the default capacity must evict overflow immediately");
        assert_eq!(store.oldest_ts(&key), Some(300));
    }
}
