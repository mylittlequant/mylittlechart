use std::sync::Arc;
use std::collections::{HashMap, VecDeque};
use crate::types::{
    Order, Position, Balance, TradingCapabilities, Fill, PositionPnl, SessionInfo,
};

#[derive(Debug, Clone)]
pub struct TradingSnapshot {
    pub session: Option<SessionInfo>,
    pub open_orders: Vec<Order>,
    pub positions: Vec<Position>,
    pub pnl: HashMap<String, PositionPnl>,
    pub balances: Vec<Balance>,
    pub capabilities: Option<TradingCapabilities>,
    pub is_paper: bool,
    pub recent_fills: VecDeque<Fill>,
    pub last_error: Option<String>,
    pub order_in_flight: bool,
}

impl Default for TradingSnapshot {
    fn default() -> Self {
        Self {
            session: None,
            open_orders: Vec::new(),
            positions: Vec::new(),
            pnl: HashMap::new(),
            balances: Vec::new(),
            capabilities: None,
            is_paper: false,
            recent_fills: VecDeque::new(),
            last_error: None,
            order_in_flight: false,
        }
    }
}

pub type SharedTradingSnapshot = Arc<std::sync::RwLock<TradingSnapshot>>;

impl TradingSnapshot {
    pub fn new_shared() -> SharedTradingSnapshot {
        Arc::new(std::sync::RwLock::new(Self::default()))
    }
}
