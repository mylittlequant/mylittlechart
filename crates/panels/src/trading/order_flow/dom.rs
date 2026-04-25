use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use orderbook_service::OrderbookSeries;
use trade_service::TradeSeries;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

/// Which columns to show in the DOM panel + optional custom separator offsets.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DomColumnConfig {
    pub show_bid_orders: bool,
    pub show_sell_trades: bool,
    pub show_buy_trades: bool,
    pub show_ask_orders: bool,
}

impl Default for DomColumnConfig {
    fn default() -> Self {
        Self {
            show_bid_orders: true,
            show_sell_trades: true,
            show_buy_trades: true,
            show_ask_orders: true,
        }
    }
}

/// DOM panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DomId(pub u64);

/// DOM panel state (heavy data)
#[derive(Clone)]
pub struct DomState {
    /// Current symbol being displayed
    pub symbol: String,

    /// Exchange identifier string (e.g. "binance")
    pub exchange: String,

    /// Account type short label (e.g. "S", "F", "FI")
    pub account_type: String,

    /// Current market price (last trade or mid-price)
    pub market_price: f64,

    /// Best bid price from the shared orderbook (max of `current.bids`).
    /// Source of truth for highlighting — independent of which buckets are
    /// currently visible on screen.
    pub best_bid_price: Option<f64>,

    /// Best ask price from the shared orderbook (min of `current.asks`).
    pub best_ask_price: Option<f64>,

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

    /// Shared orderbook series (written by the bridge, read here each tick).
    pub shared_orderbook: Option<Arc<RwLock<OrderbookSeries>>>,

    /// Last `OrderbookSnapshot::version` we consumed.  When `series.current.version`
    /// differs we pull a fresh aggregation from the shared series.
    pub last_seen_orderbook_version: u64,

    /// Shared trade series — read each tick to accumulate trade volume per price.
    pub shared_trades: Option<Arc<RwLock<TradeSeries>>>,

    /// Last trade series version we consumed.
    pub last_seen_trade_version: u64,

    /// Accumulated buy/sell trade volume per price tick for the last N minutes.
    /// HashMap<price_tick, (buy_qty, sell_qty)>
    pub trade_volume_by_price: HashMap<i64, (f64, f64)>,

    /// Window for trade accumulation in minutes (default: 8).
    pub trade_window_minutes: u64,

    /// Maximum trade volume across visible levels (for bar scaling).
    pub max_trade_volume: f64,

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

    // --- Chase Tracker ---

    /// Consecutive ticks best_bid has moved upward.
    pub chase_bid_streak: i32,

    /// Consecutive ticks best_ask has moved downward.
    pub chase_ask_streak: i32,

    /// Previous best bid (for direction comparison).
    pub last_best_bid: f64,

    /// Previous best ask (for direction comparison).
    pub last_best_ask: f64,

    /// Timestamp (ms) of the last chase update, for the 200ms window.
    pub chase_last_update_ms: i64,

    /// Column visibility configuration.
    pub column_config: DomColumnConfig,
}

impl fmt::Debug for DomState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DomState")
            .field("symbol", &self.symbol)
            .field("exchange", &self.exchange)
            .field("account_type", &self.account_type)
            .field("market_price", &self.market_price)
            .field("center_price", &self.center_price)
            .field("tick_size", &self.tick_size)
            .field("auto_center", &self.auto_center)
            .finish_non_exhaustive()
    }
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
            best_bid_price: None,
            best_ask_price: None,
            center_price: 0.0,
            levels_displayed: 20,
            tick_size,
            volume_by_price: HashMap::new(),
            max_volume: 0.0,
            shared_orderbook: None,
            last_seen_orderbook_version: 0,
            shared_trades: None,
            last_seen_trade_version: 0,
            trade_volume_by_price: HashMap::new(),
            trade_window_minutes: 8,
            max_trade_volume: 0.0,
            user_orders: HashMap::new(),
            hovered_price: None,
            recent_fills: HashMap::new(),
            auto_center: true,
            min_volume_filter: 0.0,
            crosshair_price: None,
            chase_bid_streak: 0,
            chase_ask_streak: 0,
            last_best_bid: 0.0,
            last_best_ask: 0.0,
            chase_last_update_ms: 0,
            column_config: DomColumnConfig::default(),
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

    /// Pull the latest snapshot from `shared_orderbook` and update derived state.
    ///
    /// Returns immediately when there is no shared handle or when the version
    /// has not advanced since the last call.
    pub fn tick(&mut self) {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        // --- Orderbook update ---
        if let Some(ref ob_handle) = self.shared_orderbook.clone() {
            if let Ok(series) = ob_handle.read() {
                if series.current.version != self.last_seen_orderbook_version {
                    self.last_seen_orderbook_version = series.current.version;

                    self.best_bid_price = series.current.best_bid();
                    self.best_ask_price = series.current.best_ask();
                    if let Some(mid) = series.current.mid() {
                        self.market_price = mid;
                        if self.center_price == 0.0 || self.auto_center {
                            self.center_price = mid;
                        }
                    }

                    let bids: Vec<_> = series.current.bids.iter().map(|(k, &v)| (k.0, v)).collect();
                    let asks: Vec<_> = series.current.asks.iter().map(|(k, &v)| (k.0, v)).collect();
                    drop(series);
                    self.rebuild_aggregation_from_levels(&bids, &asks);
                }
            }
        }

        // --- Chase Tracker update ---
        let cur_bid = self.best_bid_price.unwrap_or(0.0);
        let cur_ask = self.best_ask_price.unwrap_or(0.0);

        if self.last_best_bid > 0.0 && self.last_best_ask > 0.0 {
            let dt_ms = now_ms - self.chase_last_update_ms;
            let in_window = dt_ms <= 200;

            let bid_moved_up = cur_bid > self.last_best_bid + f64::EPSILON;
            let ask_moved_down = cur_ask < self.last_best_ask - f64::EPSILON
                && cur_ask > 0.0;

            if bid_moved_up && in_window {
                self.chase_bid_streak += 1;
                self.chase_ask_streak = 0;
            } else if ask_moved_down && in_window {
                self.chase_ask_streak += 1;
                self.chase_bid_streak = 0;
            } else {
                // Fade: reduce streaks by 1 each tick
                self.chase_bid_streak = (self.chase_bid_streak - 1).max(0);
                self.chase_ask_streak = (self.chase_ask_streak - 1).max(0);
            }
        }

        if cur_bid > 0.0 { self.last_best_bid = cur_bid; }
        if cur_ask > 0.0 { self.last_best_ask = cur_ask; }
        self.chase_last_update_ms = now_ms;

        // --- Trade volume accumulation ---
        if let Some(ref trade_handle) = self.shared_trades.clone() {
            if let Ok(series) = trade_handle.read() {
                if series.version != self.last_seen_trade_version {
                    self.last_seen_trade_version = series.version;

                    let cutoff_ms = now_ms - (self.trade_window_minutes as i64 * 60 * 1000);
                    self.trade_volume_by_price.clear();

                    let (s0, s1) = series.as_slices();
                    for trade in s0.iter().chain(s1.iter()) {
                        if trade.timestamp_ms < cutoff_ms {
                            continue;
                        }
                        let tick = (trade.price / self.tick_size).round() as i64;
                        let entry = self.trade_volume_by_price.entry(tick).or_insert((0.0, 0.0));
                        // is_buyer_maker=1 means seller was the taker → sell trade
                        // is_buyer_maker=0 means buyer was the taker → buy trade
                        if trade.is_buyer_maker == 0 {
                            entry.0 += trade.quantity; // buy
                        } else {
                            entry.1 += trade.quantity; // sell
                        }
                    }
                    drop(series);
                    self.recompute_max_trade_volume();
                }
            }
        }
    }

    /// Recompute max_trade_volume across all stored trade levels.
    fn recompute_max_trade_volume(&mut self) {
        self.max_trade_volume = self.trade_volume_by_price.values()
            .map(|(buy, sell)| buy.max(*sell))
            .fold(0.0_f64, f64::max);
    }

    /// Rebuild `volume_by_price` from flat bid/ask level lists.
    fn rebuild_aggregation_from_levels(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)]) {
        // Side-aware rounding (flowsurface model): bids round DOWN, asks round UP.
        // Guarantees that bid and ask buckets never collide on the same row, so
        // each bucket has a single dominant side. Without this, a bid at 76095
        // and an ask at 76105 (tick=10) would both land in bucket 76090..76100
        // and the renderer can't decide which side it represents.
        self.volume_by_price.clear();
        for &(price, qty) in bids {
            let tick = (price / self.tick_size).floor() as i64;
            let entry = self.volume_by_price.entry(tick).or_insert((0.0, 0.0, 0, 0));
            entry.0 += qty;
            entry.2 += 1;
        }
        for &(price, qty) in asks {
            let tick = (price / self.tick_size).ceil() as i64;
            let entry = self.volume_by_price.entry(tick).or_insert((0.0, 0.0, 0, 0));
            entry.1 += qty;
            entry.3 += 1;
        }
        self.recompute_max_volume();
    }

    /// Apply a fresh REST orderbook snapshot.
    ///
    /// Deprecated — DOM now reads from `shared_orderbook` via `tick()`.
    /// Kept as a no-op so that any remaining call sites compile without changes.
    #[deprecated(note = "Use tick() to pull data from the shared OrderbookSeries instead")]
    pub fn apply_rest_snapshot(&mut self, _bids: &[(f64, f64)], _asks: &[(f64, f64)]) {}

    /// Apply a fresh WS orderbook snapshot.
    ///
    /// Deprecated — DOM now reads from `shared_orderbook` via `tick()`.
    #[deprecated(note = "Use tick() to pull data from the shared OrderbookSeries instead")]
    pub fn apply_ws_snapshot(&mut self, _bids: &[(f64, f64)], _asks: &[(f64, f64)]) {}

    /// Apply an incremental orderbook delta.
    ///
    /// Deprecated — DOM now reads from `shared_orderbook` via `tick()`.
    #[deprecated(note = "Use tick() to pull data from the shared OrderbookSeries instead")]
    pub fn apply_delta(&mut self, _bids: &[(f64, f64)], _asks: &[(f64, f64)]) {}

    /// Handle a named key event.  Returns `true` if the key was consumed.
    pub fn handle_key(&mut self, _key: zengeld_chart::input::KeyCode) -> bool {
        false
    }

    /// Change `tick_size` (depth aggregation granularity) and re-pull from the
    /// shared series so the change takes effect immediately.
    pub fn set_tick_size(&mut self, new_tick: f64) {
        if new_tick <= 0.0 || (new_tick - self.tick_size).abs() < f64::EPSILON {
            return;
        }
        self.tick_size = new_tick;
        // Force a full rebuild on the next tick by resetting the version cursor.
        self.last_seen_orderbook_version = 0;
        self.last_seen_trade_version = 0;
        self.tick();
    }

    /// Recompute max_volume from ALL stored levels (called after each data update).
    ///
    /// This is a coarse pass so that `max_volume` is never zero when render runs.
    /// The render path immediately overrides it with the visible-only max via
    /// `recompute_max_volume_for_visible`.
    fn recompute_max_volume(&mut self) {
        let filter = self.min_volume_filter;
        self.max_volume = self.volume_by_price.values()
            .filter(|(bv, av, _, _)| filter <= 0.0 || (bv + av) >= filter)
            .map(|(bv, av, _, _)| bv.max(*av))
            .fold(0.0f64, f64::max);
    }

    /// Recompute max_volume restricted to the price ticks that are actually
    /// visible on screen for the given panel height.  Must be called at the
    /// start of each render frame so that off-screen iceberg orders cannot
    /// compress the visible bar scale.
    pub fn recompute_max_volume_for_visible(&mut self, height: f32) {
        let row_h = 20.0_f32; // DOM_ROW_HEIGHT
        let visible_rows = ((height / row_h) as usize).max(1);
        let half = visible_rows / 2;
        let center_tick = self.price_to_tick(self.center_price);
        let filter = self.min_volume_filter;

        let mut max_vol = 0.0_f64;
        for i in 0..visible_rows {
            let offset = half as i64 - i as i64;
            let tick = center_tick + offset;
            if let Some(&(bv, av, _, _)) = self.volume_by_price.get(&tick) {
                if filter <= 0.0 || (bv + av) >= filter {
                    let m = bv.max(av);
                    if m > max_vol {
                        max_vol = m;
                    }
                }
            }
        }
        // Only override if we found at least one visible level; fall back to
        // the global max so bars never vanish completely on an empty view.
        if max_vol > 0.0 {
            self.max_volume = max_vol;
        }
    }
}

/// Format a volume value using K / M suffixes when the magnitude warrants it.
/// Values below 1000 are printed with the given decimal places unchanged.
fn abbreviate_number(val: f64, decimals: usize) -> String {
    if val >= 1_000_000.0 {
        let m = val / 1_000_000.0;
        // One decimal place is enough for M-scale; trim trailing ".0"
        if (m - m.floor()).abs() < 0.05 {
            format!("{:.0}M", m)
        } else {
            format!("{:.1}M", m)
        }
    } else if val >= 1_000.0 {
        let k = val / 1_000.0;
        if (k - k.floor()).abs() < 0.05 {
            format!("{:.0}K", k)
        } else {
            format!("{:.1}K", k)
        }
    } else {
        format!("{:.*}", decimals, val)
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

// ============================================================
// DOM layout constants
// ============================================================

const DOM_ROW_HEIGHT: f32 = 20.0;
const DOM_LEFT_PAD: f32 = 6.0;
const DOM_PRICE_COL_WIDTH: f32 = 70.0;
const DOM_COL_HEADER_HEIGHT: f32 = 16.0;

// Trade column proportions relative to available width.
// Layout: [BidOrders 25%] [SellTrades 10%] [Price ~30%] [BuyTrades 10%] [AskOrders 25%]
// These are fractions of the non-price non-pad available width.
const TRADE_COL_FRACTION: f32 = 0.10; // each trade column as fraction of avail
const ORDER_COL_FRACTION: f32 = 0.25; // each order column as fraction of avail

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
        _coordinator: &mut uzor::InputCoordinator,
        _slot_prefix: &str,
    ) {
        // Background fill
        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        // ── Column header sub-row ────────────────────────────────────────
        let col_header_y = y;
        let body_y = y + DOM_COL_HEADER_HEIGHT;
        let body_h = (h - DOM_COL_HEADER_HEIGHT).max(0.0);

        // Header background (slightly different from panel bg)
        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, col_header_y as f64, w as f64, DOM_COL_HEADER_HEIGHT as f64);

        // === STEP 0: Restrict bar scale to visible rows only ===
        let visible_max_volume: f64 = {
            let row_h = DOM_ROW_HEIGHT;
            let visible_rows = ((body_h / row_h) as usize).max(1);
            let half = visible_rows / 2;
            let center_tick = self.price_to_tick(self.center_price);
            let filter = self.min_volume_filter;
            let mut mv = 0.0_f64;
            for i in 0..visible_rows {
                let offset = half as i64 - i as i64;
                let tick = center_tick + offset;
                if let Some(&(bv, av, _, _)) = self.volume_by_price.get(&tick) {
                    if filter <= 0.0 || (bv + av) >= filter {
                        let m = bv.max(av);
                        if m > mv { mv = m; }
                    }
                }
            }
            if mv > 0.0 { mv } else { self.max_volume }
        };

        // Compute visible max trade volume for scaling trade bars.
        let visible_max_trade: f64 = {
            let row_h = DOM_ROW_HEIGHT;
            let visible_rows = ((body_h / row_h) as usize).max(1);
            let half = visible_rows / 2;
            let center_tick = self.price_to_tick(self.center_price);
            let mut mv = 0.0_f64;
            for i in 0..visible_rows {
                let offset = half as i64 - i as i64;
                let tick = center_tick + offset;
                if let Some(&(buy, sell)) = self.trade_volume_by_price.get(&tick) {
                    let m = buy.max(sell);
                    if m > mv { mv = m; }
                }
            }
            if mv > 0.0 { mv } else { self.max_trade_volume }
        };

        // === STEP 1: Calculate layout ===
        let levels = self.visible_levels_for_height(body_h);
        let row_height = DOM_ROW_HEIGHT;

        // Column layout — only visible columns get width
        let pad = 4.0_f32;
        let price_col_w = DOM_PRICE_COL_WIDTH;
        let avail = (w - price_col_w - pad * 2.0 - DOM_LEFT_PAD * 2.0).max(0.0);

        // Count visible columns on each side (left = bid_orders + sell_trades, right = buy_trades + ask_orders)
        let left_count = self.column_config.show_bid_orders as u8 + self.column_config.show_sell_trades as u8;
        let right_count = self.column_config.show_buy_trades as u8 + self.column_config.show_ask_orders as u8;
        let total_side_cols = left_count + right_count;

        // Distribute available width proportionally
        let left_avail = if total_side_cols > 0 {
            avail * left_count as f32 / total_side_cols as f32
        } else {
            0.0
        };
        let right_avail = avail - left_avail;

        // Within each side, split between order and trade columns
        let (bid_ord_col_w, sell_trade_col_w) = if left_count == 2 {
            let total = ORDER_COL_FRACTION + TRADE_COL_FRACTION;
            (left_avail * ORDER_COL_FRACTION / total, left_avail * TRADE_COL_FRACTION / total)
        } else if self.column_config.show_bid_orders {
            (left_avail, 0.0_f32)
        } else if self.column_config.show_sell_trades {
            (0.0_f32, left_avail)
        } else {
            (0.0_f32, 0.0_f32)
        };

        let (ask_ord_col_w, buy_trade_col_w) = if right_count == 2 {
            let total = ORDER_COL_FRACTION + TRADE_COL_FRACTION;
            (right_avail * ORDER_COL_FRACTION / total, right_avail * TRADE_COL_FRACTION / total)
        } else if self.column_config.show_ask_orders {
            (right_avail, 0.0_f32)
        } else if self.column_config.show_buy_trades {
            (0.0_f32, right_avail)
        } else {
            (0.0_f32, 0.0_f32)
        };

        // X positions
        let mut cur_x = x + DOM_LEFT_PAD;
        let bid_ord_col_x = cur_x;
        if self.column_config.show_bid_orders { cur_x += bid_ord_col_w + pad; }

        let sell_trade_col_x = cur_x;
        if self.column_config.show_sell_trades { cur_x += sell_trade_col_w + pad; }

        let price_col_x = cur_x;
        cur_x = price_col_x + price_col_w + pad;

        let buy_trade_col_x = cur_x;
        if self.column_config.show_buy_trades { cur_x += buy_trade_col_w + pad; }

        let ask_ord_col_x = cur_x;

        // Column labels
        ctx.set_font("9px monospace");
        ctx.set_text_baseline(TextBaseline::Middle);
        let label_y = (col_header_y + DOM_COL_HEADER_HEIGHT / 2.0) as f64;
        ctx.set_fill_color(&theme.text_muted);

        if self.column_config.show_bid_orders {
            ctx.set_text_align(TextAlign::Center);
            ctx.fill_text("BID", (bid_ord_col_x + bid_ord_col_w / 2.0) as f64, label_y);
        }
        if self.column_config.show_sell_trades {
            ctx.set_text_align(TextAlign::Center);
            ctx.fill_text("SELL", (sell_trade_col_x + sell_trade_col_w / 2.0) as f64, label_y);
        }
        ctx.set_text_align(TextAlign::Center);
        ctx.fill_text("PRICE", (price_col_x + price_col_w / 2.0) as f64, label_y);
        if self.column_config.show_buy_trades {
            ctx.set_text_align(TextAlign::Center);
            ctx.fill_text("BUY", (buy_trade_col_x + buy_trade_col_w / 2.0) as f64, label_y);
        }
        if self.column_config.show_ask_orders {
            ctx.set_text_align(TextAlign::Center);
            ctx.fill_text("ASK", (ask_ord_col_x + ask_ord_col_w / 2.0) as f64, label_y);
        }

        // Separator line below header
        ctx.set_fill_color(&theme.text_muted);
        ctx.fill_rect(x as f64, (col_header_y + DOM_COL_HEADER_HEIGHT - 1.0) as f64, w as f64, 1.0);

        // === STEP 3: Best bid/ask bucket prices ===
        let best_bid_bucket_price = self.best_bid_price.map(|p| {
            let tick = (p / self.tick_size).floor() as i64;
            self.tick_to_price(tick)
        });
        let best_ask_bucket_price = self.best_ask_price.map(|p| {
            let tick = (p / self.tick_size).ceil() as i64;
            self.tick_to_price(tick)
        });
        let best_bid_price = best_bid_bucket_price;
        let best_ask_price = best_ask_bucket_price;

        let mid_as_row = match (best_bid_price, best_ask_price) {
            (Some(bb), Some(ba)) => (ba - bb) > self.tick_size * 1.5,
            _ => true,
        };

        // === STEP 4: Render each price level row ===
        for (i, level) in levels.iter().enumerate() {
            let row_y = body_y + (i as f32 * row_height);
            let price_tick = self.price_to_tick(level.price);

            // --- Step 4.1: Row background ---
            let is_best_bid = best_bid_price.map_or(false, |p| (level.price - p).abs() < 0.001);
            let is_best_ask = best_ask_price.map_or(false, |p| (level.price - p).abs() < 0.001);
            let is_current_price = (level.price - self.market_price).abs() < self.tick_size * 0.5;

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

            if is_current_price && mid_as_row {
                let cp_bg = if theme.current_price.len() >= 7 {
                    format!("{}50", &theme.current_price[..7])
                } else {
                    theme.current_price.clone()
                };
                ctx.set_fill_color(&cp_bg);
                ctx.fill_rect(x as f64, row_y as f64, w as f64, row_height as f64);
            }
            if !mid_as_row && is_best_ask {
                ctx.set_fill_color(&theme.current_price);
                ctx.fill_rect(x as f64, (row_y + row_height) as f64, w as f64, 1.0);
            }

            // --- Hover highlight overlay ---
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

            // --- Crosshair highlight ---
            let is_crosshair = self.crosshair_price
                .map_or(false, |cp| (level.price - cp).abs() < self.tick_size * 0.5);

            if is_crosshair && !is_hovered {
                ctx.set_fill_color("#ffffff26");
                ctx.fill_rect(x as f64, row_y as f64, w as f64, row_height as f64);
            }

            // --- Step 4.2: Bid order volume bar (right-aligned, grows leftward) ---
            if self.column_config.show_bid_orders && level.bid_volume > 0.0 && !is_filtered {
                let bar_width = if visible_max_volume == 0.0 {
                    0.0_f32
                } else {
                    (level.bid_volume / visible_max_volume * bid_ord_col_w as f64) as f32
                };
                let bar_x = bid_ord_col_x + bid_ord_col_w - bar_width;
                let bar_y = row_y + 2.0;
                let bar_h = row_height - 4.0;

                let bid_bar_color = if theme.buy.len() >= 7 {
                    format!("{}33", &theme.buy[..7])
                } else {
                    theme.buy.clone()
                };
                ctx.set_fill_color(&bid_bar_color);
                ctx.fill_rect(bar_x as f64, bar_y as f64, bar_width as f64, bar_h as f64);

                ctx.set_font("10px monospace");
                ctx.set_text_align(TextAlign::Right);
                ctx.set_text_baseline(TextBaseline::Middle);

                let text_x = bid_ord_col_x + bid_ord_col_w - 4.0;
                let text_y = row_y + row_height / 2.0;
                let vol_text = abbreviate_number(level.bid_volume, self.volume_decimals());
                let text_w = ctx.measure_text(&vol_text);
                let bid_text_color = if bar_width as f64 >= text_w {
                    &theme.text_primary
                } else {
                    &theme.buy
                };
                ctx.set_fill_color(bid_text_color);
                ctx.fill_text(&vol_text, text_x as f64, text_y as f64);
            }

            // --- Step 4.3: Sell trade bar (right-aligned within sell trade col) ---
            if let Some(&(buy_qty, sell_qty)) = self.trade_volume_by_price.get(&price_tick) {
                let text_y = row_y + row_height / 2.0;

                if self.column_config.show_sell_trades && sell_qty > 0.0 && visible_max_trade > 0.0 {
                    let bar_width = (sell_qty / visible_max_trade * sell_trade_col_w as f64) as f32;
                    let bar_x = sell_trade_col_x + sell_trade_col_w - bar_width;
                    let bar_y = row_y + 2.0;
                    let bar_h = row_height - 4.0;

                    // Sell color at 30% opacity (0x4C ≈ 0.30 * 255)
                    let sell_bar_color = if theme.sell.len() >= 7 {
                        format!("{}4C", &theme.sell[..7])
                    } else {
                        theme.sell.clone()
                    };
                    ctx.set_fill_color(&sell_bar_color);
                    ctx.fill_rect(bar_x as f64, bar_y as f64, bar_width as f64, bar_h as f64);

                    if bar_width > 12.0 {
                        ctx.set_font("9px monospace");
                        ctx.set_text_align(TextAlign::Right);
                        ctx.set_text_baseline(TextBaseline::Middle);
                        ctx.set_fill_color(&theme.sell);
                        let t = abbreviate_number(sell_qty, self.volume_decimals());
                        ctx.fill_text(&t, (sell_trade_col_x + sell_trade_col_w - 2.0) as f64, text_y as f64);
                    }
                }

                // --- Step 4.4: Buy trade bar (left-aligned within buy trade col) ---
                if self.column_config.show_buy_trades && buy_qty > 0.0 && visible_max_trade > 0.0 {
                    let bar_width = (buy_qty / visible_max_trade * buy_trade_col_w as f64) as f32;
                    let bar_x = buy_trade_col_x;
                    let bar_y = row_y + 2.0;
                    let bar_h = row_height - 4.0;

                    // Buy color at 30% opacity
                    let buy_bar_color = if theme.buy.len() >= 7 {
                        format!("{}4C", &theme.buy[..7])
                    } else {
                        theme.buy.clone()
                    };
                    ctx.set_fill_color(&buy_bar_color);
                    ctx.fill_rect(bar_x as f64, bar_y as f64, bar_width as f64, bar_h as f64);

                    if bar_width > 12.0 {
                        ctx.set_font("9px monospace");
                        ctx.set_text_align(TextAlign::Left);
                        ctx.set_text_baseline(TextBaseline::Middle);
                        ctx.set_fill_color(&theme.buy);
                        let t = abbreviate_number(buy_qty, self.volume_decimals());
                        ctx.fill_text(&t, (buy_trade_col_x + 2.0) as f64, text_y as f64);
                    }
                }
            }

            // --- Step 4.5: Ask order volume bar (left-aligned, grows rightward) ---
            if self.column_config.show_ask_orders && level.ask_volume > 0.0 && !is_filtered {
                let bar_width = if visible_max_volume == 0.0 {
                    0.0_f32
                } else {
                    (level.ask_volume / visible_max_volume * ask_ord_col_w as f64) as f32
                };
                let bar_x = ask_ord_col_x;
                let bar_y = row_y + 2.0;
                let bar_h = row_height - 4.0;

                let ask_bar_color = if theme.sell.len() >= 7 {
                    format!("{}33", &theme.sell[..7])
                } else {
                    theme.sell.clone()
                };
                ctx.set_fill_color(&ask_bar_color);
                ctx.fill_rect(bar_x as f64, bar_y as f64, bar_width as f64, bar_h as f64);

                ctx.set_font("10px monospace");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);

                let text_x = ask_ord_col_x + 4.0;
                let text_y = row_y + row_height / 2.0;
                let vol_text = abbreviate_number(level.ask_volume, self.volume_decimals());
                let text_w = ctx.measure_text(&vol_text);
                let ask_text_color = if bar_width as f64 >= text_w {
                    &theme.text_primary
                } else {
                    &theme.sell
                };
                ctx.set_fill_color(ask_text_color);
                ctx.fill_text(&vol_text, text_x as f64, text_y as f64);
            }

            // --- Step 4.6: Price text (centered in price column) ---
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

            // Spread row improvement: show "Spread: X.XX" when spread > 1.5 * tick_size
            if level.is_spread && mid_as_row {
                let sp = self.spread();
                if sp > self.tick_size * 1.5 {
                    let dec = self.price_decimals();
                    let spread_text = format!("Spread: {:.*}", dec, sp);
                    ctx.set_fill_color("#ffff0099");
                    ctx.set_font("9px monospace");
                    ctx.set_text_align(TextAlign::Center);
                    ctx.fill_text(&spread_text, price_x as f64, price_y as f64);
                    // Skip normal price text on spread rows with the label
                    // User order marker still applies below
                } else {
                    let price_text = format!("{:.*}", self.price_decimals(), level.price);
                    ctx.fill_text(&price_text, price_x as f64, price_y as f64);
                }
            } else {
                let price_text = format!("{:.*}", self.price_decimals(), level.price);
                ctx.fill_text(&price_text, price_x as f64, price_y as f64);
            }

            // --- Step 4.7: User order markers ---
            if level.has_user_order {
                ctx.set_fill_color(&theme.dom_user_order);
                ctx.set_font("10px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("▲", bid_ord_col_x as f64, price_y as f64);
            }

            // Per-row hover registrations removed — DOM is now a BlackboxPanel.
            // Hover highlight deferred to follow-up (needs handle_blackbox_event on TradingPanel).
        }

        // === STEP 5: Chase Tracker indicator on the spread row ===
        // Draw on the row closest to the mid-price
        let chase_bid = self.chase_bid_streak;
        let chase_ask = self.chase_ask_streak;

        if chase_bid > 0 || chase_ask > 0 {
            // Find spread/mid row index
            let spread_row_idx = levels.iter().position(|l| l.is_spread || {
                (l.price - self.market_price).abs() < self.tick_size * 0.5
            });

            if let Some(row_idx) = spread_row_idx {
                let row_y = body_y + (row_idx as f32 * row_height);
                let indicator_y = row_y + row_height / 2.0;

                // Draw indicator centered above the price column
                let center_x = price_col_x + price_col_w / 2.0;

                if chase_bid > 0 {
                    let streak = chase_bid;
                    let alpha = (1.0 - 1.0 / (1.0 + streak as f64)).clamp(0.15, 0.9);
                    let alpha_hex = (alpha * 255.0) as u8;
                    let indicator_color = if theme.buy.len() >= 7 {
                        format!("{}{:02X}", &theme.buy[..7], alpha_hex)
                    } else {
                        theme.buy.clone()
                    };
                    ctx.set_fill_color(&indicator_color);
                    ctx.set_font("11px sans-serif");
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    // Arrow + count
                    let label = format!("▲{}", streak);
                    ctx.fill_text(&label, (center_x - 8.0) as f64, indicator_y as f64);
                } else if chase_ask > 0 {
                    let streak = chase_ask;
                    let alpha = (1.0 - 1.0 / (1.0 + streak as f64)).clamp(0.15, 0.9);
                    let alpha_hex = (alpha * 255.0) as u8;
                    let indicator_color = if theme.sell.len() >= 7 {
                        format!("{}{:02X}", &theme.sell[..7], alpha_hex)
                    } else {
                        theme.sell.clone()
                    };
                    ctx.set_fill_color(&indicator_color);
                    ctx.set_font("11px sans-serif");
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    let label = format!("▼{}", streak);
                    ctx.fill_text(&label, (center_x + 8.0) as f64, indicator_y as f64);
                }
            }
        }

        // === STEP 6: Flash animation for recent fills ===
        let now = std::time::Instant::now();
        for (price_tick, (_volume, timestamp)) in &self.recent_fills {
            let elapsed_ms = now.duration_since(*timestamp).as_millis() as u64;
            if elapsed_ms < 300 {
                let price = self.tick_to_price(*price_tick);
                if let Some(row_idx) = levels.iter().position(|l| (l.price - price).abs() < 0.001) {
                    let flash_y = body_y + (row_idx as f32 * row_height);
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

    fn handle_hover(&mut self, _local_id: &str) -> bool {
        // Per-row hover removed (DOM BlackboxPanel migration deferred).
        // hovered_price is not updated until handle_blackbox_event is available.
        false
    }
}
