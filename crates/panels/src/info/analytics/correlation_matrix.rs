use serde::{Serialize, Deserialize};
use std::time::Instant;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CorrelationMatrixId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct CorrelationMatrixState {
    pub assets: Vec<String>,  // Ordered list of asset symbols
    pub correlation_matrix: Vec<Vec<f64>>,  // [i][j] = correlation(asset_i, asset_j)
    pub time_window_ms: u64,  // Rolling window for correlation calculation
    pub selected_cell: Option<(usize, usize)>,
    pub last_update: Option<Instant>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CorrelationMatrixConfig {
    pub cell_size: f32,
    pub cell_spacing: f32,
    pub color_gradient: CorrelationGradient,
    pub show_values: bool,  // Display correlation values in cells
    pub value_font_size: f32,
    pub show_dendogram: bool,  // Hierarchical clustering visualization
    pub cluster_method: ClusterMethod,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CorrelationGradient {
    pub negative: [f32; 3],  // OKLCH for -1.0 (strong negative)
    pub neutral: [f32; 3],   // OKLCH for 0.0 (no correlation)
    pub positive: [f32; 3],  // OKLCH for +1.0 (strong positive)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClusterMethod {
    Single,
    Complete,
    Average,
}

impl CorrelationMatrixState {
    pub fn new() -> Self {
        Self {
            time_window_ms: 86400000,  // 24 hours default
            ..Default::default()
        }
    }

    /// Returns the color for a correlation matrix cell (blue negative, white zero, red positive)
    pub fn cell_color(&self, row: usize, col: usize) -> [f32; 4] {
        if row >= self.correlation_matrix.len() || col >= self.correlation_matrix.get(row).map_or(0, |v| v.len()) {
            return [0.5, 0.0, 0.0, 1.0];
        }

        let corr = self.correlation_matrix[row][col];

        if corr < 0.0 {
            let intensity = (-corr).clamp(0.0, 1.0) as f32;
            [0.60, 0.18 * intensity, 25.0, 1.0]
        } else if corr > 0.0 {
            let intensity = corr.clamp(0.0, 1.0) as f32;
            [0.65, 0.15 * intensity, 145.0, 1.0]
        } else {
            [0.5, 0.0, 0.0, 1.0]
        }
    }

    /// Returns the correlation value for a specific cell
    pub fn cell_value(&self, row: usize, col: usize) -> f64 {
        if row >= self.correlation_matrix.len() || col >= self.correlation_matrix.get(row).map_or(0, |v| v.len()) {
            return 0.0;
        }
        self.correlation_matrix[row][col]
    }

    /// Returns the dimension of the NxN matrix
    pub fn grid_size(&self) -> usize {
        self.correlation_matrix.len()
    }

    /// Returns visible cells for rendering (all cells in the matrix)
    pub fn visible_cells(&self) -> Vec<(usize, usize, f64)> {
        let mut cells = Vec::new();
        for (row, row_vec) in self.correlation_matrix.iter().enumerate() {
            for (col, &value) in row_vec.iter().enumerate() {
                cells.push((row, col, value));
            }
        }
        cells
    }

    /// Format correlation value for display
    pub fn format_value(&self, value: f64) -> String {
        format!("{:.2}", value)
    }

    /// Get color for a correlation value (same as cell_color but for direct value)
    pub fn correlation_color(&self, corr: f64) -> [f32; 4] {
        if corr < 0.0 {
            let intensity = (-corr).clamp(0.0, 1.0) as f32;
            [0.60, 0.18 * intensity, 25.0, 1.0]
        } else if corr > 0.0 {
            let intensity = corr.clamp(0.0, 1.0) as f32;
            [0.65, 0.15 * intensity, 145.0, 1.0]
        } else {
            [0.5, 0.0, 0.0, 1.0]
        }
    }
}

impl Default for CorrelationMatrixConfig {
    fn default() -> Self {
        Self {
            cell_size: 40.0,
            cell_spacing: 1.0,
            color_gradient: CorrelationGradient {
                negative: [0.60, 0.18, 25.0],   // Red
                neutral: [0.5, 0.0, 0.0],       // Gray
                positive: [0.65, 0.15, 145.0],  // Green
            },
            show_values: true,
            value_font_size: 12.0,
            show_dendogram: false,
            cluster_method: ClusterMethod::Average,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CorrelationMatrixPanel {
    id: CorrelationMatrixId,
    title: String,
}

impl CorrelationMatrixPanel {
    pub fn new(id: CorrelationMatrixId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> CorrelationMatrixId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "correlation_matrix"
    }

    pub fn kind_label(&self) -> &'static str {
        "Correlation Matrix"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 300.0)
    }
}
