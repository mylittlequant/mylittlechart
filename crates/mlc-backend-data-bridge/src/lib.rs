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
//! use mlc_backend_data_bridge::{DataBridge, LiveDataProvider, LiveUpdate};
//! use digdigdig3::ExchangeId;
//! use std::sync::Arc;
//!
//! // Create the bridge (one per application). Bars, trades, and orderbooks
//! // live in dig3-station rings / OrderBookTracker; the bridge manages them.
//! let (bridge, mut rx, _ready_rx) = DataBridge::new().await;
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
mod provider;
mod convert;
pub mod backfill_engine;

pub use bridge::{
    DataBridge, LiveUpdate, OrderbookSource, account_type_from_short_label,
    SharedTradeSeries, SharedOrderBookTracker, HistoryDepthWire,
    RawTradeWire,
};
pub use backfill_engine::{BackfillEngine, BackfillKey, LayerId, RampState};
pub use provider::LiveDataProvider;
pub use convert::{
    account_type_from_dig3, account_type_to_dig3, kline_to_bar, resolve_exchange, timeframe_to_interval,
    to_dig3, UnknownExchange,
};

// ── Debug log gate (MLC_PERF_LOG) ────────────────────────────────────────────
static DEBUG_LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub fn debug_log_enabled() -> bool {
    // On wasm, std::env::var is unavailable and always returns Err — but we
    // want bridge logs visible by default in the browser (no terminal to
    // pipe MLC_PERF_LOG into). Default ON for wasm, OFF for native unless
    // MLC_PERF_LOG is set.
    #[cfg(target_arch = "wasm32")]
    { *DEBUG_LOG.get_or_init(|| true) }
    #[cfg(not(target_arch = "wasm32"))]
    { *DEBUG_LOG.get_or_init(|| std::env::var("MLC_PERF_LOG").is_ok()) }
}

#[cfg(target_arch = "wasm32")]
pub fn console_log_owned(s: String) {
    web_sys::console::log_1(&s.into());
}

#[macro_export]
macro_rules! dlog {
    ($($arg:tt)*) => {
        if $crate::debug_log_enabled() {
            #[cfg(target_arch = "wasm32")]
            { $crate::console_log_owned(format!($($arg)*)); }
            #[cfg(not(target_arch = "wasm32"))]
            { eprintln!($($arg)*); }
        }
    };
}

// ── Archive CORS-proxy base (wasm day-file fetch, Э6-Ф4) ─────────────────────
// Browsers cannot cross-origin GET the archive hosts (`public.bybit.com`,
// `data.binance.vision`) directly — `backfill_engine::run_trade_ramp`'s
// archive-chaining path routes wasm day-file fetches through a CORS proxy
// (`digdigdig3-proxy`, prefix mode) instead. Native fetches pass `None`
// through unchanged (direct HTTPS, no CORS concern off-browser).
//
// `RwLock<Option<String>>` rather than `OnceLock` — unlike `DEBUG_LOG` (an
// env var, fixed for the process lifetime), this value is a live-editable
// profile setting (`UserProfile.domain.data_load.archive_proxy_base`) that
// can change if the user edits settings and the profile is re-applied, so
// the setter must be callable more than once.
static ARCHIVE_PROXY_BASE: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

/// Set the CORS proxy base used for wasm archive day-file fetches. Called
/// from the profile-apply path (`ChartApp::apply_profile_state` and
/// equivalent shell-side profile hydration) with
/// `profile.domain.data_load.archive_proxy_base`. A no-op write lock
/// poisoning (a prior panic while holding the lock) falls back to the
/// poisoned guard's data rather than propagating — losing one proxy-base
/// update is preferable to panicking the whole profile-apply path.
pub fn set_archive_proxy_base(base: String) {
    let mut guard = ARCHIVE_PROXY_BASE.write().unwrap_or_else(|e| e.into_inner());
    *guard = if base.is_empty() { None } else { Some(base) };
}

/// Current CORS proxy base, if one has been set via
/// [`set_archive_proxy_base`]. `None` before the profile has been applied
/// once (native archive fetches never read this — they always pass `None`
/// through regardless of this value).
pub fn archive_proxy_base() -> Option<String> {
    ARCHIVE_PROXY_BASE
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

// ── Process-relative clock (startup-freeze diagnostics) ──────────────────────
// A single monotonic origin captured the first time it is queried. Bridge
// startup logs stamp every connect/symbol milestone with `elapsed_ms()` so the
// connect wave can be lined up against the skeleton→live promote on the main
// thread (which logs its own wall-clock-relative timestamps). Lazily init'd so
// it costs nothing when MLC_PERF_LOG is off and the logs never fire.
static PROCESS_START: std::sync::OnceLock<web_time::Instant> = std::sync::OnceLock::new();

/// Milliseconds since the first call to this function (≈ process / bridge init).
pub fn elapsed_ms() -> u128 {
    PROCESS_START
        .get_or_init(web_time::Instant::now)
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
pub use digdigdig3_station::TpoSource;
