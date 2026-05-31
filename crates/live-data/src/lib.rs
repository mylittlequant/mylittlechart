//! Live data bridge — connects async V5 exchange connectors to the sync chart.
//!
//! # Architecture
//!
//! ```text
//! DataBridge (owns tokio runtime)
//!   ├── ConnectorPool (DashMap<ExchangeId, Arc<AnyConnector>>)
//!   ├── request_bars()  → spawns async get_klines → sends LiveUpdate
//!   └── request_bars_blocking() → blocks on async call
//!
//! LiveDataProvider (implements sync DataProvider)
//!   ├── cache: RwLock<HashMap<(symbol, tf), Vec<Bar>>>
//!   └── get_bars() → cache hit: return, miss: fire async request
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use live_data::{DataBridge, LiveDataProvider, LiveUpdate};
//! use digdigdig3::ExchangeId;
//! use std::sync::Arc;
//!
//! // Create the bridge (one per application)
//! let shared_series = bar_service::SharedSeriesMap::default();
//! let (bridge, mut rx, _ready_rx) = DataBridge::new(shared_series);
//! let bridge = Arc::new(bridge);
//!
//! // Start a connector (non-blocking)
//! bridge.ensure_connector(ExchangeId::Binance);
//!
//! // Build a provider for a specific exchange
//! let provider = Arc::new(LiveDataProvider::new(
//!     ExchangeId::Binance,
//!     "Binance",
//!     Arc::clone(&bridge),
//! ));
//!
//! // Each frame: drain updates and feed bars into the provider
//! while let Ok(update) = rx.try_recv() {
//!     if let LiveUpdate::BarsLoaded { symbol, timeframe, bars, .. } = update {
//!         provider.insert_bars(&symbol, &timeframe, bars);
//!     }
//! }
//! ```

mod bridge;
mod depth_book;
mod provider;
mod convert;
mod ws_manager;

pub use bridge::{DataBridge, LiveUpdate, OrderbookSource, account_type_from_short_label};
pub use provider::LiveDataProvider;
pub use convert::{kline_to_bar, timeframe_to_interval};

// ── Debug log gate (MLC_PERF_LOG) ────────────────────────────────────────────
static DEBUG_LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub fn debug_log_enabled() -> bool {
    *DEBUG_LOG.get_or_init(|| std::env::var("MLC_PERF_LOG").is_ok())
}

macro_rules! dlog {
    ($($arg:tt)*) => {
        if $crate::debug_log_enabled() {
            eprintln!($($arg)*);
        }
    };
}
pub(crate) use dlog;

// ── Process-relative clock (startup-freeze diagnostics) ──────────────────────
// A single monotonic origin captured the first time it is queried. Bridge
// startup logs stamp every connect/symbol milestone with `elapsed_ms()` so the
// connect wave can be lined up against the skeleton→live promote on the main
// thread (which logs its own wall-clock-relative timestamps). Lazily init'd so
// it costs nothing when MLC_PERF_LOG is off and the logs never fire.
static PROCESS_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Milliseconds since the first call to this function (≈ process / bridge init).
pub fn elapsed_ms() -> u128 {
    PROCESS_START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis()
}

/// A broadcast receiver for [`LiveUpdate`] messages.
///
/// Re-exported here so that crates that depend on `live-data` but not directly
/// on `tokio` can hold a receiver without an explicit `tokio` dependency.
pub type LiveUpdateReceiver = tokio::sync::broadcast::Receiver<LiveUpdate>;

/// A dedicated mpsc receiver for `ConnectorReady` exchange IDs.
///
/// Returned by [`DataBridge::new`] alongside the broadcast receiver.
/// Using a separate mpsc channel for `ConnectorReady` means the app-level
/// consumer does not need to hold an open broadcast subscription, so the
/// broadcast buffer is never held back by a slow app-level drain loop.
pub type ConnectorReadyReceiver = tokio::sync::mpsc::UnboundedReceiver<digdigdig3::ExchangeId>;

// Re-export key V5 types for convenience so callers need fewer direct
// dependencies on `connectors-v5`.
pub use digdigdig3::{ExchangeId, AccountType, Symbol};
pub use digdigdig3::SymbolInfo;
