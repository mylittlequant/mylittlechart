use serde::{Serialize, Deserialize};
use std::collections::VecDeque;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamingHeatmapId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct StreamingHeatmapState {
    pub grid: VecDeque<Vec<f64>>,  // Rolling window of [time_bucket][value_level]
    pub time_window_ms: u64,  // How much history to retain
    pub bucket_size_ms: u64,
    pub value_range: (f64, f64),
    pub max_intensity: f64,
    pub scroll_offset: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamingHeatmapConfig {
    pub cell_width: f32,
    pub cell_height: f32,
    pub color_gradient: HeatmapGradient,
    pub fade_duration_ms: u64,  // HeatmapFade animation
    pub auto_scroll: bool,
    pub vertical_orientation: bool,  // Time flows top-to-bottom vs left-to-right
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeatmapGradient {
    pub low: [f32; 3],   // OKLCH for low intensity
    pub mid: [f32; 3],   // OKLCH for medium intensity
    pub high: [f32; 3],  // OKLCH for high intensity
}

impl StreamingHeatmapState {
    pub fn new() -> Self {
        Self {
            time_window_ms: 60000,  // 60 seconds default
            bucket_size_ms: 100,
            ..Default::default()
        }
    }

    /// Returns the color for a specific cell based on intensity
    pub fn cell_color(&self, col: usize, row: usize) -> [f32; 4] {
        if col >= self.grid.len() || row >= self.grid.get(col).map_or(0, |v| v.len()) {
            return [0.0, 0.0, 0.0, 0.0];
        }

        let intensity = self.grid[col][row];
        let norm_intensity = if self.max_intensity > 0.0 {
            (intensity / self.max_intensity).clamp(0.0, 1.0)
        } else {
            0.0
        };

        if norm_intensity < 0.5 {
            let t = (norm_intensity / 0.5) as f32;
            [0.3 + t * 0.3, 0.0, 0.0, t]
        } else {
            let t = ((norm_intensity - 0.5) / 0.5) as f32;
            [0.6, 0.2 * t, 60.0 * (1.0 - t) + 25.0 * t, 0.5 + t * 0.5]
        }
    }

    // Render helpers
    pub fn visible_cells(&self) -> Vec<(usize, usize, [f32; 4])> {
        let mut cells = Vec::new();
        for (col_idx, col) in self.grid.iter().enumerate() {
            for (row_idx, _) in col.iter().enumerate() {
                let color = self.cell_color(col_idx, row_idx);
                cells.push((col_idx, row_idx, color));
            }
        }
        cells
    }

    pub fn intensity_color(&self, intensity: f64) -> [f32; 4] {
        let norm = if self.max_intensity > 0.0 {
            (intensity / self.max_intensity).clamp(0.0, 1.0)
        } else {
            0.0
        };

        if norm < 0.5 {
            let t = (norm / 0.5) as f32;
            [0.3 + t * 0.3, 0.0, 0.0, t]
        } else {
            let t = ((norm - 0.5) / 0.5) as f32;
            [0.6, 0.2 * t, 60.0 * (1.0 - t) + 25.0 * t, 0.5 + t * 0.5]
        }
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub fn time_labels(&self) -> Vec<String> {
        let num_buckets = self.grid.len();
        let step = (num_buckets / 5).max(1);
        (0..num_buckets)
            .step_by(step)
            .map(|i| format!("{}ms", i as u64 * self.bucket_size_ms))
            .collect()
    }
}

impl Default for StreamingHeatmapConfig {
    fn default() -> Self {
        Self {
            cell_width: 10.0,
            cell_height: 10.0,
            color_gradient: HeatmapGradient {
                low: [0.3, 0.0, 0.0],       // Dark
                mid: [0.6, 0.2, 60.0],      // Yellow
                high: [0.60, 0.18, 25.0],   // Red
            },
            fade_duration_ms: 500,
            auto_scroll: true,
            vertical_orientation: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamingHeatmapPanel {
    id: StreamingHeatmapId,
    title: String,
}

impl StreamingHeatmapPanel {
    pub fn new(id: StreamingHeatmapId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> StreamingHeatmapId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "streaming_heatmap"
    }

    pub fn kind_label(&self) -> &'static str {
        "Streaming Heatmap"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
