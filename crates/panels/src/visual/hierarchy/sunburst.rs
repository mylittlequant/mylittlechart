use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SunburstId(pub u64);

#[derive(Clone, Debug)]
pub struct SunburstNode {
    pub id: String,
    pub label: String,
    pub value: f64,
    pub depth: usize,
    pub parent: Option<String>,
    pub start_angle: f64,
    pub end_angle: f64,
    pub inner_radius: f32,
    pub outer_radius: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug, Default)]
pub struct SunburstState {
    pub nodes: Vec<SunburstNode>,
    pub root_id: String,
    pub max_depth: usize,
    pub center: (f32, f32),
    pub radius: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SunburstConfig {
    pub padding: f32,
    pub start_angle: f64,
    pub end_angle: f64,
}

impl Default for SunburstConfig {
    fn default() -> Self {
        Self {
            padding: 2.0,
            start_angle: 0.0,
            end_angle: std::f64::consts::TAU,
        }
    }
}

impl SunburstState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns visible arcs with start_angle, end_angle, inner_r, outer_r
    pub fn visible_arcs(&self) -> Vec<SunburstArc> {
        self.nodes
            .iter()
            .map(|node| {
                let color = [
                    node.color[0] as f32 / 255.0,
                    node.color[1] as f32 / 255.0,
                    node.color[2] as f32 / 255.0,
                    node.color[3] as f32 / 255.0,
                ];
                SunburstArc {
                    start_angle: node.start_angle,
                    end_angle: node.end_angle,
                    inner_r: node.inner_radius,
                    outer_r: node.outer_radius,
                    color,
                    label: node.label.clone(),
                }
            })
            .collect()
    }

    /// Returns color for a node based on its depth
    pub fn depth_color(&self, depth: usize) -> [f32; 4] {
        let norm_depth = if self.max_depth > 0 {
            depth as f32 / self.max_depth as f32
        } else {
            0.0
        };

        // Lighter to darker as depth increases
        let lightness = 0.7 - norm_depth * 0.3;
        [lightness, 0.15, 145.0, 1.0]
    }

    /// Compute arc positions from hierarchy
    pub fn layout(&mut self) {
        if self.nodes.is_empty() {
            return;
        }

        let config = SunburstConfig::default();
        let angle_span = config.end_angle - config.start_angle;

        // Compute total value first
        let total_value: f64 = self.nodes.iter().map(|n| n.value).sum::<f64>().max(1.0);

        // Simple layout: distribute angle based on value
        let mut angle_offset = config.start_angle;

        for node in &mut self.nodes {
            let depth_ratio = if self.max_depth > 0 {
                node.depth as f32 / self.max_depth as f32
            } else {
                0.0
            };

            node.inner_radius = depth_ratio * self.radius;
            node.outer_radius = ((node.depth + 1) as f32 / (self.max_depth + 1) as f32) * self.radius;

            // Distribute angle based on value
            let angle_width = angle_span * (node.value / total_value);
            node.start_angle = angle_offset;
            node.end_angle = angle_offset + angle_width;
            angle_offset = node.end_angle;
        }
    }
}

#[derive(Clone, Debug)]
pub struct SunburstArc {
    pub start_angle: f64,
    pub end_angle: f64,
    pub inner_r: f32,
    pub outer_r: f32,
    pub color: [f32; 4],
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SunburstPanel {
    id: SunburstId,
    title: String,
    config: SunburstConfig,
}

impl SunburstPanel {
    pub fn new(id: SunburstId, title: String) -> Self {
        Self {
            id,
            title,
            config: SunburstConfig::default(),
        }
    }

    pub fn id(&self) -> SunburstId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "sunburst"
    }

    pub fn kind_label(&self) -> &'static str {
        "Sunburst"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 300.0)
    }
}
