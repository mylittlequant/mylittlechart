use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LiquidationHeatmapId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct LiquidationHeatmapState {
    pub symbol: String,
    pub grid: Vec<Vec<f64>>,  // [time_bucket][price_level] = liq_volume
    pub time_range: (i64, i64),  // Unix timestamps
    pub price_range: (f64, f64),
    pub estimated_liqui_levels: Vec<(f64, f64)>,  // (price, estimated_volume)
    pub realized_liqui_events: Vec<LiquidationEvent>,
}

#[derive(Clone, Debug)]
pub struct LiquidationEvent {
    pub timestamp: i64,  // Unix timestamp
    pub price: f64,
    pub volume: f64,
    pub side: Side,  // Long or Short liquidation
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Side {
    Long,
    Short,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiquidationHeatmapConfig {
    pub cell_size: (f32, f32),  // (width, height) in pixels
    pub color_gradient: LiquidationGradient,
    pub show_estimated_levels: bool,
    pub show_realized_events: bool,
    pub event_marker_size: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiquidationGradient {
    pub low: [f32; 3],    // OKLCH: low liquidation risk
    pub medium: [f32; 3], // OKLCH: medium risk
    pub high: [f32; 3],   // OKLCH: high risk (danger zone)
}

impl LiquidationHeatmapState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns visible cells with their coordinates and intensity
    pub fn visible_cells(&self, w: f32, h: f32) -> Vec<(usize, usize, f32, f32, f32, f32, f64)> {
        let mut cells = Vec::new();

        if self.grid.is_empty() {
            return cells;
        }

        let cols = self.grid.len();
        let rows = self.grid.first().map_or(0, |v| v.len());

        if cols == 0 || rows == 0 {
            return cells;
        }

        let cell_w = w / cols as f32;
        let cell_h = h / rows as f32;

        for (t_idx, row) in self.grid.iter().enumerate() {
            for (p_idx, &intensity) in row.iter().enumerate() {
                let x = t_idx as f32 * cell_w;
                let y = p_idx as f32 * cell_h;
                cells.push((t_idx, p_idx, x, y, cell_w, cell_h, intensity));
            }
        }

        cells
    }

    /// Returns the color for liquidation intensity (green low -> yellow mid -> red high)
    pub fn liquidation_color(&self, liq_volume: f64) -> [f32; 4] {
        if liq_volume == 0.0 {
            return [0.0, 0.0, 0.0, 0.0];
        }

        let max_vol = self.grid.iter()
            .flat_map(|row| row.iter())
            .fold(0.0f64, |max, &v| max.max(v));

        let norm_intensity = if max_vol > 0.0 {
            (liq_volume / max_vol).clamp(0.0, 1.0)
        } else {
            0.0
        };

        if norm_intensity < 0.33 {
            let t = (norm_intensity / 0.33) as f32;
            [0.65, 0.15, 145.0, t]
        } else if norm_intensity < 0.66 {
            let t = ((norm_intensity - 0.33) / 0.33) as f32;
            [0.7, 0.2, 60.0, 0.5 + t * 0.5]
        } else {
            let t = ((norm_intensity - 0.66) / 0.34) as f32;
            [0.60, 0.18, 25.0, 0.7 + t * 0.3]
        }
    }

    /// Returns the liquidation cluster at a specific price level
    pub fn cluster_at(&self, price: f64) -> Option<f64> {
        let (min_price, max_price) = self.price_range;
        let price_range = max_price - min_price;
        if price_range == 0.0 {
            return None;
        }

        let rows = self.grid.first().map_or(0, |v| v.len());
        if rows == 0 {
            return None;
        }

        let price_idx = ((price - min_price) / price_range * rows as f64) as usize;
        if price_idx >= rows {
            return None;
        }

        // Sum liquidations across all time buckets for this price level
        let total: f64 = self.grid.iter()
            .filter_map(|row| row.get(price_idx))
            .sum();

        Some(total)
    }
}

impl Default for LiquidationHeatmapConfig {
    fn default() -> Self {
        Self {
            cell_size: (10.0, 10.0),
            color_gradient: LiquidationGradient {
                low: [0.65, 0.15, 145.0],   // Green
                medium: [0.7, 0.2, 60.0],   // Yellow
                high: [0.60, 0.18, 25.0],   // Red
            },
            show_estimated_levels: true,
            show_realized_events: true,
            event_marker_size: 6.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiquidationHeatmapPanel {
    id: LiquidationHeatmapId,
    title: String,
}

impl LiquidationHeatmapPanel {
    pub fn new(id: LiquidationHeatmapId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> LiquidationHeatmapId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "liquidation_heatmap"
    }

    pub fn kind_label(&self) -> &'static str {
        "Liquidation Map"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
