use serde::{Serialize, Deserialize};
use std::collections::VecDeque;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

/// BigTrades panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BigTradesId(pub u64);

/// BigTrades panel state (heavy data)
#[derive(Clone, Debug)]
pub struct BigTradesState {
    /// Symbol source binding (how to resolve which instrument to display)
    pub source: crate::trading::SymbolSource,

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
            source: crate::trading::SymbolSource::default(),
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

fn rgba_to_hex(rgba: [f32; 4]) -> String {
    let r = (rgba[0].clamp(0.0, 1.0) * 255.0) as u8;
    let g = (rgba[1].clamp(0.0, 1.0) * 255.0) as u8;
    let b = (rgba[2].clamp(0.0, 1.0) * 255.0) as u8;
    let a = (rgba[3].clamp(0.0, 1.0) * 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", r, g, b, a)
}

const BT_BG: [f32; 4] = [0.051, 0.067, 0.090, 1.0];
const BT_HEADER_BG: [f32; 4] = [0.071, 0.086, 0.110, 1.0];
const BT_HEADER_TEXT: [f32; 4] = [0.5, 0.55, 0.65, 1.0];
const BT_TEXT_DEFAULT: [f32; 4] = [0.88, 0.88, 0.88, 1.0];
const BT_BUY_TEXT: [f32; 4] = [0.2, 0.85, 0.4, 1.0];
const BT_SELL_TEXT: [f32; 4] = [0.95, 0.27, 0.36, 1.0];
const BT_BAR_BUY: [f32; 4] = [0.0, 0.67, 0.33, 0.18];
const BT_BAR_SELL: [f32; 4] = [0.8, 0.1, 0.15, 0.18];
const BT_SYMBOL_TEXT: [f32; 4] = [0.4, 0.45, 0.55, 1.0];
const BT_HEADER_HEIGHT: f32 = 18.0;
const BT_ROW_HEIGHT: f32 = 20.0;
const BT_LEFT_PAD: f32 = 6.0;

impl TradingPanel for BigTradesState {
    fn kind(&self) -> &'static str { "big_trades" }
    fn label(&self) -> &'static str { "Big Trades" }

    fn render(&self, ctx: &mut dyn RenderContext, x: f32, y: f32, w: f32, h: f32) {
        ctx.set_fill_color(&rgba_to_hex(BT_BG));
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        let time_col_x = x + BT_LEFT_PAD;
        let time_col_w = 60.0_f32;
        let side_col_x = time_col_x + time_col_w + 4.0;
        let side_col_w = 36.0_f32;
        let price_col_x = side_col_x + side_col_w + 4.0;
        let price_col_w = 80.0_f32;
        let size_col_x = price_col_x + price_col_w + 4.0;
        let size_col_w = 70.0_f32;
        let notional_col_x = size_col_x + size_col_w + 4.0;

        ctx.set_fill_color(&rgba_to_hex(BT_HEADER_BG));
        ctx.fill_rect(x as f64, y as f64, w as f64, BT_HEADER_HEIGHT as f64);

        ctx.set_fill_color(&rgba_to_hex(BT_HEADER_TEXT));
        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);

        let header_y = (y + BT_HEADER_HEIGHT / 2.0) as f64;
        ctx.fill_text("TIME", time_col_x as f64, header_y);
        ctx.fill_text("SIDE", side_col_x as f64, header_y);
        ctx.fill_text("PRICE", price_col_x as f64, header_y);
        ctx.fill_text("SIZE", size_col_x as f64, header_y);
        ctx.fill_text("NOTIONAL", notional_col_x as f64, header_y);

        let available_height = h - BT_HEADER_HEIGHT;
        let max_rows = (available_height / BT_ROW_HEIGHT) as usize;
        let trades = self.visible_trades(max_rows);

        let bar_max_width = w - BT_LEFT_PAD * 2.0;

        for (i, trade) in trades.iter().enumerate() {
            let row_y = y + BT_HEADER_HEIGHT + (i as f32 * BT_ROW_HEIGHT);

            let bar_width = self.size_bar_width(trade, bar_max_width);
            let bar_color = match trade.side {
                TradeSide::Buy => BT_BAR_BUY,
                TradeSide::Sell => BT_BAR_SELL,
            };
            ctx.set_fill_color(&rgba_to_hex(bar_color));
            ctx.fill_rect(x as f64, row_y as f64, bar_width as f64, BT_ROW_HEIGHT as f64);

            let text_y = (row_y + BT_ROW_HEIGHT / 2.0) as f64;

            let time_str = {
                let secs = (trade.timestamp / 1000) % 86400;
                let hh = secs / 3600;
                let mm = (secs % 3600) / 60;
                let ss = secs % 60;
                format!("{:02}:{:02}:{:02}", hh, mm, ss)
            };

            ctx.set_fill_color(&rgba_to_hex(BT_TEXT_DEFAULT));
            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(&time_str, time_col_x as f64, text_y);

            let (side_str, side_color) = match trade.side {
                TradeSide::Buy => ("BUY", BT_BUY_TEXT),
                TradeSide::Sell => ("SELL", BT_SELL_TEXT),
            };
            ctx.set_fill_color(&rgba_to_hex(side_color));
            ctx.fill_text(side_str, side_col_x as f64, text_y);

            let price_str = format!("{:.4}", trade.price);
            ctx.set_fill_color(&rgba_to_hex(BT_TEXT_DEFAULT));
            ctx.fill_text(&price_str, price_col_x as f64, text_y);

            let size_str = format!("{:.4}", trade.quantity);
            ctx.fill_text(&size_str, size_col_x as f64, text_y);

            let notional = trade.price * trade.quantity;
            let notional_str = format!("{:.2}", notional);
            ctx.fill_text(&notional_str, notional_col_x as f64, text_y);

            ctx.set_fill_color(&rgba_to_hex([0.15, 0.17, 0.22, 0.6]));
            ctx.fill_rect(x as f64, (row_y + BT_ROW_HEIGHT - 1.0) as f64, w as f64, 1.0);
        }

        if !self.symbol.is_empty() {
            ctx.set_fill_color(&rgba_to_hex(BT_SYMBOL_TEXT));
            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Right);
            ctx.set_text_baseline(TextBaseline::Middle);
            let sym_x = (x + w - BT_LEFT_PAD) as f64;
            let sym_y = (y + BT_HEADER_HEIGHT / 2.0) as f64;
            ctx.fill_text(&self.symbol, sym_x, sym_y);
        }
    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool { false }
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
