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

pub use bridge::{DataBridge, LiveUpdate, account_type_from_short_label};
pub use provider::LiveDataProvider;
pub use convert::{kline_to_bar, timeframe_to_interval};

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
