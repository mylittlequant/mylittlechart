mod error;
mod format;
mod store;

pub use error::OrderbookStoreError;
pub use store::OrderbookStoreHandle;

/// A full orderbook snapshot captured at a specific moment.
///
/// Stored on disk by `OrderbookStoreHandle` and held in the in-memory
/// history ring of `OrderbookSeries`.
///
/// - `bids`: descending price order (highest bid first)
/// - `asks`: ascending price order (lowest ask first)
#[derive(Debug, Clone)]
pub struct TimedSnapshot {
    pub timestamp_ms: i64,
    pub bids: Vec<(f64, f64)>,  // (price, qty)
    pub asks: Vec<(f64, f64)>,  // (price, qty)
}
