mod series;
mod types;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub use series::{TradeSeries, DEFAULT_CAPACITY};
pub use types::{TradeKey, Trade};

/// Shared trade ring-map — cloneable handle.
///
/// Owned by `DataBridge`. Populated lazily as windows subscribe to trades.
/// Each entry is a per-(exchange,symbol,account_type) ring buffer wrapped
/// in `Arc<RwLock<...>>` for cross-thread reads.
pub type SharedTradeMap = Arc<RwLock<HashMap<TradeKey, Arc<RwLock<TradeSeries>>>>>;
