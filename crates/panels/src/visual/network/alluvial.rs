use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AlluvialId(pub u64);

#[derive(Clone, Debug)]
pub struct AlluvialBlock {
    pub column: usize,
    pub category: String,
    pub value: f64,
    pub y0: f32,
    pub y1: f32,
    pub x0: f32,
    pub x1: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug)]
pub struct AlluvialFlow {
    pub source_column: usize,
    pub target_column: usize,
    pub source_category: String,
    pub target_category: String,
    pub value: f64,
    pub source_y0: f32,
    pub source_y1: f32,
    pub target_y0: f32,
    pub target_y1: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug, Default)]
pub struct AlluvialState {
    pub blocks: Vec<AlluvialBlock>,
    pub flows: Vec<AlluvialFlow>,
    pub columns: Vec<String>,
    pub categories: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AlluvialConfig {
    pub block_width: f32,
    pub block_spacing: f32,
    pub column_spacing: f32,
}

impl Default for AlluvialConfig {
    fn default() -> Self {
        Self {
            block_width: 40.0,
            block_spacing: 8.0,
            column_spacing: 80.0,
        }
    }
}

impl AlluvialState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns column blocks as (x, y, w, h, color, label)
    pub fn column_blocks(&self, w: f32, h: f32) -> Vec<(f32, f32, f32, f32, [f32; 4], &str)> {
        self.blocks
            .iter()
            .map(|block| {
                let x = block.x0 * w;
                let y = block.y0 * h;
                let width = (block.x1 - block.x0) * w;
                let height = (block.y1 - block.y0) * h;
                let color = [
                    block.color[0] as f32 / 255.0,
                    block.color[1] as f32 / 255.0,
                    block.color[2] as f32 / 255.0,
                    block.color[3] as f32 / 255.0,
                ];
                (x, y, width, height, color, block.category.as_str())
            })
            .collect()
    }

    /// Returns flow curves as ((from_x, from_y), (to_x, to_y), width, color)
    pub fn flow_curves(&self, w: f32, h: f32) -> Vec<((f32, f32), (f32, f32), f32, [f32; 4])> {
        self.flows
            .iter()
            .filter_map(|flow| {
                let source_block = self.blocks.iter().find(|b| {
                    b.column == flow.source_column && b.category == flow.source_category
                })?;
                let target_block = self.blocks.iter().find(|b| {
                    b.column == flow.target_column && b.category == flow.target_category
                })?;

                let from_x = source_block.x1 * w;
                let from_y = ((flow.source_y0 + flow.source_y1) / 2.0) * h;
                let to_x = target_block.x0 * w;
                let to_y = ((flow.target_y0 + flow.target_y1) / 2.0) * h;
                let width = (flow.source_y1 - flow.source_y0) * h;

                let color = [
                    flow.color[0] as f32 / 255.0,
                    flow.color[1] as f32 / 255.0,
                    flow.color[2] as f32 / 255.0,
                    flow.color[3] as f32 / 255.0,
                ];

                Some(((from_x, from_y), (to_x, to_y), width, color))
            })
            .collect()
    }

    // Render helpers
    pub fn visible_columns(&self) -> &[String] {
        &self.columns
    }

    pub fn visible_flows(&self) -> &[AlluvialFlow] {
        &self.flows
    }

    pub fn flow_color(&self, flow_idx: usize) -> Option<[f32; 4]> {
        let flow = self.flows.get(flow_idx)?;
        Some([
            flow.color[0] as f32 / 255.0,
            flow.color[1] as f32 / 255.0,
            flow.color[2] as f32 / 255.0,
            flow.color[3] as f32 / 255.0,
        ])
    }

    pub fn column_x(&self, column_idx: usize, w: f32) -> f32 {
        if self.columns.is_empty() {
            return 0.0;
        }
        let spacing = w / (self.columns.len().max(1) as f32);
        column_idx as f32 * spacing
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AlluvialPanel {
    id: AlluvialId,
    title: String,
    config: AlluvialConfig,
}

impl AlluvialPanel {
    pub fn new(id: AlluvialId, title: String) -> Self {
        Self {
            id,
            title,
            config: AlluvialConfig::default(),
        }
    }

    pub fn id(&self) -> AlluvialId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "alluvial"
    }

    pub fn kind_label(&self) -> &'static str {
        "Alluvial"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
