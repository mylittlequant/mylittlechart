use serde::{Serialize, Deserialize};
use std::collections::VecDeque;
use std::fmt;

use crate::trading::order_flow::trade_tape::SharedTradeSeries;

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
    /// Manual skin pick — `None` follows the theme-skin's `tape_skin`
    /// default (DomSkin precedence pattern; shared `TapeSkin` enum).
    pub skin: Option<super::trade_tape::TapeSkin>,
    /// Market price from linked DOM
    pub dom_market_price: Option<f64>,
    /// Tick size from linked DOM
    pub dom_tick_size: Option<f64>,
    /// Scroll offset
    pub scroll_offset: f64,

    /// Maximum quantity seen in the current window — used for bar scaling.
    pub max_qty_seen: f64,

    /// Handle to the station's `Series<TradePoint>` for this (exchange, symbol,
    /// account_type). `None` until `subscribe_trades` returns a populated handle.
    /// Panels read; the station forwarder is the sole writer.
    pub shared_trades: Option<SharedTradeSeries>,

    /// Timestamp (ms) of the newest trade we have already pulled into `big_trades`.
    /// Per-consumer cursor: multiple panels can share one `Series<TradePoint>`
    /// without competing for a single global version counter.
    pub last_seen_ts_ms: i64,

    /// Crosshair price synced from a linked chart window.
    pub crosshair_price: Option<f64>,

    /// Column visibility configuration.
    pub column_config: BigTradesColumnConfig,

    /// Multi-venue mode (M/BT, по образцу Multi-DOM, owner 2026-07-31).
    /// When ON, `tick` pulls a merged time-sorted stream from the roster
    /// handles (thresholds applied per print) and render shows the
    /// EXCHANGE column; single-venue fields survive toggling back.
    pub multi_mode: bool,
    /// Embedded multi-venue roster + per-venue feed state.
    pub multi: super::multi_tape::MultiTapeState,
}

impl fmt::Debug for BigTradesState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BigTradesState")
            .field("symbol", &self.symbol)
            .field("exchange", &self.exchange)
            .field("account_type", &self.account_type)
            .field("big_trades_len", &self.big_trades.len())
            .field("size_threshold", &self.size_threshold)
            .field("last_seen_ts_ms", &self.last_seen_ts_ms)
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
    /// Local wall-clock at ingest (ms) — drives the new-row flash
    /// (TapeSurf trades-feed polish, 2026-07-31).
    pub arrival_ms: u64,
    /// Roster index of the source venue in multi mode (`None` =
    /// single-venue print). Drives the EXCHANGE column (TapeSurf ref 17).
    pub venue: Option<u8>,
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
            skin: None,
            dom_market_price: None,
            dom_tick_size: None,
            scroll_offset: 0.0,
            max_qty_seen: 0.0,
            shared_trades: None,
            last_seen_ts_ms: 0,
            crosshair_price: None,
            column_config: BigTradesColumnConfig::default(),
            multi_mode: false,
            multi: super::multi_tape::MultiTapeState::new(),
        }
    }

    /// Toggle multi-venue mode (M/BT — same contract as Trade Tape's
    /// `set_multi_mode`): roster base symbol re-synced, panel's own venue
    /// seeds the roster on first use, visible buffer cleared on switch.
    /// Subscriptions are the caller's job.
    pub fn set_multi_mode(&mut self, on: bool) {
        if self.multi_mode == on {
            return;
        }
        self.multi_mode = on;
        self.big_trades.clear();
        self.scroll_offset = 0.0;
        self.max_qty_seen = 0.0;
        if on {
            if !self.multi.symbol.eq_ignore_ascii_case(&self.symbol) {
                let sym = self.symbol.clone();
                self.multi.reset_for_symbol(&sym);
            }
            if self.multi.entries.is_empty() && !self.exchange.is_empty() {
                self.multi.add_entry(&super::multi_dom::col_entry(
                    &self.exchange,
                    &self.account_type,
                ));
            }
        } else {
            self.last_seen_ts_ms = 0;
        }
    }

    /// Pull new trades from the shared series into the local filter cache.
    ///
    /// Call this once per frame (or before render). It is a no-op when there is
    /// no shared series or when the version has not advanced.
    pub fn tick(&mut self) {
        if self.multi_mode {
            self.tick_multi();
            return;
        }
        let handle = match self.shared_trades.as_ref() {
            Some(h) => h.clone(),
            None => return,
        };

        let guard = handle.blocking_read();
        let snapshot = guard.snapshot();
        drop(guard);

        const MAX_TRADES: usize = 1000;
        let cursor = self.last_seen_ts_ms;
        let mut new_max_ts = cursor;
        let arrival_ms = web_time::SystemTime::now()
            .duration_since(web_time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        for trade in snapshot.iter().filter(|p| p.ts_ms > cursor) {
            if trade.ts_ms > new_max_ts { new_max_ts = trade.ts_ms; }

            if trade.quantity < self.size_threshold {
                continue;
            }
            if let Some(notional_min) = self.notional_threshold {
                if trade.price * trade.quantity < notional_min {
                    continue;
                }
            }

            let public = PublicTrade {
                timestamp: trade.ts_ms,
                price: trade.price,
                quantity: trade.quantity,
                // TradePoint.side 0 = buyer maker → taker is seller → Sell
                side: if trade.side == 0 {
                    TradeSide::Sell
                } else {
                    TradeSide::Buy
                },
                arrival_ms,
                venue: None,
            };

            if public.quantity > self.max_qty_seen {
                self.max_qty_seen = public.quantity;
            }

            if self.big_trades.len() >= MAX_TRADES {
                self.big_trades.pop_front();
            }
            self.big_trades.push_back(public);
        }

        self.last_seen_ts_ms = new_max_ts;
    }

    /// Multi-venue pull (M/BT): drain every roster handle past its own
    /// cursor, apply size/notional thresholds per print, tag with the
    /// venue index, merge time-sorted into the shared buffer.
    fn tick_multi(&mut self) {
        const MAX_TRADES: usize = 1000;
        self.multi.sync_len();
        let arrival_ms = web_time::SystemTime::now()
            .duration_since(web_time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut merged: Vec<PublicTrade> = Vec::new();
        for i in 0..self.multi.entries.len() {
            let Some(handle) = self.multi.trade_handles[i].clone() else { continue };
            let guard = handle.blocking_read();
            let snapshot = guard.snapshot();
            drop(guard);
            let cursor = self.multi.trade_cursors[i];
            let mut new_max_ts = cursor;
            for trade in snapshot.iter().filter(|p| p.ts_ms > cursor) {
                if trade.ts_ms > new_max_ts {
                    new_max_ts = trade.ts_ms;
                }
                if trade.quantity < self.size_threshold {
                    continue;
                }
                if let Some(notional_min) = self.notional_threshold {
                    if trade.price * trade.quantity < notional_min {
                        continue;
                    }
                }
                merged.push(PublicTrade {
                    timestamp: trade.ts_ms,
                    price: trade.price,
                    quantity: trade.quantity,
                    side: if trade.side == 0 { TradeSide::Sell } else { TradeSide::Buy },
                    arrival_ms,
                    venue: Some(i as u8),
                });
            }
            self.multi.trade_cursors[i] = new_max_ts;
        }
        if merged.is_empty() {
            return;
        }
        merged.sort_by_key(|t| t.timestamp);
        for public in merged {
            if public.quantity > self.max_qty_seen {
                self.max_qty_seen = public.quantity;
            }
            if self.big_trades.len() >= MAX_TRADES {
                self.big_trades.pop_front();
            }
            self.big_trades.push_back(public);
        }
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
    pub fn handle_key(&mut self, _key: mlc_core::input::KeyCode) -> bool {
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
        coordinator: &mut uzor::InputCoordinator,
        slot_prefix: &str,
    ) {
        // Register interactive body rect for scroll / drag / double-click dispatch.
        {
            let body_id = format!("{}:bigtrades:body", slot_prefix);
            coordinator.register(
                body_id.as_str(),
                uzor::Rect::new(x as f64, y as f64, w as f64, h as f64),
                uzor::input::Sense::SCROLL | uzor::input::Sense::DRAG | uzor::input::Sense::DOUBLE_CLICK,
            );
        }
        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        // Multi mode: per-venue columns under one tagged header (по
        // образцу Multi-DOM, owner order 2026-07-31).
        if self.multi_mode {
            self.render_multi(ctx, x, y, w, h, theme);
            return;
        }

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
        // Skin dispatch (owner order 2026-07-31): Classic = the original
        // flat side-color size bar; Ramp = TapeSurf trades-feed style —
        // bright-endpoint mix by size + 250ms accent flash on fresh rows.
        let skin = super::trade_tape::TapeSkin::effective(self.skin, theme);
        let flat_bars =
            theme.style.bar_texture == mlc_core::theme::BarTexture::Flat;
        let now_ms = web_time::SystemTime::now()
            .duration_since(web_time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        for (i, trade) in trades.iter().enumerate() {
            let row_y = y + BT_HEADER_HEIGHT + (i as f32 * BT_ROW_HEIGHT);

            let bar_width = self.size_bar_width(trade, bar_max_width);
            let t_mix = if skin == super::trade_tape::TapeSkin::Classic
                || flat_bars
                || self.max_qty_seen <= 0.0
            {
                0.0
            } else {
                (trade.quantity / self.max_qty_seen).clamp(0.0, 1.0)
            };
            let (base, bright) = match trade.side {
                TradeSide::Buy => (&theme.buy, &theme.buy_bright),
                TradeSide::Sell => (&theme.sell, &theme.sell_bright),
            };
            let bar_color = mlc_core::mix_colors(base, bright, t_mix);
            ctx.set_fill_color(&bar_color);
            ctx.fill_rect(x as f64, row_y as f64, bar_width as f64, BT_ROW_HEIGHT as f64);
            if skin == super::trade_tape::TapeSkin::Ramp
                && now_ms.saturating_sub(trade.arrival_ms) < 250
            {
                ctx.set_fill_color(&mlc_core::apply_opacity(&theme.accent, 0.22));
                ctx.fill_rect(x as f64, row_y as f64, w as f64, BT_ROW_HEIGHT as f64);
            }

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

    fn handle_scroll(&mut self, local_id: &str, _dx: f64, dy: f64) -> bool {
        if local_id == "bigtrades:body" {
            BigTradesState::handle_scroll(self, dy);
            true
        } else {
            false
        }
    }

    fn handle_drag_start(&mut self, local_id: &str, _x: f64, _y: f64) -> bool {
        local_id == "bigtrades:body"
    }

    fn handle_drag_move(&mut self, local_id: &str, dx: f64, dy: f64) -> bool {
        if local_id == "bigtrades:body" {
            BigTradesState::handle_drag(self, dx, dy);
            true
        } else {
            false
        }
    }

    fn handle_drag_end(&mut self, _local_id: &str) -> bool {
        true
    }

    fn handle_double_click(&mut self, local_id: &str, _x: f64, _y: f64) -> bool {
        if local_id == "bigtrades:body" {
            BigTradesState::handle_double_click(self);
            true
        } else {
            false
        }
    }
}

impl BigTradesState {
    /// M/BT columnar render (по образцу Multi-DOM, owner order
    /// 2026-07-31): one venue per column, one unifying tagged header.
    /// Each column shows ITS venue's threshold-passing prints newest-at-
    /// top: size bar underlay + price + size (same visual language as the
    /// single panel).
    fn render_multi(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        theme: &crate::panel_theme::PanelTheme,
    ) {
        let n = self.multi.entries.len();
        if n == 0 {
            ctx.set_fill_color(&theme.text_muted);
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(
                "Multi tape — add venues via the header picker",
                (x + w * 0.5) as f64,
                (y + h * 0.5) as f64,
            );
            return;
        }
        let col_w = w / n as f32;
        let narrow = col_w < 72.0;

        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, BT_HEADER_HEIGHT as f64);
        ctx.set_font(if narrow { "8px monospace" } else { "10px monospace" });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        for (i, e) in self.multi.entries.iter().enumerate() {
            let live = self
                .multi
                .trade_handles
                .get(i)
                .map(|h| h.is_some())
                .unwrap_or(false);
            ctx.set_fill_color(if live { &theme.text_primary } else { &theme.text_muted });
            let label = if narrow {
                self.multi.venue_tag(i)
            } else {
                super::multi_dom::col_label(e)
            };
            let cx = x + col_w * i as f32 + col_w * 0.5;
            ctx.fill_text(&label, cx as f64, (y + BT_HEADER_HEIGHT * 0.5) as f64);
        }

        let content_h = h - BT_HEADER_HEIGHT;
        let max_rows = (content_h / BT_ROW_HEIGHT).floor() as usize;
        if max_rows == 0 {
            return;
        }
        let skip = (self.scroll_offset as usize).min(self.big_trades.len());
        let skin = super::trade_tape::TapeSkin::effective(self.skin, theme);
        let flat_bars = theme.style.bar_texture == mlc_core::theme::BarTexture::Flat;
        let now_ms = web_time::SystemTime::now()
            .duration_since(web_time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        ctx.set_font(if narrow { "9px monospace" } else { "10px monospace" });
        for i in 0..n {
            let cx0 = x + col_w * i as f32;
            let rows = self
                .big_trades
                .iter()
                .rev()
                .filter(|t| t.venue == Some(i as u8))
                .skip(skip)
                .take(max_rows);
            for (row_idx, trade) in rows.enumerate() {
                let row_y = y + BT_HEADER_HEIGHT + row_idx as f32 * BT_ROW_HEIGHT;
                let text_y = (row_y + BT_ROW_HEIGHT / 2.0) as f64;
                let t_mix = if skin == super::trade_tape::TapeSkin::Classic
                    || flat_bars
                    || self.max_qty_seen <= 0.0
                {
                    0.0
                } else {
                    (trade.quantity / self.max_qty_seen).clamp(0.0, 1.0)
                };
                let (base, bright) = match trade.side {
                    TradeSide::Buy => (&theme.buy, &theme.buy_bright),
                    TradeSide::Sell => (&theme.sell, &theme.sell_bright),
                };
                let bar_w = self.size_bar_width(trade, col_w - 2.0);
                ctx.set_fill_color(&mlc_core::mix_colors(base, bright, t_mix));
                ctx.fill_rect(cx0 as f64, row_y as f64, bar_w as f64, BT_ROW_HEIGHT as f64);
                if skin == super::trade_tape::TapeSkin::Ramp
                    && now_ms.saturating_sub(trade.arrival_ms) < 250
                {
                    ctx.set_fill_color(&mlc_core::apply_opacity(&theme.accent, 0.22));
                    ctx.fill_rect(cx0 as f64, row_y as f64, col_w as f64, BT_ROW_HEIGHT as f64);
                }
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_fill_color(match trade.side {
                    TradeSide::Buy => &theme.buy,
                    TradeSide::Sell => &theme.sell,
                });
                ctx.fill_text(&format!("{:.2}", trade.price), (cx0 + 3.0) as f64, text_y);
                ctx.set_text_align(TextAlign::Right);
                ctx.set_fill_color(&theme.text_primary);
                ctx.fill_text(
                    &format!("{:.4}", trade.quantity),
                    (cx0 + col_w - 3.0) as f64,
                    text_y,
                );
                ctx.set_fill_color(&theme.separator);
                ctx.fill_rect(cx0 as f64, (row_y + BT_ROW_HEIGHT - 1.0) as f64, col_w as f64, 1.0);
            }
        }

        ctx.set_fill_color(&theme.separator);
        for i in 1..n {
            let sx = (x + col_w * i as f32) as f64;
            ctx.fill_rect(sx, y as f64, 1.0, h as f64);
        }
    }
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
    pub fn min_size(&self) -> (f32, f32) { (100.0, 80.0) }
}
