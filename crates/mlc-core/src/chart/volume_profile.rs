//! Volume Profile chart overlay — bar-aggregated VP from OHLCV data.
//!
//! Computes horizontal histograms from bars already loaded in the chart window.
//! This is the chart-overlay VP (C10). The live trade-aggregated VP lives in
//! `mlc-app-panels/src/trading/order_flow/volume_profile.rs`.

use serde::{Deserialize, Serialize};
use crate::Bar;

// ─── Mode ────────────────────────────────────────────────────────────────────

/// Which window of bars to aggregate the Volume Profile over.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum VpMode {
    /// Aggregate visible bars (re-computes on pan / zoom).
    #[default]
    VisibleRange,
    /// Aggregate the current UTC trading day (last group of day-aligned bars).
    Session,
    /// Aggregate a fixed time range. The drawing tool is deferred; this variant
    /// is present so the data path exists and FixedRange can be set programmatically.
    FixedRange {
        start_ts_ms: i64,
        end_ts_ms: i64,
    },
    /// Merge the last N sessions into one profile.
    Composite,
}

// ─── Row ─────────────────────────────────────────────────────────────────────

/// One price-bucket row in a Volume Profile histogram.
#[derive(Clone, Debug, PartialEq)]
pub struct VpRow {
    /// Center price of this bucket.
    pub price: f64,
    /// Share of up-bar (`close >= open`) volume distributed into this
    /// bucket — see [`VolumeProfileData::compute`]'s own doc for the
    /// distribution rule. This is a bar-DIRECTION split, not a trade-SIDE
    /// split: OHLCV data carries no record of which side of the book
    /// initiated each trade, so "buy" here means "volume from bars that
    /// closed at or above where they opened," the standard approximation
    /// used whenever the input is bars rather than individual trades.
    pub buy_volume: f64,
    /// Share of down-bar (`close < open`) volume distributed into this
    /// bucket — same bar-direction approximation as `buy_volume` above.
    pub sell_volume: f64,
    /// Total volume: buy_volume + sell_volume.
    pub total_volume: f64,
}

// ─── Computed data ────────────────────────────────────────────────────────────

/// Pre-computed Volume Profile ready for rendering.
///
/// All price values are in instrument price units; all volumes in raw units.
#[derive(Clone, Debug, PartialEq)]
pub struct VolumeProfileData {
    pub mode: VpMode,
    /// Rows sorted ascending by price.
    pub rows: Vec<VpRow>,
    /// Width of each price bucket.
    pub bucket_size: f64,
    /// Point of Control: price at the bucket with the highest total volume.
    pub poc_price: f64,
    /// Value Area High: upper bound of the 70 % value area.
    pub vah_price: f64,
    /// Value Area Low: lower bound of the 70 % value area.
    pub val_price: f64,
    /// Total volume across all rows.
    pub total_volume: f64,
    /// Volume of the tallest row — used to normalize bar widths.
    pub max_row_volume: f64,
    /// Start timestamp (ms) of the bar window this was computed from.
    pub start_ts_ms: i64,
    /// End timestamp (ms) of the bar window.
    pub end_ts_ms: i64,
}

impl VolumeProfileData {
    /// Compute a VolumeProfileData from a slice of bars.
    ///
    /// `bars` must already be filtered to the desired time window
    /// (VisibleRange callers pass the visible slice; Session/Composite callers
    /// pass the filtered subset).
    ///
    /// Each bar's own volume is spread uniformly across every row its
    /// `[low, high]` range overlaps, weighted by row overlap — see the
    /// "Distribute bar volume into buckets" section below for the full
    /// rule and its degenerate cases (`high == low`, zero volume).
    ///
    /// `value_area_pct` is typically 0.70 (70 % value area).
    /// `bucket_count` controls granularity (default 24 is a good trade-off).
    pub fn compute(bars: &[Bar], mode: VpMode, value_area_pct: f64, bucket_count: usize) -> Self {
        // ── Empty guard ───────────────────────────────────────────────────────
        if bars.is_empty() || bucket_count == 0 {
            return Self::empty(mode);
        }

        // ── Price range across high-low spans ─────────────────────────────────
        let mut price_min = f64::INFINITY;
        let mut price_max = f64::NEG_INFINITY;
        for b in bars {
            if b.low < price_min { price_min = b.low; }
            if b.high > price_max { price_max = b.high; }
        }
        if !price_min.is_finite() || !price_max.is_finite() || price_max <= price_min {
            return Self::empty(mode);
        }

        // ── Bucket geometry ───────────────────────────────────────────────────
        let bucket_size = (price_max - price_min) / bucket_count as f64;
        if bucket_size <= 0.0 {
            return Self::empty(mode);
        }

        // ── Distribute bar volume into buckets ────────────────────────────────
        // Each bar spreads its own volume UNIFORMLY across every row its own
        // [low, high] range overlaps, weighted by how much of the row that
        // range covers — a bar spanning twelve rows contributes to twelve of
        // them, in proportion to how much of each row it actually touched;
        // a bar that only reaches partway into a row contributes only that
        // fraction. Uniform-within-the-bar is the honest assumption an OHLCV
        // bar supports: the bar records a price range and a total volume,
        // nothing about where inside that range the volume concentrated.
        // Getting closer than uniform (e.g. weighting toward one edge) would
        // need per-trade data — individual trade prices/sizes, or at least
        // a tick-level feed — which this function is never handed.
        let mut buy_buckets: Vec<f64> = vec![0.0; bucket_count];
        let mut sell_buckets: Vec<f64> = vec![0.0; bucket_count];

        for b in bars {
            // Zero (or negative, which should not occur but is defensive)
            // volume has nothing to distribute — skip before any division
            // so a zero-volume bar can never be the source of a NaN.
            if b.volume <= 0.0 {
                continue;
            }

            let target = if b.is_bullish() { &mut buy_buckets } else { &mut sell_buckets };

            // Bars are guaranteed to sit inside [price_min, price_max] by
            // construction (that range comes from folding every bar's own
            // low/high), so this clamp only ever guards float rounding at
            // the exact edges — it never clips real bar range away.
            let bar_lo = b.low.max(price_min);
            let bar_hi = b.high.min(price_max);
            let idx_lo = row_index(bar_lo, price_min, bucket_size, bucket_count);
            let idx_hi = row_index(bar_hi, price_min, bucket_size, bucket_count);

            let span = bar_hi - bar_lo;
            if span <= 0.0 {
                // Degenerate range: `high == low` (a bar that never moved,
                // or a synthetic/tick-derived bar with zero width). There is
                // no range to weight a distribution by, so the bar's own
                // uniform-across-its-range model collapses to "all of it in
                // the single row that contains it" — the same row a
                // midpoint bucket would have picked, but reached without
                // dividing by a zero-width span.
                target[idx_lo] += b.volume;
                continue;
            }

            for idx in idx_lo..=idx_hi {
                let row_lo = price_min + idx as f64 * bucket_size;
                let row_hi = row_lo + bucket_size;
                let overlap = (bar_hi.min(row_hi) - bar_lo.max(row_lo)).max(0.0);
                if overlap > 0.0 {
                    target[idx] += b.volume * (overlap / span);
                }
            }
        }

        // ── Build rows ────────────────────────────────────────────────────────
        let rows: Vec<VpRow> = (0..bucket_count)
            .map(|i| {
                let buy = buy_buckets[i];
                let sell = sell_buckets[i];
                VpRow {
                    price: price_min + (i as f64 + 0.5) * bucket_size,
                    buy_volume: buy,
                    sell_volume: sell,
                    total_volume: buy + sell,
                }
            })
            .collect();

        // ── Aggregate stats ───────────────────────────────────────────────────
        let total_volume: f64 = rows.iter().map(|r| r.total_volume).sum();
        let max_row_volume = rows.iter().map(|r| r.total_volume).fold(0.0_f64, f64::max);

        if total_volume <= 0.0 {
            return Self::empty(mode);
        }

        // ── POC ───────────────────────────────────────────────────────────────
        let poc_idx = rows
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                a.total_volume
                    .partial_cmp(&b.total_volume)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
            .unwrap_or(0);
        let poc_price = rows[poc_idx].price;

        // ── Value Area (mirrors mlc-app-panels volume_profile.rs:compute_value_area) ──
        // Both POC and value area below read only `rows[i].total_volume` —
        // neither assumes a row's volume came from a whole number of bars
        // (the old one-bucket-per-bar model) or from any particular bar at
        // all. Range distribution changes how much volume each row holds,
        // never what a row IS, so this maths is unchanged and correct
        // against the new distribution exactly as it was against the old.
        let target = total_volume * value_area_pct.clamp(0.0, 1.0);
        let mut accumulated = rows[poc_idx].total_volume;
        let mut lo = poc_idx;
        let mut hi = poc_idx;

        while accumulated < target {
            let can_lo = lo > 0;
            let can_hi = hi + 1 < rows.len();
            if !can_lo && !can_hi { break; }

            let vol_lo = if can_lo { rows[lo - 1].total_volume } else { 0.0 };
            let vol_hi = if can_hi { rows[hi + 1].total_volume } else { 0.0 };

            // Expand toward the side with more volume; prefer high side on tie.
            if can_hi && vol_hi >= vol_lo {
                hi += 1;
                accumulated += vol_hi;
            } else if can_lo {
                lo -= 1;
                accumulated += vol_lo;
            } else {
                hi += 1;
                accumulated += vol_hi;
            }
        }

        let val_price = rows[lo].price - bucket_size * 0.5;
        let vah_price = rows[hi].price + bucket_size * 0.5;

        // ── Timestamps ───────────────────────────────────────────────────────
        let start_ts_ms = bars.first().map(|b| b.ts_ms()).unwrap_or(0);
        let end_ts_ms = bars.last().map(|b| b.ts_ms()).unwrap_or(0);

        // Rows are already sorted ascending (built by index).
        Self {
            mode,
            rows,
            bucket_size,
            poc_price,
            vah_price,
            val_price,
            total_volume,
            max_row_volume,
            start_ts_ms,
            end_ts_ms,
        }
    }

    fn empty(mode: VpMode) -> Self {
        Self {
            mode,
            rows: Vec::new(),
            bucket_size: 0.0,
            poc_price: 0.0,
            vah_price: 0.0,
            val_price: 0.0,
            total_volume: 0.0,
            max_row_volume: 0.0,
            start_ts_ms: 0,
            end_ts_ms: 0,
        }
    }
}

// ─── Bucket geometry helper ────────────────────────────────────────────────────

/// The row index `price` falls into, for a histogram covering
/// `[price_min, price_min + bucket_count * bucket_size]` in `bucket_count`
/// equal-width rows.
///
/// Rows are half-open `[row_lo, row_hi)` except the last, which is closed
/// on both ends — clamping here is what makes `price_max` itself land in
/// the LAST row instead of one past the end of `buy_buckets`/`sell_buckets`.
fn row_index(price: f64, price_min: f64, bucket_size: f64, bucket_count: usize) -> usize {
    let raw = ((price - price_min) / bucket_size).floor() as i64;
    raw.clamp(0, bucket_count as i64 - 1) as usize
}

// ─── Day-grouping helpers (Session / Composite) ───────────────────────────────

/// Group bars by UTC calendar day. Each group is a (start_ts_sec, Vec<bar_idx>) pair
/// ordered chronologically. Bars without a timestamp are skipped.
///
/// Used by `filter_bars_for_mode` to implement Session and Composite modes.
fn group_by_utc_day(bars: &[Bar]) -> Vec<(i64, Vec<usize>)> {
    let mut groups: Vec<(i64, Vec<usize>)> = Vec::new();
    for (i, b) in bars.iter().enumerate() {
        let day_start = (b.timestamp / 86_400) * 86_400; // floor to UTC midnight (seconds)
        if let Some(last) = groups.last_mut() {
            if last.0 == day_start {
                last.1.push(i);
                continue;
            }
        }
        groups.push((day_start, vec![i]));
    }
    groups
}

/// Return the subset of `bars` appropriate for `mode`.
///
/// - `VisibleRange`: caller is responsible for pre-filtering; this returns a clone
///   of `bars` unchanged.
/// - `Session`: last UTC day group.
/// - `FixedRange`: bars whose `ts_ms()` falls within `[start_ts_ms, end_ts_ms]`.
/// - `Composite`: last `composite_n` day groups merged.
pub fn filter_bars_for_mode<'a>(
    bars: &'a [Bar],
    mode: VpMode,
    composite_n: usize,
) -> Vec<&'a Bar> {
    match mode {
        VpMode::VisibleRange => bars.iter().collect(),
        VpMode::Session => {
            let groups = group_by_utc_day(bars);
            match groups.last() {
                Some((_, indices)) => indices.iter().map(|&i| &bars[i]).collect(),
                None => Vec::new(),
            }
        }
        VpMode::FixedRange { start_ts_ms, end_ts_ms } => bars
            .iter()
            .filter(|b| {
                let ts = b.ts_ms();
                ts >= start_ts_ms && ts <= end_ts_ms
            })
            .collect(),
        VpMode::Composite => {
            let groups = group_by_utc_day(bars);
            let n = composite_n.max(1);
            let start = groups.len().saturating_sub(n);
            groups[start..]
                .iter()
                .flat_map(|(_, indices)| indices.iter().map(|&i| &bars[i]))
                .collect()
        }
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! Coverage for the range-distribution rewrite of
    //! [`VolumeProfileData::compute`] — the fix for the "whole bar volume
    //! into one midpoint bucket" defect. Every test below picks bar
    //! ranges/bucket counts that make the expected per-row volume an exact
    //! floating-point value (no rounding slop needed) so numbers can be
    //! checked by hand against the assertions.

    use super::*;

    const EPS: f64 = 1e-9;

    fn bar(ts: i64, open: f64, high: f64, low: f64, close: f64, volume: f64) -> Bar {
        Bar::with_volume(ts, open, high, low, close, volume)
    }

    /// A bar spanning exactly N of the histogram's rows puts V/N in each of
    /// those N rows and exactly 0.0 in every other row.
    ///
    /// `bar_wide` (zero volume) only stretches [price_min, price_max] to 24
    /// rows so there ARE rows outside the volume bar's own [100, 104] range
    /// for "nothing elsewhere" to be a real claim, not a vacuous one.
    #[test]
    fn bar_spanning_n_rows_puts_v_over_n_in_each_and_nothing_elsewhere() {
        let bar_wide = bar(0, 90.0, 114.0, 90.0, 114.0, 0.0);
        let bar_target = bar(60, 100.0, 104.0, 100.0, 104.0, 400.0);
        let bars = [bar_wide, bar_target];

        let vp = VolumeProfileData::compute(&bars, VpMode::VisibleRange, 0.70, 24);

        assert_eq!(vp.rows.len(), 24);
        assert!((vp.bucket_size - 1.0).abs() < EPS, "bucket_size = {}", vp.bucket_size);
        for (i, row) in vp.rows.iter().enumerate() {
            if (10..=13).contains(&i) {
                assert!(
                    (row.total_volume - 100.0).abs() < EPS,
                    "row {i} total_volume = {}, want 100.0 (400/4)",
                    row.total_volume
                );
                assert!((row.buy_volume - 100.0).abs() < EPS, "row {i} buy_volume = {}", row.buy_volume);
                assert_eq!(row.sell_volume, 0.0, "row {i} sell_volume");
            } else {
                assert_eq!(row.total_volume, 0.0, "row {i} total_volume, want nothing elsewhere");
            }
        }
        assert!((vp.total_volume - 400.0).abs() < EPS);
    }

    /// A bar whose high equals its low (zero range) puts its whole volume
    /// in the single row containing that price — reached without dividing
    /// by the bar's own zero-width span.
    #[test]
    fn zero_range_bar_puts_all_volume_in_its_one_row() {
        // Zero volume: only stretches the overall range to [100, 110], never
        // contributes any volume of its own.
        let bar_range = bar(0, 100.0, 110.0, 100.0, 110.0, 0.0);
        // high == low == 105: a bar that never moved (so open/close, which
        // must sit within [low, high], are pinned to 105 too).
        let bar_flat = bar(60, 105.0, 105.0, 105.0, 105.0, 250.0);
        let bars = [bar_range, bar_flat];

        let vp = VolumeProfileData::compute(&bars, VpMode::VisibleRange, 0.70, 10);

        assert_eq!(vp.rows.len(), 10);
        assert!((vp.bucket_size - 1.0).abs() < EPS);
        // 105 sits at the boundary between row 4 ([104,105)) and row 5
        // ([105,106)); `row_index`'s floor lands it in row 5.
        for (i, row) in vp.rows.iter().enumerate() {
            if i == 5 {
                assert!((row.total_volume - 250.0).abs() < EPS, "row 5 total_volume = {}", row.total_volume);
                assert!((row.buy_volume - 250.0).abs() < EPS, "flat bar is bullish (close >= open)");
            } else {
                assert_eq!(row.total_volume, 0.0, "row {i} total_volume, want nothing outside the flat bar's row");
            }
        }
        assert!((vp.total_volume - 250.0).abs() < EPS);
    }

    /// A zero-volume bar contributes nothing and never reaches a division —
    /// paired here with a zero-RANGE zero-volume bar in the same window, so
    /// the 0/0 shape is actually exercised without ever hitting the
    /// zero-volume `continue` first would make moot on its own.
    #[test]
    fn zero_volume_bar_contributes_nothing_and_never_divides_by_zero() {
        let bar_real = bar(0, 0.0, 50.0, 0.0, 50.0, 100.0);
        // Zero volume AND zero range together: the volume check must skip
        // this bar before the span check ever runs.
        let bar_zero = bar(60, 25.0, 25.0, 25.0, 25.0, 0.0);
        let bars = [bar_real, bar_zero];

        let vp = VolumeProfileData::compute(&bars, VpMode::VisibleRange, 0.70, 5);

        assert!((vp.total_volume - 100.0).abs() < EPS, "the zero-volume bar must not add anything");
        for row in &vp.rows {
            assert!(row.total_volume.is_finite(), "no NaN/inf from a 0/0 bar");
            assert!((row.total_volume - 20.0).abs() < EPS, "100 over 5 rows = 20 each");
        }
    }

    /// Two bars — one spread across the whole range, one wholly inside a
    /// single row with much larger volume — produce a POC at the row the
    /// concentrated bar landed in. Range distribution must get this right
    /// on purpose; midpoint bucketing only got it right by accident because
    /// the concentrated bar's midpoint happened to sit in the same row.
    #[test]
    fn poc_lands_on_the_row_the_concentrated_bar_landed_in() {
        // Spread bar: 10 rows, volume 100 -> 10.0 into every row.
        let bar_spread = bar(0, 100.0, 110.0, 100.0, 110.0, 100.0);
        // Concentrated bar: wholly inside row 3 ([103, 104)), far larger
        // volume, opposite (bearish) direction from the spread bar.
        let bar_concentrated = bar(60, 103.8, 103.8, 103.2, 103.2, 500.0);
        let bars = [bar_spread, bar_concentrated];

        let vp = VolumeProfileData::compute(&bars, VpMode::VisibleRange, 0.70, 10);

        assert!((vp.poc_price - 103.5).abs() < EPS, "poc_price = {}", vp.poc_price);
        let row3 = &vp.rows[3];
        assert!((row3.buy_volume - 10.0).abs() < EPS, "row3 buy_volume = {}", row3.buy_volume);
        assert!((row3.sell_volume - 500.0).abs() < EPS, "row3 sell_volume = {}", row3.sell_volume);
        assert!((row3.total_volume - 510.0).abs() < EPS);
    }

    /// A known 5-row synthetic distribution [10, 20, 100, 30, 5]: the value
    /// area walk starts at the POC row (100, index 2) and expands toward
    /// the larger neighbour until 70 % of the 165 total is covered.
    /// 100 (accumulated) < 115.5 (target) -> expand toward index 3 (30 >=
    /// 20) -> accumulated 130 >= 115.5 -> stop. Value area = rows {2, 3}.
    #[test]
    fn value_area_covers_exactly_the_rows_crossing_seventy_percent() {
        let bars = [
            bar(0, 0.0, 8.0, 0.0, 8.0, 10.0),      // row 0: [0, 10)
            bar(60, 12.0, 18.0, 12.0, 18.0, 20.0), // row 1: [10, 20)
            bar(120, 22.0, 28.0, 22.0, 28.0, 100.0), // row 2: [20, 30)
            bar(180, 32.0, 38.0, 32.0, 38.0, 30.0),  // row 3: [30, 40)
            bar(240, 42.0, 50.0, 42.0, 50.0, 5.0),   // row 4: [40, 50]
        ];

        let vp = VolumeProfileData::compute(&bars, VpMode::VisibleRange, 0.70, 5);

        let totals: Vec<f64> = vp.rows.iter().map(|r| r.total_volume).collect();
        assert!(
            totals.iter().zip([10.0, 20.0, 100.0, 30.0, 5.0]).all(|(a, b)| (a - b).abs() < EPS),
            "row totals = {totals:?}"
        );
        assert!((vp.total_volume - 165.0).abs() < EPS);
        assert!((vp.poc_price - 25.0).abs() < EPS, "poc_price = {}", vp.poc_price);
        assert!((vp.val_price - 20.0).abs() < EPS, "val_price = {}", vp.val_price);
        assert!((vp.vah_price - 40.0).abs() < EPS, "vah_price = {}", vp.vah_price);
    }

    /// The buy/sell split sums to the row total for every row, including a
    /// row that receives volume from BOTH a buy bar and a sell bar.
    #[test]
    fn buy_sell_split_sums_to_total_for_every_row() {
        // Buy bar spans all 5 rows: 100 volume -> 20.0 into every row.
        let bar_buy = bar(0, 0.0, 50.0, 0.0, 50.0, 100.0);
        // Sell bar wholly inside row 2 ([20, 30)).
        let bar_sell = bar(60, 28.0, 28.0, 22.0, 22.0, 50.0);
        let bars = [bar_buy, bar_sell];

        let vp = VolumeProfileData::compute(&bars, VpMode::VisibleRange, 0.70, 5);

        for (i, row) in vp.rows.iter().enumerate() {
            assert!(
                (row.buy_volume + row.sell_volume - row.total_volume).abs() < EPS,
                "row {i}: buy {} + sell {} != total {}", row.buy_volume, row.sell_volume, row.total_volume
            );
        }
        let row2 = &vp.rows[2];
        assert!((row2.buy_volume - 20.0).abs() < EPS, "row2 buy_volume = {}", row2.buy_volume);
        assert!((row2.sell_volume - 50.0).abs() < EPS, "row2 sell_volume = {}", row2.sell_volume);
        assert!((row2.total_volume - 70.0).abs() < EPS);
        assert!((vp.total_volume - 150.0).abs() < EPS, "100 buy + 50 sell");
    }
}
