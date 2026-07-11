//! TPO Profile chart overlay — time-at-price histogram from OHLCV bars.
//!
//! Mirrors `volume_profile.rs`'s shape (same POC/VAH/VAL/row model, same
//! `compute`-from-bars entry point) but counts BAR TOUCHES per price row
//! instead of summing volume — a "how many bars spent time at this price"
//! profile, the block-histogram counterpart of the letter-based TPO Market
//! Profile (`mlc_core::types::TpoSession`) rendered by
//! `mlc-app-layout::chart_render::draw_tpo`. This overlay is a DIFFERENT,
//! simpler visualization: no letters, no session boundaries — just a
//! right-anchored block histogram over the VISIBLE bar range, styled like
//! (but visually distinct from) the Volume Profile overlay.

use crate::Bar;

/// One price-bucket row in a TPO Profile histogram.
#[derive(Clone, Debug)]
pub struct TpoProfileRow {
    /// Center price of this bucket.
    pub price: f64,
    /// Number of visible bars whose `[low, high]` span touches this row.
    pub count: u32,
}

/// Pre-computed TPO Profile ready for rendering.
///
/// All price values are in instrument price units.
#[derive(Clone, Debug)]
pub struct TpoProfileData {
    /// Rows sorted ascending by price.
    pub rows: Vec<TpoProfileRow>,
    /// Width of each price bucket (the `nice_step`-picked row height).
    pub bucket_size: f64,
    /// Point of Control: price at the bucket with the highest touch count.
    pub poc_price: f64,
    /// Value Area High: upper bound of the 70% value area.
    pub vah_price: f64,
    /// Value Area Low: lower bound of the 70% value area.
    pub val_price: f64,
    /// Total touch-count across all rows (sum of `count`, NOT bar count —
    /// a bar spanning N rows contributes N touches).
    pub total_count: u64,
    /// Touch count of the tallest row — used to normalize bar widths.
    pub max_row_count: u32,
}

/// Target number of display rows the profile aims for — mirrors dig3's own
/// `TPO_TARGET_ROWS` convention for the letter-based TPO renderer (industry
/// profiles commonly run 40-100 rows; 60 sits in the middle).
const TARGET_ROWS: f64 = 60.0;

/// Round `price_range / TARGET_ROWS` up to the nearest "nice" step — 1, 2,
/// 2.5, or 5 x 10^k. Small local port of dig3-station's
/// `derived::nice_step` (that function is `pub(crate)` inside
/// `digdigdig3-station`, not exported) — kept intentionally minimal: this
/// overlay only needs the row-count-shaping behavior, not dig3's
/// instrument-tick clamp (the caller already guards `price_range <= 0.0`
/// and passes a non-zero tick floor via `min_step`).
fn nice_step(price_range: f64, min_step: f64) -> f64 {
    let min_step = min_step.max(1e-12);
    if !price_range.is_finite() || price_range <= 0.0 {
        return min_step;
    }

    let raw_step = (price_range / TARGET_ROWS).max(min_step);
    if raw_step <= min_step {
        return min_step;
    }

    let exp = raw_step.log10().floor();
    let base = 10f64.powf(exp);
    let mantissa = raw_step / base;

    let nice_mantissa = if mantissa <= 1.0 {
        1.0
    } else if mantissa <= 2.0 {
        2.0
    } else if mantissa <= 2.5 {
        2.5
    } else if mantissa <= 5.0 {
        5.0
    } else {
        10.0
    };

    (nice_mantissa * base).max(min_step)
}

impl TpoProfileData {
    /// Compute a `TpoProfileData` from a slice of bars (the caller passes
    /// the VISIBLE bar range — see `ChartContainer::ensure_tpo_profile_from_bars`).
    ///
    /// Row step is `nice_step(visible price range / 60)` (owner spec) —
    /// reuses the same row-density target the letter-based TPO renderer's
    /// upstream (dig3) uses, just computed locally over whatever bars are
    /// on screen rather than a full session.
    ///
    /// `value_area_pct` is typically 0.70 (70% value area). `min_tick` is a
    /// floor on the row step (pass the instrument's price precision, or a
    /// small positive epsilon when unknown) so a near-flat visible range
    /// never produces a degenerate zero-width bucket.
    pub fn compute(bars: &[Bar], value_area_pct: f64, min_tick: f64) -> Self {
        if bars.is_empty() {
            return Self::empty();
        }

        let mut price_min = f64::INFINITY;
        let mut price_max = f64::NEG_INFINITY;
        for b in bars {
            if b.low < price_min { price_min = b.low; }
            if b.high > price_max { price_max = b.high; }
        }
        if !price_min.is_finite() || !price_max.is_finite() || price_max <= price_min {
            return Self::empty();
        }

        let bucket_size = nice_step(price_max - price_min, min_tick);
        if bucket_size <= 0.0 || !bucket_size.is_finite() {
            return Self::empty();
        }

        let bucket_count = (((price_max - price_min) / bucket_size).ceil() as usize).max(1);
        let mut counts: Vec<u32> = vec![0; bucket_count];

        // A bar "touches" every row its [low, high] span crosses (mirrors
        // the letter-TPO convention of marking every price level a period
        // traded through, not just its close).
        for b in bars {
            if !b.low.is_finite() || !b.high.is_finite() { continue; }
            let lo_idx = (((b.low - price_min) / bucket_size).floor() as i64).clamp(0, bucket_count as i64 - 1) as usize;
            let hi_idx = (((b.high - price_min) / bucket_size).floor() as i64).clamp(0, bucket_count as i64 - 1) as usize;
            for idx in lo_idx..=hi_idx {
                counts[idx] = counts[idx].saturating_add(1);
            }
        }

        let rows: Vec<TpoProfileRow> = (0..bucket_count)
            .map(|i| TpoProfileRow {
                price: price_min + (i as f64 + 0.5) * bucket_size,
                count: counts[i],
            })
            .collect();

        let total_count: u64 = rows.iter().map(|r| r.count as u64).sum();
        let max_row_count = rows.iter().map(|r| r.count).max().unwrap_or(0);

        if total_count == 0 || max_row_count == 0 {
            return Self::empty();
        }

        let poc_idx = rows.iter().enumerate()
            .max_by_key(|(_, r)| r.count)
            .map(|(i, _)| i)
            .unwrap_or(0);
        let poc_price = rows[poc_idx].price;

        // Value Area (mirrors `VolumeProfileData::compute`'s expand-toward-
        // more-volume walk, counts instead of volume).
        let target = (total_count as f64 * value_area_pct.clamp(0.0, 1.0)).ceil() as u64;
        let mut accumulated = rows[poc_idx].count as u64;
        let mut lo = poc_idx;
        let mut hi = poc_idx;

        while accumulated < target {
            let can_lo = lo > 0;
            let can_hi = hi + 1 < rows.len();
            if !can_lo && !can_hi { break; }

            let count_lo = if can_lo { rows[lo - 1].count as u64 } else { 0 };
            let count_hi = if can_hi { rows[hi + 1].count as u64 } else { 0 };

            if can_hi && count_hi >= count_lo {
                hi += 1;
                accumulated += count_hi;
            } else if can_lo {
                lo -= 1;
                accumulated += count_lo;
            } else {
                hi += 1;
                accumulated += count_hi;
            }
        }

        let val_price = rows[lo].price - bucket_size * 0.5;
        let vah_price = rows[hi].price + bucket_size * 0.5;

        Self {
            rows,
            bucket_size,
            poc_price,
            vah_price,
            val_price,
            total_count,
            max_row_count,
        }
    }

    fn empty() -> Self {
        Self {
            rows: Vec::new(),
            bucket_size: 0.0,
            poc_price: 0.0,
            vah_price: 0.0,
            val_price: 0.0,
            total_count: 0,
            max_row_count: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(ts: i64, open: f64, high: f64, low: f64, close: f64) -> Bar {
        Bar { timestamp: ts, open, high, low, close, volume: 1.0, derived_ts_ms: 0 }
    }

    #[test]
    fn empty_bars_yields_empty_profile() {
        let data = TpoProfileData::compute(&[], 0.70, 0.01);
        assert!(data.rows.is_empty());
        assert_eq!(data.total_count, 0);
    }

    #[test]
    fn flat_price_range_yields_empty_profile() {
        // price_max == price_min -> degenerate range, no bucket.
        let bars = vec![bar(0, 100.0, 100.0, 100.0, 100.0); 3];
        let data = TpoProfileData::compute(&bars, 0.70, 0.01);
        assert!(data.rows.is_empty());
    }

    #[test]
    fn poc_lands_on_most_touched_row() {
        // Bars clustered around 100-101 far more than the outliers at 90/110.
        let mut bars = vec![bar(0, 90.0, 90.0, 90.0, 90.0), bar(1, 110.0, 110.0, 110.0, 110.0)];
        for i in 0..20 {
            bars.push(bar(2 + i, 100.0, 100.5, 100.0, 100.5));
        }
        let data = TpoProfileData::compute(&bars, 0.70, 0.01);
        assert!(!data.rows.is_empty());
        assert!(data.poc_price > 95.0 && data.poc_price < 105.0, "POC should land in the dense cluster, got {}", data.poc_price);
    }

    #[test]
    fn value_area_bounds_are_within_session_high_low() {
        let bars = vec![
            bar(0, 100.0, 101.0, 99.0, 100.5),
            bar(1, 100.5, 102.0, 100.0, 101.5),
            bar(2, 101.5, 103.0, 101.0, 102.5),
        ];
        let data = TpoProfileData::compute(&bars, 0.70, 0.01);
        assert!(data.val_price <= data.poc_price);
        assert!(data.vah_price >= data.poc_price);
        assert!(data.val_price >= 99.0 - data.bucket_size);
        assert!(data.vah_price <= 103.0 + data.bucket_size);
    }

    #[test]
    fn row_count_is_capped_near_target_rows() {
        let bars = vec![
            bar(0, 100.0, 200.0, 100.0, 150.0),
            bar(1, 150.0, 160.0, 140.0, 155.0),
        ];
        let data = TpoProfileData::compute(&bars, 0.70, 0.0001);
        // TARGET_ROWS=60 -> row count should land in a reasonable band, not
        // thousands of degenerate rows.
        assert!(data.rows.len() <= 120, "row count {} far exceeds the target density", data.rows.len());
        assert!(!data.rows.is_empty());
    }

    #[test]
    fn bucket_size_never_below_min_tick_floor() {
        let bars = vec![bar(0, 100.0, 100.01, 99.99, 100.0), bar(1, 100.0, 100.02, 99.98, 100.01)];
        let data = TpoProfileData::compute(&bars, 0.70, 0.5);
        assert!(data.bucket_size >= 0.5 - 1e-9, "bucket_size {} must respect the min_tick floor 0.5", data.bucket_size);
    }

    #[test]
    fn wide_bar_touches_every_row_it_spans() {
        // One bar spanning the whole range should populate several rows,
        // not just the endpoints.
        let bars = vec![bar(0, 100.0, 200.0, 100.0, 150.0)];
        let data = TpoProfileData::compute(&bars, 0.70, 0.01);
        let touched = data.rows.iter().filter(|r| r.count > 0).count();
        assert!(touched > 1, "a bar spanning 100 units should touch more than 1 row, got {touched}");
    }
}
