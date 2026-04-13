//! L2 Tape: Order Book Event Stream panel state.
//!
//! Shows individual MBO (Market-By-Order) events: order additions,
//! modifications, cancellations, and executions in real-time.

use std::collections::{HashMap, VecDeque};
use serde::{Serialize, Deserialize};

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
    /// Symbol being monitored
    pub symbol: String,
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

impl L2TapeState {
    pub fn new() -> Self {
        Self {
            symbol: String::new(),
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
