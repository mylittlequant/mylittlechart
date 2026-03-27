use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use bar_store::BarStoreHandle;
use zengeld_chart::Bar;
use crate::{BarSeries, BarSeriesKey};

/// Convert chart Bar to bar_store Bar for disk persistence.
fn to_store_bar(b: &Bar) -> bar_store::Bar {
    bar_store::Bar {
        timestamp: b.timestamp,
        open: b.open,
        high: b.high,
        low: b.low,
        close: b.close,
        volume: b.volume,
    }
}

/// Convert bar_store Bar to chart Bar after disk load.
fn from_store_bar(b: bar_store::Bar) -> Bar {
    Bar {
        timestamp: b.timestamp,
        open: b.open,
        high: b.high,
        low: b.low,
        close: b.close,
        volume: b.volume,
    }
}

/// Shared series registry — cloneable handle for async bridge tasks.
pub type SharedSeriesMap = Arc<std::sync::RwLock<HashMap<BarSeriesKey, Arc<RwLock<BarSeries>>>>>;

/// Event emitted by `BarService` so callers can react without polling.
#[derive(Debug, Clone)]
pub enum BarServiceEvent {
    /// A new bar was pushed (candle closed or REST load finished).
    NewBar { key: BarSeriesKey, bar: Bar },
    /// The current (last) bar was updated in-place.
    BarUpdated { key: BarSeriesKey },
    /// A REST batch was merged into a series.
    BatchMerged { key: BarSeriesKey, count: usize },
    /// Bars were rotated to disk (ring buffer full).
    Rotated { key: BarSeriesKey, rotated_count: usize },
}

/// Central OHLCV data store. Singleton owned by `App`.
///
/// # Ownership model
/// - `App` owns `BarService` exclusively (accessed only from the main render thread).
/// - `ChartWindow` holds `Arc<RwLock<BarSeries>>` handles — read-only.
/// - `IndicatorManager::calculate_for_window` receives `&[Bar]` slices
///   collected from the `RwLock` guard.
///
/// # Thread safety
/// `BarService` itself is `!Sync` — it is accessed only from the main thread.
/// The `Arc<RwLock<BarSeries>>` handles ARE `Sync` and can be cloned into
/// the GPU render thread for read access.
pub struct BarService {
    /// All known series, keyed by `BarSeriesKey`.
    series: SharedSeriesMap,

    /// Disk persistence handle.
    /// `BarService` takes ownership so `App` no longer touches the store directly.
    bar_store: BarStoreHandle,

    /// Global capacity for new series (overridable per-series in the future).
    default_capacity: usize,
}

impl BarService {
    pub fn new(bar_store: BarStoreHandle, default_capacity: usize) -> Self {
        Self {
            series: Arc::new(std::sync::RwLock::new(HashMap::new())),
            bar_store,
            default_capacity,
        }
    }

    /// Create a `BarService` that shares an existing `SharedSeriesMap`.
    ///
    /// Use this when `DataBridge` already holds a clone of the same map — both
    /// sides will read and write the same underlying `HashMap` via `Arc`.
    pub fn with_map(series: SharedSeriesMap, bar_store: BarStoreHandle, default_capacity: usize) -> Self {
        Self {
            series,
            bar_store,
            default_capacity,
        }
    }

    /// Get a clone of the shared series map for use in async bridge tasks.
    pub fn shared_series(&self) -> SharedSeriesMap {
        self.series.clone()
    }

    /// Expose the underlying bars directory for cleanup tasks.
    pub fn bars_dir(&self) -> &std::path::Path {
        &self.bar_store.bars_dir
    }

    /// Delegate to the underlying store's bulk disk load.
    pub fn load_many(&self, keys: &[(&str, &str, &str, &str)]) -> Vec<(String, String, String, String, Vec<Bar>)> {
        self.bar_store.load_many(keys)
            .into_iter()
            .map(|(e, s, t, a, bars)| {
                let chart_bars: Vec<Bar> = bars.into_iter().map(from_store_bar).collect();
                (e, s, t, a, chart_bars)
            })
            .collect()
    }

    /// Get or create a series handle. Cheap after first call — just a
    /// HashMap lookup returning an `Arc` clone.
    pub fn get_or_create(
        &mut self,
        key: BarSeriesKey,
        period_secs: i64,
    ) -> Arc<RwLock<BarSeries>> {
        let default_capacity = self.default_capacity;
        let mut map = self.series.write().unwrap_or_else(|e| e.into_inner());
        map.entry(key)
            .or_insert_with(|| {
                Arc::new(RwLock::new(BarSeries::new(default_capacity, period_secs)))
            })
            .clone()
    }

    /// Returns `None` if the series does not exist yet.
    pub fn get(&self, key: &BarSeriesKey) -> Option<Arc<RwLock<BarSeries>>> {
        self.series.read().unwrap_or_else(|e| e.into_inner()).get(key).cloned()
    }

    /// Apply a trade to the matching series.
    ///
    /// Returns the event generated (`NewBar` / `BarUpdated`) so the caller
    /// can drive indicator recalc and viewport snap.
    ///
    /// Returns `None` if the series does not exist yet.
    pub fn apply_trade(
        &mut self,
        key: &BarSeriesKey,
        price: f64,
        quantity: f64,
        timestamp_ms: i64,
    ) -> Option<BarServiceEvent> {
        let handle = self.series.read().unwrap_or_else(|e| e.into_inner()).get(key).cloned()?;
        let mut series = handle.write().ok()?;

        let trade_ts_secs = timestamp_ms / 1000;
        let period = series.period_secs;

        // Determine the current candle boundary (if any) without holding a
        // mutable borrow over the push_back call below.
        enum Action {
            UpdateLast,
            PushNew { candle_start: i64 },
            PushFirst { candle_start: i64 },
        }

        let action = if let Some(last) = series.bars.back() {
            let candle_end = last.timestamp + period;
            if trade_ts_secs >= candle_end {
                let candle_start = (trade_ts_secs / period) * period;
                Action::PushNew { candle_start }
            } else {
                Action::UpdateLast
            }
        } else {
            let candle_start = (trade_ts_secs / period) * period;
            Action::PushFirst { candle_start }
        };

        let event = match action {
            Action::UpdateLast => {
                let last = series.bars.back_mut()?;
                last.close = price;
                if price > last.high {
                    last.high = price;
                }
                if price < last.low {
                    last.low = price;
                }
                last.volume += quantity;
                series.version += 1;
                series.dirty = true;
                series.last_trade_ts = trade_ts_secs;
                BarServiceEvent::BarUpdated { key: key.clone() }
            }
            Action::PushNew { candle_start } | Action::PushFirst { candle_start } => {
                let new_bar = Bar {
                    timestamp: candle_start,
                    open: price,
                    high: price,
                    low: price,
                    close: price,
                    volume: quantity,
                };
                series.bars.push_back(new_bar);
                series.version += 1;
                series.dirty = true;
                series.last_trade_ts = trade_ts_secs;
                BarServiceEvent::NewBar { key: key.clone(), bar: new_bar }
            }
        };

        // Enforce ring buffer capacity.
        Self::maybe_rotate_series(&mut series, key, &self.bar_store);

        Some(event)
    }

    /// Merge a REST-loaded batch into the series.
    ///
    /// `source_bars` wins on timestamp conflicts (REST data is authoritative
    /// over trade-aggregated bars for historical data).
    pub fn merge_rest_batch(
        &mut self,
        key: &BarSeriesKey,
        source_bars: Vec<Bar>,
        period_secs: i64,
    ) -> BarServiceEvent {
        let default_capacity = self.default_capacity;
        let handle = {
            let mut map = self.series.write().unwrap_or_else(|e| e.into_inner());
            map.entry(key.clone())
                .or_insert_with(|| {
                    Arc::new(RwLock::new(BarSeries::new(default_capacity, period_secs)))
                })
                .clone()
        };

        let mut series = handle.write().unwrap_or_else(|e| e.into_inner());
        let count = source_bars.len();

        let existing: Vec<Bar> = series.bars.iter().copied().collect();
        let merged = merge_bars_sorted(existing, source_bars);
        series.bars = VecDeque::from(merged);
        series.version += 1;
        series.dirty = true;

        let bar_store = self.bar_store.clone();
        Self::maybe_rotate_series(&mut series, key, &bar_store);

        BarServiceEvent::BatchMerged { key: key.clone(), count }
    }

    /// Seed a series from disk-loaded bars (startup path).
    ///
    /// Does NOT increment version — seeded data is not "new" relative to
    /// a window that hasn't rendered yet.
    pub fn seed_from_disk(&mut self, key: BarSeriesKey, bars: Vec<Bar>, period_secs: i64) {
        let handle = self.get_or_create(key, period_secs);
        let mut series = handle.write().unwrap_or_else(|e| e.into_inner());
        series.bars = VecDeque::from(bars);
        series.dirty = false; // just loaded from disk — no need to write back yet
    }

    /// Flush all dirty series to disk (shutdown / periodic).
    ///
    /// Non-blocking: sends to `BarStoreHandle`'s internal channel.
    pub fn flush_dirty(&mut self) {
        let map = self.series.read().unwrap_or_else(|e| e.into_inner());
        for (key, handle) in map.iter() {
            if let Ok(mut series) = handle.write() {
                if series.dirty {
                    let bars_vec: Arc<Vec<bar_store::Bar>> = Arc::new(
                        series.bars.iter().map(to_store_bar).collect()
                    );
                    self.bar_store.write_async(
                        key.exchange_str(),
                        &key.symbol,
                        &key.timeframe,
                        key.account_type_label(),
                        bars_vec,
                    );
                    series.dirty = false;
                }
            }
        }
    }

    /// Synchronous flush — call only from the shutdown path.
    pub fn flush_sync(&self) {
        self.bar_store.flush_sync();
    }

    /// Returns true if any series has been mutated since the last flush.
    /// Used by App to decide whether an event-driven disk write is needed.
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

    /// Snapshot all series for disk persistence.
    ///
    /// Returns `(exchange, symbol, timeframe, account_type, bars)` tuples.
    pub fn dump_snapshot(&self) -> Vec<(String, String, String, String, Vec<Bar>)> {
        let map = self.series.read().unwrap_or_else(|e| e.into_inner());
        map.iter()
            .map(|(key, handle)| {
                let bars = handle.read().map(|s| s.to_vec()).unwrap_or_default();
                (
                    key.exchange_str().to_string(),
                    key.symbol.clone(),
                    key.timeframe.clone(),
                    key.account_type_label().to_string(),
                    bars,
                )
            })
            .collect()
    }

    /// Returns true when the trade timestamp skipped more than one candle
    /// past the last known bar (gap detected).
    pub fn has_gap(&self, key: &BarSeriesKey, trade_ts_ms: i64) -> bool {
        let handle = match self.series.read().unwrap_or_else(|e| e.into_inner()).get(key).cloned() {
            Some(h) => h,
            None => return false,
        };
        let series = match handle.read() {
            Ok(s) => s,
            Err(_) => return false,
        };
        if let Some(last) = series.bars.back() {
            let trade_secs = trade_ts_ms / 1000;
            let candle_end = last.timestamp + series.period_secs;
            trade_secs >= candle_end + series.period_secs
        } else {
            false
        }
    }

    // --- Private helpers ---

    fn maybe_rotate_series(series: &mut BarSeries, key: &BarSeriesKey, bar_store: &BarStoreHandle) {
        if series.bars.len() <= series.capacity {
            return;
        }
        // Rotate out 10% of capacity at a time to amortise flush cost.
        let rotate_n = (series.capacity / 10).max(1);
        let rotated: Vec<Bar> = series.bars.drain(..rotate_n).collect();
        if let Some(first) = rotated.first() {
            series.oldest_rotated_ts = Some(first.timestamp);
        }
        // Write the whole (trimmed) series to disk on rotation.
        // Phase 3 will replace this with an append-only archive write.
        let bars_vec: Arc<Vec<bar_store::Bar>> = Arc::new(
            series.bars.iter().map(to_store_bar).collect()
        );
        bar_store.write_async(
            key.exchange_str(),
            &key.symbol,
            &key.timeframe,
            key.account_type_label(),
            bars_vec,
        );
    }
}

/// Sorted merge of two `Vec<Bar>` slices, deduplicating by timestamp.
/// When both contain the same timestamp, `new_bars` wins.
fn merge_bars_sorted(existing: Vec<Bar>, new_bars: Vec<Bar>) -> Vec<Bar> {
    let mut map: BTreeMap<i64, Bar> = existing
        .into_iter()
        .map(|b| (b.timestamp, b))
        .collect();
    for bar in new_bars {
        map.insert(bar.timestamp, bar);
    }
    map.into_values().collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use digdigdig3::{ExchangeId, AccountType};

    fn make_store() -> BarStoreHandle {
        // Use a temp directory for tests.
        let dir = std::env::temp_dir().join("bar_service_tests");
        std::fs::create_dir_all(&dir).ok();
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime must start");
        BarStoreHandle::new(dir, &rt)
        // Note: rt is dropped here but the spawned task keeps running until the
        // channel is closed — acceptable for short unit tests.
    }

    fn make_key(tf: &str) -> BarSeriesKey {
        BarSeriesKey::new(ExchangeId::Binance, AccountType::Spot, "BTCUSDT", tf)
    }

    fn period_for(tf: &str) -> i64 {
        match tf {
            "1m" => 60,
            "5m" => 300,
            "1h" => 3600,
            _ => 60,
        }
    }

    fn make_service() -> BarService {
        BarService::new(make_store(), 100)
    }

    #[test]
    fn test_trade_creates_first_bar() {
        let mut svc = make_service();
        let key = make_key("1m");
        // Seed an empty series so apply_trade finds it.
        svc.seed_from_disk(key.clone(), vec![], period_for("1m"));

        let ts_ms = 1_700_000_000_000_i64; // arbitrary timestamp
        let event = svc.apply_trade(&key, 30_000.0, 1.0, ts_ms);

        assert!(matches!(event, Some(BarServiceEvent::NewBar { .. })));

        let handle = svc.get(&key).unwrap();
        let series = handle.read().unwrap();
        assert_eq!(series.len(), 1);
        let bar = series.last().unwrap();
        assert_eq!(bar.open, 30_000.0);
        assert_eq!(bar.close, 30_000.0);
        assert_eq!(bar.volume, 1.0);
    }

    #[test]
    fn test_trade_updates_same_candle() {
        let mut svc = make_service();
        let key = make_key("1m");
        svc.seed_from_disk(key.clone(), vec![], period_for("1m"));

        let base_ms = 1_700_000_000_000_i64;
        svc.apply_trade(&key, 30_000.0, 1.0, base_ms);
        // Second trade in same 1-minute candle (within 60 seconds).
        let event = svc.apply_trade(&key, 31_000.0, 0.5, base_ms + 30_000);

        assert!(matches!(event, Some(BarServiceEvent::BarUpdated { .. })));

        let handle = svc.get(&key).unwrap();
        let series = handle.read().unwrap();
        assert_eq!(series.len(), 1);
        let bar = series.last().unwrap();
        assert_eq!(bar.open, 30_000.0);
        assert_eq!(bar.close, 31_000.0);
        assert_eq!(bar.high, 31_000.0);
        assert!((bar.volume - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_trade_opens_new_candle() {
        let mut svc = make_service();
        let key = make_key("1m");
        svc.seed_from_disk(key.clone(), vec![], period_for("1m"));

        let base_ms = 1_700_000_000_000_i64;
        svc.apply_trade(&key, 30_000.0, 1.0, base_ms);
        // Trade arrives more than 60 seconds later.
        let event = svc.apply_trade(&key, 31_000.0, 0.5, base_ms + 70_000);

        assert!(matches!(event, Some(BarServiceEvent::NewBar { .. })));

        let handle = svc.get(&key).unwrap();
        let series = handle.read().unwrap();
        assert_eq!(series.len(), 2);
    }

    #[test]
    fn test_gap_detection() {
        let mut svc = make_service();
        let key = make_key("1m");
        svc.seed_from_disk(key.clone(), vec![], period_for("1m"));

        let base_ms = 1_700_000_000_000_i64;
        svc.apply_trade(&key, 30_000.0, 1.0, base_ms);

        // Trade 3 minutes later — skips a full candle → gap.
        assert!(svc.has_gap(&key, base_ms + 180_000));
        // Trade within the same candle — no gap.
        assert!(!svc.has_gap(&key, base_ms + 30_000));
        // Trade in the very next candle — no gap (exactly adjacent).
        assert!(!svc.has_gap(&key, base_ms + 65_000));
    }

    #[test]
    fn test_merge_rest_batch_deduplicates_by_timestamp() {
        let mut svc = make_service();
        let key = make_key("1m");

        let bar1 = Bar { timestamp: 100, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0 };
        let bar2 = Bar { timestamp: 160, open: 2.0, high: 2.0, low: 2.0, close: 2.0, volume: 2.0 };
        svc.seed_from_disk(key.clone(), vec![bar1, bar2], period_for("1m"));

        // Merge a batch that overlaps on timestamp 160.
        let bar2_new = Bar { timestamp: 160, open: 9.0, high: 9.0, low: 9.0, close: 9.0, volume: 9.0 };
        let bar3 = Bar { timestamp: 220, open: 3.0, high: 3.0, low: 3.0, close: 3.0, volume: 3.0 };
        svc.merge_rest_batch(&key, vec![bar2_new, bar3], period_for("1m"));

        let handle = svc.get(&key).unwrap();
        let series = handle.read().unwrap();
        // Deduplication: ts=160 appears once; total = 3 bars.
        assert_eq!(series.len(), 3);
    }

    #[test]
    fn test_merge_rest_batch_new_wins_on_conflict() {
        let mut svc = make_service();
        let key = make_key("1m");

        let old = Bar { timestamp: 100, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0 };
        svc.seed_from_disk(key.clone(), vec![old], period_for("1m"));

        let new_bar = Bar { timestamp: 100, open: 99.0, high: 99.0, low: 99.0, close: 99.0, volume: 99.0 };
        svc.merge_rest_batch(&key, vec![new_bar], period_for("1m"));

        let handle = svc.get(&key).unwrap();
        let series = handle.read().unwrap();
        assert_eq!(series.len(), 1);
        assert_eq!(series.last().unwrap().open, 99.0);
    }

    #[test]
    fn test_ring_buffer_capacity_enforcement() {
        let mut svc = BarService::new(make_store(), 10);
        let key = make_key("1m");
        svc.seed_from_disk(key.clone(), vec![], period_for("1m"));

        // Feed 15 bars — 5 more than capacity.
        let base_ms = 1_700_000_000_000_i64;
        for i in 0..15_i64 {
            svc.apply_trade(&key, 1000.0 + i as f64, 1.0, base_ms + i * 70_000);
        }

        let handle = svc.get(&key).unwrap();
        let series = handle.read().unwrap();
        // After rotation (10/10 = 1 bar rotated per call, called at bar 11, 12, 13, 14, 15),
        // the ring should not exceed capacity.
        assert!(series.len() <= 10);
        assert!(series.oldest_rotated_ts.is_some());
    }

    #[test]
    fn test_version_increments_on_every_mutation() {
        let mut svc = make_service();
        let key = make_key("1m");
        svc.seed_from_disk(key.clone(), vec![], period_for("1m"));

        let base_ms = 1_700_000_000_000_i64;
        svc.apply_trade(&key, 1.0, 1.0, base_ms);
        svc.apply_trade(&key, 2.0, 1.0, base_ms + 10_000);
        svc.apply_trade(&key, 3.0, 1.0, base_ms + 70_000);

        let handle = svc.get(&key).unwrap();
        let series = handle.read().unwrap();
        assert_eq!(series.version, 3);
    }
}
