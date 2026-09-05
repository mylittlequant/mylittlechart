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
pub mod kline_agg;
pub mod zip_coverage;

pub use bridge::{
    DataBridge, LiveUpdate, OrderbookSource, account_type_from_short_label,
    SharedTradeSeries, SharedOrderBookTracker, SharedLiquidationSeries,
    HistoryDepthWire, RawTradeWire, oi_history_period,
};
pub use backfill_engine::{BackfillEngine, BackfillKey, LayerId, RampState, TradeDemand, TradeLadderStage};
pub use kline_agg::{
    aggregation_base, bucket_start_ms, fold_bars_into_buckets, fold_trades_into_bars,
    is_trade_built, MAX_AGGREGATION_RATIO,
};
pub use provider::LiveDataProvider;
pub use convert::{
    account_type_from_dig3, account_type_to_dig3, interval_to_ms, kline_to_bar, resolve_exchange,
    timeframe_to_interval, to_dig3, UnknownExchange,
};

// ── Debug log gate (MLC_PERF_LOG) ────────────────────────────────────────────
static DEBUG_LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub fn debug_log_enabled() -> bool {
    // Native: OFF unless MLC_PERF_LOG is set. Wasm: OFF unless the shell
    // called `enable_debug_log(true)` at boot — it does so when the page was
    // opened with `?log=debug|trace` (or localStorage "mlc.log"). It used to
    // be unconditionally ON in the browser ("no terminal to pipe into"),
    // which shipped every `dlog!` line to every visitor's console; owner
    // ruling 2026-09-05: diagnostics are opt-in, nothing prints by default.
    #[cfg(target_arch = "wasm32")]
    { DEBUG_LOG.get().copied().unwrap_or(false) }
    #[cfg(not(target_arch = "wasm32"))]
    { *DEBUG_LOG.get_or_init(|| std::env::var("MLC_PERF_LOG").is_ok()) }
}

/// Wasm shells call this once at boot with the outcome of their `?log=`
/// parsing; a second call is ignored (the gate is a `OnceLock`). Native
/// ignores it too — the environment decides there.
pub fn enable_debug_log(on: bool) {
    let _ = DEBUG_LOG.set(on);
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
///
/// On wasm a same-origin relative base (the `"/archive-proxy"` default) is
/// absolutized against the page origin: reqwest parses request URLs with
/// `url::Url::parse`, which rejects relative URLs — the request died at
/// BUILD time ("builder error", zero packets sent) until the base became
/// absolute.
pub fn archive_proxy_base() -> Option<String> {
    if !archive_proxy_allowed() {
        return None;
    }
    ARCHIVE_PROXY_BASE
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .map(|b| absolutize_proxy_base(&b))
}

// ── Archive-proxy entitlement (must-proxy + future paid relay) ───────────────
//
// One gate for the WHOLE `/archive-proxy` prefix: CORS-blocked live REST
// (must-proxy) AND archive CDNs AND a future geo-relay. Owner doctrine
// 2026-08-28: today the gate is `Open` (must-proxy stays in the free catalog);
// flipping to `Entitled` later locks every proxy use behind a paid claim —
// regular registered users then lose must-proxy and relay together.
//
// Client-side is a routing hint (wasm stops sending). The real lock is nginx
// `auth_request` once `MLC_PROXY_GATE=entitled` (see
// docs/mlc/plans/archive-proxy-entitlement-2026-08-28.md).

/// Who may use `/archive-proxy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveProxyGate {
    /// Everyone, including anonymous. Current production default.
    Open,
    /// Only sessions marked entitled. Must-proxy and relay both require this.
    Entitled,
}

static ARCHIVE_PROXY_GATE: std::sync::RwLock<ArchiveProxyGate> =
    std::sync::RwLock::new(ArchiveProxyGate::Open);
static ARCHIVE_PROXY_ENTITLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

/// Set the process-wide proxy gate. `Open` (default) ignores entitlement.
pub fn set_archive_proxy_gate(gate: ArchiveProxyGate) {
    let mut g = ARCHIVE_PROXY_GATE.write().unwrap_or_else(|e| e.into_inner());
    *g = gate;
}

pub fn archive_proxy_gate() -> ArchiveProxyGate {
    *ARCHIVE_PROXY_GATE.read().unwrap_or_else(|e| e.into_inner())
}

/// Paid/session claim. Ignored while the gate is [`ArchiveProxyGate::Open`].
pub fn set_archive_proxy_entitled(entitled: bool) {
    ARCHIVE_PROXY_ENTITLED.store(entitled, std::sync::atomic::Ordering::Relaxed);
}

pub fn archive_proxy_entitled() -> bool {
    ARCHIVE_PROXY_ENTITLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether this process may send traffic through `/archive-proxy`.
pub fn archive_proxy_allowed() -> bool {
    match archive_proxy_gate() {
        ArchiveProxyGate::Open => true,
        ArchiveProxyGate::Entitled => archive_proxy_entitled(),
    }
}

/// True when the wasm page is served from loopback (DEV http-server).
/// Native is never a browser CORS client.
#[cfg(target_arch = "wasm32")]
pub fn is_loopback_dev() -> bool {
    web_sys::window()
        .and_then(|w| w.location().hostname().ok())
        .map(|h| h == "127.0.0.1" || h == "localhost" || h == "[::1]")
        .unwrap_or(false)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn is_loopback_dev() -> bool {
    false
}

/// Local d-watchdog CORS proxy (`digdigdig3-proxy`).
pub const LOCAL_CORS_PROXY: &str = "http://127.0.0.1:17800";

/// Absolutize a same-origin relative proxy base (starts with `/`) against
/// the page origin. Identity on native and for already-absolute bases.

#[cfg(target_arch = "wasm32")]
pub fn absolutize_proxy_base(base: &str) -> String {
    // DEV stand on :17499 has no nginx `/archive-proxy`. Route through the
    // loopback dig3 proxy (ACAO *) so CORS-blocked venues actually load.
    if is_loopback_dev() && (base.starts_with('/') || base.is_empty()) {
        return LOCAL_CORS_PROXY.to_string();
    }
    if base.starts_with('/') {
        if let Some(origin) = web_sys::window().and_then(|w| w.location().origin().ok()) {
            return format!("{origin}{base}");
        }
    }
    base.to_string()
}

/// Native builds never proxy (direct HTTPS), so the base passes through.
#[cfg(not(target_arch = "wasm32"))]
pub fn absolutize_proxy_base(base: &str) -> String {
    base.to_string()
}

// ── Raw-trade retention window (owner rule 2026-07-15) ───────────────────────
// THE single raw-trade depth bound, in UTC days: the shared `TradeStore`
// evicts raw rows older than this window, and the backfill planner refuses
// to fetch DELTA (REST) depth below it — depth beyond the window is either
// zip-covered (fetched, folded, raw dropped — re-pullable from the immutable
// archive) or not obtainable at all (no-zip venue → latch dry + gap band).
// Replaced the three uncoordinated limits (engine 1.5M cumulative point cap,
// store 1M count ring, demand-widened time horizon) after the 2026-07-15
// live incident wedged all three against each other. `AtomicU32` — a live
// profile setting, same re-apply semantics as `ARCHIVE_PROXY_BASE`.
static TRADE_RAW_WINDOW_DAYS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(4);

/// Set the raw-trade retention window (days). Called from the profile-apply
/// path with `profile.domain.data_load.trade_raw_window_days`. Values are
/// clamped to at least 1 — a zero window would evict the live head itself.
pub fn set_trade_raw_window_days(days: u32) {
    TRADE_RAW_WINDOW_DAYS.store(days.max(1), std::sync::atomic::Ordering::Relaxed);
}

/// Current raw-trade retention window in days (default 4 before the profile
/// has been applied once).
pub fn trade_raw_window_days() -> u32 {
    TRADE_RAW_WINDOW_DAYS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Current raw-trade retention window in milliseconds — the form the
/// planner's fetch-floor arithmetic and the store's eviction both consume.
pub fn trade_raw_window_ms() -> i64 {
    trade_raw_window_days() as i64 * 86_400_000
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
