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
#[derive(Clone, Debug)]
pub struct VpRow {
    /// Center price of this bucket.
    pub price: f64,
    /// Volume from up-bars (close >= open) that fell into this bucket.
    pub buy_volume: f64,
    /// Volume from down-bars (close < open) that fell into this bucket.
    pub sell_volume: f64,
    /// Total volume: buy_volume + sell_volume.
    pub total_volume: f64,
}

// ─── Computed data ────────────────────────────────────────────────────────────

/// Pre-computed Volume Profile ready for rendering.
///
/// All price values are in instrument price units; all volumes in raw units.
#[derive(Clone, Debug)]
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
        // MVP: assign bar.volume to the bucket that contains the bar's midpoint
        // (open+close)/2. This is the standard bar-data approximation when
        // tick-level trades are unavailable.
        let mut buy_buckets: Vec<f64> = vec![0.0; bucket_count];
        let mut sell_buckets: Vec<f64> = vec![0.0; bucket_count];

        for b in bars {
            let midpoint = (b.open + b.close) * 0.5;
            let raw_idx = ((midpoint - price_min) / bucket_size).floor() as i64;
            let idx = raw_idx.clamp(0, (bucket_count as i64) - 1) as usize;
            if b.close >= b.open {
                buy_buckets[idx] += b.volume;
            } else {
                sell_buckets[idx] += b.volume;
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
