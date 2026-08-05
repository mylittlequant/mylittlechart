//! L2 Tape: Order Book Event Stream panel state.
//!
//! Shows individual MBO (Market-By-Order) events: order additions,
//! modifications, cancellations, and executions in real-time.

use std::collections::{HashMap, VecDeque};
use web_time::SystemTime;
use web_time::UNIX_EPOCH;
use serde::{Serialize, Deserialize};

use rust_decimal::prelude::ToPrimitive;

use crate::trading::order_flow::trade_tape::SharedOrderBookTracker;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::i18n::{TradingKey, current_language};

/// Types of L2 order book events
#[derive(Clone, Debug, PartialEq)]
pub enum L2EventType {
    /// New order added to the book
    Add,
    /// Existing order modified (price or quantity changed)
    Modify,
    /// Order cancelled/removed from the book
    Cancel,
    /// Order executed (partial or full fill)
    Execute,
}

/// Side of the order book
#[derive(Clone, Debug, PartialEq)]
pub enum L2Side {
    Bid,
    Ask,
}

/// A single L2 order book event
#[derive(Clone, Debug)]
pub struct L2Event {
    /// Timestamp in milliseconds
    pub timestamp: i64,
    /// Event type
    pub event_type: L2EventType,
    /// Order side
    pub side: L2Side,
    /// Price level
    pub price: f64,
    /// Quantity (size of order or fill)
    pub quantity: f64,
    /// Order count affected (optional)
    pub order_count: Option<usize>,
    /// Order ID (if available from exchange)
    pub order_id: Option<String>,
    /// Roster index of the source venue in multi mode (`None` =
    /// single-venue event). Drives the EXCHANGE column (TapeSurf ref 17).
    pub venue: Option<u8>,
    /// `true` on a Cancel that removed a LARGE add shortly after it was
    /// placed (spoof detector, owner order 2026-08-01) — render marks the
    /// row.
    pub spoof: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct L2TapeColumnConfig {
    pub show_time: bool,
    pub show_type: bool,
    pub show_side: bool,
    pub show_price: bool,
    pub show_qty: bool,
}

impl Default for L2TapeColumnConfig {
    fn default() -> Self {
        Self {
            show_time: true,
            show_type: true,
            show_side: true,
            show_price: true,
            show_qty: true,
        }
    }
}

/// L2 Tape panel state
#[derive(Clone, Debug)]
pub struct L2TapeState {
    /// Symbol being monitored
    pub symbol: String,

    /// Frame separator on/off — visually delineate WS frame boundaries.
    /// Default: true. Binance @depth@100ms emits one frame with N levels sharing
    /// a single E timestamp; this separator shows those groupings honestly.
    pub frame_separator: bool,

    /// Exchange identifier string (e.g. "binance")
    pub exchange: String,

    /// Account type short label (e.g. "S", "F", "FI")
    pub account_type: String,
    /// Ring buffer of recent L2 events
    pub events: VecDeque<L2Event>,
    /// Maximum events to keep in buffer
    pub max_events: usize,
    /// Time-based retention in milliseconds (default 120 000 ms = 2 min).
    /// Events older than `retention_ms` are drained from the front on each tick.
    pub retention_ms: u64,
    /// Maximum quantity seen in the current event buffer.
    /// Recalculated after time-based pruning; used to scale row alpha.
    pub max_qty_seen: f64,
    /// Auto-scroll to latest event
    pub auto_scroll: bool,
    /// Filter by event type
    pub filter_type: Option<L2EventType>,
    /// Filter by side
    pub filter_side: Option<L2Side>,
    /// Minimum quantity filter
    pub min_quantity: Option<f64>,
    /// DOM market price (synced from linked DOM)
    pub dom_market_price: Option<f64>,
    /// DOM tick size (synced from linked DOM)
    pub dom_tick_size: Option<f64>,
    /// Scroll offset
    pub scroll_offset: f64,
    /// Flash animation state: `(events_len_at_push, flash_start_ms)`.
    ///
    /// When an event is pushed with serial = `events.len()` at push time,
    /// render maps visible event → its serial and checks for a matching entry
    /// whose age is < 200 ms.  Expired entries are pruned in `tick()`.
    pub flash_events: Vec<(usize, u64)>,

    /// Monotone counter: total events ever pushed (never decrements).
    /// Identifies each event uniquely even after the front of the deque
    /// is evicted.  `events[i]` has serial `push_total - events.len() + i`.
    pub push_total: usize,
    /// Spoofing detection: recent large order adds that cancelled quickly
    pub spoof_alerts: VecDeque<SpoofAlert>,
    /// Previous orderbook state for event classification (Add vs Modify vs Cancel)
    pub previous_book: HashMap<i64, (f64, f64)>,  // tick -> (bid_qty, ask_qty)
    /// Tick size for price-to-tick conversion
    pub tick_size: f64,
    /// Crosshair price synced from a linked chart window.
    /// When set, events at this price level are highlighted.
    pub crosshair_price: Option<f64>,

    /// Live orderbook reconstructor. L2-tape reads `bbo()` from this for best-bid/ask
    /// metadata display. Raw delta events still arrive via the broadcast channel
    /// (`apply_ob_update_with_ts`) because L2-tape needs per-event timestamps.
    pub shared_orderbook: Option<SharedOrderBookTracker>,

    /// `last_update_id` from the most recent tracker read. Used to skip tick() when
    /// the book has not advanced.
    pub last_seen_update_id: Option<u64>,

    /// Column visibility configuration.
    pub column_config: L2TapeColumnConfig,

    /// Multi-venue mode (M/L2, по образцу Multi-DOM, owner 2026-07-31).
    /// When ON, `tick` runs the snapshot diff per roster venue and events
    /// carry the venue index; single-venue fields survive toggling back.
    pub multi_mode: bool,
    /// Embedded multi-venue roster + per-venue book/diff state.
    pub multi: super::multi_tape::MultiTapeState,

    /// Add/Cancel imbalance footer strip (owner order 2026-08-01): is the
    /// book FILLING (add volume) or DRAINING (cancel volume) over the
    /// window. Modal "Imb" toggle.
    pub show_imbalance: bool,
    /// Imbalance window in minutes (modal click-cycle 1/2/5/10).
    pub imb_window_min: u8,
    /// Spoof-detector memory: recent LARGE adds by `(venue, tick)` →
    /// `(qty, arrival ts_ms)`. A Cancel of the same level within the
    /// spoof window marks the event + records a [`SpoofAlert`].
    pub last_big_adds: HashMap<(Option<u8>, i64), (f64, i64)>,

    /// Per-event-type visibility toggles (owner order 2026-08-01
    /// «включение выключение какие именно типы событий показывать»).
    /// Rows only — the lifecycle footer keeps counting every type.
    pub show_adds: bool,
    pub show_mods: bool,
    pub show_cxls: bool,
    pub show_execs: bool,
}

/// Spoofing alert data
#[derive(Clone, Debug)]
pub struct SpoofAlert {
    pub timestamp: i64,
    pub price: f64,
    pub side: L2Side,
    pub quantity: f64,
    pub duration_ms: u64,  // how quickly the order was cancelled
}

/// L2 Tape panel ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct L2TapeId(pub u64);

/// Lightweight L2 Tape panel wrapper (for PanelKind)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct L2TapePanel {
    id: L2TapeId,
    title: String,
}

impl L2TapePanel {
    pub fn new(id: L2TapeId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> L2TapeId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "l2_tape" }
    pub fn kind_label(&self) -> &'static str { "L2 Tape" }
    pub fn min_size(&self) -> (f32, f32) { (100.0, 80.0) }
}

const L2_ROW_HEIGHT: f32 = 16.0;
const L2_HEADER_HEIGHT: f32 = 16.0;
const L2_FOOTER_HEIGHT: f32 = 16.0;
const L2_LEFT_PAD: f32 = 6.0;

/// Format a quantity value using K / M suffixes when the magnitude warrants it.
fn abbreviate_qty(val: f64) -> String {
    if val >= 1_000_000.0 {
        let m = val / 1_000_000.0;
        if (m - m.floor()).abs() < 0.05 {
            format!("{:.0}M", m)
        } else {
            format!("{:.1}M", m)
        }
    } else if val >= 1_000.0 {
        let k = val / 1_000.0;
        if (k - k.floor()).abs() < 0.05 {
            format!("{:.0}K", k)
        } else {
            format!("{:.1}K", k)
        }
    } else if val >= 1.0 {
        format!("{:.2}", val)
    } else {
        format!("{:.4}", val)
    }
}

impl TradingPanel for L2TapeState {
    fn kind(&self) -> &'static str { "l2_tape" }
    fn label(&self) -> &'static str { "L2 Tape" }

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
            let body_id = format!("{}:l2tape:body", slot_prefix);
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

        // Build visible column list from config.
        // Each entry: (label, original fraction).
        let all_cols: &[(&str, f32, bool)] = &[
            ("TIME",  0.28, self.column_config.show_time),
            ("TYPE",  0.12, self.column_config.show_type),
            ("SIDE",  0.12, self.column_config.show_side),
            ("PRICE", 0.24, self.column_config.show_price),
            ("QTY",   0.24, self.column_config.show_qty),
        ];
        let total_frac: f32 = all_cols.iter()
            .filter(|&&(_, _, vis)| vis)
            .map(|&(_, frac, _)| frac)
            .sum();
        // Compute X positions for each column (None when hidden).
        let usable_w = w - L2_LEFT_PAD;
        let mut col_x = [Option::<f32>::None; 5];
        let mut cursor = x + L2_LEFT_PAD;
        for (i, &(_, frac, vis)) in all_cols.iter().enumerate() {
            if vis {
                col_x[i] = Some(cursor);
                let col_w = if total_frac > 0.0 { usable_w * frac / total_frac } else { 0.0 };
                cursor += col_w;
            }
        }
        let col_time_x  = col_x[0];
        let col_type_x  = col_x[1];
        let col_side_x  = col_x[2];
        let col_price_x = col_x[3];
        let col_qty_x   = col_x[4];

        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, L2_HEADER_HEIGHT as f64);

        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&theme.text_header);

        let lang = current_language();
        let header_text_y = (y + L2_HEADER_HEIGHT / 2.0) as f64;
        if let Some(cx) = col_time_x  { ctx.fill_text(TradingKey::Time.get(lang),  cx as f64, header_text_y); }
        if let Some(cx) = col_type_x  { ctx.fill_text(TradingKey::Type.get(lang),  cx as f64, header_text_y); }
        if let Some(cx) = col_side_x  { ctx.fill_text(TradingKey::Side.get(lang),  cx as f64, header_text_y); }
        if let Some(cx) = col_price_x { ctx.fill_text(TradingKey::Price.get(lang), cx as f64, header_text_y); }
        if let Some(cx) = col_qty_x   { ctx.fill_text(TradingKey::Qty.get(lang),   cx as f64, header_text_y); }

        if !self.symbol.is_empty() {
            ctx.set_font("9px sans-serif");
            ctx.set_text_align(TextAlign::Right);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_fill_color(&theme.text_muted);
            ctx.fill_text(&self.symbol, (x + w - 6.0) as f64, header_text_y);
        }

        // Compute wall-clock time once for S5 rate badge and flash detection below.
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // S5: rate badge — frames/sec + levels/sec from last 1 second of events.
        {
            let cutoff = (now_ms as i64).saturating_sub(1000);
            let mut uniq_ts: std::collections::HashSet<i64> = std::collections::HashSet::new();
            let mut total_levels = 0usize;
            for ev in self.events.iter() {
                if ev.timestamp >= cutoff {
                    uniq_ts.insert(ev.timestamp);
                    total_levels += 1;
                }
            }
            let frames_per_sec = uniq_ts.len();
            let levels_per_sec = total_levels;
            let badge_text = format!("{}f/s {}l/s", frames_per_sec, levels_per_sec);
            let symbol_w = if !self.symbol.is_empty() {
                self.symbol.len() as f64 * 5.5
            } else {
                0.0
            };
            let badge_x = (x + w - 6.0) as f64 - symbol_w - 10.0;
            ctx.set_font("9px sans-serif");
            ctx.set_text_align(TextAlign::Right);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_fill_color(&theme.text_muted);
            ctx.fill_text(&badge_text, badge_x, header_text_y);
        }

        let footer_h = if self.show_imbalance { L2_FOOTER_HEIGHT } else { 0.0 };
        let content_h = h - L2_HEADER_HEIGHT - footer_h;
        let max_rows = (content_h / L2_ROW_HEIGHT).floor() as usize;
        if max_rows == 0 {
            return;
        }

        let events = self.visible_events(max_rows);

        // Build flash-active serial set once per frame.
        let flash_active: std::collections::HashSet<usize> = self.flash_events
            .iter()
            .filter(|&&(_, start)| now_ms.saturating_sub(start) < 200)
            .map(|&(serial, _)| serial)
            .collect();

        let decimals = if self.tick_size >= 1.0 {
            0usize
        } else if self.tick_size >= 0.1 {
            1
        } else if self.tick_size >= 0.01 {
            2
        } else if self.tick_size >= 0.001 {
            3
        } else {
            4
        };

        let mut prev_ts: Option<i64> = None;
        for (row_idx, (deque_idx, event)) in events.iter().enumerate() {
            let row_y = y + L2_HEADER_HEIGHT + (row_idx as f32 * L2_ROW_HEIGHT);
            let row_mid_y = (row_y + L2_ROW_HEIGHT / 2.0) as f64;

            // Zebra base
            let row_bg = if row_idx % 2 == 0 { &theme.panel_bg } else { &theme.row_bg_alt };
            ctx.set_fill_color(row_bg);
            ctx.fill_rect(x as f64, row_y as f64, w as f64, L2_ROW_HEIGHT as f64);

            // Chromatic overlay: color by event type + side, alpha scales with qty.
            let chroma_color = self.row_chroma_color(event, theme);
            ctx.set_fill_color(&chroma_color);
            ctx.fill_rect(x as f64, row_y as f64, w as f64, L2_ROW_HEIGHT as f64);

            // Flash overlay: bright semi-transparent rect for recently-pushed events.
            let serial = self.event_serial(*deque_idx);
            if flash_active.contains(&serial) {
                ctx.set_fill_color(&mlc_core::apply_opacity(&theme.text_primary, 0.16));
                ctx.fill_rect(x as f64, row_y as f64, w as f64, L2_ROW_HEIGHT as f64);
            }

            // Spoof marker (owner order 2026-08-01): bright edge on a
            // Cancel that pulled a fresh large add — DOM big-marker
            // grammar, current-price (gold) token. Wall marker: a LARGE
            // Add (same threshold family) gets the side-bright edge —
            // «стена встала».
            if event.spoof {
                ctx.set_fill_color(&mlc_core::apply_opacity(&theme.current_price, 0.9));
                ctx.fill_rect(x as f64, row_y as f64, 3.0, L2_ROW_HEIGHT as f64);
            } else if event.event_type == L2EventType::Add
                && self.max_qty_seen > 0.0
                && event.quantity >= self.max_qty_seen * Self::SPOOF_BIG_FRAC
            {
                ctx.set_fill_color(&mlc_core::apply_opacity(
                    match event.side {
                        L2Side::Bid => &theme.buy_bright,
                        L2Side::Ask => &theme.sell_bright,
                    },
                    0.9,
                ));
                ctx.fill_rect(x as f64, row_y as f64, 3.0, L2_ROW_HEIGHT as f64);
            }

            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);

            if let Some(cx) = col_time_x {
                let total_secs = (event.timestamp / 1000) % 86400;
                let hours  = total_secs / 3600;
                let mins   = (total_secs % 3600) / 60;
                let secs   = total_secs % 60;
                let millis = event.timestamp % 1000;
                let time_str = format!("{:02}:{:02}:{:02}.{:03}", hours, mins, secs, millis);
                ctx.set_fill_color(&theme.text_primary);
                ctx.fill_text(&time_str, cx as f64, row_mid_y);
            }

            if let Some(cx) = col_type_x {
                if event.spoof {
                    ctx.set_fill_color(&theme.current_price);
                    ctx.fill_text("SPF", cx as f64, row_mid_y);
                } else {
                    let type_color = self.event_color(event, theme);
                    ctx.set_fill_color(&type_color);
                    ctx.fill_text(L2TapeState::event_label(&event.event_type), cx as f64, row_mid_y);
                }
            }

            if let Some(cx) = col_side_x {
                let side_color = match event.side {
                    L2Side::Bid => &theme.buy,
                    L2Side::Ask => &theme.sell,
                };
                ctx.set_fill_color(side_color);
                ctx.fill_text(L2TapeState::side_label(&event.side), cx as f64, row_mid_y);
            }

            if let Some(cx) = col_price_x {
                let price_str = format!("{:.prec$}", event.price, prec = decimals);
                ctx.set_fill_color(&theme.text_primary);
                ctx.fill_text(&price_str, cx as f64, row_mid_y);
            }

            if let Some(cx) = col_qty_x {
                let qty_str = abbreviate_qty(event.quantity);
                ctx.set_fill_color(&theme.text_primary);
                ctx.fill_text(&qty_str, cx as f64, row_mid_y);
            }

            // S2: frame separator — thin line on top edge when this row starts a new WS frame.
            if self.frame_separator {
                if let Some(p_ts) = prev_ts {
                    if event.timestamp != p_ts {
                        ctx.set_fill_color(&mlc_core::apply_opacity(&theme.separator, 0.6));
                        ctx.fill_rect(x as f64, row_y as f64, w as f64, 1.0);
                    }
                }
                prev_ts = Some(event.timestamp);
            }
        }

        if events.is_empty() {
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_fill_color(&theme.text_header);
            ctx.fill_text(
                "No events",
                (x + w / 2.0) as f64,
                (y + h / 2.0) as f64,
            );
        }

        // ── Add/Cancel imbalance footer (owner order 2026-08-01): is the
        // book filling or draining over the modal-config window.
        if self.show_imbalance {
            let win_cutoff = now_ms as i64 - self.imb_window_min.max(1) as i64 * 60_000;
            let (add_vol, mod_vol, cxl_vol) = self.imb_lifecycle(None, win_cutoff);
            self.draw_imbalance_footer(
                ctx,
                x,
                y + h - L2_FOOTER_HEIGHT,
                w,
                theme,
                add_vol,
                mod_vol,
                cxl_vol,
                true,
            );
        }

    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool { false }

    fn handle_scroll(&mut self, local_id: &str, _dx: f64, dy: f64) -> bool {
        if local_id == "l2tape:body" {
            L2TapeState::handle_scroll(self, dy * 3.0);
            true
        } else {
            false
        }
    }

    fn handle_drag_start(&mut self, local_id: &str, _x: f64, _y: f64) -> bool {
        local_id == "l2tape:body"
    }

    fn handle_drag_move(&mut self, local_id: &str, dx: f64, dy: f64) -> bool {
        if local_id == "l2tape:body" {
            L2TapeState::handle_drag(self, dx, dy);
            true
        } else {
            false
        }
    }

    fn handle_drag_end(&mut self, _local_id: &str) -> bool {
        true
    }

    fn handle_double_click(&mut self, local_id: &str, _x: f64, _y: f64) -> bool {
        if local_id == "l2tape:body" {
            L2TapeState::handle_double_click(self);
            true
        } else {
            false
        }
    }
}

impl L2TapeState {
    /// Order-lifecycle volumes over the imbalance window for one venue
    /// group (`None` = the single feed): `(added, modified, cancelled)`.
    /// This footer is NOT the Trade Tape's buy/sell imbalance — L2 is an
    /// order-event stream, so its footer reads book lifecycle (owner
    /// order 2026-08-01 «нужно что-то ещё что показывает созданные
    /// ордера, изменённые, отменённые»).
    fn imb_lifecycle(&self, venue: Option<u8>, win_cutoff: i64) -> (f64, f64, f64) {
        let (mut add, mut mdf, mut cxl) = (0.0_f64, 0.0_f64, 0.0_f64);
        for e in self.events.iter().rev() {
            if e.timestamp < win_cutoff {
                break;
            }
            if e.venue != venue {
                continue;
            }
            match e.event_type {
                L2EventType::Add => add += e.quantity,
                L2EventType::Modify => mdf += e.quantity,
                L2EventType::Cancel => cxl += e.quantity,
                _ => {}
            }
        }
        (add, mdf, cxl)
    }

    /// One lifecycle strip segment: FILL share bar (adds vs cancels — is
    /// the book filling or draining) + `A/M/C` volume readout when the
    /// segment is wide enough.
    #[allow(clippy::too_many_arguments)]
    fn draw_imbalance_footer(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32,
        y: f32,
        w: f32,
        theme: &crate::panel_theme::PanelTheme,
        add_vol: f64,
        mod_vol: f64,
        cxl_vol: f64,
        with_label: bool,
    ) {
        ctx.set_fill_color(&theme.separator);
        ctx.fill_rect(x as f64, y as f64, w as f64, 1.0);
        let total = add_vol + cxl_vol;
        let share = if total > 0.0 { add_vol / total } else { 0.5 };
        let label_w = if with_label { 24.0_f32 } else { 0.0 };
        let bar_x = x + label_w + 2.0;
        let bar_w = (w - label_w - 6.0).max(0.0);
        if with_label {
            ctx.set_fill_color(&theme.text_muted);
            ctx.set_font("9px monospace");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(
                &format!("{}m", self.imb_window_min.max(1)),
                (x + 3.0) as f64,
                (y + L2_FOOTER_HEIGHT * 0.5) as f64,
            );
        }
        let gw = bar_w as f64 * share.clamp(0.0, 1.0);
        ctx.set_fill_color(&mlc_core::apply_opacity(&theme.buy, 0.55));
        ctx.fill_rect(bar_x as f64, (y + 2.0) as f64, gw, (L2_FOOTER_HEIGHT - 4.0) as f64);
        ctx.set_fill_color(&mlc_core::apply_opacity(&theme.sell, 0.55));
        ctx.fill_rect(
            bar_x as f64 + gw,
            (y + 2.0) as f64,
            bar_w as f64 - gw,
            (L2_FOOTER_HEIGHT - 4.0) as f64,
        );
        ctx.set_fill_color(&theme.text_primary);
        ctx.set_font("9px monospace");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        let text = if bar_w >= 190.0 {
            format!(
                "A {}  M {}  C {}",
                abbreviate_qty(add_vol),
                abbreviate_qty(mod_vol),
                abbreviate_qty(cxl_vol),
            )
        } else {
            format!("A{:.0}%", share * 100.0)
        };
        ctx.fill_text(
            &text,
            (bar_x + bar_w * 0.5) as f64,
            (y + L2_FOOTER_HEIGHT * 0.5) as f64,
        );
    }

    /// M/L2 columnar render (по образцу Multi-DOM, owner order
    /// 2026-07-31): one venue per column, one unifying tagged header.
    /// Each column streams ITS venue's book events newest-at-top — the
    /// chromatic type/side background carries the event class, text =
    /// type tag (wide columns) + price + qty.
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
        // TIME first when it fits (owner defect 2026-08-01 «в л2 почему-то
        // не пишут время» — the multi columns dropped the single panel's
        // TIME column entirely); TYPE next when wider still.
        let show_time = self.column_config.show_time && col_w >= 110.0;
        let show_type = self.column_config.show_type
            && col_w >= if show_time { 160.0 } else { 120.0 };

        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, L2_HEADER_HEIGHT as f64);
        ctx.set_font(if narrow { "8px monospace" } else { "10px monospace" });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        for (i, e) in self.multi.entries.iter().enumerate() {
            let live = self
                .multi
                .book_handles
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
            ctx.fill_text(&label, cx as f64, (y + L2_HEADER_HEIGHT * 0.5) as f64);
        }

        let footer_h = if self.show_imbalance { L2_FOOTER_HEIGHT } else { 0.0 };
        let content_h = h - L2_HEADER_HEIGHT - footer_h;
        let max_rows = (content_h / L2_ROW_HEIGHT).floor() as usize;
        if max_rows == 0 {
            return;
        }
        let skip = if self.auto_scroll { 0 } else { self.scroll_offset as usize };
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let flash_active: std::collections::HashSet<usize> = self.flash_events
            .iter()
            .filter(|&&(_, start)| now_ms.saturating_sub(start) < 200)
            .map(|&(serial, _)| serial)
            .collect();
        let decimals = if self.tick_size >= 1.0 {
            0usize
        } else if self.tick_size >= 0.1 {
            1
        } else if self.tick_size >= 0.01 {
            2
        } else if self.tick_size >= 0.001 {
            3
        } else {
            4
        };
        ctx.set_font(if narrow { "9px monospace" } else { "10px monospace" });
        for i in 0..n {
            let cx0 = x + col_w * i as f32;
            // Shared scroll offset, clamped per column to that venue's own
            // history depth (ring-scroll, owner order 2026-08-01).
            let cnt = self
                .events
                .iter()
                .filter(|e| e.venue == Some(i as u8) && self.event_visible(&e.event_type))
                .count();
            let skip_i = skip.min(cnt.saturating_sub(max_rows.min(cnt)));
            let rows = self
                .events
                .iter()
                .enumerate()
                .rev()
                .filter(|(_, e)| {
                    e.venue == Some(i as u8) && self.event_visible(&e.event_type)
                })
                .skip(skip_i)
                .take(max_rows);
            for (row_idx, (deque_idx, event)) in rows.enumerate() {
                let row_y = y + L2_HEADER_HEIGHT + row_idx as f32 * L2_ROW_HEIGHT;
                let row_mid_y = (row_y + L2_ROW_HEIGHT / 2.0) as f64;
                // Zebra base + chromatic type/side overlay + flash — the
                // single panel's visual language per column.
                let row_bg = if row_idx % 2 == 0 { &theme.panel_bg } else { &theme.row_bg_alt };
                ctx.set_fill_color(row_bg);
                ctx.fill_rect(cx0 as f64, row_y as f64, col_w as f64, L2_ROW_HEIGHT as f64);
                let chroma = self.row_chroma_color(event, theme);
                ctx.set_fill_color(&chroma);
                ctx.fill_rect(cx0 as f64, row_y as f64, col_w as f64, L2_ROW_HEIGHT as f64);
                if flash_active.contains(&self.event_serial(deque_idx)) {
                    ctx.set_fill_color(&mlc_core::apply_opacity(&theme.text_primary, 0.16));
                    ctx.fill_rect(cx0 as f64, row_y as f64, col_w as f64, L2_ROW_HEIGHT as f64);
                }
                if event.spoof {
                    ctx.set_fill_color(&mlc_core::apply_opacity(&theme.current_price, 0.9));
                    ctx.fill_rect(cx0 as f64, row_y as f64, 3.0, L2_ROW_HEIGHT as f64);
                } else if event.event_type == L2EventType::Add
                    && self.max_qty_seen > 0.0
                    && event.quantity >= self.max_qty_seen * Self::SPOOF_BIG_FRAC
                {
                    ctx.set_fill_color(&mlc_core::apply_opacity(
                        match event.side {
                            L2Side::Bid => &theme.buy_bright,
                            L2Side::Ask => &theme.sell_bright,
                        },
                        0.9,
                    ));
                    ctx.fill_rect(cx0 as f64, row_y as f64, 3.0, L2_ROW_HEIGHT as f64);
                }
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.set_text_align(TextAlign::Left);
                let mut tx = cx0 + 3.0;
                if show_time {
                    let total_secs = (event.timestamp / 1000) % 86400;
                    let time_str = format!(
                        "{:02}:{:02}:{:02}",
                        total_secs / 3600,
                        (total_secs % 3600) / 60,
                        total_secs % 60
                    );
                    ctx.set_fill_color(&theme.text_muted);
                    ctx.fill_text(&time_str, tx as f64, row_mid_y);
                    tx += 50.0;
                }
                if show_type {
                    let type_color = self.event_color(event, theme);
                    ctx.set_fill_color(&type_color);
                    ctx.fill_text(L2TapeState::event_label(&event.event_type), tx as f64, row_mid_y);
                    tx += 30.0;
                }
                ctx.set_fill_color(match event.side {
                    L2Side::Bid => &theme.buy,
                    L2Side::Ask => &theme.sell,
                });
                ctx.fill_text(
                    &format!("{:.prec$}", event.price, prec = decimals),
                    tx as f64,
                    row_mid_y,
                );
                ctx.set_text_align(TextAlign::Right);
                ctx.set_fill_color(&theme.text_primary);
                ctx.fill_text(
                    &abbreviate_qty(event.quantity),
                    (cx0 + col_w - 3.0) as f64,
                    row_mid_y,
                );
            }
        }

        // ── Per-venue Add/Cancel imbalance footer.
        if self.show_imbalance {
            let win_cutoff = now_ms as i64 - self.imb_window_min.max(1) as i64 * 60_000;
            let fy = y + h - L2_FOOTER_HEIGHT;
            for i in 0..n {
                let (add_vol, mod_vol, cxl_vol) = self.imb_lifecycle(Some(i as u8), win_cutoff);
                self.draw_imbalance_footer(
                    ctx,
                    x + col_w * i as f32,
                    fy,
                    col_w,
                    theme,
                    add_vol,
                    mod_vol,
                    cxl_vol,
                    i == 0,
                );
            }
        }

        ctx.set_fill_color(&theme.separator);
        for i in 1..n {
            let sx = (x + col_w * i as f32) as f64;
            ctx.fill_rect(sx, y as f64, 1.0, h as f64);
        }

    }

    pub fn new() -> Self {
        Self {
            symbol: String::new(),
            frame_separator: true,
            exchange: String::new(),
            account_type: String::new(),
            events: VecDeque::new(),
            max_events: 10000,
            retention_ms: 120_000,
            max_qty_seen: 0.0,
            auto_scroll: true,
            filter_type: None,
            filter_side: None,
            min_quantity: None,
            dom_market_price: None,
            dom_tick_size: None,
            scroll_offset: 0.0,
            flash_events: vec![],
            push_total: 0,
            spoof_alerts: VecDeque::new(),
            previous_book: HashMap::new(),
            tick_size: 0.01,
            crosshair_price: None,
            shared_orderbook: None,
            last_seen_update_id: None,
            column_config: L2TapeColumnConfig::default(),
            multi_mode: false,
            multi: super::multi_tape::MultiTapeState::new(),
            show_imbalance: true,
            imb_window_min: 2,
            last_big_adds: HashMap::new(),
            show_adds: true,
            show_mods: true,
            show_cxls: true,
            show_execs: true,
        }
    }

    /// Row visibility per event type (modal "Event types" chips).
    pub fn event_visible(&self, et: &L2EventType) -> bool {
        match et {
            L2EventType::Add => self.show_adds,
            L2EventType::Modify => self.show_mods,
            L2EventType::Cancel => self.show_cxls,
            L2EventType::Execute => self.show_execs,
        }
    }

    /// Toggle multi-venue mode (M/L2 — same contract as Trade Tape's
    /// `set_multi_mode`): roster base symbol re-synced, panel's own venue
    /// seeds the roster on first use, event buffer cleared on switch.
    /// Subscriptions are the caller's job.
    pub fn set_multi_mode(&mut self, on: bool) {
        if self.multi_mode == on {
            return;
        }
        self.multi_mode = on;
        self.events.clear();
        self.flash_events.clear();
        self.scroll_offset = 0.0;
        self.auto_scroll = true;
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
            // Fresh diff baseline for the single book + single-feed ring cap.
            self.previous_book.clear();
            self.last_seen_update_id = None;
            self.max_events = 10_000;
        }
    }

    /// Pull BBO metadata from `shared_orderbook` and diff against `previous_book` if
    /// the book has advanced. L2-tape receives raw delta events via `apply_ob_update_with_ts`
    /// from the broadcast drain (not from the tracker) because it needs per-event timestamps.
    /// This tick() is used only for the periodic snapshot-based diff path.
    pub fn tick(&mut self) {
        if self.multi_mode {
            self.tick_multi_diff();
        } else {
            self.tick_single_diff();
        }
        self.prune_events_retention();
    }

    /// Time-based retention WITHOUT starving a quiet venue (owner defect
    /// 2026-07-31): eviction is front-only (the flash-serial mapping
    /// depends on it), and stops at the first aged event whose venue
    /// group would drop below its keep-minimum — a thin venue's visible
    /// column must not empty out by age alone.
    fn prune_events_retention(&mut self) {
        /// Age-prune floor per venue group.
        const L2_MIN_KEEP_PER_GROUP: usize = 150;
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let cutoff_ms = now_ms.saturating_sub(self.retention_ms) as i64;
        if self.events.front().map_or(true, |f| f.timestamp >= cutoff_ms) {
            return;
        }
        let mut counts: std::collections::HashMap<Option<u8>, usize> =
            std::collections::HashMap::new();
        for e in &self.events {
            *counts.entry(e.venue).or_insert(0) += 1;
        }
        let before_len = self.events.len();
        while let Some(front) = self.events.front() {
            if front.timestamp >= cutoff_ms {
                break;
            }
            let Some(c) = counts.get_mut(&front.venue) else { break };
            if *c <= L2_MIN_KEEP_PER_GROUP {
                break;
            }
            *c -= 1;
            self.events.pop_front();
        }
        // Only recalculate when events were removed (the old max may be gone).
        if self.events.len() < before_len {
            self.recalc_max_qty();
        }
    }

    /// Single-venue snapshot diff (the original `tick` body).
    fn tick_single_diff(&mut self) {
        let Some(ref ob_handle) = self.shared_orderbook else { return };

        let guard = ob_handle.blocking_read();
        let current_uid = guard.last_update_id();
        if current_uid == self.last_seen_update_id {
            return; // nothing new
        }
        self.last_seen_update_id = current_uid;

        // Build a snapshot from the tracker for the diff path.
        let bids: Vec<(f64, f64)> = guard.top_bids(1000)
            .iter()
            .map(|(p, q)| (p.to_f64().unwrap_or(0.0), q.to_f64().unwrap_or(0.0)))
            .collect();
        let asks: Vec<(f64, f64)> = guard.top_asks(1000)
            .iter()
            .map(|(p, q)| (p.to_f64().unwrap_or(0.0), q.to_f64().unwrap_or(0.0)))
            .collect();
        drop(guard);

        // Use wall-clock time for event timestamps (not REST ts which may be stale).
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        // Update mid-price tracker from bids/asks slices.
        let best_bid = bids.last().map(|(p, _)| *p).unwrap_or(0.0);
        let best_ask = asks.first().map(|(p, _)| *p).unwrap_or(0.0);
        if best_bid > 0.0 && best_ask > 0.0 {
            self.dom_market_price = Some((best_bid + best_ask) / 2.0);
        }

        // Diff current snapshot against previous_book and generate L2Events.
        self.diff_and_push_events(&bids, &asks, timestamp);
    }

    /// Multi-venue snapshot diff (M/L2): the same diff/classify pass per
    /// roster venue against that venue's OWN previous book; events carry
    /// the venue index.
    fn tick_multi_diff(&mut self) {
        self.multi.sync_len();
        // Ring capacity scales with the roster — one venue's burst must
        // not flush another venue's events out of the shared ring
        // (owner defect 2026-08-01: BINANCE column starved).
        self.max_events = 10_000 * self.multi.entries.len().max(1);
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        for i in 0..self.multi.entries.len() {
            // The raw delta drain is the primary feed (owner defect
            // 2026-08-01 «лента стала будто медленнее» — the old 250ms
            // snapshot-diff throttle was batching the stream). While a
            // venue's delta stream is live, the snapshot diff would only
            // double-emit — skip it. It remains as the RESYNC fallback
            // for venues whose deltas never reach the drain.
            if now_ms - self.multi.delta_fed_ms[i] < 1500 {
                continue;
            }
            let Some(handle) = self.multi.book_handles[i].clone() else { continue };
            let guard = handle.blocking_read();
            let current_uid = guard.last_update_id();
            if current_uid == self.multi.book_uids[i] {
                continue; // this venue's book has not advanced
            }
            let first_seed = self.multi.book_uids[i].is_none();
            self.multi.book_uids[i] = current_uid;
            self.multi.book_last_diff_ms[i] = now_ms;
            let bids: Vec<(f64, f64)> = guard.top_bids(1000)
                .iter()
                .map(|(p, q)| (p.to_f64().unwrap_or(0.0), q.to_f64().unwrap_or(0.0)))
                .collect();
            let asks: Vec<(f64, f64)> = guard.top_asks(1000)
                .iter()
                .map(|(p, q)| (p.to_f64().unwrap_or(0.0), q.to_f64().unwrap_or(0.0)))
                .collect();
            drop(guard);

            // First contact with this venue's book: seed the baseline
            // SILENTLY (no events) — diffing a full snapshot against an
            // empty book would fabricate an Add for every level.
            if first_seed && self.multi.prev_books[i].is_empty() {
                let book = &mut self.multi.prev_books[i];
                for &(price, qty) in &bids {
                    if qty > 0.0 {
                        let tick = (price / self.tick_size).round() as i64;
                        book.entry(tick).or_insert((0.0, 0.0)).0 = qty;
                    }
                }
                for &(price, qty) in &asks {
                    if qty > 0.0 {
                        let tick = (price / self.tick_size).round() as i64;
                        book.entry(tick).or_insert((0.0, 0.0)).1 = qty;
                    }
                }
                continue;
            }

            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;

            // The venue's diff state is swapped out for the pass — the
            // classify body needs `&mut self` for `push_event`.
            let mut book = std::mem::take(&mut self.multi.prev_books[i]);
            self.diff_into(&mut book, &bids, &asks, timestamp, Some(i as u8));
            self.multi.prev_books[i] = book;
        }
    }

    /// Apply one venue's WS delta event (M/L2 primary feed, 2026-08-01):
    /// per-event classification against that venue's OWN book with the
    /// original wire timestamp — the same firehose grammar the single
    /// panel gets from the drain, so the multi columns stream at event
    /// rate instead of batched snapshot diffs.
    pub fn apply_ob_delta_venue(
        &mut self,
        venue: usize,
        delta_bids: &[(f64, f64)],
        delta_asks: &[(f64, f64)],
        ts_ms: i64,
    ) {
        if !self.multi_mode {
            return;
        }
        self.multi.sync_len();
        if venue >= self.multi.entries.len() {
            return;
        }
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        self.multi.delta_fed_ms[venue] = now_ms;

        // First delta before any baseline: seed the venue book silently
        // from its tracker so the delta classifies against real state.
        if self.multi.prev_books[venue].is_empty() {
            if let Some(handle) = self.multi.book_handles[venue].clone() {
                let guard = handle.blocking_read();
                let uid = guard.last_update_id();
                let bids: Vec<(f64, f64)> = guard.top_bids(1000)
                    .iter()
                    .map(|(p, q)| (p.to_f64().unwrap_or(0.0), q.to_f64().unwrap_or(0.0)))
                    .collect();
                let asks: Vec<(f64, f64)> = guard.top_asks(1000)
                    .iter()
                    .map(|(p, q)| (p.to_f64().unwrap_or(0.0), q.to_f64().unwrap_or(0.0)))
                    .collect();
                drop(guard);
                self.multi.book_uids[venue] = uid;
                let book = &mut self.multi.prev_books[venue];
                for &(price, qty) in &bids {
                    if qty > 0.0 {
                        let tick = (price / self.tick_size).round() as i64;
                        book.entry(tick).or_insert((0.0, 0.0)).0 = qty;
                    }
                }
                for &(price, qty) in &asks {
                    if qty > 0.0 {
                        let tick = (price / self.tick_size).round() as i64;
                        book.entry(tick).or_insert((0.0, 0.0)).1 = qty;
                    }
                }
                return;
            }
        }

        if delta_bids.is_empty() && delta_asks.is_empty() {
            return;
        }
        // Some venues' delta frames carry no wire timestamp (ts 0) —
        // stamp arrival time instead of rendering 00:00:00 rows that
        // age-prune instantly (stand defect 2026-08-01).
        let ts = if ts_ms > 0 { ts_ms } else { now_ms };
        let mut book = std::mem::take(&mut self.multi.prev_books[venue]);
        self.classify_deltas_into(&mut book, delta_bids, delta_asks, ts, Some(venue as u8));
        self.multi.prev_books[venue] = book;
        self.prune_events_retention();
    }

    /// Get visible events as `(deque_index, &L2Event)`, most recent first.
    ///
    /// Applies filters and `scroll_offset`.  When `auto_scroll` is true the
    /// most recent `max_count` events are shown; when false the viewport is
    /// shifted by `scroll_offset` rows toward older events.
    pub fn visible_events(&self, max_count: usize) -> Vec<(usize, &L2Event)> {
        let skip = if self.auto_scroll { 0 } else { self.scroll_offset as usize };
        self.events
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, e)| {
                if !self.event_visible(&e.event_type) {
                    return false;
                }
                if let Some(ref ft) = self.filter_type {
                    if &e.event_type != ft {
                        return false;
                    }
                }
                if let Some(ref fs) = self.filter_side {
                    if &e.side != fs {
                        return false;
                    }
                }
                if let Some(min_q) = self.min_quantity {
                    if e.quantity < min_q {
                        return false;
                    }
                }
                true
            })
            .skip(skip)
            .take(max_count)
            .collect()
    }

    /// Return the monotone serial of the event at deque position `i`.
    ///
    /// Serial 0 is unused (push_total starts at 0 before any push).
    /// After N total pushes, `events[i]` has serial = `push_total - (len-1-i)`.
    fn event_serial(&self, deque_index: usize) -> usize {
        let len = self.events.len();
        // deque_index counts from 0 (oldest) to len-1 (newest).
        // newest has serial = push_total; each step back subtracts 1.
        self.push_total.saturating_sub(len.saturating_sub(deque_index + 1))
    }

    /// Color for the event-type label — theme tokens (was a hardcoded
    /// rgba table; skin doctrine 2026-07-31).
    pub fn event_color(&self, event: &L2Event, theme: &crate::panel_theme::PanelTheme) -> String {
        match event.event_type {
            L2EventType::Add => match event.side {
                L2Side::Bid => mlc_core::apply_opacity(&theme.buy, 0.8),
                L2Side::Ask => mlc_core::apply_opacity(&theme.sell, 0.8),
            },
            L2EventType::Modify => mlc_core::apply_opacity(&theme.accent, 0.8),
            L2EventType::Cancel => mlc_core::apply_opacity(&theme.text_muted, 0.6),
            L2EventType::Execute => mlc_core::apply_opacity(&theme.current_price, 0.9),
        }
    }

    /// Short label for event type
    pub fn event_label(event_type: &L2EventType) -> &'static str {
        match event_type {
            L2EventType::Add => "ADD",
            L2EventType::Modify => "MOD",
            L2EventType::Cancel => "CXL",
            L2EventType::Execute => "EXE",
        }
    }

    /// Short label for side
    pub fn side_label(side: &L2Side) -> &'static str {
        match side {
            L2Side::Bid => "BID",
            L2Side::Ask => "ASK",
        }
    }

    /// Add an event to the buffer, recording a flash animation entry.
    ///
    /// Central spoof detector (owner order 2026-08-01; both the
    /// snapshot-diff and the delta path land here): a LARGE Add
    /// (≥ [`Self::SPOOF_BIG_FRAC`] of the window max) is remembered by
    /// `(venue, tick)`; a Cancel of that level within
    /// [`Self::SPOOF_WINDOW_MS`] marks the event + records a
    /// [`SpoofAlert`].
    pub fn push_event(&mut self, mut event: L2Event) {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let tick = self.price_to_tick(event.price);
        let key = (event.venue, tick);
        match event.event_type {
            L2EventType::Add => {
                if self.max_qty_seen > 0.0
                    && event.quantity >= self.max_qty_seen * Self::SPOOF_BIG_FRAC
                {
                    self.last_big_adds.insert(key, (event.quantity, now_ms as i64));
                }
            }
            L2EventType::Cancel => {
                if let Some((add_qty, add_ts)) = self.last_big_adds.remove(&key) {
                    let dt = (now_ms as i64 - add_ts).max(0) as u64;
                    if dt <= Self::SPOOF_WINDOW_MS {
                        event.spoof = true;
                        if self.spoof_alerts.len() >= 64 {
                            self.spoof_alerts.pop_front();
                        }
                        self.spoof_alerts.push_back(SpoofAlert {
                            timestamp: event.timestamp,
                            price: event.price,
                            side: event.side.clone(),
                            quantity: add_qty,
                            duration_ms: dt,
                        });
                    }
                }
            }
            _ => {}
        }

        if self.events.len() >= self.max_events {
            self.events.pop_front();
        }
        // Update max_qty_seen before pushing so the new event is included.
        if event.quantity > self.max_qty_seen {
            self.max_qty_seen = event.quantity;
        }
        self.events.push_back(event);
        self.push_total += 1;
        // Scrolled-back view stays anchored on the rows under the cursor
        // while the feed keeps flowing — NO pause semantics on scroll
        // (owner order 2026-08-01 «нахуй паузд»; ring scroll).
        if !self.auto_scroll {
            self.scroll_offset += 1.0;
        }

        // Serial of the event just pushed equals push_total.
        self.flash_events.push((self.push_total, now_ms));
    }

    /// An Add at or above this fraction of the window max is "large"
    /// (same threshold family as the DOM's `BIG_ORDER_FRAC`).
    const SPOOF_BIG_FRAC: f64 = 0.30;
    /// A large Add cancelled within this window is a spoof print.
    const SPOOF_WINDOW_MS: u64 = 5_000;

    /// Drop spoof-memory entries older than twice the spoof window —
    /// called from the tick loop alongside flash pruning.
    pub fn prune_spoof_memory(&mut self) {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let cutoff = now_ms - (Self::SPOOF_WINDOW_MS as i64) * 2;
        self.last_big_adds.retain(|_, (_, ts)| *ts >= cutoff);
    }

    /// Recompute `max_qty_seen` from the current event buffer.
    ///
    /// Called after time-based pruning removes events from the front,
    /// which might have held the previous maximum.
    fn recalc_max_qty(&mut self) {
        self.max_qty_seen = self.events
            .iter()
            .map(|e| e.quantity)
            .fold(0.0_f64, f64::max);
    }

    /// Return the RGBA hex color string for the chromatic row background.
    ///
    /// Alpha scales with `(qty / max_qty_seen).sqrt().clamp(0.3, 1.0)` so
    /// larger events get more opaque overlays.
    fn row_chroma_color(&self, event: &L2Event, theme: &crate::panel_theme::PanelTheme) -> String {
        let qty_ratio = if self.max_qty_seen > 0.0 {
            (event.quantity / self.max_qty_seen).sqrt().clamp(0.3, 1.0)
        } else {
            0.3
        };

        // Theme tokens (was a hardcoded rgba table; skin doctrine
        // 2026-07-31): executes use the bright side endpoints, adds the
        // base side colors, modify/cancel the accent/muted neutrals.
        let (color, base_alpha) = match event.event_type {
            L2EventType::Execute => match event.side {
                L2Side::Bid => (theme.buy_bright.clone(), 0.30),
                L2Side::Ask => (theme.sell_bright.clone(), 0.30),
            },
            L2EventType::Add => match event.side {
                L2Side::Bid => (theme.buy.clone(), 0.15),
                L2Side::Ask => (theme.sell.clone(), 0.15),
            },
            L2EventType::Modify => (theme.accent.clone(), 0.10),
            L2EventType::Cancel => (theme.text_muted.clone(), 0.10),
        };

        mlc_core::apply_opacity(&color, (base_alpha * qty_ratio).clamp(0.0, 1.0))
    }

    /// Scroll by `delta` rows (positive = scroll toward older events).
    ///
    /// Switches off `auto_scroll` when scrolling up; re-enables it when the
    /// user scrolls back to offset 0.
    pub fn handle_scroll(&mut self, delta: f64) {
        let filtered_len = self.events
            .iter()
            .filter(|e| {
                if let Some(ref ft) = self.filter_type {
                    if &e.event_type != ft { return false; }
                }
                if let Some(ref fs) = self.filter_side {
                    if &e.side != fs { return false; }
                }
                if let Some(min_q) = self.min_quantity {
                    if e.quantity < min_q { return false; }
                }
                true
            })
            .count();

        // delta > 0: scroll wheel up → show older events → increase offset.
        let new_offset = (self.scroll_offset + delta).max(0.0);
        // Cannot scroll past oldest visible event.
        let max_offset = filtered_len.saturating_sub(1) as f64;
        self.scroll_offset = new_offset.min(max_offset);
        self.auto_scroll = self.scroll_offset <= 0.0;
    }

    /// Reset scroll to the latest event and re-enable auto-scroll.
    pub fn handle_double_click(&mut self) {
        self.scroll_offset = 0.0;
        self.auto_scroll = true;
    }

    /// Continuous drag scroll: drag up (dy < 0) shows older events, drag down
    /// (dy > 0) scrolls back toward newest.  Sensitivity: 1 row per 16 px.
    pub fn handle_drag(&mut self, _dx: f64, dy: f64) {
        // Invert: dragging upward (negative dy) = scroll toward older events.
        self.handle_scroll(-dy / 16.0);
    }

    /// Handle a named key event.  Returns `true` if the key was consumed.
    pub fn handle_key(&mut self, _key: mlc_core::input::KeyCode) -> bool {
        false
    }

    /// Prune expired flash entries and, if `auto_scroll`, reset scroll offset.
    ///
    /// Call once per frame / tick after `tick()`.
    pub fn prune_flash(&mut self) {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.flash_events.retain(|&(_, start)| {
            now_ms.saturating_sub(start) < 200
        });

        if self.auto_scroll {
            self.scroll_offset = 0.0;
        }
    }

    /// Set tick size (called when panel is linked to a symbol with known tick size)
    pub fn set_tick_size(&mut self, tick_size: f64) {
        self.tick_size = if tick_size > 0.0 { tick_size } else { 0.01 };
    }

    fn price_to_tick(&self, price: f64) -> i64 {
        (price / self.tick_size).round() as i64
    }

    /// Diff the given bids/asks against `previous_book` and push L2Events for changes.
    ///
    /// Also updates `previous_book` to reflect the new state.
    /// Called by `tick()` after pulling a fresh snapshot from the shared series.
    fn diff_and_push_events(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], timestamp: i64) {
        // The book is swapped out for the pass — the classify body needs
        // `&mut self` for `push_event` (same dance `tick_multi_diff` does
        // with a venue book).
        let mut book = std::mem::take(&mut self.previous_book);
        self.diff_into(&mut book, bids, asks, timestamp, None);
        self.previous_book = book;
    }

    /// Snapshot diff/classify against an EXTERNAL previous-book map — the
    /// single book or one multi-roster venue's own (M/L2). Pushed events
    /// carry `venue`.
    fn diff_into(
        &mut self,
        book: &mut HashMap<i64, (f64, f64)>,
        bids: &[(f64, f64)],
        asks: &[(f64, f64)],
        timestamp: i64,
        venue: Option<u8>,
    ) {
        for &(price, qty) in bids {
            let tick = self.price_to_tick(price);
            let prev_qty = book.get(&tick).map(|(b, _)| *b).unwrap_or(0.0);

            let event_type = if qty == 0.0 && prev_qty > 0.0 {
                L2EventType::Cancel
            } else if qty > 0.0 && prev_qty == 0.0 {
                L2EventType::Add
            } else if qty > 0.0 && prev_qty > 0.0 && (qty - prev_qty).abs() > f64::EPSILON {
                L2EventType::Modify
            } else {
                let entry = book.entry(tick).or_insert((0.0, 0.0));
                entry.0 = qty;
                continue;
            };

            if let Some(min_q) = self.min_quantity {
                let relevant_qty = if qty > 0.0 { qty } else { prev_qty };
                if relevant_qty < min_q {
                    let entry = book.entry(tick).or_insert((0.0, 0.0));
                    entry.0 = qty;
                    continue;
                }
            }

            self.push_event(L2Event {
                timestamp,
                event_type,
                side: L2Side::Bid,
                price,
                quantity: if qty > 0.0 { qty } else { prev_qty },
                order_count: None,
                order_id: None,
                venue,
                spoof: false,
            });

            let entry = book.entry(tick).or_insert((0.0, 0.0));
            entry.0 = qty;
        }

        for &(price, qty) in asks {
            let tick = self.price_to_tick(price);
            let prev_qty = book.get(&tick).map(|(_, a)| *a).unwrap_or(0.0);

            let event_type = if qty == 0.0 && prev_qty > 0.0 {
                L2EventType::Cancel
            } else if qty > 0.0 && prev_qty == 0.0 {
                L2EventType::Add
            } else if qty > 0.0 && prev_qty > 0.0 && (qty - prev_qty).abs() > f64::EPSILON {
                L2EventType::Modify
            } else {
                let entry = book.entry(tick).or_insert((0.0, 0.0));
                entry.1 = qty;
                continue;
            };

            if let Some(min_q) = self.min_quantity {
                let relevant_qty = if qty > 0.0 { qty } else { prev_qty };
                if relevant_qty < min_q {
                    let entry = book.entry(tick).or_insert((0.0, 0.0));
                    entry.1 = qty;
                    continue;
                }
            }

            self.push_event(L2Event {
                timestamp,
                event_type,
                side: L2Side::Ask,
                price,
                quantity: if qty > 0.0 { qty } else { prev_qty },
                order_count: None,
                order_id: None,
                venue,
                spoof: false,
            });

            let entry = book.entry(tick).or_insert((0.0, 0.0));
            entry.1 = qty;
        }

        // Reverse pass (2026-08-01): previous-book levels that VANISHED
        // from the snapshot are CANCELS — the forward loops only see
        // levels present in the NEW snapshot, so removals never produced
        // an event here (the delta path gets explicit qty==0 rows
        // instead; this snapshot path did not). Range-guarded: a level
        // outside the snapshot's covered band merely scrolled out of the
        // top-N window — not a cancel.
        let bid_min = bids
            .iter()
            .filter(|&&(p, q)| p > 0.0 && q > 0.0)
            .map(|&(p, _)| self.price_to_tick(p))
            .min();
        let ask_max = asks
            .iter()
            .filter(|&&(p, q)| p > 0.0 && q > 0.0)
            .map(|&(p, _)| self.price_to_tick(p))
            .max();
        let bid_set: std::collections::HashSet<i64> = bids
            .iter()
            .filter(|&&(_, q)| q > 0.0)
            .map(|&(p, _)| self.price_to_tick(p))
            .collect();
        let ask_set: std::collections::HashSet<i64> = asks
            .iter()
            .filter(|&&(_, q)| q > 0.0)
            .map(|&(p, _)| self.price_to_tick(p))
            .collect();
        let mut vanished: Vec<(i64, f64, L2Side)> = Vec::new();
        for (&tick, &(bq, aq)) in book.iter() {
            if bq > 0.0 {
                if let Some(min) = bid_min {
                    if tick >= min && !bid_set.contains(&tick) {
                        vanished.push((tick, bq, L2Side::Bid));
                    }
                }
            }
            if aq > 0.0 {
                if let Some(max) = ask_max {
                    if tick <= max && !ask_set.contains(&tick) {
                        vanished.push((tick, aq, L2Side::Ask));
                    }
                }
            }
        }
        for (tick, qty, side) in vanished {
            let entry = book.entry(tick).or_insert((0.0, 0.0));
            match side {
                L2Side::Bid => entry.0 = 0.0,
                L2Side::Ask => entry.1 = 0.0,
            }
            if let Some(min_q) = self.min_quantity {
                if qty < min_q {
                    continue;
                }
            }
            self.push_event(L2Event {
                timestamp,
                event_type: L2EventType::Cancel,
                side,
                price: tick as f64 * self.tick_size,
                quantity: qty,
                order_count: None,
                order_id: None,
                venue,
                spoof: false,
            });
        }
    }

    /// Apply an orderbook update from the live-data frame tick.
    ///
    /// Called once per frame by the coalesced ob_dirty loop in tick.rs.
    /// Generates L2Events by diffing the delta against `previous_book` —
    /// O(delta) instead of O(full book).
    ///
    /// ## Full rebuild path (`force_rebuild` or empty `previous_book`)
    /// Seeds `previous_book` from the tracker without pushing events (panel just
    /// opened or reassigned). Also updates `dom_market_price`.
    ///
    /// ## Incremental path
    /// Classifies each (price, qty) pair as Add/Modify/Cancel and pushes L2Events.
    pub fn apply_ob_update(
        &mut self,
        delta_bids: &[(f64, f64)],
        delta_asks: &[(f64, f64)],
        force_rebuild: bool,
    ) {
        // Multi mode streams through `tick_multi_diff` — the single-venue
        // delta drain must not mix its events into the merged buffer.
        if self.multi_mode {
            return;
        }
        // Update BBO from tracker.
        if let Some(ref ob_handle) = self.shared_orderbook {
            let guard = ob_handle.blocking_read();
            if let Some((bid, ask)) = guard.bbo() {
                let best_bid = bid.to_f64().unwrap_or(0.0);
                let best_ask = ask.to_f64().unwrap_or(0.0);
                if best_bid > 0.0 && best_ask > 0.0 {
                    self.dom_market_price = Some((best_bid + best_ask) / 2.0);
                }
            }

            // Full rebuild: seed previous_book from tracker, no events.
            if force_rebuild || self.previous_book.is_empty() {
                self.previous_book.clear();
                let uid = guard.last_update_id();
                let bids: Vec<(f64, f64)> = guard.top_bids(1000)
                    .iter().map(|(p, q)| (p.to_f64().unwrap_or(0.0), q.to_f64().unwrap_or(0.0))).collect();
                let asks: Vec<(f64, f64)> = guard.top_asks(1000)
                    .iter().map(|(p, q)| (p.to_f64().unwrap_or(0.0), q.to_f64().unwrap_or(0.0))).collect();
                drop(guard);
                for &(price, qty) in &bids {
                    if qty > 0.0 {
                        let tick = self.price_to_tick(price);
                        let entry = self.previous_book.entry(tick).or_insert((0.0, 0.0));
                        entry.0 = qty;
                    }
                }
                for &(price, qty) in &asks {
                    if qty > 0.0 {
                        let tick = self.price_to_tick(price);
                        let entry = self.previous_book.entry(tick).or_insert((0.0, 0.0));
                        entry.1 = qty;
                    }
                }
                self.last_seen_update_id = uid;
                return;
            }
        }

        // Nothing to do when delta is empty.
        if delta_bids.is_empty() && delta_asks.is_empty() {
            return;
        }

        // Wall-clock fallback for callers that don't supply a wire ts.
        // Per-event wire ts is preferred — use `apply_ob_update_with_ts` from the drain.
        let timestamp = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        self.apply_delta_classified(delta_bids, delta_asks, timestamp);
    }

    /// Apply one WS delta event using its original wire timestamp `ts_ms`.
    ///
    /// Called from the tick.rs drain once per `ObDeltaEvent` so each L2-tape row
    /// carries the real per-tick wire time instead of a single `SystemTime::now()`
    /// for the whole per-frame batch.
    pub fn apply_ob_update_with_ts(
        &mut self,
        delta_bids: &[(f64, f64)],
        delta_asks: &[(f64, f64)],
        ts_ms: i64,
    ) {
        // Multi mode streams through `tick_multi_diff` — see apply_ob_update.
        if self.multi_mode {
            return;
        }
        // Update BBO from tracker.
        if let Some(ref ob_handle) = self.shared_orderbook {
            let guard = ob_handle.blocking_read();
            if let Some((bid, ask)) = guard.bbo() {
                let best_bid = bid.to_f64().unwrap_or(0.0);
                let best_ask = ask.to_f64().unwrap_or(0.0);
                if best_bid > 0.0 && best_ask > 0.0 {
                    self.dom_market_price = Some((best_bid + best_ask) / 2.0);
                }
            }

            // If previous_book is empty (first event after init or REST rebuild) seed
            // it from the tracker first, then apply the incremental delta on top.
            if self.previous_book.is_empty() {
                self.previous_book.clear();
                let bids: Vec<(f64, f64)> = guard.top_bids(1000)
                    .iter().map(|(p, q)| (p.to_f64().unwrap_or(0.0), q.to_f64().unwrap_or(0.0))).collect();
                let asks: Vec<(f64, f64)> = guard.top_asks(1000)
                    .iter().map(|(p, q)| (p.to_f64().unwrap_or(0.0), q.to_f64().unwrap_or(0.0))).collect();
                drop(guard);
                for &(price, qty) in &bids {
                    if qty > 0.0 {
                        let tick = self.price_to_tick(price);
                        let entry = self.previous_book.entry(tick).or_insert((0.0, 0.0));
                        entry.0 = qty;
                    }
                }
                for &(price, qty) in &asks {
                    if qty > 0.0 {
                        let tick = self.price_to_tick(price);
                        let entry = self.previous_book.entry(tick).or_insert((0.0, 0.0));
                        entry.1 = qty;
                    }
                }
            }
        }

        if delta_bids.is_empty() && delta_asks.is_empty() {
            return;
        }

        self.apply_delta_classified(delta_bids, delta_asks, ts_ms);
    }

    /// Core delta-classification loop shared by both public update methods.
    ///
    /// Classifies each (price, qty) pair as Add / Modify / Cancel, updates
    /// `previous_book`, pushes `L2Event`s with `timestamp`, and prunes stale
    /// events from the front of the deque.
    fn apply_delta_classified(
        &mut self,
        delta_bids: &[(f64, f64)],
        delta_asks: &[(f64, f64)],
        timestamp: i64,
    ) {
        // The book is swapped out for the pass — the classify body needs
        // `&mut self` for `push_event` (same dance the venue paths do).
        let mut book = std::mem::take(&mut self.previous_book);
        self.classify_deltas_into(&mut book, delta_bids, delta_asks, timestamp, None);
        self.previous_book = book;

        // Time-based retention (venue-keep-min aware).
        self.prune_events_retention();
    }

    /// Classify one delta batch against an EXTERNAL book map — the single
    /// `previous_book` or one multi-roster venue's own (M/L2 delta feed).
    /// Pushed events carry `venue`.
    fn classify_deltas_into(
        &mut self,
        book: &mut HashMap<i64, (f64, f64)>,
        delta_bids: &[(f64, f64)],
        delta_asks: &[(f64, f64)],
        timestamp: i64,
        venue: Option<u8>,
    ) {
        // Incremental: classify each delta level and push events.
        for &(price, qty) in delta_bids {
            let tick = self.price_to_tick(price);
            let prev_qty = book.get(&tick).map(|(b, _)| *b).unwrap_or(0.0);

            let event_type = if qty == 0.0 && prev_qty > 0.0 {
                L2EventType::Cancel
            } else if qty > 0.0 && prev_qty == 0.0 {
                L2EventType::Add
            } else if qty > 0.0 && (qty - prev_qty).abs() > f64::EPSILON {
                L2EventType::Modify
            } else {
                // Unchanged — update the book and skip.
                let entry = book.entry(tick).or_insert((0.0, 0.0));
                entry.0 = qty;
                continue;
            };

            if let Some(min_q) = self.min_quantity {
                let relevant = if qty > 0.0 { qty } else { prev_qty };
                if relevant < min_q {
                    let entry = book.entry(tick).or_insert((0.0, 0.0));
                    entry.0 = qty;
                    continue;
                }
            }

            self.push_event(L2Event {
                timestamp,
                event_type,
                side: L2Side::Bid,
                price,
                quantity: if qty > 0.0 { qty } else { prev_qty },
                order_count: None,
                order_id: None,
                venue,
                spoof: false,
            });

            let entry = book.entry(tick).or_insert((0.0, 0.0));
            entry.0 = qty;
        }

        for &(price, qty) in delta_asks {
            let tick = self.price_to_tick(price);
            let prev_qty = book.get(&tick).map(|(_, a)| *a).unwrap_or(0.0);

            let event_type = if qty == 0.0 && prev_qty > 0.0 {
                L2EventType::Cancel
            } else if qty > 0.0 && prev_qty == 0.0 {
                L2EventType::Add
            } else if qty > 0.0 && (qty - prev_qty).abs() > f64::EPSILON {
                L2EventType::Modify
            } else {
                let entry = book.entry(tick).or_insert((0.0, 0.0));
                entry.1 = qty;
                continue;
            };

            if let Some(min_q) = self.min_quantity {
                let relevant = if qty > 0.0 { qty } else { prev_qty };
                if relevant < min_q {
                    let entry = book.entry(tick).or_insert((0.0, 0.0));
                    entry.1 = qty;
                    continue;
                }
            }

            self.push_event(L2Event {
                timestamp,
                event_type,
                side: L2Side::Ask,
                price,
                quantity: if qty > 0.0 { qty } else { prev_qty },
                order_count: None,
                order_id: None,
                venue,
                spoof: false,
            });

            let entry = book.entry(tick).or_insert((0.0, 0.0));
            entry.1 = qty;
        }
    }

}
