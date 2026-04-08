use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrderFlowHeatmapId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct OrderFlowHeatmapState {
    pub symbol: String,
    pub grid: Vec<Vec<f64>>,  // [time_bucket][price_level] = intensity
    pub time_range: (i64, i64),  // Unix timestamps (start, end)
    pub price_range: (f64, f64),
    pub bucket_size_ms: u64,
    pub price_tick: f64,
    pub max_intensity: f64,
    pub scroll_offset: usize,  // For horizontal scrolling through time
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderFlowHeatmapConfig {
    pub cell_width: f32,  // Width of each time bucket in pixels
    pub cell_height: f32,  // Height of each price level in pixels
    pub color_gradient: HeatmapGradient,
    pub show_grid_lines: bool,
    pub fade_duration_ms: u64,  // For HeatmapFade animation
    pub imbalance_threshold: f64,  // Show buy/sell imbalance above this ratio
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeatmapGradient {
    pub low: [f32; 3],   // OKLCH for low intensity
    pub mid: [f32; 3],   // OKLCH for medium intensity
    pub high: [f32; 3],  // OKLCH for high intensity
}

impl OrderFlowHeatmapState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns visible cells within the current scroll window
    pub fn visible_cells(&self, w: f32, h: f32) -> Vec<(usize, usize, f32, f32, f32, f32)> {
        let mut cells = Vec::new();
        let cols = self.grid.len().saturating_sub(self.scroll_offset);
        let rows = self.grid.first().map_or(0, |v| v.len());

        if cols == 0 || rows == 0 {
            return cells;
        }

        let cell_w = w / cols as f32;
        let cell_h = h / rows as f32;

        for t_idx in self.scroll_offset..self.grid.len() {
            let visible_t = t_idx - self.scroll_offset;
            if let Some(row) = self.grid.get(t_idx) {
                for (p_idx, _) in row.iter().enumerate() {
                    let x = visible_t as f32 * cell_w;
                    let y = p_idx as f32 * cell_h;
                    cells.push((t_idx, p_idx, x, y, cell_w, cell_h));
                }
            }
        }

        cells
    }

    /// Returns the color for a specific cell based on intensity
    pub fn intensity_color(&self, time_idx: usize, price_idx: usize) -> [f32; 4] {
        if time_idx >= self.grid.len() || price_idx >= self.grid.get(time_idx).map_or(0, |v| v.len()) {
            return [0.0, 0.0, 0.0, 0.0];
        }

        let intensity = self.grid[time_idx][price_idx];
        let norm_intensity = if self.max_intensity > 0.0 {
            (intensity / self.max_intensity).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Gradient from dark -> yellow -> red
        if norm_intensity < 0.33 {
            let t = (norm_intensity / 0.33) as f32;
            [0.3, 0.0, 0.0, t * 0.5]
        } else if norm_intensity < 0.66 {
            let t = ((norm_intensity - 0.33) / 0.33) as f32;
            [0.6, 0.2, 60.0, 0.5 + t * 0.3]
        } else {
            let t = ((norm_intensity - 0.66) / 0.34) as f32;
            [0.60, 0.18, 25.0, 0.8 + t * 0.2]
        }
    }

    /// Returns the current time range being displayed
    pub fn time_range(&self) -> (i64, i64) {
        self.time_range
    }

    /// Returns the current price range being displayed
    pub fn price_range(&self) -> (f64, f64) {
        self.price_range
    }
}

impl Default for OrderFlowHeatmapConfig {
    fn default() -> Self {
        Self {
            cell_width: 10.0,
            cell_height: 10.0,
            color_gradient: HeatmapGradient {
                low: [0.3, 0.0, 0.0],       // Dark
                mid: [0.6, 0.2, 60.0],      // Yellow
                high: [0.60, 0.18, 25.0],   // Red
            },
            show_grid_lines: false,
            fade_duration_ms: 500,
            imbalance_threshold: 1.5,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderFlowHeatmapPanel {
    id: OrderFlowHeatmapId,
    title: String,
}

impl OrderFlowHeatmapPanel {
    pub fn new(id: OrderFlowHeatmapId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> OrderFlowHeatmapId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "order_flow_heatmap"
    }

    pub fn kind_label(&self) -> &'static str {
        "Flow Heatmap"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
