use serde::{Serialize, Deserialize};
use std::collections::HashSet;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ParallelCoordinatesId(pub u64);

#[derive(Clone, Debug)]
pub struct ParallelCoordinatesState {
    /// Data points (N-dimensional)
    pub data: Vec<Vec<f32>>,

    /// Dimension names
    pub dimension_names: Vec<String>,

    /// Dimension ranges (min, max)
    pub dimension_ranges: Vec<(f32, f32)>,

    /// Selected/highlighted points
    pub selected_indices: HashSet<usize>,

    /// Axis positions (screen x coordinates)
    pub axis_positions: Vec<f32>,

    /// Color mode
    pub color_mode: ParallelCoordsColorMode,
}

#[derive(Clone, Debug)]
pub enum ParallelCoordsColorMode {
    Uniform([f32; 4]),
    ByDimension(usize, ColorMapPC),
    ByCluster(Vec<[f32; 4]>),
}

#[derive(Clone, Debug)]
pub struct ColorMapPC {
    pub min_value: f32,
    pub max_value: f32,
    pub gradient: Vec<[f32; 4]>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParallelCoordinatesConfig {
    /// Line width
    pub line_width: f32,

    /// Line opacity
    pub line_opacity: f32,

    /// Highlight selected
    pub highlight_selected: bool,

    /// Selected line width
    pub selected_line_width: f32,

    /// Axis label font size
    pub label_font_size: f32,

    /// Curve lines (vs straight)
    pub use_curves: bool,
}

impl Default for ParallelCoordinatesState {
    fn default() -> Self {
        Self::new()
    }
}

impl ParallelCoordinatesState {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            dimension_names: Vec::new(),
            dimension_ranges: Vec::new(),
            selected_indices: HashSet::new(),
            axis_positions: Vec::new(),
            color_mode: ParallelCoordsColorMode::Uniform([0.5, 0.5, 0.5, 0.3]),
        }
    }

    /// Get axis positions (x coordinates)
    pub fn axis_positions(&self, w: f32) -> Vec<f32> {
        if self.dimension_names.is_empty() {
            return Vec::new();
        }

        let margin = 50.0;
        let usable_width = w - 2.0 * margin;
        let num_axes = self.dimension_names.len();

        if num_axes <= 1 {
            return vec![w / 2.0];
        }

        let spacing = usable_width / (num_axes - 1) as f32;

        (0..num_axes)
            .map(|i| margin + i as f32 * spacing)
            .collect()
    }

    /// Get polylines for rendering (one per data point)
    pub fn polylines(&self, w: f32, h: f32) -> Vec<(Vec<(f32, f32)>, [f32; 4])> {
        let positions = self.axis_positions(w);

        if positions.is_empty() || self.dimension_ranges.is_empty() {
            return Vec::new();
        }

        let mut lines = Vec::new();

        for (data_idx, point) in self.data.iter().enumerate() {
            if point.len() != positions.len() {
                continue;
            }

            let mut line_points = Vec::new();

            for (dim_idx, (&value, &x)) in point.iter().zip(positions.iter()).enumerate() {
                if dim_idx >= self.dimension_ranges.len() {
                    break;
                }

                let y = self.value_to_y(dim_idx, value as f64, h);
                line_points.push((x, y));
            }

            let color = self.get_point_color(data_idx, point);
            lines.push((line_points, color));
        }

        lines
    }

    /// Convert value to y coordinate for a specific axis
    pub fn value_to_y(&self, axis_idx: usize, value: f64, h: f32) -> f32 {
        if axis_idx >= self.dimension_ranges.len() {
            return h / 2.0;
        }

        let (min, max) = self.dimension_ranges[axis_idx];
        let margin = 30.0;
        let usable_height = h - 2.0 * margin;

        let t = if max > min {
            ((value as f32 - min) / (max - min)).clamp(0.0, 1.0)
        } else {
            0.5
        };

        h - margin - t * usable_height
    }

    fn get_point_color(&self, data_idx: usize, point: &[f32]) -> [f32; 4] {
        if self.selected_indices.contains(&data_idx) {
            return [1.0, 0.5, 0.0, 1.0];
        }

        match &self.color_mode {
            ParallelCoordsColorMode::Uniform(color) => *color,
            ParallelCoordsColorMode::ByDimension(dim_idx, colormap) => {
                if *dim_idx < point.len() && *dim_idx < self.dimension_ranges.len() {
                    let value = point[*dim_idx];
                    let (min, max) = self.dimension_ranges[*dim_idx];
                    let t = if max > min {
                        ((value - min) / (max - min)).clamp(0.0, 1.0)
                    } else {
                        0.5
                    };
                    let gradient_idx = (t * (colormap.gradient.len() - 1) as f32) as usize;
                    colormap.gradient[gradient_idx.min(colormap.gradient.len() - 1)]
                } else {
                    [0.5, 0.5, 0.5, 0.3]
                }
            }
            ParallelCoordsColorMode::ByCluster(colors) => {
                let cluster_idx = data_idx % colors.len();
                colors[cluster_idx]
            }
        }
    }

    // Render helpers
    pub fn visible_lines(&self) -> &[Vec<f32>] {
        &self.data
    }

    pub fn normalize_value(&self, axis_idx: usize, value: f64) -> f32 {
        if axis_idx >= self.dimension_ranges.len() {
            return 0.5;
        }
        let (min, max) = self.dimension_ranges[axis_idx];
        if max > min {
            ((value as f32 - min) / (max - min)).clamp(0.0, 1.0)
        } else {
            0.5
        }
    }

    pub fn line_color(&self, line_idx: usize) -> [f32; 4] {
        if let Some(point) = self.data.get(line_idx) {
            self.get_point_color(line_idx, point)
        } else {
            [0.5, 0.5, 0.5, 0.3]
        }
    }

    pub fn format_axis_label(&self, axis_idx: usize) -> String {
        if let Some(name) = self.dimension_names.get(axis_idx) {
            name.clone()
        } else {
            format!("Axis {}", axis_idx)
        }
    }
}

impl Default for ParallelCoordinatesConfig {
    fn default() -> Self {
        Self {
            line_width: 1.0,
            line_opacity: 0.3,
            highlight_selected: true,
            selected_line_width: 2.0,
            label_font_size: 12.0,
            use_curves: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParallelCoordinatesPanel {
    id: ParallelCoordinatesId,
    title: String,
}

impl ParallelCoordinatesPanel {
    pub fn new(id: ParallelCoordinatesId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> ParallelCoordinatesId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "parallel_coordinates"
    }

    pub fn kind_label(&self) -> &'static str {
        "Parallel Coords"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
