mod series;
mod types;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub use series::{OrderbookSnapshot, OrderbookSeries, OrderbookView, DEFAULT_HISTORY_CAPACITY};
pub use types::{OrderbookKey, TimedSnapshot};

// Re-export so panel crates need no direct arc-swap dependency.
pub use arc_swap::ArcSwap;

/// Shared orderbook series map — cloneable handle.
///
/// Owned by `DataBridge`. Populated lazily as windows subscribe to depth streams.
/// Each entry is a per-(exchange,symbol,account_type) `OrderbookSeries` wrapped
/// in `Arc<RwLock<...>>` for cross-thread reads.
pub type SharedOrderbookMap = Arc<RwLock<HashMap<OrderbookKey, Arc<RwLock<OrderbookSeries>>>>>;
