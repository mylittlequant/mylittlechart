//! Trade visualization — displays completed trades on chart.
//!
//! After K2.4 `TradeManager` is a **stateless dispatcher** that lives in
//! `Managers.trade`.  All trade data (`trades` map, `trade_next_id`)
//! lives on the `DataBubble` data slice.
//!
//! Every mutating method accepts `&mut DataBubble` (or `&DataBubble` for
//! reads) and writes directly to the bubble.  No method reaches back to a
//! `trade_manager` field — that field is gone.
//!
//! Phase 2A of the Tag/Bubble/UiContainer cutover removed the
//! `trades_visible` + `trade_show_lines` flags + the dead-end
//! `is_visible/set_visible/show_lines/set_show_lines` wrappers — there
//! was no `render()` and no UI toggle wired to them.

use serde::{Deserialize, Serialize};

/// Trade direction
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[derive(Default)]
pub enum TradeDirection {
    #[default]
    Long,
    Short,
}


impl TradeDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Long => "long",
            Self::Short => "short",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "long" | "buy" => Self::Long,
            "short" | "sell" => Self::Short,
            _ => Self::Long,
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Long => "▲",
            Self::Short => "▼",
        }
    }
}

/// A completed trade for visualization
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Trade {
    /// Unique trade ID
    pub id: u64,
    /// Trade direction
    pub direction: TradeDirection,
    /// Entry bar index
    pub entry_bar: f64,
    /// Entry price
    pub entry_price: f64,
    /// Exit bar index
    pub exit_bar: f64,
    /// Exit price
    pub exit_price: f64,
    /// Profit/Loss (from nemo)
    pub pnl: f64,
    /// Strategy tag
    pub strategy_tag: String,
    /// Visibility
    pub visible: bool,
}

impl Trade {
    pub fn new(
        id: u64,
        direction: TradeDirection,
        entry_bar: f64,
        entry_price: f64,
        exit_bar: f64,
        exit_price: f64,
        pnl: f64,
        strategy_tag: &str,
    ) -> Self {
        Self {
            id,
            direction,
            entry_bar,
            entry_price,
            exit_bar,
            exit_price,
            pnl,
            strategy_tag: strategy_tag.to_string(),
            visible: true,
        }
    }

    /// Check if trade is profitable
    pub fn is_profitable(&self) -> bool {
        self.pnl > 0.0
    }

    /// Get duration in bars
    pub fn duration_bars(&self) -> f64 {
        self.exit_bar - self.entry_bar
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}

/// Global service for trade visualization annotations.
///
/// Stateless: all trade data lives on the [`crate::tag_manager::DataBubble`]
/// data slice (`bubble.trades`, `bubble.trade_next_id`).
/// Lives in [`crate::managers::Managers::trade`].
#[derive(Clone, Debug, Default)]
pub struct TradeManager;

impl TradeManager {
    pub fn new() -> Self {
        Self
    }

    // =========================================================================
    // Trade Management
    // =========================================================================

    /// Add a trade. Returns the assigned trade ID.
    pub fn add_trade(
        &self,
        bubble: &mut crate::tag_manager::DataBubble,
        direction: TradeDirection,
        entry_bar: f64,
        entry_price: f64,
        exit_bar: f64,
        exit_price: f64,
        pnl: f64,
        strategy_tag: &str,
    ) -> u64 {
        let id = bubble.trade_next_id;
        bubble.trade_next_id += 1;

        let trade = Trade::new(
            id,
            direction,
            entry_bar,
            entry_price,
            exit_bar,
            exit_price,
            pnl,
            strategy_tag,
        );

        bubble.trades.insert(id, trade);
        id
    }

    /// Remove a trade by ID.
    pub fn remove_trade(&self, bubble: &mut crate::tag_manager::DataBubble, id: u64) -> Option<Trade> {
        bubble.trades.remove(&id)
    }

    /// Remove all trades belonging to a strategy.
    pub fn remove_strategy_trades(&self, bubble: &mut crate::tag_manager::DataBubble, strategy_tag: &str) {
        bubble.trades.retain(|_, t| t.strategy_tag != strategy_tag);
    }

    /// Clear all trades from a bubble.
    pub fn clear(&self, bubble: &mut crate::tag_manager::DataBubble) {
        bubble.trades.clear();
    }

    /// Get trade by ID (immutable).
    pub fn get<'b>(&self, bubble: &'b crate::tag_manager::DataBubble, id: u64) -> Option<&'b Trade> {
        bubble.trades.get(&id)
    }

    /// Iterate over all trades in a bubble.
    pub fn trades<'b>(&self, bubble: &'b crate::tag_manager::DataBubble) -> impl Iterator<Item = &'b Trade> {
        bubble.trades.values()
    }

    /// Iterate over trades belonging to a specific strategy.
    pub fn trades_for_strategy<'b>(
        &self,
        bubble: &'b crate::tag_manager::DataBubble,
        strategy_tag: &'b str,
    ) -> impl Iterator<Item = &'b Trade> + 'b {
        bubble.trades.values().filter(move |t| t.strategy_tag == strategy_tag)
    }

    /// Total trade count in a bubble.
    pub fn count(&self, bubble: &crate::tag_manager::DataBubble) -> usize {
        bubble.trades.len()
    }

    // =========================================================================
    // Statistics
    // =========================================================================

    /// Get total PnL across all trades in a bubble.
    pub fn total_pnl(&self, bubble: &crate::tag_manager::DataBubble) -> f64 {
        bubble.trades.values().map(|t| t.pnl).sum()
    }

    /// Get win count (PnL > 0).
    pub fn win_count(&self, bubble: &crate::tag_manager::DataBubble) -> usize {
        bubble.trades.values().filter(|t| t.pnl > 0.0).count()
    }

    /// Get loss count (PnL <= 0).
    pub fn loss_count(&self, bubble: &crate::tag_manager::DataBubble) -> usize {
        bubble.trades.values().filter(|t| t.pnl <= 0.0).count()
    }

    /// Get win rate (0.0 – 1.0).
    pub fn win_rate(&self, bubble: &crate::tag_manager::DataBubble) -> f64 {
        let total = bubble.trades.len();
        if total == 0 {
            return 0.0;
        }
        self.win_count(bubble) as f64 / total as f64
    }

    // =========================================================================
    // Serialization
    // =========================================================================

    /// Get all trades as JSON array.
    pub fn to_json(&self, bubble: &crate::tag_manager::DataBubble) -> String {
        let trades: Vec<String> = bubble.trades.values()
            .map(|t| t.to_json())
            .collect();
        format!("[{}]", trades.join(","))
    }

    /// Load trades from JSON array. Advances `trade_next_id` past loaded ids.
    pub fn load_json(&self, bubble: &mut crate::tag_manager::DataBubble, json: &str) {
        if let Ok(trades) = serde_json::from_str::<Vec<Trade>>(json) {
            for trade in trades {
                let id = trade.id;
                if id >= bubble.trade_next_id {
                    bubble.trade_next_id = id + 1;
                }
                bubble.trades.insert(id, trade);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tag_manager::BubbleStore;

    fn make_bubble() -> crate::tag_manager::DataBubble {
        let mut store = BubbleStore::new();
        let id = store.create_group(
            [0.0; 4],
            "BTCUSDT".to_string(),
            crate::state::Timeframe::h1(),
        );
        store.bubble_mut(id).unwrap().clone()
    }

    #[test]
    fn test_add_trade() {
        let manager = TradeManager::new();
        let mut bubble = make_bubble();
        let id = manager.add_trade(
            &mut bubble,
            TradeDirection::Long,
            100.0, 50000.0,
            110.0, 51000.0,
            1000.0,
            "test_strategy",
        );
        assert_eq!(id, 1);
        assert_eq!(manager.count(&bubble), 1);
    }

    #[test]
    fn test_trade_stats() {
        let manager = TradeManager::new();
        let mut bubble = make_bubble();
        manager.add_trade(&mut bubble, TradeDirection::Long, 100.0, 50000.0, 110.0, 51000.0, 1000.0, "test");
        manager.add_trade(&mut bubble, TradeDirection::Long, 120.0, 51000.0, 130.0, 50500.0, -500.0, "test");
        manager.add_trade(&mut bubble, TradeDirection::Short, 140.0, 50500.0, 150.0, 49500.0, 1000.0, "test");

        assert_eq!(manager.count(&bubble), 3);
        assert_eq!(manager.win_count(&bubble), 2);
        assert_eq!(manager.loss_count(&bubble), 1);
        assert!((manager.win_rate(&bubble) - 0.6666).abs() < 0.01);
        assert!((manager.total_pnl(&bubble) - 1500.0).abs() < 0.01);
    }

    #[test]
    fn test_trade_serialization() {
        let trade = Trade::new(
            1, TradeDirection::Long,
            100.0, 50000.0, 110.0, 51000.0,
            1000.0, "test",
        );
        let json = trade.to_json();
        let restored = Trade::from_json(&json).unwrap();
        assert_eq!(restored.pnl, 1000.0);
    }
}
