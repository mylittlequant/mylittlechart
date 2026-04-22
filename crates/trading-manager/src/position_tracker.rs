use std::collections::HashMap;
use crate::types::{Position, Fill, PositionPnl};

pub struct PositionTracker {
    positions: HashMap<String, Position>,
    realized_pnl: HashMap<String, f64>,
}

impl PositionTracker {
    pub fn new() -> Self {
        Self {
            positions: HashMap::new(),
            realized_pnl: HashMap::new(),
        }
    }

    pub fn apply_fill(&mut self, fill: &Fill) {
        let _ = fill;
    }

    pub fn update_positions(&mut self, positions: Vec<Position>) {
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
