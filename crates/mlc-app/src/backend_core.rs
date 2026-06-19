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
use mlc_live_data::{DataBridge, SymbolInfo};
use mlc_trading_manager::TradingManager;

use crate::MiniTickerData;

/// Shared data plane owned by every chart-app process. Mode-1 hosts (vello,
/// future tessera-shell, wasm) own one `Arc<BackendCore>` and pass `Arc::clone`
/// to every `ChartApp` they spawn. Mode-2 daemon hosts own one `Arc<BackendCore>`
/// and serve it through the HTTP/WS API.
pub struct BackendCore {
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
}

impl BackendCore {
    /// Construct a fresh backend wrapping an already-spawned `DataBridge`.
    pub fn new(bridge: Arc<DataBridge>, trading: Option<TradingManager>) -> Arc<Self> {
        Arc::new(Self {
            bridge,
            mini_ticker: DashMap::new(),
            exchange_symbols: std::sync::RwLock::new(HashMap::new()),
            trading: trading.map(std::sync::Mutex::new),
        })
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
}
