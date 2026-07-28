//! `TradingManager` — the stateful order/position/session registry.
//!
//! Owns everything about live trading state: per-`(ExchangeId,
//! AccountType)` sessions, order managers, position trackers, paper-trading
//! engines, cached balances, recent fills, and the published
//! [`TradingSnapshot`]. Compute-free value types
//! (`Order`/`Position`/`Fill`/`TradingConfig`/`TradingError`/…) live in
//! `mlc-backend-trading-engine`, re-exported here for construction — this
//! is the doctrine "manager" half of the trading system (3.7c lift, mirrors
//! `mlc_backend::indicator::manager` from 3.7a).
//!
//! App code never constructs or holds this type directly — access goes
//! through `mlc_backend::trading::{Trading, TradingMut}`
//! (`BackendCore::trading` / `trading_mut`).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::path::PathBuf;

use mlc_backend_data_bridge::DataBridge;

use mlc_backend_trading_engine::{
    ExchangeId, AccountType,
    Order, OrderStatus, Position, Balance, Fill, MarginType, TimeInForce,
    OrderRequest, CancelRequest, CancelScope, PlaceOrderResponse, PositionQuery,
    TradingCapabilities, AccountCapabilities, SessionInfo,
    TradingConfig, TradingError, TradingResult,
    TradingSnapshot, SharedTradingSnapshot,
};
use crate::trading::order_manager::OrderManager;
use crate::trading::position_tracker::PositionTracker;
use crate::trading::paper_engine::PaperEngine;

enum TradingTaskResult {
    OrderPlaced {
        key: (ExchangeId, AccountType),
        result: TradingResult<PlaceOrderResponse>,
    },
    OrderCancelled {
        key: (ExchangeId, AccountType),
        result: TradingResult<Order>,
    },
    BalancesRefreshed {
        key: (ExchangeId, AccountType),
        result: TradingResult<Vec<Balance>>,
    },
    PositionsRefreshed {
        key: (ExchangeId, AccountType),
        result: TradingResult<Vec<Position>>,
    },
}

pub struct TradingManager {
    bridge: Arc<DataBridge>,
    paper_engines: HashMap<(ExchangeId, AccountType), PaperEngine>,
    order_managers: HashMap<(ExchangeId, AccountType), OrderManager>,
    position_trackers: HashMap<(ExchangeId, AccountType), PositionTracker>,
    sessions: HashMap<(ExchangeId, AccountType), SessionInfo>,
    capabilities: HashMap<(ExchangeId, AccountType), TradingCapabilities>,
    account_caps: HashMap<(ExchangeId, AccountType), AccountCapabilities>,
    snapshots: HashMap<(ExchangeId, AccountType), SharedTradingSnapshot>,
    config: TradingConfig,
    /// Native: tokio runtime handle for spawning trading API calls.
    /// Wasm: unused — spawns go through `wasm_bindgen_futures::spawn_local`.
    #[cfg(not(target_arch = "wasm32"))]
    runtime_handle: tokio::runtime::Handle,
    result_tx: std::sync::mpsc::SyncSender<TradingTaskResult>,
    result_rx: std::sync::mpsc::Receiver<TradingTaskResult>,
    /// Fed by every live `PaperEngine` (one per `(exchange_id, account_type)`
    /// key); each fill carries its origin key so the drain in `tick()` can
    /// route it into the exact `recent_fills[key]` bucket instead of
    /// guessing.
    paper_fill_tx: tokio::sync::mpsc::UnboundedSender<((ExchangeId, AccountType), Fill)>,
    paper_fill_rx: tokio::sync::mpsc::UnboundedReceiver<((ExchangeId, AccountType), Fill)>,
    data_dir: PathBuf,
    cached_balances: HashMap<(ExchangeId, AccountType), Vec<Balance>>,
    recent_fills: HashMap<(ExchangeId, AccountType), VecDeque<Fill>>,
    /// Bounded (100) terminal-order history for LIVE (non-paper) sessions —
    /// mirrors `PaperEngine::history` for paper sessions. Fed from the
    /// `LiveUpdate::OrderUpdate` branch of `tick()` when the resulting order
    /// status is terminal, and from a successful `OrderCancelled` task
    /// result.
    order_history: HashMap<(ExchangeId, AccountType), VecDeque<Order>>,
    last_error: HashMap<(ExchangeId, AccountType), String>,
    orders_in_flight: HashSet<(ExchangeId, AccountType)>,
}

impl TradingManager {
    pub fn new(
        bridge: Arc<DataBridge>,
        data_dir: PathBuf,
        #[cfg(not(target_arch = "wasm32"))] runtime_handle: tokio::runtime::Handle,
    ) -> TradingResult<Self> {
        let config = TradingConfig::load(&data_dir.join("trading_config.json"))?;
        let (result_tx, result_rx) = std::sync::mpsc::sync_channel(256);
        let (paper_fill_tx, paper_fill_rx) = tokio::sync::mpsc::unbounded_channel();

        Ok(Self {
            bridge,
            paper_engines: HashMap::new(),
            order_managers: HashMap::new(),
            position_trackers: HashMap::new(),
            sessions: HashMap::new(),
            capabilities: HashMap::new(),
            account_caps: HashMap::new(),
            snapshots: HashMap::new(),
            config,
            #[cfg(not(target_arch = "wasm32"))]
            runtime_handle,
            result_tx,
            result_rx,
            paper_fill_tx,
            paper_fill_rx,
            data_dir,
            cached_balances: HashMap::new(),
            recent_fills: HashMap::new(),
            order_history: HashMap::new(),
            last_error: HashMap::new(),
            orders_in_flight: HashSet::new(),
        })
    }

    /// Shared handle to the `(exchange_id, account_type)` session's snapshot,
    /// lazily creating an empty one the first time this key is requested.
    pub fn snapshot_for(&mut self, exchange_id: ExchangeId, account_type: AccountType) -> SharedTradingSnapshot {
        Arc::clone(
            self.snapshots
                .entry((exchange_id, account_type))
                .or_insert_with(TradingSnapshot::new_shared),
        )
    }

    /// Every currently known session snapshot, keyed by `(exchange_id,
    /// account_type)`. Enumeration only — does not create new slots.
    pub fn all_snapshots(&self) -> Vec<(ExchangeId, AccountType, SharedTradingSnapshot)> {
        self.snapshots
            .iter()
            .map(|(&(exchange_id, account_type), snap)| (exchange_id, account_type, Arc::clone(snap)))
            .collect()
    }

    /// Spawn a fire-and-forget trading API call.
    ///
    /// Native: tokio::Handle::spawn (Send-bound future).
    /// Wasm: wasm_bindgen_futures::spawn_local (!Send future, no JoinHandle).
    #[cfg(not(target_arch = "wasm32"))]
    fn spawn_task<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.runtime_handle.spawn(fut);
    }

    #[cfg(target_arch = "wasm32")]
    fn spawn_task<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + 'static,
    {
        wasm_bindgen_futures::spawn_local(fut);
    }

    pub fn place_order(
        &mut self,
        exchange_id: ExchangeId,
        account_type: AccountType,
        req: OrderRequest,
    ) -> TradingResult<()> {
        let key = (exchange_id, account_type);

        if let Some(caps) = self.capabilities.get(&key) {
            let _ = caps;
        }

        if self.config.is_paper(exchange_id, account_type) {
            // No silent no-op: a missing paper engine (exchange connector not
            // yet ready, or `on_connector_ready` never ran for this pair —
            // e.g. the connector toggle is disabled) previously fell through
            // this `if let` and returned `Ok(())` regardless, so the order
            // vanished with a false success and no way for the caller to
            // surface anything to the user.
            let Some(engine) = self.paper_engines.get_mut(&key) else {
                return Err(TradingError::NoSession { exchange: exchange_id, account_type });
            };
            let _order = engine.place_order_sync(req)?;
            return Ok(());
        }

        self.orders_in_flight.insert(key);
        let bridge = Arc::clone(&self.bridge);
        let tx = self.result_tx.clone();
        // Reserve a client ID slot even though we don't attach it to the result yet
        let _client_id = self.order_managers
            .entry(key)
            .or_insert_with(|| OrderManager::new("live"))
            .next_client_id();

        self.spawn_task(async move {
            let result = bridge.place_order(exchange_id, req).await
                .map_err(TradingError::Exchange);
            let _ = tx.send(TradingTaskResult::OrderPlaced { key, result });
        });

        Ok(())
    }

    pub fn cancel_order(
        &mut self,
        exchange_id: ExchangeId,
        account_type: AccountType,
        req: CancelRequest,
    ) -> TradingResult<()> {
        let key = (exchange_id, account_type);

        if let Some(engine) = self.paper_engines.get_mut(&key) {
            if let CancelScope::Single { order_id } = &req.scope {
                engine.cancel_order(order_id)?;
            }
            return Ok(());
        }

        let bridge = Arc::clone(&self.bridge);
        let tx = self.result_tx.clone();
        self.spawn_task(async move {
            let result = bridge.cancel_order(exchange_id, req).await
                .map_err(TradingError::Exchange);
            let _ = tx.send(TradingTaskResult::OrderCancelled { key, result });
        });
        Ok(())
    }

    pub fn refresh_balances(&mut self, exchange_id: ExchangeId, account_type: AccountType) {
        let key = (exchange_id, account_type);
        let bridge = Arc::clone(&self.bridge);
        let tx = self.result_tx.clone();

        self.spawn_task(async move {
            let query = digdigdig3::core::BalanceQuery { asset: None, account_type };
            let result = bridge.get_balance(exchange_id, query).await
                .map_err(TradingError::Exchange);
            let _ = tx.send(TradingTaskResult::BalancesRefreshed { key, result });
        });
    }

    pub fn refresh_positions(&mut self, exchange_id: ExchangeId, account_type: AccountType) {
        let key = (exchange_id, account_type);
        let bridge = Arc::clone(&self.bridge);
        let tx = self.result_tx.clone();

        self.spawn_task(async move {
            let query = PositionQuery { symbol: None, account_type };
            let result = bridge.get_positions(exchange_id, query).await
                .map_err(TradingError::Exchange);
            let _ = tx.send(TradingTaskResult::PositionsRefreshed { key, result });
        });
    }

    pub fn tick(&mut self, live_updates: &[mlc_backend_data_bridge::LiveUpdate]) {
        while let Ok(result) = self.result_rx.try_recv() {
            match result {
                TradingTaskResult::OrderPlaced { key, result } => {
                    self.orders_in_flight.remove(&key);
                    match result {
                        Ok(_response) => {
                            self.last_error.remove(&key);
                        }
                        Err(e) => {
                            self.last_error.insert(key, e.to_string());
                        }
                    }
                }
                TradingTaskResult::OrderCancelled { key, result } => {
                    match result {
                        Ok(order) => {
                            if let Some(om) = self.order_managers.get_mut(&key) {
                                om.remove(&order.id);
                            }
                            Self::push_order_history(&mut self.order_history, key, order);
                            self.last_error.remove(&key);
                        }
                        Err(e) => {
                            self.last_error.insert(key, e.to_string());
                        }
                    }
                }
                TradingTaskResult::BalancesRefreshed { key, result } => {
                    if let Ok(balances) = result {
                        self.cached_balances.insert(key, balances);
                    }
                }
                TradingTaskResult::PositionsRefreshed { key, result } => {
                    if let Ok(positions) = result {
                        if let Some(pt) = self.position_trackers.get_mut(&key) {
                            pt.update_positions(positions);
                        }
                    }
                }
            }
        }

        for update in live_updates {
            match update {
                mlc_backend_data_bridge::LiveUpdate::OrderUpdate { exchange_id, account_type, symbol: order_symbol, event } => {
                    let key = (*exchange_id, *account_type);
                    if let Some(om) = self.order_managers.get_mut(&key) {
                        let order = Order {
                            id: event.order_id.clone(),
                            client_order_id: event.client_order_id.clone(),
                            symbol: Some(order_symbol.clone()),
                            side: event.side,
                            order_type: event.order_type.clone(),
                            status: event.status,
                            price: event.price,
                            stop_price: None,
                            quantity: event.quantity,
                            filled_quantity: event.filled_quantity,
                            average_price: event.average_price,
                            commission: event.last_fill_commission,
                            commission_asset: event.commission_asset.clone(),
                            created_at: event.timestamp,
                            updated_at: Some(event.timestamp),
                            time_in_force: TimeInForce::Gtc,
                        };
                        om.upsert(order.clone());
                        if matches!(
                            order.status,
                            OrderStatus::Filled | OrderStatus::Canceled | OrderStatus::Rejected | OrderStatus::Expired
                        ) {
                            Self::push_order_history(&mut self.order_history, key, order);
                        }

                        if let (Some(fill_price), Some(fill_qty)) = (event.last_fill_price, event.last_fill_quantity) {
                            if fill_qty > 0.0 {
                                let fill = Fill {
                                    order_id: event.order_id.clone(),
                                    symbol: order_symbol.clone(),
                                    side: event.side,
                                    price: fill_price,
                                    quantity: fill_qty,
                                    fee: event.last_fill_commission.unwrap_or(0.0),
                                    fee_asset: event.commission_asset.clone().unwrap_or_default(),
                                    timestamp: event.timestamp,
                                    is_paper: false,
                                };
                                if let Some(pt) = self.position_trackers.get_mut(&key) {
                                    pt.apply_fill(&fill);
                                }
                                let fills = self.recent_fills.entry(key).or_insert_with(VecDeque::new);
                                fills.push_back(fill);
                                if fills.len() > 100 {
                                    fills.pop_front();
                                }
                            }
                        }
                    }
                }
                mlc_backend_data_bridge::LiveUpdate::BalanceUpdate { exchange_id, account_type, event } => {
                    let key = (*exchange_id, *account_type);
                    let balances = self.cached_balances.entry(key).or_insert_with(Vec::new);
                    if let Some(existing) = balances.iter_mut().find(|b| b.asset == event.asset) {
                        existing.free = event.free;
                        existing.locked = event.locked;
                        existing.total = event.total;
                    } else {
                        balances.push(Balance {
                            asset: event.asset.clone(),
                            free: event.free,
                            locked: event.locked,
                            total: event.total,
                        });
                    }
                }
                mlc_backend_data_bridge::LiveUpdate::PositionUpdate { exchange_id, account_type, symbol: pos_symbol, event } => {
                    let key = (*exchange_id, *account_type);
                    let pt = self.position_trackers.entry(key)
                        .or_insert_with(|| PositionTracker::new(false));
                    let pos = Position {
                        symbol: pos_symbol.clone(),
                        side: event.side,
                        quantity: event.quantity,
                        entry_price: event.entry_price,
                        mark_price: event.mark_price,
                        unrealized_pnl: event.unrealized_pnl,
                        realized_pnl: event.realized_pnl,
                        liquidation_price: event.liquidation_price,
                        leverage: event.leverage.unwrap_or(1),
                        margin_type: event.margin_type.unwrap_or(MarginType::Cross),
                        margin: None,
                        take_profit: None,
                        stop_loss: None,
                    };
                    pt.update_positions(vec![pos]);
                }
                _ => {}
            }
        }

        for engine in self.paper_engines.values_mut() {
            engine.tick();
        }

        // Drain paper fills into recent_fills, keyed by the exact
        // (exchange_id, account_type) each fill's origin `PaperEngine` sent
        // alongside it — NOT a "first paper session found" guess. With 2+
        // paper sessions live at once (e.g. Binance/Bybit/OKX all
        // auto-created by `on_connector_ready` for market data), routing
        // every fill to `paper_keys.first()` (nondeterministic `HashMap`
        // order) meant a fill almost never landed in the `recent_fills`
        // bucket its own snapshot reads from, so `TradeLog` stayed empty
        // even after a successful paper fill. PaperEngine applies fills to
        // positions internally; here we only record them so snapshots can
        // show recent activity.
        while let Ok((key, fill)) = self.paper_fill_rx.try_recv() {
            let fills = self.recent_fills.entry(key).or_insert_with(VecDeque::new);
            fills.push_back(fill);
            if fills.len() > 100 {
                fills.pop_front();
            }
        }

        let mut all_keys: HashSet<(ExchangeId, AccountType)> = HashSet::new();
        all_keys.extend(self.sessions.keys().cloned());
        all_keys.extend(self.paper_engines.keys().cloned());
        all_keys.extend(self.position_trackers.keys().cloned());
        for key in all_keys {
            self.publish_snapshot(key);
        }
    }

    fn publish_snapshot(&mut self, key: (ExchangeId, AccountType)) {
        let is_paper = self.paper_engines.contains_key(&key);

        let (open_orders, positions, pnl, balances, order_history, position_history) = if let Some(engine) = self.paper_engines.get(&key) {
            let oo: Vec<Order> = engine.orders().open_orders().cloned().collect();
            let pos: Vec<Position> = engine.positions().all_positions().values().cloned().collect();
            let p = engine.positions().all_pnl();
            let bal: Vec<Balance> = engine.balances().iter().map(|(asset, &amount)| {
                Balance { asset: asset.clone(), free: amount, locked: 0.0, total: amount }
            }).collect();
            let hist = engine.history().clone();
            let pos_hist = engine.positions().closed_history().clone();
            (oo, pos, p, bal, hist, pos_hist)
        } else {
            let oo = self.order_managers.get(&key)
                .map(|om| om.open_orders().cloned().collect())
                .unwrap_or_default();
            let pos = self.position_trackers.get(&key)
                .map(|pt| pt.all_positions().values().cloned().collect())
                .unwrap_or_default();
            let p = self.position_trackers.get(&key)
                .map(|pt| pt.all_pnl())
                .unwrap_or_default();
            let bal = self.cached_balances.get(&key).cloned().unwrap_or_default();
            let hist = self.order_history.get(&key).cloned().unwrap_or_default();
            let pos_hist = self.position_trackers.get(&key)
                .map(|pt| pt.closed_history().clone())
                .unwrap_or_default();
            (oo, pos, p, bal, hist, pos_hist)
        };

        let new_snap = TradingSnapshot {
            session: self.sessions.get(&key).cloned(),
            open_orders,
            positions,
            pnl,
            balances,
            capabilities: self.capabilities.get(&key).copied(),
            is_paper,
            recent_fills: self.recent_fills.get(&key).cloned().unwrap_or_default(),
            last_error: self.last_error.get(&key).cloned(),
            order_in_flight: self.orders_in_flight.contains(&key),
            order_history,
            position_history,
        };
        let slot = self.snapshot_for(key.0, key.1);
        if let Ok(mut snap) = slot.write() {
            *snap = new_snap;
        };
    }

    /// Push a terminal-state LIVE order into the bounded (100) per-key
    /// history bucket, evicting the oldest entry past capacity. Free fn
    /// (takes the map by `&mut` explicitly) so it can be called from inside
    /// `tick()`'s `live_updates` loop without a second `&mut self` borrow.
    fn push_order_history(
        bucket: &mut HashMap<(ExchangeId, AccountType), VecDeque<Order>>,
        key: (ExchangeId, AccountType),
        order: Order,
    ) {
        let hist = bucket.entry(key).or_insert_with(VecDeque::new);
        hist.push_back(order);
        if hist.len() > 100 {
            hist.pop_front();
        }
    }

    pub fn on_connector_ready(&mut self, exchange_id: ExchangeId) {
        if let Some(account_types) = self.bridge.supported_account_types(exchange_id) {
            for at in account_types {
                let key = (exchange_id, at);
                if let Some(caps) = self.bridge.trading_capabilities(exchange_id, at) {
                    self.capabilities.insert(key, caps);
                }
                if let Some(acaps) = self.bridge.account_capabilities(exchange_id, at) {
                    self.account_caps.insert(key, acaps);
                }

                let is_paper = self.config.is_paper(exchange_id, at);

                self.sessions.entry(key).or_insert(SessionInfo {
                    exchange_id,
                    account_type: at,
                    is_paper,
                    is_testnet: false,
                });

                if !is_paper {
                    self.order_managers.entry(key).or_insert_with(|| OrderManager::new("live"));
                }

                self.position_trackers.entry(key).or_insert_with(|| PositionTracker::new(is_paper));

                if is_paper {
                    let bridge = Arc::clone(&self.bridge);
                    let initial_balances = self.config.paper_initial_balances.clone();
                    let fill_tx = self.paper_fill_tx.clone();
                    self.paper_engines.entry(key).or_insert_with(|| {
                        PaperEngine::new(
                            exchange_id,
                            at,
                            bridge,
                            initial_balances,
                            fill_tx,
                        )
                    });
                }
            }
        }
    }

    /// Returns the data directory path (used for config persistence).
    pub fn data_dir(&self) -> &std::path::Path {
        &self.data_dir
    }
}
