use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ForceGraphId(pub u64);

#[derive(Clone, Debug)]
pub struct ForceNode {
    pub id: String,
    pub label: String,
    pub group: Option<String>,
    pub weight: f64,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub fx: Option<f32>,
    pub fy: Option<f32>,
    pub radius: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug)]
pub struct ForceEdge {
    pub source: String,
    pub target: String,
    pub weight: f64,
    pub color: [u8; 4],
}

#[derive(Clone, Debug, Default)]
pub struct ForceGraphState {
    pub nodes: Vec<ForceNode>,
    pub edges: Vec<ForceEdge>,
    pub node_map: HashMap<String, usize>,
    pub simulation_alpha: f64,
    pub simulation_running: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForceGraphConfig {
    pub center_force: f32,
    pub charge_force: f32,
    pub link_distance: f32,
    pub link_strength: f32,
    pub collision_radius: f32,
    pub alpha_min: f64,
    pub alpha_decay: f64,
    pub velocity_decay: f32,
}

impl Default for ForceGraphConfig {
    fn default() -> Self {
        Self {
            center_force: 0.1,
            charge_force: -30.0,
            link_distance: 30.0,
            link_strength: 1.0,
            collision_radius: 1.5,
            alpha_min: 0.001,
            alpha_decay: 0.0228,
            velocity_decay: 0.4,
        }
    }
}

impl ForceGraphState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns node circles as (x, y, radius, color)
    pub fn node_circles(&self) -> Vec<(f32, f32, f32, [f32; 4])> {
        self.nodes
            .iter()
            .map(|node| {
                let color = [
                    node.color[0] as f32 / 255.0,
                    node.color[1] as f32 / 255.0,
                    node.color[2] as f32 / 255.0,
                    node.color[3] as f32 / 255.0,
                ];
                (node.x, node.y, node.radius, color)
            })
            .collect()
    }

    /// Returns edge lines as ((from_x, from_y), (to_x, to_y), color)
    pub fn edge_lines(&self) -> Vec<((f32, f32), (f32, f32), [f32; 4])> {
        self.edges
            .iter()
            .filter_map(|edge| {
                let from_idx = self.node_map.get(&edge.source)?;
                let to_idx = self.node_map.get(&edge.target)?;
                let from_node = self.nodes.get(*from_idx)?;
                let to_node = self.nodes.get(*to_idx)?;

                let color = [
                    edge.color[0] as f32 / 255.0,
                    edge.color[1] as f32 / 255.0,
                    edge.color[2] as f32 / 255.0,
                    edge.color[3] as f32 / 255.0,
                ];

                Some(((from_node.x, from_node.y), (to_node.x, to_node.y), color))
            })
            .collect()
    }

    /// Advance physics simulation
    pub fn tick(&mut self, dt: f64) {
        if !self.simulation_running {
            return;
        }

        self.apply_forces();

        let config = ForceGraphConfig::default();

        // Update positions
        for node in &mut self.nodes {
            if node.fx.is_none() {
                node.vx *= config.velocity_decay;
                node.x += node.vx * dt as f32;
            } else {
                node.x = node.fx.unwrap();
                node.vx = 0.0;
            }

            if node.fy.is_none() {
                node.vy *= config.velocity_decay;
                node.y += node.vy * dt as f32;
            } else {
                node.y = node.fy.unwrap();
                node.vy = 0.0;
            }
        }

        // Decay alpha
        self.simulation_alpha *= 1.0 - config.alpha_decay;
        if self.simulation_alpha < config.alpha_min {
            self.simulation_running = false;
        }
    }

    /// Apply forces for one iteration
    pub fn apply_forces(&mut self) {
        let config = ForceGraphConfig::default();

        // Center force
        let mut cx = 0.0;
        let mut cy = 0.0;
        for node in &self.nodes {
            cx += node.x;
            cy += node.y;
        }
        if !self.nodes.is_empty() {
            cx /= self.nodes.len() as f32;
            cy /= self.nodes.len() as f32;
        }

        for node in &mut self.nodes {
            if node.fx.is_none() {
                node.vx += (0.0 - cx) * config.center_force;
            }
            if node.fy.is_none() {
                node.vy += (0.0 - cy) * config.center_force;
            }
        }

        // Charge force (repulsion)
        for i in 0..self.nodes.len() {
            for j in (i + 1)..self.nodes.len() {
                let dx = self.nodes[j].x - self.nodes[i].x;
                let dy = self.nodes[j].y - self.nodes[i].y;
                let dist_sq = dx * dx + dy * dy;
                let dist = dist_sq.sqrt().max(1.0);

                let force = config.charge_force / dist_sq;
                let fx = (dx / dist) * force;
                let fy = (dy / dist) * force;

                if self.nodes[i].fx.is_none() {
                    self.nodes[i].vx -= fx;
                }
                if self.nodes[i].fy.is_none() {
                    self.nodes[i].vy -= fy;
                }
                if self.nodes[j].fx.is_none() {
                    self.nodes[j].vx += fx;
                }
                if self.nodes[j].fy.is_none() {
                    self.nodes[j].vy += fy;
                }
            }
        }

        // Link force (spring)
        for edge in &self.edges {
            if let (Some(&from_idx), Some(&to_idx)) = (
                self.node_map.get(&edge.source),
                self.node_map.get(&edge.target),
            ) {
                let dx = self.nodes[to_idx].x - self.nodes[from_idx].x;
                let dy = self.nodes[to_idx].y - self.nodes[from_idx].y;
                let dist = (dx * dx + dy * dy).sqrt().max(1.0);

                let delta = dist - config.link_distance;
                let force = delta * config.link_strength;
                let fx = (dx / dist) * force;
                let fy = (dy / dist) * force;

                if self.nodes[from_idx].fx.is_none() {
                    self.nodes[from_idx].vx += fx;
                }
                if self.nodes[from_idx].fy.is_none() {
                    self.nodes[from_idx].vy += fy;
                }
                if self.nodes[to_idx].fx.is_none() {
                    self.nodes[to_idx].vx -= fx;
                }
                if self.nodes[to_idx].fy.is_none() {
                    self.nodes[to_idx].vy -= fy;
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForceGraphPanel {
    id: ForceGraphId,
    title: String,
    config: ForceGraphConfig,
}

impl ForceGraphPanel {
    pub fn new(id: ForceGraphId, title: String) -> Self {
        Self {
            id,
            title,
            config: ForceGraphConfig::default(),
        }
    }

    pub fn id(&self) -> ForceGraphId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "force_graph"
    }

    pub fn kind_label(&self) -> &'static str {
        "Force Graph"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
