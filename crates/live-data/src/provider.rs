//! `LiveDataProvider` — implements the chart's sync `DataProvider` trait
//! using data fetched asynchronously through `DataBridge`.
//!
//! # Caching model
//!
//! `get_bars()` returns cached data immediately. On a cache miss it fires an
//! async fetch via the bridge and returns `None`. The host application
//! (`ChartApp`) is expected to drain the bridge's `LiveUpdate` channel each
//! frame and call `insert_bars()` when a `BarsLoaded` message arrives.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use digdigdig3::{AccountType, ExchangeId};
use zengeld_chart::Bar;
use zengeld_chart::data_provider::DataProvider;
use zengeld_chart::state::Timeframe;

use crate::bridge::{account_type_from_short_label, DataBridge};

/// Synchronous data provider backed by an async exchange connector.
///
/// Implements the chart's `DataProvider` trait so it can be used directly as a
/// `SharedDataProvider`. The provider caches bars in memory and fires async
/// fetches through the bridge on cache misses.
///
/// Each `LiveDataProvider` is bound to a single exchange and account type
/// combination. The `account_type` is stored on the provider so that
/// `get_bars()` can pass it to the bridge without requiring trait-level changes.
pub struct LiveDataProvider {
    /// Exchange this provider is associated with.
    exchange_id: ExchangeId,
    /// Human-readable exchange name returned by `exchange_name()`.
    exchange_name: String,
    /// Account type for this provider (e.g. Spot, FuturesCross).
    account_type: AccountType,
    /// Bridge that owns the tokio runtime and connector pool.
    bridge: Arc<DataBridge>,
    /// Cached bars: key = `(symbol, timeframe_name, account_type_label)`.
    cache: RwLock<HashMap<(String, String, String), Vec<Bar>>>,
    /// Keys for which an async fetch is already in flight.
    pending: RwLock<HashSet<(String, String, String)>>,
}

impl LiveDataProvider {
    /// Create a new provider.
    pub fn new(
        exchange_id: ExchangeId,
        exchange_name: impl Into<String>,
        account_type: AccountType,
        bridge: Arc<DataBridge>,
    ) -> Self {
        Self {
            exchange_id,
            exchange_name: exchange_name.into(),
            account_type,
            bridge,
            cache: RwLock::new(HashMap::new()),
            pending: RwLock::new(HashSet::new()),
        }
    }

    /// The exchange this provider is associated with.
    pub fn exchange_id(&self) -> ExchangeId {
        self.exchange_id
    }

    /// The account type this provider is associated with.
    pub fn account_type(&self) -> AccountType {
        self.account_type
    }

    /// Insert bars into the cache for the given symbol, timeframe, and account type label.
    ///
    /// Called by the host application when a `BarsLoaded` update arrives.
    pub fn insert_bars(&self, symbol: &str, timeframe: &str, account_type_label: &str, bars: Vec<Bar>) {
        let key = (symbol.to_string(), timeframe.to_string(), account_type_label.to_string());
        if let Ok(mut cache) = self.cache.write() {
            cache.insert(key.clone(), bars);
        }
        // Clear the pending flag so a future cache miss will re-fetch if needed
        if let Ok(mut pending) = self.pending.write() {
            pending.remove(&key);
        }
    }

    /// Return `true` if an async fetch is currently in flight for this key.
    pub fn is_pending(&self, symbol: &str, timeframe: &str, account_type_label: &str) -> bool {
        let key = (symbol.to_string(), timeframe.to_string(), account_type_label.to_string());
        self.pending
            .read()
            .map(|p| p.contains(&key))
            .unwrap_or(false)
    }
}

impl DataProvider for LiveDataProvider {
    fn get_bars(&self, symbol: &str, timeframe: &Timeframe) -> Option<Vec<Bar>> {
        let at_label = self.account_type.short_label().to_string();
        let key = (symbol.to_string(), timeframe.name.clone(), at_label.clone());

        // Fast path: cache hit
        if let Ok(cache) = self.cache.read() {
            if let Some(bars) = cache.get(&key) {
                return Some(bars.clone());
            }
        }

        // Slow path: cache miss — fire an async fetch (deduplicated by pending set)
        let already_pending = {
            let mut pending_guard = match self.pending.write() {
                Ok(g) => g,
                Err(_) => return None,
            };
            if pending_guard.contains(&key) {
                true
            } else {
                pending_guard.insert(key);
                false
            }
        };

        if !already_pending {
            self.bridge
                .request_bars(self.exchange_id, symbol, timeframe, self.account_type, None, None);
        }

        None
    }

    fn insert_bars(&self, symbol: &str, timeframe: &str, bars: Vec<Bar>) {
        // This trait method does not carry account_type — use the provider's own
        // account_type to form the cache key, matching what get_bars() uses.
        let at_label = self.account_type.short_label().to_string();
        let key = (symbol.to_string(), timeframe.to_string(), at_label);
        if let Ok(mut cache) = self.cache.write() {
            cache.insert(key.clone(), bars);
        }
        if let Ok(mut pending) = self.pending.write() {
            pending.remove(&key);
        }
    }

    fn has_symbol(&self, _symbol: &str) -> bool {
        // We cannot know without an async round-trip; optimistically return true.
        true
    }

    fn available_symbols(&self) -> Vec<String> {
        // Would require an async exchange-info call; return empty for now.
        Vec::new()
    }

    fn available_timeframes(&self, _symbol: &str) -> Vec<Timeframe> {
        vec![
            Timeframe::m1(),
            Timeframe::m5(),
            Timeframe::m15(),
            Timeframe::m30(),
            Timeframe::h1(),
            Timeframe::h4(),
            Timeframe::d1(),
            Timeframe::w1(),
        ]
    }

    fn exchange_name(&self, _symbol: &str) -> String {
        self.exchange_name.clone()
    }
}

/// Parse an account type short label from a string.
///
/// Re-exports the bridge helper so callers in chart-app can use it without
/// directly importing from bridge internals.
pub fn parse_account_type(label: &str) -> AccountType {
    account_type_from_short_label(label)
}
