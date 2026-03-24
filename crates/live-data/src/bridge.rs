//! `DataBridge` — async-to-sync bridge with a dedicated tokio runtime.
//!
//! Owns a tokio runtime and a connector pool. All async exchange calls happen
//! on the runtime's threads; results are sent back via an unbounded channel
//! that the sync chart thread can drain each frame.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use tokio::sync::{broadcast, mpsc};
use tokio::runtime::Runtime;

use digdigdig3::{
    ExchangeId, AccountType, Symbol, MarketData, SymbolInfo,
    StreamEvent, WebSocketConnector, WebSocketExt,
};
use digdigdig3::connector_manager::{ConnectorFactory, ConnectorPool};
use digdigdig3::crypto::cex::binance::BinanceWebSocket;
use digdigdig3::crypto::cex::bybit::BybitWebSocket;
use digdigdig3::crypto::cex::okx::OkxWebSocket;
use digdigdig3::crypto::cex::kucoin::KuCoinWebSocket;
use digdigdig3::crypto::cex::kraken::KrakenWebSocket;
use digdigdig3::crypto::cex::coinbase::CoinbaseWebSocket;
use digdigdig3::crypto::cex::gateio::GateioWebSocket;
use digdigdig3::crypto::cex::bitfinex::BitfinexWebSocket;
use digdigdig3::crypto::cex::bitstamp::{BitstampWebSocket, BitstampConnector};
use digdigdig3::crypto::cex::mexc::MexcWebSocket;
use digdigdig3::crypto::cex::htx::HtxWebSocket;
use digdigdig3::crypto::cex::bitget::BitgetWebSocket;
use digdigdig3::crypto::cex::bingx::BingxWebSocket;
use digdigdig3::crypto::cex::phemex::PhemexWebSocket;
use digdigdig3::crypto::cex::upbit::UpbitWebSocket;
use digdigdig3::crypto::cex::deribit::DeribitWebSocket;
use digdigdig3::crypto::cex::hyperliquid::HyperliquidWebSocket;
use digdigdig3::crypto::dex::dydx::DydxWebSocket;
use digdigdig3::crypto::dex::paradex::ParadexWebSocket;
use digdigdig3::crypto::dex::gmx::GmxWebSocket;
use digdigdig3::stocks::russia::moex::MoexWebSocket;
use digdigdig3::crypto::cex::gemini::{GeminiWebSocket, GeminiConnector};
use digdigdig3::crypto::cex::crypto_com::CryptoComWebSocket;
use digdigdig3::crypto::dex::lighter::LighterWebSocket;
use zengeld_chart::Bar;
use zengeld_chart::state::Timeframe;

use crate::convert::{kline_to_bar, timeframe_to_interval};

/// Updates sent from async tasks to the sync chart thread.
#[derive(Debug, Clone)]
pub enum LiveUpdate {
    /// Historical bars loaded from the exchange REST API.
    BarsLoaded {
        exchange_id: ExchangeId,
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
        symbol: String,
        bar: Bar,
        is_closed: bool,
    },
    /// Live trade update from WebSocket.
    TradeUpdate {
        exchange_id: ExchangeId,
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
    /// Active WebSocket subscription tasks.
    ///
    /// Key: `"exchange_id:symbol"` (e.g. `"binance:BTCUSDT"`).
    ws_tasks: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    /// Session-level bar cache.
    ///
    /// Key: `(exchange_id, symbol, timeframe_name)`. Stores bars from previous
    /// requests so that switching back to an already-visited exchange+symbol+TF
    /// is instant. On each new `request_bars`, the cache is sent immediately
    /// and then a background fetch retrieves only the *newer* bars (after the
    /// last cached timestamp).
    bar_cache: Arc<Mutex<HashMap<(ExchangeId, String, String), Vec<Bar>>>>,
    /// Session-level symbol cache.
    ///
    /// Key: `ExchangeId`. Stores the full list of trading symbols so that
    /// repeated `request_symbols` calls don't hit the network.
    symbol_cache: Arc<Mutex<HashMap<ExchangeId, Vec<SymbolInfo>>>>,
    /// In-flight bar fetch keys.
    ///
    /// Prevents duplicate concurrent fetches for the same `(exchange, symbol,
    /// timeframe)` triple. A key is inserted before the task is spawned and
    /// removed at every exit point of the task.
    active_fetches: Arc<Mutex<HashSet<(ExchangeId, String, String)>>>,
    /// Live WS ping RTT handles, keyed by exchange.
    ///
    /// Populated when a WebSocket task creates a connector that exposes a
    /// shared RTT arc (currently OKX only). `collect_metrics` reads from
    /// these handles via `try_lock` so it never blocks.
    ws_rtt_handles: Arc<Mutex<HashMap<ExchangeId, Arc<tokio::sync::Mutex<u64>>>>>,
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
            ws_tasks: Mutex::new(HashMap::new()),
            bar_cache: Arc::new(Mutex::new(HashMap::new())),
            symbol_cache: Arc::new(Mutex::new(HashMap::new())),
            active_fetches: Arc::new(Mutex::new(HashSet::new())),
            ws_rtt_handles: Arc::new(Mutex::new(HashMap::new())),
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
            match ConnectorFactory::create_public(exchange_id).await {
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
    /// **Session cache**: If bars for this `(symbol, timeframe)` were already
    /// loaded during this session, the cached bars are sent *immediately* (before
    /// any network request) via `BarsLoaded` so the chart renders without a blank
    /// frame. A background incremental fetch then retrieves only the *newer* bars
    /// (after the last cached timestamp), merges them into the cache, and sends a
    /// second `BarsLoaded` with the complete up-to-date set. If no new bars
    /// arrived, the second send is skipped entirely.
    ///
    /// When `total_bars` is `Some(n)` and the exchange supports pagination
    /// (currently only Binance), multiple sequential requests are made to
    /// fetch up to `n` bars. Otherwise, a single request is made with the
    /// given `limit` (default 500).
    pub fn request_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        timeframe: &Timeframe,
        limit: Option<u16>,
        total_bars: Option<usize>,
    ) {
        let pool = self.pool.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let interval = timeframe_to_interval(timeframe);
        let tf_name = timeframe.name.clone();
        let cache = self.bar_cache.clone();
        let active_fetches = self.active_fetches.clone();

        // ── Cache lookup: serve instantly, then do an incremental refresh ──
        let cache_key = (exchange_id, symbol_str.clone(), tf_name.clone());

        // ── Deduplication guard: skip if a fetch for this key is already running ──
        {
            let mut af = active_fetches.lock().unwrap_or_else(|e| e.into_inner());
            if af.contains(&cache_key) {
                eprintln!("[Bridge] fetch already in flight for {:?} sym={} tf={}, skipping", exchange_id, symbol_str, tf_name);
                return;
            }
            af.insert(cache_key.clone());
        }

        let cached_bars = self.bar_cache.lock().ok()
            .and_then(|c| c.get(&cache_key).cloned());

        // If we have cached bars, send them immediately so the chart renders
        // without waiting for the network round-trip.
        if let Some(ref bars) = cached_bars {
            eprintln!("[Bridge] {:?} serving {} cached bars instantly for sym={} tf={}", exchange_id, bars.len(), symbol_str, tf_name);
            let _ = tx.send(LiveUpdate::BarsLoaded {
                exchange_id,
                symbol: symbol_str.clone(),
                timeframe: tf_name.clone(),
                bars: bars.clone(),
            });
        }

        // Determine the last cached timestamp so the background task can do
        // an incremental fetch instead of a full pagination run.
        let last_cached_ts: Option<i64> = cached_bars
            .as_ref()
            .and_then(|bars| bars.last())
            .map(|b| b.timestamp);

        // ── Background fetch: incremental (if cached) or full pagination ──
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

            // ── Decide: incremental fetch or full pagination ───────────────
            let fresh_bars = if let Some(last_ts) = last_cached_ts {
                // Incremental mode: fetch only bars newer than the last cached bar.
                // Paginate forward from the last cached timestamp to handle gaps
                // larger than one page (e.g. returning after a long absence).
                let page_size: u16 = limit.unwrap_or(500).min(2000);
                let mut all_new: Vec<Bar> = Vec::new();
                let mut end_time_cursor: Option<i64> = None;
                let mut pages: usize = 0;
                eprintln!("[Bridge] incremental fetch: {:?} sym={} interval={} after_ts={} page_size={}", exchange_id, symbol_str, interval, last_ts, page_size);

                loop {
                    let result = connector
                        .get_klines(sym.clone(), &interval, Some(page_size), AccountType::Spot, end_time_cursor)
                        .await;

                    match result {
                        Ok(klines) => {
                            if klines.is_empty() {
                                break;
                            }
                            pages += 1;
                            let batch: Vec<Bar> = klines
                                .iter()
                                .map(kline_to_bar)
                                .filter(|b| b.timestamp > last_ts)
                                .collect();
                            let full_page = klines.len();
                            // If the most recent page has no bars newer than our cache,
                            // we're already up to date — no need to paginate backward.
                            if batch.is_empty() && pages == 1 {
                                break;
                            }
                            // Find the oldest timestamp in this page for backward pagination
                            if let Some(oldest_ts) = klines.iter().map(|k| k.open_time as i64).min() {
                                // If oldest bar in this page is already older than our cache,
                                // we've covered the gap — stop paginating.
                                if oldest_ts <= last_ts {
                                    all_new.extend(batch);
                                    break;
                                }
                                end_time_cursor = Some(oldest_ts * 1000 - 1);
                            } else {
                                all_new.extend(batch);
                                break;
                            }
                            all_new.extend(batch);
                            // If we got less than a full page, no more data available
                            if full_page < page_size as usize {
                                break;
                            }
                            // Safety: don't paginate more than 20 pages for incremental
                            if pages >= 20 {
                                eprintln!("[Bridge] incremental fetch capped at {} pages", pages);
                                break;
                            }
                        }
                        Err(e) => {
                            eprintln!("[Bridge] {:?} incremental fetch error: {}", exchange_id, e);
                            if all_new.is_empty() {
                                // Non-fatal: cached bars were already served; nothing more to do.
                                finish_fetch!();
                                return;
                            }
                            break;
                        }
                    }
                }
                // Deduplicate by timestamp (in case of overlap between pages)
                all_new.sort_by_key(|b| b.timestamp);
                all_new.dedup_by_key(|b| b.timestamp);
                eprintln!("[Bridge] incremental fetch: {:?} {} new bars over {} pages (ts > {})", exchange_id, all_new.len(), pages, last_ts);
                all_new
            } else {
                // Full pagination mode: no cache exists — fetch as many bars as requested.
                let desired_total = total_bars.unwrap_or(2000);
                // Request large pages — each connector clamps to its own API max.
                let page_size: u16 = limit.unwrap_or(2000).min(2000);
                let mut all_bars: Vec<Bar> = Vec::with_capacity(desired_total);
                let mut end_time_cursor: Option<i64> = None;
                let mut pages: usize = 0;

                eprintln!("[Bridge] full fetch: {:?} sym={} interval={} desired={} page_size={}", exchange_id, symbol_str, interval, desired_total, page_size);

                'paginate: loop {
                    let prev_count = all_bars.len();

                    let result = connector
                        .get_klines(sym.clone(), &interval, Some(page_size), AccountType::Spot, end_time_cursor)
                        .await;

                    match result {
                        Ok(klines) => {
                            if klines.is_empty() {
                                break 'paginate;
                            }

                            let got = klines.len();
                            let batch: Vec<Bar> = klines.iter().map(kline_to_bar).collect();

                            // The oldest bar's timestamp drives the next cursor.
                            // Convert to milliseconds (most exchanges expect ms).
                            if let Some(oldest) = batch.first() {
                                end_time_cursor = Some(oldest.timestamp * 1000 - 1);
                            }

                            // Prepend this (older) batch before what we already have.
                            all_bars = merge_bars(batch, all_bars);
                            pages += 1;

                            eprintln!("[Bridge] {:?} page {} -> {} bars total (got {}) next_end_time={:?}", exchange_id, pages, all_bars.len(), got, end_time_cursor);

                            // Stop when we have enough bars.
                            if all_bars.len() >= desired_total {
                                break 'paginate;
                            }
                            if all_bars.len() == prev_count {
                                eprintln!("[Bridge] {:?} exchange doesn't support pagination (no new bars), stopping", exchange_id);
                                break 'paginate;
                            }
                            if got < 10 {
                                // Tiny batch = probably at the beginning of the asset's history.
                                break 'paginate;
                            }
                        }
                        Err(e) => {
                            if all_bars.is_empty() {
                                let _ = tx.send(LiveUpdate::Error {
                                    exchange_id,
                                    message: format!("get_klines failed: {}", e),
                                });
                                finish_fetch!();
                                return;
                            }
                            eprintln!("[Bridge] {:?} pagination stopped at page {} due to error: {}", exchange_id, pages, e);
                            break 'paginate;
                        }
                    }
                }

                eprintln!("[Bridge] {:?} full fetch done: {} bars over {} pages", exchange_id, all_bars.len(), pages);
                all_bars
            };

            if let (Some(first), Some(last)) = (fresh_bars.first(), fresh_bars.last()) {
                eprintln!("[Bridge] {:?} fresh range: {} -> {} ({} bars)", exchange_id, first.timestamp, last.timestamp, fresh_bars.len());
            }

            // Merge fresh bars into cached set (or use fresh directly if no cache).
            let merged = if let Some(old) = cached_bars {
                if fresh_bars.is_empty() {
                    // Nothing new from the incremental fetch — cached data is already current.
                    eprintln!("[Bridge] {:?} incremental fetch: no new bars, cache already up to date", exchange_id);
                    // No need to re-send; cached bars were already pushed above.
                    finish_fetch!();
                    return;
                }
                eprintln!("[Bridge] {:?} merging {} fresh + {} cached", exchange_id, fresh_bars.len(), old.len());
                merge_bars(old, fresh_bars)
            } else {
                fresh_bars
            };

            // Update cache.
            if let Ok(mut c) = cache.lock() {
                c.insert(cache_key.clone(), merged.clone());
            }

            let _ = tx.send(LiveUpdate::BarsLoaded {
                exchange_id,
                symbol: symbol_str,
                timeframe: tf_name,
                bars: merged,
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
        limit: Option<u16>,
        total_bars: Option<usize>,
    ) -> Option<Vec<Bar>> {
        let connector = self.pool.get(&exchange_id)?;
        let sym = parse_symbol_for_exchange(exchange_id, symbol);
        let interval = timeframe_to_interval(timeframe);

        self.runtime.block_on(async move {
            let page_size: u16 = limit.unwrap_or(500).min(500);
            let result = connector.get_klines(sym, &interval, Some(page_size), AccountType::Spot, None).await;

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

            // Uses get_exchange_info from the MarketData trait, available on all connectors.
            // Non-supporting connectors return UnsupportedOperation.
            let result = connector.get_exchange_info(AccountType::Spot).await;

            match result {
                Ok(symbols) => {
                    // Cache for session.
                    if let Ok(mut c) = cache.lock() {
                        c.insert(exchange_id, symbols.clone());
                    }
                    let _ = tx.send(LiveUpdate::SymbolsLoaded {
                        exchange_id,
                        symbols,
                    });
                }
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("get_exchange_info failed: {}", e),
                    });
                }
            }
        });
    }

    /// Subscribe to live trade updates via WebSocket.
    ///
    /// Spawns a long-running async task that connects to the exchange WebSocket,
    /// subscribes to the trade stream, and sends `LiveUpdate::TradeUpdate` for
    /// each incoming trade event.
    ///
    /// If a subscription already exists for `exchange_id:symbol`, the existing
    /// task is aborted before a new one is started.
    pub fn subscribe_trades(&self, exchange_id: ExchangeId, symbol: &str) {
        let key = format!("{}:{}", exchange_id.as_str(), symbol);

        // Abort any existing task for the same stream.
        if let Ok(mut tasks) = self.ws_tasks.lock() {
            if let Some(handle) = tasks.remove(&key) {
                handle.abort();
            }
        }

        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let ws_rtt_handles = self.ws_rtt_handles.clone();

        let handle = self.runtime.spawn(async move {
            let mut retry_count: u32 = 0;
            loop {
                if retry_count > 0 {
                    // Exponential backoff capped at 30s, plus simple pseudo-jitter.
                    let base_secs = std::cmp::min(1u64 << std::cmp::min(retry_count - 1, 5), 30);
                    // Jitter: 0..500ms derived from retry_count to avoid thundering herd.
                    let jitter_ms = (retry_count as u64 * 137) % 500;
                    let delay = std::time::Duration::from_millis(base_secs * 1000 + jitter_ms);

                    if retry_count <= 3 || retry_count % 5 == 0 {
                        eprintln!("[WS] Reconnect attempt {} for trade stream {:?} {} after {:?}", retry_count, exchange_id, symbol_str, delay);
                    }
                    tokio::time::sleep(delay).await;
                }
                retry_count += 1;
                run_generic_trade_ws(tx.clone(), exchange_id, symbol_str.clone(), ws_rtt_handles.clone()).await;
                // Stream ended or connection failed — trigger backfill to fill any gap
                // before reconnecting. ConnectorReady causes lib.rs to call request_bars().
                let _ = tx.send(LiveUpdate::ConnectorReady { exchange_id });
                // Loop and retry.
            }
        });

        if let Ok(mut tasks) = self.ws_tasks.lock() {
            tasks.insert(key, handle);
        }
    }

    /// Subscribe to a single symbol's mini ticker stream via WebSocket.
    ///
    /// Key format: `"miniticker:exchange_id:SYMBOL"`. If a subscription already
    /// exists for this key, the existing task is aborted first.
    pub fn subscribe_mini_ticker(&self, exchange_id: ExchangeId, symbol: &str) {
        let key = format!("miniticker:{}:{}", exchange_id.as_str(), symbol);

        if let Ok(mut tasks) = self.ws_tasks.lock() {
            if let Some(handle) = tasks.remove(&key) {
                handle.abort();
            }
        }

        let tx = self.tx.clone();
        let original_symbol = symbol.to_string();
        let symbol_lower = symbol.to_lowercase();
        let ws_rtt_handles = self.ws_rtt_handles.clone();

        let handle = self.runtime.spawn(async move {
            let mut retry_count: u32 = 0;
            loop {
                if retry_count > 0 {
                    // Exponential backoff capped at 30s, plus simple pseudo-jitter.
                    let base_secs = std::cmp::min(1u64 << std::cmp::min(retry_count - 1, 5), 30);
                    // Jitter: 0..500ms derived from retry_count to avoid thundering herd.
                    let jitter_ms = (retry_count as u64 * 137) % 500;
                    let delay = std::time::Duration::from_millis(base_secs * 1000 + jitter_ms);

                    if retry_count <= 3 || retry_count % 5 == 0 {
                        eprintln!("[WS] Reconnect attempt {} for ticker stream {:?} {} after {:?}", retry_count, exchange_id, symbol_lower, delay);
                    }
                    tokio::time::sleep(delay).await;
                }
                retry_count += 1;
                run_generic_ticker_ws(tx.clone(), exchange_id, symbol_lower.clone(), original_symbol.clone(), ws_rtt_handles.clone()).await;
                // If we reach here the stream ended or connection failed — loop and retry.
            }
        });

        if let Ok(mut tasks) = self.ws_tasks.lock() {
            tasks.insert(key, handle);
        }
    }

    /// Abort trade WebSocket subscriptions (not mini ticker).
    ///
    /// Call this when switching symbols to stop old trade streams.
    /// Mini ticker subscriptions (watchlist) are preserved.
    pub fn unsubscribe_all(&self) {
        if let Ok(mut tasks) = self.ws_tasks.lock() {
            let keys_to_remove: Vec<String> = tasks.keys()
                .filter(|k| !k.starts_with("miniticker:"))
                .cloned()
                .collect();
            for key in keys_to_remove {
                if let Some(handle) = tasks.remove(&key) {
                    handle.abort();
                }
            }
        }
    }

    /// Unsubscribe a single mini-ticker stream (when symbol removed from watchlist).
    pub fn unsubscribe_mini_ticker(&self, exchange_id: ExchangeId, symbol: &str) {
        let key = format!("miniticker:{}:{}", exchange_id.as_str(), symbol);
        if let Ok(mut tasks) = self.ws_tasks.lock() {
            if let Some(handle) = tasks.remove(&key) {
                handle.abort();
            }
        }
    }

    /// Stop all WebSocket tasks for a specific exchange.
    pub fn unsubscribe_exchange(&self, exchange_id: ExchangeId) {
        let prefix = exchange_id.as_str().to_string();
        if let Ok(mut ws) = self.ws_tasks.lock() {
            let keys_to_remove: Vec<String> = ws.keys()
                .filter(|k| {
                    // Match keys like "exchange:symbol" or "miniticker:exchange:symbol"
                    let exchange_str = prefix.as_str();
                    k.starts_with(&format!("{}:", exchange_str))
                        || k.contains(&format!(":{}:", exchange_str))
                })
                .cloned()
                .collect();
            for key in keys_to_remove {
                if let Some(handle) = ws.remove(&key) {
                    handle.abort();
                    eprintln!("[Bridge] Aborted WS task: {}", key);
                }
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

    /// Count active WebSocket tasks for a specific exchange.
    ///
    /// A task is considered active if it has been registered and its
    /// `JoinHandle` has not yet finished.
    pub fn ws_task_count(&self, exchange_id: ExchangeId) -> usize {
        let prefix = exchange_id.as_str();
        let ws = self.ws_tasks.lock().unwrap();
        ws.iter()
            .filter(|(key, handle)| key.contains(prefix) && !handle.is_finished())
            .count()
    }

    /// Count total active WebSocket tasks across all exchanges.
    pub fn ws_task_count_total(&self) -> usize {
        let ws = self.ws_tasks.lock().unwrap();
        ws.iter()
            .filter(|(_, handle)| !handle.is_finished())
            .count()
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

    /// Get cached bars for a specific (exchange, symbol, timeframe) key.
    ///
    /// Returns `None` if the key is not in the cache or the lock is poisoned.
    pub fn get_cached_bars(
        &self,
        exchange_id: &ExchangeId,
        symbol: &str,
        timeframe: &str,
    ) -> Option<Vec<Bar>> {
        let cache = self.bar_cache.lock().ok()?;
        cache
            .get(&(*exchange_id, symbol.to_string(), timeframe.to_string()))
            .cloned()
    }

    /// Return all keys currently stored in the bar cache.
    pub fn cached_bar_keys(&self) -> Vec<(ExchangeId, String, String)> {
        self.bar_cache
            .lock()
            .map(|c| c.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Snapshot the entire bar cache for disk persistence.
    ///
    /// Returns `(exchange_str, symbol, timeframe, bars)` tuples for all cached entries.
    pub fn dump_cache_snapshot(&self) -> Vec<(String, String, String, Vec<bar_store::Bar>)> {
        let Ok(cache) = self.bar_cache.lock() else {
            return vec![];
        };
        cache
            .iter()
            .map(|((ex, sym, tf), bars)| {
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
                (ex.as_str().to_string(), sym.clone(), tf.clone(), store_bars)
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
        entries: Vec<(String, String, String, Vec<bar_store::Bar>)>,
    ) {
        let Ok(mut cache) = self.bar_cache.lock() else {
            return;
        };
        for (exchange_str, symbol, timeframe, store_bars) in entries {
            if store_bars.is_empty() {
                continue;
            }
            let Some(exchange_id) = digdigdig3::ExchangeId::from_str(&exchange_str) else {
                continue;
            };
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
            let key = (exchange_id, symbol, timeframe);
            cache.entry(key).or_insert(bars);
        }
    }

    /// Get a reference to the tokio runtime owned by this bridge.
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// WEBSOCKET MACRO
// ─────────────────────────────────────────────────────────────────────────────

/// Create a WebSocket, connect, subscribe, and return the event stream.
///
/// Used for WS types with `async fn new(Option<Credentials>, bool, AccountType)`.
macro_rules! ws_standard {
    ($ws_type:ty, $tx:expr, $exchange_id:expr, $symbol:expr, $subscribe_fn:ident, $ws_rtt_handles:expr) => {{
        let mut ws = match <$ws_type>::new(None, false, AccountType::Spot).await {
            Ok(ws) => ws,
            Err(e) => {
                let _ = $tx.send(LiveUpdate::Error {
                    exchange_id: $exchange_id,
                    message: format!("WS create failed: {}", e),
                });
                return;
            }
        };
        if let Err(e) = ws.connect(AccountType::Spot).await {
            let _ = $tx.send(LiveUpdate::Error {
                exchange_id: $exchange_id,
                message: format!("WS connect failed: {}", e),
            });
            return;
        }
        if let Err(e) = ws.$subscribe_fn($symbol).await {
            let _ = $tx.send(LiveUpdate::Error {
                exchange_id: $exchange_id,
                message: format!("WS subscribe failed: {}", e),
            });
            return;
        }
        // Capture RTT handle if the connector provides one.
        if let Some(rtt_handle) = ws.ping_rtt_handle() {
            if let Ok(mut handles) = $ws_rtt_handles.lock() {
                handles.insert($exchange_id, rtt_handle);
            }
        }
        ws.event_stream()
    }};
}

/// Create a WS with `async fn new(Option<Credentials>, bool)` (no AccountType in ctor).
macro_rules! ws_no_account_type {
    ($ws_type:ty, $tx:expr, $exchange_id:expr, $symbol:expr, $subscribe_fn:ident, $ws_rtt_handles:expr) => {{
        let mut ws = match <$ws_type>::new(None, false).await {
            Ok(ws) => ws,
            Err(e) => {
                let _ = $tx.send(LiveUpdate::Error {
                    exchange_id: $exchange_id,
                    message: format!("WS create failed: {}", e),
                });
                return;
            }
        };
        if let Err(e) = ws.connect(AccountType::Spot).await {
            let _ = $tx.send(LiveUpdate::Error {
                exchange_id: $exchange_id,
                message: format!("WS connect failed: {}", e),
            });
            return;
        }
        if let Err(e) = ws.$subscribe_fn($symbol).await {
            let _ = $tx.send(LiveUpdate::Error {
                exchange_id: $exchange_id,
                message: format!("WS subscribe failed: {}", e),
            });
            return;
        }
        // Capture RTT handle if the connector provides one.
        if let Some(rtt_handle) = ws.ping_rtt_handle() {
            if let Ok(mut handles) = $ws_rtt_handles.lock() {
                handles.insert($exchange_id, rtt_handle);
            }
        }
        ws.event_stream()
    }};
}

/// Create a WS with `async fn new(Option<Credentials>)` (no testnet, no AccountType).
macro_rules! ws_credentials_only {
    ($ws_type:ty, $tx:expr, $exchange_id:expr, $symbol:expr, $subscribe_fn:ident, $ws_rtt_handles:expr) => {{
        let mut ws = match <$ws_type>::new(None).await {
            Ok(ws) => ws,
            Err(e) => {
                let _ = $tx.send(LiveUpdate::Error {
                    exchange_id: $exchange_id,
                    message: format!("WS create failed: {}", e),
                });
                return;
            }
        };
        if let Err(e) = ws.connect(AccountType::Spot).await {
            let _ = $tx.send(LiveUpdate::Error {
                exchange_id: $exchange_id,
                message: format!("WS connect failed: {}", e),
            });
            return;
        }
        if let Err(e) = ws.$subscribe_fn($symbol).await {
            let _ = $tx.send(LiveUpdate::Error {
                exchange_id: $exchange_id,
                message: format!("WS subscribe failed: {}", e),
            });
            return;
        }
        // Capture RTT handle if the connector provides one.
        if let Some(rtt_handle) = ws.ping_rtt_handle() {
            if let Ok(mut handles) = $ws_rtt_handles.lock() {
                handles.insert($exchange_id, rtt_handle);
            }
        }
        ws.event_stream()
    }};
}

/// Create a WS with sync `fn new(Option<Credentials>, bool, AccountType)`.
macro_rules! ws_sync_new {
    ($ws_type:ty, $tx:expr, $exchange_id:expr, $symbol:expr, $subscribe_fn:ident, $ws_rtt_handles:expr) => {{
        let mut ws = match <$ws_type>::new(None, false, AccountType::Spot) {
            Ok(ws) => ws,
            Err(e) => {
                let _ = $tx.send(LiveUpdate::Error {
                    exchange_id: $exchange_id,
                    message: format!("WS create failed: {}", e),
                });
                return;
            }
        };
        if let Err(e) = ws.connect(AccountType::Spot).await {
            let _ = $tx.send(LiveUpdate::Error {
                exchange_id: $exchange_id,
                message: format!("WS connect failed: {}", e),
            });
            return;
        }
        if let Err(e) = ws.$subscribe_fn($symbol).await {
            let _ = $tx.send(LiveUpdate::Error {
                exchange_id: $exchange_id,
                message: format!("WS subscribe failed: {}", e),
            });
            return;
        }
        // Capture RTT handle if the connector provides one.
        if let Some(rtt_handle) = ws.ping_rtt_handle() {
            if let Ok(mut handles) = $ws_rtt_handles.lock() {
                handles.insert($exchange_id, rtt_handle);
            }
        }
        ws.event_stream()
    }};
}

// ─────────────────────────────────────────────────────────────────────────────
// GENERIC TRADE WEBSOCKET RUNNER
// ─────────────────────────────────────────────────────────────────────────────

/// Long-running async function that drives a trade WebSocket stream for any exchange.
///
/// Connects, subscribes, and forwards `StreamEvent::Trade` events as
/// `LiveUpdate::TradeUpdate` until the stream ends or the task is cancelled.
async fn run_generic_trade_ws(
    tx: broadcast::Sender<LiveUpdate>,
    exchange_id: ExchangeId,
    symbol_str: String,
    ws_rtt_handles: Arc<Mutex<HashMap<ExchangeId, Arc<tokio::sync::Mutex<u64>>>>>,
) {
    let sym = parse_symbol_for_exchange(exchange_id, &symbol_str);

    let mut stream = match exchange_id {
        // ── CEX: standard constructors (credentials, testnet, account_type) ──
        ExchangeId::Binance => {
            ws_standard!(BinanceWebSocket, tx, exchange_id, sym, subscribe_trades, ws_rtt_handles)
        }
        ExchangeId::Bybit => {
            ws_standard!(BybitWebSocket, tx, exchange_id, sym, subscribe_trades, ws_rtt_handles)
        }
        ExchangeId::GateIO => {
            ws_standard!(GateioWebSocket, tx, exchange_id, sym, subscribe_trades, ws_rtt_handles)
        }
        ExchangeId::Bitfinex => {
            ws_standard!(BitfinexWebSocket, tx, exchange_id, sym, subscribe_trades, ws_rtt_handles)
        }
        ExchangeId::BingX => {
            ws_standard!(BingxWebSocket, tx, exchange_id, sym, subscribe_trades, ws_rtt_handles)
        }
        ExchangeId::Bitget => {
            ws_standard!(BitgetWebSocket, tx, exchange_id, sym, subscribe_trades, ws_rtt_handles)
        }
        ExchangeId::Deribit => {
            ws_standard!(DeribitWebSocket, tx, exchange_id, sym, subscribe_trades, ws_rtt_handles)
        }
        ExchangeId::KuCoin => {
            ws_standard!(KuCoinWebSocket, tx, exchange_id, sym, subscribe_trades, ws_rtt_handles)
        }

        // ── CEX: (credentials, testnet) — no AccountType in constructor ──
        ExchangeId::OKX => {
            let mut ws = match OkxWebSocket::new(None, false).await {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("WS create failed: {}", e),
                    });
                    return;
                }
            };
            if let Err(e) = ws.connect(AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = ws.subscribe_trades(sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = <OkxWebSocket as WebSocketConnector>::ping_rtt_handle(&ws) {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            ws.event_stream()
        }
        ExchangeId::Phemex => {
            ws_no_account_type!(PhemexWebSocket, tx, exchange_id, sym, subscribe_trades, ws_rtt_handles)
        }
        ExchangeId::Paradex => {
            ws_no_account_type!(ParadexWebSocket, tx, exchange_id, sym, subscribe_trades, ws_rtt_handles)
        }

        // ── CEX: credentials only (no testnet, no account_type) ──
        ExchangeId::Coinbase => {
            ws_credentials_only!(CoinbaseWebSocket, tx, exchange_id, sym, subscribe_trades, ws_rtt_handles)
        }
        ExchangeId::MEXC => {
            ws_credentials_only!(MexcWebSocket, tx, exchange_id, sym, subscribe_trades, ws_rtt_handles)
        }

        // ── CEX: sync constructors ──
        ExchangeId::HTX => {
            ws_sync_new!(HtxWebSocket, tx, exchange_id, sym, subscribe_trades, ws_rtt_handles)
        }

        // ── CEX: custom constructors ──
        ExchangeId::Kraken => {
            let mut ws = match KrakenWebSocket::new(None, AccountType::Spot).await {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("WS create failed: {}", e),
                    });
                    return;
                }
            };
            if let Err(e) = ws.connect(AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = ws.subscribe_trades(sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = ws.ping_rtt_handle() {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            ws.event_stream()
        }
        ExchangeId::Bitstamp => {
            // BitstampWebSocket::new() takes no arguments
            let mut ws = match BitstampWebSocket::new().await {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("WS create failed: {}", e),
                    });
                    return;
                }
            };
            if let Err(e) = ws.connect(AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = ws.subscribe_trades(sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = ws.ping_rtt_handle() {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            ws.event_stream()
        }
        ExchangeId::HyperLiquid => {
            let mut ws = HyperliquidWebSocket::new(false);
            if let Err(e) = ws.connect(AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = ws.subscribe_trades(sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = ws.ping_rtt_handle() {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            ws.event_stream()
        }
        ExchangeId::Upbit => {
            // Upbit uses region string; default to Singapore
            let mut ws = match UpbitWebSocket::new(None, "sg").await {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("WS create failed: {}", e),
                    });
                    return;
                }
            };
            if let Err(e) = ws.connect(AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = ws.subscribe_trades(sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = ws.ping_rtt_handle() {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            ws.event_stream()
        }
        ExchangeId::Dydx => {
            let mut ws = match DydxWebSocket::new(false, AccountType::Spot).await {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("WS create failed: {}", e),
                    });
                    return;
                }
            };
            if let Err(e) = ws.connect(AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = ws.subscribe_trades(sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = ws.ping_rtt_handle() {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            ws.event_stream()
        }
        ExchangeId::Gmx => {
            let mut ws = match GmxWebSocket::new(None).await {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("WS create failed: {}", e),
                    });
                    return;
                }
            };
            if let Err(e) = ws.connect(AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = ws.subscribe_trades(sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = ws.ping_rtt_handle() {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            ws.event_stream()
        }
        ExchangeId::Moex => {
            let mut ws = MoexWebSocket::new_public();
            if let Err(e) = ws.connect(AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = ws.subscribe_trades(sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = ws.ping_rtt_handle() {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            ws.event_stream()
        }

        ExchangeId::Gemini => {
            let mut ws = match GeminiWebSocket::new_market_data(false).await {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("WS create failed: {}", e),
                    });
                    return;
                }
            };
            if let Err(e) = WebSocketConnector::connect(&mut ws, AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = WebSocketExt::subscribe_trades(&mut ws, sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = WebSocketConnector::ping_rtt_handle(&ws) {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            WebSocketConnector::event_stream(&ws)
        }
        ExchangeId::CryptoCom => {
            let mut ws = CryptoComWebSocket::new(None, false);
            if let Err(e) = WebSocketConnector::connect(&mut ws, AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = WebSocketExt::subscribe_trades(&mut ws, sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = WebSocketConnector::ping_rtt_handle(&ws) {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            WebSocketConnector::event_stream(&ws)
        }
        ExchangeId::Lighter => {
            let mut ws = match LighterWebSocket::public(false).await {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("WS create failed: {}", e),
                    });
                    return;
                }
            };
            if let Err(e) = WebSocketConnector::connect(&mut ws, AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = WebSocketExt::subscribe_trades(&mut ws, sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = WebSocketConnector::ping_rtt_handle(&ws) {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            WebSocketConnector::event_stream(&ws)
        }

        // Exchanges without WS support or with complex constructors
        other => {
            let _ = tx.send(LiveUpdate::Error {
                exchange_id: other,
                message: format!("WebSocket trade subscription not supported for {:?}", other),
            });
            return;
        }
    };

    // Generic event loop — same for all exchanges
    loop {
        let result = match tokio::time::timeout(
            std::time::Duration::from_secs(60),
            stream.next(),
        )
        .await
        {
            Ok(Some(result)) => result,
            Ok(None) => break, // stream ended normally
            Err(_) => {
                // No data for 60s — assume the connection is silently dead.
                eprintln!("[WS trades] No data for 60s on {:?}, reconnecting", exchange_id);
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: "WS timeout — no data for 60s".to_string(),
                });
                break;
            }
        };
        match result {
            Ok(StreamEvent::Trade(trade)) => {
                let _ = tx.send(LiveUpdate::TradeUpdate {
                    exchange_id,
                    symbol: symbol_str.clone(),
                    price: trade.price,
                    quantity: trade.quantity,
                    timestamp: trade.timestamp,
                });
            }
            Ok(_) => {}
            Err(e) => {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS stream error: {}", e),
                });
                break;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GENERIC TICKER WEBSOCKET RUNNER
// ─────────────────────────────────────────────────────────────────────────────

/// Long-running async function that drives a ticker WebSocket stream for any exchange.
///
/// Connects, subscribes to the ticker stream, and forwards `StreamEvent::Ticker`
/// events as `LiveUpdate::MiniTickerUpdate` until the stream ends or the task is cancelled.
async fn run_generic_ticker_ws(
    tx: broadcast::Sender<LiveUpdate>,
    exchange_id: ExchangeId,
    symbol_lower: String,
    original_symbol: String,
    ws_rtt_handles: Arc<Mutex<HashMap<ExchangeId, Arc<tokio::sync::Mutex<u64>>>>>,
) {
    let sym = parse_symbol_for_exchange(exchange_id, &symbol_lower);

    let mut stream = match exchange_id {
        // ── CEX: standard constructors (credentials, testnet, account_type) ──
        ExchangeId::Binance => {
            ws_standard!(BinanceWebSocket, tx, exchange_id, sym, subscribe_ticker, ws_rtt_handles)
        }
        ExchangeId::Bybit => {
            ws_standard!(BybitWebSocket, tx, exchange_id, sym, subscribe_ticker, ws_rtt_handles)
        }
        ExchangeId::GateIO => {
            ws_standard!(GateioWebSocket, tx, exchange_id, sym, subscribe_ticker, ws_rtt_handles)
        }
        ExchangeId::Bitfinex => {
            ws_standard!(BitfinexWebSocket, tx, exchange_id, sym, subscribe_ticker, ws_rtt_handles)
        }
        ExchangeId::BingX => {
            ws_standard!(BingxWebSocket, tx, exchange_id, sym, subscribe_ticker, ws_rtt_handles)
        }
        ExchangeId::Bitget => {
            ws_standard!(BitgetWebSocket, tx, exchange_id, sym, subscribe_ticker, ws_rtt_handles)
        }
        ExchangeId::Deribit => {
            ws_standard!(DeribitWebSocket, tx, exchange_id, sym, subscribe_ticker, ws_rtt_handles)
        }
        ExchangeId::KuCoin => {
            ws_standard!(KuCoinWebSocket, tx, exchange_id, sym, subscribe_ticker, ws_rtt_handles)
        }

        // ── CEX: (credentials, testnet) — no AccountType in constructor ──
        ExchangeId::OKX => {
            let mut ws = match OkxWebSocket::new(None, false).await {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("WS create failed: {}", e),
                    });
                    return;
                }
            };
            if let Err(e) = ws.connect(AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = ws.subscribe_ticker(sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = <OkxWebSocket as WebSocketConnector>::ping_rtt_handle(&ws) {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            ws.event_stream()
        }
        ExchangeId::Phemex => {
            ws_no_account_type!(PhemexWebSocket, tx, exchange_id, sym, subscribe_ticker, ws_rtt_handles)
        }
        ExchangeId::Paradex => {
            ws_no_account_type!(ParadexWebSocket, tx, exchange_id, sym, subscribe_ticker, ws_rtt_handles)
        }

        // ── CEX: credentials only ──
        ExchangeId::Coinbase => {
            ws_credentials_only!(CoinbaseWebSocket, tx, exchange_id, sym, subscribe_ticker, ws_rtt_handles)
        }
        ExchangeId::MEXC => {
            ws_credentials_only!(MexcWebSocket, tx, exchange_id, sym, subscribe_ticker, ws_rtt_handles)
        }

        // ── CEX: sync constructors ──
        ExchangeId::HTX => {
            ws_sync_new!(HtxWebSocket, tx, exchange_id, sym, subscribe_ticker, ws_rtt_handles)
        }

        // ── CEX: custom constructors ──
        ExchangeId::Kraken => {
            let mut ws = match KrakenWebSocket::new(None, AccountType::Spot).await {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("WS create failed: {}", e),
                    });
                    return;
                }
            };
            if let Err(e) = ws.connect(AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = ws.subscribe_ticker(sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = ws.ping_rtt_handle() {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            ws.event_stream()
        }
        ExchangeId::Bitstamp => {
            let mut ws = match BitstampWebSocket::new().await {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("WS create failed: {}", e),
                    });
                    return;
                }
            };
            if let Err(e) = ws.connect(AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = ws.subscribe_ticker(sym.clone()).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Spawn a REST polling task for 24h stats (price_change%, high, low, volume).
            // Bitstamp's WS live_trades channel only carries last_price; the 24h
            // fields come from the ticker REST endpoint polled every 30 seconds.
            {
                let tx_rest = tx.clone();
                let rest_sym = sym.clone();
                let rest_original_symbol = original_symbol.clone();
                tokio::spawn(async move {
                    let connector = match BitstampConnector::public().await {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!("[bitstamp rest_poll] connector create error: {}", e);
                            return;
                        }
                    };
                    loop {
                        match connector.get_ticker(rest_sym.clone(), AccountType::Spot).await {
                            Ok(ticker) => {
                                let _ = tx_rest.send(LiveUpdate::MiniTickerUpdate {
                                    exchange_id,
                                    symbol: rest_original_symbol.clone(),
                                    last_price: ticker.last_price,
                                    price_change_percent: ticker.price_change_percent_24h,
                                    high_price: ticker.high_24h,
                                    low_price: ticker.low_24h,
                                    volume: ticker.volume_24h,
                                });
                            }
                            Err(e) => {
                                eprintln!("[bitstamp rest_poll] get_ticker error: {}", e);
                            }
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    }
                });
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = ws.ping_rtt_handle() {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            ws.event_stream()
        }
        ExchangeId::HyperLiquid => {
            let mut ws = HyperliquidWebSocket::new(false);
            if let Err(e) = ws.connect(AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = ws.subscribe_ticker(sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = ws.ping_rtt_handle() {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            ws.event_stream()
        }
        ExchangeId::Upbit => {
            let mut ws = match UpbitWebSocket::new(None, "sg").await {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("WS create failed: {}", e),
                    });
                    return;
                }
            };
            if let Err(e) = ws.connect(AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = ws.subscribe_ticker(sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = ws.ping_rtt_handle() {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            ws.event_stream()
        }
        ExchangeId::Dydx => {
            let mut ws = match DydxWebSocket::new(false, AccountType::Spot).await {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("WS create failed: {}", e),
                    });
                    return;
                }
            };
            if let Err(e) = ws.connect(AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = ws.subscribe_ticker(sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = ws.ping_rtt_handle() {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            ws.event_stream()
        }
        ExchangeId::Gmx => {
            let mut ws = match GmxWebSocket::new(None).await {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("WS create failed: {}", e),
                    });
                    return;
                }
            };
            if let Err(e) = ws.connect(AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = ws.subscribe_ticker(sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = ws.ping_rtt_handle() {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            ws.event_stream()
        }
        ExchangeId::Moex => {
            let mut ws = MoexWebSocket::new_public();
            if let Err(e) = ws.connect(AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = ws.subscribe_ticker(sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = ws.ping_rtt_handle() {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            ws.event_stream()
        }

        ExchangeId::Gemini => {
            let mut ws = match GeminiWebSocket::new_market_data(false).await {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("WS create failed: {}", e),
                    });
                    return;
                }
            };
            if let Err(e) = WebSocketConnector::connect(&mut ws, AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = WebSocketExt::subscribe_ticker(&mut ws, sym.clone()).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Spawn a REST polling task for 24h stats (last_price, price_change%, high, low, volume).
            // Gemini's WS l2_updates channel only carries book deltas and rare trade events;
            // the 24h ticker fields are only available via the REST ticker endpoint.
            {
                let tx_rest = tx.clone();
                let rest_sym = sym.clone();
                let rest_original_symbol = original_symbol.clone();
                tokio::spawn(async move {
                    let connector = match GeminiConnector::public(false).await {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!("[gemini rest_poll] connector create error: {}", e);
                            return;
                        }
                    };
                    loop {
                        match connector.get_ticker(rest_sym.clone(), AccountType::Spot).await {
                            Ok(ticker) => {
                                let _ = tx_rest.send(LiveUpdate::MiniTickerUpdate {
                                    exchange_id,
                                    symbol: rest_original_symbol.clone(),
                                    last_price: ticker.last_price,
                                    price_change_percent: ticker.price_change_percent_24h,
                                    high_price: ticker.high_24h,
                                    low_price: ticker.low_24h,
                                    volume: ticker.volume_24h,
                                });
                            }
                            Err(e) => {
                                eprintln!("[gemini rest_poll] get_ticker error: {}", e);
                            }
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                    }
                });
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = WebSocketConnector::ping_rtt_handle(&ws) {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            WebSocketConnector::event_stream(&ws)
        }
        ExchangeId::CryptoCom => {
            let mut ws = CryptoComWebSocket::new(None, false);
            if let Err(e) = WebSocketConnector::connect(&mut ws, AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = WebSocketExt::subscribe_ticker(&mut ws, sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = WebSocketConnector::ping_rtt_handle(&ws) {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            WebSocketConnector::event_stream(&ws)
        }
        ExchangeId::Lighter => {
            let mut ws = match LighterWebSocket::public(false).await {
                Ok(ws) => ws,
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        message: format!("WS create failed: {}", e),
                    });
                    return;
                }
            };
            if let Err(e) = WebSocketConnector::connect(&mut ws, AccountType::Spot).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS connect failed: {}", e),
                });
                return;
            }
            if let Err(e) = WebSocketExt::subscribe_ticker(&mut ws, sym).await {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("WS subscribe failed: {}", e),
                });
                return;
            }
            // Capture RTT handle if the connector provides one.
            if let Some(rtt_handle) = WebSocketConnector::ping_rtt_handle(&ws) {
                if let Ok(mut handles) = ws_rtt_handles.lock() {
                    handles.insert(exchange_id, rtt_handle);
                }
            }
            WebSocketConnector::event_stream(&ws)
        }

        // Exchanges without WS support or with complex constructors
        other => {
            let _ = tx.send(LiveUpdate::Error {
                exchange_id: other,
                message: format!("WebSocket ticker subscription not supported for {:?}", other),
            });
            return;
        }
    };

    // Generic event loop — same for all exchanges
    loop {
        let result = match tokio::time::timeout(
            std::time::Duration::from_secs(60),
            stream.next(),
        )
        .await
        {
            Ok(Some(result)) => result,
            Ok(None) => break, // stream ended normally
            Err(_) => {
                // No data for 60s — assume the connection is silently dead.
                eprintln!("[WS ticker] No data for 60s on {:?}, reconnecting", exchange_id);
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: "WS timeout — no data for 60s".to_string(),
                });
                break;
            }
        };
        match result {
            Ok(StreamEvent::Ticker(ticker)) => {
                let _ = tx.send(LiveUpdate::MiniTickerUpdate {
                    exchange_id,
                    symbol: original_symbol.clone(),
                    last_price: ticker.last_price,
                    // Pass None through directly: BBO-only events (e.g. KuCoin
                    // `trade.ticker`) have None here and must not overwrite the
                    // 24h stats that a prior snapshot event wrote into the cache.
                    price_change_percent: ticker.price_change_percent_24h,
                    high_price: ticker.high_24h,
                    low_price: ticker.low_24h,
                    volume: ticker.volume_24h,
                });
            }
            Ok(StreamEvent::OrderbookSnapshot(ob)) => {
                // Exchanges like Bitstamp emit orderbook updates instead of ticker.
                // Use mid-price (avg of best bid and best ask) as synthetic last_price.
                let bid = ob.bids.first().map(|(p, _)| *p).unwrap_or(0.0);
                let ask = ob.asks.first().map(|(p, _)| *p).unwrap_or(0.0);
                let mid = if bid > 0.0 && ask > 0.0 { (bid + ask) / 2.0 } else { bid.max(ask) };
                if mid > 0.0 {
                    let _ = tx.send(LiveUpdate::MiniTickerUpdate {
                        exchange_id,
                        symbol: original_symbol.clone(),
                        last_price: mid,
                        price_change_percent: None,
                        high_price: None,
                        low_price: None,
                        volume: None,
                    });
                }
            }
            Ok(StreamEvent::OrderbookDelta { bids, asks, .. }) => {
                // Gemini subscribe_ticker maps to the l2 channel which emits OrderbookDelta
                // when there are only book changes (no trades). Use mid-price as synthetic price.
                let best_bid = bids.iter().map(|(p, _)| *p).filter(|p| *p > 0.0).fold(0.0f64, f64::max);
                let best_ask = asks.iter().map(|(p, _)| *p).filter(|p| *p > 0.0).fold(f64::MAX, f64::min);
                let mid = if best_bid > 0.0 && best_ask < f64::MAX {
                    (best_bid + best_ask) / 2.0
                } else {
                    best_bid.max(if best_ask < f64::MAX { best_ask } else { 0.0 })
                };
                if mid > 0.0 {
                    let _ = tx.send(LiveUpdate::MiniTickerUpdate {
                        exchange_id,
                        symbol: original_symbol.clone(),
                        last_price: mid,
                        price_change_percent: None,
                        high_price: None,
                        low_price: None,
                        volume: None,
                    });
                }
            }
            Ok(StreamEvent::Trade(trade)) => {
                // Gemini l2_updates with executed trades: use trade price as last price.
                let _ = tx.send(LiveUpdate::MiniTickerUpdate {
                    exchange_id,
                    symbol: original_symbol.clone(),
                    last_price: trade.price,
                    price_change_percent: None,
                    high_price: None,
                    low_price: None,
                    volume: None,
                });
            }
            Ok(_) => {}
            Err(e) => {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    message: format!("Ticker WS error: {}", e),
                });
                break;
            }
        }
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
fn parse_symbol_for_exchange(exchange_id: ExchangeId, s: &str) -> Symbol {
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
