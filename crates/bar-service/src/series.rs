use std::collections::VecDeque;
use zengeld_chart::Bar;

/// Default maximum bars held in memory per series.
pub const DEFAULT_CAPACITY: usize = 10_000;

/// A single OHLCV time series with ring-buffer memory management.
///
/// Mutated only by `BarService`. Windows hold `Arc<RwLock<BarSeries>>`
/// and call `.read()` during render — never write through the guard.
pub struct BarSeries {
    /// The ring buffer. Front = oldest, back = newest.
    pub bars: VecDeque<Bar>,

    /// Incremented on every mutation (push, update, merge, rotate).
    /// Windows track their `last_seen_version` to skip redundant recalcs.
    pub version: u64,

    /// Maximum number of bars kept in memory. When exceeded, old bars are
    /// rotated out before being removed.
    pub capacity: usize,

    /// Timestamp (seconds) of the trade that last updated the current (last) bar.
    /// Used for candle boundary detection in trade aggregation.
    pub last_trade_ts: i64,

    /// True when in-memory bars have been mutated since the last disk flush.
    /// Set by every mutation; cleared by `BarService` on flush.
    pub dirty: bool,

    /// Timestamp of the oldest bar that was rotated out.
    /// `None` until the first rotation happens.
    pub oldest_rotated_ts: Option<i64>,

    /// Timeframe period in seconds (derived from `Timeframe.minutes * 60`).
    /// Cached here so trade aggregation does not need the `Timeframe` struct.
    pub period_secs: i64,
}

impl BarSeries {
    pub fn new(capacity: usize, period_secs: i64) -> Self {
        Self {
            bars: VecDeque::with_capacity(capacity.min(1024)),
            version: 0,
            capacity,
            last_trade_ts: 0,
            dirty: false,
            oldest_rotated_ts: None,
            period_secs,
        }
    }

    /// Number of bars currently in memory.
    pub fn len(&self) -> usize {
        self.bars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bars.is_empty()
    }

    /// Most recent bar (last in VecDeque = newest).
    pub fn last(&self) -> Option<&Bar> {
        self.bars.back()
    }

    /// Oldest in-memory bar.
    pub fn first(&self) -> Option<&Bar> {
        self.bars.front()
    }

    /// Two-slice view of the ring buffer — no allocation.
    ///
    /// Callers that need a contiguous slice should use `to_vec()`.
    pub fn as_slices(&self) -> (&[Bar], &[Bar]) {
        self.bars.as_slices()
    }

    /// Collect to a `Vec<Bar>` for callers that need a contiguous slice
    /// (e.g. disk flush, indicator calculation from outside `BarService`).
    pub fn to_vec(&self) -> Vec<Bar> {
        self.bars.iter().copied().collect()
    }
}
