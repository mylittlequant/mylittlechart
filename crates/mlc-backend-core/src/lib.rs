//! `BackendCore` — shared data plane for the chart system.
//!
//! Lives one-per-process. Multiple `ChartApp` instances (i.e. registered
//! AppSession-like consumers) hold `Arc<BackendCore>` and **share** all the
//! data + compute fields below. Connectors don't duplicate, bars don't
//! duplicate, indicator catalogs don't duplicate.
//!
//! This module is staged in `mlc-app` for the cutover so the surgery can
//! ship as one PR without needing a new crate first. Once every shared
//! field is migrated and call sites are updated, the struct moves wholesale
//! into a `mlc-backend-core` crate above `mlc-indicators`.
//!
//! ## Migration plan
//!
//! Fields migrate one session at a time, each with a fully passing build:
//!
//! - **Session 1 (this commit)** — `bridge`, `mini_ticker`, `exchange_symbols`,
//!   `trading`. All four are either already `Arc` or `DashMap` (lock-free) or
//!   `Option<Arc<…>>`. No lock guard discipline required.
//! - **Session 2** — `managers: RwLock<Managers>` (contains `BubbleStore`).
//!   This is the real surgery: 654 call sites, requires lock-guard scoping in
//!   `prepare.rs` (long read frame) and `tick.rs` (17 short write arms).
//! - **Session 3** — `indicators: RwLock<IndicatorManager>`. 169 call sites.
//!   Smaller scope, similar lock discipline.
//! - **Final** — lift module into `mlc-backend-core` crate.

use std::sync::Arc;
use std::collections::HashMap;

use dashmap::DashMap;

use digdigdig3::ExchangeId;
use mlc_core::managers::Managers;
use mlc_core::preset::preset::ChartPreset;
use mlc_core::templates::manager::TemplateManager;
use mlc_indicators::IndicatorManager;
use mlc_live_data::{DataBridge, SymbolInfo};
use mlc_sidebar_content::watchlist::WatchlistManager;
use mlc_trading_manager::TradingManager;

/// Cached 24-hour ticker statistics from the WebSocket mini-ticker stream.
///
/// One entry per `(symbol, exchange, account_type)`, updated every second from
/// `LiveUpdate::MiniTickerUpdate`. Used as a fallback price source in the
/// watchlist sidebar for symbols that do not have a chart window open.
pub struct MiniTickerData {
    pub last_price: f64,
    pub price_change_percent: f64,
    pub high_price: f64,
    pub low_price: f64,
    pub volume: f64,
}

/// Shared data plane owned by every chart-app process. Mode-1 hosts (vello,
/// future tessera-shell, wasm) own one `Arc<BackendCore>` and pass `Arc::clone`
/// to every `ChartApp` they spawn. Mode-2 daemon hosts own one `Arc<BackendCore>`
/// and serve it through the HTTP/WS API.
pub struct BackendCore {
    /// All `mlc-core` managers: bubble store (with bars + 13 derived series +
    /// signals + trades + markers + primitives + indicator_configs), alert
    /// manager, plus stateless signal/trade/marker dispatchers. Held under a
    /// single `RwLock` because their data slices touch the same bubble.
    ///
    /// In single-threaded embedded mode (one main loop = one writer + one
    /// renderer running sequentially) lock contention is zero — guards are
    /// taken and dropped each tick frame, no waits.
    pub managers: std::sync::RwLock<Managers>,

    /// Indicator manager — owns all indicator instances + recalc state +
    /// per-window assignment + global definitions catalog. Cannot live inside
    /// `Managers` (mlc-core) because mlc-indicators is above mlc-core in the
    /// dep graph; that's the architectural reason BackendCore exists.
    pub indicators: std::sync::RwLock<IndicatorManager>,

    /// Live data bridge — owns the tokio runtime, station, connector pool,
    /// broadcast channel. Already internally synchronised; `Arc::clone` is
    /// the only thing consumers need.
    pub bridge: Arc<DataBridge>,

    /// Lock-free per-symbol mini-ticker cache. Read every frame by the
    /// watchlist + by the `LiquidationBucketUpdate` writer arm for mark price.
    /// Writes only from the `MiniTickerUpdate` arm. Using `DashMap` so all
    /// access is lock-free at the per-key level.
    pub mini_ticker: DashMap<String, MiniTickerData>,

    /// Symbol catalogue per exchange — populated asynchronously from
    /// `LiveUpdate::SymbolsLoaded`. Same for every consumer of this backend.
    /// `RwLock` because writes are rare (once per exchange) and reads are
    /// frequent (symbol search overlay, watchlist).
    pub exchange_symbols: std::sync::RwLock<HashMap<ExchangeId, Vec<SymbolInfo>>>,

    /// Order routing + paper engine. `None` when trading is disabled.
    /// Wrapped in `Mutex` because `TradingManager` has `&mut self` methods
    /// (`on_connector_ready`, `tick`, `refresh_balances`, etc) and is mutated
    /// from the tick loop. Single-threaded contention in embedded mode = 0.
    pub trading: Option<std::sync::Mutex<TradingManager>>,

    /// Chart presets — in-memory store keyed by preset id.
    ///
    /// Previously lived on `ChartPanelApp.presets`. Moved here so that the
    /// persist layer, input handlers, and render path all share one authoritative
    /// copy without per-window duplication (single-`BackendCore` hosts).
    pub presets: std::sync::RwLock<HashMap<String, ChartPreset>>,

    /// In-memory template manager — drawing primitive / indicator / compare /
    /// chart templates and indicator sets.
    ///
    /// Previously on `ChartPanelApp.template_manager`. Moved here for the same
    /// reason as `presets`.
    pub template_manager: std::sync::RwLock<TemplateManager>,

    /// Watchlist manager — all named watchlists with their symbol rows.
    ///
    /// Previously on `SidebarState.watchlist_manager`. Moved here so that the
    /// sidebar render, input handlers, and agent snapshot builder all read from
    /// one place.
    pub watchlist_manager: std::sync::RwLock<WatchlistManager>,
}

impl BackendCore {
    /// Construct a fresh backend wrapping an already-spawned `DataBridge`.
    pub fn new(bridge: Arc<DataBridge>, trading: Option<TradingManager>) -> Arc<Self> {
        Arc::new(Self {
            managers: std::sync::RwLock::new(Managers::new()),
            indicators: std::sync::RwLock::new(IndicatorManager::new()),
            bridge,
            mini_ticker: DashMap::new(),
            exchange_symbols: std::sync::RwLock::new(HashMap::new()),
            trading: trading.map(std::sync::Mutex::new),
            presets: std::sync::RwLock::new(HashMap::new()),
            template_manager: std::sync::RwLock::new(TemplateManager::new()),
            watchlist_manager: std::sync::RwLock::new(WatchlistManager::default()),
        })
    }

    /// Read guard for the indicator manager.
    #[inline]
    pub fn indicators(&self) -> std::sync::RwLockReadGuard<'_, IndicatorManager> {
        self.indicators.read().expect("BackendCore.indicators poisoned (read)")
    }

    /// Write guard for the indicator manager.
    #[inline]
    pub fn indicators_mut(&self) -> std::sync::RwLockWriteGuard<'_, IndicatorManager> {
        self.indicators.write().expect("BackendCore.indicators poisoned (write)")
    }

    /// Read guard for the bubble store + all managers. Hold for the duration
    /// of one logical read operation. The guard drops automatically.
    #[inline]
    pub fn bubble(&self) -> std::sync::RwLockReadGuard<'_, Managers> {
        self.managers.read().expect("BackendCore.managers poisoned (read)")
    }

    /// Write guard for the bubble store + all managers. Take one per writer
    /// arm in tick.rs; release at end of arm.
    #[inline]
    pub fn bubble_mut(&self) -> std::sync::RwLockWriteGuard<'_, Managers> {
        self.managers.write().expect("BackendCore.managers poisoned (write)")
    }

    /// Lock the trading manager for mutation. Returns `None` if trading is
    /// disabled, `Some(guard)` otherwise.
    #[inline]
    pub fn trading_lock(&self) -> Option<std::sync::MutexGuard<'_, TradingManager>> {
        self.trading.as_ref().map(|m| m.lock().expect("BackendCore.trading poisoned"))
    }

    /// Short alias for the most common read pattern on exchange symbols.
    #[inline]
    pub fn exchange_symbols_read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<ExchangeId, Vec<SymbolInfo>>> {
        self.exchange_symbols.read().expect("BackendCore.exchange_symbols poisoned (read)")
    }

    #[inline]
    pub fn exchange_symbols_mut(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<ExchangeId, Vec<SymbolInfo>>> {
        self.exchange_symbols.write().expect("BackendCore.exchange_symbols poisoned (write)")
    }

    /// Read guard for the presets map.
    #[inline]
    pub fn presets(&self) -> std::sync::RwLockReadGuard<'_, HashMap<String, ChartPreset>> {
        self.presets.read().expect("BackendCore.presets poisoned (read)")
    }

    /// Write guard for the presets map.
    #[inline]
    pub fn presets_mut(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<String, ChartPreset>> {
        self.presets.write().expect("BackendCore.presets poisoned (write)")
    }

    /// Read guard for the template manager.
    #[inline]
    pub fn template_manager(&self) -> std::sync::RwLockReadGuard<'_, TemplateManager> {
        self.template_manager.read().expect("BackendCore.template_manager poisoned (read)")
    }

    /// Write guard for the template manager.
    #[inline]
    pub fn template_manager_mut(&self) -> std::sync::RwLockWriteGuard<'_, TemplateManager> {
        self.template_manager.write().expect("BackendCore.template_manager poisoned (write)")
    }

    /// Read guard for the watchlist manager.
    #[inline]
    pub fn watchlist(&self) -> std::sync::RwLockReadGuard<'_, WatchlistManager> {
        self.watchlist_manager.read().expect("BackendCore.watchlist_manager poisoned (read)")
    }

    /// Write guard for the watchlist manager.
    #[inline]
    pub fn watchlist_mut(&self) -> std::sync::RwLockWriteGuard<'_, WatchlistManager> {
        self.watchlist_manager.write().expect("BackendCore.watchlist_manager poisoned (write)")
    }
}
