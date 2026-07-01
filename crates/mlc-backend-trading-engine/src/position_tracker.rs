use std::collections::HashMap;
use crate::types::{Position, Fill, PositionPnl, PositionSide, OrderSide, MarginType};

pub struct PositionTracker {
    positions: HashMap<String, Position>,
    realized_pnl: HashMap<String, f64>,
    is_paper: bool,
}

impl PositionTracker {
    pub fn new(is_paper: bool) -> Self {
        Self {
            positions: HashMap::new(),
            realized_pnl: HashMap::new(),
            is_paper,
        }
    }

    pub fn apply_fill(&mut self, fill: &Fill) {
        if !self.is_paper {
            // Live mode: only accumulate realized PnL; exchange owns positions.
            if let Some(pos) = self.positions.get(&fill.symbol) {
                let closing = matches!(
                    (&pos.side, &fill.side),
                    (PositionSide::Long, OrderSide::Sell) | (PositionSide::Short, OrderSide::Buy)
                );
                if closing {
                    let close_qty = fill.quantity.min(pos.quantity);
                    let pnl = match pos.side {
                        PositionSide::Long => close_qty * (fill.price - pos.entry_price),
                        PositionSide::Short => close_qty * (pos.entry_price - fill.price),
                        PositionSide::Both => 0.0,
                    };
                    *self.realized_pnl.entry(fill.symbol.clone()).or_insert(0.0) += pnl;
                }
            }
            return;
        }

        // Paper mode: build positions from fills.
        match self.positions.get(&fill.symbol).cloned() {
            None => {
                let side = match fill.side {
                    OrderSide::Buy => PositionSide::Long,
                    OrderSide::Sell => PositionSide::Short,
                };
                self.positions.insert(fill.symbol.clone(), Position {
                    symbol: fill.symbol.clone(),
                    side,
                    quantity: fill.quantity,
                    entry_price: fill.price,
                    mark_price: None,
                    unrealized_pnl: 0.0,
                    realized_pnl: None,
                    liquidation_price: None,
                    leverage: 1,
                    margin_type: MarginType::Cross,
                    margin: None,
                    take_profit: None,
                    stop_loss: None,
                });
            }
            Some(pos) => {
                let same_direction = matches!(
                    (&pos.side, &fill.side),
                    (PositionSide::Long, OrderSide::Buy) | (PositionSide::Short, OrderSide::Sell)
                );

                if same_direction {
                    let new_qty = pos.quantity + fill.quantity;
                    let new_entry = (pos.entry_price * pos.quantity + fill.price * fill.quantity) / new_qty;
                    let p = self.positions.get_mut(&fill.symbol).expect("just confirmed present");
                    p.quantity = new_qty;
                    p.entry_price = new_entry;
                } else {
                    let close_qty = fill.quantity.min(pos.quantity);
                    let pnl = match pos.side {
                        PositionSide::Long => close_qty * (fill.price - pos.entry_price),
                        PositionSide::Short => close_qty * (pos.entry_price - fill.price),
                        PositionSide::Both => 0.0,
                    };
                    *self.realized_pnl.entry(fill.symbol.clone()).or_insert(0.0) += pnl;

                    let remaining = pos.quantity - close_qty;
                    if remaining == 0.0 {
                        self.positions.remove(&fill.symbol);
                    } else {
                        let p = self.positions.get_mut(&fill.symbol).expect("just confirmed present");
                        p.quantity = remaining;
                    }

                    let leftover = fill.quantity - close_qty;
                    if leftover > 0.0 {
                        let new_side = match fill.side {
                            OrderSide::Buy => PositionSide::Long,
                            OrderSide::Sell => PositionSide::Short,
                        };
                        self.positions.insert(fill.symbol.clone(), Position {
                            symbol: fill.symbol.clone(),
                            side: new_side,
                            quantity: leftover,
                            entry_price: fill.price,
                            mark_price: None,
                            unrealized_pnl: 0.0,
                            realized_pnl: None,
                            liquidation_price: None,
                            leverage: 1,
                            margin_type: MarginType::Cross,
                            margin: None,
                            take_profit: None,
                            stop_loss: None,
                        });
                    }
                }
            }
        }
    }

    pub fn update_mark_price(&mut self, symbol: &str, price: f64) {
        if let Some(pos) = self.positions.get_mut(symbol) {
            pos.mark_price = Some(price);
            pos.unrealized_pnl = match pos.side {
                PositionSide::Long => (price - pos.entry_price) * pos.quantity,
                PositionSide::Short => (pos.entry_price - price) * pos.quantity,
                PositionSide::Both => 0.0,
            };
        }
    }

    pub fn update_positions(&mut self, positions: Vec<Position>) {
        if self.is_paper {
            return;
        }
        self.positions.clear();
        for p in positions {
            self.positions.insert(p.symbol.clone(), p);
        }
    }

    pub fn all_positions(&self) -> &HashMap<String, Position> {
        &self.positions
    }

    pub fn all_pnl(&self) -> HashMap<String, PositionPnl> {
        self.positions.iter().map(|(sym, pos)| {
            (sym.clone(), PositionPnl {
                unrealized: pos.unrealized_pnl,
                realized: self.realized_pnl.get(sym).copied().unwrap_or(0.0),
                entry_price: pos.entry_price,
                current_price: pos.mark_price.unwrap_or(pos.entry_price),
            })
        }).collect()
    }
}
