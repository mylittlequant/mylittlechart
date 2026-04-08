use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RadarChartId(pub u64);

#[derive(Clone, Debug)]
pub struct RadarAxis {
    pub label: String,
    pub angle: f64,
    pub min: f64,
    pub max: f64,
}

#[derive(Clone, Debug)]
pub struct RadarDataset {
    pub label: String,
    pub values: Vec<f64>,
    pub color: [u8; 4],
}

#[derive(Clone, Debug, Default)]
pub struct RadarChartState {
    pub axes: Vec<RadarAxis>,
    pub datasets: Vec<RadarDataset>,
    pub center: (f32, f32),
    pub radius: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RadarChartConfig {
    pub num_levels: usize,
    pub fill_opacity: u8,
}

impl Default for RadarChartConfig {
    fn default() -> Self {
        Self {
            num_levels: 5,
            fill_opacity: 100,
        }
    }
}

impl RadarChartState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns axis lines from center to edge
    pub fn axis_lines(&self, cx: f32, cy: f32, r: f32) -> Vec<((f32, f32), (f32, f32))> {
        self.axes
            .iter()
            .map(|axis| {
                let end_x = cx + r * (axis.angle as f32).cos();
                let end_y = cy + r * (axis.angle as f32).sin();
                ((cx, cy), (end_x, end_y))
            })
            .collect()
    }

    /// Returns polygon vertices for a dataset
    pub fn data_polygon(&self, dataset_idx: usize, cx: f32, cy: f32, r: f32) -> Vec<(f32, f32)> {
        if let Some(dataset) = self.datasets.get(dataset_idx) {
            self.axes
                .iter()
                .enumerate()
                .filter_map(|(i, axis)| {
                    let value = dataset.values.get(i)?;
                    let normalized = if axis.max > axis.min {
                        ((value - axis.min) / (axis.max - axis.min)).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };

                    let distance = (normalized as f32) * r;
                    let x = cx + distance * (axis.angle as f32).cos();
                    let y = cy + distance * (axis.angle as f32).sin();
                    Some((x, y))
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Returns axis label positions
    pub fn axis_labels(&self, cx: f32, cy: f32, r: f32) -> Vec<(f32, f32, &str)> {
        let label_offset = r * 1.1;
        self.axes
            .iter()
            .map(|axis| {
                let x = cx + label_offset * (axis.angle as f32).cos();
                let y = cy + label_offset * (axis.angle as f32).sin();
                (x, y, axis.label.as_str())
            })
            .collect()
    }

    // Render helpers
    pub fn axis_points(&self, cx: f32, cy: f32, r: f32) -> Vec<(f32, f32)> {
        self.axes
            .iter()
            .map(|axis| {
                let x = cx + r * (axis.angle as f32).cos();
                let y = cy + r * (axis.angle as f32).sin();
                (x, y)
            })
            .collect()
    }

    pub fn polygon_points(&self, dataset_idx: usize, cx: f32, cy: f32, r: f32) -> Vec<(f32, f32)> {
        self.data_polygon(dataset_idx, cx, cy, r)
    }

    pub fn format_axis(&self, axis_idx: usize) -> String {
        if let Some(axis) = self.axes.get(axis_idx) {
            axis.label.clone()
        } else {
            format!("Axis {}", axis_idx)
        }
    }

    pub fn value_normalized(&self, dataset_idx: usize, axis_idx: usize) -> f32 {
        if let Some(dataset) = self.datasets.get(dataset_idx) {
            if let Some(axis) = self.axes.get(axis_idx) {
                if let Some(&value) = dataset.values.get(axis_idx) {
                    if axis.max > axis.min {
                        return ((value - axis.min) / (axis.max - axis.min)).clamp(0.0, 1.0) as f32;
                    }
                }
            }
        }
        0.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RadarChartPanel {
    id: RadarChartId,
    title: String,
    config: RadarChartConfig,
}

impl RadarChartPanel {
    pub fn new(id: RadarChartId, title: String) -> Self {
        Self {
            id,
            title,
            config: RadarChartConfig::default(),
        }
    }

    pub fn id(&self) -> RadarChartId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "radar_chart"
    }

    pub fn kind_label(&self) -> &'static str {
        "Radar Chart"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (250.0, 250.0)
    }
}
