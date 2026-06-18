//! Liquidation Heatmap data types for the Hyblock/CoinGlass-style chart-type (C13).
//!
//! Stores two complementary datasets:
//!
//! 1. **`LiquidationHeatmapBuffer`** — rolling 2D ring of `(time_bucket, price_tick)`
//!    cells aggregated from `LiquidationBucketPoint` (period-bucketed USD totals).
//!    Used for the background heatmap layer.
//!
//! 2. **`LiquidationDot`** — per-event liquidation positioned at real `(ts_ms, price)`.
//!    Rendered as a dot overlay on top of the heatmap.
//!
//! ## Side encoding
//!
//! dig3 `LiquidationPoint.side`: `TradeSide::Buy => 0`, `TradeSide::Sell => 1`.
//! `TradeSide::Buy` on a forced-close means the exchange bought back a SHORT position
//! → the SHORT-side trader was liquidated.  `TradeSide::Sell` means a LONG was liquidated.
//! Industry convention (Hyblock/CoinGlass): long-liquidated = RED, short-liquidated = GREEN.
//! Therefore: `side == 1` (Sell) = long liquidated = RED, `side == 0` (Buy) = short liquidated = GREEN.
//!
//! ## Axis strategy: virtual-index axis
//!
//! Mirrors C12's choice: each time bucket is one virtual "bar".  The chart reuses the
//! existing index-axis viewport geometry without touching the time_axis path.
//!
//! ## Out of scope
//!
//! CoinGlass "Liquidation Levels" (predicted future liquidations by leverage-distribution
//! model) is a model layer, not raw aggregation.  dig3 does not expose it.  Out of scope.

use std::collections::VecDeque;

/// Default bucket width in milliseconds — 4 buckets/s matches C12.
pub const DEFAULT_BUCKET_MS: i64 = 250;

/// Maximum cells retained in the rolling ring.
pub const MAX_CELLS: usize = 10_000;

/// One cell in the 2-D liquidation heatmap grid.
#[derive(Clone, Copy, Debug)]
pub struct LiquidationCell {
    /// Bucket start time (ms, wall clock from the exchange event).
    pub ts_ms: i64,
    /// `price / tick_size` rounded to i64 — identifies the price row.
    pub price_tick: i64,
    /// USD value of long liquidations in this bucket × price row.
    pub long_value: f64,
    /// USD value of short liquidations in this bucket × price row.
    pub short_value: f64,
}

/// Rolling ring buffer of `LiquidationCell` values.
#[derive(Clone, Debug)]
pub struct LiquidationHeatmapBuffer {
    /// Instrument tick size (e.g. 0.1 for BTCUSDT).
    pub tick_size: f64,
    /// Time bucket width in ms.
    pub time_bucket_ms: i64,
    /// Cell ring — newest at back.
    pub cells: VecDeque<LiquidationCell>,
    /// Maximum cells before oldest are evicted.
    pub max_cells: usize,
    /// Running maximum of (long_value + short_value) across all cells.
    /// EMA-decayed (α = 0.001 per push) because liquidation events are sparser than OB.
    pub max_volume_seen: f64,
}

impl LiquidationHeatmapBuffer {
    /// Create an empty buffer with the given tick size.
    pub fn new(tick_size: f64) -> Self {
        let ts = if tick_size > 0.0 { tick_size } else { 1.0 };
        Self {
            tick_size: ts,
            time_bucket_ms: DEFAULT_BUCKET_MS,
            cells: VecDeque::new(),
            max_cells: MAX_CELLS,
            max_volume_seen: 0.0,
        }
    }

    /// Snap `ts_ms` to the nearest bucket boundary.
    fn bucket_of(&self, ts_ms: i64) -> i64 {
        (ts_ms / self.time_bucket_ms) * self.time_bucket_ms
    }

    /// Quantise `price` to a price_tick integer.
    fn price_tick_of(&self, price: f64) -> i64 {
        (price / self.tick_size).round() as i64
    }

    /// Push a single liquidation event (from `LiquidationPoint`).
    ///
    /// `side`: 0 = short liquidated (Buy-side forced close), 1 = long liquidated (Sell-side).
    /// `value_usd`: `LiquidationPoint.value` (NaN when absent — treated as 0).
    pub fn push_liquidation(&mut self, ts_ms: i64, price: f64, value_usd: f64, side: u8) {
        let bucket = self.bucket_of(ts_ms);
        let price_tick = self.price_tick_of(price);
        let usd = if value_usd.is_finite() { value_usd } else { 0.0 };
        let (long_delta, short_delta) = if side == 1 {
            // Sell-side forced close = long position liquidated
            (usd, 0.0)
        } else {
            // Buy-side forced close = short position liquidated
            (0.0, usd)
        };
        // Try to merge into last cell with same (bucket, price_tick).
        if let Some(last) = self.cells.back_mut() {
            if last.ts_ms == bucket && last.price_tick == price_tick {
                last.long_value += long_delta;
                last.short_value += short_delta;
                let total = last.long_value + last.short_value;
                self.update_max(total);
                return;
            }
        }
        let total = long_delta + short_delta;
        self.update_max(total);
        self.push_cell(LiquidationCell { ts_ms: bucket, price_tick, long_value: long_delta, short_value: short_delta });
    }

    /// Push from a `LiquidationBucketPoint`.
    ///
    /// Field mapping (real dig3 field names):
    /// - `point.ts_ms` — bucket timestamp
    /// - `point.long_liq_usd` — long liquidation USD (NaN when absent)
    /// - `point.short_liq_usd` — short liquidation USD (NaN when absent)
    ///
    /// Bucket points carry no price, so they feed a sentinel price_tick=0 cell
    /// used for the time-series strip at the bottom of the heatmap.
    pub fn push_bucket(
        &mut self,
        ts_ms: i64,
        long_liq_usd: f64,
        short_liq_usd: f64,
    ) {
        let bucket = self.bucket_of(ts_ms);
        let long_v = if long_liq_usd.is_finite() { long_liq_usd } else { 0.0 };
        let short_v = if short_liq_usd.is_finite() { short_liq_usd } else { 0.0 };
        let total = long_v + short_v;
        self.update_max(total);
        // price_tick=0 marks a bucket-level aggregate (no per-price resolution).
        self.push_cell(LiquidationCell { ts_ms: bucket, price_tick: 0, long_value: long_v, short_value: short_v });
    }

    /// Update `max_volume_seen` with EMA decay.
    fn update_max(&mut self, total: f64) {
        // Slower decay than C12 (0.001 vs 0.01) because liquidation events are sparser.
        self.max_volume_seen = self.max_volume_seen * 0.999 + total * 0.001;
        if total > self.max_volume_seen {
            self.max_volume_seen = total;
        }
    }

    /// Push a single cell, enforcing the ring cap.
    fn push_cell(&mut self, cell: LiquidationCell) {
        self.cells.push_back(cell);
        while self.cells.len() > self.max_cells {
            self.cells.pop_front();
        }
    }

    /// Iterate cells within a time range (inclusive).
    pub fn visible_cells(&self, t_start_ms: i64, t_end_ms: i64) -> impl Iterator<Item = &LiquidationCell> {
        self.cells.iter().filter(move |c| c.ts_ms >= t_start_ms && c.ts_ms <= t_end_ms)
    }

    /// Number of distinct time buckets stored.
    pub fn bucket_count(&self) -> usize {
        let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
        for c in &self.cells {
            seen.insert(c.ts_ms);
        }
        seen.len()
    }

    /// Oldest bucket timestamp stored, or 0 if empty.
    pub fn oldest_bucket_ms(&self) -> i64 {
        self.cells.front().map(|c| c.ts_ms).unwrap_or(0)
    }

    /// Newest bucket timestamp stored, or 0 if empty.
    pub fn newest_bucket_ms(&self) -> i64 {
        self.cells.back().map(|c| c.ts_ms).unwrap_or(0)
    }
}

/// One liquidation event positioned at its real `(ts_ms, price)`.
///
/// Used for the dot overlay layer on top of the heatmap.
#[derive(Clone, Copy, Debug)]
pub struct LiquidationDot {
    /// Exchange timestamp (ms).
    pub ts_ms: i64,
    /// Liquidation price.
    pub price: f64,
    /// USD value of the liquidation (`LiquidationPoint.value`; NaN → 0).
    pub value_usd: f64,
    /// Side encoding (mirrors dig3): `0` = short liquidated (green), `1` = long liquidated (red).
    pub side: u8,
}
