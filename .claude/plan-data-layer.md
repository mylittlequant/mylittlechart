# Implementation Plan: Unified Data Layer (BarService)

## Architecture Decision

A singleton `BarService` — owned by `App` in `chart-app-vello/main.rs` — replaces the scattered `Vec<Bar>` ownership model. Windows hold `Arc<RwLock<BarSeries>>` handles and read directly from them, eliminating redundant copies and making preset-switch instantaneous. The bridge becomes a pure event dispatcher; all candle aggregation logic moves into `BarService`. Migration is phased so that each phase ships independently without breaking the running app.

---

## Current State (Confirmed by Survey)

**Key findings from the code:**

- `ChartWindow.bars: Vec<Bar>` at `crates/chart/src/state/chart_window.rs:115` — each window owns its own bars
- `TradeUpdate` handler at `crates/chart-app/src/lib.rs:2113` iterates `windows_mut()` and mutates `window.bars` directly — N windows = N separate aggregations of the same trade event
- `bridge.bar_cache: Arc<Mutex<HashMap<(ExchangeId, AccountType, String, String), Vec<Bar>>>>` at `crates/live-data/src/bridge.rs:164` — session cache lives in the bridge, never updated by `TradeUpdate` (only by REST fetches)
- `IndicatorManager::calculate_for_window(symbol, window_id, &w.bars)` at `crates/chart-app/src/lib.rs:1817` — indicators take `&[Bar]` slice from the window's owned vec
- `BarStoreHandle` in `crates/bar-store/` — already exists, writes `Arc<Vec<Bar>>` to disk asynchronously
- `App.bar_store: BarStoreHandle` at `crates/chart-app-vello/src/main.rs:641` — disk persistence handle is on `App`
- WS subscription is done per-window at `ConnectorReady` time, subscribing only active-preset symbols

---

## Types and Traits

### Core key type (lives in `bar-service` crate or `live-data`)

```rust
// crates/bar-service/src/types.rs

use digdigdig3::{ExchangeId, AccountType};

/// Canonical key for a bar series.
/// Matches the existing bar_cache key exactly — no migration needed.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BarSeriesKey {
    pub exchange_id: ExchangeId,
    pub account_type: AccountType,
    pub symbol: String,
    pub timeframe: String,
}

impl BarSeriesKey {
    pub fn new(
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: impl Into<String>,
        timeframe: impl Into<String>,
    ) -> Self {
        Self {
            exchange_id,
            account_type,
            symbol: symbol.into(),
            timeframe: timeframe.into(),
        }
    }

    /// Convenience: exchange name string (for disk store compatibility)
    pub fn exchange_str(&self) -> &str {
        self.exchange_id.as_str()
    }

    /// Account type short label ("S", "FC", etc.) for disk compatibility
    pub fn account_type_label(&self) -> &str {
        self.account_type.short_label()
    }
}
```

### BarSeries — the ring buffer

```rust
// crates/bar-service/src/series.rs

use std::collections::VecDeque;
use bar_store::Bar;

/// Default maximum bars held in memory per series.
pub const DEFAULT_CAPACITY: usize = 10_000;

/// A single OHLCV time series with ring-buffer memory management.
///
/// Mutated only by `BarService`. Windows hold `Arc<RwLock<BarSeries>>`
/// and call `.read()` during render — never write through the guard.
pub struct BarSeries {
    /// The ring buffer. Front = oldest, back = newest.
    pub bars: VecDeque<Bar>,

    /// Incremented on every mutation (push, update, merge, rotate).
    /// Windows track their `last_seen_version` to skip redundant recalcs.
    pub version: u64,

    /// Maximum number of bars kept in memory. When exceeded, old bars are
    /// rotated out to disk before being removed.
    pub capacity: usize,

    /// Timestamp of the trade that last updated the current (last) bar.
    /// Used for candle boundary detection in trade aggregation.
    pub last_trade_ts: i64,

    /// True when in-memory bars have been mutated since the last disk flush.
    /// Set by every mutation; cleared by `BarService` on flush.
    pub dirty: bool,

    /// Timestamp of the oldest bar that was rotated out to disk.
    /// `None` until the first rotation happens.
    pub oldest_rotated_ts: Option<i64>,

    /// Timeframe period in seconds (derived from `Timeframe.minutes * 60`).
    /// Cached here so trade aggregation does not need the `Timeframe` struct.
    pub period_secs: i64,
}

impl BarSeries {
    pub fn new(capacity: usize, period_secs: i64) -> Self {
        Self {
            bars: VecDeque::with_capacity(capacity.min(1024)),
            version: 0,
            capacity,
            last_trade_ts: 0,
            dirty: false,
            oldest_rotated_ts: None,
            period_secs,
        }
    }

    /// Number of bars currently in memory.
    pub fn len(&self) -> usize {
        self.bars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bars.is_empty()
    }

    /// Most recent bar (last in VecDeque = newest).
    pub fn last(&self) -> Option<&Bar> {
        self.bars.back()
    }

    /// Oldest in-memory bar.
    pub fn first(&self) -> Option<&Bar> {
        self.bars.front()
    }

    /// Read-only slice for indicators and rendering.
    /// Returns a contiguous slice if VecDeque has not wrapped around;
    /// callers must handle the two-slice case for wrapped deques.
    ///
    /// Use `bars.make_contiguous()` on a `&mut BarSeries` for O(n) contiguous
    /// access, but prefer the iterator for read-only paths.
    pub fn as_slices(&self) -> (&[Bar], &[Bar]) {
        self.bars.as_slices()
    }

    /// Collect to a `Vec<Bar>` — for callers that need a contiguous slice
    /// (e.g. disk flush, indicator calculation from outside BarService).
    pub fn to_vec(&self) -> Vec<Bar> {
        self.bars.iter().copied().collect()
    }

    /// Total number of bars ever seen for this series (in-memory + rotated to disk).
    pub fn total_bar_count(&self) -> usize {
        self.bars.len()
            + self.oldest_rotated_ts.map(|_| 0).unwrap_or(0) // placeholder — disk count is not tracked here
    }
}
```

### BarService — the singleton

```rust
// crates/bar-service/src/service.rs

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use bar_store::{Bar, BarStoreHandle};
use crate::{BarSeries, BarSeriesKey};

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
/// - `App` owns `BarService` exclusively.
/// - `ChartWindow` holds `Arc<RwLock<BarSeries>>` handles — read-only.
/// - `IndicatorManager::calculate_for_window` receives `&[Bar]` slices
///   collected from the `RwLock` guard.
///
/// # Thread safety
/// `BarService` itself is `!Sync` — it is accessed only from the main
/// (render) thread. The `Arc<RwLock<BarSeries>>` handles ARE `Sync` and
/// can be cloned into the GPU render thread for read access (future work).
pub struct BarService {
    /// All known series, keyed by `BarSeriesKey`.
    series: HashMap<BarSeriesKey, Arc<RwLock<BarSeries>>>,

    /// Disk persistence handle — same `BarStoreHandle` as before.
    /// `BarService` takes ownership so `App` no longer touches the store directly.
    bar_store: BarStoreHandle,

    /// Global capacity for new series (overridable per-series in the future).
    default_capacity: usize,
}

impl BarService {
    pub fn new(bar_store: BarStoreHandle, default_capacity: usize) -> Self {
        Self {
            series: HashMap::new(),
            bar_store,
            default_capacity,
        }
    }

    /// Get or create a series handle. Cheap after first call — just a
    /// HashMap lookup returning an `Arc` clone.
    pub fn get_or_create(
        &mut self,
        key: BarSeriesKey,
        period_secs: i64,
    ) -> Arc<RwLock<BarSeries>> {
        self.series
            .entry(key)
            .or_insert_with(|| {
                Arc::new(RwLock::new(BarSeries::new(self.default_capacity, period_secs)))
            })
            .clone()
    }

    /// Returns `None` if the series does not exist yet.
    pub fn get(&self, key: &BarSeriesKey) -> Option<Arc<RwLock<BarSeries>>> {
        self.series.get(key).cloned()
    }

    /// Apply a `TradeUpdate` to the matching series.
    ///
    /// Returns the event(s) generated (NewBar / BarUpdated) so the caller
    /// can drive indicator recalc and viewport snap.
    ///
    /// Called from `ChartApp::tick()` — same thread as before.
    pub fn apply_trade(
        &mut self,
        key: &BarSeriesKey,
        price: f64,
        quantity: f64,
        timestamp_ms: i64,
    ) -> Option<BarServiceEvent> {
        let handle = self.series.get(key)?;
        let mut series = handle.write().ok()?;

        let trade_ts_secs = timestamp_ms / 1000;
        let period = series.period_secs;

        let event = if let Some(last) = series.bars.back_mut() {
            let candle_end = last.timestamp + period;
            if trade_ts_secs >= candle_end {
                // New candle boundary
                let candle_start = (trade_ts_secs / period) * period;
                let new_bar = Bar {
                    timestamp: candle_start,
                    open: price,
                    high: price,
                    low: price,
                    close: price,
                    volume: quantity,
                };
                drop(last); // release the mutable borrow before push
                let new_bar_copy = new_bar;
                series.bars.push_back(new_bar);
                series.version += 1;
                series.dirty = true;
                series.last_trade_ts = trade_ts_secs;
                BarServiceEvent::NewBar { key: key.clone(), bar: new_bar_copy }
            } else {
                // Same candle
                last.close = price;
                if price > last.high { last.high = price; }
                if price < last.low  { last.low  = price; }
                last.volume += quantity;
                series.version += 1;
                series.dirty = true;
                series.last_trade_ts = trade_ts_secs;
                BarServiceEvent::BarUpdated { key: key.clone() }
            }
        } else {
            // Empty series — create first bar from trade
            let candle_start = (trade_ts_secs / period) * period;
            let bar = Bar {
                timestamp: candle_start,
                open: price,
                high: price,
                low: price,
                close: price,
                volume: quantity,
            };
            let bar_copy = bar;
            series.bars.push_back(bar);
            series.version += 1;
            series.dirty = true;
            series.last_trade_ts = trade_ts_secs;
            BarServiceEvent::NewBar { key: key.clone(), bar: bar_copy }
        };

        // Enforce ring buffer capacity.
        self.maybe_rotate(&mut series, key);

        Some(event)
    }

    /// Merge a REST-loaded batch into the series.
    ///
    /// Uses the same sorted-merge logic as the current `merge_bars()` function
    /// in `bridge.rs`. `source_bars` wins on timestamp conflicts (REST data
    /// is authoritative over trade-aggregated bars for historical data).
    pub fn merge_rest_batch(
        &mut self,
        key: &BarSeriesKey,
        source_bars: Vec<Bar>,
        period_secs: i64,
    ) -> BarServiceEvent {
        let handle = self
            .series
            .entry(key.clone())
            .or_insert_with(|| {
                Arc::new(RwLock::new(BarSeries::new(self.default_capacity, period_secs)))
            })
            .clone();

        let mut series = handle.write().unwrap_or_else(|e| e.into_inner());
        let count = source_bars.len();

        // Merge: collect existing + new, deduplicate by timestamp (new wins),
        // sort ascending, retain capacity.
        let existing: Vec<Bar> = series.bars.iter().copied().collect();
        let merged = merge_bars_sorted(existing, source_bars);
        series.bars = VecDeque::from(merged);
        series.version += 1;
        series.dirty = true;

        self.maybe_rotate(&mut series, key);

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

    /// Flush all dirty series to disk immediately (shutdown / periodic).
    ///
    /// Non-blocking: sends to `BarStoreHandle`'s internal channel.
    pub fn flush_dirty(&mut self) {
        for (key, handle) in &self.series {
            if let Ok(mut series) = handle.write() {
                if series.dirty {
                    let bars_vec: Arc<Vec<Bar>> = Arc::new(series.to_vec());
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

    /// Synchronous flush — call only from shutdown path.
    pub fn flush_sync(&self) {
        self.bar_store.flush_sync();
    }

    /// Snapshot all series for disk persistence (replaces `bridge.dump_cache_snapshot()`).
    /// Returns `(exchange, symbol, timeframe, account_type, bars)` tuples.
    pub fn dump_snapshot(&self) -> Vec<(String, String, String, String, Vec<Bar>)> {
        self.series
            .iter()
            .map(|(key, handle)| {
                let bars = handle.read()
                    .map(|s| s.to_vec())
                    .unwrap_or_default();
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

    /// Detect multi-bar gap: returns true when the trade timestamp skipped
    /// more than one candle past the last known bar.
    pub fn has_gap(&self, key: &BarSeriesKey, trade_ts_ms: i64) -> bool {
        let handle = match self.series.get(key) {
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

    fn maybe_rotate(&self, series: &mut BarSeries, key: &BarSeriesKey) {
        if series.bars.len() <= series.capacity {
            return;
        }
        // Rotate out 10% of capacity at a time to amortize flush cost.
        let rotate_n = series.capacity / 10;
        let rotated: Vec<Bar> = series.bars.drain(..rotate_n).collect();
        if let Some(first) = rotated.first() {
            series.oldest_rotated_ts = Some(first.timestamp);
        }
        // Write rotated bars to disk — they are gone from memory.
        let path_bars: Arc<Vec<Bar>> = Arc::new(rotated);
        // Note: rotation flushes to a separate "archive" path in Phase 3.
        // For Phase 1/2, the whole series is written on flush — rotation is just trimming.
        let _ = path_bars; // placeholder: Phase 3 will wire the archive write here
    }
}

/// Sorted merge of two `Vec<Bar>` slices, deduplicating by timestamp.
/// When both slices contain the same timestamp, `new_bars` wins.
fn merge_bars_sorted(existing: Vec<Bar>, new_bars: Vec<Bar>) -> Vec<Bar> {
    let mut map: std::collections::BTreeMap<i64, Bar> = existing
        .into_iter()
        .map(|b| (b.timestamp, b))
        .collect();
    for bar in new_bars {
        map.insert(bar.timestamp, bar); // new_bars wins on conflict
    }
    map.into_values().collect()
}
```

### Version-tracking handle for windows

```rust
// crates/bar-service/src/tracked_handle.rs

use std::sync::{Arc, RwLock};
use crate::BarSeries;

/// Window-side handle: holds the shared series and the last version this
/// window rendered against. Used for cheap change detection.
pub struct TrackedSeriesHandle {
    pub series: Arc<RwLock<BarSeries>>,
    /// Version number at the last render that consumed this series.
    pub last_seen_version: u64,
}

impl TrackedSeriesHandle {
    pub fn new(series: Arc<RwLock<BarSeries>>) -> Self {
        Self { series, last_seen_version: 0 }
    }

    /// Returns true if the series was mutated since `last_seen_version`.
    pub fn is_stale(&self) -> bool {
        self.series
            .read()
            .map(|s| s.version > self.last_seen_version)
            .unwrap_or(false)
    }

    /// Mark as consumed — call after rendering/recalculating.
    pub fn mark_seen(&mut self) {
        if let Ok(s) = self.series.read() {
            self.last_seen_version = s.version;
        }
    }
}
```

### lib.rs for the new crate

```rust
// crates/bar-service/src/lib.rs
mod series;
mod service;
mod tracked_handle;
mod types;

pub use series::{BarSeries, DEFAULT_CAPACITY};
pub use service::{BarService, BarServiceEvent};
pub use tracked_handle::TrackedSeriesHandle;
pub use types::BarSeriesKey;

// Re-export Bar so callers import from one place
pub use bar_store::Bar;
```

---

## Module Layout

```
crates/
├── bar-service/                     NEW crate
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                   public API
│       ├── types.rs                 BarSeriesKey
│       ├── series.rs                BarSeries (VecDeque ring buffer)
│       ├── service.rs               BarService (singleton logic)
│       └── tracked_handle.rs        TrackedSeriesHandle (per-window)
│
├── bar-store/                       EXISTING (unchanged in Phase 1)
│   └── src/...
│
├── live-data/
│   └── src/bridge.rs               MODIFIED — bar_cache thinned/removed in phases
│
├── chart/
│   └── src/state/chart_window.rs   MODIFIED — bars: Vec<Bar> → TrackedSeriesHandle
│
├── chart-app/
│   └── src/lib.rs                  MODIFIED — TradeUpdate delegates to BarService
│
├── chart-app-vello/
│   └── src/main.rs                 MODIFIED — App owns BarService, passes ref down
│
└── indicators/
    └── src/managers/
        └── indicator_manager.rs    MODIFIED — calculate_for_window accepts &[Bar]
                                    (already the case; wiring changes, not signature)
```

---

## Files to Create

- `c:\Users\VA PC\CODING\ML_TRADING\nemo\mylittlechart\crates\bar-service\Cargo.toml` — new crate manifest
- `c:\Users\VA PC\CODING\ML_TRADING\nemo\mylittlechart\crates\bar-service\src\lib.rs`
- `c:\Users\VA PC\CODING\ML_TRADING\nemo\mylittlechart\crates\bar-service\src\types.rs`
- `c:\Users\VA PC\CODING\ML_TRADING\nemo\mylittlechart\crates\bar-service\src\series.rs`
- `c:\Users\VA PC\CODING\ML_TRADING\nemo\mylittlechart\crates\bar-service\src\service.rs`
- `c:\Users\VA PC\CODING\ML_TRADING\nemo\mylittlechart\crates\bar-service\src\tracked_handle.rs`

---

## Files to Modify

| File | Line(s) | What changes |
|------|---------|--------------|
| `Cargo.toml` (workspace) | `members` array | Add `"crates/bar-service"` |
| `crates/chart-app-vello/src/main.rs:641` | `App` struct | Add `bar_service: BarService`; move `bar_store` ownership into it |
| `crates/chart-app-vello/src/main.rs` (App::new) | ~2167 | Seed `BarService` from disk instead of `bridge.seed_bar_cache()` |
| `crates/chart-app-vello/src/main.rs` (about_to_wait) | ~3857 | `bar_service.flush_dirty()` instead of `bridge.dump_cache_snapshot()` loop |
| `crates/chart-app/src/lib.rs:296` | `bridge` field | Keep bridge; add `bar_service: Arc<BarService>` reference (or pass via method arg) |
| `crates/chart-app/src/lib.rs:2113` | `TradeUpdate` handler | Delegate to `bar_service.apply_trade()`, drop the per-window loop |
| `crates/chart-app/src/lib.rs:1800` | `recalc_indicators_for_symbol` | Read bars from `Arc<RwLock<BarSeries>>` instead of `&window.bars` |
| `crates/chart-app/src/lib.rs:1895` | `BarsLoaded` handler | Call `bar_service.merge_rest_batch()`, then update window handle |
| `crates/chart/src/state/chart_window.rs:115` | `pub bars: Vec<Bar>` | Change to `pub bar_series: TrackedSeriesHandle` (Phase 1) |
| `crates/chart/src/state/chart_window.rs:745` | `set_bars()` / `update_bars()` | Operate on `BarService` not on self (called via ChartApp) |
| `crates/live-data/src/bridge.rs:164` | `bar_cache` field | Remove in Phase 2; thin in Phase 1 to redirect to BarService |
| `crates/live-data/src/bridge.rs` | `seed_bar_cache()` | Remove — replaced by `BarService::seed_from_disk()` |
| `crates/live-data/src/bridge.rs` | `dump_cache_snapshot()` | Remove — replaced by `BarService::dump_snapshot()` |

---

## Implementation Steps

### Phase 1: BarService singleton + shared series (no ring buffer yet)

**Goal**: Eliminate duplicated `Vec<Bar>` across windows. One series per key.

**Step 1.1 — Create the `bar-service` crate**
- Add `crates/bar-service/Cargo.toml` with deps: `bar-store` (path), `digdigdig3` (path via workspace patch).
- Add to workspace `members`.

**Step 1.2 — Wire `BarService` into `App`**
- In `chart-app-vello/src/main.rs`, `App` struct:
  ```rust
  bar_service: bar_service::BarService,
  // Remove: bar_store: bar_store::BarStoreHandle (it moves into BarService)
  ```
- In `App::new()`, replace:
  ```rust
  // OLD:
  let loaded = bar_store.load_many(&bar_key_refs);
  bridge.seed_bar_cache(loaded);
  // NEW:
  let loaded = bar_service_handle.bar_store.load_many(&bar_key_refs); // bar_store now accessed via BarService
  for (exchange, symbol, timeframe, at, bars) in loaded {
      let key = BarSeriesKey::from_parts(&exchange, &symbol, &timeframe, &at);
      let period_secs = resolve_period_secs(&timeframe);
      bar_service_handle.seed_from_disk(key, bars, period_secs);
  }
  ```

**Step 1.3 — Pass BarService reference into ChartApp**
- Add to `ChartApp`:
  ```rust
  // In ChartApp struct (chart-app/src/lib.rs):
  bar_service: Arc<parking_lot::RwLock<BarService>>,
  // (parking_lot to avoid poisoning; or std::sync::RwLock is fine too)
  ```
  Alternatively keep `ChartApp` ignorant and have `App` call `bar_service` methods before calling `chart.tick()`, passing events in. **Preferred approach**: App calls `bar_service` on the outer loop, then passes resulting `BarServiceEvent`s into `ChartApp::apply_events()`. This keeps `ChartApp` decoupled from `BarService`.

**Step 1.4 — Change `ChartWindow.bars` to `TrackedSeriesHandle`**
- At `crates/chart/src/state/chart_window.rs:115`:
  ```rust
  // Phase 1: keep Vec<Bar> as a compatibility shim OR change immediately.
  // Recommended: immediate change; add a compatibility accessor method.

  // Replace:
  pub bars: Vec<Bar>,
  // With:
  pub bar_series: bar_service::TrackedSeriesHandle,

  // Add compatibility accessor (used by render, indicators, drawing tools):
  pub fn bars_snapshot(&self) -> Vec<Bar> {
      self.bar_series.series
          .read()
          .map(|s| s.to_vec())
          .unwrap_or_default()
  }
  ```
  Note: `bars_snapshot()` allocates. In Phase 4 this is replaced by a zero-copy read guard. For Phase 1 correctness beats performance.

**Step 1.5 — Redirect `set_bars()` and `update_bars()`**
- These methods currently set `self.bars = bars`. After Phase 1 they become no-ops on the window; the caller (`ChartApp`) sets bars via `BarService` and the window's `TrackedSeriesHandle` auto-reflects the update.
- Rename them to `notify_bars_set()` and `notify_bars_updated()` — they update only viewport state (snap_to_end, calc_auto_scale, bar_count) not data. The data is already in `BarSeries`.

**Step 1.6 — Redirect TradeUpdate handler**
- `crates/chart-app/src/lib.rs:2113`: replace the `for window in windows_mut()` loop with:
  ```rust
  LiveUpdate::TradeUpdate { exchange_id, account_type, symbol, price, quantity, timestamp } => {
      let key = BarSeriesKey::new(exchange_id, account_type, &symbol, /* timeframe is implicit per-window */
      // NOTE: TradeUpdate has no timeframe — must iterate unique (key, timeframe) pairs
      // that match (exchange_id, account_type, symbol).
      // Collect unique timeframes from windows that match:
      let matching_windows: Vec<(ChartId, String, i64)> = self
          .panel_app.panel_grid.windows()
          .values()
          .filter(|w| w.symbol == symbol && w.account_type == account_type.short_label()
                   && w.exchange == exchange_id.as_str())
          .map(|w| (w.id, w.timeframe.name.clone(), (w.timeframe.minutes as i64) * 60))
          .collect();

      let mut is_new_bar = false;
      let mut needs_backfill = false;

      for (chart_id, tf_name, period_secs) in &matching_windows {
          let key = BarSeriesKey::new(exchange_id, account_type, &symbol, tf_name.as_str());
          // Check for gap BEFORE applying trade
          if bar_service.has_gap(&key, timestamp) {
              needs_backfill = true;
          }
          if let Some(event) = bar_service.apply_trade(&key, price, quantity, timestamp) {
              if matches!(event, BarServiceEvent::NewBar { .. }) {
                  is_new_bar = true;
                  // Viewport snap for this window
                  if let Some(w) = self.panel_app.panel_grid.windows_mut().get_mut(chart_id) {
                      w.bar_series.mark_seen();
                      let bar_count = w.bar_series.series.read().map(|s| s.len()).unwrap_or(0);
                      w.viewport.bar_count = bar_count;
                      if w.price_scale.scale_mode.is_follow() {
                          w.snap_to_end(zengeld_chart::DEFAULT_SNAP_MARGIN);
                      }
                  }
              }
          }
      }
      // ... rest of indicator recalc dispatch unchanged ...
  }
  ```

**Step 1.7 — Redirect `bar_cache` in bridge**
- `bridge.bar_cache` in Phase 1 is kept but its role changes: it is only updated by `merge_rest_batch()` callback from `BarService`. `request_bars()` still operates the same internally, but instead of updating the cache directly it sends a `BarsLoaded` LiveUpdate as before. The handler in `ChartApp::tick()` calls `bar_service.merge_rest_batch()`.
- `bridge.seed_bar_cache()` is removed; `bridge.dump_cache_snapshot()` is removed. Both responsibilities move to `BarService`.

**Step 1.8 — Redirect indicator recalc**
- `recalc_indicators_for_symbol()` at `crates/chart-app/src/lib.rs:1800`:
  ```rust
  fn recalc_indicators_for_symbol(&mut self, symbol: &str) {
      let matching: Vec<(u64, String)> = self.panel_app.panel_grid.windows()
          .values()
          .filter(|w| w.symbol == symbol)
          .map(|w| (w.id.0, w.timeframe.name.clone()))
          .collect();

      for (window_id, tf_name) in matching {
          let chart_id = ChartId(window_id);
          if let Some(w) = self.panel_app.panel_grid.windows().get(&chart_id) {
              // Read bars from BarSeries — zero extra copy via as_slices()
              // For Phase 1, use to_vec() for simplicity.
              let bars_vec = w.bar_series.series
                  .read()
                  .map(|s| s.to_vec())
                  .unwrap_or_default();
              self.indicator_manager.calculate_for_window(symbol, window_id, &bars_vec);
          }
      }
  }
  ```

---

### Phase 2: Always-on subscriptions

**Goal**: Subscribe all symbols from all presets at startup; remove unsubscribe-on-preset-switch.

**Step 2.1 — Collect all known symbols at startup**
- After `BarService` is seeded from disk, collect unique `(exchange_id, account_type, symbol)` triples from ALL presets (not just the active one). Presets are loaded into `AppState.presets: HashMap<String, ChartPreset>`.
- For each unique triple, call `bridge.subscribe_trades(exchange_id, symbol, account_type)`.

**Step 2.2 — Remove `unsubscribe_all()` call on preset switch**
- The call at `crates/chart-app/src/lib.rs` (wherever `LoadPreset` triggers symbol change) is removed. Instead of unsubscribing, windows simply point to a different `Arc<RwLock<BarSeries>>` handle.
- Keep `bridge.unsubscribe_trades()` only for when a symbol is removed from ALL presets (i.e. the `WsActorMap` refcount hits zero).

**Step 2.3 — Preset switch path**
- `LoadPreset` in `ChartApp` currently calls `window.set_bars(vec![])` then `bridge.request_bars()`. After Phase 2:
  - Look up `BarService.get(&key)` for the new symbol+TF combination.
  - If it exists (already subscribed): assign the existing handle to the window's `bar_series`. Mark `needs_initial_viewport_fit = true`. Done — no REST fetch needed unless data is stale.
  - If it does not exist: call `bridge.request_bars()` as before (Phase A+B fetch). The result will go into `BarService` via the `BarsLoaded` handler.
- Gate REST fetch on "freshness": if the series exists and `last_bar.timestamp > now - 2 * period_secs`, skip Phase A entirely.

**Step 2.4 — Add subscription on preset create/modify**
- When a new preset is created or a symbol changed, call `bridge.subscribe_trades()` for the new symbol. `WsActorMap` already ref-counts, so duplicate subscribes are safe.

---

### Phase 3: Ring buffer + disk rotation

**Goal**: Bound memory usage. Old bars are written to disk before removal.

**Step 3.1 — Implement rotation in `BarService::maybe_rotate()`**
- Currently a placeholder. In Phase 3:
  - When `series.bars.len() > capacity`, drain the oldest `capacity / 10` bars.
  - Write them to a separate archive file: `{bars_dir}/{exchange}/{symbol}_{at}_{tf}_archive.bin`.
  - The archive file is an append-only binary format (use `bar-store/src/format.rs` for encoding).
  - Update `series.oldest_rotated_ts` to the oldest drained bar's timestamp.

**Step 3.2 — Scroll-left from disk**
- When `ChartWindow` detects scroll-left past the first in-memory bar (i.e. `viewport.view_start < 0`), it calls a new method on `BarService`:
  ```rust
  pub fn load_historical(
      &self,
      key: &BarSeriesKey,
      before_ts: i64,
      limit: usize,
  ) -> Vec<Bar>;
  ```
  This reads from the archive file and returns bars WITHOUT inserting them into the ring buffer. The render path uses a temporary `Vec<Bar>` for the historical view. The ring buffer stays bounded.
- The existing `ScrollBarsLoaded` LiveUpdate variant from the bridge is kept for exchanges where we need to fetch older bars from the network (not disk). The disk path adds a fast local fallback before triggering the REST fetch.

**Step 3.3 — Capacity config**
- Add a `PerfAction::SetMaxBars(usize)` handler (already exists in the action enum) that calls `bar_service.set_default_capacity(n)`.
- Existing series can have their capacity updated lazily (applied on next rotation).

---

### Phase 4: Notification optimization

**Goal**: Eliminate unnecessary indicator recalculations and render work.

**Step 4.1 — Version-based indicator skip**
- `IndicatorManager::calculate_for_window()` is called speculatively today. After Phase 4, `ChartApp` checks `w.bar_series.is_stale()` before calling it. If `!is_stale()`, skip the recalculation entirely.
- After recalc, call `w.bar_series.mark_seen()`.

**Step 4.2 — Version-based render skip**
- In `chart-app-vello`, the scene cache (`toolbar_dirty`, `sidebar_dirty_scene`) pattern can be extended to the chart canvas. If `!w.bar_series.is_stale()` and no viewport change occurred, skip the chart scene rebuild.

**Step 4.3 — Zero-copy indicator access**
- Replace `to_vec()` in `recalc_indicators_for_symbol()` with a read guard + slice approach:
  ```rust
  // Requires IndicatorManager::calculate_for_window to accept both slices
  // (VecDeque as_slices returns two non-contiguous &[Bar]).
  // Option A: make_contiguous() — mutates the VecDeque, needs &mut RwLock guard.
  // Option B: collect into smallvec on the stack for short series.
  // Option C: accept a iterator — requires refactoring IndicatorCalculator.
  // Recommended for Phase 4: Option A via a write guard (rare, only when needed).
  ```
  This optimization is deferred to Phase 4 because it requires refactoring `IndicatorCalculator::calculate_with_id()` which currently takes `bars: &[Bar]`.

---

## Error Handling

```rust
// crates/bar-service/src/error.rs

#[derive(thiserror::Error, Debug)]
pub enum BarServiceError {
    #[error("series not found: {key:?}")]
    SeriesNotFound { key: String },

    #[error("RwLock poisoned for key {key:?}")]
    LockPoisoned { key: String },

    #[error("disk I/O error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },
}
```

All methods that could fail return `Result<T, BarServiceError>`. In Phase 1, the `apply_trade()` and `merge_rest_batch()` methods use `unwrap_or_else(|e| e.into_inner())` on the `RwLock` guard to tolerate poisoning gracefully (a poisoned lock is acceptable — we log and continue). Poisoning only occurs on a panic inside a write guard, which should not happen in well-reviewed code.

---

## Crate Dependencies

```toml
# crates/bar-service/Cargo.toml
[package]
name = "bar-service"
version.workspace = true
edition.workspace = true

[dependencies]
bar-store = { path = "../bar-store" }
digdigdig3 = { workspace = true }  # ExchangeId, AccountType

# Optional for Phase 4 zero-copy optimization:
# smallvec = "1"
```

Workspace `Cargo.toml` changes:
- Add `"crates/bar-service"` to `members`.

`chart-app/Cargo.toml`:
- Add `bar-service = { path = "../bar-service" }`.

`chart-app-vello/Cargo.toml`:
- Add `bar-service = { path = "../bar-service" }`.

`chart/Cargo.toml`:
- Add `bar-service = { path = "../bar-service" }` (for `TrackedSeriesHandle` in `ChartWindow`).

---

## Key Design Decisions (with Rationale)

### 1. `std::sync::RwLock` vs `parking_lot::RwLock`
Use `parking_lot::RwLock`. Reasons:
- No poisoning — if a write guard panics, subsequent reads don't have to handle `PoisonError`.
- Significantly faster under low contention (LIFO writer policy, no OS wait in the common case).
- `parking_lot` is already in the dependency tree (check: `uzor` and other crates use it).
- `std::sync::RwLock` is acceptable as a fallback if `parking_lot` is unavailable — the API is nearly identical.

### 2. Where `BarService` lives — `App` in main.rs, NOT inside `ChartApp`
`ChartApp` is one per OS window. `BarService` must be shared across OS windows (two windows showing the same BTCUSDT/1m must share one `BarSeries`). `App` is the single owner of state shared across all windows, consistent with how `AppState`, `bar_store`, and `bridge` are currently owned.

`ChartApp` receives `Arc<RwLock<BarSeries>>` handles from `App` — it never touches `BarService` directly. This matches the existing delegation pattern (windows push `WatchlistAction`, `PresetAction`, etc. upward to `App`).

### 3. TradeUpdate has no timeframe — candle aggregation is per-series
The existing code loops all windows matching `(exchange, account_type, symbol)` and aggregates per window's `timeframe.minutes`. After migration, `BarService` does the same: for each unique `(exchange, account_type, symbol, timeframe)` combination among windows that match the trade's `(exchange, account_type, symbol)`, it aggregates independently. This is correct because BTCUSDT/1m and BTCUSDT/4H have different candle boundaries.

### 4. `window.bars` compatibility shim
`ChartWindow.bars: Vec<Bar>` is referenced in dozens of places: rendering, indicator recalc, drawing timestamps, alert crossing detection, compare overlays, scroll calculation. A big-bang replacement risks breakage. The plan introduces `bars_snapshot() -> Vec<Bar>` as a transient compatibility method on `ChartWindow`, which allocates a `Vec` from the `VecDeque`. This is replaced by zero-copy access in Phase 4. The allocation is fine because `bars_snapshot()` is called at most a few times per frame (rendering + indicator recalc), not in a hot inner loop.

### 5. `bridge.bar_cache` removal is Phase 2, not Phase 1
The bridge's `request_bars()` method currently reads/writes `bar_cache` for Level 1 instant-serve. After `BarService` exists, the Level 1 serve should come from `BarService.get(&key)` instead. But changing `request_bars()` internals requires re-threading the `BarService` reference into the async closure (which crosses the `spawn` boundary). This is doable but is a larger change. Phase 1 keeps the bridge cache for Level 1, with `BarService` as the authoritative live store. Phase 2 removes the bridge cache entirely.

### 6. Agent API (`zengeld-server`) reads bars
Currently the Agent API reads from `bridge.bar_cache`. After Phase 1, it should read from `BarService`. The server has access to the bridge (passed at startup). In Phase 1: add a `bar_service: Arc<parking_lot::RwLock<BarService>>` to the server's shared state alongside the bridge reference. In Phase 2, remove the bridge cache and the agent API's dependency on it.

---

## Testing Plan

### Unit tests in `crates/bar-service/src/service.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use bar_store::Bar;

    fn make_key(tf: &str) -> BarSeriesKey { ... }
    fn make_bar(ts: i64, price: f64) -> Bar { ... }

    #[test]
    fn test_trade_creates_first_bar() { ... }

    #[test]
    fn test_trade_updates_same_candle() { ... }

    #[test]
    fn test_trade_opens_new_candle() { ... }

    #[test]
    fn test_gap_detection() { ... }

    #[test]
    fn test_merge_rest_batch_deduplicates_by_timestamp() { ... }

    #[test]
    fn test_merge_rest_batch_new_wins_on_conflict() { ... }

    #[test]
    fn test_ring_buffer_capacity_enforcement() { ... }

    #[test]
    fn test_version_increments_on_every_mutation() { ... }

    #[test]
    fn test_tracked_handle_is_stale() { ... }
}
```

### Integration test: multi-window shared series

```rust
// crates/bar-service/tests/multi_window.rs
// Verifies: two TrackedSeriesHandle instances from the same Arc share data.
// Trade applied via service → both handles see version increment.
```

### Regression test: preset switch with existing data

```rust
// Verifies: after seeding a series from disk, applying trades, then
// creating a second TrackedSeriesHandle for the same key, the new handle
// sees all bars without a REST fetch.
```

---

## Migration Checklist (Phase Gates)

| Phase | Gate condition | Verification |
|-------|----------------|--------------|
| 1 → 2 | All windows read bars from `TrackedSeriesHandle`; `TradeUpdate` does not mutate `window.bars` directly | `cargo check` + manual test: open 3 BTCUSDT windows, confirm single bar series in BarService map |
| 2 → 3 | Preset switch requires zero REST fetches for already-subscribed symbols | Manual: open preset A, switch to B, switch back to A; no `[Bridge] Phase A` log line for A's symbols |
| 3 → 4 | Ring buffer holds at most `default_capacity` bars; `oldest_rotated_ts` is set after overflow | Unit test: feed `capacity + 100` bars, assert `series.len() == capacity - rotate_n + 100` |
| 4 → done | Indicator recalc skipped when version unchanged | Perf test: `recalc_count` stops incrementing on a frozen chart |

---

## Estimated Complexity

**Phase 1**: High — touches ChartWindow, ChartApp, DataBridge, App, and requires the new crate. The blast radius is large but changes are mechanical (redirection, not new logic).

**Phase 2**: Medium — subscription management changes, preset-switch path changes, but no data structure changes.

**Phase 3**: Medium — ring buffer rotation logic is new, archive file format needs design, scroll-left from disk is new UI path.

**Phase 4**: Low-Medium — mostly optimization passes on top of stable Phase 1-3 infrastructure.

---

## What Does NOT Change

- `bar-store` crate — `BarStoreHandle` API is unchanged; `BarService` uses it internally.
- `WsActorMap` and WebSocket actor logic — unchanged.
- `LiveUpdate` enum — unchanged; all variants remain.
- `IndicatorManager::calculate_for_window(symbol, window_id, &[Bar])` signature — unchanged; only the call site changes to source the `&[Bar]` from `BarSeries` instead of `window.bars`.
- Disk file format — `BarStoreHandle.file_path()` naming convention is unchanged; `bar-service` re-uses the same paths.
- `merge_bars()` logic — moved from `bridge.rs` into `bar-service/src/service.rs:merge_bars_sorted()` as a private function. Semantics identical.
