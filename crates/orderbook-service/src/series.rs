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

    /// Full REST replace. Clears all levels and inserts the incoming set.
    pub fn apply_rest(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], ts_ms: i64) {
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
        self.current.last_rest_ts_ms = ts_ms;
        self.current.version += 1;
        self.dirty = true;

        let snap = self.current_to_timed_snapshot();
        self.append_history(snap);
    }

    /// WS partial replace (range-based).
    ///
    /// Computes the price range covered by the incoming levels, removes all
    /// existing entries within that range on both sides, then inserts the new
    /// levels.
    pub fn apply_ws_snapshot(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], ts_ms: i64) {
        // Compute min/max price from all incoming levels
        let all_prices = bids.iter().chain(asks.iter()).map(|&(p, _)| p);
        let mut min_price = f64::MAX;
        let mut max_price = f64::MIN;
        for p in all_prices {
            if p < min_price { min_price = p; }
            if p > max_price { max_price = p; }
        }

        if min_price <= max_price {
            // Remove existing entries in the covered range
            self.current.bids.retain(|k, _| k.0 < min_price || k.0 > max_price);
            self.current.asks.retain(|k, _| k.0 < min_price || k.0 > max_price);
        }

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
        self.current.last_ws_ts_ms = ts_ms;
        self.current.version += 1;
        self.dirty = true;

        let snap = self.current_to_timed_snapshot();
        self.append_history(snap);
    }

    /// WS incremental delta. `qty == 0.0` means remove that price level.
    pub fn apply_ws_delta(&mut self, bid_changes: &[(f64, f64)], ask_changes: &[(f64, f64)], ts_ms: i64) {
        for &(p, q) in bid_changes {
            if q == 0.0 {
                self.current.bids.remove(&OrderedFloat(p));
            } else {
                self.current.bids.insert(OrderedFloat(p), q);
            }
        }
        for &(p, q) in ask_changes {
            if q == 0.0 {
                self.current.asks.remove(&OrderedFloat(p));
            } else {
                self.current.asks.insert(OrderedFloat(p), q);
            }
        }
        self.current.last_ws_ts_ms = ts_ms;
        self.current.version += 1;
        self.dirty = true;

        let snap = self.current_to_timed_snapshot();
        self.append_history(snap);
    }
}
