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
/// ## Simple last-write-wins model
///
/// Two writers, one map per side:
///
/// 1. **Snapshot** (`apply_ws_snapshot` / `apply_rest`) — full replace. Clears
///    both maps and inserts the incoming levels. Last snapshot always wins.
/// 2. **WS delta** (`apply_ws_delta`) — incremental per-level update. `qty == 0`
///    removes the level; any positive `qty` inserts or replaces it.
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

    /// WS full snapshot — replaces the entire book.
    ///
    /// Both maps are cleared and repopulated from the incoming levels. Any level
    /// with `qty <= 0` is skipped. This is the source of truth for the full ladder.
    pub fn apply_ws_snapshot(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], ts_ms: i64) {
        self.replace_all(bids, asks);
        self.current.last_ws_ts_ms = ts_ms;
        self.current.version += 1;
        self.dirty = true;
        let snap = self.current_to_timed_snapshot();
        self.append_history(snap);
    }

    /// REST background snapshot — replaces the entire book.
    ///
    /// Identical to `apply_ws_snapshot` in behaviour; only the timestamp field
    /// differs. Last write wins regardless of source.
    pub fn apply_rest(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], ts_ms: i64) {
        self.replace_all(bids, asks);
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

    /// Shared full-replace logic used by both snapshot apply methods.
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

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn series() -> OrderbookSeries {
        OrderbookSeries::new(100)
    }

    // ── apply_rest — full replace ────────────────────────────────────────────

    #[test]
    fn apply_rest_full_replace() {
        let mut s = series();
        s.apply_rest(&[(100.0, 1.0), (99.0, 2.0)], &[(101.0, 1.5)], 1000);
        assert_eq!(s.current.bids.len(), 2);
        assert_eq!(s.current.asks.len(), 1);
        assert_eq!(s.current.best_bid(), Some(100.0));
        assert_eq!(s.current.best_ask(), Some(101.0));

        // Second REST call replaces the whole book
        s.apply_rest(&[(98.0, 3.0)], &[(103.0, 1.0)], 2000);
        assert_eq!(s.current.bids.len(), 1);
        assert_eq!(s.current.asks.len(), 1);
        assert_eq!(s.current.best_bid(), Some(98.0));
        assert_eq!(s.current.best_ask(), Some(103.0));
    }

    // ── apply_ws_snapshot — full replace ────────────────────────────────────

    #[test]
    fn apply_ws_snapshot_full_replace() {
        let mut s = series();
        s.apply_ws_snapshot(&[(99.0, 5.0), (99.5, 3.0)], &[(100.5, 2.0), (101.0, 1.0)], 1000);
        assert_eq!(s.current.bids.len(), 2);
        assert_eq!(s.current.asks.len(), 2);
        assert_eq!(s.current.best_bid(), Some(99.5));
        assert_eq!(s.current.best_ask(), Some(100.5));

        // Second snapshot replaces everything — previous levels gone
        s.apply_ws_snapshot(&[(50.0, 1.0)], &[(51.0, 1.0)], 2000);
        assert_eq!(s.current.bids.len(), 1);
        assert_eq!(s.current.asks.len(), 1);
        assert_eq!(s.current.best_bid(), Some(50.0));
        assert_eq!(s.current.best_ask(), Some(51.0));
    }

    // ── snapshot fully replaces previous state regardless of source ──────────

    #[test]
    fn snapshot_replaces_previous_state_across_sources() {
        let mut s = series();

        // Start with REST
        s.apply_rest(&[(100.0, 1.0), (99.0, 2.0)], &[(101.0, 1.5), (102.0, 3.0)], 1000);
        assert_eq!(s.current.bids.len(), 2);
        assert_eq!(s.current.asks.len(), 2);

        // WS snapshot fully replaces — REST levels gone
        s.apply_ws_snapshot(&[(50.0, 1.0)], &[(51.0, 1.0)], 2000);
        assert_eq!(s.current.bids.len(), 1);
        assert_eq!(s.current.asks.len(), 1);
        assert!(!s.current.bids.contains_key(&OrderedFloat(100.0)));
        assert!(!s.current.asks.contains_key(&OrderedFloat(101.0)));

        // REST snapshot after WS — WS levels gone, REST wins
        s.apply_rest(&[(200.0, 5.0)], &[(201.0, 5.0)], 3000);
        assert_eq!(s.current.bids.len(), 1);
        assert_eq!(s.current.asks.len(), 1);
        assert!(!s.current.bids.contains_key(&OrderedFloat(50.0)));
        assert_eq!(s.current.best_bid(), Some(200.0));
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
