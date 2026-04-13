use serde::{Serialize, Deserialize};

/// TradeLog panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TradeLogId(pub u64);

/// TradeLog panel state (heavy data)
#[derive(Clone, Debug)]
pub struct TradeLogState {
    /// List of user's executed trades
    pub trades: Vec<UserTrade>,
    /// Time range filter
    pub time_range: TimeRange,
    /// Symbol filter (optional)
    pub symbol_filter: Option<String>,
    /// Total PnL (computed)
    pub total_pnl: f64,
    /// Sort configuration
    pub sort: (TradeLogColumn, bool),
}

#[derive(Clone, Debug)]
pub struct UserTrade {
    pub timestamp: i64,
    pub symbol: String,
    pub side: OrderSide,
    pub price: f64,
    pub quantity: f64,
    pub commission: f64,
}

#[derive(Clone, Debug)]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Clone, Debug)]
pub enum TimeRange {
    Today,
    Week,
    Month,
    All,
    Custom(i64, i64),
}

#[derive(Clone, Debug, Copy)]
pub enum TradeLogColumn {
    Time,
    Symbol,
    Side,
    Type,
    Price,
    Quantity,
    Commission,
    PnL,
}

impl TradeLogState {
    pub fn new() -> Self {
        Self {
            trades: Vec::new(),
            time_range: TimeRange::Today,
            symbol_filter: None,
            total_pnl: 0.0,
            sort: (TradeLogColumn::Time, false),
        }
    }

    /// Get visible trades for rendering
    pub fn visible_trades(&self, scroll_offset: usize, max_rows: usize) -> &[UserTrade] {
        let end = (scroll_offset + max_rows).min(self.trades.len());
        &self.trades[scroll_offset..end]
    }

    /// Format trade for display
    pub fn format_trade(&self, trade: &UserTrade, column: TradeLogColumn) -> String {
        match column {
            TradeLogColumn::Time => format_timestamp(trade.timestamp),
            TradeLogColumn::Symbol => trade.symbol.clone(),
            TradeLogColumn::Side => match trade.side {
                OrderSide::Buy => "BUY".to_string(),
                OrderSide::Sell => "SELL".to_string(),
            },
            TradeLogColumn::Type => "LIMIT".to_string(), // placeholder
            TradeLogColumn::Price => format!("{:.4}", trade.price),
            TradeLogColumn::Quantity => format!("{:.4}", trade.quantity),
            TradeLogColumn::Commission => format!("{:.4}", trade.commission),
            TradeLogColumn::PnL => format!("{:+.2}", 0.0), // would calculate from position tracking
        }
    }

    /// Get color based on trade side or PnL
    pub fn pnl_color(&self, pnl: f64) -> [f32; 4] {
        if pnl > 0.0 {
            [0.2, 0.8, 0.3, 1.0] // green
        } else if pnl < 0.0 {
            [0.9, 0.2, 0.2, 1.0] // red
        } else {
            [0.6, 0.6, 0.7, 1.0] // neutral
        }
    }

    /// Apply an order update event received from the private WebSocket stream.
    ///
    /// Parameters match the fields of `digdigdig3::core::types::websocket::OrderUpdateEvent`.
    /// Callers extract these values before calling, keeping this crate free of digdigdig3.
    ///
    /// Only fill events (status Filled or PartiallyFilled with a non-zero last fill quantity)
    /// produce a new `UserTrade` entry.
    ///
    /// - `side_buy`: true = Buy side, false = Sell side
    /// - `status_filled`: true when status is Filled or PartiallyFilled
    /// - `last_fill_price`: fill execution price (None → event is not a fill)
    /// - `last_fill_quantity`: fill quantity (None or 0.0 → skip)
    /// - `last_fill_commission`: commission charged on this fill
    /// - `timestamp`: event timestamp in milliseconds
    pub fn apply_order_update(
        &mut self,
        symbol: &str,
        side_buy: bool,
        status_filled: bool,
        last_fill_price: Option<f64>,
        last_fill_quantity: Option<f64>,
        last_fill_commission: Option<f64>,
        timestamp: i64,
    ) {
        // Only record fills with a non-zero fill quantity.
        if !status_filled {
            return;
        }
        let fill_price = match last_fill_price {
            Some(p) if p > 0.0 => p,
            _ => return,
        };
        let fill_qty = match last_fill_quantity {
            Some(q) if q > 0.0 => q,
            _ => return,
        };

        let trade = UserTrade {
            timestamp,
            symbol: symbol.to_owned(),
            side: if side_buy { OrderSide::Buy } else { OrderSide::Sell },
            price: fill_price,
            quantity: fill_qty,
            commission: last_fill_commission.unwrap_or(0.0),
        };

        self.trades.push(trade);

        // Keep the list sorted newest-first so scrolling starts at the most recent trade.
        self.trades.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    }

    /// Get column headers for the trade log table
    pub fn column_headers(&self) -> Vec<&'static str> {
        vec!["Time", "Symbol", "Side", "Type", "Price", "Quantity", "Commission", "PnL"]
    }
}

fn format_timestamp(ts: i64) -> String {
    let secs = (ts / 1000) % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

/// TradeLog panel configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeLogConfig {
    /// Show commission column
    pub show_commission: bool,
    /// Group by order
    pub group_by_order: bool,
    /// PnL calculation method
    pub pnl_method: PnLMethod,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PnLMethod {
    FIFO,
    LIFO,
    Average,
}

/// TradeLog panel wrapper (lightweight, lives in PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeLogPanel {
    id: TradeLogId,
    title: String,
}

impl TradeLogPanel {
    pub fn new(id: TradeLogId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> TradeLogId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "tradelog" }
    pub fn kind_label(&self) -> &'static str { "Trade Log" }
    pub fn min_size(&self) -> (f32, f32) { (200.0, 150.0) }
}
