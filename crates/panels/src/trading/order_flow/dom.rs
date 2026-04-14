use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::time::Instant;

/// DOM panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DomId(pub u64);

/// DOM panel state (heavy data)
#[derive(Clone, Debug)]
pub struct DomState {
    /// Symbol source binding (how to resolve which instrument to display)
    pub source: crate::trading::SymbolSource,

    /// Current symbol being displayed
    pub symbol: String,

    /// Current market price (last trade or mid-price)
    pub market_price: f64,

    /// Center price for ladder (usually market_price, can be adjusted)
    pub center_price: f64,

    /// Number of price levels to display above/below center (default: 20)
    pub levels_displayed: usize,

    /// Price tick size (minimum price increment)
    pub tick_size: f64,

    /// Aggregated volume per price level
    /// HashMap<price_level, (bid_volume, ask_volume, bid_order_count, ask_order_count)>
    pub volume_by_price: HashMap<i64, (f64, f64, usize, usize)>, // Using i64 for price ticks

    /// Maximum volume across all visible levels (for bar scaling)
    pub max_volume: f64,

    /// User's pending orders at each price level
    pub user_orders: HashMap<i64, Vec<String>>, // price_tick -> order_ids

    /// Hovered price level (for click-to-trade)
    pub hovered_price: Option<f64>,

    /// Recently filled volume per price (for flash animation)
    pub recent_fills: HashMap<i64, (f64, Instant)>, // volume, timestamp
}

/// Helper struct for rendering: represents one price level in the DOM
#[derive(Debug, Clone)]
pub struct DomLevel {
    pub price: f64,
    pub bid_volume: f64,
    pub ask_volume: f64,
    pub bid_orders: usize,
    pub ask_orders: usize,
    pub is_bid: bool,
    pub is_ask: bool,
    pub is_spread: bool,
    pub has_user_order: bool,
}

/// Data from DOM that can be synced to linked panels
#[derive(Clone, Debug)]
pub struct DomSyncData {
    pub symbol: String,
    pub market_price: f64,
    pub center_price: f64,
    pub tick_size: f64,
    pub levels_displayed: usize,
}

impl DomState {
    pub fn new(symbol: String, tick_size: f64) -> Self {
        Self {
            source: crate::trading::SymbolSource::default(),
            symbol,
            market_price: 0.0,
            center_price: 0.0,
            levels_displayed: 20,
            tick_size,
            volume_by_price: HashMap::new(),
            max_volume: 0.0,
            user_orders: HashMap::new(),
            hovered_price: None,
            recent_fills: HashMap::new(),
        }
    }

    /// Get sync data for propagating to linked panels
    pub fn sync_data(&self) -> DomSyncData {
        DomSyncData {
            symbol: self.symbol.clone(),
            market_price: self.market_price,
            center_price: self.center_price,
            tick_size: self.tick_size,
            levels_displayed: self.levels_displayed,
        }
    }

    /// Returns visible price levels around center_price, ready for rendering
    pub fn visible_levels(&self) -> Vec<DomLevel> {
        let mut levels = Vec::new();
        let center_tick = self.price_to_tick(self.center_price);

        // Best bid/ask for spread detection
        let best_bid_tick = self.volume_by_price.iter()
            .filter(|(_, (bid_vol, _, _, _))| *bid_vol > 0.0)
            .map(|(tick, _)| *tick)
            .max();
        let best_ask_tick = self.volume_by_price.iter()
            .filter(|(_, (_, ask_vol, _, _))| *ask_vol > 0.0)
            .map(|(tick, _)| *tick)
            .min();

        for i in 0..=(self.levels_displayed * 2) {
            let offset = i as i64 - self.levels_displayed as i64;
            let tick = center_tick + offset;
            let price = self.tick_to_price(tick);

            let (bid_volume, ask_volume, bid_orders, ask_orders) =
                self.volume_by_price.get(&tick).copied().unwrap_or((0.0, 0.0, 0, 0));

            let is_spread = match (best_bid_tick, best_ask_tick) {
                (Some(bid), Some(ask)) => tick > bid && tick < ask,
                _ => false,
            };

            let has_user_order = self.user_orders.contains_key(&tick);

            levels.push(DomLevel {
                price,
                bid_volume,
                ask_volume,
                bid_orders,
                ask_orders,
                is_bid: bid_volume > 0.0,
                is_ask: ask_volume > 0.0,
                is_spread,
                has_user_order,
            });
        }

        levels
    }

    /// Convert price to tick index
    pub fn price_to_tick(&self, price: f64) -> i64 {
        (price / self.tick_size).round() as i64
    }

    /// Convert tick index back to price
    pub fn tick_to_price(&self, tick: i64) -> f64 {
        tick as f64 * self.tick_size
    }

    /// Calculate proportional width for bid bar
    pub fn bid_bar_width(&self, volume: f64, max_width: f32) -> f32 {
        if self.max_volume == 0.0 {
            0.0
        } else {
            (volume / self.max_volume * max_width as f64) as f32
        }
    }

    /// Calculate proportional width for ask bar
    pub fn ask_bar_width(&self, volume: f64, max_width: f32) -> f32 {
        if self.max_volume == 0.0 {
            0.0
        } else {
            (volume / self.max_volume * max_width as f64) as f32
        }
    }

    /// Get current spread (best ask - best bid)
    pub fn spread(&self) -> f64 {
        let best_bid = self.volume_by_price.iter()
            .filter(|(_, (bid_vol, _, _, _))| *bid_vol > 0.0)
            .map(|(tick, _)| self.tick_to_price(*tick))
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let best_ask = self.volume_by_price.iter()
            .filter(|(_, (_, ask_vol, _, _))| *ask_vol > 0.0)
            .map(|(tick, _)| self.tick_to_price(*tick))
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        match (best_ask, best_bid) {
            (Some(ask), Some(bid)) => ask - bid,
            _ => 0.0,
        }
    }

    /// Apply a full orderbook snapshot — replaces all volume data.
    pub fn apply_snapshot(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)]) {
        self.volume_by_price.clear();
        for &(price, qty) in bids {
            if qty > 0.0 {
                let tick = self.price_to_tick(price);
                let entry = self.volume_by_price.entry(tick).or_insert((0.0, 0.0, 0, 0));
                entry.0 += qty;   // bid volume
                entry.2 += 1;     // bid order count
            }
        }
        for &(price, qty) in asks {
            if qty > 0.0 {
                let tick = self.price_to_tick(price);
                let entry = self.volume_by_price.entry(tick).or_insert((0.0, 0.0, 0, 0));
                entry.1 += qty;   // ask volume
                entry.3 += 1;     // ask order count
            }
        }
        self.recompute_max_volume();
        // Update market price from best bid/ask mid
        let best_bid = bids.first().map(|(p, _)| *p).unwrap_or(0.0);
        let best_ask = asks.first().map(|(p, _)| *p).unwrap_or(0.0);
        if best_bid > 0.0 && best_ask > 0.0 {
            self.market_price = (best_bid + best_ask) / 2.0;
            if self.center_price == 0.0 {
                self.center_price = self.market_price;
            }
        }
    }

    /// Apply an incremental orderbook delta — update changed levels only.
    pub fn apply_delta(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)]) {
        for &(price, qty) in bids {
            let tick = self.price_to_tick(price);
            if qty == 0.0 {
                // Remove level
                if let Some(entry) = self.volume_by_price.get_mut(&tick) {
                    entry.0 = 0.0;
                    entry.2 = 0;
                    if entry.1 == 0.0 {
                        self.volume_by_price.remove(&tick);
                    }
                }
            } else {
                let entry = self.volume_by_price.entry(tick).or_insert((0.0, 0.0, 0, 0));
                entry.0 = qty;
                entry.2 = 1;
            }
        }
        for &(price, qty) in asks {
            let tick = self.price_to_tick(price);
            if qty == 0.0 {
                if let Some(entry) = self.volume_by_price.get_mut(&tick) {
                    entry.1 = 0.0;
                    entry.3 = 0;
                    if entry.0 == 0.0 {
                        self.volume_by_price.remove(&tick);
                    }
                }
            } else {
                let entry = self.volume_by_price.entry(tick).or_insert((0.0, 0.0, 0, 0));
                entry.1 = qty;
                entry.3 = 1;
            }
        }
        self.recompute_max_volume();
        // Update market price from best bid/ask
        let best_bid = self.volume_by_price.iter()
            .filter(|(_, (bv, _, _, _))| *bv > 0.0)
            .map(|(t, _)| self.tick_to_price(*t))
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let best_ask = self.volume_by_price.iter()
            .filter(|(_, (_, av, _, _))| *av > 0.0)
            .map(|(t, _)| self.tick_to_price(*t))
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if let (Some(bid), Some(ask)) = (best_bid, best_ask) {
            self.market_price = (bid + ask) / 2.0;
        }
    }

    /// Recompute max_volume from all visible levels.
    fn recompute_max_volume(&mut self) {
        self.max_volume = self.volume_by_price.values()
            .map(|(bv, av, _, _)| bv.max(*av))
            .fold(0.0f64, f64::max);
    }
}

/// DOM panel configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DomConfig {
    /// Number of levels to display (default: 20 each side = 40 total)
    pub ladder_depth: usize,

    /// Auto-center on market price vs manual scroll
    pub auto_center: bool,

    /// Volume bar max width (% of column width)
    pub volume_bar_max_width: f32,

    /// Show order count vs only volume
    pub show_order_count: bool,

    /// Flash duration for fills (ms)
    pub fill_flash_duration_ms: u64,

    /// Font size for price/volume
    pub font_size: f32,

    /// Column widths
    pub price_column_width: f32,
    pub volume_column_width: f32,
    pub order_count_column_width: f32,
}

impl Default for DomConfig {
    fn default() -> Self {
        Self {
            ladder_depth: 20,
            auto_center: true,
            volume_bar_max_width: 0.8,
            show_order_count: true,
            fill_flash_duration_ms: 500,
            font_size: 12.0,
            price_column_width: 100.0,
            volume_column_width: 150.0,
            order_count_column_width: 60.0,
        }
    }
}

/// DOM panel wrapper (lightweight, lives in PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DomPanel {
    id: DomId,
    title: String,
}

impl DomPanel {
    pub fn new(id: DomId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> DomId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "dom" }
    pub fn kind_label(&self) -> &'static str { "DOM" }
    pub fn min_size(&self) -> (f32, f32) { (200.0, 150.0) }
}
