//! L2 Tape: Order Book Event Stream panel state.
//!
//! Shows individual MBO (Market-By-Order) events: order additions,
//! modifications, cancellations, and executions in real-time.

use std::collections::VecDeque;
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
}
