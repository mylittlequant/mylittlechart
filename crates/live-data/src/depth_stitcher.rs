//! Orderbook depth stitcher — maintains a local order book from
//! REST snapshots + WS diff stream deltas (Binance `@depth` protocol).
//!
//! ## Protocol (Binance diff stream stitching)
//!
//! 1. Subscribe to `<symbol>@depth@100ms` before fetching the REST snapshot.
//! 2. Fetch REST snapshot (`GET /api/v3/depth?symbol=X&limit=1000`).
//! 3. Drop buffered deltas whose `lastUpdateId` ≤ snapshot's `lastUpdateId`.
//! 4. Find the first remaining delta where:
//!    `firstUpdateId ≤ snapshotLastUpdateId + 1 ≤ lastUpdateId`
//! 5. Apply that delta and all subsequent ones in order.
//! 6. On gap: `firstUpdateId > lastAppliedUid + 1` → re-bootstrap.

use std::collections::{BTreeMap, VecDeque};

use digdigdig3::core::types::{OrderBook, OrderbookDelta};

// ─────────────────────────────────────────────────────────────────────────────
// CONSTANTS
// ─────────────────────────────────────────────────────────────────────────────

const MAX_PENDING_DELTAS: usize = 1000;
pub(crate) const EMIT_LEVELS: usize = 500;
const MIN_SNAPSHOT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

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
    /// price → size; highest price is best bid (iterate in reverse for desc order)
    bids: BTreeMap<OrderedFloat, f64>,
    /// price → size; lowest price is best ask (iterate forward for asc order)
    asks: BTreeMap<OrderedFloat, f64>,
    last_update_id: u64,
    timestamp: i64,
}

impl LocalBook {
    fn new(last_update_id: u64, timestamp: i64) -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            last_update_id,
            timestamp,
        }
    }

    /// Apply a batch of bid/ask level updates.
    /// A size of 0.0 means the price level should be removed.
    fn apply_levels(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], timestamp: i64) {
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
    fn emit_top_n(&self, n: usize) -> ReconstructedBook {
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
        ReconstructedBook {
            bids,
            asks,
            timestamp: self.timestamp,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PUBLIC TYPES
// ─────────────────────────────────────────────────────────────────────────────

/// A fully-stitched orderbook snapshot ready for rendering.
pub(crate) struct ReconstructedBook {
    /// `(price, size)` sorted descending (best bid first).
    pub bids: Vec<(f64, f64)>,
    /// `(price, size)` sorted ascending (best ask first).
    pub asks: Vec<(f64, f64)>,
    pub timestamp: i64,
}

// ─────────────────────────────────────────────────────────────────────────────
// INTERNAL TYPES
// ─────────────────────────────────────────────────────────────────────────────

enum StitchPhase {
    AwaitingSnapshot,
    Live,
    Resyncing,
}

struct BufferedDelta {
    bids: Vec<(f64, f64)>,
    asks: Vec<(f64, f64)>,
    first_update_id: u64,
    last_update_id: u64,
    timestamp: i64,
}

// ─────────────────────────────────────────────────────────────────────────────
// DEPTH STITCHER
// ─────────────────────────────────────────────────────────────────────────────

/// State machine that maintains a local order book from REST snapshots and
/// WS diff stream deltas according to the Binance `@depth` protocol.
pub(crate) struct DepthStitcher {
    local_book: Option<LocalBook>,
    pending_deltas: VecDeque<BufferedDelta>,
    phase: StitchPhase,
    last_applied_uid: u64,
    snapshot_requested: bool,
    last_snapshot_time: std::time::Instant,
}

impl DepthStitcher {
    pub(crate) fn new() -> Self {
        Self {
            local_book: None,
            pending_deltas: VecDeque::new(),
            phase: StitchPhase::AwaitingSnapshot,
            last_applied_uid: 0,
            snapshot_requested: false,
            last_snapshot_time: std::time::Instant::now(),
        }
    }

    /// Returns `true` when the caller should initiate a REST snapshot fetch.
    ///
    /// The condition is: we are not yet live AND we haven't already issued a
    /// request AND at least [`MIN_SNAPSHOT_INTERVAL`] has passed since the
    /// last request (rate-limit guard).
    pub(crate) fn needs_snapshot(&self) -> bool {
        let phase_needs = matches!(self.phase, StitchPhase::AwaitingSnapshot | StitchPhase::Resyncing);
        phase_needs
            && !self.snapshot_requested
            && self.last_snapshot_time.elapsed() >= MIN_SNAPSHOT_INTERVAL
    }

    /// Call this immediately after dispatching the REST snapshot request so
    /// the stitcher doesn't issue duplicate requests.
    pub(crate) fn mark_snapshot_requested(&mut self) {
        self.snapshot_requested = true;
        self.last_snapshot_time = std::time::Instant::now();
    }

    /// Feed a diff-stream delta into the state machine.
    ///
    /// Returns `Some(ReconstructedBook)` when the book was updated and the
    /// caller should propagate the new state, `None` otherwise.
    pub(crate) fn feed_delta(
        &mut self,
        delta: &OrderbookDelta,
        emit_levels: usize,
    ) -> Option<ReconstructedBook> {
        let first_uid = delta.first_update_id?;
        let last_uid = delta.last_update_id?;

        match self.phase {
            StitchPhase::AwaitingSnapshot | StitchPhase::Resyncing => {
                // Buffer delta; drop oldest if queue is full.
                if self.pending_deltas.len() >= MAX_PENDING_DELTAS {
                    self.pending_deltas.pop_front();
                }
                self.pending_deltas.push_back(BufferedDelta {
                    bids: delta.bids.iter().map(|l| (l.price, l.size)).collect(),
                    asks: delta.asks.iter().map(|l| (l.price, l.size)).collect(),
                    first_update_id: first_uid,
                    last_update_id: last_uid,
                    timestamp: delta.timestamp,
                });
                None
            }

            StitchPhase::Live => {
                // Stale: already incorporated.
                if last_uid <= self.last_applied_uid {
                    return None;
                }

                // Gap detected: sequence broke.
                if first_uid > self.last_applied_uid + 1 {
                    eprintln!(
                        "[DepthStitcher] gap detected: expected uid {} but got first_uid {}; resyncing",
                        self.last_applied_uid + 1,
                        first_uid
                    );
                    self.phase = StitchPhase::Resyncing;
                    self.snapshot_requested = false;
                    // Buffer this delta so it can be replayed after the next snapshot.
                    self.pending_deltas.push_back(BufferedDelta {
                        bids: delta.bids.iter().map(|l| (l.price, l.size)).collect(),
                        asks: delta.asks.iter().map(|l| (l.price, l.size)).collect(),
                        first_update_id: first_uid,
                        last_update_id: last_uid,
                        timestamp: delta.timestamp,
                    });
                    return None;
                }

                // Normal path: apply delta.
                if let Some(book) = &mut self.local_book {
                    let bids: Vec<(f64, f64)> =
                        delta.bids.iter().map(|l| (l.price, l.size)).collect();
                    let asks: Vec<(f64, f64)> =
                        delta.asks.iter().map(|l| (l.price, l.size)).collect();
                    book.apply_levels(&bids, &asks, delta.timestamp);
                    book.last_update_id = last_uid;
                    self.last_applied_uid = last_uid;
                    Some(book.emit_top_n(emit_levels))
                } else {
                    // Should not happen in Live phase, but guard defensively.
                    None
                }
            }
        }
    }

    /// Apply a REST snapshot to seed the local book, then replay any buffered
    /// deltas that follow it in sequence.
    ///
    /// Returns `Some(ReconstructedBook)` when the book was successfully seeded
    /// (and optionally updated by buffered deltas), `None` if the qualifying
    /// delta has not arrived yet (caller should try again after the next delta).
    pub(crate) fn apply_rest_snapshot(
        &mut self,
        snapshot: OrderBook,
        emit_levels: usize,
    ) -> Option<ReconstructedBook> {
        let snap_uid = snapshot.last_update_id?;

        // Drain stale buffered deltas that are entirely covered by the snapshot.
        self.pending_deltas
            .retain(|d| d.last_update_id > snap_uid);

        // Find the first qualifying delta: firstUpdateId ≤ snapUid + 1 ≤ lastUpdateId
        let qualifying_idx = self
            .pending_deltas
            .iter()
            .position(|d| d.first_update_id <= snap_uid + 1 && snap_uid + 1 <= d.last_update_id);

        let start_idx = match qualifying_idx {
            Some(idx) => idx,
            None => {
                // No qualifying delta yet — stay in current phase and wait.
                return None;
            }
        };

        // Seed the local book from the snapshot.
        let timestamp = snapshot.timestamp;
        let mut book = LocalBook::new(snap_uid, timestamp);
        let snap_bids: Vec<(f64, f64)> =
            snapshot.bids.iter().map(|l| (l.price, l.size)).collect();
        let snap_asks: Vec<(f64, f64)> =
            snapshot.asks.iter().map(|l| (l.price, l.size)).collect();
        book.apply_levels(&snap_bids, &snap_asks, timestamp);

        // Apply the qualifying delta and all subsequent buffered deltas.
        let mut last_applied = snap_uid;
        let deltas_to_apply: Vec<&BufferedDelta> = self
            .pending_deltas
            .iter()
            .skip(start_idx)
            .collect();

        for delta in deltas_to_apply {
            book.apply_levels(&delta.bids, &delta.asks, delta.timestamp);
            book.last_update_id = delta.last_update_id;
            last_applied = delta.last_update_id;
        }

        // All buffered deltas have been applied — clear the queue.
        self.pending_deltas.clear();

        self.last_applied_uid = last_applied;
        self.phase = StitchPhase::Live;
        self.snapshot_requested = false;

        let result = book.emit_top_n(emit_levels);
        self.local_book = Some(book);
        Some(result)
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

    fn make_snapshot(last_update_id: u64, bids: Vec<(f64, f64)>, asks: Vec<(f64, f64)>) -> OrderBook {
        OrderBook {
            bids: bids.into_iter().map(|(p, s)| make_level(p, s)).collect(),
            asks: asks.into_iter().map(|(p, s)| make_level(p, s)).collect(),
            timestamp: 1000,
            sequence: None,
            last_update_id: Some(last_update_id),
            first_update_id: None,
            prev_update_id: None,
            event_time: None,
            transaction_time: None,
            checksum: None,
        }
    }

    fn make_delta(
        first_uid: u64,
        last_uid: u64,
        bids: Vec<(f64, f64)>,
        asks: Vec<(f64, f64)>,
    ) -> OrderbookDelta {
        OrderbookDelta {
            bids: bids.into_iter().map(|(p, s)| make_level(p, s)).collect(),
            asks: asks.into_iter().map(|(p, s)| make_level(p, s)).collect(),
            timestamp: 2000,
            first_update_id: Some(first_uid),
            last_update_id: Some(last_uid),
            prev_update_id: None,
            event_time: None,
            checksum: None,
        }
    }

    // ── Test: initial state needs snapshot, delta returns None ────────────────

    #[test]
    fn test_initial_state_needs_snapshot() {
        let mut stitcher = DepthStitcher::new();
        // Immediately after construction the 1-second guard hasn't elapsed,
        // so needs_snapshot() is false until we back-date last_snapshot_time.
        stitcher.last_snapshot_time =
            std::time::Instant::now() - std::time::Duration::from_secs(2);
        assert!(stitcher.needs_snapshot());
    }

    #[test]
    fn test_delta_returns_none_before_snapshot() {
        let mut stitcher = DepthStitcher::new();
        let delta = make_delta(1, 5, vec![(100.0, 1.0)], vec![(101.0, 1.0)]);
        assert!(stitcher.feed_delta(&delta, EMIT_LEVELS).is_none());
    }

    // ── Test: snapshot seeds book correctly ───────────────────────────────────

    #[test]
    fn test_snapshot_seeds_book() {
        let mut stitcher = DepthStitcher::new();

        // Buffer a qualifying delta first.
        let delta = make_delta(5, 10, vec![(99.0, 2.0)], vec![(101.0, 3.0)]);
        stitcher.feed_delta(&delta, EMIT_LEVELS);

        let snap = make_snapshot(7, vec![(100.0, 1.0), (99.0, 0.5)], vec![(101.0, 1.5)]);
        let book = stitcher.apply_rest_snapshot(snap, EMIT_LEVELS).unwrap();

        // After snapshot + delta replay: bid at 99.0 should be 2.0 (delta overwrote 0.5).
        let bid_99 = book.bids.iter().find(|(p, _)| *p == 99.0);
        assert!(bid_99.is_some());
        assert_eq!(bid_99.unwrap().1, 2.0);
    }

    // ── Test: delta applied after snapshot ────────────────────────────────────

    #[test]
    fn test_delta_applied_after_snapshot() {
        let mut stitcher = DepthStitcher::new();

        // Qualifying delta covers snapshot uid+1.
        let delta = make_delta(5, 10, vec![], vec![]);
        stitcher.feed_delta(&delta, EMIT_LEVELS);

        let snap = make_snapshot(7, vec![(100.0, 1.0)], vec![(101.0, 1.0)]);
        stitcher.apply_rest_snapshot(snap, EMIT_LEVELS).unwrap();

        // Now Live — new delta should be applied.
        let delta2 = make_delta(11, 15, vec![(100.0, 5.0)], vec![]);
        let result = stitcher.feed_delta(&delta2, EMIT_LEVELS);
        assert!(result.is_some());
        let book = result.unwrap();
        let bid_100 = book.bids.iter().find(|(p, _)| *p == 100.0).unwrap();
        assert_eq!(bid_100.1, 5.0);
    }

    // ── Test: stale delta dropped ─────────────────────────────────────────────

    #[test]
    fn test_stale_delta_dropped() {
        let mut stitcher = DepthStitcher::new();

        let qualifying = make_delta(5, 10, vec![], vec![]);
        stitcher.feed_delta(&qualifying, EMIT_LEVELS);

        let snap = make_snapshot(7, vec![(100.0, 1.0)], vec![(101.0, 1.0)]);
        stitcher.apply_rest_snapshot(snap, EMIT_LEVELS).unwrap();

        // Delta with last_uid <= last_applied_uid should be dropped.
        let stale = make_delta(3, 8, vec![(100.0, 999.0)], vec![]);
        let result = stitcher.feed_delta(&stale, EMIT_LEVELS);
        assert!(result.is_none());

        // Book price should not have changed to 999.
        let new_delta = make_delta(11, 12, vec![], vec![]);
        let result2 = stitcher.feed_delta(&new_delta, EMIT_LEVELS);
        if let Some(book) = result2 {
            let bid_100 = book.bids.iter().find(|(p, _)| *p == 100.0).unwrap();
            assert_eq!(bid_100.1, 1.0);
        }
    }

    // ── Test: gap triggers resync ──────────────────────────────────────────────

    #[test]
    fn test_gap_triggers_resync() {
        let mut stitcher = DepthStitcher::new();

        let qualifying = make_delta(5, 10, vec![], vec![]);
        stitcher.feed_delta(&qualifying, EMIT_LEVELS);

        let snap = make_snapshot(7, vec![(100.0, 1.0)], vec![(101.0, 1.0)]);
        stitcher.apply_rest_snapshot(snap, EMIT_LEVELS).unwrap();

        // Feed a delta that has a gap (first_uid skips ahead).
        let gap_delta = make_delta(20, 25, vec![], vec![]);
        let result = stitcher.feed_delta(&gap_delta, EMIT_LEVELS);
        assert!(result.is_none());

        // After gap, needs_snapshot should become true (once interval elapses).
        stitcher.last_snapshot_time =
            std::time::Instant::now() - std::time::Duration::from_secs(2);
        assert!(stitcher.needs_snapshot());
    }

    // ── Test: buffer overflow capped at 1000 ──────────────────────────────────

    #[test]
    fn test_buffer_overflow_capped() {
        let mut stitcher = DepthStitcher::new();

        for i in 0..1100u64 {
            let delta = make_delta(i, i + 1, vec![], vec![]);
            stitcher.feed_delta(&delta, EMIT_LEVELS);
        }

        assert_eq!(stitcher.pending_deltas.len(), MAX_PENDING_DELTAS);
        // Oldest entries should have been dropped; first entry should be from i=100.
        assert_eq!(stitcher.pending_deltas.front().unwrap().first_update_id, 100);
    }

    // ── Test: level removal (size == 0) ───────────────────────────────────────

    #[test]
    fn test_level_removal() {
        let mut stitcher = DepthStitcher::new();

        let qualifying = make_delta(5, 10, vec![], vec![]);
        stitcher.feed_delta(&qualifying, EMIT_LEVELS);

        let snap = make_snapshot(7, vec![(100.0, 1.0), (99.0, 2.0)], vec![(101.0, 1.0)]);
        stitcher.apply_rest_snapshot(snap, EMIT_LEVELS).unwrap();

        // Remove bid at 100.0 by sending size = 0.
        let remove_delta = make_delta(11, 12, vec![(100.0, 0.0)], vec![]);
        let result = stitcher.feed_delta(&remove_delta, EMIT_LEVELS).unwrap();

        let bid_100 = result.bids.iter().find(|(p, _)| *p == 100.0);
        assert!(bid_100.is_none(), "price level 100.0 should have been removed");
    }

    // ── Test: pending deltas drained on snapshot ──────────────────────────────

    #[test]
    fn test_pending_deltas_drained_on_snapshot() {
        let mut stitcher = DepthStitcher::new();

        // Buffer deltas: some stale, some qualifying, some post-qualifying.
        let d1 = make_delta(1, 3, vec![], vec![]);  // stale (before snap uid=5)
        let d2 = make_delta(4, 6, vec![], vec![]);  // qualifying (4 ≤ 6 ≤ 6)
        let d3 = make_delta(7, 9, vec![], vec![]);  // post-qualifying
        stitcher.feed_delta(&d1, EMIT_LEVELS);
        stitcher.feed_delta(&d2, EMIT_LEVELS);
        stitcher.feed_delta(&d3, EMIT_LEVELS);

        let snap = make_snapshot(5, vec![(100.0, 1.0)], vec![(101.0, 1.0)]);
        let result = stitcher.apply_rest_snapshot(snap, EMIT_LEVELS);
        assert!(result.is_some());
        assert!(stitcher.pending_deltas.is_empty());
    }

    // ── Test: no qualifying delta stays in current phase ─────────────────────

    #[test]
    fn test_no_qualifying_delta_stays_in_phase() {
        let mut stitcher = DepthStitcher::new();

        // Only stale deltas buffered; no qualifying delta for snap uid=10.
        let d1 = make_delta(1, 3, vec![], vec![]);
        stitcher.feed_delta(&d1, EMIT_LEVELS);

        let snap = make_snapshot(10, vec![(100.0, 1.0)], vec![]);
        let result = stitcher.apply_rest_snapshot(snap, EMIT_LEVELS);
        assert!(result.is_none(), "should return None when no qualifying delta");

        // Must still be able to request another snapshot.
        stitcher.last_snapshot_time =
            std::time::Instant::now() - std::time::Duration::from_secs(2);
        assert!(stitcher.needs_snapshot());
    }

    // ── Test: mark_snapshot_requested prevents duplicate requests ─────────────

    #[test]
    fn test_mark_snapshot_requested() {
        let mut stitcher = DepthStitcher::new();
        stitcher.last_snapshot_time =
            std::time::Instant::now() - std::time::Duration::from_secs(2);

        assert!(stitcher.needs_snapshot());
        stitcher.mark_snapshot_requested();
        assert!(!stitcher.needs_snapshot());
    }
}
