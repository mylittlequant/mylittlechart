use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::time::Instant;

/// TickerTape panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TickerTapeId(pub u64);

/// TickerTape panel state (heavy data)
#[derive(Clone, Debug)]
pub struct TickerTapeState {
    /// List of symbols to display
    pub symbols: Vec<String>,

    /// Ticker data per symbol
    pub tickers: HashMap<String, TickerData>,

    /// Scroll animation state
    pub scroll_offset: f32, // pixels scrolled
    pub scroll_speed: f32,  // pixels per second

    /// Auto-scroll vs manual
    pub auto_scroll: bool,

    /// Total width of all ticker items (for loop calculation)
    pub total_width: f32,

    /// Paused state (for user interaction)
    pub paused: bool,
}

impl TickerTapeState {
    pub fn new(symbols: Vec<String>, scroll_speed: f32) -> Self {
        Self {
            symbols,
            tickers: HashMap::new(),
            scroll_offset: 0.0,
            scroll_speed,
            auto_scroll: true,
            total_width: 0.0,
            paused: false,
        }
    }

    /// Get visible ticker items based on viewport width and scroll position
    pub fn visible_items(&self, _viewport_width: f32) -> Vec<&TickerData> {
        // For simplicity, return all tickers
        // In a real implementation, this would filter based on scroll_offset and viewport_width
        self.symbols
            .iter()
            .filter_map(|symbol| self.tickers.get(symbol))
            .collect()
    }

    /// Get color for price change (green positive, red negative)
    pub fn item_color(&self, change_pct: f64) -> [f32; 4] {
        if change_pct > 0.0 {
            // Green for positive
            let intensity = (change_pct / 10.0).min(1.0) as f32;
            [0.0, 0.5 + intensity * 0.5, 0.0, 1.0]
        } else if change_pct < 0.0 {
            // Red for negative
            let intensity = (change_pct.abs() / 10.0).min(1.0) as f32;
            [0.5 + intensity * 0.5, 0.0, 0.0, 1.0]
        } else {
            // Gray for no change
            [0.5, 0.5, 0.5, 1.0]
        }
    }

    /// Advance scroll position based on delta time
    pub fn advance_scroll(&mut self, dt: f64) {
        if !self.paused && self.auto_scroll {
            self.scroll_offset += self.scroll_speed * dt as f32;

            // Loop when reaching the end
            if self.total_width > 0.0 && self.scroll_offset >= self.total_width {
                self.scroll_offset = 0.0;
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct TickerData {
    pub symbol: String,
    pub last_price: f64,
    pub price_change_24h: f64,    // absolute change
    pub price_change_percent: f64, // percentage change
    pub volume_24h: f64,
    pub high_24h: f64,
    pub low_24h: f64,
    pub last_update: Instant,
}

/// TickerTape panel configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TickerTapeConfig {
    /// Symbols to track
    pub symbols: Vec<String>,

    /// Scroll speed (pixels per second)
    pub scroll_speed: f32,

    /// Auto-scroll enabled
    pub auto_scroll: bool,

    /// Item width (each ticker item)
    pub item_width: f32,

    /// Item spacing
    pub item_spacing: f32,

    /// Font sizes
    pub symbol_font_size: f32,
    pub price_font_size: f32,
    pub change_font_size: f32,

    /// Show fields
    pub show_volume: bool,
    pub show_high_low: bool,
}

impl Default for TickerTapeConfig {
    fn default() -> Self {
        Self {
            symbols: Vec::new(),
            scroll_speed: 50.0,
            auto_scroll: true,
            item_width: 200.0,
            item_spacing: 20.0,
            symbol_font_size: 14.0,
            price_font_size: 16.0,
            change_font_size: 12.0,
            show_volume: true,
            show_high_low: false,
        }
    }
}

/// TickerTape panel wrapper (lightweight, lives in PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TickerTapePanel {
    id: TickerTapeId,
    title: String,
}

impl TickerTapePanel {
    pub fn new(id: TickerTapeId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> TickerTapeId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "ticker_tape" }
    pub fn kind_label(&self) -> &'static str { "Ticker Tape" }
    pub fn min_size(&self) -> (f32, f32) { (300.0, 50.0) }
}
