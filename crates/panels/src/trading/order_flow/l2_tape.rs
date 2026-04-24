//! L2 Tape: Order Book Event Stream panel state.
//!
//! Shows individual MBO (Market-By-Order) events: order additions,
//! modifications, cancellations, and executions in real-time.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Serialize, Deserialize};

use orderbook_service::OrderbookSeries;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

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

    /// Shared orderbook series (written by the bridge, read here each tick).
    pub shared_orderbook: Option<Arc<RwLock<OrderbookSeries>>>,

    /// Last `OrderbookSnapshot::version` we consumed. When `series.current.version`
    /// differs we pull a fresh snapshot and generate L2 events from the diff.
    pub last_seen_orderbook_version: u64,

    /// Column visibility configuration.
    pub column_config: L2TapeColumnConfig,
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
    pub fn min_size(&self) -> (f32, f32) { (200.0, 150.0) }
}

const L2_ROW_HEIGHT: f32 = 16.0;
const L2_HEADER_HEIGHT: f32 = 16.0;
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

        let header_text_y = (y + L2_HEADER_HEIGHT / 2.0) as f64;
        if let Some(cx) = col_time_x  { ctx.fill_text("TIME",  cx as f64, header_text_y); }
        if let Some(cx) = col_type_x  { ctx.fill_text("TYPE",  cx as f64, header_text_y); }
        if let Some(cx) = col_side_x  { ctx.fill_text("SIDE",  cx as f64, header_text_y); }
        if let Some(cx) = col_price_x { ctx.fill_text("PRICE", cx as f64, header_text_y); }
        if let Some(cx) = col_qty_x   { ctx.fill_text("QTY",   cx as f64, header_text_y); }

        if !self.symbol.is_empty() {
            ctx.set_font("9px sans-serif");
            ctx.set_text_align(TextAlign::Right);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_fill_color(&theme.text_muted);
            ctx.fill_text(&self.symbol, (x + w - 6.0) as f64, header_text_y);
        }

        let content_h = h - L2_HEADER_HEIGHT;
        let max_rows = (content_h / L2_ROW_HEIGHT).floor() as usize;
        if max_rows == 0 {
            return;
        }

        let events = self.visible_events(max_rows);

        // Build flash-active serial set once per frame.
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

        for (row_idx, (deque_idx, event)) in events.iter().enumerate() {
            let row_y = y + L2_HEADER_HEIGHT + (row_idx as f32 * L2_ROW_HEIGHT);
            let row_mid_y = (row_y + L2_ROW_HEIGHT / 2.0) as f64;

            // Zebra base
            let row_bg = if row_idx % 2 == 0 { &theme.panel_bg } else { &theme.row_bg_alt };
            ctx.set_fill_color(row_bg);
            ctx.fill_rect(x as f64, row_y as f64, w as f64, L2_ROW_HEIGHT as f64);

            // Chromatic overlay: color by event type + side, alpha scales with qty.
            let chroma_color = self.row_chroma_color(event);
            ctx.set_fill_color(&chroma_color);
            ctx.fill_rect(x as f64, row_y as f64, w as f64, L2_ROW_HEIGHT as f64);

            // Flash overlay: bright semi-transparent rect for recently-pushed events.
            let serial = self.event_serial(*deque_idx);
            if flash_active.contains(&serial) {
                ctx.set_fill_color("#ffffff28");
                ctx.fill_rect(x as f64, row_y as f64, w as f64, L2_ROW_HEIGHT as f64);
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
                // event_color returns f32 rgba — convert to hex inline.
                let type_color = self.event_color(event);
                let type_hex = format!(
                    "#{:02x}{:02x}{:02x}{:02x}",
                    (type_color[0].clamp(0.0, 1.0) * 255.0) as u8,
                    (type_color[1].clamp(0.0, 1.0) * 255.0) as u8,
                    (type_color[2].clamp(0.0, 1.0) * 255.0) as u8,
                    (type_color[3].clamp(0.0, 1.0) * 255.0) as u8,
                );
                ctx.set_fill_color(&type_hex);
                ctx.fill_text(L2TapeState::event_label(&event.event_type), cx as f64, row_mid_y);
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

        // PAUSED badge — shown when user has scrolled up away from the tail.
        if !self.auto_scroll {
            let badge_w = 54.0_f64;
            let badge_h = 14.0_f64;
            let badge_x = (x + w / 2.0) as f64 - badge_w / 2.0;
            let badge_y = (y + L2_HEADER_HEIGHT + 2.0) as f64;
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
    pub fn new() -> Self {
        Self {
            symbol: String::new(),
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
            last_seen_orderbook_version: 0,
            column_config: L2TapeColumnConfig::default(),
        }
    }

    /// Pull the latest snapshot from `shared_orderbook`, diff against `previous_book`,
    /// and push any changed levels as L2Events.
    ///
    /// Returns immediately when there is no shared handle or the version has not
    /// advanced since the last call.
    pub fn tick(&mut self) {
        let Some(ref ob_handle) = self.shared_orderbook else { return };
        let Ok(series) = ob_handle.read() else { return };
        if series.current.version == self.last_seen_orderbook_version {
            return; // nothing new
        }
        self.last_seen_orderbook_version = series.current.version;

        // Use wall-clock time for event timestamps (not REST ts which may be stale).
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let bids: Vec<(f64, f64)> = series.current.bids.iter().map(|(k, &v)| (k.0, v)).collect();
        let asks: Vec<(f64, f64)> = series.current.asks.iter().map(|(k, &v)| (k.0, v)).collect();
        drop(series);

        // Update mid-price tracker.
        let best_bid = bids.first().map(|(p, _)| *p).unwrap_or(0.0);
        let best_ask = asks.first().map(|(p, _)| *p).unwrap_or(0.0);
        if best_bid > 0.0 && best_ask > 0.0 {
            self.dom_market_price = Some((best_bid + best_ask) / 2.0);
        }

        // Diff current snapshot against previous_book and generate L2Events.
        // This reuses the same logic as apply_delta but operates on the full
        // current book (treating missing levels as qty=0).
        self.diff_and_push_events(&bids, &asks, timestamp);

        // Time-based retention: drain events older than retention_ms.
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let cutoff_ms = now_ms.saturating_sub(self.retention_ms) as i64;
        let before_len = self.events.len();
        while let Some(front) = self.events.front() {
            if front.timestamp < cutoff_ms {
                self.events.pop_front();
            } else {
                break;
            }
        }
        // Only recalculate when events were removed (the old max may be gone).
        if self.events.len() < before_len {
            self.recalc_max_qty();
        }
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

    /// Color for event type
    pub fn event_color(&self, event: &L2Event) -> [f32; 4] {
        match event.event_type {
            L2EventType::Add => match event.side {
                L2Side::Bid => [0.055, 0.796, 0.506, 0.8],  // green for bid add
                L2Side::Ask => [0.965, 0.275, 0.365, 0.8],  // red for ask add
            },
            L2EventType::Modify => [0.529, 0.467, 0.878, 0.8],  // purple
            L2EventType::Cancel => [0.471, 0.482, 0.525, 0.6],  // grey
            L2EventType::Execute => [1.0, 0.843, 0.0, 0.9],     // gold
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
    pub fn push_event(&mut self, event: L2Event) {
        if self.events.len() >= self.max_events {
            self.events.pop_front();
        }
        // Update max_qty_seen before pushing so the new event is included.
        if event.quantity > self.max_qty_seen {
            self.max_qty_seen = event.quantity;
        }
        self.events.push_back(event);
        self.push_total += 1;

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        // Serial of the event just pushed equals push_total.
        self.flash_events.push((self.push_total, now_ms));
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
    fn row_chroma_color(&self, event: &L2Event) -> String {
        let qty_ratio = if self.max_qty_seen > 0.0 {
            (event.quantity / self.max_qty_seen).sqrt().clamp(0.3, 1.0)
        } else {
            0.3
        };

        // (r, g, b, base_alpha) — all in [0, 1]
        let (r, g, b, base_alpha) = match event.event_type {
            L2EventType::Execute => match event.side {
                L2Side::Bid  => (0.0,  0.9,  0.35, 0.30), // bright green
                L2Side::Ask  => (0.95, 0.18, 0.18, 0.30), // bright red
            },
            L2EventType::Add => match event.side {
                L2Side::Bid  => (0.12, 0.47, 0.95, 0.15), // subtle blue
                L2Side::Ask  => (0.95, 0.55, 0.05, 0.15), // subtle orange
            },
            L2EventType::Modify => (0.55, 0.35, 0.90, 0.10), // neutral purple
            L2EventType::Cancel => (0.45, 0.45, 0.50, 0.10), // gray
        };

        let alpha = (base_alpha * qty_ratio).clamp(0.0, 1.0);
        let a_byte = (alpha * 255.0) as u8;
        let r_byte = (r * 255.0) as u8;
        let g_byte = (g * 255.0) as u8;
        let b_byte = (b * 255.0) as u8;
        format!("#{:02x}{:02x}{:02x}{:02x}", r_byte, g_byte, b_byte, a_byte)
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
    pub fn handle_key(&mut self, _key: zengeld_chart::input::KeyCode) -> bool {
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
        for &(price, qty) in bids {
            let tick = self.price_to_tick(price);
            let prev_qty = self.previous_book.get(&tick).map(|(b, _)| *b).unwrap_or(0.0);

            let event_type = if qty == 0.0 && prev_qty > 0.0 {
                L2EventType::Cancel
            } else if qty > 0.0 && prev_qty == 0.0 {
                L2EventType::Add
            } else if qty > 0.0 && prev_qty > 0.0 && (qty - prev_qty).abs() > f64::EPSILON {
                L2EventType::Modify
            } else {
                let entry = self.previous_book.entry(tick).or_insert((0.0, 0.0));
                entry.0 = qty;
                continue;
            };

            if let Some(min_q) = self.min_quantity {
                let relevant_qty = if qty > 0.0 { qty } else { prev_qty };
                if relevant_qty < min_q {
                    let entry = self.previous_book.entry(tick).or_insert((0.0, 0.0));
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
            });

            let entry = self.previous_book.entry(tick).or_insert((0.0, 0.0));
            entry.0 = qty;
        }

        for &(price, qty) in asks {
            let tick = self.price_to_tick(price);
            let prev_qty = self.previous_book.get(&tick).map(|(_, a)| *a).unwrap_or(0.0);

            let event_type = if qty == 0.0 && prev_qty > 0.0 {
                L2EventType::Cancel
            } else if qty > 0.0 && prev_qty == 0.0 {
                L2EventType::Add
            } else if qty > 0.0 && prev_qty > 0.0 && (qty - prev_qty).abs() > f64::EPSILON {
                L2EventType::Modify
            } else {
                let entry = self.previous_book.entry(tick).or_insert((0.0, 0.0));
                entry.1 = qty;
                continue;
            };

            if let Some(min_q) = self.min_quantity {
                let relevant_qty = if qty > 0.0 { qty } else { prev_qty };
                if relevant_qty < min_q {
                    let entry = self.previous_book.entry(tick).or_insert((0.0, 0.0));
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
            });

            let entry = self.previous_book.entry(tick).or_insert((0.0, 0.0));
            entry.1 = qty;
        }
    }

    /// Apply a full orderbook snapshot.
    ///
    /// Deprecated — L2Tape now reads from `shared_orderbook` via `tick()`.
    /// Kept as a no-op so remaining call sites compile.
    #[deprecated(note = "Use tick() to pull data from the shared OrderbookSeries instead")]
    pub fn apply_snapshot(&mut self, _bids: &[(f64, f64)], _asks: &[(f64, f64)], _timestamp: i64) {}

    /// Apply an incremental orderbook delta — generates L2Events from changes.
    ///
    /// Deprecated — L2Tape now reads from `shared_orderbook` via `tick()`.
    /// Kept as a no-op so remaining call sites compile.
    #[deprecated(note = "Use tick() to pull data from the shared OrderbookSeries instead")]
    pub fn apply_delta(&mut self, _bids: &[(f64, f64)], _asks: &[(f64, f64)], _timestamp: i64) {}
}
