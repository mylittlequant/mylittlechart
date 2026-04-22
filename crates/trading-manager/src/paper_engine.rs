use std::collections::HashMap;
use std::path::PathBuf;
use crate::types::{ExchangeId, AccountType, OrderRequest, Order, Fill};
use crate::order_manager::OrderManager;
use crate::position_tracker::PositionTracker;
use crate::error::TradingResult;
use orderbook_service::SharedOrderbookMap;

pub struct PaperEngine {
    _exchange_id: ExchangeId,
    _account_type: AccountType,
    _orderbook: SharedOrderbookMap,
    _pending: Vec<PaperOrder>,
    _balances: HashMap<String, f64>,
    orders: OrderManager,
    positions: PositionTracker,
    _fill_tx: tokio::sync::mpsc::UnboundedSender<Fill>,
    _persist_path: PathBuf,
}

struct PaperOrder {
    _order: Order,
    _trigger_price: Option<f64>,
}

impl PaperEngine {
    pub fn new(
        exchange_id: ExchangeId,
        account_type: AccountType,
        orderbook: SharedOrderbookMap,
        initial_balances: HashMap<String, f64>,
        fill_tx: tokio::sync::mpsc::UnboundedSender<Fill>,
        persist_path: PathBuf,
    ) -> Self {
        Self {
            _exchange_id: exchange_id,
            _account_type: account_type,
            _orderbook: orderbook,
            _pending: Vec::new(),
            _balances: initial_balances,
            orders: OrderManager::new("paper"),
            positions: PositionTracker::new(),
            _fill_tx: fill_tx,
            _persist_path: persist_path,
        }
    }

    pub fn place_order_sync(&mut self, req: OrderRequest) -> TradingResult<Order> {
        let _ = req;
        Err(crate::error::TradingError::Paper("not yet implemented".into()))
    }

    pub fn tick(&mut self, _symbol: &str, _last_price: f64, _best_bid: f64, _best_ask: f64) {
    }

    pub fn positions(&self) -> &PositionTracker {
        &self.positions
    }

    pub fn orders(&self) -> &OrderManager {
        &self.orders
    }
}
