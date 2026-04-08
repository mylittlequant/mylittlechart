use serde::{Serialize, Deserialize};
use std::time::Instant;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DepthChartId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct DepthChartState {
    pub symbol: String,
    pub bids: Vec<(f64, f64)>,  // (price, cumulative_size)
    pub asks: Vec<(f64, f64)>,  // (price, cumulative_size)
    pub mid_price: f64,
    pub max_depth: f64,
    pub last_update: Option<Instant>,
    pub animation_progress: f32,  // 0.0..1.0 for AreaFill animation
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DepthChartConfig {
    pub bid_color: [f32; 3],      // OKLCH: [0.65, 0.15, 145.0] (green)
    pub ask_color: [f32; 3],      // OKLCH: [0.60, 0.18, 25.0] (red)
    pub grid_color: [f32; 4],     // OKLCH with alpha
    pub depth_levels: usize,      // Number of levels to show (e.g., 20)
    pub auto_scale: bool,
    pub show_spread: bool,
    pub animation_duration_ms: u64,
}

impl DepthChartState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns bid curve points in screen coordinates as a cumulative curve
    pub fn bid_curve_points(&self, w: f32, h: f32) -> Vec<(f32, f32)> {
        if self.bids.is_empty() || self.max_depth == 0.0 {
            return Vec::new();
        }

        let price_range = self.mid_price * 0.1;
        let min_price = self.mid_price - price_range;
        let max_price = self.mid_price;

        self.bids.iter()
            .filter_map(|(price, cum_size)| {
                if *price < min_price || *price > max_price {
                    return None;
                }
                let x = ((price - min_price) / (max_price - min_price)) as f32 * (w / 2.0);
                let y = h - ((*cum_size / self.max_depth) as f32 * h);
                Some((x, y))
            })
            .collect()
    }

    /// Returns ask curve points in screen coordinates as a cumulative curve
    pub fn ask_curve_points(&self, w: f32, h: f32) -> Vec<(f32, f32)> {
        if self.asks.is_empty() || self.max_depth == 0.0 {
            return Vec::new();
        }

        let price_range = self.mid_price * 0.1;
        let min_price = self.mid_price;
        let max_price = self.mid_price + price_range;

        self.asks.iter()
            .filter_map(|(price, cum_size)| {
                if *price < min_price || *price > max_price {
                    return None;
                }
                let x = (w / 2.0) + (((price - min_price) / (max_price - min_price)) as f32 * (w / 2.0));
                let y = h - ((*cum_size / self.max_depth) as f32 * h);
                Some((x, y))
            })
            .collect()
    }

    /// Returns the mid price value
    pub fn mid_price(&self) -> f64 {
        self.mid_price
    }

    /// Returns the maximum depth volume
    pub fn max_depth_volume(&self) -> f64 {
        self.max_depth
    }

    /// Returns the x coordinate for the mid price line
    pub fn mid_price_x(&self, w: f32) -> f32 {
        w / 2.0
    }
}

impl Default for DepthChartConfig {
    fn default() -> Self {
        Self {
            bid_color: [0.65, 0.15, 145.0],
            ask_color: [0.60, 0.18, 25.0],
            grid_color: [0.5, 0.0, 0.0, 0.2],
            depth_levels: 20,
            auto_scale: true,
            show_spread: true,
            animation_duration_ms: 300,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DepthChartPanel {
    id: DepthChartId,
    title: String,
}

impl DepthChartPanel {
    pub fn new(id: DepthChartId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> DepthChartId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "depthchart"
    }

    pub fn kind_label(&self) -> &'static str {
        "Depth Chart"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (200.0, 150.0)
    }
}
