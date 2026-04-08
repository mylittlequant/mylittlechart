use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TreemapId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct TreemapState {
    pub root: TreeNode,
    pub layout: Vec<TreemapRect>,  // Flattened layout for rendering
    pub hover_path: Vec<String>,  // Hierarchy path of hovered item
    pub zoom_stack: Vec<String>,  // For drill-down navigation
}

#[derive(Clone, Debug, Default)]
pub struct TreeNode {
    pub id: String,
    pub label: String,
    pub value: f64,  // Market cap
    pub change_pct: f64,
    pub children: Vec<TreeNode>,
}

#[derive(Clone, Debug)]
pub struct TreemapRect {
    pub node_id: String,
    pub rect: (f32, f32, f32, f32),  // (x0, y0, x1, y1)
    pub depth: usize,
    pub color: [f32; 4],  // RGBA
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreemapConfig {
    pub color_scale: DivergingColorScale,
    pub border_width: f32,
    pub border_color: [f32; 4],  // RGBA
    pub label_threshold: f64,  // Min area to show label (in pixels²)
    pub padding: f32,  // Padding between nested levels
    pub tile_algorithm: TileAlgorithm,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DivergingColorScale {
    pub negative: [f32; 3],  // OKLCH for -5%
    pub neutral: [f32; 3],   // OKLCH for 0%
    pub positive: [f32; 3],  // OKLCH for +5%
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TileAlgorithm {
    Squarify,
    Slice,
    Dice,
    SliceDice,
}

impl TreemapState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns layout rectangles with x, y, w, h coordinates
    pub fn layout_rects(&self) -> Vec<TreemapRectOutput> {
        self.layout.iter().map(|rect| {
            TreemapRectOutput {
                x: rect.rect.0,
                y: rect.rect.1,
                w: rect.rect.2 - rect.rect.0,
                h: rect.rect.3 - rect.rect.1,
                label: rect.node_id.clone(),
                value: 0.0,
                color: rect.color,
            }
        }).collect()
    }

    /// Returns color for a node based on its change percentage
    pub fn node_color(&self, change_pct: f64) -> [f32; 4] {
        let clamped = change_pct.clamp(-5.0, 5.0);
        let norm = (clamped + 5.0) / 10.0; // Normalize to 0..1

        if norm < 0.5 {
            // Negative: interpolate from red to neutral
            let t = (norm * 2.0) as f32;
            [0.60 - t * 0.10, 0.18 - t * 0.18, 25.0 + t * (-25.0), 1.0]
        } else {
            // Positive: interpolate from neutral to green
            let t = ((norm - 0.5) * 2.0) as f32;
            [0.5 + t * 0.15, t * 0.15, t * 145.0, 1.0]
        }
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
}

#[derive(Clone, Debug)]
pub struct TreemapRectOutput {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub label: String,
    pub value: f64,
    pub color: [f32; 4],
}

impl Default for TreemapConfig {
    fn default() -> Self {
        Self {
            color_scale: DivergingColorScale {
                negative: [0.60, 0.18, 25.0],   // Red
                neutral: [0.5, 0.0, 0.0],       // Gray
                positive: [0.65, 0.15, 145.0],  // Green
            },
            border_width: 2.0,
            border_color: [0.2, 0.2, 0.2, 1.0],
            label_threshold: 1000.0,
            padding: 2.0,
            tile_algorithm: TileAlgorithm::Squarify,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreemapPanel {
    id: TreemapId,
    title: String,
}

impl TreemapPanel {
    pub fn new(id: TreemapId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> TreemapId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "treemap"
    }

    pub fn kind_label(&self) -> &'static str {
        "Treemap"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
