use serde::{Serialize, Deserialize};
use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, RwLock};

use trade_service::TradeSeries;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

/// BigTrades panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BigTradesId(pub u64);

/// BigTrades panel state (heavy data)
#[derive(Clone)]
pub struct BigTradesState {
    /// Symbol being monitored
    pub symbol: String,

    /// Exchange identifier string (e.g. "binance")
    pub exchange: String,

    /// Account type short label (e.g. "S", "F", "FI")
    pub account_type: String,
    /// Derived, filtered ring buffer — kept in sync with the shared series via `tick()`.
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

    /// Maximum quantity seen in the current window — used for bar scaling.
    pub max_qty_seen: f64,

    /// Handle to the shared trade ring for this (exchange, symbol, account_type).
    ///
    /// `None` until `subscribe_trades` is called on the bridge. Panels read
    /// from this at tick time; they NEVER write through it.
    pub shared_trades: Option<Arc<RwLock<TradeSeries>>>,

    /// The `TradeSeries::version` we last processed in `tick()`.
    ///
    /// When `shared_trades.version != last_seen_trade_version` there are new
    /// trades to pull into `big_trades`.
    pub last_seen_trade_version: u64,

    /// Crosshair price synced from a linked chart window.
    pub crosshair_price: Option<f64>,
}

impl fmt::Debug for BigTradesState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BigTradesState")
            .field("symbol", &self.symbol)
            .field("exchange", &self.exchange)
            .field("account_type", &self.account_type)
            .field("big_trades_len", &self.big_trades.len())
            .field("size_threshold", &self.size_threshold)
            .field("last_seen_trade_version", &self.last_seen_trade_version)
            .field("has_shared_trades", &self.shared_trades.is_some())
            .finish()
    }
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
            exchange: String::new(),
            account_type: String::new(),
            big_trades: VecDeque::new(),
            size_threshold: 0.0,
            notional_threshold: None,
            flash_trades: Vec::new(),
            dom_market_price: None,
            dom_tick_size: None,
            scroll_offset: 0.0,
            max_qty_seen: 0.0,
            shared_trades: None,
            last_seen_trade_version: 0,
            crosshair_price: None,
        }
    }

    /// Pull new trades from the shared series into the local filter cache.
    ///
    /// Call this once per frame (or before render). It is a no-op when there is
    /// no shared series or when the version has not advanced.
    pub fn tick(&mut self) {
        let handle = match self.shared_trades.as_ref() {
            Some(h) => h,
            None => return,
        };

        let series = match handle.read() {
            Ok(s) => s,
            Err(_) => return,
        };

        if series.version == self.last_seen_trade_version {
            return;
        }

        // How many trades have been added since we last processed?
        // Version is incremented once per push, so the delta == number of new trades
        // (assuming no rotations dropped anything — if they did we accept a gap).
        let new_count = (series.version.saturating_sub(self.last_seen_trade_version)) as usize;
        let len = series.trades.len();

        // Walk only the tail that is new.
        let skip = if new_count < len { len - new_count } else { 0 };

        const MAX_TRADES: usize = 1000;

        for trade in series.trades.iter().skip(skip) {
            // Apply size threshold filter.
            if trade.quantity < self.size_threshold {
                continue;
            }
            // Apply notional threshold filter if set.
            if let Some(notional_min) = self.notional_threshold {
                if trade.price * trade.quantity < notional_min {
                    continue;
                }
            }

            let public = PublicTrade {
                timestamp: trade.timestamp_ms,
                price: trade.price,
                quantity: trade.quantity,
                side: if trade.is_buyer_maker != 0 {
                    TradeSide::Sell
                } else {
                    TradeSide::Buy
                },
            };

            if public.quantity > self.max_qty_seen {
                self.max_qty_seen = public.quantity;
            }

            if self.big_trades.len() >= MAX_TRADES {
                self.big_trades.pop_front();
            }
            self.big_trades.push_back(public);
        }

        self.last_seen_trade_version = series.version;
    }

    /// Scroll through the trade list.  Positive `delta` scrolls toward older trades.
    pub fn handle_scroll(&mut self, delta: f64) {
        let raw = self.scroll_offset + delta * 30.0;
        let max_offset = self.big_trades.len().saturating_sub(1) as f64;
        self.scroll_offset = raw.clamp(0.0, max_offset);
    }

    /// Reset scroll to latest (show most recent trades).
    pub fn handle_double_click(&mut self) {
        self.scroll_offset = 0.0;
    }

    /// Get visible trades for rendering (most recent first, respecting `scroll_offset`).
    ///
    /// `scroll_offset` rows are skipped from the newest end so the user can scroll
    /// back through history. The offset is clamped to the available trade count so
    /// it never underflows.
    pub fn visible_trades(&self, max_count: usize) -> Vec<&PublicTrade> {
        let skip = (self.scroll_offset as usize).min(self.big_trades.len());
        self.big_trades.iter().rev().skip(skip).take(max_count).collect()
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

    /// Apply a live trade directly (legacy broadcast path, kept for compatibility
    /// while Footprint and VolumeProfile are still on the old fan-out).
    ///
    /// BigTrades now reads from `shared_trades` via `tick()`. This method is
    /// called from the fan-out loop and is intentionally a no-op — all filtering
    /// happens in `tick()` instead.
    #[deprecated(note = "BigTrades reads from shared_trades via tick(); this method is a no-op")]
    pub fn push_trade(&mut self, _price: f64, _quantity: f64, _is_buyer_maker: bool, _timestamp: i64) {
        // No-op: BigTrades now reads from the shared TradeSeries.
    }

    /// Calculate bar width for size visualization (0.0–1.0 fraction of max_width).
    ///
    /// Bars scale against the largest trade seen in the current window so that
    /// the biggest trade always fills the full width. When no trades have been
    /// received yet (`max_qty_seen == 0`) the bar is zero-width.
    pub fn size_bar_width(&self, trade: &PublicTrade, max_width: f32) -> f32 {
        if self.max_qty_seen == 0.0 {
            return 0.0;
        }
        let ratio = (trade.quantity / self.max_qty_seen).clamp(0.0, 1.0);
        ratio as f32 * max_width
    }
}

fn format_timestamp(ts: i64) -> String {
    let secs = (ts / 1000) % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

const BT_HEADER_HEIGHT: f32 = 18.0;
const BT_ROW_HEIGHT: f32 = 20.0;
const BT_LEFT_PAD: f32 = 6.0;

impl TradingPanel for BigTradesState {
    fn kind(&self) -> &'static str { "big_trades" }
    fn label(&self) -> &'static str { "Big Trades" }

    fn render(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        theme: &crate::panel_theme::PanelTheme,
    ) {
        ctx.set_fill_color(&theme.panel_bg);
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

        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, BT_HEADER_HEIGHT as f64);

        ctx.set_fill_color(&theme.text_header);
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
                TradeSide::Buy => &theme.buy,
                TradeSide::Sell => &theme.sell,
            };
            ctx.set_fill_color(bar_color);
            ctx.fill_rect(x as f64, row_y as f64, bar_width as f64, BT_ROW_HEIGHT as f64);

            let text_y = (row_y + BT_ROW_HEIGHT / 2.0) as f64;

            let time_str = {
                let secs = (trade.timestamp / 1000) % 86400;
                let hh = secs / 3600;
                let mm = (secs % 3600) / 60;
                let ss = secs % 60;
                format!("{:02}:{:02}:{:02}", hh, mm, ss)
            };

            ctx.set_fill_color(&theme.text_primary);
            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(&time_str, time_col_x as f64, text_y);

            let (side_str, side_color) = match trade.side {
                TradeSide::Buy => ("BUY", &theme.buy),
                TradeSide::Sell => ("SELL", &theme.sell),
            };
            ctx.set_fill_color(side_color);
            ctx.fill_text(side_str, side_col_x as f64, text_y);

            let price_str = format!("{:.4}", trade.price);
            ctx.set_fill_color(&theme.text_primary);
            ctx.fill_text(&price_str, price_col_x as f64, text_y);

            let size_str = format!("{:.4}", trade.quantity);
            ctx.fill_text(&size_str, size_col_x as f64, text_y);

            let notional = trade.price * trade.quantity;
            let notional_str = format!("{:.2}", notional);
            ctx.fill_text(&notional_str, notional_col_x as f64, text_y);

            ctx.set_fill_color(&theme.separator);
            ctx.fill_rect(x as f64, (row_y + BT_ROW_HEIGHT - 1.0) as f64, w as f64, 1.0);
        }

        if !self.symbol.is_empty() {
            ctx.set_fill_color(&theme.text_muted);
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
