use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::types::{
    ExchangeId, AccountType, OrderRequest, OrderType, OrderSide,
    Order, OrderStatus, Fill,
};
use crate::order_manager::OrderManager;
use crate::position_tracker::PositionTracker;
use crate::error::{TradingError, TradingResult};
use orderbook_service::{SharedOrderbookMap, OrderbookKey};

struct PendingOrder {
    order: Order,
    stop_price: Option<f64>,
    limit_price: Option<f64>,
}

pub struct PaperEngine {
    exchange_id: ExchangeId,
    account_type: AccountType,
    orderbook: SharedOrderbookMap,
    pending: Vec<PendingOrder>,
    balances: HashMap<String, f64>,
    orders: OrderManager,
    positions: PositionTracker,
    fill_tx: tokio::sync::mpsc::UnboundedSender<Fill>,
    next_order_id: u64,
}

impl PaperEngine {
    pub fn new(
        exchange_id: ExchangeId,
        account_type: AccountType,
        orderbook: SharedOrderbookMap,
        initial_balances: HashMap<String, f64>,
        fill_tx: tokio::sync::mpsc::UnboundedSender<Fill>,
    ) -> Self {
        Self {
            exchange_id,
            account_type,
            orderbook,
            pending: Vec::new(),
            balances: initial_balances,
            orders: OrderManager::new("paper"),
            positions: PositionTracker::new(true),
            fill_tx,
            next_order_id: 1,
        }
    }

    pub fn place_order_sync(&mut self, req: OrderRequest) -> TradingResult<Order> {
        let order_id = format!("PAPER-{}", self.next_order_id);
        self.next_order_id += 1;

        let client_order_id = req.client_order_id
            .clone()
            .unwrap_or_else(|| self.orders.next_client_id());

        let now = now_ms();
        let sym_str = sym_to_string(&req.symbol);
        let base = req.symbol.base.clone();
        let quote = req.symbol.quote.clone();

        match req.order_type.clone() {
            OrderType::Market => {
                let (best_bid, best_ask) = self.get_orderbook_price(&sym_str)
                    .ok_or_else(|| TradingError::Paper("no orderbook data".into()))?;

                let fill_price = match req.side {
                    OrderSide::Buy => best_ask,
                    OrderSide::Sell => best_bid,
                };

                self.check_and_deduct_balance(req.side, fill_price, req.quantity, &base, &quote)?;

                let fee = fill_price * req.quantity * 0.001;
                match req.side {
                    OrderSide::Buy => {
                        *self.balances.entry(base.clone()).or_insert(0.0) += req.quantity;
                    }
                    OrderSide::Sell => {
                        let proceeds = fill_price * req.quantity - fee;
                        *self.balances.entry(quote.clone()).or_insert(0.0) += proceeds;
                    }
                }

                let fill = Fill {
                    order_id: order_id.clone(),
                    symbol: sym_str.clone(),
                    side: req.side,
                    price: fill_price,
                    quantity: req.quantity,
                    fee,
                    fee_asset: quote.clone(),
                    timestamp: now,
                    is_paper: true,
                };
                let _ = self.fill_tx.send(fill.clone());
                self.positions.apply_fill(&fill);

                let order = Order {
                    id: order_id,
                    client_order_id: Some(client_order_id),
                    symbol: sym_str,
                    side: req.side,
                    order_type: req.order_type,
                    status: OrderStatus::Filled,
                    price: Some(fill_price),
                    stop_price: None,
                    quantity: req.quantity,
                    filled_quantity: req.quantity,
                    average_price: Some(fill_price),
                    commission: Some(fee),
                    commission_asset: Some(quote),
                    created_at: now,
                    updated_at: Some(now),
                    time_in_force: req.time_in_force,
                };
                self.orders.upsert(order.clone());
                Ok(order)
            }

            OrderType::Limit { price } => {
                self.check_and_deduct_balance(req.side, price, req.quantity, &base, &quote)?;

                let order = Order {
                    id: order_id.clone(),
                    client_order_id: Some(client_order_id),
                    symbol: sym_str,
                    side: req.side,
                    order_type: req.order_type,
                    status: OrderStatus::New,
                    price: Some(price),
                    stop_price: None,
                    quantity: req.quantity,
                    filled_quantity: 0.0,
                    average_price: None,
                    commission: None,
                    commission_asset: None,
                    created_at: now,
                    updated_at: None,
                    time_in_force: req.time_in_force,
                };
                self.orders.upsert(order.clone());
                self.pending.push(PendingOrder {
                    order: order.clone(),
                    stop_price: None,
                    limit_price: Some(price),
                });
                Ok(order)
            }

            OrderType::StopMarket { stop_price } => {
                let order = Order {
                    id: order_id.clone(),
                    client_order_id: Some(client_order_id),
                    symbol: sym_str,
                    side: req.side,
                    order_type: req.order_type,
                    status: OrderStatus::New,
                    price: None,
                    stop_price: Some(stop_price),
                    quantity: req.quantity,
                    filled_quantity: 0.0,
                    average_price: None,
                    commission: None,
                    commission_asset: None,
                    created_at: now,
                    updated_at: None,
                    time_in_force: req.time_in_force,
                };
                self.orders.upsert(order.clone());
                self.pending.push(PendingOrder {
                    order: order.clone(),
                    stop_price: Some(stop_price),
                    limit_price: None,
                });
                Ok(order)
            }

            OrderType::StopLimit { stop_price, limit_price } => {
                let order = Order {
                    id: order_id.clone(),
                    client_order_id: Some(client_order_id),
                    symbol: sym_str,
                    side: req.side,
                    order_type: req.order_type,
                    status: OrderStatus::New,
                    price: Some(limit_price),
                    stop_price: Some(stop_price),
                    quantity: req.quantity,
                    filled_quantity: 0.0,
                    average_price: None,
                    commission: None,
                    commission_asset: None,
                    created_at: now,
                    updated_at: None,
                    time_in_force: req.time_in_force,
                };
                self.orders.upsert(order.clone());
                self.pending.push(PendingOrder {
                    order: order.clone(),
                    stop_price: Some(stop_price),
                    limit_price: Some(limit_price),
                });
                Ok(order)
            }

            _ => Err(TradingError::Paper("unsupported order type for paper trading".into())),
        }
    }

    pub fn tick(&mut self) {
        let symbols: Vec<String> = self.pending.iter()
            .map(|p| p.order.symbol.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let mut prices: HashMap<String, (f64, f64)> = HashMap::new();
        for sym in &symbols {
            if let Some(p) = self.get_orderbook_price(sym) {
                prices.insert(sym.clone(), p);
            }
        }

        let now = now_ms();
        let mut to_fill: Vec<usize> = Vec::new();

        for (i, pending) in self.pending.iter_mut().enumerate() {
            let sym = &pending.order.symbol;
            let Some(&(best_bid, best_ask)) = prices.get(sym) else { continue };

            match (pending.stop_price, pending.limit_price) {
                (None, Some(limit_price)) => {
                    let fills = match pending.order.side {
                        OrderSide::Buy => best_ask <= limit_price,
                        OrderSide::Sell => best_bid >= limit_price,
                    };
                    if fills {
                        to_fill.push(i);
                    }
                }
                (Some(stop_price), None) => {
                    let triggered = match pending.order.side {
                        OrderSide::Buy => best_ask >= stop_price,
                        OrderSide::Sell => best_bid <= stop_price,
                    };
                    if triggered {
                        to_fill.push(i);
                    }
                }
                (Some(stop_price), Some(_)) => {
                    let triggered = match pending.order.side {
                        OrderSide::Buy => best_ask >= stop_price,
                        OrderSide::Sell => best_bid <= stop_price,
                    };
                    if triggered {
                        pending.stop_price = None;
                        pending.order.stop_price = None;
                        pending.order.updated_at = Some(now);
                        self.orders.upsert(pending.order.clone());
                    }
                }
                (None, None) => {}
            }
        }

        for i in to_fill.into_iter().rev() {
            let pending = self.pending.remove(i);
            let sym = &pending.order.symbol;
            let Some(&(best_bid, best_ask)) = prices.get(sym) else { continue };
            let (base, quote) = parse_assets(sym);

            let fill_price = match pending.limit_price {
                Some(lp) => lp,
                None => match pending.order.side {
                    OrderSide::Buy => best_ask,
                    OrderSide::Sell => best_bid,
                },
            };

            let fee = fill_price * pending.order.quantity * 0.001;
            match pending.order.side {
                OrderSide::Buy => {
                    *self.balances.entry(base).or_insert(0.0) += pending.order.quantity;
                }
                OrderSide::Sell => {
                    let proceeds = fill_price * pending.order.quantity - fee;
                    *self.balances.entry(quote.clone()).or_insert(0.0) += proceeds;
                }
            }

            let fill = Fill {
                order_id: pending.order.id.clone(),
                symbol: sym.to_string(),
                side: pending.order.side,
                price: fill_price,
                quantity: pending.order.quantity,
                fee,
                fee_asset: quote,
                timestamp: now,
                is_paper: true,
            };
            let _ = self.fill_tx.send(fill.clone());
            self.positions.apply_fill(&fill);

            let mut order = pending.order;
            order.status = OrderStatus::Filled;
            order.filled_quantity = order.quantity;
            order.average_price = Some(fill_price);
            order.commission = Some(fee);
            order.updated_at = Some(now);
            self.orders.upsert(order);
        }

        for (sym, (best_bid, best_ask)) in &prices {
            let mid = (best_bid + best_ask) / 2.0;
            self.positions.update_mark_price(sym, mid);
        }
    }

    pub fn cancel_order(&mut self, order_id: &str) -> TradingResult<Order> {
        let pos = self.pending.iter().position(|p| p.order.id == order_id);
        let Some(idx) = pos else {
            return Err(TradingError::OrderNotFound { client_id: order_id.into() });
        };

        let pending = self.pending.remove(idx);
        let (base, quote) = parse_assets(&pending.order.symbol);

        if let Some(lp) = pending.limit_price {
            match pending.order.side {
                OrderSide::Buy => {
                    *self.balances.entry(quote).or_insert(0.0) += lp * pending.order.quantity;
                }
                OrderSide::Sell => {
                    *self.balances.entry(base).or_insert(0.0) += pending.order.quantity;
                }
            }
        }

        let mut order = pending.order;
        order.status = OrderStatus::Canceled;
        order.updated_at = Some(now_ms());
        self.orders.upsert(order.clone());
        Ok(order)
    }

    pub fn balances(&self) -> &HashMap<String, f64> {
        &self.balances
    }

    pub fn positions(&self) -> &PositionTracker {
        &self.positions
    }

    pub fn orders(&self) -> &OrderManager {
        &self.orders
    }

    fn get_orderbook_price(&self, symbol: &str) -> Option<(f64, f64)> {
        let key = OrderbookKey::new(self.exchange_id, self.account_type, symbol);
        let map = self.orderbook.read().ok()?;
        let series_arc = map.get(&key)?.clone();
        drop(map);
        let series = series_arc.read().ok()?;
        let bid = series.current.best_bid()?;
        let ask = series.current.best_ask()?;
        Some((bid, ask))
    }

    fn check_and_deduct_balance(
        &mut self,
        side: OrderSide,
        price: f64,
        quantity: f64,
        base: &str,
        quote: &str,
    ) -> TradingResult<()> {
        match side {
            OrderSide::Buy => {
                let cost = price * quantity;
                let available = self.balances.get(quote).copied().unwrap_or(0.0);
                if available < cost {
                    return Err(TradingError::Paper("insufficient balance".into()));
                }
                *self.balances.entry(quote.to_string()).or_insert(0.0) -= cost;
            }
            OrderSide::Sell => {
                let available = self.balances.get(base).copied().unwrap_or(0.0);
                if available < quantity {
                    return Err(TradingError::Paper("insufficient balance".into()));
                }
                *self.balances.entry(base.to_string()).or_insert(0.0) -= quantity;
            }
        }
        Ok(())
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn sym_to_string(sym: &crate::types::Symbol) -> String {
    sym.raw().map(|s| s.to_string()).unwrap_or_else(|| sym.to_concat())
}

fn parse_assets(symbol: &str) -> (String, String) {
    for suffix in &["USDT", "USDC", "BUSD", "BTC", "ETH", "BNB"] {
        if symbol.ends_with(suffix) {
            let base = &symbol[..symbol.len() - suffix.len()];
            return (base.to_string(), (*suffix).to_string());
        }
    }
    (symbol.to_string(), "USDT".to_string())
}
