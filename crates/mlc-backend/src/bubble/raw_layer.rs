//! `RawLayer<T>` — bounded chronological ring buffer for raw per-instrument
//! data (kline bars per timeframe, trades), plus `RawTrade`.
//!
//! Unified-data-plane Stage Э1 (`nemo/docs/mlc/plans/unified-data-plane-2026-07-09.md`):
//! the bubble gains a raw storage layer PER touched kline timeframe plus one
//! trade layer, filled continuously by live WS ticks and REST backfill,
//! independent of whatever chart_type/param/TF the ACTIVE render series is
//! currently on. Nothing evicts entries except capacity overflow
//! (drop-oldest) — chart-type switches, param changes, TF switches on other
//! windows must never clear these. Only a genuine instrument reassignment
//! (symbol switch) resets a bubble's layers, because at that point the data
//! belongs to a different instrument entirely (see `BubbleStore::set_instrument`
//! family in `store.rs`).
//!
//! Kept deliberately dumb: no derived recompute, no callbacks, no viewport
//! awareness. Just correct chronological storage with a ts-keyed identity
//! for live-tick replace-in-place vs append, and dedup at the prepend seam
//! for REST backfill pages.

use std::collections::VecDeque;

/// One raw trade tick, matching the fields
/// [`mlc_backend_data_bridge::bridge::LiveUpdate::TradeUpdate`] carries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawTrade {
    pub ts_ms: i64,
    pub price: f64,
    pub qty: f64,
    pub is_buyer_maker: bool,
}

/// A value with a chronological identity — the key `RawLayer` uses to tell
/// "this is the same point, replace it" from "this is a new point, append
/// it" for live ticks, and to dedup at the prepend seam for backfill pages.
pub trait ChronoKey {
    /// Monotonic identity for this point. For klines this is the bar's
    /// wall-clock open time; for trades it's the trade timestamp. Ticks
    /// sharing the same key are the SAME point (e.g. a kline's in-progress
    /// last bar getting updated tick-by-tick) — `push_live` replaces rather
    /// than appends when the key matches the current last entry.
    fn chrono_key(&self) -> i64;
}

impl ChronoKey for mlc_core::Bar {
    fn chrono_key(&self) -> i64 {
        if self.derived_ts_ms != 0 { self.derived_ts_ms } else { self.timestamp }
    }
}

impl ChronoKey for RawTrade {
    fn chrono_key(&self) -> i64 {
        self.ts_ms
    }
}

/// Bounded chronological ring buffer. Oldest-first ordering is an
/// invariant maintained by every mutator — callers must not bypass
/// `push_live`/`prepend_batch`.
#[derive(Debug, Clone)]
pub struct RawLayer<T> {
    points: VecDeque<T>,
    capacity: usize,
}

impl<T: ChronoKey> RawLayer<T> {
    /// New empty layer with the given ring capacity. `capacity == 0` means
    /// unbounded (no eviction) — mirrors `DataLoadSettings::max_loaded_bars`'s
    /// "0 = unlimited" convention.
    pub fn new(capacity: usize) -> Self {
        Self { points: VecDeque::new(), capacity }
    }

    /// Ring capacity. `0` means unbounded.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Change the ring capacity in place. If the new capacity is smaller
    /// than the current length, evicts from the front (drop-oldest) to fit
    /// — same eviction direction `push_live` uses.
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
        self.evict_overflow();
    }

    /// Number of points currently stored.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Oldest point's chronological key, if any.
    pub fn oldest_ts(&self) -> Option<i64> {
        self.points.front().map(|p| p.chrono_key())
    }

    /// Newest point's chronological key, if any.
    pub fn newest_ts(&self) -> Option<i64> {
        self.points.back().map(|p| p.chrono_key())
    }

    /// Read-only view of every point, oldest-first.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.points.iter()
    }

    /// Full chronological snapshot as a `Vec`, oldest-first.
    pub fn snapshot(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.points.iter().cloned().collect()
    }

    /// Append a live point, or replace the current last entry if its
    /// chronological key matches (the in-progress-bar / same-trade-tick
    /// case). Evicts the oldest entry on overflow (drop-oldest, ring
    /// semantics) — never evicts to make room for a same-key replace since
    /// that doesn't grow the ring.
    pub fn push_live(&mut self, point: T) {
        let key = point.chrono_key();
        if let Some(last) = self.points.back_mut() {
            if last.chrono_key() == key {
                *last = point;
                return;
            }
        }
        self.points.push_back(point);
        self.evict_overflow();
    }

    /// Prepend an older backfill batch in front of the existing series.
    /// `batch` may be in any order — sorted ascending by chrono key first.
    /// Points whose key already exists at the seam (front of the current
    /// ring) are dropped (dedup) rather than duplicated. Evicts from the
    /// NEW back (newest) on overflow — a backfill batch never displaces
    /// live data, since eviction happens at the exact end from which
    /// `push_live` grows the ring in the first place; overflow after a
    /// prepend can only come from the batch itself being larger than
    /// capacity, so trimming its own tail (nearest to "now") is the
    /// correct drop-oldest-from-the-far-end analogue while ensuring the
    /// live edge already in the ring is preserved unconditionally.
    pub fn prepend_batch(&mut self, mut batch: Vec<T>) {
        if batch.is_empty() {
            return;
        }
        batch.sort_by_key(|p| p.chrono_key());

        let seam_key = self.points.front().map(|p| p.chrono_key());
        let mut to_prepend: VecDeque<T> = VecDeque::with_capacity(batch.len());
        for point in batch {
            if seam_key == Some(point.chrono_key()) {
                continue; // dedup at the seam
            }
            to_prepend.push_back(point);
        }
        if to_prepend.is_empty() {
            return;
        }

        // Trim the incoming batch's own newest-first overflow BEFORE
        // splicing, so the live tail already resident in `self.points` is
        // never the side that gets dropped.
        if self.capacity > 0 {
            let total = to_prepend.len() + self.points.len();
            if total > self.capacity {
                let drop_n = total - self.capacity;
                let keep_from_batch = to_prepend.len().saturating_sub(drop_n);
                to_prepend.truncate(keep_from_batch);
            }
        }

        for point in to_prepend.into_iter().rev() {
            self.points.push_front(point);
        }
    }

    fn evict_overflow(&mut self) {
        if self.capacity == 0 {
            return;
        }
        while self.points.len() > self.capacity {
            self.points.pop_front();
        }
    }

    /// Drop every stored point. Used ONLY on genuine instrument reassignment
    /// (symbol switch) — never on chart-type/param/TF switches. See module
    /// doc.
    pub fn clear(&mut self) {
        self.points.clear();
    }

    /// Seed the layer with a fresh "right-edge" block (a Phase-A initial
    /// load / re-subscribe page — always the newest window for the
    /// instrument, not a scroll-left backfill page).
    ///
    /// Two cases:
    /// - Layer empty, or the block overlaps/abuts the layer's current
    ///   newest point within tolerance: each point is folded in via
    ///   [`Self::push_live`] (append-or-replace-last), so a stale-but-
    ///   CONTIGUOUS layer (e.g. a `RampState` that survived a TF round-trip
    ///   and kept extending the SAME instrument older-ward) simply grows —
    ///   this is the common, correct path.
    /// - Layer non-empty AND disjoint from the block (the gap between the
    ///   layer's `newest_ts` and the block's oldest key is more than double
    ///   the block's own typical point-to-point step — i.e. a real time gap
    ///   with nothing bridging it, e.g. the app was closed for a week and
    ///   reopened, or a stale `RampState` ladder left the layer anchored
    ///   months behind): the stale data would otherwise sit UNDER the fresh
    ///   block with a hole between them. Reset the layer to just the fresh
    ///   block instead of storing a gapped ring — see the
    ///   `unified-data-plane` doc + P0 kline-display-corruption fix
    ///   (2026-07-09). A single-point block has no step to measure — treated
    ///   as contiguous (never resets) since one point alone can't establish
    ///   a gap.
    pub fn seed_block(&mut self, block: Vec<T>)
    where
        T: Clone,
    {
        if block.is_empty() {
            return;
        }
        let mut sorted = block;
        sorted.sort_by_key(|p| p.chrono_key());

        if let Some(newest) = self.newest_ts() {
            let block_oldest = sorted[0].chrono_key();
            let typical_step = sorted.windows(2)
                .map(|w| (w[1].chrono_key() - w[0].chrono_key()).max(0))
                .max();
            let disjoint = match typical_step {
                Some(step) if step > 0 => block_oldest - newest > step * 2,
                _ => false,
            };
            if disjoint {
                self.clear();
            }
        }
        for point in sorted {
            self.push_live(point);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trade(ts_ms: i64, price: f64) -> RawTrade {
        RawTrade { ts_ms, price, qty: 1.0, is_buyer_maker: false }
    }

    #[test]
    fn push_live_appends_new_and_replaces_same_key() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(10);
        layer.push_live(trade(100, 1.0));
        layer.push_live(trade(200, 2.0));
        assert_eq!(layer.len(), 2);

        // Same ts_ms as the current last entry -> replace in place, not append.
        layer.push_live(trade(200, 2.5));
        assert_eq!(layer.len(), 2);
        assert_eq!(layer.snapshot(), vec![trade(100, 1.0), trade(200, 2.5)]);
    }

    #[test]
    fn push_live_replace_only_checks_the_last_entry() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(10);
        layer.push_live(trade(100, 1.0));
        layer.push_live(trade(200, 2.0));
        // ts_ms 100 is NOT the last entry anymore -> appended as new, not
        // mistaken for a replace of an older point.
        layer.push_live(trade(100, 9.0));
        assert_eq!(layer.len(), 3);
        assert_eq!(layer.snapshot(), vec![trade(100, 1.0), trade(200, 2.0), trade(100, 9.0)]);
    }

    #[test]
    fn prepend_batch_dedups_at_seam() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(10);
        layer.push_live(trade(300, 3.0));
        layer.push_live(trade(400, 4.0));

        // Batch overlaps the seam at ts=300 — that point must be dropped
        // from the incoming batch, not duplicated.
        layer.prepend_batch(vec![trade(300, 3.0), trade(200, 2.0), trade(100, 1.0)]);

        assert_eq!(layer.len(), 4);
        assert_eq!(
            layer.snapshot(),
            vec![trade(100, 1.0), trade(200, 2.0), trade(300, 3.0), trade(400, 4.0)]
        );
    }

    #[test]
    fn prepend_batch_accepts_unsorted_input() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(10);
        layer.push_live(trade(400, 4.0));

        layer.prepend_batch(vec![trade(300, 3.0), trade(100, 1.0), trade(200, 2.0)]);

        assert_eq!(
            layer.snapshot(),
            vec![trade(100, 1.0), trade(200, 2.0), trade(300, 3.0), trade(400, 4.0)]
        );
    }

    #[test]
    fn capacity_eviction_drops_oldest_on_push_live() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(3);
        layer.push_live(trade(100, 1.0));
        layer.push_live(trade(200, 2.0));
        layer.push_live(trade(300, 3.0));
        assert_eq!(layer.len(), 3);

        // Overflow — oldest (ts=100) must be dropped, not newest.
        layer.push_live(trade(400, 4.0));
        assert_eq!(layer.len(), 3);
        assert_eq!(
            layer.snapshot(),
            vec![trade(200, 2.0), trade(300, 3.0), trade(400, 4.0)]
        );
    }

    #[test]
    fn capacity_eviction_on_prepend_trims_the_batchs_own_far_end_not_the_live_tail() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(3);
        layer.push_live(trade(400, 4.0));
        layer.push_live(trade(500, 5.0));

        // Prepending 3 older points into a capacity-3 layer that already
        // holds 2 live points would overflow to 5 — must trim from the
        // batch's OWN newest end (closest to the seam), never touch the
        // live tail (400, 500).
        layer.prepend_batch(vec![trade(100, 1.0), trade(200, 2.0), trade(300, 3.0)]);

        assert_eq!(layer.len(), 3);
        assert_eq!(
            layer.snapshot(),
            vec![trade(100, 1.0), trade(400, 4.0), trade(500, 5.0)]
        );
    }

    #[test]
    fn zero_capacity_is_unbounded() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(0);
        for i in 0..50 {
            layer.push_live(trade(i, i as f64));
        }
        assert_eq!(layer.len(), 50);
    }

    #[test]
    fn oldest_and_newest_ts_track_the_ring_ends() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(10);
        assert_eq!(layer.oldest_ts(), None);
        assert_eq!(layer.newest_ts(), None);
        layer.push_live(trade(100, 1.0));
        layer.push_live(trade(200, 2.0));
        assert_eq!(layer.oldest_ts(), Some(100));
        assert_eq!(layer.newest_ts(), Some(200));
    }

    #[test]
    fn clear_empties_the_ring() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(10);
        layer.push_live(trade(100, 1.0));
        layer.clear();
        assert!(layer.is_empty());
        assert_eq!(layer.oldest_ts(), None);
    }

    #[test]
    fn seed_block_on_empty_layer_just_inserts() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(10);
        layer.seed_block(vec![trade(200, 2.0), trade(100, 1.0), trade(300, 3.0)]);
        assert_eq!(
            layer.snapshot(),
            vec![trade(100, 1.0), trade(200, 2.0), trade(300, 3.0)]
        );
    }

    #[test]
    fn seed_block_contiguous_with_stale_tail_grows_in_place() {
        // Stale layer ends at ts=100 with step 100 established by (100,200,300..).
        let mut layer: RawLayer<RawTrade> = RawLayer::new(100);
        layer.push_live(trade(100, 1.0));
        layer.push_live(trade(200, 2.0));
        layer.push_live(trade(300, 3.0));

        // Fresh block starts right after the stale tail (step 100, gap 100 <= 2*100).
        layer.seed_block(vec![trade(400, 4.0), trade(500, 5.0)]);

        assert_eq!(
            layer.snapshot(),
            vec![trade(100, 1.0), trade(200, 2.0), trade(300, 3.0), trade(400, 4.0), trade(500, 5.0)],
            "contiguous seed must extend the layer, not reset it"
        );
    }

    #[test]
    fn seed_block_disjoint_from_stale_tail_resets_to_fresh_block() {
        // Stale layer is a ladder from a prior session, months (huge gap) behind.
        let mut layer: RawLayer<RawTrade> = RawLayer::new(100);
        layer.push_live(trade(100, 1.0));
        layer.push_live(trade(200, 2.0));
        layer.push_live(trade(300, 3.0)); // step = 100, newest_ts = 300

        // Fresh PhaseA block is far in the future relative to the established
        // step — a real gap, not a next-page continuation.
        layer.seed_block(vec![trade(100_000, 10.0), trade(100_100, 11.0)]);

        assert_eq!(
            layer.snapshot(),
            vec![trade(100_000, 10.0), trade(100_100, 11.0)],
            "disjoint seed must reset the layer to the fresh block, no gapped ring"
        );
    }

    #[test]
    fn seed_block_single_point_never_resets() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(100);
        layer.push_live(trade(100, 1.0));
        layer.push_live(trade(200, 2.0));

        // A single-point block has no measurable step — treated as
        // contiguous, appended rather than resetting the layer.
        layer.seed_block(vec![trade(100_000, 99.0)]);

        assert_eq!(
            layer.snapshot(),
            vec![trade(100, 1.0), trade(200, 2.0), trade(100_000, 99.0)]
        );
    }
}
