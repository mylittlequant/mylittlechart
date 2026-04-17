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

    /// Merge deeper levels from a REST snapshot without replacing the top of book.
    /// Only adds levels that don't already exist (preserves WS-fresh data).
    fn merge_deep(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)]) {
        for &(price, size) in bids {
            if size > 0.0 {
                self.bids.entry(OrderedFloat(price)).or_insert(size);
            }
        }
        for &(price, size) in asks {
            if size > 0.0 {
                self.asks.entry(OrderedFloat(price)).or_insert(size);
            }
        }
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

// ─────────────────────────────────────────────────────────────────────────────
// PUBLIC TYPES
// ─────────────────────────────────────────────────────────────────────────────

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
    pub(crate) fn feed_snapshot(
        &mut self,
        snapshot: &OrderBook,
        emit_levels: usize,
    ) -> EmittedBook {
        let bids: Vec<(f64, f64)> = snapshot.bids.iter().map(|l| (l.price, l.size)).collect();
        let asks: Vec<(f64, f64)> = snapshot.asks.iter().map(|l| (l.price, l.size)).collect();
        self.book.seed(&bids, &asks, snapshot.timestamp);
        self.seeded = true;
        self.book.emit_top_n(emit_levels)
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

    /// Merge deeper levels from a REST snapshot (background deep fill).
    /// Only adds levels not already in the book.
    pub(crate) fn merge_rest_snapshot(
        &mut self,
        snapshot: &OrderBook,
        emit_levels: usize,
    ) -> Option<EmittedBook> {
        if !self.seeded {
            return None;
        }
        let bids: Vec<(f64, f64)> = snapshot.bids.iter().map(|l| (l.price, l.size)).collect();
        let asks: Vec<(f64, f64)> = snapshot.asks.iter().map(|l| (l.price, l.size)).collect();
        self.book.merge_deep(&bids, &asks);
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
    use digdigdig3::core::types::market_data::OrderBookLevel;

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
        let emitted = book.feed_snapshot(&snap, EMIT_LEVELS);
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
        let emitted = book.feed_snapshot(&snap2, EMIT_LEVELS);
        assert_eq!(emitted.bids.len(), 1);
        assert_eq!(emitted.bids[0], (200.0, 3.0));
        assert!(emitted.bids.iter().all(|(p, _)| *p != 100.0));
    }

    #[test]
    fn test_merge_deep_adds_only_new_levels() {
        let mut book = DepthBook::new();
        let snap = make_snapshot(vec![(100.0, 1.0)], vec![(101.0, 1.0)]);
        book.feed_snapshot(&snap, EMIT_LEVELS);

        // REST snapshot with deeper levels + overlapping 100.0
        let rest = make_snapshot(
            vec![(100.0, 999.0), (98.0, 5.0), (97.0, 10.0)],
            vec![(101.0, 999.0), (102.0, 5.0)],
        );
        let result = book.merge_rest_snapshot(&rest, EMIT_LEVELS).unwrap();

        // 100.0 should keep WS value (1.0), not REST (999.0)
        let bid_100 = result.bids.iter().find(|(p, _)| *p == 100.0).unwrap();
        assert_eq!(bid_100.1, 1.0);
        // 98.0 and 97.0 should be added
        assert!(result.bids.iter().any(|(p, _)| *p == 98.0));
        assert!(result.bids.iter().any(|(p, _)| *p == 97.0));
        // 101.0 should keep WS value
        let ask_101 = result.asks.iter().find(|(p, _)| *p == 101.0).unwrap();
        assert_eq!(ask_101.1, 1.0);
        // 102.0 should be added
        assert!(result.asks.iter().any(|(p, _)| *p == 102.0));
    }
}
