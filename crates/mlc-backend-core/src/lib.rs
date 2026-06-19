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
use mlc_core::tag_manager::BubbleId;
use mlc_core::Bar;
use mlc_indicators::IndicatorManager;
use mlc_live_data::{DataBridge, LiveUpdate, LiveUpdateReceiver, OrderbookSource, SymbolInfo};
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

/// Events emitted by [`BackendCore::tick_writers`] for each logical update
/// received from the live-data broadcast channel.
///
/// **Backend-write events** (BarsLoaded through LiquidationBucketPushed) signal
/// that a write into the shared bubble store / caches has already been
/// completed inside `tick_writers`. Per-app reactors use the event payload to
/// run viewport snap, `pending_symbol_load` clear, indicator recalc, etc.
///
/// **Pass-through events** (arms whose write target depends on per-app window
/// flags such as `oi_show`, `funding_show`, `chart_type`) carry the raw data
/// and are handled entirely by the per-app reactor.
///
/// Mode-2 daemon calls [`BackendCore::tick_bridge_only`] which drops all events.
#[derive(Debug, Clone)]
pub enum BubbleEvent {
    /// Historical bars written into one bubble (BarsLoaded arm).
    BarsLoaded {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
        timeframe: String,
        bars: Vec<Bar>,
    },
    /// Background backfill bars written into one bubble.
    BackfillCompleted {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
        timeframe: String,
        bars: Vec<Bar>,
    },
    /// Scroll-fetch bars (left-extend) written into one bubble.
    ScrollBarsAppended {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
        timeframe: String,
        bars: Vec<Bar>,
        prepend_count: usize,
    },
    /// BarUpdate pass-through — the per-app reactor owns this write because
    /// the target bubbles depend on per-window `is_derived` / `chart_type` flags
    /// that are not visible from BackendCore.
    BarUpdatePassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        bar: mlc_core::Bar,
    },
    /// TradeUpdate pass-through — the per-app reactor owns the OHLCV aggregation
    /// because the aggregation period and derived-window exclusion depend on
    /// per-window TF metadata and `is_derived` flags not available in BackendCore.
    TradeUpdatePassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        price: f64,
        quantity: f64,
        timestamp: i64,
        is_buyer_maker: bool,
    },
    /// Mini-ticker cache entry written/updated.
    MiniTickerUpdated {
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
        last_price: f64,
    },
    /// Connector became ready (connection established).
    ConnectorReady {
        exchange_id: ExchangeId,
    },
    /// Exchange symbol list written into the shared cache.
    SymbolsLoaded {
        exchange_id: ExchangeId,
        count: usize,
    },
    /// A data-fetch error was reported for an instrument.
    ErrorReported {
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
        timeframe: String,
        message: String,
    },
    /// REST orderbook snapshot arrived (DOM needs full rebuild).
    OrderbookSnapshotRest {
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
    },
    /// WS orderbook delta arrived (L2 tape + DOM incremental update).
    OrderbookDelta {
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
        bids: Vec<(f64, f64)>,
        asks: Vec<(f64, f64)>,
        ts_ms: i64,
    },
    /// Multi-DOM fan-out: delta was routed into a multi-dom panel state.
    /// Carried per-event so the reactor doesn't need to re-route.
    OrderbookDeltaForMultiDom {
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
        bids: Vec<(f64, f64)>,
        asks: Vec<(f64, f64)>,
    },
    /// Private stream updates (order/balance/position) — passed through for
    /// the per-app trading manager tick.
    TradingUpdate { update: LiveUpdate },
    /// Footprint bar upserted into one bubble.
    FootprintBarAppended {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
    },
    /// Renko brick appended to one bubble.
    RenkoBarAppended {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
    },
    /// PnF column upserted in one bubble.
    PnfBarAppended {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
    },
    /// Kagi segment appended to one bubble.
    KagiBarAppended {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
    },
    /// CVD scalar sample appended to one bubble.
    CvdSampleAppended {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
    },
    /// TPO session upserted in one bubble.
    TpoSessionUpdated {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
    },
    /// Three-line-break line appended to one bubble.
    TlbLineAppended {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
    },
    /// Open interest update — write target depends on per-app `oi_show` flag.
    /// Pass-through: reactor finds matching window bubbles and does the write.
    OiUpdatePassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        point: mlc_core::types::OiDeltaBar,
    },
    /// Funding rate update — write target depends on per-app `funding_show` flag.
    /// Pass-through: reactor finds matching window bubbles and does the write.
    FundingRatePassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        point: mlc_core::types::FundingRateCell,
    },
    /// DOM heatmap snapshot — write target depends on per-app `chart_type=="dom_heatmap"`.
    /// Pass-through: reactor finds matching window bubbles and does the write.
    DomHeatmapSnapshotPassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        ts_ms: i64,
        bids: Vec<(f64, f64)>,
        asks: Vec<(f64, f64)>,
    },
    /// DOM trade dot — write target depends on per-app `chart_type=="dom_heatmap"`.
    /// Pass-through: reactor finds matching window bubbles and does the write.
    DomTradeDotPassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        dot: mlc_core::chart::dom_heatmap::DomTradeDot,
    },
    /// Liquidation per-event — write target depends on `chart_type=="liquidation_heatmap"`.
    /// Pass-through.
    LiquidationEventPassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        ts_ms: i64,
        price: f64,
        value_usd: f64,
        side: u8,
    },
    /// Liquidation bucket update — write target depends on `chart_type=="liquidation_heatmap"`.
    /// Pass-through.
    LiquidationBucketPassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        ts_ms: i64,
        long_liq_usd: f64,
        short_liq_usd: f64,
    },
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

    /// Backend's own broadcast receiver. Drained by `tick_writers` each tick
    /// to write incoming data into the shared bubble store / caches and emit
    /// `BubbleEvent`s for per-app reactors.
    ///
    /// `Mutex` because `try_recv` requires `&mut Receiver` and `tick_writers`
    /// takes `&self` (so multiple callers can coexist without `&mut BackendCore`).
    broadcast_rx: std::sync::Mutex<LiveUpdateReceiver>,

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

    /// Registry of attached `AppSession`s. One per consumer of this backend.
    ///
    /// In Mode 1 (embedded `ChartApp`) the host calls `register_session` once
    /// at construction and `deregister_session` on shutdown. In Mode 2 the
    /// daemon registers a session per external HTTP client.
    ///
    /// Sessions are pure metadata for now — `(app_id, label, created_at_secs,
    /// last_heartbeat_secs)`. The reactor logic that runs per session lives
    /// on the per-app side (`ChartApp::react_to_bubble_event`); session id
    /// is the handle the agent server uses to scope queries.
    pub sessions: std::sync::RwLock<SessionRegistry>,
}

/// Registry of attached `AppSession`s. Shared across the agent server's
/// `/v1/apps` endpoints and the daemon's `/v1/apps` endpoints — both end up
/// pointing at this one registry on the per-process `BackendCore`.
#[derive(Default)]
pub struct SessionRegistry {
    pub by_id: HashMap<String, AppSession>,
}

/// One attached consumer of the backend. Pure metadata. The render thread,
/// reactor, and persist layer all run regardless of how many sessions are
/// registered; sessions exist so external observers can correlate their own
/// client state with backend events.
#[derive(Clone, Debug)]
pub struct AppSession {
    pub app_id: String,
    pub label: Option<String>,
    pub created_at_secs: u64,
    pub last_heartbeat_secs: u64,
}

impl BackendCore {
    /// Construct a fresh backend wrapping an already-spawned `DataBridge`.
    pub fn new(bridge: Arc<DataBridge>, trading: Option<TradingManager>) -> Arc<Self> {
        let broadcast_rx = bridge.add_listener();
        Arc::new(Self {
            managers: std::sync::RwLock::new(Managers::new()),
            indicators: std::sync::RwLock::new(IndicatorManager::new()),
            broadcast_rx: std::sync::Mutex::new(broadcast_rx),
            bridge,
            mini_ticker: DashMap::new(),
            exchange_symbols: std::sync::RwLock::new(HashMap::new()),
            trading: trading.map(std::sync::Mutex::new),
            presets: std::sync::RwLock::new(HashMap::new()),
            template_manager: std::sync::RwLock::new(TemplateManager::new()),
            watchlist_manager: std::sync::RwLock::new(WatchlistManager::default()),
            sessions: std::sync::RwLock::new(SessionRegistry::default()),
        })
    }

    /// Register a new AppSession. Returns the freshly minted `app_id`.
    pub fn register_session(&self, label: Option<String>, now_secs: u64) -> String {
        let app_id = format!("app-{:016x}", rand_u64());
        let session = AppSession {
            app_id: app_id.clone(),
            label,
            created_at_secs: now_secs,
            last_heartbeat_secs: now_secs,
        };
        self.sessions
            .write()
            .expect("BackendCore.sessions poisoned (write)")
            .by_id
            .insert(app_id.clone(), session);
        app_id
    }

    /// Drop a session by id. Returns true if it existed.
    pub fn deregister_session(&self, app_id: &str) -> bool {
        self.sessions
            .write()
            .expect("BackendCore.sessions poisoned (write)")
            .by_id
            .remove(app_id)
            .is_some()
    }

    /// Bump the heartbeat on a session. Returns true if the session existed.
    pub fn heartbeat_session(&self, app_id: &str, now_secs: u64) -> bool {
        let mut reg = self
            .sessions
            .write()
            .expect("BackendCore.sessions poisoned (write)");
        if let Some(s) = reg.by_id.get_mut(app_id) {
            s.last_heartbeat_secs = now_secs;
            true
        } else {
            false
        }
    }

    /// Drop sessions whose heartbeat is older than `idle_timeout_secs`.
    /// Returns the number reaped. Callers (Mode-2 daemon, embedded host idle
    /// timer) drive this periodically.
    pub fn reap_idle_sessions(&self, now_secs: u64, idle_timeout_secs: u64) -> usize {
        let mut reg = self
            .sessions
            .write()
            .expect("BackendCore.sessions poisoned (write)");
        let before = reg.by_id.len();
        reg.by_id
            .retain(|_, s| now_secs.saturating_sub(s.last_heartbeat_secs) < idle_timeout_secs);
        before - reg.by_id.len()
    }

    /// Snapshot of all currently-registered sessions.
    pub fn sessions_snapshot(&self) -> Vec<AppSession> {
        self.sessions
            .read()
            .expect("BackendCore.sessions poisoned (read)")
            .by_id
            .values()
            .cloned()
            .collect()
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

    /// Drain the live-data broadcast channel, write incoming updates into
    /// shared state (bubble store, `mini_ticker`, `exchange_symbols`), and
    /// emit one [`BubbleEvent`] per logical write for per-app reactors.
    ///
    /// Uses `bubbles_for_instrument` to locate all registered bubbles for an
    /// incoming instrument key — correct for N≥1 apps sharing one BackendCore.
    ///
    /// Returns events in the order they were processed. Mode-1 embedded apps
    /// pass them to [`ChartApp::react_to_bubble_event`]; Mode-2 daemon drops
    /// or forwards them over HTTP/WS.
    ///
    /// # Lagged messages
    ///
    /// When the broadcast channel signals `Lagged`, this method silently
    /// skips the gap (no backfill trigger — that is per-app logic). The caller
    /// can detect a lag by comparing event counts across ticks if needed.
    pub fn tick_writers(&self, _now_ms: u64) -> Vec<BubbleEvent> {
        let mut events: Vec<BubbleEvent> = Vec::new();
        let mut rx = self.broadcast_rx.lock().expect("BackendCore.broadcast_rx poisoned");

        loop {
            let update = match rx.try_recv() {
                Ok(u) => u,
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
                Err(_) => break,
            };

            match update {
                // ── BarsLoaded ─────────────────────────────────────────────────
                LiveUpdate::BarsLoaded { exchange_id, account_type, symbol, timeframe: tf_name, bars } => {
                    let bids_for_instrument = self.bubble()
                        .bubble
                        .bubbles_for_instrument(exchange_id.as_str(), account_type.short_label(), &symbol);
                    if bids_for_instrument.is_empty() {
                        // No registered bubble for this instrument yet; skip bubble write.
                        // Per-app reactors may still want to hear about this (e.g. to create
                        // a bubble and then write), but the write itself is a no-op.
                        events.push(BubbleEvent::BarsLoaded {
                            bid: BubbleId(0),
                            exchange_id,
                            symbol,
                            account_type: account_type.short_label().to_string(),
                            timeframe: tf_name,
                            bars,
                        });
                    } else {
                        for bid in bids_for_instrument {
                            {
                                let mut mgr = self.bubble_mut();
                                if let Some(bubble) = mgr.bubble.bubble_mut(bid) {
                                    bubble.bars = bars.clone();
                                }
                            }
                            events.push(BubbleEvent::BarsLoaded {
                                bid,
                                exchange_id,
                                symbol: symbol.clone(),
                                account_type: account_type.short_label().to_string(),
                                timeframe: tf_name.clone(),
                                bars: bars.clone(),
                            });
                        }
                    }
                }

                // ── BackfillComplete ────────────────────────────────────────────
                LiveUpdate::BackfillComplete { exchange_id, account_type, symbol, timeframe: tf_name, bars } => {
                    let bids = self.bubble()
                        .bubble
                        .bubbles_for_instrument(exchange_id.as_str(), account_type.short_label(), &symbol);
                    for bid in bids {
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble.bubble_mut(bid) {
                                bubble.bars = bars.clone();
                            }
                        }
                        events.push(BubbleEvent::BackfillCompleted {
                            bid,
                            exchange_id,
                            symbol: symbol.clone(),
                            account_type: account_type.short_label().to_string(),
                            timeframe: tf_name.clone(),
                            bars: bars.clone(),
                        });
                    }
                }

                // ── ScrollBarsLoaded ────────────────────────────────────────────
                LiveUpdate::ScrollBarsLoaded { exchange_id, account_type, symbol, timeframe: tf_name, bars, prepend_count } => {
                    let at_label = account_type.short_label().to_string();
                    let bids = self.bubble()
                        .bubble
                        .bubbles_for_instrument(exchange_id.as_str(), &at_label, &symbol);
                    for bid in bids {
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble.bubble_mut(bid) {
                                bubble.bars = bars.clone();
                            }
                        }
                        events.push(BubbleEvent::ScrollBarsAppended {
                            bid,
                            exchange_id,
                            symbol: symbol.clone(),
                            account_type: at_label.clone(),
                            timeframe: tf_name.clone(),
                            bars: bars.clone(),
                            prepend_count,
                        });
                    }
                }

                // ── BarUpdate — pass-through ────────────────────────────────────
                // Bubble write depends on per-window `is_derived` / `chart_type` flags;
                // reactor owns the full arm.
                LiveUpdate::BarUpdate { exchange_id, account_type: bar_at, symbol: bar_sym, bar } => {
                    events.push(BubbleEvent::BarUpdatePassthrough {
                        exchange_id,
                        account_type: bar_at.short_label().to_string(),
                        symbol: bar_sym,
                        bar,
                    });
                }

                // ── TradeUpdate — pass-through ──────────────────────────────────
                // Aggregation period and derived-window exclusion depend on per-window
                // TF metadata and `is_derived` flags; reactor owns the full arm.
                LiveUpdate::TradeUpdate { exchange_id, account_type, symbol, price, quantity, timestamp, is_buyer_maker } => {
                    events.push(BubbleEvent::TradeUpdatePassthrough {
                        exchange_id,
                        account_type: account_type.short_label().to_string(),
                        symbol,
                        price,
                        quantity,
                        timestamp,
                        is_buyer_maker,
                    });
                }

                // ── MiniTickerUpdate ────────────────────────────────────────────
                LiveUpdate::MiniTickerUpdate { exchange_id, account_type, symbol, last_price,
                    price_change_percent, high_price, low_price, volume } =>
                {
                    let cache_key = format!("{}:{}:{}", symbol, exchange_id.as_str(), account_type.short_label());
                    let mut entry = self.mini_ticker
                        .entry(cache_key)
                        .or_insert(MiniTickerData {
                            last_price,
                            price_change_percent: 0.0,
                            high_price: 0.0,
                            low_price: 0.0,
                            volume: 0.0,
                        });
                    entry.last_price = last_price;
                    if let Some(v) = price_change_percent { entry.price_change_percent = v; }
                    if let Some(v) = high_price           { entry.high_price = v; }
                    if let Some(v) = low_price            { entry.low_price  = v; }
                    if let Some(v) = volume               { entry.volume     = v; }
                    events.push(BubbleEvent::MiniTickerUpdated {
                        exchange_id,
                        symbol,
                        account_type: account_type.short_label().to_string(),
                        last_price,
                    });
                }

                // ── ConnectorReady ──────────────────────────────────────────────
                LiveUpdate::ConnectorReady { exchange_id } => {
                    events.push(BubbleEvent::ConnectorReady { exchange_id });
                }

                // ── SymbolsLoaded ───────────────────────────────────────────────
                LiveUpdate::SymbolsLoaded { exchange_id, symbols } => {
                    let count = symbols.len();
                    self.exchange_symbols_mut().insert(exchange_id, symbols);
                    self.bridge.prewarm_wire_symbol_cache(exchange_id);
                    events.push(BubbleEvent::SymbolsLoaded { exchange_id, count });
                }

                // ── Error ───────────────────────────────────────────────────────
                LiveUpdate::Error { exchange_id, account_type, symbol, timeframe, message } => {
                    events.push(BubbleEvent::ErrorReported {
                        exchange_id,
                        symbol,
                        account_type: account_type.short_label().to_string(),
                        timeframe,
                        message,
                    });
                }

                // ── OrderbookSnapshot ───────────────────────────────────────────
                LiveUpdate::OrderbookSnapshot { exchange_id, account_type, symbol, source, .. } => {
                    if matches!(source, OrderbookSource::Rest) {
                        events.push(BubbleEvent::OrderbookSnapshotRest {
                            exchange_id,
                            symbol,
                            account_type: account_type.short_label().to_string(),
                        });
                    }
                    // WS snapshot: tracker already updated in bridge; skip.
                }

                // ── OrderbookDelta ──────────────────────────────────────────────
                LiveUpdate::OrderbookDelta { exchange_id, account_type, symbol, bids, asks, timestamp } => {
                    let at_str = account_type.short_label().to_string();
                    // Emit one event for the L2-tape / DOM coalescer.
                    events.push(BubbleEvent::OrderbookDelta {
                        exchange_id,
                        symbol: symbol.clone(),
                        account_type: at_str.clone(),
                        bids: bids.clone(),
                        asks: asks.clone(),
                        ts_ms: timestamp,
                    });
                    // Separate event tag for multi-dom fan-out so reactor can
                    // distinguish without re-cloning the slice.
                    events.push(BubbleEvent::OrderbookDeltaForMultiDom {
                        exchange_id,
                        symbol,
                        account_type: at_str,
                        bids,
                        asks,
                    });
                }

                // ── ConnectorMetrics ────────────────────────────────────────────
                LiveUpdate::ConnectorMetrics { .. } => {
                    // Collected on-demand; no action.
                }

                // ── Trading (Order / Balance / Position) ─────────────────────────
                LiveUpdate::OrderUpdate { .. }
                | LiveUpdate::BalanceUpdate { .. }
                | LiveUpdate::PositionUpdate { .. } => {
                    events.push(BubbleEvent::TradingUpdate { update });
                }

                // ── FootprintUpdate ─────────────────────────────────────────────
                LiveUpdate::FootprintUpdate { exchange_id, account_type: fp_at, symbol: fp_sym, point } => {
                    let fp_at_label = fp_at.short_label().to_string();
                    let new_bar = mlc_core::FootprintBar {
                        open_time: point.open_time,
                        open: point.open,
                        high: point.high,
                        low: point.low,
                        close: point.close,
                        volume: point.volume,
                        levels: point.levels.iter()
                            .map(|(price, buy_vol, sell_vol)| mlc_core::FootprintLevel {
                                price: *price,
                                buy_vol: *buy_vol,
                                sell_vol: *sell_vol,
                                is_single_print: false,
                                is_unfinished: false,
                            })
                            .collect(),
                        cumulative_delta: 0.0,
                        stacked_ask_start: None,
                        stacked_ask_len: None,
                        stacked_bid_start: None,
                        stacked_bid_len: None,
                        is_absorption: false,
                    };
                    let tick_size = {
                        let mut step = f64::INFINITY;
                        for w in new_bar.levels.windows(2) {
                            let d = (w[1].price - w[0].price).abs();
                            if d > 0.0 && d < step { step = d; }
                        }
                        if step.is_finite() { step } else { 1.0 }
                    };
                    let mut new_bar_with_static = new_bar;
                    new_bar_with_static.compute_static_extras(3.0, 3, 0.15, 0.05, tick_size);
                    let ohlc_ts = new_bar_with_static.open_time / 1000;
                    let ohlc_bar = mlc_core::Bar::with_volume(
                        ohlc_ts,
                        new_bar_with_static.open,
                        new_bar_with_static.high,
                        new_bar_with_static.low,
                        new_bar_with_static.close,
                        new_bar_with_static.volume,
                    );
                    let bids = self.bubble()
                        .bubble
                        .bubbles_for_instrument(exchange_id.as_str(), &fp_at_label, fp_sym.as_str());
                    for bid in bids {
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble.bubble_mut(bid) {
                                let prev_cvd = {
                                    let store = bubble.footprint_bars.get_or_insert_with(Vec::new);
                                    match store.last() {
                                        Some(last) if last.open_time != new_bar_with_static.open_time => last.cumulative_delta,
                                        Some(_) if store.len() >= 2 => store[store.len() - 2].cumulative_delta,
                                        _ => 0.0,
                                    }
                                };
                                let store = bubble.footprint_bars.get_or_insert_with(Vec::new);
                                let mut bar = new_bar_with_static.clone();
                                bar.set_cumulative_delta(prev_cvd);
                                match store.last_mut() {
                                    Some(last) if last.open_time == bar.open_time => { *last = bar; }
                                    _ => store.push(bar),
                                }
                                let last_ohlc_ts = bubble.bars.last().map(|b| b.timestamp);
                                if last_ohlc_ts == Some(ohlc_ts) {
                                    if let Some(last) = bubble.last_bar_mut() {
                                        last.open   = ohlc_bar.open;
                                        last.high   = ohlc_bar.high;
                                        last.low    = ohlc_bar.low;
                                        last.close  = ohlc_bar.close;
                                        last.volume = ohlc_bar.volume;
                                    }
                                } else {
                                    bubble.append_bar(ohlc_bar.clone());
                                }
                            }
                        }
                        events.push(BubbleEvent::FootprintBarAppended {
                            bid,
                            exchange_id,
                            symbol: fp_sym.clone(),
                            account_type: fp_at_label.clone(),
                        });
                    }
                }

                // ── RenkoBarUpdate ──────────────────────────────────────────────
                LiveUpdate::RenkoBarUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    let at_label = at.short_label().to_string();
                    let brick = mlc_core::types::RenkoBrick {
                        open_time: point.open_time,
                        bottom: point.bottom,
                        top: point.top,
                        up: point.up,
                        volume: point.volume,
                        trades_count: point.trades_count,
                    };
                    let bids = self.bubble()
                        .bubble
                        .bubbles_for_instrument(exchange_id.as_str(), &at_label, sym.as_str());
                    for bid in bids {
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble.bubble_mut(bid) {
                                bubble.append_renko(brick.clone());
                            }
                        }
                        events.push(BubbleEvent::RenkoBarAppended {
                            bid,
                            exchange_id,
                            symbol: sym.clone(),
                            account_type: at_label.clone(),
                        });
                    }
                }

                // ── PnfBarUpdate ────────────────────────────────────────────────
                LiveUpdate::PnfBarUpdate { exchange_id, account_type: at, symbol: sym, point, box_size: bs } => {
                    let at_label = at.short_label().to_string();
                    let col = mlc_core::types::PnfColumn {
                        open_time: point.open_time,
                        column_id: point.column_id,
                        is_x: point.is_x,
                        bottom: point.bottom,
                        top: point.top,
                        volume: point.volume,
                        trades_count: point.trades_count,
                        box_size: bs,
                    };
                    let bids = self.bubble()
                        .bubble
                        .bubbles_for_instrument(exchange_id.as_str(), &at_label, sym.as_str());
                    for bid in bids {
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble.bubble_mut(bid) {
                                let store = bubble.pnf_columns.get_or_insert_with(Vec::new);
                                match store.last_mut() {
                                    Some(last) if last.column_id == col.column_id => *last = col.clone(),
                                    _ => store.push(col.clone()),
                                }
                            }
                        }
                        events.push(BubbleEvent::PnfBarAppended {
                            bid,
                            exchange_id,
                            symbol: sym.clone(),
                            account_type: at_label.clone(),
                        });
                    }
                }

                // ── KagiBarUpdate ───────────────────────────────────────────────
                LiveUpdate::KagiBarUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    let at_label = at.short_label().to_string();
                    let seg = mlc_core::types::KagiSegment {
                        open_time: point.open_time,
                        start_price: point.start_price,
                        end_price: point.end_price,
                        up: point.up,
                        yang: point.yang,
                        is_connector: point.is_connector,
                    };
                    let bids = self.bubble()
                        .bubble
                        .bubbles_for_instrument(exchange_id.as_str(), &at_label, sym.as_str());
                    for bid in bids {
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble.bubble_mut(bid) {
                                bubble.append_kagi(seg.clone());
                            }
                        }
                        events.push(BubbleEvent::KagiBarAppended {
                            bid,
                            exchange_id,
                            symbol: sym.clone(),
                            account_type: at_label.clone(),
                        });
                    }
                }

                // ── CvdLineUpdate ───────────────────────────────────────────────
                LiveUpdate::CvdLineUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    let at_label = at.short_label().to_string();
                    let sample = mlc_core::types::CvdSample {
                        ts_ms: point.ts_ms,
                        value: point.value,
                    };
                    let bids = self.bubble()
                        .bubble
                        .bubbles_for_instrument(exchange_id.as_str(), &at_label, sym.as_str());
                    for bid in bids {
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble.bubble_mut(bid) {
                                bubble.append_cvd(sample);
                            }
                        }
                        events.push(BubbleEvent::CvdSampleAppended {
                            bid,
                            exchange_id,
                            symbol: sym.clone(),
                            account_type: at_label.clone(),
                        });
                    }
                }

                // ── TpoProfileUpdate ────────────────────────────────────────────
                LiveUpdate::TpoProfileUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    let at_label = at.short_label().to_string();
                    let session = mlc_core::types::TpoSession {
                        open_time: point.open_time,
                        tick_size: point.tick_size,
                        session_high: point.session_high,
                        session_low: point.session_low,
                        poc_price: point.poc_price,
                        vah_price: point.vah_price,
                        val_price: point.val_price,
                        rows: point.rows.clone(),
                    };
                    let bids = self.bubble()
                        .bubble
                        .bubbles_for_instrument(exchange_id.as_str(), &at_label, sym.as_str());
                    for bid in bids {
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble.bubble_mut(bid) {
                                let store = bubble.tpo_sessions.get_or_insert_with(Vec::new);
                                match store.last_mut() {
                                    Some(last) if last.open_time == session.open_time => *last = session.clone(),
                                    _ => {
                                        store.push(session.clone());
                                        const MAX_TPO_SESSIONS: usize = 11;
                                        while store.len() > MAX_TPO_SESSIONS {
                                            store.remove(0);
                                        }
                                    }
                                }
                            }
                        }
                        events.push(BubbleEvent::TpoSessionUpdated {
                            bid,
                            exchange_id,
                            symbol: sym.clone(),
                            account_type: at_label.clone(),
                        });
                    }
                }

                // ── ThreeLineBreakUpdate ────────────────────────────────────────
                LiveUpdate::ThreeLineBreakUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    let at_label = at.short_label().to_string();
                    let line = mlc_core::types::ThreeLineBreakLine {
                        ts_open: point.ts_open,
                        ts_close: point.ts_close,
                        price_open: point.open,
                        price_close: point.close,
                        volume: point.volume,
                        direction: point.direction,
                    };
                    let bids = self.bubble()
                        .bubble
                        .bubbles_for_instrument(exchange_id.as_str(), &at_label, sym.as_str());
                    for bid in bids {
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble.bubble_mut(bid) {
                                bubble.append_three_line_break(line.clone());
                            }
                        }
                        events.push(BubbleEvent::TlbLineAppended {
                            bid,
                            exchange_id,
                            symbol: sym.clone(),
                            account_type: at_label.clone(),
                        });
                    }
                }

                // ── OpenInterestUpdate ──────────────────────────────────────────
                // Write target depends on per-app `oi_show` window flag — pass through.
                LiveUpdate::OpenInterestUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    events.push(BubbleEvent::OiUpdatePassthrough {
                        exchange_id,
                        account_type: at.short_label().to_string(),
                        symbol: sym,
                        point,
                    });
                }

                // ── FundingRateUpdate ───────────────────────────────────────────
                // Write target depends on per-app `funding_show` window flag — pass through.
                LiveUpdate::FundingRateUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    events.push(BubbleEvent::FundingRatePassthrough {
                        exchange_id,
                        account_type: at.short_label().to_string(),
                        symbol: sym,
                        point,
                    });
                }

                // ── DomHeatmapSnapshot — pass-through ───────────────────────────
                // Write target depends on per-app `chart_type=="dom_heatmap"` filter.
                LiveUpdate::DomHeatmapSnapshot { exchange_id, account_type: at, symbol: sym, ts_ms, bids, asks } => {
                    events.push(BubbleEvent::DomHeatmapSnapshotPassthrough {
                        exchange_id,
                        account_type: at.short_label().to_string(),
                        symbol: sym,
                        ts_ms,
                        bids,
                        asks,
                    });
                }

                // ── DomTradeDot ─────────────────────────────────────────────────
                // Write target depends on per-app `chart_type=="dom_heatmap"` — pass through.
                LiveUpdate::DomTradeDot { exchange_id, account_type: at, symbol: sym, dot } => {
                    events.push(BubbleEvent::DomTradeDotPassthrough {
                        exchange_id,
                        account_type: at.short_label().to_string(),
                        symbol: sym,
                        dot,
                    });
                }

                // ── LiquidationEvent — pass-through ─────────────────────────────
                // Write target depends on per-app `chart_type=="liquidation_heatmap"`.
                LiveUpdate::LiquidationEvent { exchange_id, account_type: at, symbol: sym, ts_ms, price, value_usd, side } => {
                    events.push(BubbleEvent::LiquidationEventPassthrough {
                        exchange_id,
                        account_type: at.short_label().to_string(),
                        symbol: sym,
                        ts_ms,
                        price,
                        value_usd,
                        side,
                    });
                }

                // ── LiquidationBucketUpdate — pass-through ───────────────────────
                // Write target depends on per-app `chart_type=="liquidation_heatmap"`.
                LiveUpdate::LiquidationBucketUpdate { exchange_id, account_type: at, symbol: sym, ts_ms, long_liq_usd, short_liq_usd, .. } => {
                    events.push(BubbleEvent::LiquidationBucketPassthrough {
                        exchange_id,
                        account_type: at.short_label().to_string(),
                        symbol: sym,
                        ts_ms,
                        long_liq_usd,
                        short_liq_usd,
                    });
                }
            }
        }

        events
    }

    /// Mode-2 daemon entry point: drive the writers, discard per-app events.
    ///
    /// Equivalent to `let _ = self.tick_writers(now_ms);` but more explicit
    /// about intent — the daemon has no `ChartApp` to consume events.
    pub fn tick_bridge_only(&self, now_ms: u64) {
        let _ = self.tick_writers(now_ms);
    }
}

/// xorshift64 over a thread-local seed; good enough for app-id uniqueness
/// in-process. Not a security-grade RNG; never use for crypto.
fn rand_u64() -> u64 {
    use std::cell::Cell;
    use std::time::{SystemTime, UNIX_EPOCH};
    thread_local! {
        static STATE: Cell<u64> = Cell::new(0);
    }
    STATE.with(|s| {
        let mut x = s.get();
        if x == 0 {
            // Lazy seed from wall clock + thread id hash.
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0xdead_beef);
            x = now ^ 0x9E37_79B9_7F4A_7C15;
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        x
    })
}
