use serde::{Serialize, Deserialize};
use std::collections::VecDeque;

/// TimeSales panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimeSalesId(pub u64);

/// TimeSales panel state (heavy data)
#[derive(Clone, Debug)]
pub struct TimeSalesState {
    /// Symbol being monitored
    pub symbol: String,
    /// Ring buffer of recent trades (max_trades capacity)
    pub trades: VecDeque<PublicTrade>,
    /// Auto-scroll enabled
    pub auto_scroll: bool,
    /// Filter configuration
    pub filter: Option<TimeSalesFilter>,
    /// Flash animation state for new trades
    pub flash_trades: Vec<(usize, u64)>,  // (trade_index, flash_start_ms)
    /// Market price from linked DOM (for price highlighting)
    pub dom_market_price: Option<f64>,
    /// Tick size from linked DOM
    pub dom_tick_size: Option<f64>,
    /// Scroll offset for the tape
    pub scroll_offset: f64,
}

#[derive(Clone, Debug)]
pub struct TimeSalesFilter {
    /// Minimum trade size
    pub min_size: Option<f64>,
    /// Side filter (Buy/Sell/Both)
    pub side: Option<TradeSide>,
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

impl TimeSalesState {
    pub fn new() -> Self {
        Self {
            symbol: String::new(),
            trades: VecDeque::new(),
            auto_scroll: true,
            filter: None,
            flash_trades: Vec::new(),
            dom_market_price: None,
            dom_tick_size: None,
            scroll_offset: 0.0,
        }
    }

    /// Get visible trades for rendering (most recent first)
    pub fn visible_trades(&self, max_count: usize) -> Vec<&PublicTrade> {
        self.trades.iter().rev().take(max_count).collect()
    }

    /// Format trade for display
    pub fn format_trade(&self, trade: &PublicTrade) -> (String, String, String, String) {
        let time = format_timestamp(trade.timestamp);
        let price = format!("{:.4}", trade.price);
        let quantity = format!("{:.4}", trade.quantity);
        let side = match trade.side {
            TradeSide::Buy => "BUY",
            TradeSide::Sell => "SELL",
        };
        (time, price, quantity, side.to_string())
    }

    /// Get color based on trade side
    pub fn trade_color(&self, trade: &PublicTrade) -> [f32; 4] {
        match trade.side {
            TradeSide::Buy => [0.2, 0.8, 0.3, 1.0],  // green
            TradeSide::Sell => [0.9, 0.2, 0.2, 1.0], // red
        }
    }
}

fn format_timestamp(ts: i64) -> String {
    // Simple HH:MM:SS format (would use chrono in real impl)
    let secs = (ts / 1000) % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

/// TimeSales panel configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeSalesConfig {
    /// Maximum trades to keep in memory
    pub max_trades: usize,
    /// Show milliseconds in timestamp
    pub show_milliseconds: bool,
    /// Color-code by side
    pub color_by_side: bool,
}

/// TimeSales panel wrapper (lightweight, lives in PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimeSalesPanel {
    id: TimeSalesId,
    title: String,
}

impl TimeSalesPanel {
    pub fn new(id: TimeSalesId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> TimeSalesId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "time_sales" }
    pub fn kind_label(&self) -> &'static str { "Time & Sales" }
    pub fn min_size(&self) -> (f32, f32) { (200.0, 200.0) }
}
