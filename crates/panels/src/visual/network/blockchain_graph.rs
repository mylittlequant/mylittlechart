use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockchainGraphId(pub u64);

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum NodeType {
    Exchange,
    SmartContract,
    EOA,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct BlockchainNode {
    pub address: String,
    pub label: Option<String>,
    pub balance: f64,
    pub tx_count: usize,
    pub node_type: NodeType,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub radius: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug)]
pub struct BlockchainEdge {
    pub tx_hash: String,
    pub from: String,
    pub to: String,
    pub value: f64,
    pub timestamp: u64,
    pub width: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug, Default)]
pub struct BlockchainGraphState {
    pub nodes: Vec<BlockchainNode>,
    pub edges: Vec<BlockchainEdge>,
    pub node_map: HashMap<String, usize>,
    pub simulation_alpha: f64,
    pub simulation_running: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockchainGraphConfig {
    pub force_strength: f32,
    pub link_distance: f32,
    pub link_strength: f32,
    pub alpha_decay: f64,
    pub velocity_decay: f32,
    pub use_barnes_hut: bool,
}

impl Default for BlockchainGraphConfig {
    fn default() -> Self {
        Self {
            force_strength: -300.0,
            link_distance: 100.0,
            link_strength: 0.5,
            alpha_decay: 0.0228,
            velocity_decay: 0.6,
            use_barnes_hut: true,
        }
    }
}

impl BlockchainGraphState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns node positions as (x, y, radius, color, label)
    pub fn node_positions(&self) -> Vec<(f32, f32, f32, [f32; 4], &str)> {
        self.nodes
            .iter()
            .map(|node| {
                let label = node.label.as_deref().unwrap_or(&node.address);
                let color = [
                    node.color[0] as f32 / 255.0,
                    node.color[1] as f32 / 255.0,
                    node.color[2] as f32 / 255.0,
                    node.color[3] as f32 / 255.0,
                ];
                (node.x, node.y, node.radius, color, label)
            })
            .collect()
    }

    /// Returns edge lines as ((from_x, from_y), (to_x, to_y), width, color)
    pub fn edge_lines(&self) -> Vec<((f32, f32), (f32, f32), f32, [f32; 4])> {
        self.edges
            .iter()
            .filter_map(|edge| {
                let from_idx = self.node_map.get(&edge.from)?;
                let to_idx = self.node_map.get(&edge.to)?;
                let from_node = self.nodes.get(*from_idx)?;
                let to_node = self.nodes.get(*to_idx)?;

                let color = [
                    edge.color[0] as f32 / 255.0,
                    edge.color[1] as f32 / 255.0,
                    edge.color[2] as f32 / 255.0,
                    edge.color[3] as f32 / 255.0,
                ];

                Some((
                    (from_node.x, from_node.y),
                    (to_node.x, to_node.y),
                    edge.width,
                    color,
                ))
            })
            .collect()
    }

    /// Advance force simulation by one timestep
    pub fn tick_simulation(&mut self, dt: f64) {
        if !self.simulation_running {
            return;
        }

        let config = BlockchainGraphConfig::default();

        // Apply repulsion forces (simplified)
        for i in 0..self.nodes.len() {
            for j in (i + 1)..self.nodes.len() {
                let dx = self.nodes[j].x - self.nodes[i].x;
                let dy = self.nodes[j].y - self.nodes[i].y;
                let dist_sq = dx * dx + dy * dy;
                let dist = dist_sq.sqrt().max(1.0);

                let force = config.force_strength / dist_sq;
                let fx = (dx / dist) * force;
                let fy = (dy / dist) * force;

                self.nodes[i].vx -= fx;
                self.nodes[i].vy -= fy;
                self.nodes[j].vx += fx;
                self.nodes[j].vy += fy;
            }
        }

        // Apply spring forces for edges
        for edge in &self.edges {
            if let (Some(&from_idx), Some(&to_idx)) = (
                self.node_map.get(&edge.from),
                self.node_map.get(&edge.to),
            ) {
                let dx = self.nodes[to_idx].x - self.nodes[from_idx].x;
                let dy = self.nodes[to_idx].y - self.nodes[from_idx].y;
                let dist = (dx * dx + dy * dy).sqrt().max(1.0);

                let delta = dist - config.link_distance;
                let force = delta * config.link_strength;
                let fx = (dx / dist) * force;
                let fy = (dy / dist) * force;

                self.nodes[from_idx].vx += fx;
                self.nodes[from_idx].vy += fy;
                self.nodes[to_idx].vx -= fx;
                self.nodes[to_idx].vy -= fy;
            }
        }

        // Update positions
        for node in &mut self.nodes {
            node.vx *= config.velocity_decay;
            node.vy *= config.velocity_decay;
            node.x += node.vx * dt as f32;
            node.y += node.vy * dt as f32;
        }

        // Decay alpha
        self.simulation_alpha *= 1.0 - config.alpha_decay;
        if self.simulation_alpha < 0.001 {
            self.simulation_running = false;
        }
    }

    // Render helpers
    pub fn visible_nodes(&self) -> &[BlockchainNode] {
        &self.nodes
    }

    pub fn visible_edges(&self) -> &[BlockchainEdge] {
        &self.edges
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

    pub fn format_address(&self, address: &str) -> String {
        if address.len() > 10 {
            format!("{}...{}", &address[..6], &address[address.len()-4..])
        } else {
            address.to_string()
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockchainGraphPanel {
    id: BlockchainGraphId,
    title: String,
    config: BlockchainGraphConfig,
}

impl BlockchainGraphPanel {
    pub fn new(id: BlockchainGraphId, title: String) -> Self {
        Self {
            id,
            title,
            config: BlockchainGraphConfig::default(),
        }
    }

    pub fn id(&self) -> BlockchainGraphId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "blockchain_graph"
    }

    pub fn kind_label(&self) -> &'static str {
        "Blockchain Graph"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
