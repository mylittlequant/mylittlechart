//! DOM Heatmap data types for the Bookmap-style chart-type (C12).
//!
//! Stores a rolling 2D buffer of resting order-book depth keyed by
//! `(time_bucket, price_tick)`.  A companion `DomTradeDot` vector
//! provides the trade-circle overlay.
//!
//! Design: **virtual-index axis**.  Each 250 ms bucket is treated as one
//! virtual "bar" so the chart can reuse the existing index-axis viewport
//! geometry without touching the time_axis path that was reverted in C4.
//! The timestamp field is stored for crosshair labels and future upgrade.

use std::collections::VecDeque;

/// Default bucket width in milliseconds — 4 buckets/s matches Bookmap.
pub const DEFAULT_BUCKET_MS: i64 = 250;

/// Maximum cells retained in the rolling ring.
/// 250 ms × 40 min × (typical 20 price ticks/column) ≈ 10 k cells visible.
pub const MAX_CELLS: usize = 10_000;

/// One cell in the 2-D heatmap grid.
#[derive(Clone, Copy, Debug)]
pub struct DomHeatmapCell {
    /// Bucket start time (ms, wall clock from the exchange event).
    pub ts_ms: i64,
    /// `price / tick_size` rounded to i64 — identifies the price row.
    pub price_tick: i64,
    /// Resting bid depth at this tick during this bucket.
    pub bid_depth: f64,
    /// Resting ask depth at this tick during this bucket.
    pub ask_depth: f64,
}

/// Rolling ring buffer of `DomHeatmapCell` values.
#[derive(Clone, Debug)]
pub struct DomHeatmapBuffer {
    /// Instrument tick size (e.g. 0.1 for BTCUSDT on Binance).
    pub tick_size: f64,
    /// Time bucket width in ms.
    pub time_bucket_ms: i64,
    /// Cell ring — newest at back.
    pub cells: VecDeque<DomHeatmapCell>,
    /// Maximum cells before oldest are evicted.
    pub max_cells: usize,
    /// Running maximum of (bid_depth + ask_depth) across all cells.
    /// Used for color-intensity normalisation.  Decays via EMA (α = 0.99 per push).
    pub max_depth_seen: f64,
}

impl DomHeatmapBuffer {
    /// Create an empty buffer with the given tick size.
    pub fn new(tick_size: f64) -> Self {
        let ts = if tick_size > 0.0 { tick_size } else { 1.0 };
        Self {
            tick_size: ts,
            time_bucket_ms: DEFAULT_BUCKET_MS,
            cells: VecDeque::new(),
            max_cells: MAX_CELLS,
            max_depth_seen: 0.0,
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

    /// Push a full order-book snapshot.
    ///
    /// All `bids` and `asks` slices are `(price, qty)` pairs.
    /// Bucketing: all cells for this snapshot share the same bucket ts.
    pub fn push_snapshot(&mut self, ts_ms: i64, bids: &[(f64, f64)], asks: &[(f64, f64)]) {
        let bucket = self.bucket_of(ts_ms);
        use std::collections::HashMap;
        // Aggregate by price_tick within this snapshot.
        let mut by_tick: HashMap<i64, (f64, f64)> = HashMap::new();
        for &(price, qty) in bids {
            if qty > 0.0 {
                by_tick.entry(self.price_tick_of(price)).or_insert((0.0, 0.0)).0 += qty;
            }
        }
        for &(price, qty) in asks {
            if qty > 0.0 {
                by_tick.entry(self.price_tick_of(price)).or_insert((0.0, 0.0)).1 += qty;
            }
        }
        for (price_tick, (bid_depth, ask_depth)) in by_tick {
            let total = bid_depth + ask_depth;
            // EMA decay on max_depth_seen so one fat snapshot doesn't
            // permanently suppress all subsequent colours (α = 0.99 per push).
            self.max_depth_seen = self.max_depth_seen * 0.99 + total * 0.01;
            if total > self.max_depth_seen {
                self.max_depth_seen = total;
            }
            self.push_cell(DomHeatmapCell { ts_ms: bucket, price_tick, bid_depth, ask_depth });
        }
    }

    /// Push a single cell, enforcing the ring cap.
    fn push_cell(&mut self, cell: DomHeatmapCell) {
        self.cells.push_back(cell);
        while self.cells.len() > self.max_cells {
            self.cells.pop_front();
        }
    }

    /// Iterate cells within a time range (inclusive).
    pub fn visible_cells(&self, t_start_ms: i64, t_end_ms: i64) -> impl Iterator<Item = &DomHeatmapCell> {
        self.cells.iter().filter(move |c| c.ts_ms >= t_start_ms && c.ts_ms <= t_end_ms)
    }

    /// Number of distinct time buckets stored.
    ///
    /// Used to calculate how many virtual "bars" worth of data exist so
    /// the caller can set `viewport.bar_count` appropriately.
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

/// One executed trade positioned at its real (ts, price).
#[derive(Clone, Copy, Debug)]
pub struct DomTradeDot {
    /// Exchange timestamp (ms).
    pub ts_ms: i64,
    /// Trade price.
    pub price: f64,
    /// Trade quantity (used for circle radius scaling).
    pub quantity: f64,
    /// `1` = buyer-initiated (green), `0` = seller-initiated (red).
    pub side: i8,
}
