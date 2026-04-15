//! L2 Tape: Order Book Event Stream panel state.
//!
//! Shows individual MBO (Market-By-Order) events: order additions,
//! modifications, cancellations, and executions in real-time.

use std::collections::{HashMap, VecDeque};
use serde::{Serialize, Deserialize};

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

/// L2 Tape panel state
#[derive(Clone, Debug)]
pub struct L2TapeState {
    /// Symbol source binding (how to resolve which instrument to display)
    pub source: crate::trading::SymbolSource,

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
    /// Flash animation state
    pub flash_events: Vec<(usize, u64)>,
    /// Spoofing detection: recent large order adds that cancelled quickly
    pub spoof_alerts: VecDeque<SpoofAlert>,
    /// Previous orderbook state for event classification (Add vs Modify vs Cancel)
    pub previous_book: HashMap<i64, (f64, f64)>,  // tick -> (bid_qty, ask_qty)
    /// Tick size for price-to-tick conversion
    pub tick_size: f64,
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
    ) {
        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        let time_w  = (w * 0.28).max(70.0);
        let type_w  = (w * 0.12).max(30.0);
        let side_w  = (w * 0.12).max(28.0);
        let price_w = (w * 0.24).max(60.0);

        let col_time_x  = x + L2_LEFT_PAD;
        let col_type_x  = col_time_x  + time_w;
        let col_side_x  = col_type_x  + type_w;
        let col_price_x = col_side_x  + side_w;
        let col_qty_x   = col_price_x + price_w;

        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, L2_HEADER_HEIGHT as f64);

        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&theme.text_header);

        let header_text_y = (y + L2_HEADER_HEIGHT / 2.0) as f64;
        ctx.fill_text("TIME",  col_time_x  as f64, header_text_y);
        ctx.fill_text("TYPE",  col_type_x  as f64, header_text_y);
        ctx.fill_text("SIDE",  col_side_x  as f64, header_text_y);
        ctx.fill_text("PRICE", col_price_x as f64, header_text_y);
        ctx.fill_text("QTY",   col_qty_x   as f64, header_text_y);

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

        for (row_idx, event) in events.iter().enumerate() {
            let row_y = y + L2_HEADER_HEIGHT + (row_idx as f32 * L2_ROW_HEIGHT);
            let row_mid_y = (row_y + L2_ROW_HEIGHT / 2.0) as f64;

            let row_bg = if row_idx % 2 == 0 { &theme.panel_bg } else { &theme.row_bg_alt };
            ctx.set_fill_color(row_bg);
            ctx.fill_rect(x as f64, row_y as f64, w as f64, L2_ROW_HEIGHT as f64);

            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);

            let total_secs = (event.timestamp / 1000) % 86400;
            let hours  = total_secs / 3600;
            let mins   = (total_secs % 3600) / 60;
            let secs   = total_secs % 60;
            let millis = event.timestamp % 1000;
            let time_str = format!("{:02}:{:02}:{:02}.{:03}", hours, mins, secs, millis);

            ctx.set_fill_color(&theme.text_primary);
            ctx.fill_text(&time_str, col_time_x as f64, row_mid_y);

            // event_color returns f32 rgba — convert to hex inline
            let type_color = self.event_color(event);
            let type_hex = format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                (type_color[0].clamp(0.0, 1.0) * 255.0) as u8,
                (type_color[1].clamp(0.0, 1.0) * 255.0) as u8,
                (type_color[2].clamp(0.0, 1.0) * 255.0) as u8,
                (type_color[3].clamp(0.0, 1.0) * 255.0) as u8,
            );
            ctx.set_fill_color(&type_hex);
            ctx.fill_text(L2TapeState::event_label(&event.event_type), col_type_x as f64, row_mid_y);

            let side_color = match event.side {
                L2Side::Bid => &theme.buy,
                L2Side::Ask => &theme.sell,
            };
            ctx.set_fill_color(side_color);
            ctx.fill_text(L2TapeState::side_label(&event.side), col_side_x as f64, row_mid_y);

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
            let price_str = format!("{:.prec$}", event.price, prec = decimals);
            ctx.set_fill_color(&theme.text_primary);
            ctx.fill_text(&price_str, col_price_x as f64, row_mid_y);

            let qty_str = if event.quantity >= 1000.0 {
                format!("{:.0}", event.quantity)
            } else if event.quantity >= 1.0 {
                format!("{:.2}", event.quantity)
            } else {
                format!("{:.4}", event.quantity)
            };
            ctx.fill_text(&qty_str, col_qty_x as f64, row_mid_y);
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
    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool { false }
}

impl L2TapeState {
    pub fn new() -> Self {
        Self {
            source: crate::trading::SymbolSource::default(),
            symbol: String::new(),
            exchange: String::new(),
            account_type: String::new(),
            events: VecDeque::new(),
            max_events: 10000,
            auto_scroll: true,
            filter_type: None,
            filter_side: None,
            min_quantity: None,
            dom_market_price: None,
            dom_tick_size: None,
            scroll_offset: 0.0,
            flash_events: vec![],
            spoof_alerts: VecDeque::new(),
            previous_book: HashMap::new(),
            tick_size: 0.01,
        }
    }

    /// Get visible events (most recent first), applying filters
    pub fn visible_events(&self, max_count: usize) -> Vec<&L2Event> {
        self.events
            .iter()
            .rev()
            .filter(|e| {
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
            .take(max_count)
            .collect()
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

    /// Add an event to the buffer
    pub fn push_event(&mut self, event: L2Event) {
        if self.events.len() >= self.max_events {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    /// Set tick size (called when panel is linked to a symbol with known tick size)
    pub fn set_tick_size(&mut self, tick_size: f64) {
        self.tick_size = if tick_size > 0.0 { tick_size } else { 0.01 };
    }

    fn price_to_tick(&self, price: f64) -> i64 {
        (price / self.tick_size).round() as i64
    }

    /// Apply a full orderbook snapshot — resets previous_book, generates no events
    /// (snapshot is the baseline, events come from subsequent deltas).
    pub fn apply_snapshot(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], _timestamp: i64) {
        self.previous_book.clear();
        for &(price, qty) in bids {
            let tick = self.price_to_tick(price);
            let entry = self.previous_book.entry(tick).or_insert((0.0, 0.0));
            entry.0 = qty;
        }
        for &(price, qty) in asks {
            let tick = self.price_to_tick(price);
            let entry = self.previous_book.entry(tick).or_insert((0.0, 0.0));
            entry.1 = qty;
        }
        // Update synced market price
        let best_bid = bids.first().map(|(p, _)| *p).unwrap_or(0.0);
        let best_ask = asks.first().map(|(p, _)| *p).unwrap_or(0.0);
        if best_bid > 0.0 && best_ask > 0.0 {
            self.dom_market_price = Some((best_bid + best_ask) / 2.0);
        }
    }

    /// Apply an incremental orderbook delta — generates L2Events from changes.
    pub fn apply_delta(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], timestamp: i64) {
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
                continue; // No change
            };

            // Apply min_quantity filter early to avoid filling the buffer with noise
            if let Some(min_q) = self.min_quantity {
                let relevant_qty = if qty > 0.0 { qty } else { prev_qty };
                if relevant_qty < min_q {
                    // Still update previous_book, just don't generate event
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

            // Update previous state
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
}
