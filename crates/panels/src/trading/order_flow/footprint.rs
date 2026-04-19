use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};

use trade_service::TradeSeries;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

/// Footprint panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FootprintId(pub u64);

/// Footprint panel state (heavy data)
#[derive(Clone)]
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

    /// Candle duration in milliseconds (default: 60_000 = 1 minute).
    pub candle_duration_ms: i64,

    /// Timestamp (ms) when the current open candle started.
    /// `0` means no candle has been opened yet.
    pub candle_start_ms: i64,

    /// Handle to the shared trade ring for this (exchange, symbol, account_type).
    ///
    /// `None` until `subscribe_trades` is called on the bridge. Panels read
    /// from this at tick time; they NEVER write through it.
    pub shared_trades: Option<Arc<RwLock<TradeSeries>>>,

    /// The `TradeSeries::version` we last processed in `tick()`.
    pub last_seen_trade_version: u64,

    /// Crosshair price synced from a linked chart window.
    pub crosshair_price: Option<f64>,
}

impl fmt::Debug for FootprintState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FootprintState")
            .field("symbol", &self.symbol)
            .field("exchange", &self.exchange)
            .field("account_type", &self.account_type)
            .field("candle_count", &self.footprints.len())
            .field("last_seen_trade_version", &self.last_seen_trade_version)
            .field("has_shared_trades", &self.shared_trades.is_some())
            .finish()
    }
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
            candle_duration_ms: 60_000,
            candle_start_ms: 0,
            shared_trades: None,
            last_seen_trade_version: 0,
            crosshair_price: None,
        }
    }

    /// Shift the horizontal scroll by `delta` pixels (positive = scroll right / older data).
    pub fn handle_scroll(&mut self, delta: f64) {
        self.scroll_x = (self.scroll_x + delta as f32).max(0.0);
    }

    /// Reset horizontal scroll to 0 (show latest candles).
    pub fn handle_double_click(&mut self) {
        self.scroll_x = 0.0;
    }

    /// Pull new trades from the shared series and accumulate into the current footprint candle.
    ///
    /// Call once per frame (or before render). No-op when there is no shared
    /// series or when the version has not advanced since the last call.
    pub fn tick(&mut self) {
        // Snapshot new trades from the shared ring under the lock, then release
        // immediately so we can call &mut self methods without borrow conflicts.
        let (new_version, new_trades) = {
            let handle = match self.shared_trades.as_ref() {
                Some(h) => h,
                None => return,
            };
            let series = match handle.read() {
                Ok(s) => s,
                Err(_) => return,
            };
            if series.version == self.last_seen_trade_version {
                return;
            }
            let new_count =
                (series.version.saturating_sub(self.last_seen_trade_version)) as usize;
            let len = series.trades.len();
            let skip = if new_count < len { len - new_count } else { 0 };
            let trades: Vec<(f64, f64, bool, i64)> = series
                .trades
                .iter()
                .skip(skip)
                .map(|t| (t.price, t.quantity, t.is_buyer_maker != 0, t.timestamp_ms))
                .collect();
            (series.version, trades)
        }; // lock released here

        for (price, quantity, is_buyer_maker, timestamp) in new_trades {
            let tick = (price / self.tick_size).round() as i64;

            if self.footprints.is_empty() || self.should_new_candle(timestamp) {
                // Recompute imbalances for the candle that just closed.
                if !self.footprints.is_empty() {
                    let closing_idx = self.footprints.len().saturating_sub(1);
                    self.compute_imbalances(closing_idx);
                }

                self.footprints.push(HashMap::new());
                self.poc_by_candle.push(0.0);
                self.imbalances.push(HashMap::new());
                // Align candle start to the nearest boundary so candles don't
                // drift based on when the first trade happened.
                if self.candle_start_ms == 0 {
                    self.candle_start_ms = timestamp
                        - (timestamp % self.candle_duration_ms);
                } else {
                    // Advance by exactly one duration to keep boundaries aligned.
                    self.candle_start_ms += self.candle_duration_ms;
                }
            }

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

        self.last_seen_trade_version = new_version;
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

    /// Calculate cell color based on bid/ask volume balance within a single cell.
    ///
    /// ratio = bid / ask (buy pressure over sell pressure).
    /// ratio > 1 → more buying → green tint.
    /// ratio < 1 → more selling → red tint.
    pub fn cell_color(&self, bid_vol: f64, ask_vol: f64) -> [f32; 4] {
        let ratio = if ask_vol < 0.001 && bid_vol < 0.001 {
            1.0
        } else if ask_vol < 0.001 {
            // Pure bid — strong buy
            f64::MAX
        } else {
            bid_vol / ask_vol
        };

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

    /// Compute diagonal imbalances for a single candle footprint and store them
    /// in `self.imbalances[candle_idx]`.
    ///
    /// Industry-standard diagonal rule:
    ///   - BuyImbalance  at tick N: buy[N+1]  / sell[N] > threshold  (bid absorption above)
    ///   - SellImbalance at tick N: sell[N]   / buy[N+1] > threshold (ask absorption above)
    ///
    /// Denominator guard: skip comparison when denominator < 0.001 (effectively zero).
    pub fn compute_imbalances(&mut self, candle_idx: usize) {
        let fp = match self.footprints.get(candle_idx) {
            Some(f) => f,
            None => return,
        };

        // Collect and sort ticks ascending so N+1 lookups are easy.
        let mut ticks: Vec<i64> = fp.keys().copied().collect();
        ticks.sort_unstable();

        let threshold = self.imbalance_threshold;

        let mut result: HashMap<i64, ImbalanceType> = HashMap::new();

        for &tick in &ticks {
            let next_tick = tick + 1; // one tick above
            let (_bid_n, ask_n) = match fp.get(&tick) {
                Some(&v) => v,
                None => continue,
            };
            // sell[N] = ask volume at tick N (ask-side = seller-initiated)
            let sell_n = ask_n;
            // buy[N+1] = bid volume one tick above (bid-side = buyer-initiated)
            let buy_above = match fp.get(&next_tick) {
                Some(&(bid, _ask)) => bid,
                None => 0.0,
            };

            if sell_n < 0.001 && buy_above < 0.001 {
                continue;
            }

            if sell_n < 0.001 {
                // Infinite ratio → buy imbalance
                result.insert(tick, ImbalanceType::BuyImbalance);
            } else if buy_above < 0.001 {
                // Infinite ratio → sell imbalance
                result.insert(tick, ImbalanceType::SellImbalance);
            } else {
                let buy_ratio = buy_above / sell_n;
                let sell_ratio = sell_n / buy_above;
                if buy_ratio > threshold {
                    result.insert(tick, ImbalanceType::BuyImbalance);
                } else if sell_ratio > threshold {
                    result.insert(tick, ImbalanceType::SellImbalance);
                }
            }
        }

        if let Some(slot) = self.imbalances.get_mut(candle_idx) {
            *slot = result;
        }
    }

    /// Recompute imbalances for all candles. Call after bulk data load.
    pub fn recompute_all_imbalances(&mut self) {
        let count = self.footprints.len();
        for i in 0..count {
            self.compute_imbalances(i);
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

    /// Get POC (Point of Control) price for a specific candle
    pub fn poc_price_for_candle(&self, candle_index: usize) -> Option<f64> {
        self.poc_by_candle.get(candle_index).copied()
    }

    /// Apply a live trade to the current candle's footprint.
    ///
    /// Footprint now reads from `shared_trades` via `tick()`. This method is
    /// intentionally a no-op and exists only to avoid breaking any call sites
    /// that have not yet been removed.
    #[deprecated(note = "Footprint reads from shared_trades via tick(); this method is a no-op")]
    pub fn push_trade(&mut self, _price: f64, _quantity: f64, _is_buyer_maker: bool, _timestamp: i64) {
        // No-op: Footprint now reads from the shared TradeSeries.
    }

    fn should_new_candle(&self, timestamp: i64) -> bool {
        self.candle_start_ms > 0
            && timestamp - self.candle_start_ms >= self.candle_duration_ms
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

                // POC border
                let price = price_tick as f64 * self.tick_size;
                if config.show_poc && (price - candle.poc).abs() < self.tick_size * 0.5 {
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

                // Imbalance markers — diagonal comparison result stored in candle.imbalances
                if config.show_imbalances {
                    if let Some(imb) = candle.imbalances.get(&price_tick) {
                        // Dot size: 5x5 px, placed at bottom-left corner of cell
                        let dot_size = 5.0_f64;
                        let dot_x = (candle_x + 2.0) as f64;
                        let dot_y = (cell_y + cell_h - dot_size as f32) as f64;
                        match imb {
                            ImbalanceType::BuyImbalance => {
                                ctx.set_fill_color("#00ff00ff");
                                ctx.fill_rect(dot_x, dot_y, dot_size, dot_size);
                            }
                            ImbalanceType::SellImbalance => {
                                ctx.set_fill_color("#ff0000ff");
                                ctx.fill_rect(dot_x, dot_y, dot_size, dot_size);
                            }
                        }
                    }
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
