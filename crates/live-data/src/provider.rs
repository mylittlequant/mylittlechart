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

use digdigdig3::ExchangeId;
use zengeld_chart::Bar;
use zengeld_chart::data_provider::DataProvider;
use zengeld_chart::state::Timeframe;

use crate::bridge::DataBridge;

/// Synchronous data provider backed by an async exchange connector.
///
/// Implements the chart's `DataProvider` trait so it can be used directly as a
/// `SharedDataProvider`. The provider caches bars in memory and fires async
/// fetches through the bridge on cache misses.
pub struct LiveDataProvider {
    /// Exchange this provider is associated with.
    exchange_id: ExchangeId,
    /// Human-readable exchange name returned by `exchange_name()`.
    exchange_name: String,
    /// Bridge that owns the tokio runtime and connector pool.
    bridge: Arc<DataBridge>,
    /// Cached bars: key = `(symbol, timeframe_name)`.
    cache: RwLock<HashMap<(String, String), Vec<Bar>>>,
    /// Keys for which an async fetch is already in flight.
    pending: RwLock<HashSet<(String, String)>>,
}

impl LiveDataProvider {
    /// Create a new provider.
    pub fn new(
        exchange_id: ExchangeId,
        exchange_name: impl Into<String>,
        bridge: Arc<DataBridge>,
    ) -> Self {
        Self {
            exchange_id,
            exchange_name: exchange_name.into(),
            bridge,
            cache: RwLock::new(HashMap::new()),
            pending: RwLock::new(HashSet::new()),
        }
    }

    /// The exchange this provider is associated with.
    pub fn exchange_id(&self) -> ExchangeId {
        self.exchange_id
    }

    /// Insert bars into the cache.
    ///
    /// Called by the host application when a `BarsLoaded` update arrives.
    pub fn insert_bars(&self, symbol: &str, timeframe: &str, bars: Vec<Bar>) {
        let key = (symbol.to_string(), timeframe.to_string());
        if let Ok(mut cache) = self.cache.write() {
            cache.insert(key.clone(), bars);
        }
        // Clear the pending flag so a future cache miss will re-fetch if needed
        if let Ok(mut pending) = self.pending.write() {
            pending.remove(&key);
        }
    }

    /// Return `true` if an async fetch is currently in flight for this key.
    pub fn is_pending(&self, symbol: &str, timeframe: &str) -> bool {
        let key = (symbol.to_string(), timeframe.to_string());
        self.pending
            .read()
            .map(|p| p.contains(&key))
            .unwrap_or(false)
    }
}

impl DataProvider for LiveDataProvider {
    fn get_bars(&self, symbol: &str, timeframe: &Timeframe) -> Option<Vec<Bar>> {
        let key = (symbol.to_string(), timeframe.name.clone());

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
                .request_bars(self.exchange_id, symbol, timeframe, None, None);
        }

        None
    }

    fn insert_bars(&self, symbol: &str, timeframe: &str, bars: Vec<Bar>) {
        let key = (symbol.to_string(), timeframe.to_string());
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
