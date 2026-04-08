use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FlameGraphId(pub u64);

#[derive(Clone, Debug)]
pub struct FlameFrame {
    pub name: String,
    pub value: u64,
    pub depth: usize,
    pub x: f32,
    pub width: f32,
    pub y: f32,
    pub height: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug, Default)]
pub struct FlameGraphState {
    pub frames: Vec<FlameFrame>,
    pub total_value: u64,
    pub max_depth: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FlameGraphConfig {
    pub frame_height: f32,
    pub frame_padding: f32,
    pub sort_alphabetically: bool,
}

impl Default for FlameGraphConfig {
    fn default() -> Self {
        Self {
            frame_height: 18.0,
            frame_padding: 1.0,
            sort_alphabetically: true,
        }
    }
}

impl FlameGraphState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns frame rectangles as (x, y, w, h, color, label)
    pub fn frame_rects(&self, w: f32, _h: f32) -> Vec<(f32, f32, f32, f32, [f32; 4], &str)> {
        self.frames
            .iter()
            .map(|frame| {
                let x = frame.x * w;
                let y = frame.y;
                let width = frame.width * w;
                let height = frame.height;
                let color = [
                    frame.color[0] as f32 / 255.0,
                    frame.color[1] as f32 / 255.0,
                    frame.color[2] as f32 / 255.0,
                    frame.color[3] as f32 / 255.0,
                ];
                (x, y, width, height, color, frame.name.as_str())
            })
            .collect()
    }

    /// Compute x positions from frame timing
    pub fn layout(&mut self, _w: f32, _h: f32) {
        if self.frames.is_empty() {
            return;
        }

        let config = FlameGraphConfig::default();

        // Group by depth
        let mut depth_frames: Vec<Vec<usize>> = vec![Vec::new(); self.max_depth + 1];
        for (idx, frame) in self.frames.iter().enumerate() {
            depth_frames[frame.depth].push(idx);
        }

        // Layout each depth
        for depth in 0..=self.max_depth {
            let frames_at_depth = &depth_frames[depth];
            if frames_at_depth.is_empty() {
                continue;
            }

            let mut x_offset = 0.0;
            let y = (self.max_depth - depth) as f32 * (config.frame_height + config.frame_padding);

            for &idx in frames_at_depth {
                let frame = &mut self.frames[idx];
                let width_ratio = if self.total_value > 0 {
                    frame.value as f64 / self.total_value as f64
                } else {
                    1.0 / frames_at_depth.len() as f64
                };

                frame.x = x_offset as f32;
                frame.width = width_ratio as f32;
                frame.y = y;
                frame.height = config.frame_height;

                x_offset += width_ratio;
            }
        }
    }

    // Render helpers
    pub fn visible_frames(&self) -> &[FlameFrame] {
        &self.frames
    }

    pub fn frame_color(&self, frame_idx: usize) -> Option<[f32; 4]> {
        let frame = self.frames.get(frame_idx)?;
        Some([
            frame.color[0] as f32 / 255.0,
            frame.color[1] as f32 / 255.0,
            frame.color[2] as f32 / 255.0,
            frame.color[3] as f32 / 255.0,
        ])
    }

    pub fn format_frame(&self, frame_idx: usize) -> String {
        if let Some(frame) = self.frames.get(frame_idx) {
            if self.total_value > 0 {
                let pct = (frame.value as f64 / self.total_value as f64) * 100.0;
                format!("{} ({:.1}%)", frame.name, pct)
            } else {
                frame.name.clone()
            }
        } else {
            String::from("N/A")
        }
    }

    pub fn search_matches(&self, query: &str) -> Vec<usize> {
        self.frames
            .iter()
            .enumerate()
            .filter_map(|(i, frame)| {
                if frame.name.to_lowercase().contains(&query.to_lowercase()) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FlameGraphPanel {
    id: FlameGraphId,
    title: String,
    config: FlameGraphConfig,
}

impl FlameGraphPanel {
    pub fn new(id: FlameGraphId, title: String) -> Self {
        Self {
            id,
            title,
            config: FlameGraphConfig::default(),
        }
    }

    pub fn id(&self) -> FlameGraphId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "flame_graph"
    }

    pub fn kind_label(&self) -> &'static str {
        "Flame Graph"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 200.0)
    }
}
