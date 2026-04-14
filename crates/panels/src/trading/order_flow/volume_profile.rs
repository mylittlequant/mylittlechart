use serde::{Serialize, Deserialize};
use std::collections::HashMap;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

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

const VOLUME_PROFILE_BAR_HEIGHT: f32 = 4.0;

impl TradingPanel for VolumeProfileState {
    fn kind(&self) -> &'static str { "volume_profile" }
    fn label(&self) -> &'static str { "Volume Profile" }

    fn render(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        theme: &crate::panel_theme::PanelTheme,
    ) {
        let config = VolumeProfileConfig::default();

        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        let levels = self.visible_levels();
        if levels.is_empty() {
            return;
        }

        let num_levels = levels.len();
        let bar_height = (h / num_levels as f32).min(VOLUME_PROFILE_BAR_HEIGHT * 2.0).max(VOLUME_PROFILE_BAR_HEIGHT / 2.0);

        let vah_y = levels.iter()
            .position(|l| (l.price - self.vah).abs() < self.tick_size * 0.5)
            .map(|idx| y + idx as f32 * bar_height);
        let val_y = levels.iter()
            .position(|l| (l.price - self.val).abs() < self.tick_size * 0.5)
            .map(|idx| y + idx as f32 * bar_height);

        if let (Some(vah), Some(val)) = (vah_y, val_y) {
            let shade_h = (val - vah).abs() as f64;
            ctx.set_fill_color(&theme.vp_value_area);
            ctx.fill_rect(x as f64, vah as f64, w as f64, shade_h);
        }

        for (i, level) in levels.iter().enumerate() {
            let bar_y = y + (i as f32 * bar_height);
            let max_bar_pixels = w * config.max_bar_width;
            let bar_w = self.bar_width(level.total_volume, max_bar_pixels);
            let has_split = (level.buy_volume - level.sell_volume).abs() > 0.001;

            if has_split {
                let bid_w = self.bar_width(level.buy_volume, max_bar_pixels);
                let ask_w = self.bar_width(level.sell_volume, max_bar_pixels);

                ctx.set_fill_color(&theme.buy);
                ctx.fill_rect(x as f64, bar_y as f64, bid_w as f64, bar_height as f64);

                ctx.set_fill_color(&theme.sell);
                ctx.fill_rect((x + bid_w) as f64, bar_y as f64, ask_w as f64, bar_height as f64);
            } else {
                let bar_color = if level.is_poc { &theme.vp_bar_poc } else { &theme.vp_bar };
                ctx.set_fill_color(bar_color);
                ctx.fill_rect(x as f64, bar_y as f64, bar_w as f64, bar_height as f64);
            }

            if level.is_poc {
                ctx.set_fill_color(&theme.vp_poc_line);
                let line_y = bar_y + bar_height / 2.0 - 1.0;
                ctx.fill_rect(x as f64, line_y as f64, (w * 0.7) as f64, 2.0);

                if config.show_labels {
                    ctx.set_font("10px sans-serif");
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.set_fill_color(&theme.vp_poc_line);
                    ctx.fill_text("POC", (x + w * 0.72) as f64, (bar_y + bar_height / 2.0) as f64);
                }
            }
        }

        if let Some(dom_center) = self.dom_center_price {
            if let Some(idx) = levels.iter().position(|l| (l.price - dom_center).abs() < self.tick_size * 0.5) {
                let center_y = y + idx as f32 * bar_height + bar_height / 2.0;
                ctx.set_fill_color(&theme.current_price);
                ctx.fill_rect(x as f64, center_y as f64, (w * 0.8) as f64, 2.0);

                ctx.set_font("10px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.set_fill_color(&theme.current_price);
                ctx.fill_text("MKT", (x + w * 0.82) as f64, center_y as f64);
            }
        }

        if let Some(vah) = vah_y {
            ctx.set_fill_color(&theme.vp_vah_line);
            ctx.fill_rect(x as f64, vah as f64, (w * 0.6) as f64, 1.0);

            if config.show_labels {
                ctx.set_font("10px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Top);
                ctx.set_fill_color(&theme.vp_vah_line);
                ctx.fill_text("VAH", (x + w * 0.62) as f64, vah as f64);
            }
        }

        if let Some(val) = val_y {
            ctx.set_fill_color(&theme.vp_val_line);
            ctx.fill_rect(x as f64, val as f64, (w * 0.6) as f64, 1.0);

            if config.show_labels {
                ctx.set_font("10px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Top);
                ctx.set_fill_color(&theme.vp_val_line);
                ctx.fill_text("VAL", (x + w * 0.62) as f64, val as f64);
            }
        }
    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool { false }
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
