use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SankeyId(pub u64);

#[derive(Clone, Debug)]
pub struct SankeyNode {
    pub id: String,
    pub label: String,
    pub value: f64,
    pub depth: usize,
    pub layer: usize,
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug)]
pub struct SankeyLink {
    pub source: String,
    pub target: String,
    pub value: f64,
    pub y0: f32,
    pub y1: f32,
    pub width: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug)]
pub struct SankeyFlowPath {
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub width: f32,
    pub color: [f32; 4],
}

#[derive(Clone, Debug, Default)]
pub struct SankeyState {
    pub nodes: Vec<SankeyNode>,
    pub links: Vec<SankeyLink>,
    pub node_map: HashMap<String, usize>,
    pub layout_dirty: bool,
    pub max_depth: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SankeyConfig {
    pub node_width: f32,
    pub node_padding: f32,
    pub iterations: usize,
    pub align: SankeyAlign,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum SankeyAlign {
    Left,
    Right,
    Center,
    Justify,
}

impl Default for SankeyConfig {
    fn default() -> Self {
        Self {
            node_width: 24.0,
            node_padding: 8.0,
            iterations: 6,
            align: SankeyAlign::Justify,
        }
    }
}

impl SankeyState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns node rectangles as (x, y, w, h, color, label)
    pub fn node_rects(&self, w: f32, h: f32) -> Vec<(f32, f32, f32, f32, [f32; 4], &str)> {
        self.nodes
            .iter()
            .map(|node| {
                let x = node.x0 * w;
                let y = node.y0 * h;
                let width = (node.x1 - node.x0) * w;
                let height = (node.y1 - node.y0) * h;
                let color = [
                    node.color[0] as f32 / 255.0,
                    node.color[1] as f32 / 255.0,
                    node.color[2] as f32 / 255.0,
                    node.color[3] as f32 / 255.0,
                ];
                (x, y, width, height, color, node.label.as_str())
            })
            .collect()
    }

    /// Returns flow paths between nodes
    pub fn flow_paths(&self, w: f32, h: f32) -> Vec<SankeyFlowPath> {
        self.links
            .iter()
            .filter_map(|link| {
                let source_idx = self.node_map.get(&link.source)?;
                let target_idx = self.node_map.get(&link.target)?;
                let source = self.nodes.get(*source_idx)?;
                let target = self.nodes.get(*target_idx)?;

                let from_x = source.x1 * w;
                let from_y = (link.y0 + link.width / 2.0) * h;
                let to_x = target.x0 * w;
                let to_y = (link.y1 + link.width / 2.0) * h;

                Some(SankeyFlowPath {
                    from: (from_x, from_y),
                    to: (to_x, to_y),
                    width: link.width * h,
                    color: [
                        link.color[0] as f32 / 255.0,
                        link.color[1] as f32 / 255.0,
                        link.color[2] as f32 / 255.0,
                        link.color[3] as f32 / 255.0,
                    ],
                })
            })
            .collect()
    }

    /// Compute node positions (placeholder - implement full Sankey layout algorithm)
    pub fn layout(&mut self, _w: f32, _h: f32) {
        if self.nodes.is_empty() {
            return;
        }

        // Simple layout: distribute nodes evenly by depth
        let depth_counts = {
            let mut counts = vec![0; self.max_depth + 1];
            for node in &self.nodes {
                counts[node.depth] += 1;
            }
            counts
        };

        let mut depth_positions: Vec<usize> = vec![0; self.max_depth + 1];

        for node in &mut self.nodes {
            let depth = node.depth;
            let x0 = depth as f32 / (self.max_depth as f32 + 1.0);
            let x1 = x0 + 0.05; // Fixed width

            let y_step = 1.0 / (depth_counts[depth] as f32 + 1.0);
            let y0 = (depth_positions[depth] + 1) as f32 * y_step;
            let y1 = y0 + 0.1; // Fixed height

            node.x0 = x0;
            node.x1 = x1;
            node.y0 = y0;
            node.y1 = y1;

            depth_positions[depth] += 1;
        }

        self.layout_dirty = false;
    }

    // Render helpers
    pub fn layout_nodes(&mut self, w: f32, h: f32) {
        self.layout(w, h);
    }

    pub fn layout_links(&self) -> &[SankeyLink] {
        &self.links
    }

    pub fn link_path_points(&self, link_idx: usize, w: f32, h: f32) -> Option<Vec<(f32, f32)>> {
        let link = self.links.get(link_idx)?;
        let source_idx = self.node_map.get(&link.source)?;
        let target_idx = self.node_map.get(&link.target)?;
        let source = self.nodes.get(*source_idx)?;
        let target = self.nodes.get(*target_idx)?;

        let from_x = source.x1 * w;
        let from_y = (link.y0 + link.width / 2.0) * h;
        let to_x = target.x0 * w;
        let to_y = (link.y1 + link.width / 2.0) * h;
        let mid_x = (from_x + to_x) / 2.0;

        Some(vec![
            (from_x, from_y),
            (mid_x, from_y),
            (mid_x, to_y),
            (to_x, to_y),
        ])
    }

    pub fn node_color(&self, node_idx: usize) -> Option<[f32; 4]> {
        let node = self.nodes.get(node_idx)?;
        Some([
            node.color[0] as f32 / 255.0,
            node.color[1] as f32 / 255.0,
            node.color[2] as f32 / 255.0,
            node.color[3] as f32 / 255.0,
        ])
    }

    pub fn format_value(&self, value: f64) -> String {
        if value >= 1_000_000.0 {
            format!("{:.1}M", value / 1_000_000.0)
        } else if value >= 1_000.0 {
            format!("{:.1}K", value / 1_000.0)
        } else {
            format!("{:.1}", value)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SankeyPanel {
    id: SankeyId,
    title: String,
    config: SankeyConfig,
}

impl SankeyPanel {
    pub fn new(id: SankeyId, title: String) -> Self {
        Self {
            id,
            title,
            config: SankeyConfig::default(),
        }
    }

    pub fn id(&self) -> SankeyId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "sankey"
    }

    pub fn kind_label(&self) -> &'static str {
        "Sankey"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
