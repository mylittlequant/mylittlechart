use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use orderbook_service::OrderbookSeries;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

/// LiquidityHeatmap panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LiquidityHeatmapId(pub u64);

/// LiquidityHeatmap panel state (heavy data)
#[derive(Clone, Debug)]
pub struct LiquidityHeatmapState {
    pub symbol: String,

    /// Exchange identifier string (e.g. "binance")
    pub exchange: String,

    /// Account type short label (e.g. "S", "F", "FI")
    pub account_type: String,

    /// Time range displayed (scrollable)
    pub start_time: i64,
    pub end_time: i64,

    /// Heatmap data: (timestamp, price) -> depth
    /// Stored as Vec of snapshots for efficient time slicing
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
    /// When set, a subtle highlight line is drawn at the corresponding price row.
    pub crosshair_price: Option<f64>,

    /// Shared orderbook series (written by the bridge, read here each tick).
    pub shared_orderbook: Option<Arc<RwLock<OrderbookSeries>>>,

    /// Last `OrderbookSnapshot::version` we consumed. When `series.current.version`
    /// differs we pull a fresh snapshot and sample it into the heatmap.
    pub last_seen_orderbook_version: u64,
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
            last_seen_orderbook_version: 0,
        }
    }

    /// Shift the time-axis scroll by `delta` pixels (positive = scroll right / older data).
    pub fn handle_scroll(&mut self, delta: f64) {
        self.scroll_x = (self.scroll_x + delta as f32).max(0.0);
    }

    /// Pull the latest snapshot from `shared_orderbook` and sample it into the
    /// heatmap (rate-limited by `snapshot_interval_ms`).
    ///
    /// Returns immediately when there is no shared handle or the version has not
    /// advanced since the last call.
    pub fn tick(&mut self) {
        let Some(ref ob_handle) = self.shared_orderbook else { return };
        let Ok(series) = ob_handle.read() else { return };
        if series.current.version == self.last_seen_orderbook_version {
            return; // nothing new
        }
        self.last_seen_orderbook_version = series.current.version;

        let timestamp = series.current.last_rest_ts_ms;
        let bids: Vec<(f64, f64)> = series.current.bids.iter().map(|(k, &v)| (k.0, v)).collect();
        let asks: Vec<(f64, f64)> = series.current.asks.iter().map(|(k, &v)| (k.0, v)).collect();
        drop(series);

        // apply_snapshot is rate-limited internally — only samples if enough time passed.
        #[allow(deprecated)]
        self.apply_snapshot(&bids, &asks, timestamp);
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

        // Build depth map
        let mut depth_by_price: HashMap<i64, (f64, f64)> = HashMap::new();
        for &(price, qty) in bids {
            if qty > 0.0 {
                let tick = (price / self.tick_size).round() as i64;
                let entry = depth_by_price.entry(tick).or_insert((0.0, 0.0));
                entry.0 += qty;
            }
        }
        for &(price, qty) in asks {
            if qty > 0.0 {
                let tick = (price / self.tick_size).round() as i64;
                let entry = depth_by_price.entry(tick).or_insert((0.0, 0.0));
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

        self.snapshots.push(snapshot);

        // Enforce rolling window
        while self.snapshots.len() > self.max_snapshots {
            self.snapshots.remove(0);
        }

        // Update time range
        if let Some(first) = self.snapshots.first() {
            self.start_time = first.timestamp;
        }
        if let Some(last) = self.snapshots.last() {
            self.end_time = last.timestamp;
        }

        true
    }

    /// Get intensity (0.0-1.0) for a specific cell in the heatmap
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

    /// Convert intensity to heatmap color (blue -> green -> yellow -> red)
    pub fn intensity_to_color(&self, intensity: f64) -> [f32; 4] {
        let intensity = intensity.clamp(0.0, 1.0) as f32;

        if intensity < 0.25 {
            // Blue to cyan
            let t = intensity / 0.25;
            [0.0, t * 0.5, 1.0, 0.7]
        } else if intensity < 0.5 {
            // Cyan to green
            let t = (intensity - 0.25) / 0.25;
            [0.0, 0.5 + t * 0.5, 1.0 - t, 0.7]
        } else if intensity < 0.75 {
            // Green to yellow
            let t = (intensity - 0.5) / 0.25;
            [t, 1.0, 0.0, 0.7]
        } else {
            // Yellow to red
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

    /// Get visible cells for rendering (time_idx, price_tick, color)
    pub fn visible_cells(&self, _width: f32, _height: f32) -> Vec<(usize, i64, [f32; 4])> {
        let (start_time, end_time) = self.visible_time_range();
        let mut cells = Vec::new();

        for time_idx in start_time..end_time {
            if let Some(snapshot) = self.snapshots.get(time_idx) {
                for (price_tick, _) in &snapshot.depth_by_price {
                    let intensity = self.cell_intensity(time_idx, *price_tick);
                    let color = self.intensity_to_color(intensity);
                    cells.push((time_idx, *price_tick, color));
                }
            }
        }

        cells
    }

    /// Get color for depth value (alias for intensity_to_color)
    pub fn depth_color(&self, depth: f64) -> [f32; 4] {
        let intensity = if self.max_depth == 0.0 {
            0.0
        } else {
            (depth / self.max_depth).min(1.0)
        };
        self.intensity_to_color(intensity)
    }

    /// Convert price tick to Y coordinate
    pub fn price_to_y(&self, price_tick: i64, height: f32) -> f32 {
        // Find min/max price in current snapshots
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

#[derive(Debug, Clone)]
pub struct LiquiditySnapshot {
    pub timestamp: i64,
    /// Price -> (bid_depth, ask_depth)
    pub depth_by_price: HashMap<i64, (f64, f64)>, // Using i64 for price ticks
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
    ) {
        let config = LiquidityHeatmapConfig::default();

        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        let cells = self.visible_cells(w, h);

        for (time_idx, price_tick, color) in cells {
            let cell_x = x + self.time_to_x(time_idx, w);
            let cell_y = y + self.price_to_y(price_tick, h);

            // color comes from intensity_to_color — convert f32 rgba to hex
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
                // Find max quantity across all price levels for normalisation
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
    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool { false }
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
    pub fn min_size(&self) -> (f32, f32) { (300.0, 200.0) }
}
