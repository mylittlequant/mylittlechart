use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::time::Instant;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

/// DOM panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DomId(pub u64);

/// DOM panel state (heavy data)
#[derive(Clone, Debug)]
pub struct DomState {
    /// Current symbol being displayed
    pub symbol: String,

    /// Exchange identifier string (e.g. "binance")
    pub exchange: String,

    /// Account type short label (e.g. "S", "F", "FI")
    pub account_type: String,

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

    /// Last full REST snapshot — wide depth, source of truth for the deep ladder.
    pub rest_bids: HashMap<u64, f64>, // price.to_bits() → qty
    pub rest_asks: HashMap<u64, f64>,
    /// Last WS snapshot — narrow window around mid, freshest values.
    pub ws_bids: HashMap<u64, f64>,
    pub ws_asks: HashMap<u64, f64>,
    /// Coverage range of the current WS snapshot (used to skip REST levels inside it).
    pub ws_bid_range: Option<(f64, f64)>,
    pub ws_ask_range: Option<(f64, f64)>,

    /// Maximum volume across all visible levels (for bar scaling)
    pub max_volume: f64,

    /// User's pending orders at each price level
    pub user_orders: HashMap<i64, Vec<String>>, // price_tick -> order_ids

    /// Hovered price level (for click-to-trade)
    pub hovered_price: Option<f64>,

    /// Recently filled volume per price (for flash animation)
    pub recent_fills: HashMap<i64, (f64, Instant)>, // volume, timestamp

    /// Auto-center mode: when true, center_price tracks market_price every update.
    /// Wheel scroll or drag switches to Manual. Double-click restores Auto.
    pub auto_center: bool,

    /// Minimum volume filter — levels with total volume below this are dimmed.
    /// 0.0 means no filter (all levels drawn at full opacity).
    pub min_volume_filter: f64,

    /// Crosshair price synced from a linked chart window.
    /// When set, a subtle highlight line is drawn across the corresponding row.
    pub crosshair_price: Option<f64>,
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
            symbol,
            exchange: String::new(),
            account_type: String::new(),
            market_price: 0.0,
            center_price: 0.0,
            levels_displayed: 20,
            tick_size,
            volume_by_price: HashMap::new(),
            rest_bids: HashMap::new(),
            rest_asks: HashMap::new(),
            ws_bids: HashMap::new(),
            ws_asks: HashMap::new(),
            ws_bid_range: None,
            ws_ask_range: None,
            max_volume: 0.0,
            user_orders: HashMap::new(),
            hovered_price: None,
            recent_fills: HashMap::new(),
            auto_center: true,
            min_volume_filter: 0.0,
            crosshair_price: None,
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

    /// Returns visible price levels that fit within the given pixel height.
    /// Center price is placed at the middle row. Row height is DOM_ROW_HEIGHT (20px).
    pub fn visible_levels_for_height(&self, height: f32) -> Vec<DomLevel> {
        let row_h = 20.0_f32; // DOM_ROW_HEIGHT
        let visible_rows = ((height / row_h) as usize).max(1);
        let half = visible_rows / 2;

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

        let mut levels = Vec::with_capacity(visible_rows);

        // DOM convention: higher prices at top, lower at bottom.
        // Row 0 = highest price (center + half), row N = lowest (center - half).
        for i in 0..visible_rows {
            let offset = half as i64 - i as i64;
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

    /// Number of decimal places to display for volume, derived from `tick_size`.
    /// Coarse ticks (≥1) → 0 decimals; finer ticks → more precision so that
    /// sub-1 quantities (typical for BTC/ETH per level) don't round to "0".
    pub fn volume_decimals(&self) -> usize {
        let ts = self.tick_size.abs();
        if ts == 0.0 || ts >= 1.0 { 2 }
        else if ts >= 0.1 { 3 }
        else if ts >= 0.01 { 3 }
        else if ts >= 0.001 { 4 }
        else { 5 }
    }

    /// Number of decimal places to display for price, derived from `tick_size`.
    pub fn price_decimals(&self) -> usize {
        let ts = self.tick_size.abs();
        if ts == 0.0 { 2 }
        else if ts >= 1.0 { 0 }
        else if ts >= 0.1 { 1 }
        else if ts >= 0.01 { 2 }
        else if ts >= 0.001 { 3 }
        else if ts >= 0.0001 { 4 }
        else { 6 }
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

    /// Apply a fresh REST orderbook snapshot. Source of truth for the wide
    /// ladder. Fully replaces the REST cache.
    pub fn apply_rest_snapshot(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)]) {
        self.rest_bids.clear();
        self.rest_asks.clear();
        for &(p, q) in bids { if q > 0.0 { self.rest_bids.insert(p.to_bits(), q); } }
        for &(p, q) in asks { if q > 0.0 { self.rest_asks.insert(p.to_bits(), q); } }
        self.update_market_price_from(bids, asks);
        self.rebuild_aggregation();
    }

    /// Apply a fresh WS orderbook snapshot. Narrow window patch over the REST
    /// cache. Fully replaces the WS cache.
    pub fn apply_ws_snapshot(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)]) {
        self.ws_bids.clear();
        self.ws_asks.clear();
        for &(p, q) in bids { if q > 0.0 { self.ws_bids.insert(p.to_bits(), q); } }
        for &(p, q) in asks { if q > 0.0 { self.ws_asks.insert(p.to_bits(), q); } }
        self.ws_bid_range = price_range(bids);
        self.ws_ask_range = price_range(asks);
        self.update_market_price_from(bids, asks);
        self.rebuild_aggregation();
    }

    fn update_market_price_from(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)]) {
        let best_bid = bids.first().map(|(p, _)| *p).unwrap_or(0.0);
        let best_ask = asks.first().map(|(p, _)| *p).unwrap_or(0.0);
        if best_bid > 0.0 && best_ask > 0.0 {
            self.market_price = (best_bid + best_ask) / 2.0;
            if self.center_price == 0.0 || self.auto_center {
                self.center_price = self.market_price;
            }
        }
    }

    /// Apply an incremental orderbook delta — writes into the WS window and rebuilds aggregation.
    pub fn apply_delta(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)]) {
        for &(p, q) in bids {
            let k = p.to_bits();
            if q == 0.0 { self.ws_bids.remove(&k); } else { self.ws_bids.insert(k, q); }
        }
        for &(p, q) in asks {
            let k = p.to_bits();
            if q == 0.0 { self.ws_asks.remove(&k); } else { self.ws_asks.insert(k, q); }
        }
        // Recompute WS coverage windows from the updated maps.
        self.ws_bid_range = recompute_window(&self.ws_bids);
        self.ws_ask_range = recompute_window(&self.ws_asks);
        self.rebuild_aggregation();
        // Update market price from WS best bid/ask.
        let best_bid = self.ws_bids.keys().map(|b| f64::from_bits(*b)).fold(f64::NEG_INFINITY, f64::max);
        let best_ask = self.ws_asks.keys().map(|b| f64::from_bits(*b)).fold(f64::INFINITY, f64::min);
        if best_bid > 0.0 && best_ask.is_finite() && best_ask > 0.0 {
            self.market_price = (best_bid + best_ask) / 2.0;
            if self.center_price == 0.0 || self.auto_center {
                self.center_price = self.market_price;
            }
        }
    }

    /// Change `tick_size` (depth aggregation granularity) and rebuild the
    /// aggregated view from raw data — no data loss, no flicker waiting for
    /// the next snapshot.
    pub fn set_tick_size(&mut self, new_tick: f64) {
        if new_tick <= 0.0 || (new_tick - self.tick_size).abs() < f64::EPSILON {
            return;
        }
        self.tick_size = new_tick;
        self.rebuild_aggregation();
    }

    /// Rebuild `volume_by_price` from REST + WS overlay using the current
    /// `tick_size` as bucket width. WS levels override REST inside the WS window.
    fn rebuild_aggregation(&mut self) {
        let mut next: HashMap<i64, (f64, f64, usize, usize)> =
            HashMap::with_capacity(self.rest_bids.len() + self.ws_bids.len());

        // 1. REST bids — skip prices inside the WS bid window (WS overrides there).
        for (&pb, &qty) in &self.rest_bids {
            let price = f64::from_bits(pb);
            if let Some((lo, hi)) = self.ws_bid_range {
                if price >= lo && price <= hi { continue; }
            }
            let tick = (price / self.tick_size).floor() as i64;
            let entry = next.entry(tick).or_insert((0.0, 0.0, 0, 0));
            entry.0 += qty;
            entry.2 += 1;
        }
        // 2. WS bids — always add (they are the truth in their window).
        for (&pb, &qty) in &self.ws_bids {
            let price = f64::from_bits(pb);
            let tick = (price / self.tick_size).floor() as i64;
            let entry = next.entry(tick).or_insert((0.0, 0.0, 0, 0));
            entry.0 += qty;
            entry.2 += 1;
        }
        // 3. REST asks — skip prices inside the WS ask window.
        for (&pb, &qty) in &self.rest_asks {
            let price = f64::from_bits(pb);
            if let Some((lo, hi)) = self.ws_ask_range {
                if price >= lo && price <= hi { continue; }
            }
            let tick = (price / self.tick_size).floor() as i64;
            let entry = next.entry(tick).or_insert((0.0, 0.0, 0, 0));
            entry.1 += qty;
            entry.3 += 1;
        }
        // 4. WS asks.
        for (&pb, &qty) in &self.ws_asks {
            let price = f64::from_bits(pb);
            let tick = (price / self.tick_size).floor() as i64;
            let entry = next.entry(tick).or_insert((0.0, 0.0, 0, 0));
            entry.1 += qty;
            entry.3 += 1;
        }
        self.volume_by_price = next;
        self.recompute_max_volume();
    }

    /// Recompute max_volume from all visible levels.
    ///
    /// Levels whose total volume is below `min_volume_filter` are excluded from
    /// the max so that filtered-out noise does not compress the bar scale.
    fn recompute_max_volume(&mut self) {
        let filter = self.min_volume_filter;
        self.max_volume = self.volume_by_price.values()
            .filter(|(bv, av, _, _)| filter <= 0.0 || (bv + av) >= filter)
            .map(|(bv, av, _, _)| bv.max(*av))
            .fold(0.0f64, f64::max);
    }
}

/// Compute the [min, max] price range covered by a list of (price, qty)
/// levels. Returns `None` if the list is empty or contains no positive qty.
fn price_range(levels: &[(f64, f64)]) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut found = false;
    for &(price, qty) in levels {
        if qty > 0.0 {
            if price < lo { lo = price; }
            if price > hi { hi = price; }
            found = true;
        }
    }
    if found { Some((lo, hi)) } else { None }
}

/// Compute the [min, max] price range from a `HashMap<price_bits, qty>`.
/// Used to recompute the WS coverage window after delta updates.
fn recompute_window(map: &HashMap<u64, f64>) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut found = false;
    for &pb in map.keys() {
        let price = f64::from_bits(pb);
        if price < lo { lo = price; }
        if price > hi { hi = price; }
        found = true;
    }
    if found { Some((lo, hi)) } else { None }
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

// ============================================================
// DOM layout constants
// ============================================================

const DOM_ROW_HEIGHT: f32 = 20.0;
const DOM_LEFT_PAD: f32 = 6.0;
const DOM_PRICE_COL_WIDTH: f32 = 70.0;

// ============================================================
// TradingPanel impl for DomState
// ============================================================

impl TradingPanel for DomState {
    fn kind(&self) -> &'static str {
        "dom"
    }

    fn label(&self) -> &'static str {
        "DOM"
    }

    fn render(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        theme: &crate::panel_theme::PanelTheme,
    ) {
        // Background fill
        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        // === STEP 1: Calculate layout ===
        let levels = self.visible_levels_for_height(h);
        let row_height = DOM_ROW_HEIGHT;

        // Column layout: [Bid Volume Bar | Price | Ask Volume Bar]
        let pad = 4.0_f32;
        let price_col_w = DOM_PRICE_COL_WIDTH;
        let avail = (w - price_col_w - pad * 2.0 - DOM_LEFT_PAD * 2.0).max(0.0);
        let vol_col_w = avail / 2.0;

        let bid_vol_col_x = x + DOM_LEFT_PAD;
        let bid_vol_col_w = vol_col_w;

        let price_col_x = bid_vol_col_x + bid_vol_col_w + pad;

        let ask_vol_col_x = price_col_x + price_col_w + pad;

        // === STEP 3: Find best bid and best ask for highlighting ===
        let best_bid_price = levels.iter()
            .filter(|level| level.is_bid)
            .map(|level| level.price)
            .max_by(|a: &f64, b: &f64| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let best_ask_price = levels.iter()
            .filter(|level| level.is_ask)
            .map(|level| level.price)
            .min_by(|a: &f64, b: &f64| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // === STEP 4: Render each price level row ===
        for (i, level) in levels.iter().enumerate() {
            let row_y = y + (i as f32 * row_height);

            // --- Step 4.1: Row background ---
            let is_best_bid = best_bid_price.map_or(false, |p| (level.price - p).abs() < 0.001);
            let is_best_ask = best_ask_price.map_or(false, |p| (level.price - p).abs() < 0.001);
            let is_current_price = (level.price - self.market_price).abs() < self.tick_size * 0.5;

            // Volume filter: dim levels whose combined volume is below the threshold.
            // Current-price row is never dimmed so the price marker stays visible.
            let total_volume = level.bid_volume + level.ask_volume;
            let is_filtered = self.min_volume_filter > 0.0
                && total_volume < self.min_volume_filter
                && !is_current_price;

            let bg_color = if is_best_bid {
                &theme.dom_best_bid_bg
            } else if is_best_ask {
                &theme.dom_best_ask_bg
            } else if level.is_spread {
                &theme.dom_spread_bg
            } else {
                &theme.panel_bg
            };

            ctx.set_fill_color(bg_color);
            ctx.fill_rect(x as f64, row_y as f64, w as f64, row_height as f64);

            // Current price row: semi-transparent gold background (~30% opacity)
            if is_current_price {
                let cp_bg = if theme.current_price.len() >= 7 {
                    format!("{}50", &theme.current_price[..7])
                } else {
                    theme.current_price.clone()
                };
                ctx.set_fill_color(&cp_bg);
                ctx.fill_rect(x as f64, row_y as f64, w as f64, row_height as f64);
            }

            // --- Step 4.1b: Hover highlight overlay ---
            let is_hovered = self.hovered_price
                .map_or(false, |hp| (level.price - hp).abs() < self.tick_size * 0.5);

            if is_hovered {
                ctx.set_fill_color(&theme.hover);
                ctx.fill_rect(x as f64, row_y as f64, w as f64, row_height as f64);

                let accent = if level.is_bid {
                    &theme.buy_bright
                } else if level.is_ask {
                    &theme.sell_bright
                } else {
                    &theme.accent
                };
                ctx.set_fill_color(accent);
                ctx.fill_rect(x as f64, row_y as f64, 3.0, row_height as f64);
            }

            // --- Step 4.1c: Crosshair highlight (synced from linked chart) ---
            let is_crosshair = self.crosshair_price
                .map_or(false, |cp| (level.price - cp).abs() < self.tick_size * 0.5);

            if is_crosshair && !is_hovered {
                // Semi-transparent white overlay — subtle, distinct from hover
                ctx.set_fill_color("#ffffff26");
                ctx.fill_rect(x as f64, row_y as f64, w as f64, row_height as f64);
            }

            // --- Step 4.2: Bid volume bar (right-aligned, grows leftward) ---
            if level.bid_volume > 0.0 && !is_filtered {
                let bar_width = self.bid_bar_width(level.bid_volume, vol_col_w);
                let bar_x = bid_vol_col_x + bid_vol_col_w - bar_width;
                let bar_y = row_y + 2.0;
                let bar_h = row_height - 4.0;

                // Use buy color for bid bars
                ctx.set_fill_color(&theme.buy);
                ctx.fill_rect(bar_x as f64, bar_y as f64, bar_width as f64, bar_h as f64);

                ctx.set_font("10px monospace");
                ctx.set_text_align(TextAlign::Right);
                ctx.set_text_baseline(TextBaseline::Middle);

                let text_x = bid_vol_col_x + bid_vol_col_w - 4.0;
                let text_y = row_y + row_height / 2.0;
                let vol_text = format!("{:.*}", self.volume_decimals(), level.bid_volume);
                let text_w = ctx.measure_text(&vol_text);
                let bid_text_color = if bar_width as f64 >= text_w {
                    &theme.text_primary
                } else {
                    &theme.buy
                };
                ctx.set_fill_color(bid_text_color);
                ctx.fill_text(&vol_text, text_x as f64, text_y as f64);
            }

            // --- Step 4.3: Ask volume bar (left-aligned, grows rightward) ---
            if level.ask_volume > 0.0 && !is_filtered {
                let bar_width = self.ask_bar_width(level.ask_volume, vol_col_w);
                let bar_x = ask_vol_col_x;
                let bar_y = row_y + 2.0;
                let bar_h = row_height - 4.0;

                ctx.set_fill_color(&theme.sell);
                ctx.fill_rect(bar_x as f64, bar_y as f64, bar_width as f64, bar_h as f64);

                ctx.set_font("10px monospace");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);

                let text_x = ask_vol_col_x + 4.0;
                let text_y = row_y + row_height / 2.0;
                let vol_text = format!("{:.*}", self.volume_decimals(), level.ask_volume);
                let text_w = ctx.measure_text(&vol_text);
                let ask_text_color = if bar_width as f64 >= text_w {
                    &theme.text_primary
                } else {
                    &theme.sell
                };
                ctx.set_fill_color(ask_text_color);
                ctx.fill_text(&vol_text, text_x as f64, text_y as f64);
            }

            // --- Step 4.4: Price text (centered in price column) ---
            let price_text_color = if is_filtered {
                &theme.text_muted
            } else if is_current_price {
                &theme.text_primary
            } else if is_best_bid {
                &theme.buy_bright
            } else if is_best_ask {
                &theme.sell_bright
            } else {
                &theme.text_primary
            };

            ctx.set_fill_color(price_text_color);
            ctx.set_font("11px monospace");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);

            let price_x = price_col_x + price_col_w / 2.0;
            let price_y = row_y + row_height / 2.0;
            let price_text = format!("{:.*}", self.price_decimals(), level.price);
            ctx.fill_text(&price_text, price_x as f64, price_y as f64);

            // --- Step 4.5: User order markers ---
            if level.has_user_order {
                ctx.set_fill_color(&theme.dom_user_order);
                ctx.set_font("10px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("▲", bid_vol_col_x as f64, price_y as f64);
            }
        }

        // === STEP 5: spread separator removed — best-bid/best-ask coloured
        // backgrounds already mark the boundary; the extra 1px line clutters
        // the median row.

        // === STEP 6: Flash animation for recent fills ===
        let now = std::time::Instant::now();
        for (price_tick, (_volume, timestamp)) in &self.recent_fills {
            let elapsed_ms = now.duration_since(*timestamp).as_millis() as u64;
            if elapsed_ms < 300 {
                let price = self.tick_to_price(*price_tick);
                if let Some(row_idx) = levels.iter().position(|l| (l.price - price).abs() < 0.001) {
                    let flash_y = y + (row_idx as f32 * row_height);
                    let level = &levels[row_idx];
                    let flash_color = if level.is_bid {
                        &theme.buy_bright
                    } else {
                        &theme.sell_bright
                    };
                    ctx.set_fill_color(flash_color);
                    ctx.fill_rect(x as f64, flash_y as f64, w as f64, row_height as f64);
                }
            }
        }
    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool {
        false
    }
}
