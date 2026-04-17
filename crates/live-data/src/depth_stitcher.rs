//! Local order book maintained from WS snapshots + WS deltas.
//!
//! ## Architecture
//!
//! - **WS snapshot** seeds the book (full replacement).
//! - **WS delta** applies incremental updates on top.
//! - **REST snapshot** (optional) merges deeper levels periodically.
//!
//! No Binance-style sequence stitching — the WS stream itself provides
//! both snapshots and deltas on all exchanges.

use std::collections::BTreeMap;

use digdigdig3::core::types::{OrderBook, OrderbookDelta};

// ─────────────────────────────────────────────────────────────────────────────
// CONSTANTS
// ─────────────────────────────────────────────────────────────────────────────

pub(crate) const EMIT_LEVELS: usize = 500;

// ─────────────────────────────────────────────────────────────────────────────
// ORDERED FLOAT — total ordering wrapper for BTreeMap price keys
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
struct OrderedFloat(f64);

impl PartialEq for OrderedFloat {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for OrderedFloat {}

impl PartialOrd for OrderedFloat {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedFloat {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .partial_cmp(&other.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LOCAL BOOK
// ─────────────────────────────────────────────────────────────────────────────

struct LocalBook {
    /// price → size; highest price is best bid
    bids: BTreeMap<OrderedFloat, f64>,
    /// price → size; lowest price is best ask
    asks: BTreeMap<OrderedFloat, f64>,
    timestamp: i64,
}

impl LocalBook {
    fn new() -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            timestamp: 0,
        }
    }

    /// Replace the entire book from a snapshot.
    fn seed(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], timestamp: i64) {
        self.bids.clear();
        self.asks.clear();
        for &(price, size) in bids {
            if size > 0.0 {
                self.bids.insert(OrderedFloat(price), size);
            }
        }
        for &(price, size) in asks {
            if size > 0.0 {
                self.asks.insert(OrderedFloat(price), size);
            }
        }
        self.timestamp = timestamp;
    }

    /// Apply incremental level updates (size == 0 removes the level).
    fn apply_delta(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], timestamp: i64) {
        for &(price, size) in bids {
            let key = OrderedFloat(price);
            if size == 0.0 {
                self.bids.remove(&key);
            } else {
                self.bids.insert(key, size);
            }
        }
        for &(price, size) in asks {
            let key = OrderedFloat(price);
            if size == 0.0 {
                self.asks.remove(&key);
            } else {
                self.asks.insert(key, size);
            }
        }
        self.timestamp = timestamp;
    }

    /// Emit the top-N best bid/ask levels.
    fn emit_top_n(&self, n: usize) -> EmittedBook {
        let bids: Vec<(f64, f64)> = self
            .bids
            .iter()
            .rev()
            .take(n)
            .map(|(k, &v)| (k.0, v))
            .collect();
        let asks: Vec<(f64, f64)> = self
            .asks
            .iter()
            .take(n)
            .map(|(k, &v)| (k.0, v))
            .collect();
        EmittedBook {
            bids,
            asks,
            timestamp: self.timestamp,
        }
    }
}

impl LocalBook {
    /// Compute the diff between current state and a new snapshot.
    ///
    /// Returns entries for:
    /// - prices in NEW missing from OLD, or with a different size → `(price, new_size)`
    /// - prices in OLD missing from NEW → `(price, 0.0)` (removal)
    /// - prices with identical sizes are skipped
    fn diff_against(&self, new_bids: &[(f64, f64)], new_asks: &[(f64, f64)]) -> SyntheticDelta {
        let mut bids = Vec::new();
        let mut asks = Vec::new();

        // Bids: check new against old
        for &(price, new_size) in new_bids {
            let key = OrderedFloat(price);
            match self.bids.get(&key) {
                Some(&old_size) if old_size == new_size => {}
                _ => bids.push((price, new_size)),
            }
        }
        // Bids: emit removals for prices no longer in new snapshot
        let new_bid_keys: std::collections::HashSet<u64> = new_bids
            .iter()
            .map(|&(p, _)| OrderedFloat(p).0.to_bits())
            .collect();
        for (key, _) in &self.bids {
            if !new_bid_keys.contains(&key.0.to_bits()) {
                bids.push((key.0, 0.0));
            }
        }

        // Asks: check new against old
        for &(price, new_size) in new_asks {
            let key = OrderedFloat(price);
            match self.asks.get(&key) {
                Some(&old_size) if old_size == new_size => {}
                _ => asks.push((price, new_size)),
            }
        }
        // Asks: emit removals for prices no longer in new snapshot
        let new_ask_keys: std::collections::HashSet<u64> = new_asks
            .iter()
            .map(|&(p, _)| OrderedFloat(p).0.to_bits())
            .collect();
        for (key, _) in &self.asks {
            if !new_ask_keys.contains(&key.0.to_bits()) {
                asks.push((key.0, 0.0));
            }
        }

        SyntheticDelta { bids, asks }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PUBLIC TYPES
// ─────────────────────────────────────────────────────────────────────────────

/// Computed diff between two consecutive snapshots.
///
/// Each entry is `(price, size)` where `size == 0.0` means the level was removed.
pub(crate) struct SyntheticDelta {
    pub bids: Vec<(f64, f64)>,
    pub asks: Vec<(f64, f64)>,
}

/// A fully-assembled orderbook ready for rendering.
pub(crate) struct EmittedBook {
    /// `(price, size)` sorted descending (best bid first).
    pub bids: Vec<(f64, f64)>,
    /// `(price, size)` sorted ascending (best ask first).
    pub asks: Vec<(f64, f64)>,
    pub timestamp: i64,
}

// ─────────────────────────────────────────────────────────────────────────────
// DEPTH BOOK
// ─────────────────────────────────────────────────────────────────────────────

/// Maintains a local order book from WS snapshots and WS deltas.
///
/// No REST-based stitching — the WS stream provides both initial snapshots
/// and subsequent deltas on all supported exchanges.
pub(crate) struct DepthBook {
    book: LocalBook,
    seeded: bool,
}

impl DepthBook {
    pub(crate) fn new() -> Self {
        Self {
            book: LocalBook::new(),
            seeded: false,
        }
    }

    /// Feed a WS snapshot — replaces the entire book.
    ///
    /// Returns `(EmittedBook, None)` on the first snapshot (no previous state to diff).
    /// Returns `(EmittedBook, Some(SyntheticDelta))` on subsequent snapshots.
    pub(crate) fn feed_snapshot(
        &mut self,
        snapshot: &OrderBook,
        emit_levels: usize,
    ) -> (EmittedBook, Option<SyntheticDelta>) {
        let bids: Vec<(f64, f64)> = snapshot.bids.iter().map(|l| (l.price, l.size)).collect();
        let asks: Vec<(f64, f64)> = snapshot.asks.iter().map(|l| (l.price, l.size)).collect();
        let delta = if self.seeded {
            Some(self.book.diff_against(&bids, &asks))
        } else {
            None
        };
        self.book.seed(&bids, &asks, snapshot.timestamp);
        self.seeded = true;
        (self.book.emit_top_n(emit_levels), delta)
    }

    /// Feed a WS delta — applies incremental updates.
    /// Returns `None` if the book hasn't been seeded yet (no snapshot received).
    pub(crate) fn feed_delta(
        &mut self,
        delta: &OrderbookDelta,
        emit_levels: usize,
    ) -> Option<EmittedBook> {
        if !self.seeded {
            return None;
        }
        let bids: Vec<(f64, f64)> = delta.bids.iter().map(|l| (l.price, l.size)).collect();
        let asks: Vec<(f64, f64)> = delta.asks.iter().map(|l| (l.price, l.size)).collect();
        self.book.apply_delta(&bids, &asks, delta.timestamp);
        Some(self.book.emit_top_n(emit_levels))
    }

    /// Whether the book has been seeded with at least one snapshot.
    pub(crate) fn is_seeded(&self) -> bool {
        self.seeded
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TESTS
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use digdigdig3::core::types::OrderBookLevel;

    fn make_level(price: f64, size: f64) -> OrderBookLevel {
        OrderBookLevel {
            price,
            size,
            order_count: None,
        }
    }

    fn make_snapshot(bids: Vec<(f64, f64)>, asks: Vec<(f64, f64)>) -> OrderBook {
        OrderBook {
            bids: bids.into_iter().map(|(p, s)| make_level(p, s)).collect(),
            asks: asks.into_iter().map(|(p, s)| make_level(p, s)).collect(),
            timestamp: 1000,
            sequence: None,
            last_update_id: None,
            first_update_id: None,
            prev_update_id: None,
            event_time: None,
            transaction_time: None,
            checksum: None,
        }
    }

    fn make_delta(bids: Vec<(f64, f64)>, asks: Vec<(f64, f64)>) -> OrderbookDelta {
        OrderbookDelta {
            bids: bids.into_iter().map(|(p, s)| make_level(p, s)).collect(),
            asks: asks.into_iter().map(|(p, s)| make_level(p, s)).collect(),
            timestamp: 2000,
            first_update_id: None,
            last_update_id: None,
            prev_update_id: None,
            event_time: None,
            checksum: None,
        }
    }

    #[test]
    fn test_delta_before_snapshot_returns_none() {
        let mut book = DepthBook::new();
        let delta = make_delta(vec![(100.0, 1.0)], vec![(101.0, 1.0)]);
        assert!(book.feed_delta(&delta, EMIT_LEVELS).is_none());
        assert!(!book.is_seeded());
    }

    #[test]
    fn test_snapshot_seeds_book() {
        let mut book = DepthBook::new();
        let snap = make_snapshot(vec![(100.0, 1.0), (99.0, 2.0)], vec![(101.0, 1.5)]);
        let (emitted, _delta) = book.feed_snapshot(&snap, EMIT_LEVELS);
        assert!(book.is_seeded());
        assert_eq!(emitted.bids.len(), 2);
        assert_eq!(emitted.bids[0], (100.0, 1.0)); // best bid first
        assert_eq!(emitted.asks.len(), 1);
    }

    #[test]
    fn test_delta_applied_after_snapshot() {
        let mut book = DepthBook::new();
        let snap = make_snapshot(vec![(100.0, 1.0)], vec![(101.0, 1.0)]);
        book.feed_snapshot(&snap, EMIT_LEVELS);

        let delta = make_delta(vec![(100.0, 5.0)], vec![]);
        let result = book.feed_delta(&delta, EMIT_LEVELS).unwrap();
        assert_eq!(result.bids[0], (100.0, 5.0));
    }

    #[test]
    fn test_level_removal() {
        let mut book = DepthBook::new();
        let snap = make_snapshot(vec![(100.0, 1.0), (99.0, 2.0)], vec![(101.0, 1.0)]);
        book.feed_snapshot(&snap, EMIT_LEVELS);

        // Remove bid at 100.0 by sending size = 0.
        let delta = make_delta(vec![(100.0, 0.0)], vec![]);
        let result = book.feed_delta(&delta, EMIT_LEVELS).unwrap();
        assert!(result.bids.iter().all(|(p, _)| *p != 100.0));
    }

    #[test]
    fn test_snapshot_replaces_book() {
        let mut book = DepthBook::new();
        let snap1 = make_snapshot(vec![(100.0, 1.0)], vec![(101.0, 1.0)]);
        book.feed_snapshot(&snap1, EMIT_LEVELS);

        // New snapshot should completely replace the book.
        let snap2 = make_snapshot(vec![(200.0, 3.0)], vec![(201.0, 3.0)]);
        let (emitted, _delta) = book.feed_snapshot(&snap2, EMIT_LEVELS);
        assert_eq!(emitted.bids.len(), 1);
        assert_eq!(emitted.bids[0], (200.0, 3.0));
        assert!(emitted.bids.iter().all(|(p, _)| *p != 100.0));
    }

    #[test]
    fn test_no_delta_on_first_snapshot() {
        let mut book = DepthBook::new();
        let snap = make_snapshot(vec![(100.0, 1.0)], vec![(101.0, 1.0)]);
        let (_emitted, delta) = book.feed_snapshot(&snap, EMIT_LEVELS);
        assert!(delta.is_none(), "first snapshot must not produce a delta");
    }

    #[test]
    fn test_synthetic_delta_on_second_snapshot() {
        let mut book = DepthBook::new();

        // Seed: bids [100@1, 99@2], asks [101@1, 102@3]
        let snap1 = make_snapshot(
            vec![(100.0, 1.0), (99.0, 2.0)],
            vec![(101.0, 1.0), (102.0, 3.0)],
        );
        book.feed_snapshot(&snap1, EMIT_LEVELS);

        // New snapshot:
        //   bids: 100@5 (changed), 98@4 (new), 99 removed
        //   asks: 101@1 (unchanged), 103@7 (new), 102 removed
        let snap2 = make_snapshot(
            vec![(100.0, 5.0), (98.0, 4.0)],
            vec![(101.0, 1.0), (103.0, 7.0)],
        );
        let (_emitted, delta) = book.feed_snapshot(&snap2, EMIT_LEVELS);
        let delta = delta.expect("second snapshot must produce a delta");

        // Changed bid: 100@5
        assert!(
            delta.bids.iter().any(|&(p, s)| p == 100.0 && s == 5.0),
            "expected changed bid 100@5 in delta"
        );
        // New bid: 98@4
        assert!(
            delta.bids.iter().any(|&(p, s)| p == 98.0 && s == 4.0),
            "expected new bid 98@4 in delta"
        );
        // Removed bid: 99@0
        assert!(
            delta.bids.iter().any(|&(p, s)| p == 99.0 && s == 0.0),
            "expected removed bid 99@0.0 in delta"
        );
        // Unchanged ask 101 must NOT appear
        assert!(
            !delta.asks.iter().any(|&(p, _)| p == 101.0),
            "unchanged ask 101 must not appear in delta"
        );
        // New ask: 103@7
        assert!(
            delta.asks.iter().any(|&(p, s)| p == 103.0 && s == 7.0),
            "expected new ask 103@7 in delta"
        );
        // Removed ask: 102@0
        assert!(
            delta.asks.iter().any(|&(p, s)| p == 102.0 && s == 0.0),
            "expected removed ask 102@0.0 in delta"
        );
    }

    #[test]
    fn test_identical_snapshots_empty_delta() {
        let mut book = DepthBook::new();
        let snap = make_snapshot(vec![(100.0, 1.0), (99.0, 2.0)], vec![(101.0, 1.5)]);
        book.feed_snapshot(&snap, EMIT_LEVELS);

        // Feed the exact same snapshot again.
        let snap2 = make_snapshot(vec![(100.0, 1.0), (99.0, 2.0)], vec![(101.0, 1.5)]);
        let (_emitted, delta) = book.feed_snapshot(&snap2, EMIT_LEVELS);
        let delta = delta.expect("second snapshot must produce a delta");
        assert!(
            delta.bids.is_empty(),
            "identical bids must produce empty delta"
        );
        assert!(
            delta.asks.is_empty(),
            "identical asks must produce empty delta"
        );
    }
}
