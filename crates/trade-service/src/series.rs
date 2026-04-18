use std::collections::VecDeque;
use trade_store::Trade;

/// Default maximum trades held in memory per series.
pub const DEFAULT_CAPACITY: usize = 100_000;

/// A single trade ring buffer with lifecycle management.
///
/// Mutated only by `TradeService`. Panels hold `Arc<RwLock<TradeSeries>>`
/// and call `.read()` during render — never write through the guard.
pub struct TradeSeries {
    /// The ring buffer. Front = oldest, back = newest.
    pub trades: VecDeque<Trade>,

    /// Incremented on every mutation (push, merge, rotate).
    /// Panels track their `last_seen_version` to skip redundant recalcs.
    pub version: u64,

    /// Maximum number of trades kept in memory. When exceeded, old trades are
    /// rotated out before being removed.
    pub capacity: usize,

    /// Timestamp (ms) of the most recent trade in the ring.
    pub last_ts_ms: i64,

    /// True when in-memory trades have been mutated since the last disk flush.
    /// Set by every mutation; cleared by `TradeService` on flush.
    pub dirty: bool,

    /// Timestamp (ms) of the oldest trade that was rotated out.
    /// `None` until the first rotation happens.
    pub oldest_rotated_ts_ms: Option<i64>,

    /// Active panel subscription count.
    ///
    /// Incremented by `TradeService::acquire()`, decremented by `release()`.
    /// When it reaches 0 the series can be dropped and the WS stream stopped.
    pub refcount: usize,
}

impl TradeSeries {
    pub fn new(capacity: usize) -> Self {
        Self {
            trades: VecDeque::with_capacity(capacity.min(1024)),
            version: 0,
            capacity,
            last_ts_ms: 0,
            dirty: false,
            oldest_rotated_ts_ms: None,
            refcount: 0,
        }
    }

    /// Number of trades currently in memory.
    pub fn len(&self) -> usize {
        self.trades.len()
    }

    pub fn is_empty(&self) -> bool {
        self.trades.is_empty()
    }

    /// Most recent trade (back of VecDeque = newest).
    pub fn last(&self) -> Option<&Trade> {
        self.trades.back()
    }

    /// Oldest in-memory trade.
    pub fn first(&self) -> Option<&Trade> {
        self.trades.front()
    }

    /// Two-slice view of the ring buffer — no allocation.
    ///
    /// Callers that need a contiguous slice should use `to_vec()`.
    pub fn as_slices(&self) -> (&[Trade], &[Trade]) {
        self.trades.as_slices()
    }

    /// Collect to a `Vec<Trade>` for callers that need a contiguous slice
    /// (e.g. disk flush).
    pub fn to_vec(&self) -> Vec<Trade> {
        self.trades.iter().copied().collect()
    }
}
