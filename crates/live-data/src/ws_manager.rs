//! WS multiplexing actor system.
//!
//! One actor per `(ExchangeId, WsStreamType)` pair.  All symbols for the same
//! exchange+stream share a single WebSocket connection.  Reference-counted
//! subscribe/unsubscribe with a 30-second grace period before the actual
//! unsubscribe is sent, so fast symbol-switch patterns don't thrash the
//! connection.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

use digdigdig3::{
    AccountType, ExchangeId, StreamEvent, SubscriptionRequest, Symbol, StreamType,
    WebSocketConnector,
};
use digdigdig3::l3::open::crypto::cex::binance::BinanceWebSocket;
use digdigdig3::l3::open::crypto::cex::bybit::BybitWebSocket;
use digdigdig3::l3::open::crypto::cex::okx::OkxWebSocket;
use digdigdig3::l3::open::crypto::cex::kucoin::KuCoinWebSocket;
use digdigdig3::l3::open::crypto::cex::kraken::KrakenWebSocket;
use digdigdig3::l3::open::crypto::cex::coinbase::CoinbaseWebSocket;
use digdigdig3::l3::open::crypto::cex::gateio::GateioWebSocket;
use digdigdig3::l3::open::crypto::cex::bitfinex::BitfinexWebSocket;
use digdigdig3::l3::open::crypto::cex::bitstamp::BitstampWebSocket;
use digdigdig3::l3::open::crypto::cex::mexc::MexcWebSocket;
use digdigdig3::l3::open::crypto::cex::htx::HtxWebSocket;
use digdigdig3::l3::open::crypto::cex::bitget::BitgetWebSocket;
use digdigdig3::l3::open::crypto::cex::bingx::BingxWebSocket;
use digdigdig3::l3::open::crypto::cex::upbit::UpbitWebSocket;
use digdigdig3::l3::open::crypto::cex::deribit::DeribitWebSocket;
use digdigdig3::l3::open::crypto::cex::hyperliquid::HyperliquidWebSocket;
use digdigdig3::l3::open::crypto::dex::dydx::DydxWebSocket;
use digdigdig3::l2::free::moex::MoexWebSocket;
use digdigdig3::l3::open::crypto::cex::gemini::GeminiWebSocket;
use digdigdig3::l3::open::crypto::cex::crypto_com::CryptoComWebSocket;
use digdigdig3::l3::open::crypto::dex::lighter::LighterWebSocket;

use crate::bridge::LiveUpdate;
use orderbook_service::{SharedOrderbookMap, OrderbookKey};

// ─────────────────────────────────────────────────────────────────────────────
// PUBLIC TYPES
// ─────────────────────────────────────────────────────────────────────────────

/// Identifies a single multiplexed WS connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct WsKey {
    pub exchange_id: ExchangeId,
    pub stream_type: WsStreamType,
    pub account_type: AccountType,
}

/// The type of data stream multiplexed over a WS connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum WsStreamType {
    Trades,
    Ticker,
    /// Partial-snapshot stream (or unified snapshot+delta stream like Bybit's
    /// `orderbook.50.SYMBOL`).  Works for all exchanges.
    Depth,
    /// Full diff/delta stream (Binance `@depth@100ms`).  Only exchanges that
    /// expose a separate incremental-diff stream will produce events here;
    /// others will silently fail to connect or return an unsupported-operation
    /// error, which the actor logs and then exits gracefully.
    DepthDiff,
    /// Private authenticated stream: order updates, balance changes, position changes.
    Private,
}

/// Command sent to a running WS actor.
pub(crate) enum WsCmd {
    AddSymbol { symbol: String },
    RemoveSymbol { symbol: String },
    Shutdown,
}

pub(crate) struct WsActorHandle {
    pub cmd_tx: mpsc::Sender<WsCmd>,
    pub task: JoinHandle<()>,
}

/// Map of running WS actors, one per `WsKey`.
pub(crate) struct WsActorMap {
    pub actors: HashMap<WsKey, WsActorHandle>,
}

impl WsActorMap {
    pub fn new() -> Self {
        Self { actors: HashMap::new() }
    }

    /// Return the command sender for an existing live actor, or spawn a new one.
    pub fn get_or_spawn(
        &mut self,
        key: WsKey,
        tx: broadcast::Sender<LiveUpdate>,
        ws_rtt_handles: Arc<Mutex<HashMap<ExchangeId, Arc<tokio::sync::Mutex<u64>>>>>,
        rt: &tokio::runtime::Handle,
        credentials: Option<digdigdig3::Credentials>,
        orderbook_map: Option<SharedOrderbookMap>,
    ) -> mpsc::Sender<WsCmd> {
        if let Some(handle) = self.actors.get(&key) {
            if !handle.task.is_finished() {
                return handle.cmd_tx.clone();
            }
            self.actors.remove(&key);
        }
        let (cmd_tx, cmd_rx) = mpsc::channel::<WsCmd>(64);
        let task = rt.spawn(run_ws_actor(key, cmd_rx, tx, ws_rtt_handles, credentials, orderbook_map));
        self.actors.insert(key, WsActorHandle { cmd_tx: cmd_tx.clone(), task });
        cmd_tx
    }

    /// Send a command to an actor if it exists.
    pub fn send_cmd(&self, key: &WsKey, cmd: WsCmd) {
        if let Some(handle) = self.actors.get(key) {
            let _ = handle.cmd_tx.try_send(cmd);
        }
    }

    /// Shutdown and remove an actor.
    pub fn remove(&mut self, key: &WsKey) {
        if let Some(handle) = self.actors.remove(key) {
            let _ = handle.cmd_tx.try_send(WsCmd::Shutdown);
        }
    }

    /// Count live actors for a given exchange.
    pub fn active_count_for_exchange(&self, exchange_id: ExchangeId) -> usize {
        self.actors
            .iter()
            .filter(|(k, h)| k.exchange_id == exchange_id && !h.task.is_finished())
            .count()
    }

    /// Count all live actors across all exchanges.
    pub fn total_active_count(&self) -> usize {
        self.actors.values().filter(|h| !h.task.is_finished()).count()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ACTOR STATE
// ─────────────────────────────────────────────────────────────────────────────

struct WsActorState {
    exchange_id: ExchangeId,
    stream_type: WsStreamType,
    account_type: AccountType,
    /// Reference counts per symbol — incremented on AddSymbol, decremented on RemoveSymbol.
    refcounts: HashMap<String, u32>,
    /// Symbols with count == 0 waiting for the 30-second grace period before
    /// the actual unsubscribe is sent to the exchange.
    deferred_unsub: HashMap<String, tokio::time::Instant>,
}

// ─────────────────────────────────────────────────────────────────────────────
// ACTOR MAIN LOOP
// ─────────────────────────────────────────────────────────────────────────────

async fn run_ws_actor(
    key: WsKey,
    mut cmd_rx: mpsc::Receiver<WsCmd>,
    tx: broadcast::Sender<LiveUpdate>,
    ws_rtt_handles: Arc<Mutex<HashMap<ExchangeId, Arc<tokio::sync::Mutex<u64>>>>>,
    credentials: Option<digdigdig3::Credentials>,
    orderbook_map: Option<SharedOrderbookMap>,
) {
    // Private actors use a simplified loop that connects immediately and
    // subscribes to all three private streams without any per-symbol logic.
    if key.stream_type == WsStreamType::Private {
        run_private_ws_actor(key, cmd_rx, tx, ws_rtt_handles, credentials).await;
        return;
    }

    let mut state = WsActorState {
        exchange_id: key.exchange_id,
        stream_type: key.stream_type,
        account_type: key.account_type,
        refcounts: HashMap::new(),
        deferred_unsub: HashMap::new(),
    };
    let mut retry_count: u32 = 0;

    // Local depth book — for `WsStreamType::Depth` and `WsStreamType::DepthDiff`
    // connections.  Seeded by the first WS snapshot, updated by WS deltas.
    use crate::depth_book::{DepthBook, EMIT_LEVELS};
    let is_depth = key.stream_type == WsStreamType::Depth || key.stream_type == WsStreamType::DepthDiff;
    let mut depth_book: Option<DepthBook> = if is_depth { Some(DepthBook::new()) } else { None };

    'outer: loop {
        // Exponential backoff before reconnect attempts.
        if retry_count > 0 {
            let base_secs = std::cmp::min(1u64 << std::cmp::min(retry_count - 1, 5), 30);
            let jitter_ms = (retry_count as u64 * 137) % 500;
            tokio::time::sleep(std::time::Duration::from_millis(
                base_secs * 1000 + jitter_ms,
            ))
            .await;
        }
        retry_count += 1;

        // Reset depth book on reconnect — wait for a fresh WS snapshot.
        if let Some(ref mut db) = depth_book {
            if db.is_seeded() {
                eprintln!("[WsActor] {:?} depth book reset on reconnect", key.exchange_id);
            }
            *db = DepthBook::new();
        }

        // Build a new WS connection.
        let ws: Box<dyn WebSocketConnector> =
            match build_ws(key.exchange_id, &ws_rtt_handles, None).await {
                Ok(ws) => ws,
                Err(e) => {
                    eprintln!(
                        "[WsActor] {:?}/{:?} connect failed: {}",
                        key.exchange_id, key.stream_type, e
                    );
                    continue 'outer;
                }
            };

        // Grab the event stream BEFORE re-subscribing so we don't lose
        // any messages that arrive between subscribe() and event_stream().
        let mut event_stream = ws.event_stream();

        // Re-subscribe all symbols that were active before this reconnect.
        let ob_caps = ws.orderbook_capabilities(key.account_type);
        for symbol in state.refcounts.keys().cloned().collect::<Vec<_>>() {
            let depth = ob_caps.clamp_depth(Some(50));
            let req = make_sub_request(key.stream_type, key.exchange_id, &symbol, key.account_type, depth);
            if let Err(e) = ws.subscribe(req).await {
                eprintln!("[WsActor] re-subscribe {} failed: {}", symbol, e);
            }
        }

        // Inner event loop — exits on stream error/close; 'outer then reconnects.
        loop {
            // Calculate how long until the next deferred-unsub deadline.
            let next_defer = state.deferred_unsub.values().copied().min();
            let timeout_dur = match next_defer {
                Some(deadline) => {
                    let now = tokio::time::Instant::now();
                    if deadline <= now {
                        std::time::Duration::ZERO
                    } else {
                        deadline - now
                    }
                }
                None => std::time::Duration::from_secs(60),
            };

            tokio::select! {
                biased;

                cmd = cmd_rx.recv() => {
                    match cmd {
                        None | Some(WsCmd::Shutdown) => break 'outer,

                        Some(WsCmd::AddSymbol { symbol }) => {
                            // Cancel any pending deferred unsub for this symbol.
                            let already_subbed = state.deferred_unsub.remove(&symbol).is_some();
                            let count = state.refcounts.entry(symbol.clone()).or_insert(0);
                            *count += 1;
                            // Subscribe only if this is the first reference and was not
                            // simply re-activated from the deferred-unsub queue.
                            if *count == 1 && !already_subbed {
                                let depth = ws.orderbook_capabilities(key.account_type).clamp_depth(Some(50));
                                let req = make_sub_request(key.stream_type, key.exchange_id, &symbol, key.account_type, depth);
                                if let Err(e) = ws.subscribe(req).await {
                                    eprintln!("[WsActor] subscribe {} failed: {}", symbol, e);
                                }
                            }
                        }

                        Some(WsCmd::RemoveSymbol { symbol }) => {
                            if let Some(count) = state.refcounts.get_mut(&symbol) {
                                if *count > 0 {
                                    *count -= 1;
                                }
                                if *count == 0 {
                                    let deadline = tokio::time::Instant::now()
                                        + std::time::Duration::from_secs(30);
                                    state.deferred_unsub.insert(symbol, deadline);
                                }
                            }
                        }
                    }
                }

                _ = tokio::time::sleep(timeout_dur) => {
                    // Process any deferred unsubscribes whose grace period has expired.
                    let now = tokio::time::Instant::now();
                    let expired: Vec<String> = state
                        .deferred_unsub
                        .iter()
                        .filter(|(_, &d)| d <= now)
                        .map(|(s, _)| s.clone())
                        .collect();
                    for symbol in expired {
                        state.deferred_unsub.remove(&symbol);
                        if state.refcounts.get(&symbol).copied().unwrap_or(0) == 0 {
                            state.refcounts.remove(&symbol);
                            let depth = ws.orderbook_capabilities(key.account_type).clamp_depth(Some(50));
                            let req = make_sub_request(key.stream_type, key.exchange_id, &symbol, key.account_type, depth);
                            let _ = ws.unsubscribe(req).await;
                            eprintln!(
                                "[WsActor] {:?} unsubscribed {} after 30s grace",
                                key.exchange_id, symbol
                            );
                        }
                    }
                }

                result = tokio::time::timeout(
                    std::time::Duration::from_secs(60),
                    event_stream.next(),
                ) => {
                    match result {
                        Ok(Some(Ok(event))) => {
                            retry_count = 0; // Reset backoff on successful data.

                            // Intercept depth events — maintain local book from WS data.
                            if let Some(ref mut db) = depth_book {
                                match &event {
                                    StreamEvent::OrderbookSnapshot { book: ob, .. } => {
                                        let (emitted, synthetic_delta) = db.feed_snapshot(ob, EMIT_LEVELS);
                                        let maybe_sym = state
                                            .refcounts
                                            .iter()
                                            .find(|(_, &c)| c > 0)
                                            .map(|(s, _)| s.clone());
                                        if let Some(sym) = maybe_sym {
                                            // Write to SharedOrderbookMap — WS snapshot replaces in covered range.
                                            if let Some(ref ob_map) = orderbook_map {
                                                let ob_key = OrderbookKey::new(state.exchange_id, state.account_type, &sym);
                                                if let Some(handle) = ob_map.read().ok().and_then(|m| m.get(&ob_key).cloned()) {
                                                    if let Ok(mut series) = handle.write() {
                                                        // Clone slices for the map write; originals moved to broadcast below.
                                                        let bids_ref: Vec<(f64, f64)> = emitted.bids.clone();
                                                        let asks_ref: Vec<(f64, f64)> = emitted.asks.clone();
                                                        series.apply_ws_snapshot(&bids_ref, &asks_ref, emitted.timestamp);
                                                    }
                                                }
                                            }
                                            let _ = tx.send(LiveUpdate::OrderbookSnapshot {
                                                exchange_id: state.exchange_id,
                                                account_type: state.account_type,
                                                symbol: sym.clone(),
                                                bids: emitted.bids,
                                                asks: emitted.asks,
                                                timestamp: emitted.timestamp,
                                                source: crate::bridge::OrderbookSource::Ws,
                                            });
                                            // For exchanges that deliver full snapshots instead of
                                            // incremental deltas, synthesise a delta so that
                                            // consumers tracking incremental changes (L2 Tape, etc.)
                                            // receive the same event shape regardless of exchange.
                                            if let Some(delta) = synthetic_delta {
                                                if !delta.bids.is_empty() || !delta.asks.is_empty() {
                                                    let _ = tx.send(LiveUpdate::OrderbookDelta {
                                                        exchange_id: state.exchange_id,
                                                        account_type: state.account_type,
                                                        symbol: sym,
                                                        bids: delta.bids,
                                                        asks: delta.asks,
                                                        timestamp: ob.timestamp,
                                                    });
                                                }
                                            }
                                        }
                                        continue;
                                    }
                                    StreamEvent::OrderbookDelta { delta, .. } => {
                                        if let Some(emitted) = db.feed_delta(delta, EMIT_LEVELS) {
                                            let maybe_sym = state
                                                .refcounts
                                                .iter()
                                                .find(|(_, &c)| c > 0)
                                                .map(|(s, _)| s.clone());
                                            if let Some(sym) = maybe_sym {
                                                // Write delta to SharedOrderbookMap.
                                                if let Some(ref ob_map) = orderbook_map {
                                                    let ob_key = OrderbookKey::new(state.exchange_id, state.account_type, &sym);
                                                    if let Some(handle) = ob_map.read().ok().and_then(|m| m.get(&ob_key).cloned()) {
                                                        if let Ok(mut series) = handle.write() {
                                                            let bid_changes: Vec<(f64, f64)> = delta.bids.iter().map(|l| (l.price, l.size)).collect();
                                                            let ask_changes: Vec<(f64, f64)> = delta.asks.iter().map(|l| (l.price, l.size)).collect();
                                                            series.apply_ws_delta(&bid_changes, &ask_changes, delta.timestamp);
                                                        }
                                                    }
                                                }
                                                // Emit reconstructed book for DOM rendering.
                                                let _ = tx.send(LiveUpdate::OrderbookSnapshot {
                                                    exchange_id: state.exchange_id,
                                                    account_type: state.account_type,
                                                    symbol: sym.clone(),
                                                    bids: emitted.bids,
                                                    asks: emitted.asks,
                                                    timestamp: emitted.timestamp,
                                                    source: crate::bridge::OrderbookSource::Ws,
                                                });
                                                // Also forward raw delta for consumers that
                                                // track incremental changes (L2 Tape, etc.).
                                                let _ = tx.send(LiveUpdate::OrderbookDelta {
                                                    exchange_id: state.exchange_id,
                                                    account_type: state.account_type,
                                                    symbol: sym,
                                                    bids: delta.bids.iter().map(|l| (l.price, l.size)).collect(),
                                                    asks: delta.asks.iter().map(|l| (l.price, l.size)).collect(),
                                                    timestamp: delta.timestamp,
                                                });
                                            }
                                        }
                                        continue;
                                    }
                                    _ => {}
                                }
                            }

                            dispatch_event(&tx, &state, event, orderbook_map.as_ref());
                        }
                        Ok(Some(Err(e))) => {
                            eprintln!(
                                "[WsActor] {:?}/{:?} stream error: {}",
                                key.exchange_id, key.stream_type, e
                            );
                            break; // Trigger reconnect.
                        }
                        Ok(None) => break,   // Stream ended normally; reconnect.
                        Err(_) => {
                            eprintln!(
                                "[WsActor] {:?}/{:?} 60s silence, reconnecting",
                                key.exchange_id, key.stream_type
                            );
                            break; // Trigger reconnect.
                        }
                    }
                }
            }
        }

        // After inner-loop exit, signal a backfill so the chart can fill any gap
        // that opened while the connection was down.
        let _ = tx.send(LiveUpdate::ConnectorReady { exchange_id: key.exchange_id });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// PRIVATE STREAM ACTOR
// ─────────────────────────────────────────────────────────────────────────────

/// Dedicated actor for private (authenticated) WebSocket streams.
///
/// Unlike public stream actors, the private actor:
/// - Connects immediately without waiting for any `AddSymbol` command.
/// - Subscribes to all three private stream types (orders, balances, positions).
/// - Ignores `AddSymbol`/`RemoveSymbol` commands entirely.
/// - Only responds to `WsCmd::Shutdown`.
async fn run_private_ws_actor(
    key: WsKey,
    mut cmd_rx: mpsc::Receiver<WsCmd>,
    tx: broadcast::Sender<LiveUpdate>,
    ws_rtt_handles: Arc<Mutex<HashMap<ExchangeId, Arc<tokio::sync::Mutex<u64>>>>>,
    credentials: Option<digdigdig3::Credentials>,
) {
    let state = WsActorState {
        exchange_id: key.exchange_id,
        stream_type: WsStreamType::Private,
        account_type: key.account_type,
        refcounts: HashMap::new(),
        deferred_unsub: HashMap::new(),
    };
    let mut retry_count: u32 = 0;

    'outer: loop {
        if retry_count > 0 {
            let base_secs = std::cmp::min(1u64 << std::cmp::min(retry_count - 1, 5), 30);
            let jitter_ms = (retry_count as u64 * 137) % 500;
            tokio::time::sleep(std::time::Duration::from_millis(
                base_secs * 1000 + jitter_ms,
            ))
            .await;
        }
        retry_count += 1;

        let ws: Box<dyn WebSocketConnector> =
            match build_ws(key.exchange_id, &ws_rtt_handles, credentials.clone()).await {
                Ok(ws) => ws,
                Err(e) => {
                    eprintln!(
                        "[WsActor] {:?}/Private connect failed: {}",
                        key.exchange_id, e
                    );
                    continue 'outer;
                }
            };

        let mut event_stream = ws.event_stream();

        // Subscribe to all three private stream types immediately.
        let private_streams = [
            StreamType::OrderUpdate,
            StreamType::BalanceUpdate,
            StreamType::PositionUpdate,
        ];
        for stream_type in private_streams {
            let req = SubscriptionRequest::new(Symbol::empty(), stream_type.clone());
            if let Err(e) = ws.subscribe(req).await {
                eprintln!(
                    "[WsActor] {:?}/Private subscribe {:?} failed: {}",
                    key.exchange_id, stream_type, e
                );
            }
        }

        loop {
            tokio::select! {
                biased;

                cmd = cmd_rx.recv() => {
                    match cmd {
                        None | Some(WsCmd::Shutdown) => break 'outer,
                        // Private actors ignore symbol commands.
                        Some(WsCmd::AddSymbol { .. }) | Some(WsCmd::RemoveSymbol { .. }) => {}
                    }
                }

                result = tokio::time::timeout(
                    std::time::Duration::from_secs(60),
                    event_stream.next(),
                ) => {
                    match result {
                        Ok(Some(Ok(event))) => {
                            retry_count = 0;
                            dispatch_event(&tx, &state, event, None);
                        }
                        Ok(Some(Err(e))) => {
                            eprintln!(
                                "[WsActor] {:?}/Private stream error: {}",
                                key.exchange_id, e
                            );
                            break;
                        }
                        Ok(None) => break,
                        Err(_) => {
                            eprintln!(
                                "[WsActor] {:?}/Private 60s silence, reconnecting",
                                key.exchange_id
                            );
                            break;
                        }
                    }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// BUILD WS — create + connect without subscribing to any symbol
// ─────────────────────────────────────────────────────────────────────────────

async fn build_ws(
    exchange_id: ExchangeId,
    ws_rtt_handles: &Arc<Mutex<HashMap<ExchangeId, Arc<tokio::sync::Mutex<u64>>>>>,
    credentials: Option<digdigdig3::Credentials>,
) -> Result<Box<dyn WebSocketConnector>, String> {
    macro_rules! standard {
        ($ws_type:ty) => {{
            let ws = <$ws_type>::new(credentials.clone(), false, AccountType::Spot)
                .await
                .map_err(|e| format!("WS create failed: {}", e))?;
            ws.connect(AccountType::Spot)
                .await
                .map_err(|e| format!("WS connect failed: {}", e))?;
            capture_rtt(&ws, exchange_id, ws_rtt_handles);
            Box::new(ws) as Box<dyn WebSocketConnector>
        }};
    }

    macro_rules! credentials_only {
        ($ws_type:ty) => {{
            let ws = <$ws_type>::new(credentials.clone())
                .await
                .map_err(|e| format!("WS create failed: {}", e))?;
            ws.connect(AccountType::Spot)
                .await
                .map_err(|e| format!("WS connect failed: {}", e))?;
            capture_rtt(&ws, exchange_id, ws_rtt_handles);
            Box::new(ws) as Box<dyn WebSocketConnector>
        }};
    }

    macro_rules! credentials_with_account {
        ($ws_type:ty) => {{
            let ws = <$ws_type>::new(credentials.clone(), AccountType::Spot)
                .await
                .map_err(|e| format!("WS create failed: {}", e))?;
            ws.connect(AccountType::Spot)
                .await
                .map_err(|e| format!("WS connect failed: {}", e))?;
            capture_rtt(&ws, exchange_id, ws_rtt_handles);
            Box::new(ws) as Box<dyn WebSocketConnector>
        }};
    }

    macro_rules! sync_new {
        ($ws_type:ty) => {{
            let ws = <$ws_type>::new(credentials.clone(), false, AccountType::Spot)
                .map_err(|e| format!("WS create failed: {}", e))?;
            ws.connect(AccountType::Spot)
                .await
                .map_err(|e| format!("WS connect failed: {}", e))?;
            capture_rtt(&ws, exchange_id, ws_rtt_handles);
            Box::new(ws) as Box<dyn WebSocketConnector>
        }};
    }

    let ws: Box<dyn WebSocketConnector> = match exchange_id {
        // ── Standard: new(creds, testnet, account_type) async ──
        ExchangeId::Binance  => standard!(BinanceWebSocket),
        ExchangeId::Bybit    => standard!(BybitWebSocket),
        ExchangeId::GateIO   => standard!(GateioWebSocket),
        ExchangeId::Bitfinex => standard!(BitfinexWebSocket),
        ExchangeId::BingX    => standard!(BingxWebSocket),
        ExchangeId::Bitget   => standard!(BitgetWebSocket),
        ExchangeId::Deribit  => standard!(DeribitWebSocket),
        ExchangeId::KuCoin   => standard!(KuCoinWebSocket),

        // ── OKX: new(creds, testnet, account_type) async ──
        ExchangeId::OKX => {
            let ws = OkxWebSocket::new(credentials.clone(), false, AccountType::Spot)
                .await
                .map_err(|e| format!("WS create failed: {}", e))?;
            ws.connect(AccountType::Spot)
                .await
                .map_err(|e| format!("WS connect failed: {}", e))?;
            if let Some(rtt) = <OkxWebSocket as WebSocketConnector>::ping_rtt_handle(&ws) {
                if let Ok(mut h) = ws_rtt_handles.lock() {
                    h.insert(exchange_id, rtt);
                }
            }
            Box::new(ws)
        }

        // ── Credentials only (no testnet, no account_type) ──
        ExchangeId::Coinbase => credentials_only!(CoinbaseWebSocket),
        // ── Credentials + AccountType (no testnet) ──
        ExchangeId::MEXC     => credentials_with_account!(MexcWebSocket),

        // ── Sync constructor ──
        ExchangeId::HTX => sync_new!(HtxWebSocket),

        // ── Kraken: new(token_opt, account_type) async — uses its own token type ──
        ExchangeId::Kraken => {
            let ws = KrakenWebSocket::new(None, AccountType::Spot)
                .await
                .map_err(|e| format!("WS create failed: {}", e))?;
            ws.connect(AccountType::Spot)
                .await
                .map_err(|e| format!("WS connect failed: {}", e))?;
            capture_rtt(&ws, exchange_id, ws_rtt_handles);
            Box::new(ws)
        }

        // ── Bitstamp: new() takes no arguments ──
        ExchangeId::Bitstamp => {
            let ws = BitstampWebSocket::new()
                .await
                .map_err(|e| format!("WS create failed: {}", e))?;
            ws.connect(AccountType::Spot)
                .await
                .map_err(|e| format!("WS connect failed: {}", e))?;
            capture_rtt(&ws, exchange_id, ws_rtt_handles);
            Box::new(ws)
        }

        // ── HyperLiquid: sync new(testnet) ──
        ExchangeId::HyperLiquid => {
            let ws = HyperliquidWebSocket::new(false);
            ws.connect(AccountType::Spot)
                .await
                .map_err(|e| format!("WS connect failed: {}", e))?;
            capture_rtt(&ws, exchange_id, ws_rtt_handles);
            Box::new(ws)
        }

        // ── Upbit: new(creds, region) async ──
        ExchangeId::Upbit => {
            let ws = UpbitWebSocket::new(credentials.clone(), "sg")
                .await
                .map_err(|e| format!("WS create failed: {}", e))?;
            ws.connect(AccountType::Spot)
                .await
                .map_err(|e| format!("WS connect failed: {}", e))?;
            capture_rtt(&ws, exchange_id, ws_rtt_handles);
            Box::new(ws)
        }

        // ── dYdX: new(testnet, account_type) async ──
        ExchangeId::Dydx => {
            let ws = DydxWebSocket::new(false, AccountType::Spot)
                .await
                .map_err(|e| format!("WS create failed: {}", e))?;
            ws.connect(AccountType::Spot)
                .await
                .map_err(|e| format!("WS connect failed: {}", e))?;
            capture_rtt(&ws, exchange_id, ws_rtt_handles);
            Box::new(ws)
        }

        // ── MOEX: sync new_public() ──
        ExchangeId::Moex => {
            let ws = MoexWebSocket::new_public();
            ws.connect(AccountType::Spot)
                .await
                .map_err(|e| format!("WS connect failed: {}", e))?;
            capture_rtt(&ws, exchange_id, ws_rtt_handles);
            Box::new(ws)
        }

        // ── Gemini: new_market_data(testnet) async ──
        ExchangeId::Gemini => {
            let mut ws = GeminiWebSocket::new_market_data(false)
                .await
                .map_err(|e| format!("WS create failed: {}", e))?;
            WebSocketConnector::connect(&mut ws, AccountType::Spot)
                .await
                .map_err(|e| format!("WS connect failed: {}", e))?;
            if let Some(rtt) = WebSocketConnector::ping_rtt_handle(&ws) {
                if let Ok(mut h) = ws_rtt_handles.lock() {
                    h.insert(exchange_id, rtt);
                }
            }
            Box::new(ws)
        }

        // ── Crypto.com: sync new(auth, is_user_stream) — uses its own auth type ──
        ExchangeId::CryptoCom => {
            let mut ws = CryptoComWebSocket::new(None, false);
            WebSocketConnector::connect(&mut ws, AccountType::Spot)
                .await
                .map_err(|e| format!("WS connect failed: {}", e))?;
            if let Some(rtt) = WebSocketConnector::ping_rtt_handle(&ws) {
                if let Ok(mut h) = ws_rtt_handles.lock() {
                    h.insert(exchange_id, rtt);
                }
            }
            Box::new(ws)
        }

        // ── Lighter: public(testnet) async ──
        ExchangeId::Lighter => {
            let mut ws = LighterWebSocket::public(false)
                .await
                .map_err(|e| format!("WS create failed: {}", e))?;
            WebSocketConnector::connect(&mut ws, AccountType::Spot)
                .await
                .map_err(|e| format!("WS connect failed: {}", e))?;
            if let Some(rtt) = WebSocketConnector::ping_rtt_handle(&ws) {
                if let Ok(mut h) = ws_rtt_handles.lock() {
                    h.insert(exchange_id, rtt);
                }
            }
            Box::new(ws)
        }

        other => {
            return Err(format!(
                "WebSocket not supported for {:?}",
                other
            ));
        }
    };

    Ok(ws)
}

/// Helper: capture the ping RTT handle from any connector that exposes it.
fn capture_rtt<C: WebSocketConnector>(
    ws: &C,
    exchange_id: ExchangeId,
    ws_rtt_handles: &Arc<Mutex<HashMap<ExchangeId, Arc<tokio::sync::Mutex<u64>>>>>,
) {
    if let Some(rtt) = ws.ping_rtt_handle() {
        if let Ok(mut h) = ws_rtt_handles.lock() {
            h.insert(exchange_id, rtt);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SUBSCRIPTION REQUEST BUILDER
// ─────────────────────────────────────────────────────────────────────────────

/// `_depth` is reserved for Trades/Ticker paths that may use it in future.
/// For the Depth path it is ignored — the diff stream has no depth limit.
pub(crate) fn make_sub_request(
    stream_type: WsStreamType,
    exchange_id: ExchangeId,
    symbol: &str,
    account_type: AccountType,
    depth: Option<u32>,
) -> SubscriptionRequest {
    let sym = crate::bridge::parse_symbol_for_exchange(exchange_id, symbol);
    match stream_type {
        WsStreamType::Trades => SubscriptionRequest::trade_for(sym, account_type),
        WsStreamType::Ticker => SubscriptionRequest::ticker_for(sym, account_type),
        // Use the standard Orderbook stream (partial snapshot or per-exchange
        // default). One-shot REST bootstrap in `bridge.rs` seeds the deep
        // ladder; subsequent WS messages — snapshot or delta, exchange's
        // choice — maintain it.
        //
        // We tried hardcoding StreamType::OrderbookDelta but most exchanges
        // (Bybit, OKX, Kraken, ...) don't have a Binance-style full diff
        // stream, and forcing the type through their WS layer broke their
        // subscriptions. Each connector picks the most fitting stream for
        // its API.
        WsStreamType::Depth => {
            let req = SubscriptionRequest::orderbook(sym);
            if let Some(d) = depth {
                req.with_depth(d)
            } else {
                req
            }
        }
        // Full incremental-diff stream (Binance @depth@100ms).
        // Exchanges that don't support a separate diff stream will return an
        // error from `ws.subscribe()`, which the actor logs and ignores — the
        // Depth actor still covers those exchanges.
        WsStreamType::DepthDiff => SubscriptionRequest {
            symbol: sym,
            stream_type: StreamType::OrderbookDelta,
            account_type,
            depth: None,
            update_speed_ms: Some(100),
        },
        // Private streams don't use per-symbol subscription requests.
        // This arm exists for exhaustiveness only; the private actor bypasses
        // this function entirely.
        WsStreamType::Private => SubscriptionRequest::new(
            Symbol::empty(),
            StreamType::OrderUpdate,
        ),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// EVENT DISPATCHER
// ─────────────────────────────────────────────────────────────────────────────

fn dispatch_event(
    tx: &broadcast::Sender<LiveUpdate>,
    state: &WsActorState,
    event: StreamEvent,
    orderbook_map: Option<&SharedOrderbookMap>,
) {
    match event {
        StreamEvent::Trade { symbol, trade } => {
            let _ = tx.send(LiveUpdate::TradeUpdate {
                exchange_id: state.exchange_id,
                account_type: state.account_type,
                symbol,
                price: trade.price,
                quantity: trade.quantity,
                timestamp: trade.timestamp,
                is_buyer_maker: trade.side == digdigdig3::core::types::TradeSide::Sell,
            });
        }
        StreamEvent::Ticker { symbol, ticker } => {
            let _ = tx.send(LiveUpdate::MiniTickerUpdate {
                exchange_id: state.exchange_id,
                account_type: state.account_type,
                symbol,
                last_price: ticker.last_price,
                price_change_percent: ticker.price_change_percent_24h,
                high_price: ticker.high_24h,
                low_price: ticker.low_24h,
                volume: ticker.volume_24h,
            });
        }
        StreamEvent::OrderbookSnapshot { symbol: _, book: ob } => {
            // Some exchanges (e.g. Bitstamp) emit orderbook updates instead of
            // ticker events on the ticker stream.  Synthesize a price from the
            // mid-point of the best bid/ask.
            if state.stream_type == WsStreamType::Ticker {
                let bid = ob.bids.first().map(|l| l.price).unwrap_or(0.0);
                let ask = ob.asks.first().map(|l| l.price).unwrap_or(0.0);
                let mid = if bid > 0.0 && ask > 0.0 {
                    (bid + ask) / 2.0
                } else {
                    bid.max(ask)
                };
                if mid > 0.0 {
                    // We don't know the symbol from the event alone — skip.
                    // The symbol is embedded in the subscription, but the
                    // Bitstamp WS actor only handles one symbol anyway.
                    // We emit one update per active symbol at mid price.
                    for (sym, &count) in &state.refcounts {
                        if count > 0 {
                            let _ = tx.send(LiveUpdate::MiniTickerUpdate {
                                exchange_id: state.exchange_id,
                                account_type: state.account_type,
                                symbol: sym.clone(),
                                last_price: mid,
                                price_change_percent: None,
                                high_price: None,
                                low_price: None,
                                volume: None,
                            });
                        }
                    }
                }
            }
            if state.stream_type == WsStreamType::Depth || state.stream_type == WsStreamType::DepthDiff {
                for (sym, &count) in &state.refcounts {
                    if count > 0 {
                        let bids: Vec<(f64, f64)> = ob.bids.iter().map(|l| (l.price, l.size)).collect();
                        let asks: Vec<(f64, f64)> = ob.asks.iter().map(|l| (l.price, l.size)).collect();
                        // Write to SharedOrderbookMap.
                        if let Some(ob_map) = orderbook_map {
                            let ob_key = OrderbookKey::new(state.exchange_id, state.account_type, sym.as_str());
                            if let Some(handle) = ob_map.read().ok().and_then(|m| m.get(&ob_key).cloned()) {
                                if let Ok(mut series) = handle.write() {
                                    series.apply_ws_snapshot(&bids, &asks, ob.timestamp);
                                }
                            }
                        }
                        let _ = tx.send(LiveUpdate::OrderbookSnapshot {
                            exchange_id: state.exchange_id,
                            account_type: state.account_type,
                            symbol: sym.clone(),
                            bids,
                            asks,
                            timestamp: ob.timestamp,
                            source: crate::bridge::OrderbookSource::Ws,
                        });
                    }
                }
            }
        }
        StreamEvent::OrderbookDelta { symbol: _, delta } => {
            let bids = &delta.bids;
            let asks = &delta.asks;
            let timestamp = delta.timestamp;
            if state.stream_type == WsStreamType::Ticker {
                let best_bid = bids
                    .iter()
                    .map(|l| l.price)
                    .filter(|p| *p > 0.0)
                    .fold(0.0f64, f64::max);
                let best_ask = asks
                    .iter()
                    .map(|l| l.price)
                    .filter(|p| *p > 0.0)
                    .fold(f64::MAX, f64::min);
                let mid = if best_bid > 0.0 && best_ask < f64::MAX {
                    (best_bid + best_ask) / 2.0
                } else {
                    best_bid.max(if best_ask < f64::MAX { best_ask } else { 0.0 })
                };
                if mid > 0.0 {
                    for (sym, &count) in &state.refcounts {
                        if count > 0 {
                            let _ = tx.send(LiveUpdate::MiniTickerUpdate {
                                exchange_id: state.exchange_id,
                                account_type: state.account_type,
                                symbol: sym.clone(),
                                last_price: mid,
                                price_change_percent: None,
                                high_price: None,
                                low_price: None,
                                volume: None,
                            });
                        }
                    }
                }
            }
            if state.stream_type == WsStreamType::Depth || state.stream_type == WsStreamType::DepthDiff {
                for (sym, &count) in &state.refcounts {
                    if count > 0 {
                        let bid_changes: Vec<(f64, f64)> = bids.iter().map(|l| (l.price, l.size)).collect();
                        let ask_changes: Vec<(f64, f64)> = asks.iter().map(|l| (l.price, l.size)).collect();
                        // Write to SharedOrderbookMap.
                        if let Some(ob_map) = orderbook_map {
                            let ob_key = OrderbookKey::new(state.exchange_id, state.account_type, sym.as_str());
                            if let Some(handle) = ob_map.read().ok().and_then(|m| m.get(&ob_key).cloned()) {
                                if let Ok(mut series) = handle.write() {
                                    series.apply_ws_delta(&bid_changes, &ask_changes, timestamp);
                                }
                            }
                        }
                        let _ = tx.send(LiveUpdate::OrderbookDelta {
                            exchange_id: state.exchange_id,
                            account_type: state.account_type,
                            symbol: sym.clone(),
                            bids: bid_changes,
                            asks: ask_changes,
                            timestamp,
                        });
                    }
                }
            }
        }
        StreamEvent::OrderUpdate { symbol, event } => {
            let _ = tx.send(LiveUpdate::OrderUpdate {
                exchange_id: state.exchange_id,
                account_type: state.account_type,
                symbol,
                event,
            });
        }
        StreamEvent::BalanceUpdate(event) => {
            let _ = tx.send(LiveUpdate::BalanceUpdate {
                exchange_id: state.exchange_id,
                account_type: state.account_type,
                event,
            });
        }
        StreamEvent::PositionUpdate { symbol, event } => {
            let _ = tx.send(LiveUpdate::PositionUpdate {
                exchange_id: state.exchange_id,
                account_type: state.account_type,
                symbol,
                event,
            });
        }
        _ => {}
    }
}
