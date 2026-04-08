use serde::{Serialize, Deserialize};
use std::collections::VecDeque;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DomSurfaceId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct DomSurfaceState {
    pub symbol: String,
    pub surface_data: VecDeque<DomSnapshot>,  // Time series of DOM snapshots
    pub time_window_seconds: u64,
    pub price_range: (f64, f64),
    pub max_depth: f64,
    pub rotation: (f32, f32),  // (pitch, yaw)
    pub zoom: f32,
}

#[derive(Clone, Debug)]
pub struct DomSnapshot {
    pub timestamp: i64,  // Unix timestamp
    pub bids: Vec<(f64, f64)>,  // (price, depth)
    pub asks: Vec<(f64, f64)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DomSurfaceConfig {
    pub snapshot_interval_ms: u64,  // How often to capture DOM snapshot
    pub bid_color: [f32; 3],  // OKLCH
    pub ask_color: [f32; 3],  // OKLCH
    pub grid_lines: bool,
    pub perspective_strength: f32,
    pub light_direction: (f32, f32, f32),
}

impl DomSurfaceState {
    pub fn new() -> Self {
        Self {
            time_window_seconds: 300,  // 5 minutes default
            zoom: 1.0,
            ..Default::default()
        }
    }

    /// Returns surface points projected to 2D screen space (x, y, depth_value)
    pub fn surface_points(&self, w: f32, h: f32) -> Vec<(f32, f32, f64)> {
        let mut points = Vec::new();

        if self.surface_data.is_empty() {
            return points;
        }

        let time_range = self.surface_data.len() as f64;
        if time_range == 0.0 {
            return points;
        }

        let (min_price, max_price) = self.price_range;
        let price_range = max_price - min_price;
        if price_range == 0.0 {
            return points;
        }

        for (t_idx, snapshot) in self.surface_data.iter().enumerate() {
            let t_norm = t_idx as f64 / time_range;

            for (price, depth) in snapshot.bids.iter().chain(snapshot.asks.iter()) {
                let price_norm = (price - min_price) / price_range;
                let depth_norm = depth / self.max_depth;

                let x = (t_norm as f32 * w).clamp(0.0, w);
                let y = ((1.0 - price_norm) as f32 * h).clamp(0.0, h);

                points.push((x, y, depth_norm));
            }
        }

        points
    }

    /// Returns color for a depth value
    pub fn depth_color(&self, depth: f64, is_bid: bool) -> [f32; 4] {
        let norm_depth = if self.max_depth > 0.0 {
            (depth / self.max_depth).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let alpha = (0.3 + norm_depth * 0.7) as f32;

        if is_bid {
            [0.65, 0.15, 145.0, alpha] // Green for bids
        } else {
            [0.60, 0.18, 25.0, alpha] // Red for asks
        }
    }

    /// Converts time index to x coordinate
    pub fn time_to_x(&self, time_idx: usize, w: f32) -> f32 {
        if self.surface_data.is_empty() {
            return 0.0;
        }
        (time_idx as f32 / self.surface_data.len() as f32) * w
    }

    /// Converts price to y coordinate
    pub fn price_to_y(&self, price: f64, h: f32) -> f32 {
        let (min_price, max_price) = self.price_range;
        let price_range = max_price - min_price;
        if price_range == 0.0 {
            return h / 2.0;
        }
        let price_norm = (price - min_price) / price_range;
        ((1.0 - price_norm) as f32 * h).clamp(0.0, h)
    }
}

impl Default for DomSurfaceConfig {
    fn default() -> Self {
        Self {
            snapshot_interval_ms: 250,
            bid_color: [0.65, 0.15, 145.0],  // Green
            ask_color: [0.60, 0.18, 25.0],   // Red
            grid_lines: true,
            perspective_strength: 0.3,
            light_direction: (0.5, 0.5, 1.0),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DomSurfacePanel {
    id: DomSurfaceId,
    title: String,
}

impl DomSurfacePanel {
    pub fn new(id: DomSurfaceId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> DomSurfaceId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "dom_surface"
    }

    pub fn kind_label(&self) -> &'static str {
        "DOM Surface"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
