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
//! for live-tick replace-in-place vs append. Insertion is always an ordered
//! merge — `push_live`, `prepend_batch`, and `seed_block` all keep the ring
//! chronologically sorted no matter how the incoming point/batch/block
//! relates to what's already stored (strictly newer, strictly older, or
//! overlapping the interior), so a fresh page that overlaps retained data
//! (e.g. after a chart-type cycle) merges in at the right position instead
//! of gluing a second out-of-order run onto the tail.

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

    /// Append a live point, replace the current last entry if its
    /// chronological key matches (the in-progress-bar / same-trade-tick
    /// case), or — for any other (out-of-order / overlapping-older) key —
    /// fall back to an ordered insert so the ring stays chronologically
    /// sorted no matter what key arrives. Evicts the oldest entry on
    /// overflow (drop-oldest, ring semantics) — never evicts to make room
    /// for a same-key replace since that doesn't grow the ring.
    pub fn push_live(&mut self, point: T) {
        let key = point.chrono_key();
        let last_key = self.points.back().map(|p| p.chrono_key());
        match last_key {
            None => {
                self.points.push_back(point);
                self.evict_overflow();
            }
            Some(last_key) if key == last_key => {
                if let Some(last) = self.points.back_mut() {
                    *last = point;
                }
            }
            Some(last_key) if key > last_key => {
                self.points.push_back(point);
                self.evict_overflow();
            }
            Some(_) => {
                // Out-of-order / overlapping-older key: the fast append-or-
                // replace-last cases above only cover `key >= last_key`.
                // Anything older (a stale block folded in via `seed_block`,
                // a late-arriving tick, etc.) must land at its correct
                // chronological position instead of being appended at the
                // tail — that's exactly the P0 bug this fixes (concatenated-
                // block order breaks).
                self.ordered_insert(point);
                self.evict_overflow();
            }
        }
    }

    /// Insert `point` at its correct chronological position via binary
    /// search on `chrono_key`, replacing an exact-key match in place
    /// (same replace-not-duplicate semantics as the `push_live` fast
    /// path) rather than inserting a duplicate.
    fn ordered_insert(&mut self, point: T) {
        let key = point.chrono_key();
        match self.points.binary_search_by_key(&key, |p| p.chrono_key()) {
            Ok(idx) => self.points[idx] = point,
            Err(idx) => self.points.insert(idx, point),
        }
    }

    /// Merge an older/overlapping backfill batch into the existing series.
    /// `batch` may be in any order and may overlap the current ring at
    /// either end — sorted ascending by chrono key first, then merged
    /// point-by-point so the result is always chronologically sorted with
    /// no duplicate keys, regardless of whether `batch` is a clean
    /// strictly-older prepend or an overlapping block (the concatenated-
    /// block P0: a scroll page or Phase-A reseed that overlaps the
    /// retained layer instead of abutting it exactly).
    ///
    /// Fast path unchanged for the common case (`batch` entirely older
    /// than the current front, i.e. a genuine scroll-left prepend): no
    /// merge needed, just splice at the front. Falls back to a linear
    /// merge-by-key otherwise — correctness first, allocations acceptable
    /// (backfill pages are bounded, not a hot per-tick path).
    ///
    /// Evicts from the NEW back (newest) on overflow when taking the fast
    /// path — a backfill batch never displaces live data, since eviction
    /// happens at the exact end from which `push_live` grows the ring in
    /// the first place; overflow after a prepend can only come from the
    /// batch itself being larger than capacity, so trimming its own tail
    /// (nearest to "now") is the correct drop-oldest-from-the-far-end
    /// analogue while ensuring the live edge already in the ring is
    /// preserved unconditionally. The merge fallback instead evicts
    /// oldest-first after splicing (ordinary ring eviction), since once
    /// the batch overlaps, "the batch's own tail" and "the ring's live
    /// edge" are no longer distinct regions.
    pub fn prepend_batch(&mut self, batch: Vec<T>) -> usize {
        self.prepend_batch_inner(batch)
    }

    fn prepend_batch_inner(&mut self, mut batch: Vec<T>) -> usize {
        if batch.is_empty() {
            return 0;
        }
        batch.sort_by_key(|p| p.chrono_key());

        let front_key = self.points.front().map(|p| p.chrono_key());
        let batch_newest_key = batch.last().map(|p| p.chrono_key());
        let strictly_older = match (front_key, batch_newest_key) {
            (Some(front), Some(newest)) => newest <= front,
            _ => true,
        };

        if strictly_older {
            return self.prepend_batch_fast(batch, front_key);
        }
        self.prepend_batch_merge(batch)
    }

    /// Fast path: `batch` is entirely at or before the current front (no
    /// overlap into the ring's interior) — dedup the exact seam key, then
    /// splice at the front in one pass. Returns the count of NEW points
    /// actually inserted (seam replace does not count as new).
    fn prepend_batch_fast(&mut self, batch: Vec<T>, front_key: Option<i64>) -> usize {
        let mut to_prepend: VecDeque<T> = VecDeque::with_capacity(batch.len());
        for point in batch {
            if front_key == Some(point.chrono_key()) {
                continue; // dedup at the seam
            }
            to_prepend.push_back(point);
        }
        if to_prepend.is_empty() {
            return 0;
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

        let new_count = to_prepend.len();
        for point in to_prepend.into_iter().rev() {
            self.points.push_front(point);
        }
        new_count
    }

    /// Fallback path: `batch` overlaps the ring's interior (not a clean
    /// prepend) — merge the two already-sorted runs (`batch`, `self.points`)
    /// by chrono key into a fresh ordered sequence, deduping on exact-key
    /// match (batch's incoming value wins, mirroring `push_live`'s
    /// replace-on-match semantics for the same key). Returns the count of
    /// keys from `batch` that were not already present in the ring.
    fn prepend_batch_merge(&mut self, batch: Vec<T>) -> usize {
        let existing_keys: std::collections::HashSet<i64> =
            self.points.iter().map(|p| p.chrono_key()).collect();
        let new_count = batch.iter().filter(|p| !existing_keys.contains(&p.chrono_key())).count();

        let mut merged: VecDeque<T> = VecDeque::with_capacity(batch.len() + self.points.len());
        let mut batch_iter = batch.into_iter();
        let mut existing_iter = std::mem::take(&mut self.points).into_iter();
        let mut next_batch = batch_iter.next();
        let mut next_existing = existing_iter.next();

        loop {
            match (next_batch, next_existing) {
                (Some(b), Some(e)) => {
                    let bk = b.chrono_key();
                    let ek = e.chrono_key();
                    if bk < ek {
                        merged.push_back(b);
                        next_batch = batch_iter.next();
                        next_existing = Some(e);
                    } else if bk > ek {
                        merged.push_back(e);
                        next_batch = Some(b);
                        next_existing = existing_iter.next();
                    } else {
                        // Same key on both sides — batch's incoming value
                        // wins (replace semantics), drop the stale one.
                        merged.push_back(b);
                        next_batch = batch_iter.next();
                        next_existing = existing_iter.next();
                    }
                }
                (Some(b), None) => {
                    merged.push_back(b);
                    next_batch = batch_iter.next();
                    next_existing = None;
                }
                (None, Some(e)) => {
                    merged.push_back(e);
                    next_batch = None;
                    next_existing = existing_iter.next();
                }
                (None, None) => break,
            }
        }

        self.points = merged;
        self.evict_overflow();
        new_count
    }

    /// Test-only invariant helper: true iff every stored point's chrono key
    /// is strictly increasing along the ring (oldest-first, no duplicate
    /// keys). Used by the regression tests to assert the P0 fix
    /// (concatenated-block order breaks) can't reoccur.
    #[cfg(test)]
    fn is_sorted(&self) -> bool {
        self.points.iter().map(|p| p.chrono_key()).collect::<Vec<_>>().windows(2).all(|w| w[0] < w[1])
    }

    fn evict_overflow(&mut self) {
        if self.capacity == 0 {
            return;
        }
        while self.points.len() > self.capacity {
            self.points.pop_front();
        }
    }

    /// Drop every point whose chrono key is older than `cutoff_ms` from the
    /// front (oldest end) of the ring — a time-horizon eviction, distinct
    /// from [`Self::evict_overflow`]'s count-capacity eviction. The ring is
    /// kept chronologically sorted by every mutator, so the expired run is
    /// always a contiguous prefix: pop from the front while the oldest key
    /// is still stale.
    pub fn evict_older_than(&mut self, cutoff_ms: i64) {
        while self.points.front().map(|p| p.chrono_key() < cutoff_ms).unwrap_or(false) {
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
    ///   [`Self::push_live`] (append-or-replace-last, or — since
    ///   `push_live` is now an always-ordered insert — a correctly-
    ///   positioned merge for any point that lands OLDER than the layer's
    ///   current tail), so a stale-but-CONTIGUOUS layer (e.g. a
    ///   `RampState` that survived a TF round-trip and kept extending the
    ///   SAME instrument older-ward) simply grows, and an OVERLAPPING
    ///   block (the P0 case: a fresh Phase-A page whose window overlaps
    ///   data already retained from a prior chart-type/TF cycle) merges in
    ///   at the right position instead of gluing a second out-of-order run
    ///   onto the tail — this is the common, correct path.
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
    ///
    /// Returns the count of points from `block` that were actually NEW
    /// (a same-key replace does not count).
    pub fn seed_block(&mut self, block: Vec<T>) -> usize
    where
        T: Clone,
    {
        if block.is_empty() {
            return 0;
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
        let existing_keys: std::collections::HashSet<i64> =
            self.points.iter().map(|p| p.chrono_key()).collect();
        let new_count = sorted.iter().filter(|p| !existing_keys.contains(&p.chrono_key())).count();
        for point in sorted {
            self.push_live(point);
        }
        new_count
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
    fn push_live_matching_a_non_last_key_replaces_that_entry_not_appends() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(10);
        layer.push_live(trade(100, 1.0));
        layer.push_live(trade(200, 2.0));
        // ts_ms 100 is no longer the last entry, but it's still an exact
        // key match for an existing point — the always-ordered-merge
        // invariant means this replaces that point in place rather than
        // appending a duplicate key at the tail (the pre-fix behavior,
        // which broke chronological order).
        layer.push_live(trade(100, 9.0));
        assert!(layer.is_sorted());
        assert_eq!(layer.len(), 2);
        assert_eq!(layer.snapshot(), vec![trade(100, 9.0), trade(200, 2.0)]);
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

    #[test]
    fn push_live_out_of_order_single_is_inserted_in_place() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(10);
        layer.push_live(trade(100, 1.0));
        layer.push_live(trade(300, 3.0));
        layer.push_live(trade(400, 4.0));

        // A late-arriving tick older than the current tail must be inserted
        // at its sorted position, not appended past the newer entries.
        layer.push_live(trade(200, 2.0));

        assert!(layer.is_sorted());
        assert_eq!(
            layer.snapshot(),
            vec![trade(100, 1.0), trade(200, 2.0), trade(300, 3.0), trade(400, 4.0)]
        );
    }

    #[test]
    fn push_live_out_of_order_replaces_exact_key_match() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(10);
        layer.push_live(trade(100, 1.0));
        layer.push_live(trade(200, 2.0));
        layer.push_live(trade(300, 3.0));

        // Out-of-order key that exactly matches an interior point must
        // replace it in place, not insert a duplicate.
        layer.push_live(trade(200, 9.0));

        assert!(layer.is_sorted());
        assert_eq!(layer.len(), 3);
        assert_eq!(
            layer.snapshot(),
            vec![trade(100, 1.0), trade(200, 9.0), trade(300, 3.0)]
        );
    }

    #[test]
    fn prepend_batch_overlapping_interior_merges_deduped_and_sorted() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(20);
        layer.push_live(trade(100, 1.0));
        layer.push_live(trade(200, 2.0));
        layer.push_live(trade(300, 3.0));
        layer.push_live(trade(400, 4.0));

        // Batch overlaps the ring's interior (200..300) instead of abutting
        // the front cleanly — this is the P0 shape: a page whose newest key
        // is inside the already-retained range, not strictly older than it.
        let new_count = layer.prepend_batch(vec![
            trade(150, 1.5),
            trade(200, 22.0), // overlaps existing 200 -> batch wins
            trade(250, 2.5),
        ]);

        assert!(layer.is_sorted());
        assert_eq!(new_count, 2); // 150 and 250 are new; 200 is a replace
        assert_eq!(
            layer.snapshot(),
            vec![
                trade(100, 1.0),
                trade(150, 1.5),
                trade(200, 22.0),
                trade(250, 2.5),
                trade(300, 3.0),
                trade(400, 4.0),
            ]
        );
    }

    /// Regression test for the live P0: a candles -> range -> candles cycle
    /// glued 5 ascending runs into one deque with 4 order breaks because
    /// overlapping blocks were folded in via naive append/prepend instead of
    /// an ordered merge. Reproduces the exact session shape (retained data,
    /// then a Phase-A-style overlapping reseed, then an engine-ladder page
    /// that overlaps BOTH the retained data and the Phase-A block, then
    /// another Phase-A reseed) and asserts the final layer stays sorted with
    /// no duplicate keys throughout.
    #[test]
    fn live_pattern_overlapping_phase_a_and_ladder_pages_stay_sorted() {
        const STEP: i64 = 3600; // 1h bars, timestamp in seconds

        fn bar(ts: i64, close: f64) -> mlc_core::Bar {
            mlc_core::Bar::new(ts, close, close, close, close)
        }

        fn ladder(from_idx: i64, to_idx: i64) -> Vec<mlc_core::Bar> {
            (from_idx..=to_idx).map(|i| bar(i * STEP, i as f64)).collect()
        }

        let mut layer: RawLayer<mlc_core::Bar> = RawLayer::new(0); // unbounded for this shape check

        // Retained deeper layer from a prior chart-type cycle: t500..t2000.
        let new_seed = layer.seed_block(ladder(500, 2000));
        assert_eq!(new_seed, 1501);
        assert!(layer.is_sorted());

        // Phase A always fetches the newest window over the retained layer
        // — here it overlaps the tail and extends it a little further.
        let new_a1 = layer.seed_block(ladder(1700, 2050));
        assert!(layer.is_sorted(), "after first Phase-A overlap reseed");
        assert_eq!(new_a1, 50); // 2001..2050 are new, 1700..2000 already present

        // Engine ladder's first page overlaps Phase A's range (the exact P0
        // trigger: an older/overlapping block landing via prepend_batch) but
        // also reaches further back than anything retained so far (a
        // scroll-left page beyond the initial seed's start).
        let new_ladder = layer.prepend_batch(ladder(0, 2000));
        assert!(layer.is_sorted(), "after ladder page overlapping Phase-A range");
        assert_eq!(new_ladder, 500); // 0..499 are new; 500..2000 already present

        // A second Phase-A reseed lands again on the (now merged) tail.
        let new_a2 = layer.seed_block(ladder(1900, 2100));
        assert!(layer.is_sorted(), "after second Phase-A overlap reseed");
        assert_eq!(new_a2, 50); // 2051..2100 are new

        // Final invariant: strictly sorted, no duplicate keys, no order
        // breaks — the regression this test guards against. Every block
        // merged into a growing contiguous span from 0 to 2100, so the
        // layer ends up holding exactly that whole range, each key once.
        assert!(layer.is_sorted());
        let snapshot = layer.snapshot();
        assert_eq!(snapshot.len(), 2101); // indices 0..=2100 inclusive, each exactly once
        assert_eq!(snapshot.first().map(|b| b.chrono_key()), Some(0));
        assert_eq!(snapshot.last().map(|b| b.chrono_key()), Some(2100 * STEP));
        for w in snapshot.windows(2) {
            assert!(w[0].chrono_key() < w[1].chrono_key(), "order break detected between adjacent bars");
        }
    }

    #[test]
    fn evict_older_than_drops_only_the_stale_prefix() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(0);
        for ts in [100, 200, 300, 400, 500] {
            layer.push_live(trade(ts, ts as f64));
        }
        layer.evict_older_than(300);
        assert_eq!(
            layer.snapshot(),
            vec![trade(300, 300.0), trade(400, 400.0), trade(500, 500.0)],
            "everything strictly older than the cutoff must be dropped, cutoff itself kept"
        );
    }

    #[test]
    fn evict_older_than_on_fully_stale_layer_empties_it() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(0);
        for ts in [100, 200, 300] {
            layer.push_live(trade(ts, ts as f64));
        }
        layer.evict_older_than(1_000);
        assert!(layer.is_empty());
    }

    #[test]
    fn evict_older_than_on_fresh_layer_is_a_noop() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(0);
        for ts in [500, 600, 700] {
            layer.push_live(trade(ts, ts as f64));
        }
        layer.evict_older_than(100);
        assert_eq!(layer.len(), 3);
    }

    #[test]
    fn capacity_eviction_after_merge_keeps_newest() {
        let mut layer: RawLayer<RawTrade> = RawLayer::new(4);
        layer.push_live(trade(100, 1.0));
        layer.push_live(trade(200, 2.0));
        layer.push_live(trade(300, 3.0));
        layer.push_live(trade(400, 4.0));

        // Overlapping merge batch that would grow the ring past capacity —
        // eviction after the merge must drop the oldest keys, keeping the
        // newest ones (including the batch's own newer points).
        layer.prepend_batch(vec![trade(50, 0.5), trade(150, 1.5), trade(500, 5.0)]);

        assert!(layer.is_sorted());
        assert_eq!(layer.len(), 4);
        assert_eq!(
            layer.snapshot(),
            vec![trade(200, 2.0), trade(300, 3.0), trade(400, 4.0), trade(500, 5.0)]
        );
    }
}
