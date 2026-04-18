use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use trade_store::{Trade, TradeStoreHandle};
use crate::{TradeSeries, TradeKey};

/// Shared trade series registry — cloneable handle for async bridge tasks.
pub type SharedTradeMap = Arc<RwLock<HashMap<TradeKey, Arc<RwLock<TradeSeries>>>>>;

/// Event emitted by `TradeService` so callers can react without polling.
#[derive(Debug, Clone)]
pub enum TradeServiceEvent {
    /// A new trade was pushed into the ring.
    NewTrade { key: TradeKey, trade: Trade },
    /// A REST batch was merged into a series.
    BatchMerged { key: TradeKey, count: usize },
    /// Old trades were rotated to disk (ring buffer full).
    Rotated { key: TradeKey, rotated_count: usize },
}

/// Central trade data store. Singleton owned by `App`.
///
/// # Ownership model
/// - `App` owns `TradeService` exclusively (accessed only from the main render thread).
/// - Panels hold `Arc<RwLock<TradeSeries>>` handles — read-only at render time.
/// - `DataBridge` holds a `SharedTradeMap` clone for writing incoming trades from
///   the async WS tasks.
///
/// # Thread safety
/// `TradeService` itself is `!Sync` — it is accessed only from the main thread.
/// The `Arc<RwLock<TradeSeries>>` handles ARE `Sync` and can be cloned into
/// the GPU render thread for read access.
pub struct TradeService {
    /// All known series, keyed by `TradeKey`.
    series: SharedTradeMap,

    /// Disk persistence handle.
    trade_store: TradeStoreHandle,

    /// Global capacity for new series (overridable per-series in the future).
    default_capacity: usize,
}

impl TradeService {
    pub fn new(trade_store: TradeStoreHandle, default_capacity: usize) -> Self {
        Self {
            series: Arc::new(RwLock::new(HashMap::new())),
            trade_store,
            default_capacity,
        }
    }

    /// Create a `TradeService` that shares an existing `SharedTradeMap`.
    ///
    /// Use this when `DataBridge` already holds a clone of the same map — both
    /// sides will read and write the same underlying `HashMap` via `Arc`.
    pub fn with_map(series: SharedTradeMap, trade_store: TradeStoreHandle, default_capacity: usize) -> Self {
        Self {
            series,
            trade_store,
            default_capacity,
        }
    }

    /// Get a clone of the shared trade map for use in async bridge tasks.
    pub fn shared_series(&self) -> SharedTradeMap {
        self.series.clone()
    }

    /// Expose the underlying trades directory.
    pub fn trades_dir(&self) -> &std::path::Path {
        &self.trade_store.trades_dir
    }

    /// Acquire a series handle for the given key.
    ///
    /// Increments the refcount. Creates the series if it does not exist yet.
    /// Returns the `Arc<RwLock<TradeSeries>>` for the panel to hold.
    pub fn acquire(&mut self, key: TradeKey) -> Arc<RwLock<TradeSeries>> {
        let default_capacity = self.default_capacity;
        let mut map = self.series.write().unwrap_or_else(|e| e.into_inner());
        let handle = map.entry(key)
            .or_insert_with(|| Arc::new(RwLock::new(TradeSeries::new(default_capacity))))
            .clone();
        if let Ok(mut series) = handle.write() {
            series.refcount += 1;
        }
        handle
    }

    /// Release a panel's interest in a trade series.
    ///
    /// Decrements refcount. Returns `true` if the refcount hit zero (caller
    /// should then call `bridge.unsubscribe_trades()` and schedule a flush).
    pub fn release(&mut self, key: &TradeKey) -> bool {
        let mut map = self.series.write().unwrap_or_else(|e| e.into_inner());
        if let Some(handle) = map.get(key) {
            let refcount = {
                let mut series = handle.write().unwrap_or_else(|e| e.into_inner());
                series.refcount = series.refcount.saturating_sub(1);
                series.refcount
            };
            if refcount == 0 {
                map.remove(key);
                return true;
            }
        }
        false
    }

    /// Returns `None` if the series does not exist yet.
    pub fn get(&self, key: &TradeKey) -> Option<Arc<RwLock<TradeSeries>>> {
        self.series.read().unwrap_or_else(|e| e.into_inner()).get(key).cloned()
    }

    /// Push a single live trade into the ring.
    ///
    /// Returns `Some(TradeServiceEvent)` when the series exists, `None` otherwise.
    /// Increments `version` on every successful push.
    pub fn apply_trade(
        &mut self,
        key: &TradeKey,
        trade: Trade,
    ) -> Option<TradeServiceEvent> {
        let handle = self.series.read().unwrap_or_else(|e| e.into_inner()).get(key).cloned()?;
        let mut series = handle.write().ok()?;

        series.trades.push_back(trade);
        series.version += 1;
        series.dirty = true;
        if trade.timestamp_ms > series.last_ts_ms {
            series.last_ts_ms = trade.timestamp_ms;
        }

        // Enforce ring buffer capacity.
        Self::maybe_rotate_series(&mut series, key, &self.trade_store);

        Some(TradeServiceEvent::NewTrade { key: key.clone(), trade })
    }

    /// Merge a batch of trades into the series (future REST history path).
    ///
    /// Deduplicates by `trade_id` (not timestamp — multiple trades can share a ms).
    /// Incoming trades win on conflict. Series is created if it does not exist.
    pub fn merge_rest_batch(
        &mut self,
        key: &TradeKey,
        trades: Vec<Trade>,
    ) -> TradeServiceEvent {
        let default_capacity = self.default_capacity;
        let handle = {
            let mut map = self.series.write().unwrap_or_else(|e| e.into_inner());
            map.entry(key.clone())
                .or_insert_with(|| Arc::new(RwLock::new(TradeSeries::new(default_capacity))))
                .clone()
        };

        let count = trades.len();
        let mut series = handle.write().unwrap_or_else(|e| e.into_inner());

        // Build an id→Trade map from existing ring; incoming wins on conflict.
        use std::collections::BTreeMap;
        let mut map: BTreeMap<u64, Trade> = series.trades
            .iter()
            .map(|t| (t.trade_id, *t))
            .collect();
        for t in trades {
            map.insert(t.trade_id, t);
        }
        series.trades = map.into_values().collect();
        series.version += 1;
        series.dirty = true;

        let store = self.trade_store.clone();
        Self::maybe_rotate_series(&mut series, key, &store);

        TradeServiceEvent::BatchMerged { key: key.clone(), count }
    }

    /// Seed a series from disk-loaded trades (startup path).
    ///
    /// Does NOT increment version — seeded data is not "new" relative to
    /// a panel that hasn't rendered yet.
    pub fn seed_from_disk(&mut self, key: TradeKey, trades: Vec<Trade>) {
        let default_capacity = self.default_capacity;
        let handle = {
            let mut map = self.series.write().unwrap_or_else(|e| e.into_inner());
            map.entry(key)
                .or_insert_with(|| Arc::new(RwLock::new(TradeSeries::new(default_capacity))))
                .clone()
        };
        let mut series = handle.write().unwrap_or_else(|e| e.into_inner());
        series.trades = trades.into_iter().collect();
        series.dirty = false; // just loaded from disk — no need to write back yet
    }

    /// Flush all dirty series to disk (shutdown / periodic).
    ///
    /// Non-blocking: sends to `TradeStoreHandle`'s internal channel.
    pub fn flush_dirty(&mut self) {
        let map = self.series.read().unwrap_or_else(|e| e.into_inner());
        for (key, handle) in map.iter() {
            if let Ok(mut series) = handle.write() {
                if series.dirty {
                    let trades_vec: Arc<Vec<Trade>> = Arc::new(series.to_vec());
                    self.trade_store.write_async(
                        key.exchange_str(),
                        &key.symbol,
                        key.account_type_label(),
                        trades_vec,
                    );
                    series.dirty = false;
                }
            }
        }
    }

    /// Synchronous flush — call only from the shutdown path.
    pub fn flush_sync(&self) {
        self.trade_store.flush_sync();
    }

    /// Returns true if any series has been mutated since the last flush.
    pub fn has_any_dirty(&self) -> bool {
        let map = self.series.read().unwrap_or_else(|e| e.into_inner());
        map.values().any(|h| {
            h.read().map(|s| s.dirty).unwrap_or(false)
        })
    }

    /// Number of tracked series.
    pub fn series_count(&self) -> usize {
        self.series.read().unwrap_or_else(|e| e.into_inner()).len()
    }

    // --- Private helpers ---

    fn maybe_rotate_series(series: &mut TradeSeries, key: &TradeKey, store: &TradeStoreHandle) {
        if series.trades.len() <= series.capacity {
            return;
        }
        // Rotate out 10% of capacity at a time to amortise flush cost.
        let rotate_n = (series.capacity / 10).max(1);
        let rotated: Vec<Trade> = series.trades.drain(..rotate_n).collect();
        if let Some(first) = rotated.first() {
            series.oldest_rotated_ts_ms = Some(first.timestamp_ms);
        }
        // Write the whole (trimmed) series to disk on rotation.
        let trades_vec: Arc<Vec<Trade>> = Arc::new(series.to_vec());
        store.write_async(
            key.exchange_str(),
            &key.symbol,
            key.account_type_label(),
            trades_vec,
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use digdigdig3::{ExchangeId, AccountType};

    fn make_store() -> TradeStoreHandle {
        let dir = std::env::temp_dir().join("trade_service_tests");
        std::fs::create_dir_all(&dir).ok();
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime must start");
        TradeStoreHandle::new(dir, &rt)
    }

    fn make_key() -> TradeKey {
        TradeKey::new(ExchangeId::Binance, AccountType::Spot, "BTCUSDT")
    }

    fn make_service() -> TradeService {
        TradeService::new(make_store(), 100)
    }

    fn make_trade(ts_ms: i64, price: f64, qty: f64, id: u64) -> Trade {
        Trade {
            timestamp_ms: ts_ms,
            price,
            quantity: qty,
            trade_id: id,
            is_buyer_maker: 0,
            _pad: [0u8; 7],
        }
    }

    #[test]
    fn test_acquire_creates_series() {
        let mut svc = make_service();
        let key = make_key();
        let handle = svc.acquire(key.clone());
        let series = handle.read().unwrap();
        assert_eq!(series.refcount, 1);
        assert_eq!(series.len(), 0);
    }

    #[test]
    fn test_acquire_increments_refcount() {
        let mut svc = make_service();
        let key = make_key();
        let _h1 = svc.acquire(key.clone());
        let _h2 = svc.acquire(key.clone());
        let handle = svc.get(&key).unwrap();
        let series = handle.read().unwrap();
        assert_eq!(series.refcount, 2);
    }

    #[test]
    fn test_release_decrements_and_drops() {
        let mut svc = make_service();
        let key = make_key();
        svc.acquire(key.clone());
        let dropped = svc.release(&key);
        assert!(dropped);
        assert!(svc.get(&key).is_none());
    }

    #[test]
    fn test_apply_trade_pushes_to_ring() {
        let mut svc = make_service();
        let key = make_key();
        svc.acquire(key.clone());

        let trade = make_trade(1_700_000_000_000, 30_000.0, 1.0, 42);
        let event = svc.apply_trade(&key, trade);

        assert!(matches!(event, Some(TradeServiceEvent::NewTrade { .. })));

        let handle = svc.get(&key).unwrap();
        let series = handle.read().unwrap();
        assert_eq!(series.len(), 1);
        assert_eq!(series.last().unwrap().price, 30_000.0);
        assert_eq!(series.version, 1);
    }

    #[test]
    fn test_apply_trade_returns_none_when_no_series() {
        let mut svc = make_service();
        let key = make_key();
        let trade = make_trade(1_700_000_000_000, 30_000.0, 1.0, 1);
        let event = svc.apply_trade(&key, trade);
        assert!(event.is_none());
    }

    #[test]
    fn test_ring_buffer_capacity_enforcement() {
        let mut svc = TradeService::new(make_store(), 10);
        let key = make_key();
        svc.acquire(key.clone());

        for i in 0..15_i64 {
            let trade = make_trade(1_700_000_000_000 + i * 100, 1000.0 + i as f64, 1.0, i as u64);
            svc.apply_trade(&key, trade);
        }

        let handle = svc.get(&key).unwrap();
        let series = handle.read().unwrap();
        assert!(series.len() <= 10);
        assert!(series.oldest_rotated_ts_ms.is_some());
    }

    #[test]
    fn test_version_increments_on_every_mutation() {
        let mut svc = make_service();
        let key = make_key();
        svc.acquire(key.clone());

        svc.apply_trade(&key, make_trade(1_700_000_000_000, 1.0, 1.0, 1));
        svc.apply_trade(&key, make_trade(1_700_000_000_100, 2.0, 1.0, 2));
        svc.apply_trade(&key, make_trade(1_700_000_000_200, 3.0, 1.0, 3));

        let handle = svc.get(&key).unwrap();
        let series = handle.read().unwrap();
        assert_eq!(series.version, 3);
    }
}
