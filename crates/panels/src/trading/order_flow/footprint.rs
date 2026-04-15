use serde::{Serialize, Deserialize};
use std::collections::HashMap;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

/// Footprint panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FootprintId(pub u64);

/// Footprint panel state (heavy data)
#[derive(Clone, Debug)]
pub struct FootprintState {
    pub symbol: String,

    /// Exchange identifier string (e.g. "binance")
    pub exchange: String,

    /// Account type short label (e.g. "S", "F", "FI")
    pub account_type: String,

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
            exchange: String::new(),
            account_type: String::new(),
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

    /// Apply a live trade to the current candle's footprint
    pub fn push_trade(&mut self, price: f64, quantity: f64, is_buyer_maker: bool, timestamp: i64) {
        let tick = (price / self.tick_size).round() as i64;

        // Check if we need a new candle
        if self.footprints.is_empty() || self.should_new_candle(timestamp) {
            self.footprints.push(HashMap::new());
            self.poc_by_candle.push(0.0);
            self.imbalances.push(HashMap::new());
        }

        // Add volume to current candle
        if let Some(current) = self.footprints.last_mut() {
            let entry = current.entry(tick).or_insert((0.0, 0.0));
            if is_buyer_maker {
                entry.0 += quantity; // seller-initiated (bid side hit)
            } else {
                entry.1 += quantity; // buyer-initiated (ask side hit)
            }

            let total = entry.0 + entry.1;
            let candle_idx = self.footprints.len().saturating_sub(1);
            if let Some(poc) = self.poc_by_candle.get_mut(candle_idx) {
                // Retrieve the current POC tick to compare volumes
                let current_poc_tick = (*poc / self.tick_size).round() as i64;
                let current_poc_vol = self.footprints
                    .last()
                    .and_then(|fp| fp.get(&current_poc_tick))
                    .map(|(b, a)| b + a)
                    .unwrap_or(0.0);
                if total > current_poc_vol {
                    *poc = tick as f64 * self.tick_size;
                }
            }
        }
    }

    fn should_new_candle(&self, _timestamp: i64) -> bool {
        // Candle boundary management is handled externally via bulk data loads.
        // Live trades accumulate into the most recent candle.
        false
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

const CELL_MIN_HEIGHT: f32 = 8.0;
const CELL_MAX_HEIGHT: f32 = 30.0;

impl TradingPanel for FootprintState {
    fn kind(&self) -> &'static str { "footprint" }
    fn label(&self) -> &'static str { "Footprint" }

    fn render(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        theme: &crate::panel_theme::PanelTheme,
    ) {
        let config = FootprintConfig::default();

        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        let num_visible = (w / config.candle_width).floor() as usize;
        let start_idx = (self.scroll_x / config.candle_width) as usize;
        let end_idx = (start_idx + num_visible).min(self.footprints.len());

        let candles = self.visible_candles(start_idx, end_idx);

        for (candle_idx, candle) in candles.iter().enumerate() {
            let candle_x = x + (candle_idx as f32 * config.candle_width);
            let candle_w = config.candle_width;

            ctx.set_fill_color(&theme.fp_bullish);
            ctx.fill_rect(candle_x as f64, y as f64, 2.0, h as f64);

            let mut price_levels: Vec<(i64, f64, f64)> = candle.price_levels.clone();
            price_levels.sort_by_key(|&(tick, _, _)| std::cmp::Reverse(tick));

            let num_levels = price_levels.len();
            if num_levels == 0 {
                continue;
            }

            let cell_height = (h / num_levels as f32).clamp(CELL_MIN_HEIGHT, CELL_MAX_HEIGHT);

            for (level_idx, &(price_tick, bid_vol, ask_vol)) in price_levels.iter().enumerate() {
                let cell_y = y + (level_idx as f32 * cell_height);
                let cell_w = candle_w - 4.0;
                let cell_h = cell_height;

                let cell_bg = self.cell_color(bid_vol, ask_vol);
                // cell_color returns f32 rgba — render directly as rgba string
                let cell_bg_hex = format!(
                    "#{:02x}{:02x}{:02x}{:02x}",
                    (cell_bg[0].clamp(0.0, 1.0) * 255.0) as u8,
                    (cell_bg[1].clamp(0.0, 1.0) * 255.0) as u8,
                    (cell_bg[2].clamp(0.0, 1.0) * 255.0) as u8,
                    (cell_bg[3].clamp(0.0, 1.0) * 255.0) as u8,
                );
                ctx.set_fill_color(&cell_bg_hex);
                ctx.fill_rect((candle_x + 2.0) as f64, cell_y as f64, cell_w as f64, cell_h as f64);

                ctx.set_fill_color(&theme.separator);
                ctx.fill_rect((candle_x + 2.0) as f64, cell_y as f64, cell_w as f64, 0.5);

                let cell_text = self.format_cell(bid_vol, ask_vol);

                ctx.set_font("9px sans-serif");
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.set_fill_color(&theme.fp_cell_text);

                let text_x = candle_x + candle_w / 2.0;
                let text_y = cell_y + cell_h / 2.0;
                ctx.fill_text(&cell_text, text_x as f64, text_y as f64);

                let price = price_tick as f64 * self.tick_size;
                if (price - candle.poc).abs() < self.tick_size * 0.5 {
                    ctx.set_fill_color(&theme.fp_poc_marker);
                    let marker_x = candle_x + candle_w - 6.0;
                    let marker_y = cell_y + cell_h / 2.0 - 3.0;
                    ctx.fill_rect(marker_x as f64, marker_y as f64, 6.0, 6.0);

                    ctx.set_fill_color(&theme.fp_poc_border);
                    ctx.fill_rect((candle_x + 2.0) as f64, cell_y as f64, cell_w as f64, 1.0);
                    ctx.fill_rect((candle_x + 2.0) as f64, (cell_y + cell_h - 1.0) as f64, cell_w as f64, 1.0);
                    ctx.fill_rect((candle_x + 2.0) as f64, cell_y as f64, 1.0, cell_h as f64);
                    ctx.fill_rect((candle_x + candle_w - 3.0) as f64, cell_y as f64, 1.0, cell_h as f64);
                }
            }
        }
    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool { false }
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
