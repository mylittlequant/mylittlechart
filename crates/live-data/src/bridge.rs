//! `DataBridge` — async-to-sync bridge with a dedicated tokio runtime.
//!
//! Owns a tokio runtime and a connector pool. All async exchange calls happen
//! on the runtime's threads; results are sent back via an unbounded channel
//! that the sync chart thread can drain each frame.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use tokio::sync::{broadcast, mpsc};
use tokio::runtime::Runtime;

use digdigdig3::{
    ExchangeId, AccountType, Symbol, MarketData, SymbolInfo,
};
use digdigdig3::connector_manager::{ConnectorFactory, ConnectorPool};
use zengeld_chart::Bar;
use zengeld_chart::state::Timeframe;

use crate::convert::{kline_to_bar, timeframe_to_interval};
use crate::ws_manager::{WsActorMap, WsCmd, WsKey, WsStreamType};

/// Updates sent from async tasks to the sync chart thread.
#[derive(Debug, Clone)]
pub enum LiveUpdate {
    /// Historical bars loaded from the exchange REST API.
    BarsLoaded {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        timeframe: String,
        bars: Vec<Bar>,
    },
    /// Live bar update from WebSocket (last bar in-place update or new bar).
    ///
    /// `is_closed = true` means the candle closed and a new one has started.
    /// `is_closed = false` means the current (last) candle is being updated.
    BarUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        bar: Bar,
        is_closed: bool,
    },
    /// Live trade update from WebSocket.
    TradeUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        price: f64,
        quantity: f64,
        timestamp: i64,
    },
    /// Mini ticker update from WebSocket (24h stats for watchlist).
    ///
    /// Fields other than `last_price` are `Option` so that BBO-only events
    /// (e.g. KuCoin `trade.ticker`) do not overwrite 24h stats that were
    /// previously set by a snapshot event.
    MiniTickerUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        last_price: f64,
        /// `None` when the event does not carry 24h stats (BBO-only update).
        price_change_percent: Option<f64>,
        /// `None` when the event does not carry 24h stats.
        high_price: Option<f64>,
        /// `None` when the event does not carry 24h stats.
        low_price: Option<f64>,
        /// `None` when the event does not carry 24h stats.
        volume: Option<f64>,
    },
    /// Exchange symbol list loaded from REST API.
    SymbolsLoaded {
        exchange_id: ExchangeId,
        symbols: Vec<SymbolInfo>,
    },
    /// A connector was successfully initialized.
    ConnectorReady {
        exchange_id: ExchangeId,
    },
    /// Backfill complete — initial historical bars fetched for a newly loaded symbol/TF.
    ///
    /// Distinct from `BarsLoaded` so consumers can apply post-backfill logic
    /// (e.g. unlocking scroll-fetch) without re-checking every `BarsLoaded`.
    BackfillComplete {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        timeframe: String,
        bars: Vec<Bar>,
    },
    /// Older bars loaded in response to a scroll-left (historical extension) request.
    ///
    /// `prepend_count` is how many bars should be prepended before the existing
    /// series so the consumer can keep the viewport position stable.
    ScrollBarsLoaded {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        timeframe: String,
        bars: Vec<Bar>,
        prepend_count: usize,
    },
    /// An error occurred during an async operation.
    Error {
        exchange_id: ExchangeId,
        message: String,
    },
    /// Connector metrics snapshot collected on-demand (e.g. when a metrics panel renders).
    ConnectorMetrics {
        exchange_id: ExchangeId,
        /// Number of active WebSocket tasks for this exchange.
        ws_active: usize,
        /// Total HTTP requests made since the connector was created.
        http_requests: u64,
        /// Total HTTP errors (non-2xx responses or transport failures).
        http_errors: u64,
        /// Round-trip latency of the most recent HTTP request, in milliseconds.
        last_latency_ms: u64,
        /// Number of rate-limit tokens consumed in the current window.
        rate_used: u32,
        /// Maximum rate-limit tokens allowed in the window (0 = unknown).
        rate_max: u32,
    },
}

/// Async-to-sync bridge that owns a tokio runtime and a connector pool.
///
/// # Usage
///
/// ```ignore
/// let (bridge, mut rx) = DataBridge::new();
/// let bridge = Arc::new(bridge);
///
/// bridge.ensure_connector(ExchangeId::Binance);
///
/// // Each frame, drain updates:
/// while let Ok(update) = rx.try_recv() { ... }
/// ```
pub struct DataBridge {
    runtime: Runtime,
    pool: ConnectorPool,
    tx: broadcast::Sender<LiveUpdate>,
    /// Dedicated mpsc sender for `ConnectorReady` events.
    ///
    /// The app-level `tick_app_state` listens on the paired receiver instead
    /// of subscribing to the broadcast channel. This prevents the broadcast
    /// buffer from filling up when the app-level consumer falls behind, which
    /// would otherwise stall all other broadcast receivers.
    connector_ready_tx: mpsc::UnboundedSender<ExchangeId>,
    /// Multiplexed WebSocket actors — one per `(ExchangeId, WsStreamType)`.
    ///
    /// All symbols for the same exchange+stream type share a single WS
    /// connection, managed by a long-running actor task.
    ws_actors: Mutex<WsActorMap>,
    /// Session-level bar cache.
    ///
    /// Key: `(exchange_id, account_type, symbol, timeframe_name)`. Stores bars from previous
    /// requests so that switching back to an already-visited exchange+symbol+TF
    /// is instant. On each new `request_bars`, the cache is sent immediately
    /// and then a background fetch retrieves only the *newer* bars (after the
    /// last cached timestamp).
    bar_cache: Arc<Mutex<HashMap<(ExchangeId, AccountType, String, String), Vec<Bar>>>>,
    /// Session-level symbol cache.
    ///
    /// Key: `ExchangeId`. Stores the full list of trading symbols so that
    /// repeated `request_symbols` calls don't hit the network.
    symbol_cache: Arc<Mutex<HashMap<ExchangeId, Vec<SymbolInfo>>>>,
    /// In-flight bar fetch keys.
    ///
    /// Prevents duplicate concurrent fetches for the same `(exchange, account_type, symbol,
    /// timeframe)` tuple. A key is inserted before the task is spawned and
    /// removed at every exit point of the task.
    active_fetches: Arc<Mutex<HashSet<(ExchangeId, AccountType, String, String)>>>,
    /// Live WS ping RTT handles, keyed by exchange.
    ///
    /// Populated when a WebSocket task creates a connector that exposes a
    /// shared RTT arc (currently OKX only). `collect_metrics` reads from
    /// these handles via `try_lock` so it never blocks.
    ws_rtt_handles: Arc<Mutex<HashMap<ExchangeId, Arc<tokio::sync::Mutex<u64>>>>>,
    /// Oldest bar timestamp fetched per `(exchange_id, account_type, symbol, timeframe)`.
    ///
    /// Tracks how far back in history the bridge has already fetched so that
    /// scroll-left requests can request the correct older window without
    /// re-fetching data that is already cached.
    oldest_fetched_ts: Arc<Mutex<HashMap<(ExchangeId, AccountType, String, String), i64>>>,
}

impl DataBridge {
    /// Create a new bridge.
    ///
    /// Returns:
    /// - `DataBridge`: the bridge itself
    /// - `broadcast::Receiver<LiveUpdate>`: receives ALL live updates (bars, symbols, errors, …)
    /// - `mpsc::UnboundedReceiver<ExchangeId>`: receives ONLY `ConnectorReady` exchange IDs
    ///
    /// The separate mpsc receiver lets the app-level consumer handle `ConnectorReady`
    /// events without subscribing to the full broadcast channel.  This prevents the
    /// broadcast buffer from backing up when the app-level consumer is slow.
    ///
    /// Additional broadcast receivers can be created with [`add_listener`]. The channel
    /// capacity is 4096 messages; if a slow receiver falls behind, old messages are
    /// dropped for that receiver only.
    pub fn new() -> (Self, broadcast::Receiver<LiveUpdate>, mpsc::UnboundedReceiver<ExchangeId>) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("live-data")
            .enable_all()
            .build()
            .expect("failed to create tokio runtime for live-data");

        let (tx, rx) = broadcast::channel(65536);
        let (connector_ready_tx, connector_ready_rx) = mpsc::unbounded_channel();
        let pool = ConnectorPool::new();

        (Self {
            runtime,
            pool,
            tx,
            connector_ready_tx,
            ws_actors: Mutex::new(WsActorMap::new()),
            bar_cache: Arc::new(Mutex::new(HashMap::new())),
            symbol_cache: Arc::new(Mutex::new(HashMap::new())),
            active_fetches: Arc::new(Mutex::new(HashSet::new())),
            ws_rtt_handles: Arc::new(Mutex::new(HashMap::new())),
            oldest_fetched_ts: Arc::new(Mutex::new(HashMap::new())),
        }, rx, connector_ready_rx)
    }

    /// Create a new update receiver that receives all future updates.
    ///
    /// Use this to attach additional windows (spawned with `new_empty`) to the
    /// same `DataBridge` without spinning up a second tokio runtime or connector
    /// pool. Each receiver gets its own independent read position — messages are
    /// not consumed from other receivers when one drains its queue.
    pub fn add_listener(&self) -> broadcast::Receiver<LiveUpdate> {
        self.tx.subscribe()
    }

    /// Get a reference to the connector pool.
    pub fn pool(&self) -> &ConnectorPool {
        &self.pool
    }

    /// Record the oldest bar timestamp seen for a given key.
    ///
    /// Only updates the stored value when `ts` is strictly older (smaller) than
    /// whatever was previously recorded. Safe to call from any thread.
    pub fn record_oldest_ts(
        &self,
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: &str,
        timeframe: &str,
        ts: i64,
    ) {
        if let Ok(mut map) = self.oldest_fetched_ts.lock() {
            let key = (exchange_id, account_type, symbol.to_string(), timeframe.to_string());
            let entry = map.entry(key).or_insert(i64::MAX);
            if ts < *entry {
                *entry = ts;
            }
        }
    }

    /// Return the oldest bar timestamp fetched for the given key, or `None` if
    /// no fetch has been recorded yet.
    pub fn get_oldest_fetched_ts(
        &self,
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: &str,
        timeframe: &str,
    ) -> Option<i64> {
        let key = (exchange_id, account_type, symbol.to_string(), timeframe.to_string());
        self.oldest_fetched_ts.lock().ok()?.get(&key).copied()
    }

    /// Ensure a connector is initialized for the given exchange.
    ///
    /// Non-blocking — spawns an async task. On completion, sends either
    /// `ConnectorReady` or `Error` through the update channel.
    pub fn ensure_connector(&self, exchange_id: ExchangeId) {
        if self.pool.contains(&exchange_id) {
            return;
        }
        let pool = self.pool.clone();
        let tx = self.tx.clone();
        let connector_ready_tx = self.connector_ready_tx.clone();
        self.runtime.spawn(async move {
            // Creating connector asynchronously.
            match ConnectorFactory::create_public(exchange_id, false).await {
                Ok(connector) => {
                    pool.insert(exchange_id, connector);
                    let _ = tx.send(LiveUpdate::ConnectorReady { exchange_id });
                    // Also notify the app-level mpsc consumer so it doesn't
                    // need to hold a broadcast subscription open.
                    let _ = connector_ready_tx.send(exchange_id);
                    // Connector ready.
                }
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("{}", e),
                    });
                    // Connector init failed (error sent via channel).
                }
            }
        });
    }

    /// Request historical bars (klines) from an exchange.
    ///
    /// Non-blocking — spawns an async task. On completion, sends either
    /// `BarsLoaded` or `Error` through the update channel.
    ///
    /// **3-level bar loading:**
    ///
    /// Level 1 — instant render: if disk cache exists, it was already sent via
    /// `seed_bar_cache` + an initial `BarsLoaded` before this call.
    ///
    /// Level 2 (Phase A) — quick fresh fetch: always fetches 300 bars from now
    /// backward, merges with any session cache, sends `BarsLoaded`. Viewport
    /// enters Follow mode.
    ///
    /// Level 3 (Phase B) — async heal: if disk cache existed before Phase A and
    /// there was a gap between the disk tail and the fresh 300 bars, paginate
    /// backward to fill 2× the gap, merge, send a second `BarsLoaded`.
    ///
    /// The `limit` and `total_bars` parameters are retained for API compatibility
    /// but are no longer used — Phase A always fetches 300 bars.
    pub fn request_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        timeframe: &Timeframe,
        account_type: AccountType,
        _limit: Option<u16>,
        _total_bars: Option<usize>,
    ) {
        let pool = self.pool.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let interval = timeframe_to_interval(timeframe);
        let tf_name = timeframe.name.clone();
        let cache = self.bar_cache.clone();
        let active_fetches = self.active_fetches.clone();

        let cache_key = (exchange_id, account_type, symbol_str.clone(), tf_name.clone());

        // ── Deduplication guard: skip if a fetch for this key is already running ──
        {
            let mut af = active_fetches.lock().unwrap_or_else(|e| e.into_inner());
            if af.contains(&cache_key) {
                eprintln!("[Bridge] fetch already in flight for {:?} sym={} tf={}, skipping", exchange_id, symbol_str, tf_name);
                return;
            }
            af.insert(cache_key.clone());
        }

        // Snapshot the disk/session cache BEFORE Phase A so we can detect a gap.
        let cached_bars: Option<Vec<Bar>> = self.bar_cache.lock().ok()
            .and_then(|c| c.get(&cache_key).cloned());

        // If cache exists, send it immediately (Level 1 — instant render while
        // the network fetch is in flight).
        if let Some(ref bars) = cached_bars {
            eprintln!("[Bridge] {:?} serving {} cached bars instantly for sym={} tf={}", exchange_id, bars.len(), symbol_str, tf_name);
            let _ = tx.send(LiveUpdate::BarsLoaded {
                exchange_id,
                account_type,
                symbol: symbol_str.clone(),
                timeframe: tf_name.clone(),
                bars: bars.clone(),
            });
        }

        // Capture disk cache boundaries for gap detection (Phase B).
        let disk_last_ts: Option<i64> = cached_bars.as_ref()
            .and_then(|bars| bars.last())
            .map(|b| b.timestamp);
        let had_disk_cache = cached_bars.is_some();

        self.runtime.spawn(async move {
            // Helper macro to remove the in-flight key on every exit path.
            macro_rules! finish_fetch {
                () => {
                    if let Ok(mut af) = active_fetches.lock() {
                        af.remove(&cache_key);
                    }
                };
            }

            // Wait for connector if ensure_connector is still initializing it.
            let connector = {
                let mut attempts = 0;
                loop {
                    if let Some(c) = pool.get(&exchange_id) {
                        break c;
                    }
                    attempts += 1;
                    if attempts > 50 {
                        let _ = tx.send(LiveUpdate::Error {
                            exchange_id,
                            message: format!("connector {:?} not initialized", exchange_id),
                        });
                        finish_fetch!();
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            };

            let sym = parse_symbol_for_exchange(exchange_id, &symbol_str);

            // ── Phase A: Quick fresh fetch ───────────────────────────────────
            // Skip the network fetch if the cache is already fresh (last bar
            // within 2× the interval of now). This avoids redundant API calls
            // when switching between presets that share the same data.
            let interval_secs = interval_to_seconds(&interval);
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let cache_is_fresh = disk_last_ts
                .map(|ts| (now_secs - ts) < interval_secs * 2)
                .unwrap_or(false);

            if cache_is_fresh {
                eprintln!("[Bridge] Phase A: {:?} sym={} interval={} cache is fresh (age={}s < {}s), skipping fetch",
                    exchange_id, symbol_str, interval,
                    now_secs - disk_last_ts.unwrap_or(0),
                    interval_secs * 2);
                finish_fetch!();
                return;
            }

            eprintln!("[Bridge] Phase A: {:?} sym={} interval={} fetching 300 fresh bars", exchange_id, symbol_str, interval);

            let phase_a_result = connector
                .get_klines(sym.clone(), &interval, Some(300), account_type, None)
                .await;

            let fresh_300: Vec<Bar> = match phase_a_result {
                Ok(klines) => {
                    let bars: Vec<Bar> = klines.iter().map(kline_to_bar).collect();
                    eprintln!("[Bridge] Phase A: got {} bars", bars.len());
                    bars
                }
                Err(e) => {
                    eprintln!("[Bridge] Phase A error: {:?} {}", exchange_id, e);
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("get_klines failed: {}", e),
                    });
                    finish_fetch!();
                    return;
                }
            };

            let fresh_first_ts: Option<i64> = fresh_300.first().map(|b| b.timestamp);
            let fresh_last_ts: Option<i64> = fresh_300.last().map(|b| b.timestamp);

            if let (Some(ft), Some(lt)) = (fresh_first_ts, fresh_last_ts) {
                eprintln!("[Bridge] Phase A fresh range: {} -> {} ({} bars)", ft, lt, fresh_300.len());
            }

            // Merge fresh_300 into any existing session cache.
            let merged_a = {
                let current_cache = cache.lock().ok()
                    .and_then(|c| c.get(&cache_key).cloned());
                match current_cache {
                    Some(old) => {
                        eprintln!("[Bridge] Phase A merging {} fresh + {} cached", fresh_300.len(), old.len());
                        merge_bars(old, fresh_300.clone())
                    }
                    None => fresh_300.clone(),
                }
            };

            // Update cache with Phase A result.
            if let Ok(mut c) = cache.lock() {
                c.insert(cache_key.clone(), merged_a.clone());
            }

            // Send Phase A BarsLoaded (Level 2 — viewport goes to Follow mode).
            let _ = tx.send(LiveUpdate::BarsLoaded {
                exchange_id,
                account_type,
                symbol: symbol_str.clone(),
                timeframe: tf_name.clone(),
                bars: merged_a.clone(),
            });

            // ── Phase B: Async heal — fill gap between disk cache and fresh bars ──
            // Only runs when a disk cache existed before this call.
            if !had_disk_cache {
                eprintln!("[Bridge] Phase B: skipped (no disk cache)");
                finish_fetch!();
                return;
            }

            let disk_last = match disk_last_ts {
                Some(ts) => ts,
                None => {
                    finish_fetch!();
                    return;
                }
            };
            let fresh_first = match fresh_first_ts {
                Some(ts) => ts,
                None => {
                    finish_fetch!();
                    return;
                }
            };

            let gap_seconds = fresh_first - disk_last;
            if gap_seconds <= 0 {
                eprintln!("[Bridge] Phase B: no gap (disk_last={} fresh_first={}), done", disk_last, fresh_first);
                finish_fetch!();
                return;
            }

            let interval_secs = interval_to_seconds(&interval);
            let gap_bars = if interval_secs > 0 { gap_seconds / interval_secs } else { gap_seconds };
            let heal_target = (gap_bars * 2).max(100) as usize;

            eprintln!(
                "[Bridge] Phase B: gap={}s = ~{} bars, heal_target={} bars (disk_last={} fresh_first={})",
                gap_seconds, gap_bars, heal_target, disk_last, fresh_first
            );

            // Paginate backward from fresh_first to fill the gap toward disk_last.
            // end_time cursor starts just before the earliest fresh bar (in ms).
            let mut end_time_cursor: Option<i64> = Some(fresh_first * 1000 - 1);
            let mut heal_bars: Vec<Bar> = Vec::with_capacity(heal_target);
            let mut pages: usize = 0;

            'heal: loop {
                let result = connector
                    .get_klines(sym.clone(), &interval, Some(500), account_type, end_time_cursor)
                    .await;

                match result {
                    Ok(klines) => {
                        if klines.is_empty() {
                            eprintln!("[Bridge] Phase B: empty page, stopping");
                            break 'heal;
                        }
                        pages += 1;
                        let batch: Vec<Bar> = klines.iter().map(kline_to_bar).collect();

                        // Move cursor to before the oldest bar in this page.
                        if let Some(oldest_ms) = klines.iter().map(|k| k.open_time).min() {
                            end_time_cursor = Some(oldest_ms - 1);
                        }

                        let oldest_bar_ts = batch.first().map(|b| b.timestamp).unwrap_or(0);
                        heal_bars = merge_bars(batch, heal_bars);

                        eprintln!("[Bridge] Phase B: page {} -> {} heal bars (oldest_ts={})", pages, heal_bars.len(), oldest_bar_ts);

                        if heal_bars.len() >= heal_target {
                            eprintln!("[Bridge] Phase B: reached heal_target={}", heal_target);
                            break 'heal;
                        }
                        // Stop when we've reached into the disk cache range.
                        if oldest_bar_ts <= disk_last {
                            eprintln!("[Bridge] Phase B: reached disk_last={}, gap covered", disk_last);
                            break 'heal;
                        }
                        if pages >= 20 {
                            eprintln!("[Bridge] Phase B: capped at 20 pages");
                            break 'heal;
                        }
                    }
                    Err(e) => {
                        eprintln!("[Bridge] Phase B error at page {}: {}", pages, e);
                        break 'heal;
                    }
                }
            }

            if heal_bars.is_empty() {
                eprintln!("[Bridge] Phase B: no heal bars fetched, done");
                finish_fetch!();
                return;
            }

            eprintln!("[Bridge] Phase B: fetched {} heal bars over {} pages, merging", heal_bars.len(), pages);

            // Merge heal bars into current cache (which already has Phase A result).
            let merged_b = {
                let current = cache.lock().ok()
                    .and_then(|c| c.get(&cache_key).cloned())
                    .unwrap_or(merged_a);
                merge_bars(heal_bars, current)
            };

            // Update cache with healed result.
            if let Ok(mut c) = cache.lock() {
                c.insert(cache_key.clone(), merged_b.clone());
            }

            eprintln!("[Bridge] Phase B done: sending {} bars (fully healed)", merged_b.len());

            // Send Phase B BarsLoaded (Level 3 — fully healed dataset).
            let _ = tx.send(LiveUpdate::BarsLoaded {
                exchange_id,
                account_type,
                symbol: symbol_str,
                timeframe: tf_name,
                bars: merged_b,
            });

            finish_fetch!();
        });
    }

    /// Request bars synchronously (blocking).
    ///
    /// Use only at startup or from non-render threads. Blocks until the
    /// async call completes.
    pub fn request_bars_blocking(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        timeframe: &Timeframe,
        account_type: AccountType,
        limit: Option<u16>,
        _total_bars: Option<usize>,
    ) -> Option<Vec<Bar>> {
        let connector = self.pool.get(&exchange_id)?;
        let sym = parse_symbol_for_exchange(exchange_id, symbol);
        let interval = timeframe_to_interval(timeframe);

        self.runtime.block_on(async move {
            let page_size: u16 = limit.unwrap_or(500).min(500);
            let result = connector.get_klines(sym, &interval, Some(page_size), account_type, None).await;

            match result {
                Ok(klines) => {
                    let bars: Vec<Bar> = klines.iter().map(kline_to_bar).collect();
                    Some(bars)
                }
                Err(_e) => {
                    // Error not propagated in blocking mode.
                    None
                }
            }
        })
    }

    /// Request the full list of trading symbols from an exchange.
    ///
    /// Non-blocking — spawns an async task. On completion, sends
    /// `SymbolsLoaded` through the update channel. Results are cached for the
    /// session — subsequent calls return the cached list immediately without a
    /// network request.
    pub fn request_symbols(&self, exchange_id: ExchangeId) {
        let pool = self.pool.clone();
        let tx = self.tx.clone();
        let cache = self.symbol_cache.clone();

        // Instant cache hit.
        if let Ok(c) = cache.lock() {
            if let Some(symbols) = c.get(&exchange_id) {
                let _ = tx.send(LiveUpdate::SymbolsLoaded {
                    exchange_id,
                    symbols: symbols.clone(),
                });
                return;
            }
        }

        self.runtime.spawn(async move {
            let connector = match pool.get(&exchange_id) {
                Some(c) => c,
                None => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("connector {:?} not initialized", exchange_id),
                    });
                    return;
                }
            };

            // Fetch Spot and Futures symbols independently; merge into one list.
            // If an exchange doesn't support a given account type it returns an
            // error which we silently ignore — we keep whatever succeeded.
            let mut all_symbols: Vec<SymbolInfo> = Vec::new();

            if let Ok(spot) = connector.get_exchange_info(AccountType::Spot).await {
                all_symbols.extend(spot);
            }

            if let Ok(futures) = connector.get_exchange_info(AccountType::FuturesCross).await {
                all_symbols.extend(futures);
            }

            if all_symbols.is_empty() {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: "get_exchange_info failed for all account types".to_string(),
                });
                return;
            }

            // Cache for session.
            if let Ok(mut c) = cache.lock() {
                c.insert(exchange_id, all_symbols.clone());
            }
            let _ = tx.send(LiveUpdate::SymbolsLoaded {
                exchange_id,
                symbols: all_symbols,
            });
        });
    }

    /// Subscribe to live trade updates via WebSocket.
    ///
    /// Routes the symbol to the shared per-exchange trade actor, which
    /// multiplexes all symbols over a single WS connection.  If no actor
    /// exists yet for this exchange it is spawned automatically.
    pub fn subscribe_trades(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        let key = WsKey { exchange_id, stream_type: WsStreamType::Trades, account_type };
        let tx = self.tx.clone();
        let rtt = self.ws_rtt_handles.clone();
        let rt = self.runtime.handle().clone();
        if let Ok(mut actors) = self.ws_actors.lock() {
            let cmd_tx = actors.get_or_spawn(key, tx, rtt, &rt);
            let _ = cmd_tx.try_send(WsCmd::AddSymbol { symbol: symbol.to_string() });
        }
    }

    /// Subscribe to a single symbol's mini ticker stream via WebSocket.
    ///
    /// Routes the symbol to the shared per-exchange ticker actor.
    pub fn subscribe_mini_ticker(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        let key = WsKey { exchange_id, stream_type: WsStreamType::Ticker, account_type };
        let tx = self.tx.clone();
        let rtt = self.ws_rtt_handles.clone();
        let rt = self.runtime.handle().clone();
        if let Ok(mut actors) = self.ws_actors.lock() {
            let cmd_tx = actors.get_or_spawn(key, tx, rtt, &rt);
            let _ = cmd_tx.try_send(WsCmd::AddSymbol { symbol: symbol.to_string() });
        }
    }

    /// Remove one consumer interest in trade stream for this symbol.
    ///
    /// Sends a `RemoveSymbol` command to the trade actor; the actor applies a
    /// 30-second grace period before actually unsubscribing from the exchange.
    pub fn unsubscribe_trades(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        let key = WsKey { exchange_id, stream_type: WsStreamType::Trades, account_type };
        if let Ok(actors) = self.ws_actors.lock() {
            actors.send_cmd(&key, WsCmd::RemoveSymbol { symbol: symbol.to_string() });
        }
    }

    /// Abort trade WebSocket subscriptions (not mini ticker).
    ///
    /// Call this when switching symbols to stop old trade streams.
    /// Mini ticker subscriptions (watchlist) are preserved.
    pub fn unsubscribe_all(&self) {
        if let Ok(mut actors) = self.ws_actors.lock() {
            let trade_keys: Vec<WsKey> = actors
                .actors
                .keys()
                .filter(|k| k.stream_type == WsStreamType::Trades)
                .cloned()
                .collect();
            for key in trade_keys {
                actors.remove(&key);
            }
        }
    }

    /// Unsubscribe a single mini-ticker symbol (when symbol removed from watchlist).
    ///
    /// Sends a `RemoveSymbol` command to the ticker actor; the actor applies a
    /// 30-second grace period before actually unsubscribing from the exchange.
    pub fn unsubscribe_mini_ticker(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        let key = WsKey { exchange_id, stream_type: WsStreamType::Ticker, account_type };
        if let Ok(actors) = self.ws_actors.lock() {
            actors.send_cmd(&key, WsCmd::RemoveSymbol { symbol: symbol.to_string() });
        }
    }

    /// Stop all WebSocket actors for a specific exchange.
    pub fn unsubscribe_exchange(&self, exchange_id: ExchangeId) {
        if let Ok(mut actors) = self.ws_actors.lock() {
            let keys: Vec<WsKey> = actors
                .actors
                .keys()
                .filter(|k| k.exchange_id == exchange_id)
                .cloned()
                .collect();
            for key in keys {
                actors.remove(&key);
                eprintln!("[Bridge] Stopped WS actor: {:?}/{:?}", key.exchange_id, key.stream_type);
            }
        }
    }

    /// Disable a connector — remove from pool and stop all WS tasks.
    pub fn disable_connector(&self, exchange_id: ExchangeId) {
        self.unsubscribe_exchange(exchange_id);
        if self.pool().remove(&exchange_id).is_some() {
            eprintln!("[Bridge] Removed connector: {:?}", exchange_id);
        }
    }

    /// Enable a connector — initialize it if not already in pool.
    pub fn enable_connector(&self, exchange_id: ExchangeId) {
        self.ensure_connector(exchange_id);
    }

    /// Count active WebSocket actors for a specific exchange.
    pub fn ws_task_count(&self, exchange_id: ExchangeId) -> usize {
        self.ws_actors
            .lock()
            .map(|a| a.active_count_for_exchange(exchange_id))
            .unwrap_or(0)
    }

    /// Count total active WebSocket actors across all exchanges.
    pub fn ws_task_count_total(&self) -> usize {
        self.ws_actors
            .lock()
            .map(|a| a.total_active_count())
            .unwrap_or(0)
    }

    /// Get summary metrics for all active connectors.
    ///
    /// Returns one entry per exchange currently in the connector pool,
    /// containing the exchange ID, its `ConnectorStats`, and the number
    /// of active WebSocket tasks.
    ///
    /// Note: depends on `ConnectorStats` and `AnyConnector::metrics()` which
    /// are added by a parallel implementation task. This method will not
    /// compile until those additions are present.
    pub fn collect_metrics(&self) -> Vec<(ExchangeId, digdigdig3::core::types::ConnectorStats, usize)> {
        let mut results = Vec::new();
        let rtt_handles_snapshot = self.ws_rtt_handles.lock().ok()
            .map(|g| g.clone());
        for eid in self.pool.ids() {
            let mut stats = if let Some(connector) = self.pool.get(&eid) {
                connector.metrics()
            } else {
                digdigdig3::core::types::ConnectorStats::default()
            };
            // Overlay WS ping RTT if a live handle exists for this exchange.
            if let Some(ref handles) = rtt_handles_snapshot {
                if let Some(rtt_handle) = handles.get(&eid) {
                    if let Ok(rtt) = rtt_handle.try_lock() {
                        stats.ws_ping_rtt_ms = *rtt;
                    }
                }
            }
            let ws_count = self.ws_task_count(eid);
            results.push((eid, stats, ws_count));
        }
        results
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Agent API accessors
    // ─────────────────────────────────────────────────────────────────────────

    /// Get cached bars for a specific (exchange, account_type, symbol, timeframe) key.
    ///
    /// Returns `None` if the key is not in the cache or the lock is poisoned.
    pub fn get_cached_bars(
        &self,
        exchange_id: &ExchangeId,
        account_type: AccountType,
        symbol: &str,
        timeframe: &str,
    ) -> Option<Vec<Bar>> {
        let cache = self.bar_cache.lock().ok()?;
        cache
            .get(&(*exchange_id, account_type, symbol.to_string(), timeframe.to_string()))
            .cloned()
    }

    /// Return all keys currently stored in the bar cache.
    pub fn cached_bar_keys(&self) -> Vec<(ExchangeId, AccountType, String, String)> {
        self.bar_cache
            .lock()
            .map(|c| c.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Snapshot the entire bar cache for disk persistence.
    ///
    /// Returns `(exchange_str, symbol, timeframe, account_type_label, bars)` tuples
    /// for all cached entries. The `account_type_label` is the short label string
    /// (e.g. `"S"` for Spot, `"F"` for FuturesCross).
    pub fn dump_cache_snapshot(&self) -> Vec<(String, String, String, String, Vec<bar_store::Bar>)> {
        let Ok(cache) = self.bar_cache.lock() else {
            return vec![];
        };
        cache
            .iter()
            .map(|((ex, account_type, sym, tf), bars)| {
                let store_bars: Vec<bar_store::Bar> = bars
                    .iter()
                    .map(|b| bar_store::Bar {
                        timestamp: b.timestamp,
                        open: b.open,
                        high: b.high,
                        low: b.low,
                        close: b.close,
                        volume: b.volume,
                    })
                    .collect();
                (
                    ex.as_str().to_string(),
                    sym.clone(),
                    tf.clone(),
                    account_type.short_label().to_string(),
                    store_bars,
                )
            })
            .collect()
    }

    /// Pre-populate the bar cache from disk-loaded bars.
    ///
    /// Called at startup before the first `request_bars()` so that switching to a
    /// previously-visited symbol is instant without a network round-trip.
    /// Entries that already exist in the cache (e.g. from a very fast initial request)
    /// are left untouched (`or_insert` semantics).
    pub fn seed_bar_cache(
        &self,
        entries: Vec<(String, String, String, String, Vec<bar_store::Bar>)>,
    ) {
        let Ok(mut cache) = self.bar_cache.lock() else {
            return;
        };
        for (exchange_str, symbol, timeframe, account_type_label, store_bars) in entries {
            if store_bars.is_empty() {
                continue;
            }
            let Some(exchange_id) = digdigdig3::ExchangeId::from_str(&exchange_str) else {
                continue;
            };
            let account_type = account_type_from_short_label(&account_type_label);
            let bars: Vec<Bar> = store_bars
                .iter()
                .map(|b| Bar {
                    timestamp: b.timestamp,
                    open: b.open,
                    high: b.high,
                    low: b.low,
                    close: b.close,
                    volume: b.volume,
                })
                .collect();
            let key = (exchange_id, account_type, symbol, timeframe);
            cache.entry(key).or_insert(bars);
        }
    }

    /// Request background backfill of historical bars for a symbol/timeframe.
    ///
    /// Non-blocking — spawns an async task that pages backward through history
    /// until `target_bars` are accumulated in the cache. On completion, sends
    /// `BackfillComplete` through the update channel.
    ///
    /// This is Layer 2 of the data loading pipeline. It is designed to run
    /// silently after `request_bars` (Layer 1) has already populated the cache
    /// with recent bars, extending history further into the past.
    ///
    /// If the cache already holds `>= target_bars`, or a backfill for this key
    /// is already in-flight, the call returns immediately without spawning a task.
    pub fn request_background_backfill(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        timeframe: &Timeframe,
        account_type: AccountType,
        target_bars: u32,
    ) {
        let pool = self.pool.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let interval = timeframe_to_interval(timeframe);
        let tf_name = timeframe.name.clone();
        let cache = self.bar_cache.clone();
        let active_fetches = self.active_fetches.clone();
        let oldest_fetched_ts = self.oldest_fetched_ts.clone();

        let cache_key = (exchange_id, account_type, symbol_str.clone(), tf_name.clone());

        // Backfill dedup key: encode the "backfill" role by prefixing the symbol.
        // This avoids colliding with the regular request_bars fetch key while
        // reusing the same HashSet type.
        let backfill_fetch_key = (
            exchange_id,
            account_type,
            format!("backfill:{}", symbol_str),
            tf_name.clone(),
        );

        // ── Deduplication guard ───────────────────────────────────────────────
        {
            let mut af = active_fetches.lock().unwrap_or_else(|e| e.into_inner());
            if af.contains(&backfill_fetch_key) {
                eprintln!(
                    "[Bridge] backfill already in flight for {:?} sym={} tf={}, skipping",
                    exchange_id, symbol_str, tf_name
                );
                return;
            }
            af.insert(backfill_fetch_key.clone());
        }

        // ── Early-exit if cache is already satisfied ─────────────────────────
        let cached_len = cache
            .lock()
            .ok()
            .and_then(|c| c.get(&cache_key).map(|b| b.len()))
            .unwrap_or(0);

        if cached_len >= target_bars as usize {
            eprintln!(
                "[Bridge] backfill {:?} sym={} tf={}: cache already has {} >= {} bars, skipping",
                exchange_id, symbol_str, tf_name, cached_len, target_bars
            );
            if let Ok(mut af) = active_fetches.lock() {
                af.remove(&backfill_fetch_key);
            }
            return;
        }

        self.runtime.spawn(async move {
            macro_rules! finish_backfill {
                () => {
                    if let Ok(mut af) = active_fetches.lock() {
                        af.remove(&backfill_fetch_key);
                    }
                };
            }

            // Wait for connector (same pattern as request_bars).
            let connector = {
                let mut attempts = 0;
                loop {
                    if let Some(c) = pool.get(&exchange_id) {
                        break c;
                    }
                    attempts += 1;
                    if attempts > 50 {
                        eprintln!(
                            "[Bridge] backfill: connector {:?} not initialized after 5s",
                            exchange_id
                        );
                        let _ = tx.send(LiveUpdate::Error {
                            exchange_id,
                            message: format!(
                                "backfill: connector {:?} not initialized",
                                exchange_id
                            ),
                        });
                        finish_backfill!();
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            };

            let sym = parse_symbol_for_exchange(exchange_id, &symbol_str);

            // Re-check cache length after connector wait — another task may have
            // already satisfied the target while we were waiting.
            let (still_need, end_time_cursor_init) = {
                let guard = cache.lock().ok();
                let cached = guard.as_ref().and_then(|c| c.get(&cache_key).cloned());
                match cached {
                    Some(ref bars) if bars.len() >= target_bars as usize => {
                        eprintln!(
                            "[Bridge] backfill {:?} sym={} tf={}: satisfied after connector wait ({} bars)",
                            exchange_id, symbol_str, tf_name, bars.len()
                        );
                        finish_backfill!();
                        return;
                    }
                    Some(ref bars) => {
                        let need = target_bars as usize - bars.len();
                        // Cursor: just before the oldest cached bar (in ms).
                        let oldest_ms = bars.first().map(|b| b.timestamp * 1000 - 1);
                        (need, oldest_ms)
                    }
                    None => (target_bars as usize, None),
                }
            };

            eprintln!(
                "[Bridge] backfill {:?} sym={} tf={}: need {} more bars, cursor={:?}",
                exchange_id, symbol_str, tf_name, still_need, end_time_cursor_init
            );

            let mut end_time_cursor = end_time_cursor_init;
            let mut accumulated: Vec<Bar> = Vec::with_capacity(still_need.min(20_000));
            let mut pages: usize = 0;

            'backfill: loop {
                let result = connector
                    .get_klines(sym.clone(), &interval, Some(500), account_type, end_time_cursor)
                    .await;

                match result {
                    Ok(klines) => {
                        if klines.is_empty() {
                            eprintln!("[Bridge] backfill: empty page at page {}, stopping", pages);
                            break 'backfill;
                        }
                        pages += 1;
                        let batch: Vec<Bar> = klines.iter().map(kline_to_bar).collect();

                        // Move cursor to before the oldest bar in this page.
                        if let Some(oldest_ms) = klines.iter().map(|k| k.open_time).min() {
                            end_time_cursor = Some(oldest_ms - 1);
                        }

                        let oldest_bar_ts = batch.first().map(|b| b.timestamp).unwrap_or(0);

                        // Track oldest fetched timestamp.
                        if oldest_bar_ts > 0 {
                            if let Ok(mut map) = oldest_fetched_ts.lock() {
                                let key = (
                                    exchange_id,
                                    account_type,
                                    symbol_str.clone(),
                                    tf_name.clone(),
                                );
                                let entry = map.entry(key).or_insert(i64::MAX);
                                if oldest_bar_ts < *entry {
                                    *entry = oldest_bar_ts;
                                }
                            }
                        }

                        accumulated = merge_bars(batch, accumulated);

                        eprintln!(
                            "[Bridge] backfill: page {} -> {} accumulated bars (oldest_ts={})",
                            pages,
                            accumulated.len(),
                            oldest_bar_ts
                        );

                        if accumulated.len() >= still_need {
                            eprintln!(
                                "[Bridge] backfill: reached target ({} >= {})",
                                accumulated.len(),
                                still_need
                            );
                            break 'backfill;
                        }
                        if pages >= 40 {
                            eprintln!("[Bridge] backfill: capped at 40 pages (~20k bars)");
                            break 'backfill;
                        }
                    }
                    Err(e) => {
                        eprintln!("[Bridge] backfill: error at page {}: {}", pages, e);
                        break 'backfill;
                    }
                }
            }

            if accumulated.is_empty() {
                eprintln!("[Bridge] backfill: no bars fetched, done");
                finish_backfill!();
                return;
            }

            eprintln!(
                "[Bridge] backfill {:?} sym={} tf={}: fetched {} bars over {} pages, merging into cache",
                exchange_id, symbol_str, tf_name, accumulated.len(), pages
            );

            // Merge accumulated into current cache.
            let merged = {
                let current = cache
                    .lock()
                    .ok()
                    .and_then(|c| c.get(&cache_key).cloned())
                    .unwrap_or_default();
                merge_bars(accumulated, current)
            };

            // Update cache.
            if let Ok(mut c) = cache.lock() {
                c.insert(cache_key.clone(), merged.clone());
            }

            eprintln!(
                "[Bridge] backfill done: sending BackfillComplete with {} bars",
                merged.len()
            );

            let _ = tx.send(LiveUpdate::BackfillComplete {
                exchange_id,
                account_type,
                symbol: symbol_str,
                timeframe: tf_name,
                bars: merged,
            });

            finish_backfill!();
        });
    }

    /// Request a single page of bars older than `before_ts` for scroll-left (infinite scroll).
    ///
    /// Non-blocking — spawns an async task. On completion, sends
    /// `ScrollBarsLoaded` through the update channel.
    ///
    /// This is Layer 3 of the data loading pipeline. It is triggered when the
    /// user scrolls left past the oldest bar currently in the viewport. The
    /// caller should fire it again when the user scrolls further left; the
    /// dedup guard ensures only one in-flight scroll fetch exists per key at
    /// any time.
    ///
    /// # Parameters
    ///
    /// - `before_ts`: oldest timestamp currently in the window's bar series
    ///   (seconds). The fetch retrieves bars strictly older than this.
    /// - `batch_size`: number of bars to request per page (typically 500).
    ///
    /// # Behaviour
    ///
    /// 1. Dedup: if a scroll fetch for this key is already in-flight, returns immediately.
    /// 2. Cache hit: if the session bar cache already contains bars older than
    ///    `before_ts`, extracts them and sends `ScrollBarsLoaded` without a
    ///    network request.
    /// 3. Oldest-ts guard: if we already know the exchange has no data before
    ///    `before_ts` (recorded by a previous scroll fetch), returns immediately.
    /// 4. REST fetch: fetches one page ending just before `before_ts`, merges
    ///    the result into the cache, and sends `ScrollBarsLoaded`.
    pub fn request_scroll_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        timeframe: &Timeframe,
        account_type: AccountType,
        before_ts: i64,
        batch_size: usize,
    ) {
        let pool = self.pool.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let interval = timeframe_to_interval(timeframe);
        let tf_name = timeframe.name.clone();
        let cache = self.bar_cache.clone();
        let active_fetches = self.active_fetches.clone();
        let oldest_fetched_ts = self.oldest_fetched_ts.clone();

        let cache_key = (exchange_id, account_type, symbol_str.clone(), tf_name.clone());

        // Scroll dedup key: encode "scroll:" role by prefixing the symbol,
        // matching the pattern used by "backfill:" in request_background_backfill.
        let scroll_fetch_key = (
            exchange_id,
            account_type,
            format!("scroll:{}", symbol_str),
            tf_name.clone(),
        );

        // ── Deduplication guard ───────────────────────────────────────────────
        {
            let mut af = active_fetches.lock().unwrap_or_else(|e| e.into_inner());
            if af.contains(&scroll_fetch_key) {
                eprintln!(
                    "[Bridge] scroll fetch already in flight for {:?} sym={} tf={}, skipping",
                    exchange_id, symbol_str, tf_name
                );
                return;
            }
            af.insert(scroll_fetch_key.clone());
        }

        // ── Cache check: do we already have bars older than before_ts? ────────
        let cached_older: Option<Vec<Bar>> = cache.lock().ok().and_then(|c| {
            c.get(&cache_key).map(|bars| {
                bars.iter()
                    .filter(|b| b.timestamp < before_ts)
                    .cloned()
                    .collect::<Vec<Bar>>()
            })
        });

        if let Some(ref older_bars) = cached_older {
            if !older_bars.is_empty() {
                let prepend_count = older_bars.len().min(batch_size);
                // Full merged set is whatever is in cache (already sorted).
                let full_bars: Vec<Bar> = cache
                    .lock()
                    .ok()
                    .and_then(|c| c.get(&cache_key).cloned())
                    .unwrap_or_default();

                eprintln!(
                    "[Bridge] scroll {:?} sym={} tf={}: {} cached bars older than {}, serving {} from cache",
                    exchange_id, symbol_str, tf_name, older_bars.len(), before_ts, prepend_count
                );

                let _ = tx.send(LiveUpdate::ScrollBarsLoaded {
                    exchange_id,
                    account_type,
                    symbol: symbol_str.clone(),
                    timeframe: tf_name.clone(),
                    bars: full_bars,
                    prepend_count,
                });

                if let Ok(mut af) = active_fetches.lock() {
                    af.remove(&scroll_fetch_key);
                }
                return;
            }
        }

        // ── Oldest-ts guard: no point fetching if exchange has no more data ───
        let known_oldest = oldest_fetched_ts
            .lock()
            .ok()
            .and_then(|m| m.get(&cache_key).copied());

        if let Some(oldest) = known_oldest {
            if before_ts <= oldest {
                eprintln!(
                    "[Bridge] scroll {:?} sym={} tf={}: before_ts={} <= oldest_fetched={}, nothing more from exchange",
                    exchange_id, symbol_str, tf_name, before_ts, oldest
                );
                if let Ok(mut af) = active_fetches.lock() {
                    af.remove(&scroll_fetch_key);
                }
                return;
            }
        }

        // ── REST fetch ────────────────────────────────────────────────────────
        self.runtime.spawn(async move {
            macro_rules! finish_scroll {
                () => {
                    if let Ok(mut af) = active_fetches.lock() {
                        af.remove(&scroll_fetch_key);
                    }
                };
            }

            // Wait for connector (same pattern as other methods).
            let connector = {
                let mut attempts = 0;
                loop {
                    if let Some(c) = pool.get(&exchange_id) {
                        break c;
                    }
                    attempts += 1;
                    if attempts > 50 {
                        eprintln!(
                            "[Bridge] scroll: connector {:?} not initialized after 5s",
                            exchange_id
                        );
                        finish_scroll!();
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            };

            let sym = parse_symbol_for_exchange(exchange_id, &symbol_str);

            // end_time = just before the oldest timestamp the caller has (ms).
            let end_time_ms = Some(before_ts * 1000 - 1);

            eprintln!(
                "[Bridge] scroll fetch: {:?} sym={} tf={} end_time_ms={:?} batch={}",
                exchange_id, symbol_str, tf_name, end_time_ms, batch_size
            );

            let limit = (batch_size as u16).min(500);
            let result = connector
                .get_klines(sym, &interval, Some(limit), account_type, end_time_ms)
                .await;

            match result {
                Err(e) => {
                    eprintln!(
                        "[Bridge] scroll fetch error: {:?} sym={} tf={}: {}",
                        exchange_id, symbol_str, tf_name, e
                    );
                    // Record before_ts as the oldest known — no more data here.
                    if let Ok(mut map) = oldest_fetched_ts.lock() {
                        let key = (exchange_id, account_type, symbol_str.clone(), tf_name.clone());
                        let entry = map.entry(key).or_insert(i64::MAX);
                        if before_ts < *entry {
                            *entry = before_ts;
                        }
                    }
                    finish_scroll!();
                }
                Ok(klines) if klines.is_empty() => {
                    eprintln!(
                        "[Bridge] scroll fetch: {:?} sym={} tf={}: empty page, no more history",
                        exchange_id, symbol_str, tf_name
                    );
                    // Record before_ts as the oldest known — exchange has nothing older.
                    if let Ok(mut map) = oldest_fetched_ts.lock() {
                        let key = (exchange_id, account_type, symbol_str.clone(), tf_name.clone());
                        let entry = map.entry(key).or_insert(i64::MAX);
                        if before_ts < *entry {
                            *entry = before_ts;
                        }
                    }
                    finish_scroll!();
                }
                Ok(klines) => {
                    let new_page: Vec<Bar> = klines.iter().map(kline_to_bar).collect();
                    let prepend_count = new_page.len();

                    eprintln!(
                        "[Bridge] scroll fetch: {:?} sym={} tf={}: got {} bars",
                        exchange_id, symbol_str, tf_name, prepend_count
                    );

                    // Update oldest_fetched_ts with the oldest bar from this page.
                    let oldest_bar_ts = new_page.first().map(|b| b.timestamp).unwrap_or(0);
                    if oldest_bar_ts > 0 {
                        if let Ok(mut map) = oldest_fetched_ts.lock() {
                            let key = (
                                exchange_id,
                                account_type,
                                symbol_str.clone(),
                                tf_name.clone(),
                            );
                            let entry = map.entry(key).or_insert(i64::MAX);
                            if oldest_bar_ts < *entry {
                                *entry = oldest_bar_ts;
                            }
                        }
                    }

                    // Merge new page into bar cache.
                    let merged = {
                        let current = cache
                            .lock()
                            .ok()
                            .and_then(|c| c.get(&cache_key).cloned())
                            .unwrap_or_default();
                        merge_bars(new_page, current)
                    };

                    if let Ok(mut c) = cache.lock() {
                        c.insert(cache_key.clone(), merged.clone());
                    }

                    let _ = tx.send(LiveUpdate::ScrollBarsLoaded {
                        exchange_id,
                        account_type,
                        symbol: symbol_str,
                        timeframe: tf_name,
                        bars: merged,
                        prepend_count,
                    });

                    finish_scroll!();
                }
            }
        });
    }

    /// Get a reference to the tokio runtime owned by this bridge.
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }
}


// ─────────────────────────────────────────────────────────────────────────────
// ACCOUNT TYPE HELPERS
// ─────────────────────────────────────────────────────────────────────────────

/// Convert an account type short label string back to an [`AccountType`] enum.
///
/// Unknown labels fall back to [`AccountType::Spot`] — the safe default and
/// backward-compatible case (old disk files without an account_type prefix).
pub fn account_type_from_short_label(s: &str) -> AccountType {
    match s {
        "M"  => AccountType::Margin,
        "F"  => AccountType::FuturesCross,
        "FI" => AccountType::FuturesIsolated,
        "E"  => AccountType::Earn,
        "L"  => AccountType::Lending,
        "O"  => AccountType::Options,
        "CV" => AccountType::Convert,
        _    => AccountType::Spot, // "S" and unknown → Spot
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// INTERVAL HELPERS
// ─────────────────────────────────────────────────────────────────────────────

/// Parse an interval string to seconds.
///
/// Handles standard exchange interval notation: "1s"→1, "1m"→60, "5m"→300,
/// "15m"→900, "1h"→3600, "4h"→14400, "1d"→86400, "1w"→604800.
/// Falls back to minutes when the unit is unrecognised.
fn interval_to_seconds(interval: &str) -> i64 {
    let s = interval.to_lowercase();
    if s.is_empty() {
        return 60;
    }
    let unit = s.chars().last().unwrap_or('m');
    let num_part = &s[..s.len() - 1];
    let num: i64 = num_part.parse().unwrap_or(1);
    match unit {
        's' => num,
        'm' => num * 60,
        'h' => num * 3_600,
        'd' => num * 86_400,
        'w' => num * 604_800,
        _ => num * 60,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// BAR CACHE MERGE
// ─────────────────────────────────────────────────────────────────────────────

/// Merge two sorted bar vectors by timestamp, keeping the *newer* version of
/// any bar that appears in both (same timestamp). The result is sorted
/// ascending by timestamp.
fn merge_bars(mut old: Vec<Bar>, fresh: Vec<Bar>) -> Vec<Bar> {
    if fresh.is_empty() {
        return old;
    }
    if old.is_empty() {
        return fresh;
    }

    // Fast path: if fresh bars are entirely after old bars, just append.
    let old_last_ts = old.last().map(|b| b.timestamp).unwrap_or(0);
    let fresh_first_ts = fresh.first().map(|b| b.timestamp).unwrap_or(0);
    if fresh_first_ts > old_last_ts {
        old.extend(fresh);
        return old;
    }

    // General merge: fresh bars overwrite old bars at the same timestamp.
    // Build a map from timestamp → bar (fresh wins).
    let mut map: HashMap<i64, Bar> = HashMap::with_capacity(old.len() + fresh.len());
    for bar in old {
        map.insert(bar.timestamp, bar);
    }
    for bar in fresh {
        map.insert(bar.timestamp, bar);
    }
    let mut merged: Vec<Bar> = map.into_values().collect();
    merged.sort_by_key(|b| b.timestamp);
    merged
}

// ─────────────────────────────────────────────────────────────────────────────
// SYMBOL PARSING
// ─────────────────────────────────────────────────────────────────────────────

/// Exchange-aware symbol parser.
///
/// Handles exchange-specific raw symbol formats before falling back to
/// the generic `parse_symbol()`. This avoids the lossy
/// raw → Symbol{base,quote} → format_symbol round-trip that breaks
/// exchanges with non-standard symbol conventions.
pub(crate) fn parse_symbol_for_exchange(exchange_id: ExchangeId, s: &str) -> Symbol {
    let mut sym = match exchange_id {
        // Lighter uses USDC as quote currency (not USDT).
        // Raw symbols: "BTC", "ETH", "BTCUSDC", "BTC/USDC"
        ExchangeId::Lighter => {
            // If already has separator, use generic parser
            if s.contains('/') || s.contains('-') || s.contains('_') {
                let sym = parse_symbol(s);
                // Fix: if generic parser defaulted quote to USDT, change to USDC
                if sym.quote == "USDT" {
                    Symbol::new(&sym.base, "USDC")
                } else {
                    sym
                }
            } else {
                let upper = s.to_uppercase();
                // Strip USDC/USDT suffix if present
                if upper.ends_with("USDC") && upper.len() > 4 {
                    Symbol::new(&upper[..upper.len() - 4], "USDC")
                } else if upper.ends_with("USDT") && upper.len() > 4 {
                    Symbol::new(&upper[..upper.len() - 4], "USDC")
                } else {
                    // Bare coin name like "BTC" → use USDC as default quote
                    Symbol::new(s, "USDC")
                }
            }
        }

        // HyperLiquid uses USDC as default quote (data-feed only uses base).
        // Raw symbols: "BTC", "ETH", "HYPE"
        ExchangeId::HyperLiquid => {
            let sym = parse_symbol(s);
            // HyperLiquid only uses base, but correct quote is USDC for display
            if sym.quote == "USDT" {
                Symbol::new(&sym.base, "USDC")
            } else {
                sym
            }
        }

        // Upbit uses REVERSED format: QUOTE-BASE (e.g., "KRW-BTC" means BTC priced in KRW).
        // Known Upbit quote currencies: KRW, BTC, USDT, SGD, THB, IDR
        ExchangeId::Upbit => {
            if let Some(idx) = s.find('-') {
                let left = &s[..idx];
                let right = &s[idx + 1..];
                let upper_left = left.to_uppercase();
                // Upbit quotes: KRW, BTC, USDT, SGD, THB, IDR
                let upbit_quotes = ["KRW", "BTC", "USDT", "SGD", "THB", "IDR"];
                if upbit_quotes.iter().any(|q| upper_left == *q) {
                    // Reversed: left is quote, right is base
                    Symbol::new(right, left)
                } else {
                    // Fallback: try generic parser, but default to KRW instead of USDT
                    let sym = parse_symbol(s);
                    if sym.quote == "USDT" {
                        Symbol::new(&sym.base, "KRW")
                    } else {
                        sym
                    }
                }
            } else {
                // Fallback: try generic parser, but default to KRW instead of USDT
                let sym = parse_symbol(s);
                if sym.quote == "USDT" {
                    Symbol::new(&sym.base, "KRW")
                } else {
                    sym
                }
            }
        }

        // Deribit: "BTC-PERPETUAL", "ETH-PERPETUAL", "SOL_USDC-PERPETUAL"
        // The generic parser handles this OK for now — "BTC-PERPETUAL" → {base:"BTC", quote:"PERPETUAL"}
        // and the Deribit connector knows how to handle quote="PERPETUAL"
        ExchangeId::Deribit => parse_symbol(s),

        // All other exchanges: use generic parser
        _ => parse_symbol(s),
    };
    // Always preserve the original raw input string
    sym.raw = Some(s.to_string());
    sym
}

/// Parse a symbol string like `"BTCUSDT"` into a V5 `Symbol`.
///
/// Tries common separator characters first, then falls back to matching
/// well-known quote currency suffixes. If nothing matches, returns
/// the whole string as the base with `"USDT"` as the quote.
///
/// The raw input string is always preserved in `Symbol::raw`.
fn parse_symbol(s: &str) -> Symbol {
    let original = s;

    let mut result = 'parse: {
        if let Some(idx) = s.find('/') {
            break 'parse Symbol::new(&s[..idx], &s[idx + 1..]);
        }
        // Handle Paradex-style perpetual symbols: BASE-QUOTE-PERP or BASE-QUOTE-PERP_OPTION.
        // Split on '-' but strip trailing "-PERP" / "-PERP_OPTION" suffixes so that
        // `quote` ends up as plain "USD" rather than "USD-PERP", allowing
        // `format_symbol` to reconstruct the correct "BTC-USD-PERP" market identifier.
        if let Some(idx) = s.find('-') {
            let remainder = &s[idx + 1..];
            // Check if remainder itself contains another '-', indicating a 3-part symbol.
            if let Some(second_dash) = remainder.find('-') {
                let quote = &remainder[..second_dash];
                let suffix = &remainder[second_dash + 1..];
                // Only treat as a perpetual symbol if the trailing component looks like
                // a known derivative suffix (PERP, SWAP, PERP_OPTION, etc.).
                let upper_suffix = suffix.to_uppercase();
                if upper_suffix == "PERP"
                    || upper_suffix == "SWAP"
                    || upper_suffix.starts_with("PERP_")
                {
                    break 'parse Symbol::new(&s[..idx], quote);
                }
            }
            break 'parse Symbol::new(&s[..idx], remainder);
        }
        if let Some(idx) = s.find('_') {
            break 'parse Symbol::new(&s[..idx], &s[idx + 1..]);
        }

        // Handle Bitfinex tXXXYYY format — strip the leading 't' prefix so that
        // "tBTCUSD" and "tbtcusd" both parse as base="BTC" quote="USD" rather
        // than base="tbtc" quote="usd", which would cause format_symbol to emit
        // a double-prefixed "tTBTCUSD" that Bitfinex rejects.
        let s = if (s.starts_with('t') || s.starts_with('T')) && s.len() > 4 {
            let without_t = &s[1..];
            let upper_without_t = without_t.to_uppercase();
            let has_known_quote = ["USDT", "BUSD", "USDC", "USD", "BTC", "ETH", "BNB", "EUR", "GBP", "JPY"]
                .iter()
                .any(|q| upper_without_t.ends_with(q) && upper_without_t.len() > q.len());
            if has_known_quote { without_t } else { s }
        } else {
            s
        };

        // Try common quote-currency suffixes (longest match first to avoid
        // "BTC" matching the tail of "BTCETH" incorrectly).
        let upper = s.to_uppercase();
        for quote in &["USDT", "BUSD", "USDC", "USD", "BTC", "ETH", "BNB", "EUR", "GBP", "JPY", "RUB"] {
            if upper.ends_with(quote) && upper.len() > quote.len() {
                let base_len = upper.len() - quote.len();
                break 'parse Symbol::new(&s[..base_len], &s[base_len..]);
            }
        }

        // Fallback: treat entire string as base asset
        Symbol::new(s, "USDT")
    };

    // Always preserve the original raw input string
    result.raw = Some(original.to_string());
    result
}
