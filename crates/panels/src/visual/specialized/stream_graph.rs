use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamGraphId(pub u64);

#[derive(Clone, Debug)]
pub struct StreamLayer {
    pub label: String,
    pub values: Vec<f64>,
    pub baseline: Vec<f64>,
    pub color: [u8; 4],
}

#[derive(Clone, Debug, Default)]
pub struct StreamGraphState {
    pub layers: Vec<StreamLayer>,
    pub timestamps: Vec<u64>,
    pub max_value: f64,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum StreamBaseline {
    Zero,
    Wiggle,
    Silhouette,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamGraphConfig {
    pub baseline: StreamBaseline,
    pub curve_tension: f32,
}

impl Default for StreamGraphConfig {
    fn default() -> Self {
        Self {
            baseline: StreamBaseline::Wiggle,
            curve_tension: 0.5,
        }
    }
}

impl StreamGraphState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns layer polygons as (polygon_points, color)
    pub fn layer_polygons(&self, w: f32, h: f32) -> Vec<(Vec<(f32, f32)>, [f32; 4])> {
        if self.timestamps.is_empty() {
            return Vec::new();
        }

        self.layers
            .iter()
            .map(|layer| {
                let mut points = Vec::new();

                // Top edge
                for (i, &value) in layer.values.iter().enumerate() {
                    let x = (i as f32 / (self.timestamps.len() - 1).max(1) as f32) * w;
                    let y = (layer.baseline[i] + value) as f32 / self.max_value as f32 * h;
                    points.push((x, y));
                }

                // Bottom edge (reversed)
                for (i, &baseline) in layer.baseline.iter().enumerate().rev() {
                    let x = (i as f32 / (self.timestamps.len() - 1).max(1) as f32) * w;
                    let y = baseline as f32 / self.max_value as f32 * h;
                    points.push((x, y));
                }

                let color = [
                    layer.color[0] as f32 / 255.0,
                    layer.color[1] as f32 / 255.0,
                    layer.color[2] as f32 / 255.0,
                    layer.color[3] as f32 / 255.0,
                ];

                (points, color)
            })
            .collect()
    }

    /// Compute baseline for stream layout
    pub fn compute_baseline(&mut self) {
        if self.timestamps.is_empty() || self.layers.is_empty() {
            return;
        }

        let config = StreamGraphConfig::default();
        let n = self.timestamps.len();

        match config.baseline {
            StreamBaseline::Zero => {
                for layer in &mut self.layers {
                    layer.baseline = vec![0.0; n];
                }
            }
            StreamBaseline::Wiggle => {
                // Simplified wiggle: center around zero
                for i in 0..n {
                    let total: f64 = self.layers.iter().map(|l| l.values[i]).sum();
                    let mut offset = -total / 2.0;

                    for layer in &mut self.layers {
                        layer.baseline[i] = offset;
                        offset += layer.values[i];
                    }
                }
            }
            StreamBaseline::Silhouette => {
                // Stack symmetrically
                for i in 0..n {
                    let total: f64 = self.layers.iter().map(|l| l.values[i]).sum();
                    let mut offset = -total / 2.0;

                    for layer in &mut self.layers {
                        layer.baseline[i] = offset;
                        offset += layer.values[i];
                    }
                }
            }
        }
    }

    // Render helpers
    pub fn layer_paths(&self, w: f32, h: f32) -> Vec<(Vec<(f32, f32)>, [f32; 4])> {
        self.layer_polygons(w, h)
    }

    pub fn layer_color(&self, layer_idx: usize) -> Option<[f32; 4]> {
        let layer = self.layers.get(layer_idx)?;
        Some([
            layer.color[0] as f32 / 255.0,
            layer.color[1] as f32 / 255.0,
            layer.color[2] as f32 / 255.0,
            layer.color[3] as f32 / 255.0,
        ])
    }

    pub fn format_layer(&self, layer_idx: usize, time_idx: usize) -> String {
        if let Some(layer) = self.layers.get(layer_idx) {
            if let Some(&value) = layer.values.get(time_idx) {
                format!("{}: {:.1}", layer.label, value)
            } else {
                layer.label.clone()
            }
        } else {
            String::from("N/A")
        }
    }

    pub fn y_baseline(&self, layer_idx: usize, time_idx: usize) -> f32 {
        if let Some(layer) = self.layers.get(layer_idx) {
            if let Some(&baseline) = layer.baseline.get(time_idx) {
                baseline as f32
            } else {
                0.0
            }
        } else {
            0.0
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamGraphPanel {
    id: StreamGraphId,
    title: String,
    config: StreamGraphConfig,
}

impl StreamGraphPanel {
    pub fn new(id: StreamGraphId, title: String) -> Self {
        Self {
            id,
            title,
            config: StreamGraphConfig::default(),
        }
    }

    pub fn id(&self) -> StreamGraphId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "stream_graph"
    }

    pub fn kind_label(&self) -> &'static str {
        "Stream Graph"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 200.0)
    }
}
