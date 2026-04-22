use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::path::PathBuf;

use live_data::DataBridge;

use crate::types::*;
use crate::error::{TradingError, TradingResult};
use crate::config::TradingConfig;
use crate::order_manager::OrderManager;
use crate::position_tracker::PositionTracker;
use crate::paper_engine::PaperEngine;
use crate::snapshot::{TradingSnapshot, SharedTradingSnapshot};

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
    snapshot: SharedTradingSnapshot,
    config: TradingConfig,
    runtime_handle: tokio::runtime::Handle,
    result_tx: std::sync::mpsc::SyncSender<TradingTaskResult>,
    result_rx: std::sync::mpsc::Receiver<TradingTaskResult>,
    paper_fill_tx: tokio::sync::mpsc::UnboundedSender<Fill>,
    paper_fill_rx: tokio::sync::mpsc::UnboundedReceiver<Fill>,
    data_dir: PathBuf,
    cached_balances: HashMap<(ExchangeId, AccountType), Vec<Balance>>,
    recent_fills: HashMap<(ExchangeId, AccountType), VecDeque<Fill>>,
    last_error: HashMap<(ExchangeId, AccountType), String>,
    orders_in_flight: HashSet<(ExchangeId, AccountType)>,
}

impl TradingManager {
    pub fn new(
        bridge: Arc<DataBridge>,
        data_dir: PathBuf,
    ) -> TradingResult<Self> {
        let runtime_handle = bridge.runtime_handle();
        let config = TradingConfig::load(&data_dir.join("trading_config.json"))?;
        let snapshot = TradingSnapshot::new_shared();
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
            snapshot,
            config,
            runtime_handle,
            result_tx,
            result_rx,
            paper_fill_tx,
            paper_fill_rx,
            data_dir,
            cached_balances: HashMap::new(),
            recent_fills: HashMap::new(),
            last_error: HashMap::new(),
            orders_in_flight: HashSet::new(),
        })
    }

    pub fn snapshot(&self) -> SharedTradingSnapshot {
        Arc::clone(&self.snapshot)
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
            if let Some(engine) = self.paper_engines.get_mut(&key) {
                let _order = engine.place_order_sync(req)?;
            }
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

        self.runtime_handle.spawn(async move {
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
        self.runtime_handle.spawn(async move {
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

        self.runtime_handle.spawn(async move {
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

        self.runtime_handle.spawn(async move {
            let query = PositionQuery { symbol: None, account_type };
            let result = bridge.get_positions(exchange_id, query).await
                .map_err(TradingError::Exchange);
            let _ = tx.send(TradingTaskResult::PositionsRefreshed { key, result });
        });
    }

    pub fn tick(&mut self, live_updates: &[live_data::LiveUpdate]) {
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
                live_data::LiveUpdate::OrderUpdate { exchange_id, account_type, event } => {
                    let key = (*exchange_id, *account_type);
                    if let Some(om) = self.order_managers.get_mut(&key) {
                        let order = Order {
                            id: event.order_id.clone(),
                            client_order_id: event.client_order_id.clone(),
                            symbol: event.symbol.clone(),
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
                        om.upsert(order);

                        if let (Some(fill_price), Some(fill_qty)) = (event.last_fill_price, event.last_fill_quantity) {
                            if fill_qty > 0.0 {
                                let fill = Fill {
                                    order_id: event.order_id.clone(),
                                    symbol: event.symbol.clone(),
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
                live_data::LiveUpdate::BalanceUpdate { exchange_id, account_type, event } => {
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
                live_data::LiveUpdate::PositionUpdate { exchange_id, account_type, event } => {
                    let key = (*exchange_id, *account_type);
                    let pt = self.position_trackers.entry(key)
                        .or_insert_with(|| PositionTracker::new(false));
                    let pos = Position {
                        symbol: event.symbol.clone(),
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

        // Drain paper fills into recent_fills for the first paper session found.
        // PaperEngine applies fills to positions internally; here we record them
        // so snapshots can show recent activity.
        let paper_keys: Vec<_> = self.paper_engines.keys().copied().collect();
        while let Ok(fill) = self.paper_fill_rx.try_recv() {
            if let Some(&key) = paper_keys.first() {
                let fills = self.recent_fills.entry(key).or_insert_with(VecDeque::new);
                fills.push_back(fill);
                if fills.len() > 100 {
                    fills.pop_front();
                }
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

        let (open_orders, positions, pnl, balances) = if let Some(engine) = self.paper_engines.get(&key) {
            let oo: Vec<Order> = engine.orders().open_orders().cloned().collect();
            let pos: Vec<Position> = engine.positions().all_positions().values().cloned().collect();
            let p = engine.positions().all_pnl();
            let bal: Vec<Balance> = engine.balances().iter().map(|(asset, &amount)| {
                Balance { asset: asset.clone(), free: amount, locked: 0.0, total: amount }
            }).collect();
            (oo, pos, p, bal)
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
            (oo, pos, p, bal)
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
        };
        if let Ok(mut snap) = self.snapshot.write() {
            *snap = new_snap;
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
                    let orderbook = self.bridge.orderbook_map();
                    let initial_balances = self.config.paper_initial_balances.clone();
                    let fill_tx = self.paper_fill_tx.clone();
                    self.paper_engines.entry(key).or_insert_with(|| {
                        PaperEngine::new(
                            exchange_id,
                            at,
                            orderbook,
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
