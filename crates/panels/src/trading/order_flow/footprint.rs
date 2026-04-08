use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// Footprint panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FootprintId(pub u64);

/// Footprint panel state (heavy data)
#[derive(Clone, Debug)]
pub struct FootprintState {
    pub symbol: String,

    /// Tick size for price grid
    pub tick_size: f64,

    /// Time range displayed (start, end)
    pub time_range: (i64, i64),

    /// Viewport scroll position
    pub scroll_x: f32,
    pub scroll_y: f32,

    /// Footprint data per candle
    /// candle_index -> price_level -> (bid_volume, ask_volume)
    pub footprints: Vec<HashMap<i64, (f64, f64)>>, // Using i64 for price ticks

    /// POC (Point of Control) per candle (price with highest total volume)
    pub poc_by_candle: Vec<f64>,

    /// Imbalance detection settings
    pub imbalance_threshold: f64, // e.g., 3.0 for 300% ratio

    /// Detected imbalances: candle_index -> price_level -> ImbalanceType
    pub imbalances: Vec<HashMap<i64, ImbalanceType>>,

    /// Display mode
    pub display_mode: FootprintMode,

    /// Center price from linked DOM (for syncing price axis)
    pub dom_center_price: Option<f64>,
    /// Number of levels displayed in linked DOM
    pub dom_levels: Option<usize>,
}

/// Helper struct for rendering: represents one footprint candle
#[derive(Debug, Clone)]
pub struct FootprintCandle {
    pub candle_index: usize,
    pub price_levels: Vec<(i64, f64, f64)>, // (price_tick, bid_volume, ask_volume)
    pub poc: f64,
    pub imbalances: HashMap<i64, ImbalanceType>,
}

impl FootprintState {
    pub fn new(symbol: String, tick_size: f64) -> Self {
        Self {
            symbol,
            tick_size,
            time_range: (0, 0),
            scroll_x: 0.0,
            scroll_y: 0.0,
            footprints: Vec::new(),
            poc_by_candle: Vec::new(),
            imbalance_threshold: 3.0,
            imbalances: Vec::new(),
            display_mode: FootprintMode::BidAsk,
            dom_center_price: None,
            dom_levels: None,
        }
    }

    /// Returns a slice of visible candles for rendering
    pub fn visible_candles(&self, start: usize, end: usize) -> Vec<FootprintCandle> {
        let end = end.min(self.footprints.len());
        let start = start.min(end);

        (start..end).map(|idx| {
            let footprint = &self.footprints[idx];
            let price_levels: Vec<(i64, f64, f64)> = footprint
                .iter()
                .map(|(tick, (bid, ask))| (*tick, *bid, *ask))
                .collect();

            let poc = self.poc_by_candle.get(idx).copied().unwrap_or(0.0);
            let imbalances = self.imbalances.get(idx).cloned().unwrap_or_default();

            FootprintCandle {
                candle_index: idx,
                price_levels,
                poc,
                imbalances,
            }
        }).collect()
    }

    /// Calculate cell color based on bid/ask volume imbalance
    pub fn cell_color(&self, bid_vol: f64, ask_vol: f64) -> [f32; 4] {
        let ratio = self.imbalance_ratio(bid_vol, ask_vol);

        if ratio > 1.5 {
            // Strong buy imbalance (green)
            [0.0, 0.8, 0.0, 0.7]
        } else if ratio > 1.2 {
            // Moderate buy imbalance (light green)
            [0.0, 0.6, 0.0, 0.5]
        } else if ratio < 0.67 {
            // Strong sell imbalance (red)
            [0.8, 0.0, 0.0, 0.7]
        } else if ratio < 0.83 {
            // Moderate sell imbalance (light red)
            [0.6, 0.0, 0.0, 0.5]
        } else {
            // Balanced (neutral)
            [0.5, 0.5, 0.5, 0.3]
        }
    }

    /// Calculate imbalance ratio (ask / bid)
    pub fn imbalance_ratio(&self, bid_vol: f64, ask_vol: f64) -> f64 {
        if bid_vol == 0.0 && ask_vol == 0.0 {
            1.0
        } else if bid_vol == 0.0 {
            f64::MAX
        } else {
            ask_vol / bid_vol
        }
    }

    /// Format volume in compact notation (1.2K, 3.5M)
    pub fn format_volume(&self, vol: f64) -> String {
        if vol >= 1_000_000_000.0 {
            format!("{:.1}B", vol / 1_000_000_000.0)
        } else if vol >= 1_000_000.0 {
            format!("{:.1}M", vol / 1_000_000.0)
        } else if vol >= 1_000.0 {
            format!("{:.1}K", vol / 1_000.0)
        } else if vol >= 1.0 {
            format!("{:.0}", vol)
        } else {
            format!("{:.2}", vol)
        }
    }

    /// Format a footprint cell based on display mode
    pub fn format_cell(&self, bid_vol: f64, ask_vol: f64) -> String {
        match self.display_mode {
            FootprintMode::BidAsk => format!("{}|{}", self.format_volume(bid_vol), self.format_volume(ask_vol)),
            FootprintMode::Delta => {
                let delta = ask_vol - bid_vol;
                format!("{:+}", delta as i64)
            }
            FootprintMode::Volume | FootprintMode::VolumeDeltaColor => {
                let total = bid_vol + ask_vol;
                self.format_volume(total)
            }
            FootprintMode::DeltaProfile => {
                let delta = ask_vol - bid_vol;
                format!("{:+.0}", delta)
            }
        }
    }

    /// Get color for imbalance visualization
    pub fn imbalance_color(&self, bid_vol: f64, ask_vol: f64) -> [f32; 4] {
        self.cell_color(bid_vol, ask_vol)
    }

    /// Get POC (Point of Control) price for a specific candle
    pub fn poc_price_for_candle(&self, candle_index: usize) -> Option<f64> {
        self.poc_by_candle.get(candle_index).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FootprintMode {
    BidAsk,           // Show bid|ask numbers
    Delta,            // Show delta (ask - bid)
    Volume,           // Show total volume
    VolumeDeltaColor, // Show volume, color by delta
    DeltaProfile,     // Horizontal bar of delta per price
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImbalanceType {
    BuyImbalance,  // Ask volume >> bid volume (buyers absorbing)
    SellImbalance, // Bid volume >> ask volume (sellers absorbing)
}

/// Footprint panel configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FootprintConfig {
    /// Candle width in pixels
    pub candle_width: f32,

    /// Price cell height in pixels
    pub price_cell_height: f32,

    /// Display mode (see FootprintMode enum)
    pub mode: FootprintMode,

    /// Imbalance detection threshold (e.g., 3.0 for 300%)
    pub imbalance_threshold: f64,

    /// Font size for volume numbers
    pub font_size: f32,

    /// Show POC line
    pub show_poc: bool,

    /// Show imbalance markers
    pub show_imbalances: bool,
}

impl Default for FootprintConfig {
    fn default() -> Self {
        Self {
            candle_width: 80.0,
            price_cell_height: 20.0,
            mode: FootprintMode::BidAsk,
            imbalance_threshold: 3.0,
            font_size: 10.0,
            show_poc: true,
            show_imbalances: true,
        }
    }
}

/// Footprint panel wrapper (lightweight, lives in PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FootprintPanel {
    id: FootprintId,
    title: String,
}

impl FootprintPanel {
    pub fn new(id: FootprintId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> FootprintId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "footprint" }
    pub fn kind_label(&self) -> &'static str { "Footprint" }
    pub fn min_size(&self) -> (f32, f32) { (300.0, 200.0) }
}
