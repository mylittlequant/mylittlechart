use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CircularPackingId(pub u64);

#[derive(Clone, Debug)]
pub struct PackedCircle {
    pub id: String,
    pub label: String,
    pub value: f64,
    pub depth: usize,
    pub parent: Option<String>,
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug, Default)]
pub struct CircularPackingState {
    pub circles: Vec<PackedCircle>,
    pub root_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CircularPackingConfig {
    pub padding: f32,
}

impl Default for CircularPackingConfig {
    fn default() -> Self {
        Self { padding: 2.0 }
    }
}

impl CircularPackingState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns visible circles with cx, cy, r coordinates
    pub fn visible_circles(&self) -> Vec<PackedCircleOutput> {
        self.circles
            .iter()
            .map(|circle| {
                let color = [
                    circle.color[0] as f32 / 255.0,
                    circle.color[1] as f32 / 255.0,
                    circle.color[2] as f32 / 255.0,
                    circle.color[3] as f32 / 255.0,
                ];
                PackedCircleOutput {
                    cx: circle.x,
                    cy: circle.y,
                    r: circle.r,
                    color,
                    label: circle.label.clone(),
                }
            })
            .collect()
    }

    /// Returns color for a node based on its depth
    pub fn depth_color(&self, depth: usize) -> [f32; 4] {
        // Estimate max depth from circles
        let max_depth = self.circles.iter()
            .map(|c| c.depth)
            .max()
            .unwrap_or(0);

        let norm_depth = if max_depth > 0 {
            depth as f32 / max_depth as f32
        } else {
            0.0
        };

        // Lighter to darker as depth increases
        let lightness = 0.7 - norm_depth * 0.3;
        [lightness, 0.15, 145.0, 1.0]
    }

    /// Formats a label for display based on available space
    pub fn format_label(&self, label: &str, available_radius: f32) -> String {
        let max_chars = (available_radius * 2.0 / 8.0) as usize; // Approximate 8px per char
        if label.len() <= max_chars {
            label.to_string()
        } else if max_chars > 3 {
            format!("{}...", &label[..max_chars - 3])
        } else {
            String::new()
        }
    }

    /// Compute circle packing layout (simplified)
    pub fn pack(&mut self, w: f32, h: f32) {
        if self.circles.is_empty() {
            return;
        }

        let config = CircularPackingConfig::default();
        let center_x = w / 2.0;
        let center_y = h / 2.0;

        // Sort by value descending
        self.circles.sort_by(|a, b| {
            b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Simple packing: place in spiral
        let total_value: f64 = self.circles.iter().map(|c| c.value).sum();
        let max_radius = w.min(h) / 2.0 - config.padding;

        for (i, circle) in self.circles.iter_mut().enumerate() {
            let angle = i as f32 * 2.4; // Golden angle approximation
            let radius_ratio = (circle.value / total_value).sqrt();
            circle.r = (radius_ratio as f32 * max_radius).max(5.0);

            let distance = (i as f32).sqrt() * (circle.r + config.padding);
            circle.x = center_x + distance * angle.cos();
            circle.y = center_y + distance * angle.sin();
        }
    }
}

#[derive(Clone, Debug)]
pub struct PackedCircleOutput {
    pub cx: f32,
    pub cy: f32,
    pub r: f32,
    pub color: [f32; 4],
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CircularPackingPanel {
    id: CircularPackingId,
    title: String,
    config: CircularPackingConfig,
}

impl CircularPackingPanel {
    pub fn new(id: CircularPackingId, title: String) -> Self {
        Self {
            id,
            title,
            config: CircularPackingConfig::default(),
        }
    }

    pub fn id(&self) -> CircularPackingId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "circular_packing"
    }

    pub fn kind_label(&self) -> &'static str {
        "Circle Packing"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 300.0)
    }
}
