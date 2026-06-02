mod series;
mod tracked_handle;
mod types;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub use series::{BarSeries, DEFAULT_CAPACITY};
pub use tracked_handle::TrackedSeriesHandle;
pub use types::BarSeriesKey;
pub use zengeld_chart::Bar;

/// Shared series registry — cloneable handle.
///
/// Owned by `DataBridge`. Populated lazily as windows subscribe to bars
/// through `request_bars`. Each entry is a per-series ring buffer
/// (`BarSeries`) wrapped in `Arc<RwLock<...>>` for cross-thread reads.
pub type SharedSeriesMap = Arc<RwLock<HashMap<BarSeriesKey, Arc<RwLock<BarSeries>>>>>;
