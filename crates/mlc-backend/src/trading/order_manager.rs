//! In-memory order registry, keyed by exchange order id.
//!
//! Stateful sub-manager owned by [`super::manager::TradingManager`] (one per
//! `(ExchangeId, AccountType)` session) and by
//! [`super::paper_engine::PaperEngine`] (paper session's own order book).
//! Pure registry — no compute, no IO.

use std::collections::HashMap;
use mlc_backend_trading_engine::{Order, OrderStatus};

pub struct OrderManager {
    orders: HashMap<String, Order>,
    next_id: u64,
    prefix: String,
}

impl OrderManager {
    pub fn new(prefix: &str) -> Self {
        Self {
            orders: HashMap::new(),
            next_id: 1,
            prefix: prefix.to_string(),
        }
    }

    pub fn next_client_id(&mut self) -> String {
        let id = format!("{}-{}", self.prefix, self.next_id);
        self.next_id += 1;
        id
    }

    pub fn upsert(&mut self, order: Order) {
        self.orders.insert(order.id.clone(), order);
    }

    pub fn remove(&mut self, order_id: &str) -> Option<Order> {
        self.orders.remove(order_id)
    }

    pub fn open_orders(&self) -> impl Iterator<Item = &Order> {
        self.orders.values().filter(|o| {
            matches!(o.status, OrderStatus::New | OrderStatus::PartiallyFilled)
        })
    }
}
