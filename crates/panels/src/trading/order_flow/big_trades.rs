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

/// Column visibility configuration for the Big Trades panel.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BigTradesColumnConfig {
    pub show_time: bool,
    pub show_side: bool,
    pub show_price: bool,
    pub show_size: bool,
    pub show_notional: bool,
}

impl Default for BigTradesColumnConfig {
    fn default() -> Self {
        Self {
            show_time: true,
            show_side: true,
            show_price: true,
            show_size: true,
            show_notional: true,
        }
    }
}

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

    /// Column visibility configuration.
    pub column_config: BigTradesColumnConfig,
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
            column_config: BigTradesColumnConfig::default(),
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

    /// Continuous drag scroll: drag up (dy < 0) shows older trades, drag down
    /// (dy > 0) scrolls back toward newest.  Sensitivity: 1 px = 1 trade.
    pub fn handle_drag(&mut self, _dx: f64, dy: f64) {
        // Invert: dragging upward = scroll toward older trades.
        let raw = self.scroll_offset - dy;
        let max_offset = self.big_trades.len().saturating_sub(1) as f64;
        self.scroll_offset = raw.clamp(0.0, max_offset);
    }

    /// Handle a named key event.  Returns `true` if the key was consumed.
    pub fn handle_key(&mut self, _key: zengeld_chart::input::KeyCode) -> bool {
        false
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
        _coordinator: &mut uzor::InputCoordinator,
        _slot_prefix: &str,
    ) {
        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        // Build visible column list: (label, default_width).
        // The last visible column gets all remaining width (no fixed width needed).
        struct ColDef {
            label: &'static str,
            width: f32,
        }
        let mut cols: Vec<ColDef> = Vec::with_capacity(5);
        if self.column_config.show_time     { cols.push(ColDef { label: "TIME",     width: 60.0 }); }
        if self.column_config.show_side     { cols.push(ColDef { label: "SIDE",     width: 36.0 }); }
        if self.column_config.show_price    { cols.push(ColDef { label: "PRICE",    width: 80.0 }); }
        if self.column_config.show_size     { cols.push(ColDef { label: "SIZE",     width: 70.0 }); }
        if self.column_config.show_notional { cols.push(ColDef { label: "NOTIONAL", width: 0.0  }); }

        // Compute x positions for each column.
        // The last column stretches to fill remaining space.
        let usable_w = w - BT_LEFT_PAD * 2.0;
        let gap = 4.0_f32;
        let fixed_w: f32 = cols.iter().take(cols.len().saturating_sub(1)).map(|c| c.width + gap).sum();
        let last_w = (usable_w - fixed_w).max(0.0);

        let mut col_xs: Vec<f32> = Vec::with_capacity(cols.len());
        {
            let mut cx = x + BT_LEFT_PAD;
            for (idx, col) in cols.iter().enumerate() {
                col_xs.push(cx);
                let w_used = if idx + 1 == cols.len() { last_w } else { col.width };
                cx += w_used + gap;
            }
        }

        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, BT_HEADER_HEIGHT as f64);

        ctx.set_fill_color(&theme.text_header);
        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);

        let header_y = (y + BT_HEADER_HEIGHT / 2.0) as f64;
        for (idx, col) in cols.iter().enumerate() {
            ctx.fill_text(col.label, col_xs[idx] as f64, header_y);
        }

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

            ctx.set_fill_color(&theme.text_primary);
            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);

            // Track which visible column index maps to which data field.
            let mut col_idx = 0usize;

            if self.column_config.show_time {
                let secs = (trade.timestamp / 1000) % 86400;
                let hh = secs / 3600;
                let mm = (secs % 3600) / 60;
                let ss = secs % 60;
                let time_str = format!("{:02}:{:02}:{:02}", hh, mm, ss);
                ctx.fill_text(&time_str, col_xs[col_idx] as f64, text_y);
                col_idx += 1;
            }

            if self.column_config.show_side {
                let (side_str, side_color) = match trade.side {
                    TradeSide::Buy  => ("BUY",  &theme.buy),
                    TradeSide::Sell => ("SELL", &theme.sell),
                };
                ctx.set_fill_color(side_color);
                ctx.fill_text(side_str, col_xs[col_idx] as f64, text_y);
                ctx.set_fill_color(&theme.text_primary);
                col_idx += 1;
            }

            if self.column_config.show_price {
                let price_str = format!("{:.4}", trade.price);
                ctx.fill_text(&price_str, col_xs[col_idx] as f64, text_y);
                col_idx += 1;
            }

            if self.column_config.show_size {
                let size_str = format!("{:.4}", trade.quantity);
                ctx.fill_text(&size_str, col_xs[col_idx] as f64, text_y);
                col_idx += 1;
            }

            if self.column_config.show_notional {
                let notional_str = format!("{:.2}", trade.price * trade.quantity);
                ctx.fill_text(&notional_str, col_xs[col_idx] as f64, text_y);
            }

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
