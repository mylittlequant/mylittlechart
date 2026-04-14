use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// VolumeProfile panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VolumeProfileId(pub u64);

/// VolumeProfile panel state (heavy data)
#[derive(Clone, Debug)]
pub struct VolumeProfileState {
    /// Symbol source binding (how to resolve which instrument to display)
    pub source: crate::trading::SymbolSource,

    pub symbol: String,

    /// Time range for profile calculation
    pub start_time: i64,
    pub end_time: i64,

    /// Volume by price level: price -> total_volume
    pub volume_by_price: HashMap<i64, f64>, // Using i64 for price ticks

    /// Tick size
    pub tick_size: f64,

    /// POC (Point of Control) - price with highest volume
    pub poc: f64,

    /// Value Area High (top of 70% volume range)
    pub vah: f64,

    /// Value Area Low (bottom of 70% volume range)
    pub val: f64,

    /// Total volume across all prices
    pub total_volume: f64,

    /// Max volume at any single price (for bar scaling)
    pub max_volume_at_price: f64,

    /// Profile type
    pub profile_type: VolumeProfileType,

    /// Center price from linked DOM (for syncing price axis)
    pub dom_center_price: Option<f64>,
    /// Number of levels displayed in linked DOM
    pub dom_levels: Option<usize>,
    /// Buy/sell volume split per price tick
    pub buy_sell_by_price: HashMap<i64, (f64, f64)>,  // tick -> (buy_vol, sell_vol)
}

/// Helper struct for rendering: represents one volume level
#[derive(Debug, Clone)]
pub struct VolumeLevel {
    pub price: f64,
    pub buy_volume: f64,
    pub sell_volume: f64,
    pub total_volume: f64,
    pub is_poc: bool,
    pub is_value_area: bool,
}

impl VolumeProfileState {
    pub fn new(symbol: String, tick_size: f64) -> Self {
        Self {
            source: crate::trading::SymbolSource::default(),
            symbol,
            start_time: 0,
            end_time: 0,
            volume_by_price: HashMap::new(),
            tick_size,
            poc: 0.0,
            vah: 0.0,
            val: 0.0,
            total_volume: 0.0,
            max_volume_at_price: 0.0,
            profile_type: VolumeProfileType::Visible,
            dom_center_price: None,
            dom_levels: None,
            buy_sell_by_price: HashMap::new(),
        }
    }

    /// Returns visible price levels with volume, sorted by price descending
    pub fn visible_levels(&self) -> Vec<VolumeLevel> {
        let mut levels: Vec<VolumeLevel> = self.volume_by_price
            .iter()
            .map(|(tick, total_volume)| {
                let price = *tick as f64 * self.tick_size;
                let (buy_volume, sell_volume) = self.buy_sell_by_price
                    .get(tick)
                    .copied()
                    .unwrap_or((*total_volume * 0.5, *total_volume * 0.5));
                VolumeLevel {
                    price,
                    buy_volume,
                    sell_volume,
                    total_volume: *total_volume,
                    is_poc: self.is_poc(price),
                    is_value_area: self.is_value_area(price),
                }
            })
            .collect();

        levels.sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap_or(std::cmp::Ordering::Equal));
        levels
    }

    /// Calculate proportional bar width for rendering
    pub fn bar_width(&self, volume: f64, max_width: f32) -> f32 {
        if self.max_volume_at_price == 0.0 {
            0.0
        } else {
            (volume / self.max_volume_at_price * max_width as f64) as f32
        }
    }

    /// Check if price is within value area (VAH/VAL)
    pub fn is_value_area(&self, price: f64) -> bool {
        price >= self.val && price <= self.vah
    }

    /// Check if price is POC (Point of Control)
    pub fn is_poc(&self, price: f64) -> bool {
        (price - self.poc).abs() < self.tick_size * 0.5
    }

    /// Format volume in compact notation
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

    /// Apply a live trade to the volume profile
    pub fn push_trade(&mut self, price: f64, quantity: f64, is_buyer_maker: bool) {
        let tick = (price / self.tick_size).round() as i64;

        // Update total volume
        *self.volume_by_price.entry(tick).or_insert(0.0) += quantity;

        // Update buy/sell split
        let entry = self.buy_sell_by_price.entry(tick).or_insert((0.0, 0.0));
        if is_buyer_maker {
            entry.1 += quantity; // sell volume (seller-initiated)
        } else {
            entry.0 += quantity; // buy volume (buyer-initiated)
        }

        // Update total_volume and max_volume_at_price
        self.total_volume += quantity;
        let tick_vol = self.volume_by_price[&tick];
        if tick_vol > self.max_volume_at_price {
            self.max_volume_at_price = tick_vol;
            self.poc = tick as f64 * self.tick_size;
        }
    }

    /// Get the POC level data
    pub fn poc_level(&self) -> VolumeLevel {
        VolumeLevel {
            price: self.poc,
            buy_volume: self.max_volume_at_price * 0.5, // Approximation
            sell_volume: self.max_volume_at_price * 0.5,
            total_volume: self.max_volume_at_price,
            is_poc: true,
            is_value_area: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum VolumeProfileType {
    Visible,   // Calculate over visible time range
    Session,   // Daily session profile
    Fixed,     // User-defined time range
}

/// VolumeProfile panel configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VolumeProfileConfig {
    /// Profile type (see enum)
    pub profile_type: VolumeProfileType,

    /// Value area percentage (default: 0.70 for 70%)
    pub value_area_percent: f64,

    /// Histogram max width (% of panel width)
    pub max_bar_width: f32,

    /// Show labels (POC, VAH, VAL)
    pub show_labels: bool,

    /// Opacity for histogram bars
    pub bar_opacity: f32,
}

impl Default for VolumeProfileConfig {
    fn default() -> Self {
        Self {
            profile_type: VolumeProfileType::Visible,
            value_area_percent: 0.70,
            max_bar_width: 0.5,
            show_labels: true,
            bar_opacity: 0.7,
        }
    }
}

/// VolumeProfile panel wrapper (lightweight, lives in PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VolumeProfilePanel {
    id: VolumeProfileId,
    title: String,
}

impl VolumeProfilePanel {
    pub fn new(id: VolumeProfileId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> VolumeProfileId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "volume_profile" }
    pub fn kind_label(&self) -> &'static str { "Volume Profile" }
    pub fn min_size(&self) -> (f32, f32) { (200.0, 300.0) }
}
