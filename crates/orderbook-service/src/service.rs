use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use orderbook_store::{OrderbookStoreHandle, TimedSnapshot};
use crate::{OrderbookSeries, OrderbookKey};
use ordered_float::OrderedFloat;

/// Shared orderbook series registry — cloneable handle for async bridge tasks.
pub type SharedOrderbookMap = Arc<RwLock<HashMap<OrderbookKey, Arc<RwLock<OrderbookSeries>>>>>;

/// Central orderbook data store. Singleton owned by `App`.
///
/// # Ownership model
/// - `App` owns `OrderbookService` exclusively (accessed only from the main render thread).
/// - Panels hold `Arc<RwLock<OrderbookSeries>>` handles — read-only at render time.
/// - `DataBridge` holds a `SharedOrderbookMap` clone for writing incoming data from
///   async WS/REST tasks. Mirrors the `TradeService` / `SharedTradeMap` pattern.
///
/// # Thread safety
/// `OrderbookService` itself is `!Sync` — accessed only from the main thread.
/// The `Arc<RwLock<OrderbookSeries>>` handles ARE `Sync` and can be shared across
/// async tasks and the GPU render thread.
pub struct OrderbookService {
    /// All known series, keyed by `OrderbookKey`.
    series: SharedOrderbookMap,

    /// Disk persistence handle.
    store: OrderbookStoreHandle,

    /// Global history capacity for new series (number of `TimedSnapshot` entries).
    default_history_capacity: usize,
}

impl OrderbookService {
    pub fn new(store: OrderbookStoreHandle, default_history_capacity: usize) -> Self {
        Self {
            series: Arc::new(RwLock::new(HashMap::new())),
            store,
            default_history_capacity,
        }
    }

    /// Create an `OrderbookService` that shares an existing `SharedOrderbookMap`.
    ///
    /// Use this when `DataBridge` already holds a clone of the same map — both
    /// sides will read and write the same underlying `HashMap` via `Arc`.
    pub fn with_map(
        series: SharedOrderbookMap,
        store: OrderbookStoreHandle,
        default_history_capacity: usize,
    ) -> Self {
        Self { series, store, default_history_capacity }
    }

    /// Get a clone of the shared orderbook map for use in async bridge tasks.
    pub fn shared_series(&self) -> SharedOrderbookMap {
        self.series.clone()
    }

    /// Expose the underlying orderbook directory.
    pub fn orderbook_dir(&self) -> &std::path::Path {
        &self.store.orderbook_dir
    }

    /// Acquire a series handle for the given key.
    ///
    /// Increments the refcount. Creates the series if it does not exist yet.
    /// Returns the `Arc<RwLock<OrderbookSeries>>` for the panel to hold.
    pub fn acquire(&mut self, key: OrderbookKey) -> Arc<RwLock<OrderbookSeries>> {
        let default_capacity = self.default_history_capacity;
        let mut map = self.series.write().unwrap_or_else(|e| e.into_inner());
        let handle = map.entry(key)
            .or_insert_with(|| Arc::new(RwLock::new(OrderbookSeries::new(default_capacity))))
            .clone();
        if let Ok(mut series) = handle.write() {
            series.refcount += 1;
        }
        handle
    }

    /// Release a panel's interest in an orderbook series.
    ///
    /// Decrements refcount. Returns `true` if the refcount hit zero (caller
    /// should then call `bridge.unsubscribe_depth()` and schedule a flush).
    pub fn release(&mut self, key: &OrderbookKey) -> bool {
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
    pub fn get(&self, key: &OrderbookKey) -> Option<Arc<RwLock<OrderbookSeries>>> {
        self.series.read().unwrap_or_else(|e| e.into_inner()).get(key).cloned()
    }

    // ── Apply methods (thin wrappers that lock + delegate to OrderbookSeries) ─

    /// Full REST replace — clears and rebuilds the entire orderbook.
    pub fn apply_rest_snapshot(
        &mut self,
        key: &OrderbookKey,
        bids: &[(f64, f64)],
        asks: &[(f64, f64)],
        ts_ms: i64,
    ) {
        if let Some(handle) = self.series.read().unwrap_or_else(|e| e.into_inner()).get(key).cloned() {
            if let Ok(mut series) = handle.write() {
                series.apply_rest(bids, asks, ts_ms);
            }
        }
    }

    /// WS partial replace (range-based).
    ///
    /// Computes price range of input, removes existing levels in range, inserts new.
    pub fn apply_ws_snapshot(
        &mut self,
        key: &OrderbookKey,
        bids: &[(f64, f64)],
        asks: &[(f64, f64)],
        ts_ms: i64,
    ) {
        if let Some(handle) = self.series.read().unwrap_or_else(|e| e.into_inner()).get(key).cloned() {
            if let Ok(mut series) = handle.write() {
                series.apply_ws_snapshot(bids, asks, ts_ms);
            }
        }
    }

    /// WS incremental delta. `qty == 0.0` means remove that price level.
    pub fn apply_ws_delta(
        &mut self,
        key: &OrderbookKey,
        bid_changes: &[(f64, f64)],
        ask_changes: &[(f64, f64)],
        ts_ms: i64,
    ) {
        if let Some(handle) = self.series.read().unwrap_or_else(|e| e.into_inner()).get(key).cloned() {
            if let Ok(mut series) = handle.write() {
                series.apply_ws_delta(bid_changes, ask_changes, ts_ms);
            }
        }
    }

    /// Seed a series from disk-loaded snapshots (startup path).
    ///
    /// Rebuilds `current` from the most recent snapshot (if any) and loads the
    /// full history into the ring. Does NOT increment version — seeded data is
    /// not "new" relative to a panel that hasn't rendered yet.
    pub fn seed_from_disk(&mut self, key: OrderbookKey, history: Vec<TimedSnapshot>) {
        let default_capacity = self.default_history_capacity;
        let handle = {
            let mut map = self.series.write().unwrap_or_else(|e| e.into_inner());
            map.entry(key)
                .or_insert_with(|| Arc::new(RwLock::new(OrderbookSeries::new(default_capacity))))
                .clone()
        };

        let mut series = handle.write().unwrap_or_else(|e| e.into_inner());

        // Seed history ring
        for snap in &history {
            if series.history.len() >= series.history_capacity {
                series.history.pop_front();
            }
            series.history.push_back(snap.clone());
        }

        // Rebuild current from the most recent snapshot
        if let Some(latest) = history.last() {
            series.current.bids.clear();
            series.current.asks.clear();
            for &(p, q) in &latest.bids {
                if q > 0.0 {
                    series.current.bids.insert(OrderedFloat(p), q);
                }
            }
            for &(p, q) in &latest.asks {
                if q > 0.0 {
                    series.current.asks.insert(OrderedFloat(p), q);
                }
            }
            series.current.last_rest_ts_ms = latest.timestamp_ms;
        }

        series.dirty = false;
    }

    /// Flush all dirty series to disk (shutdown / periodic).
    ///
    /// Writes the current live snapshot (not the full history ring).
    /// Non-blocking: sends to `OrderbookStoreHandle`'s internal channel.
    pub fn flush_dirty(&mut self) {
        let map = self.series.read().unwrap_or_else(|e| e.into_inner());
        for (key, handle) in map.iter() {
            if let Ok(mut series) = handle.write() {
                if series.dirty {
                    let snap = series.current_to_timed_snapshot();
                    let snaps: Arc<Vec<TimedSnapshot>> = Arc::new(vec![snap]);
                    self.store.write_async(
                        key.exchange_str(),
                        &key.symbol,
                        key.account_type_label(),
                        snaps,
                    );
                    series.dirty = false;
                }
            }
        }
    }

    /// Synchronous flush — call only from the shutdown path.
    pub fn flush_sync(&self) {
        self.store.flush_sync();
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
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use digdigdig3::{ExchangeId, AccountType};

    fn make_store() -> OrderbookStoreHandle {
        let dir = std::env::temp_dir().join("ob_service_tests");
        std::fs::create_dir_all(&dir).ok();
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime must start");
        OrderbookStoreHandle::new(dir, &rt)
    }

    fn make_key() -> OrderbookKey {
        OrderbookKey::new(ExchangeId::Binance, AccountType::Spot, "BTCUSDT")
    }

    fn make_service() -> OrderbookService {
        OrderbookService::new(make_store(), 100)
    }

    #[test]
    fn test_acquire_creates_series() {
        let mut svc = make_service();
        let key = make_key();
        let handle = svc.acquire(key.clone());
        let series = handle.read().unwrap();
        assert_eq!(series.refcount, 1);
        assert!(series.current.bids.is_empty());
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
    fn test_release_drops_series() {
        let mut svc = make_service();
        let key = make_key();
        svc.acquire(key.clone());
        let dropped = svc.release(&key);
        assert!(dropped);
        assert!(svc.get(&key).is_none());
    }

    #[test]
    fn test_apply_rest_snapshot() {
        let mut svc = make_service();
        let key = make_key();
        svc.acquire(key.clone());

        let bids = vec![(30000.0, 1.0), (29999.0, 2.0)];
        let asks = vec![(30001.0, 1.0), (30002.0, 2.0)];
        svc.apply_rest_snapshot(&key, &bids, &asks, 1_000);

        let handle = svc.get(&key).unwrap();
        let series = handle.read().unwrap();
        assert_eq!(series.current.best_bid(), Some(30000.0));
        assert_eq!(series.current.best_ask(), Some(30001.0));
        assert_eq!(series.current.version, 1);
        assert_eq!(series.history.len(), 1);
        assert!(series.dirty);
    }

    #[test]
    fn test_apply_ws_delta_remove() {
        let mut svc = make_service();
        let key = make_key();
        svc.acquire(key.clone());

        let bids = vec![(30000.0, 1.0)];
        svc.apply_rest_snapshot(&key, &bids, &[], 1_000);

        // Remove the bid via delta (qty == 0)
        svc.apply_ws_delta(&key, &[(30000.0, 0.0)], &[], 2_000);

        let handle = svc.get(&key).unwrap();
        let series = handle.read().unwrap();
        assert!(series.current.bids.is_empty());
        assert_eq!(series.history.len(), 2);
    }

    #[test]
    fn test_version_increments() {
        let mut svc = make_service();
        let key = make_key();
        svc.acquire(key.clone());

        svc.apply_rest_snapshot(&key, &[(1.0, 1.0)], &[], 1_000);
        svc.apply_ws_delta(&key, &[(2.0, 1.0)], &[], 2_000);
        svc.apply_ws_snapshot(&key, &[(3.0, 1.0)], &[], 3_000);

        let handle = svc.get(&key).unwrap();
        let series = handle.read().unwrap();
        assert_eq!(series.current.version, 3);
    }

    #[test]
    fn test_history_ring_capacity() {
        let store = make_store();
        let mut svc = OrderbookService::new(store, 3);
        let key = make_key();
        svc.acquire(key.clone());

        for i in 0..5_i64 {
            svc.apply_rest_snapshot(&key, &[(i as f64, 1.0)], &[], i * 1000);
        }

        let handle = svc.get(&key).unwrap();
        let series = handle.read().unwrap();
        assert_eq!(series.history.len(), 3);
    }
}
