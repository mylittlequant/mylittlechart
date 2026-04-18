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
/// ## Layered priority model
///
/// Three writers, three priorities:
///
/// 1. **WS snapshot** — source of truth inside its narrow price window (~$3–5 wide
///    for BTC perp). Owns every level in `[ws_window.0, ws_window.1]`.
/// 2. **REST snapshot** — secondary truth for the deep ladder *outside* the WS
///    window. Polled every 5 s. Must not overwrite WS-owned levels.
/// 3. **WS delta** — incremental updates for levels *outside* the WS window only.
///    Updates inside the window are silently ignored (snapshot already has truth).
///
/// After every write, a crossed-book sweep removes any bid ≥ best ask overlap.
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

    /// Price range of the most recent WS snapshot. Levels inside this range
    /// are owned by the WS snapshot — REST snapshots and WS deltas must not
    /// overwrite them.
    pub ws_window: Option<(f64, f64)>,
}

impl OrderbookSeries {
    pub fn new(history_capacity: usize) -> Self {
        Self {
            current: OrderbookSnapshot::default(),
            history: VecDeque::new(),
            history_capacity,
            refcount: 0,
            dirty: false,
            ws_window: None,
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

    /// WS partial replace (range-based). Source of truth for the narrow price
    /// window it covers.
    ///
    /// Computes the price range of the incoming levels, stores it as `ws_window`,
    /// wipes all existing levels inside that window, inserts the new levels, then
    /// runs a crossed-book sweep.
    pub fn apply_ws_snapshot(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], ts_ms: i64) {
        // 1. Compute the price window of this snapshot
        let mut min_p = f64::MAX;
        let mut max_p = f64::MIN;
        for &(p, _) in bids.iter().chain(asks.iter()) {
            if p < min_p { min_p = p; }
            if p > max_p { max_p = p; }
        }
        if min_p > max_p {
            // Empty snapshot — record the timestamp but do nothing
            self.current.last_ws_ts_ms = ts_ms;
            return;
        }

        // 2. Update window
        self.ws_window = Some((min_p, max_p));

        // 3. Remove ALL existing levels in the window (both sides)
        self.current.bids.retain(|k, _| k.0 < min_p || k.0 > max_p);
        self.current.asks.retain(|k, _| k.0 < min_p || k.0 > max_p);

        // 4. Insert new levels (with cross-side cleanup)
        for &(p, q) in bids {
            if q > 0.0 {
                self.current.asks.remove(&OrderedFloat(p)); // cross-side cleanup
                self.current.bids.insert(OrderedFloat(p), q);
            }
        }
        for &(p, q) in asks {
            if q > 0.0 {
                self.current.bids.remove(&OrderedFloat(p)); // cross-side cleanup
                self.current.asks.insert(OrderedFloat(p), q);
            }
        }

        // 5. Crossed-book sweep
        self.sweep_crossed_book();

        // 6. Bookkeeping
        self.current.last_ws_ts_ms = ts_ms;
        self.current.version += 1;
        self.dirty = true;

        let snap = self.current_to_timed_snapshot();
        self.append_history(snap);
    }

    /// REST background snapshot. Secondary truth for the deep ladder *outside*
    /// the WS window.
    ///
    /// If no WS window is recorded yet, behaves as a full replace (REST is the
    /// only source). Otherwise clears REST-owned levels (outside the window),
    /// inserts the REST data for those regions, and leaves WS-owned levels
    /// (inside the window) untouched.
    pub fn apply_rest(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], ts_ms: i64) {
        let window = self.ws_window;

        // 1. Remove existing levels OUTSIDE the WS window (REST owns those)
        if let Some((lo, hi)) = window {
            // Keep WS-owned levels (inside window), drop the rest
            self.current.bids.retain(|k, _| k.0 >= lo && k.0 <= hi);
            self.current.asks.retain(|k, _| k.0 >= lo && k.0 <= hi);
        } else {
            // No WS window yet — REST is the only source, do full replace
            self.current.bids.clear();
            self.current.asks.clear();
        }

        // 2. Insert REST levels — skip those inside WS window (WS already there)
        for &(p, q) in bids {
            if q <= 0.0 { continue; }
            if let Some((lo, hi)) = window {
                if p >= lo && p <= hi { continue; } // WS owns this slot
            }
            self.current.asks.remove(&OrderedFloat(p)); // cross-side cleanup
            self.current.bids.insert(OrderedFloat(p), q);
        }
        for &(p, q) in asks {
            if q <= 0.0 { continue; }
            if let Some((lo, hi)) = window {
                if p >= lo && p <= hi { continue; } // WS owns this slot
            }
            self.current.bids.remove(&OrderedFloat(p)); // cross-side cleanup
            self.current.asks.insert(OrderedFloat(p), q);
        }

        // 3. Crossed-book sweep
        self.sweep_crossed_book();

        // 4. Bookkeeping
        self.current.last_rest_ts_ms = ts_ms;
        self.current.version += 1;
        self.dirty = true;

        let snap = self.current_to_timed_snapshot();
        self.append_history(snap);
    }

    /// WS incremental delta. Updates levels *outside* the WS window only.
    ///
    /// Levels inside the WS window are silently skipped — the WS snapshot already
    /// holds the truth for that region. `qty == 0.0` means remove that price level.
    pub fn apply_ws_delta(&mut self, bid_changes: &[(f64, f64)], ask_changes: &[(f64, f64)], ts_ms: i64) {
        let window = self.ws_window;
        let in_window = |p: f64| -> bool {
            match window {
                Some((lo, hi)) => p >= lo && p <= hi,
                None => false,
            }
        };

        for &(p, q) in bid_changes {
            if in_window(p) { continue; } // WS snapshot owns this range, skip
            let key = OrderedFloat(p);
            if q == 0.0 {
                self.current.bids.remove(&key);
            } else {
                self.current.asks.remove(&key); // cross-side cleanup
                self.current.bids.insert(key, q);
            }
        }
        for &(p, q) in ask_changes {
            if in_window(p) { continue; }
            let key = OrderedFloat(p);
            if q == 0.0 {
                self.current.asks.remove(&key);
            } else {
                self.current.bids.remove(&key); // cross-side cleanup
                self.current.asks.insert(key, q);
            }
        }

        self.sweep_crossed_book();

        self.current.last_ws_ts_ms = ts_ms;
        self.current.version += 1;
        self.dirty = true;

        let snap = self.current_to_timed_snapshot();
        self.append_history(snap);
    }

    /// Resolve a crossed book (best_bid >= best_ask) by removing the offending
    /// top-of-book entry on each side, repeating until uncrossed.
    ///
    /// This is conservative: when crossed it removes both touching levels and
    /// continues until the book is clean. Typically 1–2 iterations suffice.
    fn sweep_crossed_book(&mut self) {
        loop {
            let best_bid = self.current.bids.iter().rev().next().map(|(k, _)| k.0);
            let best_ask = self.current.asks.iter().next().map(|(k, _)| k.0);
            match (best_bid, best_ask) {
                (Some(bb), Some(ba)) if bb >= ba => {
                    // Crossed — drop the offending entry on each side and repeat.
                    self.current.bids.remove(&OrderedFloat(bb));
                    self.current.asks.remove(&OrderedFloat(ba));
                }
                _ => break,
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

    // ── apply_rest (no WS window) ────────────────────────────────────────────

    #[test]
    fn apply_rest_full_replaces_when_no_ws_window() {
        let mut s = series();
        // First REST call — full replace
        s.apply_rest(&[(100.0, 1.0), (99.0, 2.0)], &[(101.0, 1.5)], 1000);
        assert_eq!(s.current.bids.len(), 2);
        assert_eq!(s.current.asks.len(), 1);
        assert_eq!(s.current.best_bid(), Some(100.0));
        assert_eq!(s.current.best_ask(), Some(101.0));

        // Second REST call without WS window — replaces the whole book
        s.apply_rest(&[(98.0, 3.0)], &[(103.0, 1.0)], 2000);
        assert_eq!(s.current.bids.len(), 1);
        assert_eq!(s.current.asks.len(), 1);
        assert_eq!(s.current.best_bid(), Some(98.0));
        assert_eq!(s.current.best_ask(), Some(103.0));
    }

    // ── apply_rest with WS window ────────────────────────────────────────────

    #[test]
    fn apply_rest_preserves_ws_window() {
        let mut s = series();

        // WS snapshot owns 99.0 – 101.0
        s.apply_ws_snapshot(
            &[(99.0, 5.0), (99.5, 3.0)],
            &[(100.5, 2.0), (101.0, 1.0)],
            1000,
        );
        assert_eq!(s.ws_window, Some((99.0, 101.0)));

        // REST provides deep levels (outside window) AND tries to overwrite 99.0
        s.apply_rest(
            &[(95.0, 10.0), (96.0, 8.0), (99.0, 999.0)], // 99.0 is inside window
            &[(105.0, 4.0), (106.0, 3.0), (100.5, 999.0)], // 100.5 inside window
            2000,
        );

        // WS-owned levels must be untouched
        assert_eq!(s.current.bids.get(&OrderedFloat(99.0)), Some(&5.0), "WS bid 99.0 overwritten");
        assert_eq!(s.current.bids.get(&OrderedFloat(99.5)), Some(&3.0), "WS bid 99.5 overwritten");
        assert_eq!(s.current.asks.get(&OrderedFloat(100.5)), Some(&2.0), "WS ask 100.5 overwritten");
        assert_eq!(s.current.asks.get(&OrderedFloat(101.0)), Some(&1.0), "WS ask 101.0 overwritten");

        // REST deep levels should be present
        assert_eq!(s.current.bids.get(&OrderedFloat(95.0)), Some(&10.0));
        assert_eq!(s.current.bids.get(&OrderedFloat(96.0)), Some(&8.0));
        assert_eq!(s.current.asks.get(&OrderedFloat(105.0)), Some(&4.0));
        assert_eq!(s.current.asks.get(&OrderedFloat(106.0)), Some(&3.0));
    }

    // ── apply_ws_delta — inside window is ignored ────────────────────────────

    #[test]
    fn apply_ws_delta_skips_ws_window() {
        let mut s = series();

        // WS snapshot: bids 99–99.5, asks 100.5–101
        s.apply_ws_snapshot(
            &[(99.0, 5.0), (99.5, 3.0)],
            &[(100.5, 2.0), (101.0, 1.0)],
            1000,
        );

        let version_before = s.current.version;

        // Delta tries to touch levels inside the window — should be ignored
        s.apply_ws_delta(
            &[(99.0, 0.0), (99.5, 99.0)], // remove 99.0, overwrite 99.5
            &[(100.5, 0.0)],               // remove 100.5
            1001,
        );

        // All WS-window levels unchanged
        assert_eq!(s.current.bids.get(&OrderedFloat(99.0)), Some(&5.0));
        assert_eq!(s.current.bids.get(&OrderedFloat(99.5)), Some(&3.0));
        assert_eq!(s.current.asks.get(&OrderedFloat(100.5)), Some(&2.0));

        // Version still incremented (bookkeeping happens even with no-op delta)
        assert_eq!(s.current.version, version_before + 1);
    }

    // ── apply_ws_delta — outside window writes ────────────────────────────────

    #[test]
    fn apply_ws_delta_outside_window_writes() {
        let mut s = series();

        // WS snapshot window: 99.0 – 101.0
        s.apply_ws_snapshot(
            &[(99.0, 5.0)],
            &[(101.0, 1.0)],
            1000,
        );

        // Delta on prices outside the window
        s.apply_ws_delta(
            &[(95.0, 8.0), (96.0, 6.0)],   // outside: write
            &[(105.0, 3.0), (106.0, 2.0)],  // outside: write
            1001,
        );

        assert_eq!(s.current.bids.get(&OrderedFloat(95.0)), Some(&8.0));
        assert_eq!(s.current.bids.get(&OrderedFloat(96.0)), Some(&6.0));
        assert_eq!(s.current.asks.get(&OrderedFloat(105.0)), Some(&3.0));
        assert_eq!(s.current.asks.get(&OrderedFloat(106.0)), Some(&2.0));

        // Remove via zero qty
        s.apply_ws_delta(&[(95.0, 0.0)], &[(105.0, 0.0)], 1002);
        assert!(!s.current.bids.contains_key(&OrderedFloat(95.0)));
        assert!(!s.current.asks.contains_key(&OrderedFloat(105.0)));
    }

    // ── sweep_crossed_book ───────────────────────────────────────────────────

    #[test]
    fn crossed_book_sweep() {
        let mut s = series();

        // Manually create a crossed book: best_bid 102 > best_ask 100
        s.current.bids.insert(OrderedFloat(100.0), 1.0);
        s.current.bids.insert(OrderedFloat(101.0), 2.0);
        s.current.bids.insert(OrderedFloat(102.0), 3.0); // will cross
        s.current.asks.insert(OrderedFloat(100.0), 1.0); // crossed
        s.current.asks.insert(OrderedFloat(103.0), 1.0);

        s.sweep_crossed_book();

        // After sweep: no bid >= best_ask
        if let (Some(bb), Some(ba)) = (s.current.best_bid(), s.current.best_ask()) {
            assert!(bb < ba, "book still crossed after sweep: bid={bb} ask={ba}");
        }
        // Healthy levels beyond the overlap should survive
        assert!(s.current.asks.contains_key(&OrderedFloat(103.0)));
    }

    // ── ws_window is set correctly ────────────────────────────────────────────

    #[test]
    fn ws_window_tracks_snapshot_range() {
        let mut s = series();
        assert_eq!(s.ws_window, None);

        s.apply_ws_snapshot(&[(50.0, 1.0), (51.0, 2.0)], &[(52.0, 1.0), (53.0, 0.5)], 1000);
        assert_eq!(s.ws_window, Some((50.0, 53.0)));

        // Second snapshot with different range updates the window
        s.apply_ws_snapshot(&[(60.0, 1.0)], &[(61.0, 1.0)], 2000);
        assert_eq!(s.ws_window, Some((60.0, 61.0)));
    }

    // ── empty WS snapshot doesn't clear window ───────────────────────────────

    #[test]
    fn apply_ws_snapshot_empty_noop() {
        let mut s = series();
        s.apply_ws_snapshot(&[(99.0, 1.0)], &[(101.0, 1.0)], 1000);
        let window_before = s.ws_window;
        let version_before = s.current.version;

        // Empty slices → min_p > max_p → early return
        s.apply_ws_snapshot(&[], &[], 2000);

        assert_eq!(s.ws_window, window_before, "window changed on empty snapshot");
        assert_eq!(s.current.version, version_before, "version changed on empty snapshot");
        assert_eq!(s.current.last_ws_ts_ms, 2000, "ts not updated on empty snapshot");
    }
}
