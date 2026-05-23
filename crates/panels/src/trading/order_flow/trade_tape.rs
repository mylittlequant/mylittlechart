//! Trade Tape: Time & Sales panel for executed trades only.
//!
//! Shows real public trades (fills) streamed from the exchange — NOT L2 book
//! events. Alpha-by-size: row background opacity scales with
//! `(qty / max_qty_seen).sqrt().clamp(0.15, 1.0)` so big prints glow brighter.

use std::collections::VecDeque;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use trade_service::TradeSeries;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

// ─── constants ───────────────────────────────────────────────────────────────

const TT_ROW_HEIGHT: f32 = 14.0;
const TT_HEADER_HEIGHT: f32 = 18.0;
const TT_LEFT_PAD: f32 = 4.0;
const TT_MAX_TRADES: usize = 1000;
const TT_RETENTION_MS: i64 = 120_000; // 2 minutes

// ─── column config ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeTapeColumnConfig {
    pub show_time: bool,
    pub show_price: bool,
    pub show_size: bool,
}

impl Default for TradeTapeColumnConfig {
    fn default() -> Self {
        Self {
            show_time: true,
            show_price: true,
            show_size: true,
        }
    }
}

// ─── types ───────────────────────────────────────────────────────────────────

/// A single executed public trade.
#[derive(Clone, Debug)]
pub struct TapeEntry {
    /// Exchange timestamp in milliseconds.
    pub timestamp: i64,
    pub price: f64,
    pub quantity: f64,
    /// `true` = taker was buyer (aggressive buy); `false` = aggressive sell.
    pub is_buy: bool,
}

// ─── state ───────────────────────────────────────────────────────────────────

/// Trade Tape panel state.
#[derive(Clone)]
pub struct TradeTapeState {
    pub symbol: String,
    pub exchange: String,
    pub account_type: String,

    /// Ring buffer of confirmed trades (newest at back).
    pub trades: VecDeque<TapeEntry>,
    /// Hard cap on buffer size.
    pub max_trades: usize,

    /// When `true`, new trades go to `pause_buffer` instead of `trades`.
    pub paused: bool,
    /// Staging area for trades that arrive while the user is scrolled back.
    pub pause_buffer: VecDeque<TapeEntry>,

    /// Largest quantity ever seen in the current window — used for alpha scaling.
    pub max_qty_seen: f64,

    /// Cumulative buy volume in the rolling 2-minute window.
    pub buy_vol_window: f64,
    /// Cumulative sell volume in the rolling 2-minute window.
    pub sell_vol_window: f64,

    /// Draw-time filter: entries with qty below this are skipped on render but
    /// kept in storage (non-destructive).
    pub filter_min_qty: f64,

    /// When `true`, the panel is pinned to the newest trade.
    pub auto_scroll: bool,
    /// Row-level scroll offset (0 = newest visible at top).
    pub scroll_offset: f64,

    /// Shared trade ring written by the bridge.
    pub shared_trades: Option<Arc<RwLock<TradeSeries>>>,
    /// Version of `TradeSeries` we last consumed.
    pub last_seen_version: u64,

    /// Market price synced from a linked DOM panel.
    pub dom_market_price: Option<f64>,
    /// Tick size from a linked DOM panel.
    pub dom_tick_size: Option<f64>,

    /// Which columns to show.
    pub column_config: TradeTapeColumnConfig,
}

impl std::fmt::Debug for TradeTapeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TradeTapeState")
            .field("symbol", &self.symbol)
            .field("exchange", &self.exchange)
            .field("trades_len", &self.trades.len())
            .field("paused", &self.paused)
            .field("max_qty_seen", &self.max_qty_seen)
            .field("last_seen_version", &self.last_seen_version)
            .finish()
    }
}

impl TradeTapeState {
    pub fn new() -> Self {
        Self {
            symbol: String::new(),
            exchange: String::new(),
            account_type: String::new(),
            trades: VecDeque::new(),
            max_trades: TT_MAX_TRADES,
            paused: false,
            pause_buffer: VecDeque::new(),
            max_qty_seen: 0.0,
            buy_vol_window: 0.0,
            sell_vol_window: 0.0,
            filter_min_qty: 0.0,
            auto_scroll: true,
            scroll_offset: 0.0,
            shared_trades: None,
            last_seen_version: 0,
            dom_market_price: None,
            dom_tick_size: None,
            column_config: TradeTapeColumnConfig::default(),
        }
    }

    // ─── tick ─────────────────────────────────────────────────────────────

    /// Pull new trades from `shared_trades`, update window stats, and prune stale entries.
    ///
    /// Call once per frame before render.
    pub fn tick(&mut self) {
        self.pull_new_trades();
        self.prune_retention();
        self.recompute_window_stats();
    }

    fn pull_new_trades(&mut self) {
        let handle = match self.shared_trades.clone() {
            Some(h) => h,
            None => return,
        };
        let series = match handle.read() {
            Ok(s) => s,
            Err(_) => return,
        };
        if series.version == self.last_seen_version {
            return;
        }

        let new_count = (series.version.saturating_sub(self.last_seen_version)) as usize;
        let len = series.trades.len();
        let skip = if new_count < len { len - new_count } else { 0 };

        // Collect new entries before dropping the lock.
        let new_entries: Vec<TapeEntry> = series
            .trades
            .iter()
            .skip(skip)
            .map(|trade| TapeEntry {
                timestamp: trade.timestamp_ms,
                price: trade.price,
                quantity: trade.quantity,
                // is_buyer_maker != 0 means buyer is passive → taker is seller
                is_buy: trade.is_buyer_maker == 0,
            })
            .collect();
        let new_version = series.version;
        drop(series);

        for entry in new_entries {
            if self.paused {
                if self.pause_buffer.len() >= self.max_trades {
                    self.pause_buffer.pop_front();
                }
                self.pause_buffer.push_back(entry);
            } else {
                self.push_entry(entry);
            }
        }

        self.last_seen_version = new_version;
    }

    fn push_entry(&mut self, entry: TapeEntry) {
        if entry.quantity > self.max_qty_seen {
            self.max_qty_seen = entry.quantity;
        }
        if self.trades.len() >= self.max_trades {
            self.trades.pop_front();
        }
        self.trades.push_back(entry);
    }

    /// Flush `pause_buffer` into `trades` and resume live updates.
    pub fn resume(&mut self) {
        self.paused = false;
        self.auto_scroll = true;
        self.scroll_offset = 0.0;
        while let Some(entry) = self.pause_buffer.pop_front() {
            self.push_entry(entry);
        }
    }

    fn prune_retention(&mut self) {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let cutoff = now_ms - TT_RETENTION_MS;
        while let Some(front) = self.trades.front() {
            if front.timestamp < cutoff {
                self.trades.pop_front();
            } else {
                break;
            }
        }
    }

    fn recompute_window_stats(&mut self) {
        let mut max_qty = 0.0_f64;
        let mut buy_vol = 0.0_f64;
        let mut sell_vol = 0.0_f64;
        for entry in &self.trades {
            if entry.quantity > max_qty {
                max_qty = entry.quantity;
            }
            if entry.is_buy {
                buy_vol += entry.quantity;
            } else {
                sell_vol += entry.quantity;
            }
        }
        self.max_qty_seen = max_qty;
        self.buy_vol_window = buy_vol;
        self.sell_vol_window = sell_vol;
    }

    // ─── alpha scaling ────────────────────────────────────────────────────

    /// Row background alpha: `sqrt(qty / max_qty).clamp(0.15, 1.0)`.
    fn row_alpha(&self, qty: f64) -> f64 {
        if self.max_qty_seen <= 0.0 {
            return 0.15;
        }
        (qty / self.max_qty_seen).sqrt().clamp(0.15, 1.0)
    }

    // ─── visible rows ─────────────────────────────────────────────────────

    fn visible_entries(&self, max_count: usize) -> Vec<&TapeEntry> {
        let skip = if self.auto_scroll {
            0
        } else {
            self.scroll_offset as usize
        };
        self.trades
            .iter()
            .rev()
            .filter(|e| e.quantity >= self.filter_min_qty)
            .skip(skip)
            .take(max_count)
            .collect()
    }

    // ─── format helpers ───────────────────────────────────────────────────

    fn format_time(ts: i64) -> String {
        let total_ms = ts % 86_400_000;
        let h = total_ms / 3_600_000;
        let m = (total_ms % 3_600_000) / 60_000;
        let s = (total_ms % 60_000) / 1_000;
        let ms = total_ms % 1_000;
        format!("{:02}:{:02}:{:02}.{:03}", h, m, s, ms)
    }

    fn format_qty(qty: f64) -> String {
        if qty >= 1_000_000.0 {
            format!("{:.2}M", qty / 1_000_000.0)
        } else if qty >= 1_000.0 {
            format!("{:.2}K", qty / 1_000.0)
        } else if qty >= 1.0 {
            format!("{:.2}", qty)
        } else {
            format!("{:.4}", qty)
        }
    }

    fn format_price(price: f64, tick_size: f64) -> String {
        let decimals = if tick_size >= 1.0 {
            0usize
        } else if tick_size >= 0.1 {
            1
        } else if tick_size >= 0.01 {
            2
        } else if tick_size >= 0.001 {
            3
        } else {
            4
        };
        format!("{:.prec$}", price, prec = decimals)
    }

    // ─── input ────────────────────────────────────────────────────────────

    /// Scroll by `delta` rows (positive = toward older trades).
    pub fn handle_scroll(&mut self, delta: f64) {
        let filtered_count = self
            .trades
            .iter()
            .filter(|e| e.quantity >= self.filter_min_qty)
            .count();

        let new_offset = (self.scroll_offset + delta).max(0.0);
        let max_offset = filtered_count.saturating_sub(1) as f64;
        self.scroll_offset = new_offset.min(max_offset);

        if self.scroll_offset > 0.0 {
            self.auto_scroll = false;
            self.paused = true;
        } else {
            self.resume();
        }
    }

    /// Double-click: jump to newest trade and resume live feed.
    pub fn handle_double_click(&mut self) {
        self.resume();
    }

    /// Drag scroll: up = older trades, down = newer.
    pub fn handle_drag(&mut self, _dx: f64, dy: f64) {
        self.handle_scroll(-dy / 16.0);
    }

    /// Key handler (unused for now, kept for trait symmetry).
    pub fn handle_key(&mut self, _key: zengeld_chart::input::KeyCode) -> bool {
        false
    }
}

// ─── TradingPanel impl ────────────────────────────────────────────────────────

impl TradingPanel for TradeTapeState {
    fn kind(&self) -> &'static str { "trade_tape" }
    fn label(&self) -> &'static str { "Trade Tape" }

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
        // ── background ───────────────────────────────────────────────────
        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        // ── ratio bar (header) ───────────────────────────────────────────
        let total_vol = self.buy_vol_window + self.sell_vol_window;
        if total_vol > 0.0 {
            let buy_frac = (self.buy_vol_window / total_vol) as f32;
            let buy_px = (w * buy_frac).max(0.0).min(w);
            ctx.set_fill_color(&theme.buy);
            ctx.fill_rect(x as f64, y as f64, buy_px as f64, TT_HEADER_HEIGHT as f64);
            ctx.set_fill_color(&theme.sell);
            ctx.fill_rect(
                (x + buy_px) as f64,
                y as f64,
                (w - buy_px) as f64,
                TT_HEADER_HEIGHT as f64,
            );
        } else {
            ctx.set_fill_color(&theme.header_bg);
            ctx.fill_rect(x as f64, y as f64, w as f64, TT_HEADER_HEIGHT as f64);
        }

        // ── dynamic column layout ─────────────────────────────────────────
        // Build list of visible columns in order, then distribute width evenly.
        #[derive(Clone, Copy)]
        enum Col { Time, Price, Size }

        let mut visible_cols: Vec<Col> = Vec::with_capacity(3);
        if self.column_config.show_time  { visible_cols.push(Col::Time);  }
        if self.column_config.show_price { visible_cols.push(Col::Price); }
        if self.column_config.show_size  { visible_cols.push(Col::Size);  }

        // Reserve right edge for symbol label (or 0 if no cols — degenerate).
        let usable_w = w - TT_LEFT_PAD;
        let col_count = visible_cols.len();
        // x position for column i: left-pad + i * (usable_w / n_cols)
        let col_x = |i: usize| -> f32 {
            if col_count == 0 { return x + TT_LEFT_PAD; }
            x + TT_LEFT_PAD + (i as f32) * (usable_w / col_count as f32)
        };

        // ── header labels ────────────────────────────────────────────────
        ctx.set_font("9px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);

        let header_mid_y = (y + TT_HEADER_HEIGHT / 2.0) as f64;

        ctx.set_fill_color("#000000cc");
        for (i, &col) in visible_cols.iter().enumerate() {
            let lx = col_x(i) as f64;
            let label = match col {
                Col::Time  => "TIME",
                Col::Price => "PRICE",
                Col::Size  => "SIZE",
            };
            ctx.fill_text(label, lx, header_mid_y);
        }

        if !self.symbol.is_empty() {
            ctx.set_text_align(TextAlign::Right);
            ctx.fill_text(&self.symbol, (x + w - TT_LEFT_PAD) as f64, header_mid_y);
        }

        // ── rows ─────────────────────────────────────────────────────────
        let content_h = h - TT_HEADER_HEIGHT;
        let max_rows = (content_h / TT_ROW_HEIGHT).floor() as usize;
        if max_rows == 0 {
            return;
        }

        let tick_size = self.dom_tick_size.unwrap_or(0.01);
        let entries = self.visible_entries(max_rows);

        for (row_idx, entry) in entries.iter().enumerate() {
            let row_y = y + TT_HEADER_HEIGHT + (row_idx as f32 * TT_ROW_HEIGHT);
            let mid_y = (row_y + TT_ROW_HEIGHT / 2.0) as f64;

            // Colored background with alpha-by-size
            let alpha = self.row_alpha(entry.quantity);
            let alpha_byte = (alpha * 255.0).round() as u8;
            let bg_color = if entry.is_buy {
                format!("#2ea043{:02x}", alpha_byte)
            } else {
                format!("#cc2233{:02x}", alpha_byte)
            };
            ctx.set_fill_color(&bg_color);
            ctx.fill_rect(x as f64, row_y as f64, w as f64, TT_ROW_HEIGHT as f64);

            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);

            for (i, &col) in visible_cols.iter().enumerate() {
                let cx = col_x(i) as f64;
                match col {
                    Col::Time => {
                        ctx.set_fill_color(&theme.text_primary);
                        ctx.fill_text(&Self::format_time(entry.timestamp), cx, mid_y);
                    }
                    Col::Price => {
                        let price_color = if entry.is_buy { &theme.buy_bright } else { &theme.sell_bright };
                        ctx.set_fill_color(price_color);
                        ctx.fill_text(&Self::format_price(entry.price, tick_size), cx, mid_y);
                    }
                    Col::Size => {
                        ctx.set_fill_color(&theme.text_primary);
                        ctx.fill_text(&Self::format_qty(entry.quantity), cx, mid_y);
                    }
                }
            }
        }

        if entries.is_empty() {
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_fill_color(&theme.text_header);
            ctx.fill_text("No trades", (x + w / 2.0) as f64, (y + h / 2.0) as f64);
        }

        // ── PAUSED badge ─────────────────────────────────────────────────
        if !self.auto_scroll {
            let badge_w = 54.0_f64;
            let badge_h = 14.0_f64;
            let badge_x = (x + w / 2.0) as f64 - badge_w / 2.0;
            let badge_y = (y + TT_HEADER_HEIGHT + 2.0) as f64;
            ctx.set_fill_color("#c8a000cc");
            ctx.fill_rect(badge_x, badge_y, badge_w, badge_h);
            ctx.set_font("9px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_fill_color("#000000ff");
            ctx.fill_text("PAUSED", (x + w / 2.0) as f64, badge_y + badge_h / 2.0);
        }
    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool { false }
}

// ─── lightweight panel wrapper (PanelKind) ───────────────────────────────────

/// Trade Tape panel ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TradeTapeId(pub u64);

/// Lightweight wrapper stored inside `PanelKind`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeTapePanel {
    id: TradeTapeId,
    title: String,
}

impl TradeTapePanel {
    pub fn new(id: TradeTapeId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> TradeTapeId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "trade_tape" }
    pub fn kind_label(&self) -> &'static str { "Trade Tape" }
    pub fn min_size(&self) -> (f32, f32) { (100.0, 80.0) }
}
