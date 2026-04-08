use serde::{Serialize, Deserialize};

/// MarketDepthGraph panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MarketDepthGraphId(pub u64);

/// MarketDepthGraph panel state (heavy data)
#[derive(Clone, Debug)]
pub struct MarketDepthGraphState {
    pub symbol: String,

    /// Cumulative depth data for rendering
    /// Vec<(price, cumulative_volume)>
    pub bid_depth_curve: Vec<(f64, f64)>,
    pub ask_depth_curve: Vec<(f64, f64)>,

    /// Price range to display (auto or manual)
    pub price_range: (f64, f64), // (min_price, max_price)

    /// Max cumulative volume (for Y-axis scaling)
    pub max_cumulative_volume: f64,

    /// Mid price (current market price)
    pub mid_price: f64,

    /// Depth levels to display (e.g., top 50 levels each side)
    pub depth_levels: usize,

    /// Spread (ask - bid)
    pub spread: f64,
}

impl MarketDepthGraphState {
    pub fn new(symbol: String) -> Self {
        Self {
            symbol,
            bid_depth_curve: Vec::new(),
            ask_depth_curve: Vec::new(),
            price_range: (0.0, 0.0),
            max_cumulative_volume: 0.0,
            mid_price: 0.0,
            depth_levels: 50,
            spread: 0.0,
        }
    }

    /// Convert bid depth data to screen coordinates for rendering
    pub fn bid_curve_points(&self, width: f32, height: f32) -> Vec<(f32, f32)> {
        self.bid_depth_curve
            .iter()
            .map(|(price, volume)| {
                let x = self.price_to_x(*price, width);
                let y = self.volume_to_y(*volume, height);
                (x, y)
            })
            .collect()
    }

    /// Convert ask depth data to screen coordinates for rendering
    pub fn ask_curve_points(&self, width: f32, height: f32) -> Vec<(f32, f32)> {
        self.ask_depth_curve
            .iter()
            .map(|(price, volume)| {
                let x = self.price_to_x(*price, width);
                let y = self.volume_to_y(*volume, height);
                (x, y)
            })
            .collect()
    }

    /// Convert price to X coordinate
    pub fn price_to_x(&self, price: f64, width: f32) -> f32 {
        let (min_price, max_price) = self.price_range;
        if max_price == min_price {
            return width / 2.0;
        }

        let normalized = (price - min_price) / (max_price - min_price);
        (normalized * width as f64) as f32
    }

    /// Convert volume to Y coordinate (inverted: 0 at top)
    pub fn volume_to_y(&self, volume: f64, height: f32) -> f32 {
        if self.max_cumulative_volume == 0.0 {
            return height;
        }

        let normalized = volume / self.max_cumulative_volume;
        (height as f64 * (1.0 - normalized)) as f32
    }

    /// Get bid points for rendering
    pub fn bid_points(&self) -> &[(f64, f64)] {
        &self.bid_depth_curve
    }

    /// Get ask points for rendering
    pub fn ask_points(&self) -> &[(f64, f64)] {
        &self.ask_depth_curve
    }

    /// Get current mid price
    pub fn mid_price(&self) -> f64 {
        self.mid_price
    }

    /// Get max depth across all levels
    pub fn max_depth(&self) -> f64 {
        self.max_cumulative_volume
    }
}

/// MarketDepthGraph panel configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketDepthGraphConfig {
    /// Number of levels to include (default: 50 each side)
    pub depth_levels: usize,

    /// Auto-range vs fixed price range
    pub auto_range: bool,
    pub fixed_range_percent: f64, // e.g., 1.0 for ±1% around mid price

    /// Fill opacity
    pub area_opacity: f32,

    /// Show grid lines
    pub show_grid: bool,

    /// Smooth curve vs stepped
    pub smooth_curve: bool,
}

impl Default for MarketDepthGraphConfig {
    fn default() -> Self {
        Self {
            depth_levels: 50,
            auto_range: true,
            fixed_range_percent: 1.0,
            area_opacity: 0.3,
            show_grid: true,
            smooth_curve: true,
        }
    }
}

/// MarketDepthGraph panel wrapper (lightweight, lives in PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketDepthGraphPanel {
    id: MarketDepthGraphId,
    title: String,
}

impl MarketDepthGraphPanel {
    pub fn new(id: MarketDepthGraphId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> MarketDepthGraphId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "market_depth_graph" }
    pub fn kind_label(&self) -> &'static str { "Market Depth" }
    pub fn min_size(&self) -> (f32, f32) { (300.0, 200.0) }
}
