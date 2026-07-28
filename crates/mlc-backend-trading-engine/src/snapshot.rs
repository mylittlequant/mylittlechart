use std::sync::Arc;
use std::collections::{HashMap, VecDeque};
use crate::types::{
    Order, Position, Balance, TradingCapabilities, Fill, PositionPnl, SessionInfo,
    ClosedPosition,
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
    /// Bounded (100) log of orders that reached a terminal state (Filled /
    /// Canceled / Rejected / Expired), newest last — the orders-log "history"
    /// tab (distinct from `recent_fills`, which is fill-level, not
    /// order-level: one order can carry multiple fills, and a canceled order
    /// never fills at all). Fed by `PaperEngine::history` for paper sessions,
    /// by `TradingManager`'s own bucket for live sessions.
    pub order_history: VecDeque<Order>,
    /// Bounded (100) log of position-close events (each partial or full
    /// close is one record), newest last — the Positions tab's "History"
    /// section. Fed by `PositionTracker::apply_fill` for both paper and
    /// live sessions.
    pub position_history: VecDeque<ClosedPosition>,
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
            order_history: VecDeque::new(),
            position_history: VecDeque::new(),
        }
    }
}

pub type SharedTradingSnapshot = Arc<std::sync::RwLock<TradingSnapshot>>;

impl TradingSnapshot {
    pub fn new_shared() -> SharedTradingSnapshot {
        Arc::new(std::sync::RwLock::new(Self::default()))
    }
}
