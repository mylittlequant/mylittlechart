use std::collections::{BTreeMap, VecDeque};
use ordered_float::OrderedFloat;
use orderbook_store::TimedSnapshot;

/// Default maximum snapshots held in memory per series.
pub const DEFAULT_HISTORY_CAPACITY: usize = 100;

/// Live merged orderbook state for one (exchange, symbol, account_type).
///
/// `bids` and `asks` are both stored as ascending `OrderedFloat` keys in a
/// `BTreeMap`. To iterate bids high-to-low, call `.iter().rev()`.
#[derive(Debug, Clone, Default)]
pub struct OrderbookSnapshot {
    /// price → qty, ascending key order. Iterate `.rev()` for best-bid-first.
    pub bids: BTreeMap<OrderedFloat<f64>, f64>,
    /// price → qty, ascending key order. Iterate forward for best-ask-first.
    pub asks: BTreeMap<OrderedFloat<f64>, f64>,
    pub last_rest_ts_ms: i64,
    pub last_ws_ts_ms: i64,
    pub version: u64,
}

impl OrderbookSnapshot {
    pub fn best_bid(&self) -> Option<f64> {
        self.bids.iter().rev().next().map(|(k, _)| k.0)
    }

    pub fn best_ask(&self) -> Option<f64> {
        self.asks.iter().next().map(|(k, _)| k.0)
    }

    pub fn mid(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(b), Some(a)) => Some((b + a) / 2.0),
            _ => None,
        }
    }

    /// Iterator over top N bids, highest price first (price, qty).
    pub fn iter_bids_top_n(&self, n: usize) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.bids.iter().rev().take(n).map(|(k, v)| (k.0, *v))
    }

    /// Iterator over top N asks, lowest price first (price, qty).
    pub fn iter_asks_top_n(&self, n: usize) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.asks.iter().take(n).map(|(k, v)| (k.0, *v))
    }
}

/// In-memory orderbook series with history ring and lifecycle management.
///
/// Mutated only by `OrderbookService` (or directly via the apply_* methods for
/// bridge write paths). Panels hold `Arc<RwLock<OrderbookSeries>>` and call
/// `.read()` during render.
///
/// ## Write model
///
/// Three writers, one map per side:
///
/// 1. **REST bootstrap** (`apply_rest`) — full replace. Called ONCE per
///    subscription to seed the local book from a deep REST snapshot (up to
///    1000 levels). Clears both maps, then inserts incoming levels.
/// 2. **WS snapshot** (`apply_ws_snapshot`) — range replace. For exchanges
///    that send periodic partial snapshots instead of incremental deltas.
///    Replaces only the price range covered by the incoming levels.
/// 3. **WS delta** (`apply_ws_delta`) — incremental per-level update. This is
///    the primary update path. `qty == 0` removes the level; positive `qty`
///    inserts or replaces it.
///
/// No window tracking, no priority regions, no crossed-book sweep.
#[derive(Debug)]
pub struct OrderbookSeries {
    /// Live merged state, updated on every apply_* call.
    pub current: OrderbookSnapshot,

    /// Ring of past snapshots. `front` = oldest, `back` = newest.
    pub history: VecDeque<TimedSnapshot>,

    /// Maximum number of history entries to keep in memory.
    pub history_capacity: usize,

    /// Active panel subscription count.
    ///
    /// Incremented by `OrderbookService::acquire()`, decremented by `release()`.
    pub refcount: usize,

    /// True when `current` has been mutated since the last disk flush.
    pub dirty: bool,
}

impl OrderbookSeries {
    pub fn new(history_capacity: usize) -> Self {
        Self {
            current: OrderbookSnapshot::default(),
            history: VecDeque::new(),
            history_capacity,
            refcount: 0,
            dirty: false,
        }
    }

    /// Append a snapshot to the history ring, evicting the oldest if at capacity.
    pub fn append_history(&mut self, snap: TimedSnapshot) {
        if self.history.len() >= self.history_capacity {
            self.history.pop_front();
        }
        self.history.push_back(snap);
        self.dirty = true;
    }

    /// Snapshot the current live state as a `TimedSnapshot`.
    pub fn current_to_timed_snapshot(&self) -> TimedSnapshot {
        let ts = self.current.last_ws_ts_ms.max(self.current.last_rest_ts_ms);
        TimedSnapshot {
            timestamp_ms: ts,
            bids: self.current.bids.iter().rev().map(|(k, v)| (k.0, *v)).collect(),
            asks: self.current.asks.iter().map(|(k, v)| (k.0, *v)).collect(),
        }
    }

    // ── Apply methods ────────────────────────────────────────────────────────

    /// WS partial snapshot — replaces ONLY the price range it covers.
    ///
    /// A WS snapshot is a narrow window (~$3 wide for BTC perp). It must NOT
    /// wipe the deep ladder supplied by REST. We compute the price range of
    /// the incoming levels per side, drop existing entries inside that range,
    /// then insert the new ones. Levels outside the range survive untouched.
    pub fn apply_ws_snapshot(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], ts_ms: i64) {
        self.replace_in_range(bids, asks);
        self.current.last_ws_ts_ms = ts_ms;
        self.current.version += 1;
        self.dirty = true;
        let snap = self.current_to_timed_snapshot();
        self.append_history(snap);
    }

    /// REST bootstrap — range-replace seeded from the REST snapshot.
    ///
    /// Called ONCE per subscription with a deep REST snapshot (up to 1000
    /// levels). Per side: drops existing entries whose price falls within the
    /// range covered by the incoming levels, then inserts those levels.
    /// Entries outside the range survive untouched — this preserves any WS
    /// data that arrived before the REST response landed (defense-in-depth;
    /// with proper `subscribe_depth` sequencing this situation cannot occur).
    ///
    /// If the book is empty the behaviour is identical to a full replace.
    pub fn apply_rest(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], ts_ms: i64) {
        self.replace_in_range(bids, asks);
        self.current.last_rest_ts_ms = ts_ms;
        self.current.version += 1;
        self.dirty = true;
        let snap = self.current_to_timed_snapshot();
        self.append_history(snap);
    }

    /// WS incremental delta — updates individual price levels.
    ///
    /// `qty == 0.0` removes the level; any positive qty inserts or replaces it.
    /// No window filtering, no crossed-book sweep.
    pub fn apply_ws_delta(&mut self, bid_changes: &[(f64, f64)], ask_changes: &[(f64, f64)], ts_ms: i64) {
        for &(p, q) in bid_changes {
            let key = OrderedFloat(p);
            if q == 0.0 {
                self.current.bids.remove(&key);
            } else {
                self.current.bids.insert(key, q);
            }
        }
        for &(p, q) in ask_changes {
            let key = OrderedFloat(p);
            if q == 0.0 {
                self.current.asks.remove(&key);
            } else {
                self.current.asks.insert(key, q);
            }
        }
        self.current.last_ws_ts_ms = ts_ms;
        self.current.version += 1;
        self.dirty = true;
        let snap = self.current_to_timed_snapshot();
        self.append_history(snap);
    }

    /// Shared range-replace logic used by both snapshot apply methods.
    ///
    /// Per side: compute the price range of the incoming levels, drop existing
    /// entries inside that range, then insert the new ones. This keeps deep
    /// ladder data (REST) when a narrow snapshot (WS) arrives, and vice versa.
    fn replace_in_range(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)]) {
        // Bids
        if let Some((lo, hi)) = price_range(bids) {
            self.current.bids.retain(|k, _| k.0 < lo || k.0 > hi);
            for &(p, q) in bids {
                if q > 0.0 { self.current.bids.insert(OrderedFloat(p), q); }
            }
        }
        // Asks
        if let Some((lo, hi)) = price_range(asks) {
            self.current.asks.retain(|k, _| k.0 < lo || k.0 > hi);
            for &(p, q) in asks {
                if q > 0.0 { self.current.asks.insert(OrderedFloat(p), q); }
            }
        }
    }

    /// Legacy full-replace, kept for tests and potential callers that want to
    /// truly wipe the book. Not used by the apply_* paths anymore.
    #[allow(dead_code)]
    fn replace_all(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)]) {
        self.current.bids.clear();
        self.current.asks.clear();
        for &(p, q) in bids {
            if q > 0.0 {
                self.current.bids.insert(OrderedFloat(p), q);
            }
        }
        for &(p, q) in asks {
            if q > 0.0 {
                self.current.asks.insert(OrderedFloat(p), q);
            }
        }
    }
}

/// Compute (min_price, max_price) from a slice of (price, qty) levels.
/// Returns `None` if every level has qty <= 0 or the slice is empty.
fn price_range(levels: &[(f64, f64)]) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut found = false;
    for &(p, q) in levels {
        if q > 0.0 {
            if p < lo { lo = p; }
            if p > hi { hi = p; }
            found = true;
        }
    }
    if found { Some((lo, hi)) } else { None }
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn series() -> OrderbookSeries {
        OrderbookSeries::new(100)
    }

    // ── apply_rest — range-replace (bootstrap semantics) ────────────────────

    #[test]
    fn apply_rest_seeds_empty_book() {
        let mut s = series();
        // On an empty book, range-replace is equivalent to a full insert.
        s.apply_rest(&[(100.0, 1.0), (99.0, 2.0)], &[(101.0, 1.5)], 1000);
        assert_eq!(s.current.bids.len(), 2);
        assert_eq!(s.current.asks.len(), 1);
        assert_eq!(s.current.best_bid(), Some(100.0));
        assert_eq!(s.current.best_ask(), Some(101.0));
    }

    #[test]
    fn apply_rest_range_replaces_overlapping_levels() {
        let mut s = series();
        // Seed with two bids (99, 100) and two asks (101, 102).
        s.apply_rest(&[(100.0, 1.0), (99.0, 2.0)], &[(101.0, 1.5), (102.0, 3.0)], 1000);

        // Second REST call covers bids [98, 100] and asks [103, 103].
        // Bids 99 and 100 are inside [98, 100] → replaced.
        // Ask 101 and 102 are outside [103, 103] → survive.
        s.apply_rest(&[(98.0, 3.0), (100.0, 5.0)], &[(103.0, 1.0)], 2000);
        // 98 inserted, 100 updated to 5.0, 99 dropped (was in range, not in new payload).
        assert!(s.current.bids.contains_key(&OrderedFloat(98.0)));
        assert!(s.current.bids.contains_key(&OrderedFloat(100.0)));
        assert!(!s.current.bids.contains_key(&OrderedFloat(99.0)));
        // Ask 103 inserted; 101 and 102 survive (outside [103,103] range).
        assert!(s.current.asks.contains_key(&OrderedFloat(103.0)));
        assert!(s.current.asks.contains_key(&OrderedFloat(101.0)));
        assert!(s.current.asks.contains_key(&OrderedFloat(102.0)));
    }

    #[test]
    fn apply_rest_preserves_out_of_range_ws_levels() {
        let mut s = series();
        // Pre-seed with WS snapshot at a price range far from the REST range.
        s.apply_ws_snapshot(&[(50.0, 5.0), (51.0, 5.0)], &[(200.0, 5.0), (201.0, 5.0)], 500);
        assert_eq!(s.current.bids.len(), 2);

        // REST bootstrap covers [95, 97] bids and [103, 103] asks.
        // WS levels (50, 51) are outside that bid range → preserved.
        // WS levels (200, 201) are outside the ask range → preserved.
        s.apply_rest(&[(95.0, 1.0), (96.0, 2.0), (97.0, 3.0)], &[(103.0, 1.0)], 1000);
        // REST bids inserted.
        assert!(s.current.bids.contains_key(&OrderedFloat(95.0)));
        assert!(s.current.bids.contains_key(&OrderedFloat(96.0)));
        assert!(s.current.bids.contains_key(&OrderedFloat(97.0)));
        // Out-of-range WS bids preserved.
        assert!(s.current.bids.contains_key(&OrderedFloat(50.0)));
        assert!(s.current.bids.contains_key(&OrderedFloat(51.0)));
        // REST ask inserted.
        assert!(s.current.asks.contains_key(&OrderedFloat(103.0)));
        // Out-of-range WS asks preserved.
        assert!(s.current.asks.contains_key(&OrderedFloat(200.0)));
        assert!(s.current.asks.contains_key(&OrderedFloat(201.0)));
    }

    // ── apply_ws_snapshot — range replace ────────────────────────────────────

    #[test]
    fn apply_ws_snapshot_replaces_only_its_range() {
        let mut s = series();
        s.apply_ws_snapshot(&[(99.0, 5.0), (99.5, 3.0)], &[(100.5, 2.0), (101.0, 1.0)], 1000);
        assert_eq!(s.current.bids.len(), 2);
        assert_eq!(s.current.asks.len(), 2);
        assert_eq!(s.current.best_bid(), Some(99.5));
        assert_eq!(s.current.best_ask(), Some(100.5));

        // Second snapshot covers [50,50] bid and [51,51] ask. Old levels
        // (99..99.5 bids, 100.5..101 asks) are OUTSIDE that range → preserved.
        s.apply_ws_snapshot(&[(50.0, 1.0)], &[(51.0, 1.0)], 2000);
        assert_eq!(s.current.bids.len(), 3);  // 99, 99.5, 50
        assert_eq!(s.current.asks.len(), 3);  // 100.5, 101, 51
    }

    // ── REST bootstrap + WS delta: canonical Binance pattern ────────────────

    #[test]
    fn rest_bootstrap_then_ws_delta() {
        let mut s = series();

        // REST bootstrap seeds the full book (deep snapshot).
        let rest_bids: Vec<(f64,f64)> = (90..=100).map(|p| (p as f64, 1.0)).collect();
        let rest_asks: Vec<(f64,f64)> = (101..=110).map(|p| (p as f64, 1.0)).collect();
        s.apply_rest(&rest_bids, &rest_asks, 1000);
        assert_eq!(s.current.bids.len(), 11);
        assert_eq!(s.current.asks.len(), 10);

        // WS delta updates individual levels on top of the bootstrapped book.
        s.apply_ws_delta(
            &[(100.0, 5.0), (89.0, 2.0), (90.0, 0.0)],
            &[(101.0, 5.0), (111.0, 3.0), (110.0, 0.0)],
            2000,
        );
        // 100 updated to 5.0, 89 inserted, 90 removed.
        assert_eq!(s.current.bids.get(&OrderedFloat(100.0)).copied(), Some(5.0));
        assert_eq!(s.current.bids.get(&OrderedFloat(89.0)).copied(), Some(2.0));
        assert!(!s.current.bids.contains_key(&OrderedFloat(90.0)));
        // Outer REST levels still intact.
        assert_eq!(s.current.bids.get(&OrderedFloat(91.0)).copied(), Some(1.0));
        // 101 updated, 111 inserted, 110 removed.
        assert_eq!(s.current.asks.get(&OrderedFloat(101.0)).copied(), Some(5.0));
        assert_eq!(s.current.asks.get(&OrderedFloat(111.0)).copied(), Some(3.0));
        assert!(!s.current.asks.contains_key(&OrderedFloat(110.0)));
    }

    // ── WS snapshot range-replace (exchanges without diff stream) ────────────

    #[test]
    fn ws_snapshot_range_replace_on_seeded_book() {
        let mut s = series();

        // REST bootstrap seeds the full book.
        let rest_bids: Vec<(f64,f64)> = (90..=100).map(|p| (p as f64, 1.0)).collect();
        let rest_asks: Vec<(f64,f64)> = (101..=110).map(|p| (p as f64, 1.0)).collect();
        s.apply_rest(&rest_bids, &rest_asks, 1000);

        // WS snapshot covers only a narrow range — levels outside survive.
        s.apply_ws_snapshot(&[(99.0, 5.0), (100.0, 5.0)], &[(101.0, 5.0), (102.0, 5.0)], 2000);

        // Bids: 90..98 from REST (9), 99..100 replaced from WS (2) → still 11
        assert_eq!(s.current.bids.len(), 11);
        // Asks: 101..102 replaced from WS (2), 103..110 from REST (8) → 10 total
        assert_eq!(s.current.asks.len(), 10);
        assert_eq!(s.current.bids.get(&OrderedFloat(99.0)).copied(), Some(5.0));
        assert_eq!(s.current.bids.get(&OrderedFloat(90.0)).copied(), Some(1.0));
        assert_eq!(s.current.asks.get(&OrderedFloat(102.0)).copied(), Some(5.0));
        assert_eq!(s.current.asks.get(&OrderedFloat(110.0)).copied(), Some(1.0));
    }

    // ── apply_ws_delta — insert new level ────────────────────────────────────

    #[test]
    fn apply_ws_delta_inserts_new_level() {
        let mut s = series();
        s.apply_rest(&[(100.0, 1.0)], &[(101.0, 1.0)], 1000);

        s.apply_ws_delta(&[(95.0, 8.0)], &[(106.0, 3.0)], 1001);

        assert_eq!(s.current.bids.get(&OrderedFloat(95.0)), Some(&8.0));
        assert_eq!(s.current.asks.get(&OrderedFloat(106.0)), Some(&3.0));
        // Previous levels still present
        assert_eq!(s.current.bids.get(&OrderedFloat(100.0)), Some(&1.0));
        assert_eq!(s.current.asks.get(&OrderedFloat(101.0)), Some(&1.0));
    }

    // ── apply_ws_delta — remove level via qty=0 ──────────────────────────────

    #[test]
    fn apply_ws_delta_removes_level_on_zero_qty() {
        let mut s = series();
        s.apply_rest(&[(100.0, 1.0), (99.0, 2.0)], &[(101.0, 1.5), (102.0, 3.0)], 1000);

        // Remove 99.0 bid and 102.0 ask
        s.apply_ws_delta(&[(99.0, 0.0)], &[(102.0, 0.0)], 1001);

        assert!(!s.current.bids.contains_key(&OrderedFloat(99.0)));
        assert!(!s.current.asks.contains_key(&OrderedFloat(102.0)));
        // Other levels untouched
        assert_eq!(s.current.bids.get(&OrderedFloat(100.0)), Some(&1.0));
        assert_eq!(s.current.asks.get(&OrderedFloat(101.0)), Some(&1.5));
    }

    // ── apply_ws_delta — writes at any price ────────────────────────────────

    #[test]
    fn apply_ws_delta_writes_at_any_price() {
        let mut s = series();

        s.apply_ws_snapshot(&[(99.0, 5.0)], &[(101.0, 1.0)], 1000);

        // Delta on any prices — no filtering
        s.apply_ws_delta(
            &[(95.0, 8.0), (96.0, 6.0), (99.0, 0.0)],
            &[(105.0, 3.0), (106.0, 2.0), (101.0, 0.0)],
            1001,
        );

        assert_eq!(s.current.bids.get(&OrderedFloat(95.0)), Some(&8.0));
        assert_eq!(s.current.bids.get(&OrderedFloat(96.0)), Some(&6.0));
        assert!(!s.current.bids.contains_key(&OrderedFloat(99.0)));
        assert_eq!(s.current.asks.get(&OrderedFloat(105.0)), Some(&3.0));
        assert_eq!(s.current.asks.get(&OrderedFloat(106.0)), Some(&2.0));
        assert!(!s.current.asks.contains_key(&OrderedFloat(101.0)));
    }

    // ── best_bid / best_ask helpers ───────────────────────────────────────────

    #[test]
    fn best_bid_and_best_ask() {
        let mut s = series();
        assert_eq!(s.current.best_bid(), None);
        assert_eq!(s.current.best_ask(), None);

        s.apply_ws_snapshot(
            &[(99.0, 1.0), (100.0, 2.0), (98.0, 3.0)],
            &[(101.0, 1.0), (102.0, 0.5), (103.0, 0.1)],
            1000,
        );

        assert_eq!(s.current.best_bid(), Some(100.0));
        assert_eq!(s.current.best_ask(), Some(101.0));
    }

    // ── version always incremented ────────────────────────────────────────────

    #[test]
    fn version_incremented_on_every_apply() {
        let mut s = series();
        assert_eq!(s.current.version, 0);

        s.apply_ws_snapshot(&[(99.0, 1.0)], &[(101.0, 1.0)], 1000);
        assert_eq!(s.current.version, 1);

        s.apply_rest(&[(99.0, 1.0)], &[(101.0, 1.0)], 2000);
        assert_eq!(s.current.version, 2);

        s.apply_ws_delta(&[(98.0, 1.0)], &[], 3000);
        assert_eq!(s.current.version, 3);
    }
}
