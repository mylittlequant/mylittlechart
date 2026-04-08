use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BubbleChartId(pub u64);

#[derive(Clone, Debug)]
pub struct Bubble {
    pub id: String,
    pub label: String,
    pub x: f64,
    pub y: f64,
    pub r: f64,
    pub category: Option<String>,
    pub screen_x: f32,
    pub screen_y: f32,
    pub screen_r: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug, Default)]
pub struct BubbleChartState {
    pub bubbles: Vec<Bubble>,
    pub x_range: (f64, f64),
    pub y_range: (f64, f64),
    pub r_range: (f64, f64),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BubbleChartConfig {
    pub x_label: String,
    pub y_label: String,
    pub r_label: String,
    pub min_radius: f32,
    pub max_radius: f32,
}

impl Default for BubbleChartConfig {
    fn default() -> Self {
        Self {
            x_label: String::from("X"),
            y_label: String::from("Y"),
            r_label: String::from("Size"),
            min_radius: 5.0,
            max_radius: 50.0,
        }
    }
}

impl BubbleChartState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns bubble circles as (x, y, r, color, label)
    pub fn bubble_circles(&self, w: f32, h: f32) -> Vec<(f32, f32, f32, [f32; 4], &str)> {
        self.bubbles
            .iter()
            .map(|bubble| {
                let x = self.x_to_screen(bubble.x, w);
                let y = self.y_to_screen(bubble.y, h);
                let color = [
                    bubble.color[0] as f32 / 255.0,
                    bubble.color[1] as f32 / 255.0,
                    bubble.color[2] as f32 / 255.0,
                    bubble.color[3] as f32 / 255.0,
                ];
                (x, y, bubble.screen_r, color, bubble.label.as_str())
            })
            .collect()
    }

    /// Convert data x to screen x
    pub fn x_to_screen(&self, val: f64, w: f32) -> f32 {
        let (min, max) = self.x_range;
        if max > min {
            (((val - min) / (max - min)) as f32 * w).clamp(0.0, w)
        } else {
            w / 2.0
        }
    }

    /// Convert data y to screen y
    pub fn y_to_screen(&self, val: f64, h: f32) -> f32 {
        let (min, max) = self.y_range;
        if max > min {
            (h - ((val - min) / (max - min)) as f32 * h).clamp(0.0, h)
        } else {
            h / 2.0
        }
    }

    // Render helpers
    pub fn visible_bubbles(&self) -> &[Bubble] {
        &self.bubbles
    }

    pub fn bubble_color(&self, bubble_idx: usize) -> Option<[f32; 4]> {
        let bubble = self.bubbles.get(bubble_idx)?;
        Some([
            bubble.color[0] as f32 / 255.0,
            bubble.color[1] as f32 / 255.0,
            bubble.color[2] as f32 / 255.0,
            bubble.color[3] as f32 / 255.0,
        ])
    }

    pub fn format_bubble(&self, bubble_idx: usize) -> String {
        if let Some(bubble) = self.bubbles.get(bubble_idx) {
            format!("{}: ({:.1}, {:.1}) r={:.1}", bubble.label, bubble.x, bubble.y, bubble.r)
        } else {
            String::from("N/A")
        }
    }

    pub fn x_range(&self) -> (f64, f64) {
        self.x_range
    }

    pub fn y_range(&self) -> (f64, f64) {
        self.y_range
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BubbleChartPanel {
    id: BubbleChartId,
    title: String,
    config: BubbleChartConfig,
}

impl BubbleChartPanel {
    pub fn new(id: BubbleChartId, title: String) -> Self {
        Self {
            id,
            title,
            config: BubbleChartConfig::default(),
        }
    }

    pub fn id(&self) -> BubbleChartId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "bubble_chart"
    }

    pub fn kind_label(&self) -> &'static str {
        "Bubble Chart"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 300.0)
    }
}
