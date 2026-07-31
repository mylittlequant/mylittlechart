//! Trade Tape: Time & Sales panel for executed trades only.
//!
//! Shows real public trades (fills) streamed from the exchange — NOT L2 book
//! events. Alpha-by-size: row background opacity scales with
//! `(qty / max_qty_seen).sqrt().clamp(0.15, 1.0)` so big prints glow brighter.

use std::collections::VecDeque;
use std::sync::Arc;
use web_time::SystemTime;
use web_time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock as TokioRwLock;
use digdigdig3_station::series::Series as StationSeries;
use digdigdig3_station::data::TradePoint;

/// Handle to the station's per-(exchange, symbol) trade ring.
pub type SharedTradeSeries = Arc<TokioRwLock<StationSeries<TradePoint>>>;

/// Handle to the station's per-(exchange, symbol) live orderbook reconstructor.
///
/// After Phase 2B step 3 (orderbook), panels hold the station's `OrderBookTracker`
/// directly; the mlc-side `OrderbookSeries` wrapper is gone.
pub type SharedOrderBookTracker = Arc<TokioRwLock<digdigdig3_station::orderbook::OrderBookTracker>>;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::i18n::{TradingKey, current_language};

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

/// Tape rendering skin (DomSkin pattern, owner order 2026-07-31):
/// `Classic` = the original flat side-color rows (default — the
/// pre-skin look, untouched); `Ramp` = TapeSurf trades-feed style
/// (bright-mix intensity ramp + 250ms new-row accent flash). The
/// theme-skin's `tape_skin` sets the default; a manual per-panel pick
/// overrides it. Shared by Trade Tape and Big Trades.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum TapeSkin {
    #[default]
    Classic,
    Ramp,
}

impl TapeSkin {
    pub fn label(self) -> &'static str {
        match self {
            TapeSkin::Classic => "Classic",
            TapeSkin::Ramp => "Ramp",
        }
    }
    pub fn next(self) -> TapeSkin {
        match self {
            TapeSkin::Classic => TapeSkin::Ramp,
            TapeSkin::Ramp => TapeSkin::Classic,
        }
    }
    /// Resolve the effective skin: manual pick wins, else the theme-skin
    /// default.
    pub fn effective(
        manual: Option<TapeSkin>,
        theme: &crate::panel_theme::PanelTheme,
    ) -> TapeSkin {
        manual.unwrap_or(match theme.style.tape_skin {
            mlc_core::theme::TapeSkinDefault::Classic => TapeSkin::Classic,
            mlc_core::theme::TapeSkinDefault::Ramp => TapeSkin::Ramp,
        })
    }
}

/// A single executed public trade.
#[derive(Clone, Debug)]
pub struct TapeEntry {
    /// Exchange timestamp in milliseconds.
    pub timestamp: i64,
    pub price: f64,
    pub quantity: f64,
    /// `true` = taker was buyer (aggressive buy); `false` = aggressive sell.
    pub is_buy: bool,
    /// Local wall-clock at ingest (ms) — drives the new-row flash
    /// (TapeSurf trades-feed polish, 2026-07-31).
    pub arrival_ms: u64,
    /// Roster index of the source venue in multi mode (`None` =
    /// single-venue entry). Drives the EXCHANGE column (TapeSurf ref 17).
    pub venue: Option<u8>,
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

    /// Shared trade ring owned by dig3-station's forwarder.
    pub shared_trades: Option<SharedTradeSeries>,
    /// Timestamp (ms) of the newest trade we have already pulled into `self.trades`.
    /// Used as a per-consumer cursor so multi-panel sharing of one `Series<TradePoint>`
    /// stays independent.
    pub last_seen_ts_ms: i64,

    /// Market price synced from a linked DOM panel.
    pub dom_market_price: Option<f64>,
    /// Tick size from a linked DOM panel.
    pub dom_tick_size: Option<f64>,

    /// Crosshair price propagated from a linked chart in the same sync group.
    pub crosshair_price: Option<f64>,

    /// Which columns to show.
    pub column_config: TradeTapeColumnConfig,

    /// Manual skin pick — `None` follows the theme-skin's `tape_skin`
    /// default (DomSkin precedence pattern).
    pub skin: Option<TapeSkin>,

    /// Multi-venue mode (M/TT, по образцу Multi-DOM, owner 2026-07-31).
    /// When ON, `tick` pulls a merged time-sorted stream from the roster
    /// handles and render shows the EXCHANGE column; the single-venue
    /// fields above are untouched and survive toggling back.
    pub multi_mode: bool,
    /// Embedded multi-venue roster + per-venue feed state.
    pub multi: super::multi_tape::MultiTapeState,
}

impl std::fmt::Debug for TradeTapeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TradeTapeState")
            .field("symbol", &self.symbol)
            .field("exchange", &self.exchange)
            .field("trades_len", &self.trades.len())
            .field("paused", &self.paused)
            .field("max_qty_seen", &self.max_qty_seen)
            .field("last_seen_ts_ms", &self.last_seen_ts_ms)
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
            last_seen_ts_ms: 0,
            dom_market_price: None,
            dom_tick_size: None,
            crosshair_price: None,
            column_config: TradeTapeColumnConfig::default(),
            skin: None,
            multi_mode: false,
            multi: super::multi_tape::MultiTapeState::new(),
        }
    }

    /// Toggle multi-venue mode (M/TT). On enable: re-sync the roster's
    /// base symbol with the panel's (stale handles drop, self-heal
    /// re-provisions) and seed the roster with the panel's own venue on
    /// first use. The visible buffer is cleared on every switch — the two
    /// modes stream different feeds. Subscriptions are the caller's job
    /// (app layer reads `multi.entries` after this call).
    pub fn set_multi_mode(&mut self, on: bool) {
        if self.multi_mode == on {
            return;
        }
        self.multi_mode = on;
        self.trades.clear();
        self.pause_buffer.clear();
        self.scroll_offset = 0.0;
        self.auto_scroll = true;
        self.paused = false;
        self.max_qty_seen = 0.0;
        if on {
            if !self.multi.symbol.eq_ignore_ascii_case(&self.symbol) {
                self.multi.reset_for_symbol(&self.symbol.clone());
            }
            if self.multi.entries.is_empty() && !self.exchange.is_empty() {
                self.multi.add_entry(&super::multi_dom::col_entry(
                    &self.exchange,
                    &self.account_type,
                ));
            }
        } else {
            // Single feed resumes from the ring's history.
            self.last_seen_ts_ms = 0;
        }
    }

    // ─── tick ─────────────────────────────────────────────────────────────

    /// Pull new trades from `shared_trades`, update window stats, and prune stale entries.
    ///
    /// Call once per frame before render.
    pub fn tick(&mut self) {
        if self.multi_mode {
            self.pull_new_trades_multi();
        } else {
            self.pull_new_trades();
        }
        self.prune_retention();
        self.recompute_window_stats();
    }

    /// Multi-venue pull (M/TT): drain every roster handle past its own
    /// cursor, tag entries with the venue index, merge time-sorted into
    /// the shared buffer.
    fn pull_new_trades_multi(&mut self) {
        self.multi.sync_len();
        let arrival_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut merged: Vec<TapeEntry> = Vec::new();
        for i in 0..self.multi.entries.len() {
            let Some(handle) = self.multi.trade_handles[i].clone() else { continue };
            let guard = handle.blocking_read();
            let snapshot = guard.snapshot();
            drop(guard);
            let cursor = self.multi.trade_cursors[i];
            let mut new_max_ts = cursor;
            for p in snapshot {
                if p.ts_ms <= cursor {
                    continue;
                }
                if p.ts_ms > new_max_ts {
                    new_max_ts = p.ts_ms;
                }
                merged.push(TapeEntry {
                    timestamp: p.ts_ms,
                    price: p.price,
                    quantity: p.quantity,
                    is_buy: p.side != 0,
                    arrival_ms,
                    venue: Some(i as u8),
                });
            }
            self.multi.trade_cursors[i] = new_max_ts;
        }
        if merged.is_empty() {
            return;
        }
        merged.sort_by_key(|e| e.timestamp);
        for entry in merged {
            if self.paused {
                if self.pause_buffer.len() >= self.max_trades {
                    self.pause_buffer.pop_front();
                }
                self.pause_buffer.push_back(entry);
            } else {
                self.push_entry(entry);
            }
        }
    }

    fn pull_new_trades(&mut self) {
        let handle = match self.shared_trades.clone() {
            Some(h) => h,
            None => return,
        };
        // tokio::sync::RwLock — sync context, blocking_read is safe on the
        // render thread (the station forwarder holds write() for microseconds
        // per push).
        let guard = handle.blocking_read();
        let snapshot = guard.snapshot();
        drop(guard);

        let cursor = self.last_seen_ts_ms;
        let mut new_max_ts = cursor;
        let arrival_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let new_entries: Vec<TapeEntry> = snapshot
            .into_iter()
            .filter(|p| p.ts_ms > cursor)
            .map(|p| {
                if p.ts_ms > new_max_ts { new_max_ts = p.ts_ms; }
                TapeEntry {
                    timestamp: p.ts_ms,
                    price: p.price,
                    quantity: p.quantity,
                    // TradePoint.side 0 = buyer maker -> taker is seller -> is_buy=false
                    is_buy: p.side != 0,
                    arrival_ms,
                    venue: None,
                }
            })
            .collect();

        if new_entries.is_empty() {
            return;
        }

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

        self.last_seen_ts_ms = new_max_ts;
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
    pub fn handle_key(&mut self, _key: mlc_core::input::KeyCode) -> bool {
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

        // ── multi mode: per-venue columns under one tagged header ─────────
        // (по образцу Multi-DOM, owner order 2026-07-31 — NOT a merged
        // list with a text venue column).
        if self.multi_mode {
            self.render_multi(ctx, x, y, w, h, theme);
            return;
        }

        // ── dynamic column layout ─────────────────────────────────────────
        // Build list of visible columns in order, then distribute width evenly.
        #[derive(Clone, Copy, PartialEq)]
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
        // Skin dispatch (owner order 2026-07-31): Classic = the original
        // flat side-color rows; Ramp = TapeSurf trades-feed style —
        // bright-mix intensity + 250ms accent flash on fresh rows.
        let skin = TapeSkin::effective(self.skin, theme);
        let flat_bars =
            theme.style.bar_texture == mlc_core::theme::BarTexture::Flat;
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        for (row_idx, entry) in entries.iter().enumerate() {
            let row_y = y + TT_HEADER_HEIGHT + (row_idx as f32 * TT_ROW_HEIGHT);
            let mid_y = (row_y + TT_ROW_HEIGHT / 2.0) as f64;

            // Colored background with alpha-by-size; Ramp adds the
            // bright-endpoint mix by size.
            let alpha = self.row_alpha(entry.quantity);
            let t_mix = if skin == TapeSkin::Classic
                || flat_bars
                || self.max_qty_seen <= 0.0
            {
                0.0
            } else {
                (entry.quantity / self.max_qty_seen).clamp(0.0, 1.0)
            };
            let (base, bright) = if entry.is_buy {
                (&theme.buy, &theme.buy_bright)
            } else {
                (&theme.sell, &theme.sell_bright)
            };
            let bg_color = mlc_core::apply_opacity(
                &mlc_core::mix_colors(base, bright, t_mix),
                alpha,
            );
            ctx.set_fill_color(&bg_color);
            ctx.fill_rect(x as f64, row_y as f64, w as f64, TT_ROW_HEIGHT as f64);
            if skin == TapeSkin::Ramp && now_ms.saturating_sub(entry.arrival_ms) < 250 {
                ctx.set_fill_color(&mlc_core::apply_opacity(&theme.accent, 0.22));
                ctx.fill_rect(x as f64, row_y as f64, w as f64, TT_ROW_HEIGHT as f64);
            }

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
            ctx.fill_text(TradingKey::NoTrades.get(current_language()), (x + w / 2.0) as f64, (y + h / 2.0) as f64);
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
            ctx.fill_text(TradingKey::Paused.get(current_language()), (x + w / 2.0) as f64, badge_y + badge_h / 2.0);
        }
    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool { false }
}

impl TradeTapeState {
    /// M/TT columnar render (по образцу Multi-DOM, owner order
    /// 2026-07-31): one venue per column side by side, one unifying
    /// header row tagged per venue. Each column streams ITS venue's
    /// prints newest-at-top (price + size; row bg = the same
    /// alpha-by-size ramp + skin flash the single tape uses).
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

        // ── Unifying header: one tag per venue column (Multi-DOM grammar:
        // full "EXCHANGE:AT", short "BI:F" when narrow; muted = dead feed).
        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, TT_HEADER_HEIGHT as f64);
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
            ctx.fill_text(&label, cx as f64, (y + TT_HEADER_HEIGHT * 0.5) as f64);
        }

        // ── Body: per-venue row stacks. Scroll/pause state is shared —
        // every column shifts together.
        let content_h = h - TT_HEADER_HEIGHT;
        let max_rows = (content_h / TT_ROW_HEIGHT).floor() as usize;
        if max_rows == 0 {
            return;
        }
        let skip = if self.auto_scroll { 0 } else { self.scroll_offset as usize };
        let tick_size = self.dom_tick_size.unwrap_or(0.01);
        let skin = TapeSkin::effective(self.skin, theme);
        let flat_bars = theme.style.bar_texture == mlc_core::theme::BarTexture::Flat;
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        ctx.set_font(if narrow { "9px monospace" } else { "10px monospace" });
        for i in 0..n {
            let cx0 = x + col_w * i as f32;
            let rows = self
                .trades
                .iter()
                .rev()
                .filter(|e| e.venue == Some(i as u8))
                .filter(|e| e.quantity >= self.filter_min_qty)
                .skip(skip)
                .take(max_rows);
            for (row_idx, entry) in rows.enumerate() {
                let row_y = y + TT_HEADER_HEIGHT + row_idx as f32 * TT_ROW_HEIGHT;
                let mid_y = (row_y + TT_ROW_HEIGHT / 2.0) as f64;
                let alpha = self.row_alpha(entry.quantity);
                let t_mix = if skin == TapeSkin::Classic || flat_bars || self.max_qty_seen <= 0.0 {
                    0.0
                } else {
                    (entry.quantity / self.max_qty_seen).clamp(0.0, 1.0)
                };
                let (base, bright) = if entry.is_buy {
                    (&theme.buy, &theme.buy_bright)
                } else {
                    (&theme.sell, &theme.sell_bright)
                };
                let bg = mlc_core::apply_opacity(&mlc_core::mix_colors(base, bright, t_mix), alpha);
                ctx.set_fill_color(&bg);
                ctx.fill_rect(cx0 as f64, row_y as f64, col_w as f64, TT_ROW_HEIGHT as f64);
                if skin == TapeSkin::Ramp && now_ms.saturating_sub(entry.arrival_ms) < 250 {
                    ctx.set_fill_color(&mlc_core::apply_opacity(&theme.accent, 0.22));
                    ctx.fill_rect(cx0 as f64, row_y as f64, col_w as f64, TT_ROW_HEIGHT as f64);
                }
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_fill_color(if entry.is_buy { &theme.buy_bright } else { &theme.sell_bright });
                ctx.fill_text(
                    &Self::format_price(entry.price, tick_size),
                    (cx0 + 3.0) as f64,
                    mid_y,
                );
                ctx.set_text_align(TextAlign::Right);
                ctx.set_fill_color(&theme.text_primary);
                ctx.fill_text(
                    &Self::format_qty(entry.quantity),
                    (cx0 + col_w - 3.0) as f64,
                    mid_y,
                );
            }
        }

        // Column separators over the full panel height.
        ctx.set_fill_color(&theme.separator);
        for i in 1..n {
            let sx = (x + col_w * i as f32) as f64;
            ctx.fill_rect(sx, y as f64, 1.0, h as f64);
        }

        // PAUSED badge — shared scroll state.
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
            ctx.fill_text(
                TradingKey::Paused.get(current_language()),
                (x + w / 2.0) as f64,
                badge_y + badge_h / 2.0,
            );
        }
    }
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
