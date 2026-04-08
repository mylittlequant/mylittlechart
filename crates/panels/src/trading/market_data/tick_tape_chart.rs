//! Tick Tape Chart: Horizontal scatter plot of individual trades.
//!
//! Each trade is rendered as a circle on a time×price grid.
//! Circle size represents trade volume, color represents buy/sell side.
//! This is NOT the TickerTape (CNBC-style horizontal scrolling ticker).

use std::collections::VecDeque;
use serde::{Serialize, Deserialize};

/// Tick Tape Chart panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TickTapeChartId(pub u64);

/// Display style for the tick chart
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TickTapeStyle {
    /// Circles connected by lines (default, like screenshot 2)
    PointsAndLines,
    /// Just circles
    PointsOnly,
    /// Just lines
    LinesOnly,
}

/// Viewport for the tick chart (time × price)
#[derive(Clone, Debug)]
pub struct TickTapeViewport {
    /// Visible time range in milliseconds
    pub time_start: i64,
    pub time_end: i64,
    /// Visible price range
    pub price_min: f64,
    pub price_max: f64,
    /// Auto-scroll to latest trades
    pub auto_scroll: bool,
    /// Zoom level (1.0 = default)
    pub zoom: f64,
}

impl TickTapeViewport {
    pub fn new() -> Self {
        Self {
            time_start: 0,
            time_end: 0,
            price_min: 0.0,
            price_max: 0.0,
            auto_scroll: true,
            zoom: 1.0,
        }
    }

    /// Convert timestamp to X pixel coordinate
    pub fn timestamp_to_x(&self, ts: i64, chart_width: f64) -> f64 {
        let range = (self.time_end - self.time_start) as f64;
        if range <= 0.0 { return chart_width / 2.0; }
        let relative = (ts - self.time_start) as f64;
        (relative / range) * chart_width
    }

    /// Convert price to Y pixel coordinate (inverted: higher price = lower Y)
    pub fn price_to_y(&self, price: f64, chart_height: f64) -> f64 {
        let range = self.price_max - self.price_min;
        if range <= 0.0 { return chart_height / 2.0; }
        chart_height * (1.0 - (price - self.price_min) / range)
    }

    /// Convert X pixel to timestamp
    pub fn x_to_timestamp(&self, x: f64, chart_width: f64) -> i64 {
        let range = (self.time_end - self.time_start) as f64;
        self.time_start + ((x / chart_width) * range) as i64
    }

    /// Convert Y pixel to price
    pub fn y_to_price(&self, y: f64, chart_height: f64) -> f64 {
        let range = self.price_max - self.price_min;
        self.price_max - (y / chart_height) * range
    }
}

/// A single trade point on the tick chart
#[derive(Clone, Debug)]
pub struct TickTradeDot {
    pub timestamp: i64,
    pub price: f64,
    pub quantity: f64,
    pub is_buy: bool,
}

/// Tick Tape Chart state (heavy data)
#[derive(Clone, Debug)]
pub struct TickTapeChartState {
    /// Symbol being monitored
    pub symbol: String,
    /// Ring buffer of trade dots
    pub dots: VecDeque<TickTradeDot>,
    /// Maximum dots to keep
    pub max_dots: usize,
    /// Viewport (time × price)
    pub viewport: TickTapeViewport,
    /// Display style
    pub style: TickTapeStyle,
    /// Aggregation period in ms (trades within this window are grouped)
    pub aggregation_ms: u64,
    /// Minimum trade size to show
    pub min_size_filter: f64,
    /// Line thickness for connecting lines
    pub line_thickness: f64,
    /// DOM market price (synced)
    pub dom_market_price: Option<f64>,
    /// DOM tick size (synced)
    pub dom_tick_size: Option<f64>,
    /// DOM center price (synced)
    pub dom_center_price: Option<f64>,
}

impl TickTapeChartState {
    pub fn new() -> Self {
        Self {
            symbol: String::new(),
            dots: VecDeque::new(),
            max_dots: 5000,
            viewport: TickTapeViewport::new(),
            style: TickTapeStyle::PointsAndLines,
            aggregation_ms: 100_000,
            min_size_filter: 0.0,
            line_thickness: 1.0,
            dom_market_price: None,
            dom_tick_size: None,
            dom_center_price: None,
        }
    }

    /// Add a trade dot
    pub fn push_dot(&mut self, dot: TickTradeDot) {
        if self.dots.len() >= self.max_dots {
            self.dots.pop_front();
        }
        self.dots.push_back(dot);
    }

    /// Get visible dots within current viewport
    pub fn visible_dots(&self) -> Vec<&TickTradeDot> {
        self.dots.iter()
            .filter(|d| {
                d.timestamp >= self.viewport.time_start
                    && d.timestamp <= self.viewport.time_end
                    && d.price >= self.viewport.price_min
                    && d.price <= self.viewport.price_max
                    && d.quantity >= self.min_size_filter
            })
            .collect()
    }

    /// Calculate dot radius based on quantity (logarithmic scale)
    pub fn dot_radius(&self, quantity: f64) -> f64 {
        (quantity.ln().max(0.0) * 2.0 + 2.0).clamp(2.0, 14.0)
    }

    /// Update viewport to fit all dots (auto-range)
    pub fn auto_fit_viewport(&mut self) {
        if self.dots.is_empty() { return; }

        let mut min_price = f64::MAX;
        let mut max_price = f64::MIN;
        let mut min_time = i64::MAX;
        let mut max_time = i64::MIN;

        for dot in &self.dots {
            min_price = min_price.min(dot.price);
            max_price = max_price.max(dot.price);
            min_time = min_time.min(dot.timestamp);
            max_time = max_time.max(dot.timestamp);
        }

        // Add 5% padding
        let price_pad = (max_price - min_price) * 0.05;
        let time_pad = ((max_time - min_time) as f64 * 0.05) as i64;

        self.viewport.price_min = min_price - price_pad;
        self.viewport.price_max = max_price + price_pad;
        self.viewport.time_start = min_time - time_pad;
        self.viewport.time_end = max_time + time_pad;
    }
}

/// Tick Tape Chart panel wrapper (lightweight, for PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TickTapeChartPanel {
    id: TickTapeChartId,
    title: String,
}

impl TickTapeChartPanel {
    pub fn new(id: TickTapeChartId, title: String) -> Self { Self { id, title } }
    pub fn id(&self) -> TickTapeChartId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }
    pub fn type_id(&self) -> &'static str { "tick_tape_chart" }
    pub fn kind_label(&self) -> &'static str { "Tick Tape" }
    pub fn min_size(&self) -> (f32, f32) { (300.0, 200.0) }
}
