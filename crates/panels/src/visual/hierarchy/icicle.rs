use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IcicleId(pub u64);

#[derive(Clone, Debug)]
pub struct IcicleNode {
    pub id: String,
    pub label: String,
    pub value: f64,
    pub depth: usize,
    pub parent: Option<String>,
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug, Default)]
pub struct IcicleState {
    pub nodes: Vec<IcicleNode>,
    pub root_id: String,
    pub max_depth: usize,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum IcicleOrientation {
    TopDown,
    BottomUp,
    LeftRight,
    RightLeft,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IcicleConfig {
    pub orientation: IcicleOrientation,
    pub padding: f32,
}

impl Default for IcicleConfig {
    fn default() -> Self {
        Self {
            orientation: IcicleOrientation::TopDown,
            padding: 1.0,
        }
    }
}

impl IcicleState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns visible rectangles with x, y, w, h coordinates
    pub fn visible_rects(&self) -> Vec<IcicleRect> {
        self.nodes
            .iter()
            .map(|node| {
                let color = [
                    node.color[0] as f32 / 255.0,
                    node.color[1] as f32 / 255.0,
                    node.color[2] as f32 / 255.0,
                    node.color[3] as f32 / 255.0,
                ];
                IcicleRect {
                    x: node.x0,
                    y: node.y0,
                    w: node.x1 - node.x0,
                    h: node.y1 - node.y0,
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

    /// Formats a label for display based on available space
    pub fn format_label(&self, label: &str, available_width: f32) -> String {
        let max_chars = (available_width / 8.0) as usize; // Approximate 8px per char
        if label.len() <= max_chars {
            label.to_string()
        } else if max_chars > 3 {
            format!("{}...", &label[..max_chars - 3])
        } else {
            String::new()
        }
    }

    /// Compute partitioned rectangles
    pub fn layout(&mut self, _w: f32, _h: f32) {
        if self.nodes.is_empty() {
            return;
        }

        let _config = IcicleConfig::default();

        // Group nodes by depth
        let mut depth_nodes: Vec<Vec<usize>> = vec![Vec::new(); self.max_depth + 1];
        for (idx, node) in self.nodes.iter().enumerate() {
            depth_nodes[node.depth].push(idx);
        }

        // Layout each depth level
        for depth in 0..=self.max_depth {
            let nodes_at_depth = &depth_nodes[depth];
            if nodes_at_depth.is_empty() {
                continue;
            }

            let total_value: f64 = nodes_at_depth
                .iter()
                .map(|&idx| self.nodes[idx].value)
                .sum();

            let mut x_offset = 0.0;
            let y0 = depth as f32 / (self.max_depth + 1) as f32;
            let y1 = (depth + 1) as f32 / (self.max_depth + 1) as f32;

            for &idx in nodes_at_depth {
                let node = &mut self.nodes[idx];
                let width_ratio = if total_value > 0.0 {
                    node.value / total_value
                } else {
                    1.0 / nodes_at_depth.len() as f64
                };

                node.x0 = x_offset as f32;
                node.x1 = (x_offset + width_ratio) as f32;
                node.y0 = y0;
                node.y1 = y1;

                x_offset += width_ratio;
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct IcicleRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: [f32; 4],
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IciclePanel {
    id: IcicleId,
    title: String,
    config: IcicleConfig,
}

impl IciclePanel {
    pub fn new(id: IcicleId, title: String) -> Self {
        Self {
            id,
            title,
            config: IcicleConfig::default(),
        }
    }

    pub fn id(&self) -> IcicleId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "icicle"
    }

    pub fn kind_label(&self) -> &'static str {
        "Icicle Chart"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 300.0)
    }
}
