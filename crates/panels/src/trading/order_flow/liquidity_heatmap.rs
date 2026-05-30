use serde::{Serialize, Deserialize};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, RwLock};

use orderbook_service::{OrderbookSeries, OrderbookView, ArcSwap};
use trade_service::TradeSeries;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

/// LiquidityHeatmap panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LiquidityHeatmapId(pub u64);

/// LiquidityHeatmap panel state (heavy data)
#[derive(Clone)]
pub struct LiquidityHeatmapState {
    pub symbol: String,

    /// Exchange identifier string (e.g. "binance")
    pub exchange: String,

    /// Account type short label (e.g. "S", "F", "FI")
    pub account_type: String,

    /// Time range displayed (scrollable)
    pub start_time: i64,
    pub end_time: i64,

    /// Heatmap data: Vec of snapshots for efficient time slicing
    pub snapshots: Vec<LiquiditySnapshot>,

    /// Snapshot interval (ms) - how often we sample order book
    pub snapshot_interval_ms: u64,

    /// Tick size for price grid
    pub tick_size: f64,

    /// Current viewport scroll
    pub scroll_x: f32,
    pub scroll_y: f32,

    /// Max depth across all snapshots (for color scaling)
    pub max_depth: f64,

    /// Heatmap side (bid or ask or both)
    pub side: HeatmapSide,

    /// Timestamp (ms) of last sampled snapshot — for rate-limiting
    pub last_snapshot_ms: i64,
    /// Maximum snapshots to retain (rolling window)
    pub max_snapshots: usize,

    /// Crosshair price synced from a linked chart window.
    pub crosshair_price: Option<f64>,

    /// Shared orderbook series (written by the bridge, read here each tick).
    pub shared_orderbook: Option<Arc<RwLock<OrderbookSeries>>>,

    /// Lock-free view handle — cloned from `series.published` once on first tick.
    ob_view: Option<Arc<ArcSwap<OrderbookView>>>,

    /// Raw pointer identity of the current `shared_orderbook` Arc.
    ob_series_ptr: usize,

    /// Last `OrderbookSnapshot::version` we consumed.
    pub last_seen_orderbook_version: u64,

    /// Shared trade series for circle overlay rendering.
    pub shared_trades: Option<Arc<RwLock<TradeSeries>>>,

    /// The `TradeSeries::version` we last consumed in `tick()`.
    pub last_seen_trade_version: u64,

    /// Recent trades stored for circle overlay (capped at 1000).
    pub trade_circles: Vec<TradeCircle>,

    /// Maximum trade quantity seen — used to scale circle radius.
    pub max_trade_qty: f64,

    /// Number of adjacent price ticks to coalesce into one bucket (1 = no coalescing).
    pub coalesce_ticks: u32,

    /// When true, new snapshots go to `pause_buffer` instead of `snapshots`.
    pub paused: bool,

    /// Buffer accumulating snapshots while paused (max 200).
    pub pause_buffer: VecDeque<LiquiditySnapshot>,

    /// Mid price used to determine bid/ask side for coloring.
    pub mid_price: Option<f64>,
}

impl std::fmt::Debug for LiquidityHeatmapState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiquidityHeatmapState")
            .field("symbol", &self.symbol)
            .field("exchange", &self.exchange)
            .field("snapshots_len", &self.snapshots.len())
            .field("trade_circles_len", &self.trade_circles.len())
            .field("max_trade_qty", &self.max_trade_qty)
            .field("paused", &self.paused)
            .field("pause_buffer_len", &self.pause_buffer.len())
            .field("coalesce_ticks", &self.coalesce_ticks)
            .field("has_shared_orderbook", &self.shared_orderbook.is_some())
            .field("has_shared_trades", &self.shared_trades.is_some())
            .finish()
    }
}

impl LiquidityHeatmapState {
    pub fn new(symbol: String, tick_size: f64, snapshot_interval_ms: u64) -> Self {
        Self {
            symbol,
            exchange: String::new(),
            account_type: String::new(),
            start_time: 0,
            end_time: 0,
            snapshots: Vec::new(),
            snapshot_interval_ms,
            tick_size,
            scroll_x: 0.0,
            scroll_y: 0.0,
            max_depth: 0.0,
            side: HeatmapSide::Both,
            last_snapshot_ms: 0,
            max_snapshots: 1000,
            crosshair_price: None,
            shared_orderbook: None,
            ob_view: None,
            ob_series_ptr: 0,
            last_seen_orderbook_version: 0,
            shared_trades: None,
            last_seen_trade_version: 0,
            trade_circles: Vec::new(),
            max_trade_qty: 0.0,
            coalesce_ticks: 1,
            paused: false,
            pause_buffer: VecDeque::new(),
            mid_price: None,
        }
    }

    /// Shift the time-axis scroll by `delta` pixels (positive = scroll right / older data).
    pub fn handle_scroll(&mut self, delta: f64) {
        let new_x = self.scroll_x + delta as f32;
        if new_x > self.scroll_x {
            // scrolling back in time — engage pause
            self.paused = true;
        }
        self.scroll_x = new_x.max(0.0);
    }

    /// Reset time scroll to the latest snapshot and flush the pause buffer.
    pub fn handle_double_click(&mut self) {
        self.scroll_x = 0.0;
        self.scroll_y = 0.0;
        self.paused = false;
        // Drain pause buffer into main snapshots
        while let Some(snap) = self.pause_buffer.pop_front() {
            self.snapshots.push(snap);
        }
        // Enforce rolling window after drain
        while self.snapshots.len() > self.max_snapshots {
            self.snapshots.remove(0);
        }
        self.rebuild_time_range();
    }

    /// Pan the viewport: `dx` scrolls the time axis, `dy` scrolls the price axis.
    pub fn handle_drag(&mut self, dx: f64, dy: f64) {
        let new_x = self.scroll_x - dx as f32;
        if new_x > self.scroll_x {
            self.paused = true;
        }
        self.scroll_x = new_x.max(0.0);
        self.scroll_y += dy as f32;
    }

    /// Handle a named key event.  Returns `true` if the key was consumed.
    pub fn handle_key(&mut self, _key: zengeld_chart::input::KeyCode) -> bool {
        false
    }

    fn rebuild_time_range(&mut self) {
        if let Some(first) = self.snapshots.first() {
            self.start_time = first.timestamp;
        }
        if let Some(last) = self.snapshots.last() {
            self.end_time = last.timestamp;
        }
    }

    /// Pull the latest snapshot from `shared_orderbook` and sample it into the heatmap.
    /// Also pulls new trades from `shared_trades` into the circle overlay buffer.
    pub fn tick(&mut self) {
        // --- Orderbook (lock-free via ArcSwap) ---
        // Clone the Arc handle so the borrow of self.shared_orderbook ends before
        // we need &mut self below.  load_full() then gives an owned Arc<OrderbookView>
        // that is independent of self.ob_view, removing the need to clone bids/asks.
        if let Some(ob_handle) = self.shared_orderbook.clone() {
            // Detect handle reassignment and acquire lock-free view Arc (cold path).
            let cur_ptr = Arc::as_ptr(&ob_handle) as usize;
            if cur_ptr != self.ob_series_ptr {
                self.ob_view = None;
                self.ob_series_ptr = cur_ptr;
            }
            if self.ob_view.is_none() {
                if let Ok(series) = ob_handle.read() {
                    self.ob_view = Some(series.subscribe_view());
                }
            }
            // load_full() — owned Arc, no borrow of self.ob_view retained.
            let view_opt = self.ob_view.as_ref().map(|va| va.load_full());
            if let Some(view) = view_opt {
                if view.version != self.last_seen_orderbook_version {
                    self.last_seen_orderbook_version = view.version;

                    let timestamp = view.last_rest_ts_ms;

                    // Compute mid price from best bid/ask
                    if let (Some(best_bid), Some(best_ask)) = (view.best_bid, view.best_ask) {
                        self.mid_price = Some((best_bid + best_ask) / 2.0);
                    }

                    // Pass slices directly — no Vec clone needed.
                    #[allow(deprecated)]
                    self.apply_snapshot(&view.bids, &view.asks, timestamp);
                }
            }
        }

        // --- Trades ---
        if let Some(ref trade_handle) = self.shared_trades {
            if let Ok(series) = trade_handle.read() {
                if series.version != self.last_seen_trade_version {
                    let new_count =
                        (series.version.saturating_sub(self.last_seen_trade_version)) as usize;
                    let len = series.trades.len();
                    let skip = if new_count < len { len - new_count } else { 0 };

                    const MAX_CIRCLES: usize = 1000;

                    for trade in series.trades.iter().skip(skip) {
                        let circle = TradeCircle {
                            timestamp: trade.timestamp_ms,
                            price: trade.price,
                            qty: trade.quantity,
                            // is_buyer_maker == 1 → seller initiated → red (sell)
                            // is_buyer_maker == 0 → buyer initiated → green (buy)
                            is_buy: trade.is_buyer_maker == 0,
                        };
                        if circle.qty > self.max_trade_qty {
                            self.max_trade_qty = circle.qty;
                        }
                        if self.trade_circles.len() >= MAX_CIRCLES {
                            self.trade_circles.remove(0);
                        }
                        self.trade_circles.push(circle);
                    }

                    self.last_seen_trade_version = series.version;
                }
            }
        }
    }

    /// Apply an orderbook snapshot — rate-limited by snapshot_interval_ms.
    /// Returns true if a snapshot was actually recorded.
    ///
    /// Deprecated — callers should use `tick()` to pull data from the shared
    /// `OrderbookSeries`. This method is still called internally by `tick()`.
    #[deprecated(note = "Use tick() to pull data from the shared OrderbookSeries instead")]
    pub fn apply_snapshot(&mut self, bids: &[(f64, f64)], asks: &[(f64, f64)], timestamp_ms: i64) -> bool {
        // Rate-limit: skip if too soon since last snapshot
        if timestamp_ms - self.last_snapshot_ms < self.snapshot_interval_ms as i64 {
            return false;
        }
        self.last_snapshot_ms = timestamp_ms;

        let coalesce = self.coalesce_ticks.max(1) as i64;

        // Build depth map using BTreeMap for ordered iteration.
        // Coalesce adjacent price ticks into buckets.
        let mut depth_by_price: BTreeMap<i64, (f64, f64)> = BTreeMap::new();
        for &(price, qty) in bids {
            if qty > 0.0 {
                let raw_tick = (price / self.tick_size).round() as i64;
                let bucket = raw_tick / coalesce * coalesce;
                let entry = depth_by_price.entry(bucket).or_insert((0.0, 0.0));
                entry.0 += qty;
            }
        }
        for &(price, qty) in asks {
            if qty > 0.0 {
                let raw_tick = (price / self.tick_size).round() as i64;
                let bucket = raw_tick / coalesce * coalesce;
                let entry = depth_by_price.entry(bucket).or_insert((0.0, 0.0));
                entry.1 += qty;
            }
        }

        // Update max_depth
        for &(bid_d, ask_d) in depth_by_price.values() {
            let total = match self.side {
                HeatmapSide::Bids => bid_d,
                HeatmapSide::Asks => ask_d,
                HeatmapSide::Both => bid_d + ask_d,
            };
            if total > self.max_depth {
                self.max_depth = total;
            }
        }

        let snapshot = LiquiditySnapshot {
            timestamp: timestamp_ms,
            depth_by_price,
        };

        if self.paused {
            // Buffer while paused
            self.pause_buffer.push_back(snapshot);
            const MAX_PAUSE_BUFFER: usize = 200;
            while self.pause_buffer.len() > MAX_PAUSE_BUFFER {
                self.pause_buffer.pop_front();
            }
        } else {
            self.snapshots.push(snapshot);
            // Enforce rolling window
            while self.snapshots.len() > self.max_snapshots {
                self.snapshots.remove(0);
            }
            self.rebuild_time_range();
        }

        true
    }

    /// Get intensity (0.0–1.0) for a specific cell in the heatmap
    pub fn cell_intensity(&self, time_idx: usize, price_tick: i64) -> f64 {
        if time_idx >= self.snapshots.len() {
            return 0.0;
        }

        let snapshot = &self.snapshots[time_idx];
        let depth = snapshot.depth_by_price.get(&price_tick).copied().unwrap_or((0.0, 0.0));

        let total_depth = match self.side {
            HeatmapSide::Bids => depth.0,
            HeatmapSide::Asks => depth.1,
            HeatmapSide::Both => depth.0 + depth.1,
        };

        if self.max_depth == 0.0 {
            0.0
        } else {
            (total_depth / self.max_depth).min(1.0)
        }
    }

    /// Semantic bid/ask color.
    ///
    /// - `price_tick` is compared against `mid_price` to determine side.
    /// - Bids (price < mid): green (#2ecc71), alpha 0.1–0.8.
    /// - Asks (price > mid): red  (#e74c3c), alpha 0.1–0.8.
    /// - `fade_factor` (0.3–1.0) applied for horizontal fade (older = dimmer).
    pub fn side_color(&self, price_tick: i64, intensity: f64, fade_factor: f32) -> [f32; 4] {
        let alpha = ((intensity as f32).sqrt().clamp(0.1, 0.8)) * fade_factor;

        let mid_tick = self.mid_price
            .map(|m| (m / self.tick_size).round() as i64);

        let is_bid = match mid_tick {
            Some(mid) => price_tick < mid,
            // Without mid price, fall back to 0 boundary
            None => price_tick < 0,
        };

        if is_bid {
            // #2ecc71 = (46, 204, 113)
            [46.0 / 255.0, 204.0 / 255.0, 113.0 / 255.0, alpha]
        } else {
            // #e74c3c = (231, 76, 60)
            [231.0 / 255.0, 76.0 / 255.0, 60.0 / 255.0, alpha]
        }
    }

    /// Convert intensity to heatmap color (legacy rainbow, kept for `depth_color`).
    fn intensity_to_color_rainbow(intensity: f64) -> [f32; 4] {
        let intensity = intensity.clamp(0.0, 1.0) as f32;

        if intensity < 0.25 {
            let t = intensity / 0.25;
            [0.0, t * 0.5, 1.0, 0.7]
        } else if intensity < 0.5 {
            let t = (intensity - 0.25) / 0.25;
            [0.0, 0.5 + t * 0.5, 1.0 - t, 0.7]
        } else if intensity < 0.75 {
            let t = (intensity - 0.5) / 0.25;
            [t, 1.0, 0.0, 0.7]
        } else {
            let t = (intensity - 0.75) / 0.25;
            [1.0, 1.0 - t, 0.0, 0.7]
        }
    }

    /// Get visible time range based on scroll position
    pub fn visible_time_range(&self) -> (usize, usize) {
        if self.snapshots.is_empty() {
            return (0, 0);
        }

        let start = (self.scroll_x / 100.0).floor().max(0.0) as usize;
        let end = (start + 100).min(self.snapshots.len());

        (start, end)
    }

    /// Get visible cells for rendering (time_idx, price_tick, color).
    /// Colors are side-aware with horizontal fade applied.
    pub fn visible_cells(&self, _width: f32, _height: f32) -> Vec<(usize, i64, [f32; 4])> {
        let (start_time, end_time) = self.visible_time_range();
        let visible_columns = (end_time - start_time).max(1);
        let mut cells = Vec::new();

        for (col, time_idx) in (start_time..end_time).enumerate() {
            // Horizontal fade: older columns (lower col index) are dimmer.
            let fade_factor = (col as f32 / visible_columns as f32).clamp(0.3, 1.0);

            if let Some(snapshot) = self.snapshots.get(time_idx) {
                for (&price_tick, _) in &snapshot.depth_by_price {
                    let intensity = self.cell_intensity(time_idx, price_tick);
                    let color = self.side_color(price_tick, intensity, fade_factor);
                    cells.push((time_idx, price_tick, color));
                }
            }
        }

        cells
    }

    /// Get color for depth value using the rainbow ramp (used for sidebar).
    pub fn depth_color(&self, depth: f64) -> [f32; 4] {
        let intensity = if self.max_depth == 0.0 {
            0.0
        } else {
            (depth / self.max_depth).min(1.0)
        };
        Self::intensity_to_color_rainbow(intensity)
    }

    /// Convert price tick to Y coordinate
    pub fn price_to_y(&self, price_tick: i64, height: f32) -> f32 {
        let (min_price, max_price) = self.snapshots.iter()
            .flat_map(|s| s.depth_by_price.keys())
            .fold((i64::MAX, i64::MIN), |(min, max), &tick| {
                (min.min(tick), max.max(tick))
            });

        if max_price == min_price {
            return height / 2.0;
        }

        let normalized = (price_tick - min_price) as f64 / (max_price - min_price) as f64;
        (height as f64 * (1.0 - normalized)) as f32
    }

    /// Convert time index to X coordinate
    pub fn time_to_x(&self, time_idx: usize, width: f32) -> f32 {
        let (start, end) = self.visible_time_range();
        let visible_range = (end - start) as f32;

        if visible_range == 0.0 {
            return 0.0;
        }

        let relative_idx = (time_idx as f32 - start as f32) / visible_range;
        relative_idx * width
    }
}

/// A single trade recorded for the heatmap circle overlay.
#[derive(Debug, Clone)]
pub struct TradeCircle {
    /// Exchange timestamp (ms).
    pub timestamp: i64,
    /// Trade price.
    pub price: f64,
    /// Trade quantity.
    pub qty: f64,
    /// `true` = buyer-initiated (green), `false` = seller-initiated (red).
    pub is_buy: bool,
}

#[derive(Debug, Clone)]
pub struct LiquiditySnapshot {
    pub timestamp: i64,
    /// Price tick -> (bid_depth, ask_depth), ordered for iteration.
    pub depth_by_price: BTreeMap<i64, (f64, f64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum HeatmapSide {
    Bids,
    Asks,
    Both,
}

impl TradingPanel for LiquidityHeatmapState {
    fn kind(&self) -> &'static str { "liquidity_heatmap" }
    fn label(&self) -> &'static str { "Liquidity Heatmap" }

    fn render(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        theme: &crate::panel_theme::PanelTheme,
        coordinator: &mut uzor::InputCoordinator,
        slot_prefix: &str,
    ) {
        let config = LiquidityHeatmapConfig::default();

        {
            let body_id = format!("{}:heatmap:body", slot_prefix);
            coordinator.register(
                body_id.as_str(),
                uzor::Rect::new(x as f64, y as f64, w as f64, h as f64),
                uzor::input::Sense::SCROLL | uzor::input::Sense::DRAG | uzor::input::Sense::DOUBLE_CLICK,
            );
        }

        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        let cells = self.visible_cells(w, h);

        for (time_idx, price_tick, color) in cells {
            let cell_x = x + self.time_to_x(time_idx, w);
            let cell_y = y + self.price_to_y(price_tick, h);

            let color_hex = format!(
                "#{:02x}{:02x}{:02x}{:02x}",
                (color[0].clamp(0.0, 1.0) * 255.0) as u8,
                (color[1].clamp(0.0, 1.0) * 255.0) as u8,
                (color[2].clamp(0.0, 1.0) * 255.0) as u8,
                (color[3].clamp(0.0, 1.0) * 255.0) as u8,
            );
            ctx.set_fill_color(&color_hex);
            ctx.fill_rect(
                cell_x as f64,
                cell_y as f64,
                config.cell_width as f64,
                config.cell_height as f64,
            );
        }

        // Compute real price range from visible snapshots
        let (start_snap, end_snap) = self.visible_time_range();
        let (price_min_tick, price_max_tick) = self.snapshots[start_snap..end_snap]
            .iter()
            .flat_map(|s| s.depth_by_price.keys().copied())
            .fold((i64::MAX, i64::MIN), |(mn, mx), t| (mn.min(t), mx.max(t)));

        let has_price_range = price_max_tick > price_min_tick;

        // Depth profile sidebar width (~15% of panel)
        let sidebar_w = w * 0.15;
        let heatmap_w = w - sidebar_w;

        // Helper: price tick → pixel y (local, uses real range)
        let tick_to_y = |tick: i64| -> f64 {
            if !has_price_range {
                return (h / 2.0) as f64;
            }
            let normalized =
                (tick - price_min_tick) as f64 / (price_max_tick - price_min_tick) as f64;
            // high price at top → invert
            h as f64 * (1.0 - normalized)
        };

        if config.show_current_book {
            if let Some(snapshot) = self.snapshots.last() {
                let max_qty = snapshot
                    .depth_by_price
                    .values()
                    .map(|&(bid, ask)| bid + ask)
                    .fold(0.0_f64, f64::max);

                if max_qty > 0.0 {
                    let cell_px_h = if has_price_range {
                        ((h as f64) / (price_max_tick - price_min_tick).max(1) as f64)
                            .max(1.0)
                    } else {
                        2.0
                    };

                    for (&tick, &(bid_qty, ask_qty)) in &snapshot.depth_by_price {
                        let bar_y = y as f64 + tick_to_y(tick) - cell_px_h / 2.0;

                        // Bids — green
                        if bid_qty > 0.0 {
                            let bar_w = sidebar_w as f64 * (bid_qty / max_qty) * 0.5;
                            ctx.set_fill_color("#2ecc7199");
                            ctx.fill_rect(
                                (x + heatmap_w) as f64,
                                bar_y,
                                bar_w,
                                cell_px_h,
                            );
                        }

                        // Asks — red
                        if ask_qty > 0.0 {
                            let bar_w = sidebar_w as f64 * (ask_qty / max_qty) * 0.5;
                            ctx.set_fill_color("#e74c3c99");
                            ctx.fill_rect(
                                (x + heatmap_w) as f64 + sidebar_w as f64 * 0.5,
                                bar_y,
                                bar_w,
                                cell_px_h,
                            );
                        }
                    }
                }
            }
        }

        // Trade circles overlay
        if !self.trade_circles.is_empty() && self.max_trade_qty > 0.0 {
            let time_span_ms = if self.end_time > self.start_time {
                (self.end_time - self.start_time) as f64
            } else {
                1.0
            };

            let ts_to_x = |ts: i64| -> f64 {
                let frac = (ts - self.start_time) as f64 / time_span_ms;
                x as f64 + frac * heatmap_w as f64
            };

            let price_to_y_f = |price: f64| -> f64 {
                if !has_price_range {
                    return (y as f64) + (h as f64) / 2.0;
                }
                let tick = (price / self.tick_size).round() as i64;
                y as f64 + tick_to_y(tick)
            };

            use std::f64::consts::TAU;

            for circle in &self.trade_circles {
                if circle.timestamp < self.start_time || circle.timestamp > self.end_time {
                    continue;
                }

                let cx = ts_to_x(circle.timestamp);
                let cy = price_to_y_f(circle.price);

                if cx < x as f64 || cx > (x + heatmap_w) as f64 {
                    continue;
                }
                if cy < y as f64 || cy > (y + h) as f64 {
                    continue;
                }

                let radius = 2.0 + (circle.qty / self.max_trade_qty).sqrt() * 8.0;

                let color = if circle.is_buy { "#2ecc7199" } else { "#e74c3c99" };
                ctx.set_fill_color(color);
                ctx.begin_path();
                ctx.arc(cx, cy, radius, 0.0, TAU);
                ctx.fill();
            }
        }

        // Crosshair: horizontal line + price label
        if let Some(crosshair_price) = self.crosshair_price {
            let crosshair_tick = (crosshair_price / self.tick_size).round() as i64;
            let cy = y as f64 + tick_to_y(crosshair_tick);

            ctx.set_fill_color("#ffffff66");
            ctx.fill_rect(x as f64, cy, heatmap_w as f64, 1.0);

            let decimal_places = if self.tick_size < 0.01 {
                4
            } else if self.tick_size < 1.0 {
                2
            } else {
                0
            };
            let label = format!("{:.prec$}", crosshair_price, prec = decimal_places);
            ctx.set_font("9px sans-serif");
            ctx.set_text_align(TextAlign::Right);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_fill_color("#ffffffcc");
            ctx.fill_text(&label, (x + heatmap_w - 2.0) as f64, cy);
        }

        // Price axis labels (right edge of heatmap area, real prices)
        ctx.set_font("9px sans-serif");
        ctx.set_text_align(TextAlign::Right);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&theme.text_muted);

        let decimal_places = if self.tick_size < 0.01 {
            4
        } else if self.tick_size < 1.0 {
            2
        } else {
            0
        };

        let num_labels = 10_usize;
        for i in 0..=num_labels {
            let frac = i as f64 / num_labels as f64;
            let label_y = y + (frac * h as f64) as f32;
            // frac 0 = top = price_max, frac 1 = bottom = price_min
            let price = if has_price_range {
                (price_max_tick as f64 - frac * (price_max_tick - price_min_tick) as f64)
                    * self.tick_size
            } else {
                price_min_tick as f64 * self.tick_size
            };
            let label_text = format!("{:.prec$}", price, prec = decimal_places);
            ctx.fill_text(&label_text, (x + heatmap_w - 4.0) as f64, label_y as f64);
        }

        // "PAUSED" badge at top-right when paused
        if self.paused {
            let badge_text = "PAUSED";
            let badge_x = x + w - 60.0;
            let badge_y = y + 6.0;

            ctx.set_fill_color("#00000099");
            ctx.fill_rect((badge_x - 4.0) as f64, badge_y as f64, 56.0, 14.0);

            ctx.set_font("bold 9px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.set_fill_color("#e74c3cff");
            ctx.fill_text(badge_text, badge_x as f64, badge_y as f64);
        }
    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool { false }

    fn handle_scroll(&mut self, local_id: &str, _dx: f64, dy: f64) -> bool {
        if local_id == "heatmap:body" {
            LiquidityHeatmapState::handle_scroll(self, dy * 3.0);
            true
        } else {
            false
        }
    }

    fn handle_drag_start(&mut self, local_id: &str, _x: f64, _y: f64) -> bool {
        local_id == "heatmap:body"
    }

    fn handle_drag_move(&mut self, local_id: &str, dx: f64, dy: f64) -> bool {
        if local_id == "heatmap:body" {
            LiquidityHeatmapState::handle_drag(self, dx, dy);
            true
        } else {
            false
        }
    }

    fn handle_drag_end(&mut self, _local_id: &str) -> bool {
        true
    }

    fn handle_double_click(&mut self, local_id: &str, _x: f64, _y: f64) -> bool {
        if local_id == "heatmap:body" {
            LiquidityHeatmapState::handle_double_click(self);
            true
        } else {
            false
        }
    }
}

/// LiquidityHeatmap panel configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiquidityHeatmapConfig {
    /// Snapshot sampling rate (ms)
    pub snapshot_interval_ms: u64,

    /// Max snapshots to keep in memory (rolling window)
    pub max_snapshots: usize,

    /// Heatmap side
    pub side: HeatmapSide,

    /// Cell size in pixels
    pub cell_width: f32,  // time axis
    pub cell_height: f32, // price axis

    /// Show current order book line
    pub show_current_book: bool,
}

impl Default for LiquidityHeatmapConfig {
    fn default() -> Self {
        Self {
            snapshot_interval_ms: 5000, // 5 seconds
            max_snapshots: 1000,
            side: HeatmapSide::Both,
            cell_width: 5.0,
            cell_height: 3.0,
            show_current_book: true,
        }
    }
}

/// LiquidityHeatmap panel wrapper (lightweight, lives in PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiquidityHeatmapPanel {
    id: LiquidityHeatmapId,
    title: String,
}

impl LiquidityHeatmapPanel {
    pub fn new(id: LiquidityHeatmapId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> LiquidityHeatmapId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "liquidity_heatmap" }
    pub fn kind_label(&self) -> &'static str { "Liquidity Heatmap" }
    pub fn min_size(&self) -> (f32, f32) { (100.0, 80.0) }
}
