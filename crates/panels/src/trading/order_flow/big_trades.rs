use serde::{Serialize, Deserialize};
use std::collections::VecDeque;

/// BigTrades panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BigTradesId(pub u64);

/// BigTrades panel state (heavy data)
#[derive(Clone, Debug)]
pub struct BigTradesState {
    /// Symbol being monitored
    pub symbol: String,
    /// Ring buffer of large trades
    pub big_trades: VecDeque<PublicTrade>,
    /// Threshold for "big" trade
    pub size_threshold: f64,
    /// Notional threshold (price * size)
    pub notional_threshold: Option<f64>,
    /// Flash animation state for new trades
    pub flash_trades: Vec<(usize, u64)>,  // (trade_index, flash_start_ms)
    /// Market price from linked DOM
    pub dom_market_price: Option<f64>,
    /// Tick size from linked DOM
    pub dom_tick_size: Option<f64>,
    /// Scroll offset
    pub scroll_offset: f64,
}

#[derive(Clone, Debug)]
pub struct PublicTrade {
    pub timestamp: i64,
    pub price: f64,
    pub quantity: f64,
    pub side: TradeSide,
}

#[derive(Clone, Debug)]
pub enum TradeSide {
    Buy,
    Sell,
}

impl BigTradesState {
    pub fn new() -> Self {
        Self {
            symbol: String::new(),
            big_trades: VecDeque::new(),
            size_threshold: 0.0,
            notional_threshold: None,
            flash_trades: Vec::new(),
            dom_market_price: None,
            dom_tick_size: None,
            scroll_offset: 0.0,
        }
    }

    /// Get visible trades for rendering (most recent first)
    pub fn visible_trades(&self, max_count: usize) -> Vec<&PublicTrade> {
        self.big_trades.iter().rev().take(max_count).collect()
    }

    /// Format trade for display with notional value
    pub fn format_trade(&self, trade: &PublicTrade) -> (String, String, String, String, String) {
        let time = format_timestamp(trade.timestamp);
        let price = format!("{:.4}", trade.price);
        let quantity = format!("{:.4}", trade.quantity);
        let notional = format!("{:.2}", trade.price * trade.quantity);
        let side = match trade.side {
            TradeSide::Buy => "BUY",
            TradeSide::Sell => "SELL",
        };
        (time, price, quantity, notional, side.to_string())
    }

    /// Get color based on trade side with intensity based on size
    pub fn trade_color(&self, trade: &PublicTrade) -> [f32; 4] {
        let base_color = match trade.side {
            TradeSide::Buy => [0.2, 0.8, 0.3, 1.0],
            TradeSide::Sell => [0.9, 0.2, 0.2, 1.0],
        };

        // Could adjust intensity based on size relative to threshold
        // For now, return base color
        base_color
    }

    /// Apply a live trade — only keeps trades above the size threshold
    pub fn push_trade(&mut self, price: f64, quantity: f64, is_buyer_maker: bool, timestamp: i64) {
        if quantity < self.size_threshold {
            return;
        }

        let trade = PublicTrade {
            price,
            quantity,
            side: if is_buyer_maker { TradeSide::Sell } else { TradeSide::Buy },
            timestamp,
        };

        // Cap ring buffer at a fixed maximum to prevent unbounded growth
        const MAX_TRADES: usize = 1000;
        if self.big_trades.len() >= MAX_TRADES {
            self.big_trades.pop_front();
        }
        self.big_trades.push_back(trade);
    }

    /// Calculate bar width for size visualization (0.0-1.0)
    pub fn size_bar_width(&self, trade: &PublicTrade, max_width: f32) -> f32 {
        if self.size_threshold == 0.0 {
            return 0.0;
        }

        let ratio = (trade.quantity / self.size_threshold).min(3.0); // Cap at 3x threshold
        (ratio as f32 / 3.0) * max_width
    }
}

fn format_timestamp(ts: i64) -> String {
    let secs = (ts / 1000) % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

/// BigTrades panel configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BigTradesConfig {
    /// Maximum trades to display
    pub max_trades: usize,
    /// Default size threshold
    pub default_size_threshold: f64,
    /// Use notional value instead of size
    pub use_notional: bool,
    /// Alert on big trade
    pub alert_enabled: bool,
    /// Alert sound
    pub alert_sound: Option<String>,
}

/// BigTrades panel wrapper (lightweight, lives in PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BigTradesPanel {
    id: BigTradesId,
    title: String,
}

impl BigTradesPanel {
    pub fn new(id: BigTradesId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> BigTradesId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "big_trades" }
    pub fn kind_label(&self) -> &'static str { "Big Trades" }
    pub fn min_size(&self) -> (f32, f32) { (250.0, 200.0) }
}
