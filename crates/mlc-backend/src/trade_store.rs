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
//! **Time-horizon retention** (`docs/mlc/plans/trade-data-plane-2026-07-13.md`
//! §1.1, implementation order step 2a — owner doctrine: retention is
//! expressed in TIME, never in trade counts). Every entry carries its own
//! `horizon_ms`, defaulted to [`DEFAULT_TRADE_HORIZON_MS`] on first touch and
//! widened (monotonic max — never shrunk mid-session) by
//! [`TradeStore::widen_horizon`] whenever demand needs a deeper window.
//! After every write, trades older than `newest_ts_in_entry - horizon_ms`
//! are evicted from the front. `newest_ts_in_entry` — not wall clock — is
//! the "now" eviction is measured from: deterministic, replay-safe, and
//! keeps the store honest during backfill (a batch of old trades prepended
//! while the ring's live edge is fresh must not evict against a stale
//! wall-clock read taken before the write). The ring's count CAPACITY
//! (`default_capacity`, ring semantics from step 1) still exists as a
//! silent-no-more memory backstop — it may still clip in pathological
//! bursts, but a clip is now DETECTED (length before/after the write) and
//! logged (`mlc_backend_data_bridge::dlog!`), never silent.
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

/// Default retention horizon applied to a [`TradeStore`] entry the first
/// time it is touched (push_live or prepend_batch). 6 hours covers the
/// deepest cold-fill any 1m-family viewport can ask for at boot (the
/// subscribe-time warms in `chart_type::lifecycle` top out at a 1h/24h
/// ceiling ×1.3 margin — see [`TradeStore::widen_horizon`]'s callers); every
/// real demand beyond that widens the horizon monotonically from there, so
/// this default is a floor, not a cap.
pub const DEFAULT_TRADE_HORIZON_MS: i64 = 6 * 3_600_000;

/// One per-instrument entry: the raw trade ring plus its own retention
/// horizon. Wrapped (rather than a parallel `HashMap<InstrumentAddress,
/// i64>`) so a single `inner` write-lock scope always sees both the layer
/// and the horizon it must evict against — no risk of the two maps
/// disagreeing on which keys exist.
struct StoreEntry {
    layer: RawLayer<RawTrade>,
    horizon_ms: i64,
}

impl StoreEntry {
    fn new(capacity: usize) -> Self {
        Self { layer: RawLayer::new(capacity), horizon_ms: DEFAULT_TRADE_HORIZON_MS }
    }

    /// Evict trades older than the horizon, measured from this entry's OWN
    /// newest stored timestamp (not wall clock — see module doc). A no-op
    /// on an empty layer (nothing to measure "now" from yet).
    fn evict_expired(&mut self) {
        if let Some(newest) = self.layer.newest_ts() {
            self.layer.evict_older_than(newest - self.horizon_ms);
        }
    }
}

/// Per-instrument shared raw trade ring. See module doc.
pub struct TradeStore {
    inner: RwLock<HashMap<InstrumentAddress, StoreEntry>>,
    /// Ring capacity applied to every entry — same role
    /// `DataBubble.trade_layer_capacity` played before this extraction, now
    /// a single store-wide default instead of one value per bubble (every
    /// call site already sourced the same
    /// `UserProfile.domain.data_load.trade_layer_capacity` value for every
    /// bubble it touched, so this is not a behavior change). `AtomicUsize`
    /// so [`Self::set_default_capacity`] can update it without taking the
    /// `inner` write lock for a value-only read elsewhere. This is a
    /// silent-no-more memory BACKSTOP behind the time horizon above — under
    /// normal operation the horizon evicts first; this only clips if a
    /// pathological burst outruns it, and such a clip is logged (see
    /// [`Self::log_capacity_clip`]).
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
        for entry in guard.values_mut() {
            entry.layer.set_capacity(capacity);
        }
    }

    /// Widen `key`'s retention horizon to at least `horizon_ms` — monotonic
    /// max, never shrunk (a session's horizon only grows; shrinking it
    /// mid-session would re-lose data a caller already relies on being
    /// retained). Creates the entry on first touch at the current default
    /// capacity and [`DEFAULT_TRADE_HORIZON_MS`], then widens from there if
    /// `horizon_ms` is deeper. A no-op write-lock round-trip if `horizon_ms`
    /// does not exceed what the entry already has.
    pub fn widen_horizon(&self, key: &InstrumentAddress, horizon_ms: i64) {
        let capacity = self.default_capacity.load(Ordering::Relaxed);
        let mut guard = self.inner.write().expect("TradeStore poisoned (write)");
        let entry = guard.entry(key.clone()).or_insert_with(|| StoreEntry::new(capacity));
        entry.horizon_ms = entry.horizon_ms.max(horizon_ms);
    }

    /// Backfill-batch write: prepend `trades` in front of the existing ring
    /// for `key`, seam-deduped exactly like [`RawLayer::prepend_batch`].
    /// Creates the entry on first touch at the current default capacity
    /// (see [`Self::set_default_capacity`]) and [`DEFAULT_TRADE_HORIZON_MS`].
    /// After the write, evicts anything older than the entry's horizon
    /// (measured from the entry's own newest ts, see module doc), then logs
    /// if the count-capacity backstop also clipped this write.
    pub fn prepend_batch(&self, key: &InstrumentAddress, trades: &[RawTrade]) {
        if trades.is_empty() {
            return;
        }
        let capacity = self.default_capacity.load(Ordering::Relaxed);
        let mut guard = self.inner.write().expect("TradeStore poisoned (write)");
        let entry = guard.entry(key.clone()).or_insert_with(|| StoreEntry::new(capacity));
        let len_before = entry.layer.len();
        let incoming = trades.len();
        entry.layer.prepend_batch(trades.to_vec());
        let len_after_write = entry.layer.len();
        entry.evict_expired();
        Self::log_capacity_clip(key, "prepend_batch", len_before, incoming, len_after_write, entry.horizon_ms);
    }

    /// Live-tick write: append `trade`, or in-place replace the current last
    /// entry if its chronological key matches (see
    /// [`RawLayer::push_live`]). Creates the entry on first touch at the
    /// current default capacity (see [`Self::set_default_capacity`]) and
    /// [`DEFAULT_TRADE_HORIZON_MS`]. After the write, evicts anything older
    /// than the entry's horizon (measured from the entry's own newest ts,
    /// see module doc), then logs if the count-capacity backstop also
    /// clipped this write.
    pub fn push_live(&self, key: &InstrumentAddress, trade: RawTrade) {
        let capacity = self.default_capacity.load(Ordering::Relaxed);
        let mut guard = self.inner.write().expect("TradeStore poisoned (write)");
        let entry = guard.entry(key.clone()).or_insert_with(|| StoreEntry::new(capacity));
        let len_before = entry.layer.len();
        entry.layer.push_live(trade);
        let len_after_write = entry.layer.len();
        entry.evict_expired();
        Self::log_capacity_clip(key, "push_live", len_before, 1, len_after_write, entry.horizon_ms);
    }

    /// Detect and log a count-capacity clip: the ring grew by fewer points
    /// than were offered (`incoming`), which can only happen because
    /// `default_capacity` truncated the overflow — the time horizon alone
    /// never explains fewer NEW points landing (it only removes stale ones,
    /// tracked separately). The backstop must never clip silently (owner
    /// doctrine); this is the single choke point both writers funnel
    /// through so the log fires exactly where the clip happens.
    fn log_capacity_clip(
        key: &InstrumentAddress,
        writer: &str,
        len_before: usize,
        incoming: usize,
        len_after_write: usize,
        horizon_ms: i64,
    ) {
        let grew_by = len_after_write.saturating_sub(len_before);
        if grew_by < incoming {
            let clipped = incoming - grew_by;
            mlc_backend_data_bridge::dlog!(
                "[trade_store] ring cap clipped {clipped} trades for {key:?} via {writer} — memory backstop, horizon={}h",
                horizon_ms / 3_600_000
            );
        }
    }

    /// Oldest stored trade's timestamp for `key`, if the instrument has ever
    /// been touched and the ring is non-empty.
    pub fn oldest_ts(&self, key: &InstrumentAddress) -> Option<i64> {
        let guard = self.inner.read().expect("TradeStore poisoned (read)");
        guard.get(key).and_then(|e| e.layer.oldest_ts())
    }

    /// Current retention horizon for `key`, if the instrument has ever been
    /// touched. `None` for an untouched instrument (no entry exists yet to
    /// report a horizon for).
    pub fn horizon_ms(&self, key: &InstrumentAddress) -> Option<i64> {
        let guard = self.inner.read().expect("TradeStore poisoned (read)");
        guard.get(key).map(|e| e.horizon_ms)
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

    #[test]
    fn first_touch_applies_the_default_horizon() {
        let store = TradeStore::new();
        let key = addr();
        store.push_live(&key, trade(0, 1.0));
        assert_eq!(store.horizon_ms(&key), Some(DEFAULT_TRADE_HORIZON_MS));
    }

    #[test]
    fn horizon_is_never_touched_before_first_touch() {
        let store = TradeStore::new();
        let key = addr();
        assert_eq!(store.horizon_ms(&key), None, "an untouched instrument has no entry, so no horizon yet");
    }

    #[test]
    fn widen_horizon_is_monotonic_max() {
        let store = TradeStore::new();
        let key = addr();
        store.push_live(&key, trade(0, 1.0));
        assert_eq!(store.horizon_ms(&key), Some(DEFAULT_TRADE_HORIZON_MS));

        store.widen_horizon(&key, DEFAULT_TRADE_HORIZON_MS * 2);
        assert_eq!(store.horizon_ms(&key), Some(DEFAULT_TRADE_HORIZON_MS * 2), "a deeper ask must widen");

        store.widen_horizon(&key, DEFAULT_TRADE_HORIZON_MS);
        assert_eq!(
            store.horizon_ms(&key),
            Some(DEFAULT_TRADE_HORIZON_MS * 2),
            "a shallower ask must never shrink an already-widened horizon"
        );
    }

    #[test]
    fn widen_horizon_creates_the_entry_on_first_touch() {
        let store = TradeStore::new();
        let key = addr();
        store.widen_horizon(&key, 12 * 3_600_000);
        assert_eq!(store.horizon_ms(&key), Some(12 * 3_600_000));
        assert_eq!(store.len(&key), 0, "widening alone must not fabricate any trades");
    }

    #[test]
    fn push_live_evicts_trades_older_than_the_default_horizon() {
        // No explicit widen -> entry gets DEFAULT_TRADE_HORIZON_MS (6h) on
        // first touch (widen_horizon is monotonic-max, so a shallower ask
        // than the default floor is a no-op — see
        // `widen_horizon_is_monotonic_max`). Use gaps bigger than 6h so
        // eviction actually fires against the real default.
        let store = TradeStore::new();
        let key = addr();
        let six_hours = DEFAULT_TRADE_HORIZON_MS;

        store.push_live(&key, trade(0, 1.0));
        store.push_live(&key, trade(six_hours / 2, 2.0));
        // Newest so far is half the horizon -> cutoff is negative, nothing
        // expired yet.
        assert_eq!(store.len(&key), 2);

        // Newest becomes 2x the horizon -> cutoff = 2h6 - h6 = h6; both
        // earlier points (0 and h6/2) are older than the cutoff and must be
        // evicted.
        store.push_live(&key, trade(six_hours * 2, 3.0));
        assert_eq!(store.len(&key), 1);
        assert_eq!(store.oldest_ts(&key), Some(six_hours * 2));
    }

    #[test]
    fn prepend_batch_evicts_trades_older_than_the_horizon() {
        let store = TradeStore::new();
        let key = addr();
        // Widen deep enough that the assertion is unambiguously about the
        // WIDENED value, not accidentally the untouched default floor.
        store.widen_horizon(&key, 1_000);
        assert_eq!(store.horizon_ms(&key), Some(DEFAULT_TRADE_HORIZON_MS.max(1_000)));
        let horizon = store.horizon_ms(&key).unwrap_or(DEFAULT_TRADE_HORIZON_MS);

        // Live edge is far ahead of the horizon window already.
        let live_ts = horizon * 10;
        store.push_live(&key, trade(live_ts, 5.0));

        // Backfill page lands entirely before the horizon cutoff
        // (live_ts - horizon) -> every prepended point must be evicted
        // again right after landing, leaving only the live edge.
        store.prepend_batch(&key, &[trade(100, 1.0), trade(200, 2.0), trade(300, 3.0)]);
        assert_eq!(store.len(&key), 1);
        assert_eq!(store.oldest_ts(&key), Some(live_ts));
    }

    #[test]
    fn prepend_batch_keeps_points_inside_the_widened_horizon() {
        let store = TradeStore::new();
        let key = addr();
        store.widen_horizon(&key, 1_000);
        let horizon = store.horizon_ms(&key).unwrap_or(DEFAULT_TRADE_HORIZON_MS);
        let live_ts = horizon * 10;
        store.push_live(&key, trade(live_ts, 5.0));

        // This page's key sits just inside the horizon window (cutoff =
        // live_ts - horizon), so it survives eviction.
        let inside_ts = live_ts - horizon + 1;
        store.prepend_batch(&key, &[trade(inside_ts, 1.0)]);
        assert_eq!(store.len(&key), 2);
        assert_eq!(store.oldest_ts(&key), Some(inside_ts));
    }

    #[test]
    fn capacity_clip_is_still_detectable_via_len_after_widen() {
        // The count-capacity backstop must keep working even once the
        // horizon is deep enough that eviction never fires — the ring cap
        // is what actually clips in this scenario.
        let store = TradeStore::new();
        store.set_default_capacity(3);
        let key = addr();
        store.widen_horizon(&key, 1_000_000_000); // horizon far too deep to evict anything

        for ts in [100, 200, 300, 400] {
            store.push_live(&key, trade(ts, ts as f64));
        }
        assert_eq!(store.len(&key), 3, "ring capacity must still clip when the horizon does not evict");
        assert_eq!(store.oldest_ts(&key), Some(200), "oldest must be the one dropped, not newest");
    }
}
