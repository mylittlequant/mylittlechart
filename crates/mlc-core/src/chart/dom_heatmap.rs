//! DOM Heatmap data — resting order-book depth as a time-by-price grid,
//! drawn as a chart OVERLAY (Bookmap / TapeSurf "Order Book heatmap").
//!
//! ## The grid is keyed to the chart's BARS (2026-08-02)
//!
//! It used to be keyed to fixed 250 ms buckets, each treated as one virtual
//! "bar" so the renderer could reuse the index-axis geometry. That was
//! coherent while this was its own `chart_type` — the viewport's bars WERE
//! the buckets. As an overlay on candles it cannot work: one heat column is
//! 250 ms while one chart column is a whole bar (240× wider on a 1-minute
//! chart), and a 10 k-cell ring at 4 snapshots/s held about two minutes of
//! history where the reference shows hours.
//!
//! So a cell is now `(bar_start_ms, price_row)`, with `bar_ms` taken from
//! the bubble's timeframe. The heat columns line up with the candles, which
//! is exactly what the reference does — its `1m 5m 15m 30m 1h` buttons
//! re-grain the heat together with the series.
//!
//! **A cell holds the MAXIMUM depth seen at that price during the bar**, not
//! the last snapshot: the question a liquidity heatmap answers is "how much
//! was resting here", and a wall that is pulled a moment before the close
//! still mattered.
//!
//! `price_row` groups `row_ticks` instrument ticks together, so one row is a
//! few pixels tall instead of a sub-pixel hairline, and the ring holds a
//! screenful of bars instead of a screenful of ticks.

use std::collections::VecDeque;

/// Maximum cells retained in the rolling ring — about 300 bar columns of
/// ~120 price rows, i.e. a full screen of history at any normal zoom.
pub const MAX_CELLS: usize = 40_000;

/// Default price ticks folded into one heat row.
pub const DEFAULT_ROW_TICKS: i64 = 10;

/// One cell in the 2-D heatmap grid: one bar column × one price row.
#[derive(Clone, Copy, Debug)]
pub struct DomHeatmapCell {
    /// Start time of the bar this cell belongs to (ms).
    pub ts_ms: i64,
    /// `price / (tick_size * row_ticks)` rounded — identifies the price row.
    pub price_row: i64,
    /// Peak resting bid depth seen at this row during this bar.
    pub bid_depth: f64,
    /// Peak resting ask depth seen at this row during this bar.
    pub ask_depth: f64,
}

impl DomHeatmapCell {
    /// Price at the centre of this row, given the buffer's geometry.
    pub fn price(&self, tick_size: f64, row_ticks: i64) -> f64 {
        self.price_row as f64 * tick_size * row_ticks.max(1) as f64
    }
}

/// Rolling ring buffer of `DomHeatmapCell` values.
#[derive(Clone, Debug)]
pub struct DomHeatmapBuffer {
    /// Instrument tick size (e.g. 0.1 for BTCUSDT on Binance).
    pub tick_size: f64,
    /// Column width in ms — the bubble's bar duration. A timeframe change
    /// invalidates the whole grid; the writer rebuilds instead of mixing
    /// two grains.
    pub bar_ms: i64,
    /// Instrument ticks folded into one price row.
    pub row_ticks: i64,
    /// Cell ring — newest bar at back.
    pub cells: VecDeque<DomHeatmapCell>,
    /// Maximum cells before oldest are evicted.
    pub max_cells: usize,
    /// Bar the tail cells belong to, and how many of them there are — the
    /// merge window for the snapshot being folded in. Snapshots arrive in
    /// time order, so everything mergeable is at the tail.
    cur_bar_ms: i64,
    cur_bar_len: usize,
}

impl DomHeatmapBuffer {
    /// Create an empty buffer for a given instrument tick, bar duration and
    /// row grouping.
    pub fn new(tick_size: f64, bar_ms: i64, row_ticks: i64) -> Self {
        Self {
            tick_size: if tick_size > 0.0 { tick_size } else { 1.0 },
            bar_ms: bar_ms.max(1),
            row_ticks: row_ticks.max(1),
            cells: VecDeque::new(),
            max_cells: MAX_CELLS,
            cur_bar_ms: i64::MIN,
            cur_bar_len: 0,
        }
    }

    /// Whether this buffer's geometry still matches what the caller wants.
    /// A mismatch means the grid must be rebuilt, not appended to.
    pub fn matches_geometry(&self, bar_ms: i64, row_ticks: i64) -> bool {
        self.bar_ms == bar_ms.max(1) && self.row_ticks == row_ticks.max(1)
    }

    /// Height of one price row in instrument price units.
    pub fn row_size(&self) -> f64 {
        self.tick_size * self.row_ticks as f64
    }

    /// Snap `ts_ms` to the start of its bar.
    fn bar_of(&self, ts_ms: i64) -> i64 {
        ts_ms.div_euclid(self.bar_ms) * self.bar_ms
    }

    /// Quantise `price` to a price-row index.
    fn row_of(&self, price: f64) -> i64 {
        (price / self.row_size()).round() as i64
    }

    /// Fold a full order-book snapshot into the grid.
    ///
    /// `bids` / `asks` are `(price, qty)` pairs. Depth is summed across the
    /// ticks that share a row within this snapshot, then merged into the
    /// bar's cell by MAXIMUM — see the module doc.
    pub fn push_snapshot(&mut self, ts_ms: i64, bids: &[(f64, f64)], asks: &[(f64, f64)]) {
        let bar = self.bar_of(ts_ms);
        if bar != self.cur_bar_ms {
            self.cur_bar_ms = bar;
            self.cur_bar_len = 0;
        }

        use std::collections::HashMap;
        let mut by_row: HashMap<i64, (f64, f64)> = HashMap::new();
        for &(price, qty) in bids {
            if qty > 0.0 {
                by_row.entry(self.row_of(price)).or_insert((0.0, 0.0)).0 += qty;
            }
        }
        for &(price, qty) in asks {
            if qty > 0.0 {
                by_row.entry(self.row_of(price)).or_insert((0.0, 0.0)).1 += qty;
            }
        }

        for (price_row, (bid_depth, ask_depth)) in by_row {
            let tail_start = self.cells.len() - self.cur_bar_len;
            let existing = self
                .cells
                .iter_mut()
                .skip(tail_start)
                .find(|c| c.price_row == price_row);
            match existing {
                Some(cell) => {
                    cell.bid_depth = cell.bid_depth.max(bid_depth);
                    cell.ask_depth = cell.ask_depth.max(ask_depth);
                }
                None => {
                    self.cells.push_back(DomHeatmapCell {
                        ts_ms: bar,
                        price_row,
                        bid_depth,
                        ask_depth,
                    });
                    self.cur_bar_len += 1;
                    while self.cells.len() > self.max_cells {
                        self.cells.pop_front();
                        // The evicted cell is the oldest bar's, never the
                        // current one, unless the ring is smaller than a
                        // single bar's row count — guard anyway.
                        self.cur_bar_len = self.cur_bar_len.min(self.cells.len());
                    }
                }
            }
        }
    }

    /// Number of distinct bar columns stored. Reported as
    /// `dom_heatmap_columns` by the agent API / test harness.
    pub fn bucket_count(&self) -> usize {
        let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
        for c in &self.cells {
            seen.insert(c.ts_ms);
        }
        seen.len()
    }

    /// Oldest bar timestamp stored, or 0 if empty.
    pub fn oldest_bucket_ms(&self) -> i64 {
        self.cells.front().map(|c| c.ts_ms).unwrap_or(0)
    }

    /// Newest bar timestamp stored, or 0 if empty.
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
