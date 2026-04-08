use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContourPlotId(pub u64);

#[derive(Clone, Debug)]
pub struct ContourPlotState {
    /// Scalar field data (2D grid)
    pub data: Vec<f32>,
    pub width: usize,
    pub height: usize,

    /// Data range
    pub x_range: (f32, f32),
    pub y_range: (f32, f32),
    pub z_range: (f32, f32),

    /// Contour lines
    pub contours: Vec<ContourLine>,

    /// Contour levels
    pub levels: Vec<f32>,

    /// Color fill between contours
    pub filled: bool,
}

#[derive(Clone, Debug)]
pub struct ContourLine {
    pub level: f32,
    pub paths: Vec<Vec<(f32, f32)>>,
    pub color: [f32; 4],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContourPlotConfig {
    /// Number of contour levels
    pub num_levels: usize,

    /// Contour line width
    pub line_width: f32,

    /// Show labels
    pub show_labels: bool,

    /// Fill between contours
    pub filled: bool,

    /// Colormap name
    pub colormap: String,
}

impl Default for ContourPlotState {
    fn default() -> Self {
        Self::new()
    }
}

impl ContourPlotState {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            width: 0,
            height: 0,
            x_range: (0.0, 1.0),
            y_range: (0.0, 1.0),
            z_range: (0.0, 1.0),
            contours: Vec::new(),
            levels: Vec::new(),
            filled: false,
        }
    }

    /// Get contour lines for rendering (polyline, level value, color)
    pub fn contour_lines(&self, w: f32, h: f32) -> Vec<(Vec<(f32, f32)>, f64, [f32; 4])> {
        self.contours
            .iter()
            .flat_map(|contour| {
                contour.paths.iter().map(move |path| {
                    let screen_path: Vec<(f32, f32)> = path
                        .iter()
                        .map(|(x, y)| {
                            let sx = ((x - self.x_range.0) / (self.x_range.1 - self.x_range.0)) * w;
                            let sy = h - ((y - self.y_range.0) / (self.y_range.1 - self.y_range.0)) * h;
                            (sx, sy)
                        })
                        .collect();
                    (screen_path, contour.level as f64, contour.color)
                })
            })
            .collect()
    }

    /// Compute contours using marching squares (simplified)
    pub fn compute_contours(&mut self) {
        if self.data.is_empty() || self.width == 0 || self.height == 0 {
            return;
        }

        self.contours.clear();

        // Generate levels if not set
        if self.levels.is_empty() {
            let num_levels = 10;
            for i in 0..num_levels {
                let t = i as f32 / (num_levels - 1) as f32;
                let level = self.z_range.0 + t * (self.z_range.1 - self.z_range.0);
                self.levels.push(level);
            }
        }

        // Simplified marching squares for each level
        for (level_idx, &level) in self.levels.iter().enumerate() {
            let mut paths = Vec::new();

            // Scan grid cells
            for y in 0..self.height.saturating_sub(1) {
                for x in 0..self.width.saturating_sub(1) {
                    let idx_tl = y * self.width + x;
                    let idx_tr = y * self.width + (x + 1);
                    let idx_bl = (y + 1) * self.width + x;
                    let idx_br = (y + 1) * self.width + (x + 1);

                    if idx_br >= self.data.len() {
                        continue;
                    }

                    let v_tl = self.data[idx_tl];
                    let v_tr = self.data[idx_tr];
                    let v_bl = self.data[idx_bl];
                    let v_br = self.data[idx_br];

                    // Marching squares case
                    let case = ((v_tl >= level) as u8) << 3
                        | ((v_tr >= level) as u8) << 2
                        | ((v_br >= level) as u8) << 1
                        | ((v_bl >= level) as u8);

                    if case != 0 && case != 15 {
                        // Simplified: create a small line segment for this cell
                        let x0 = self.x_range.0 + (x as f32 / self.width as f32) * (self.x_range.1 - self.x_range.0);
                        let y0 = self.y_range.0 + (y as f32 / self.height as f32) * (self.y_range.1 - self.y_range.0);
                        let dx = (self.x_range.1 - self.x_range.0) / self.width as f32;
                        let dy = (self.y_range.1 - self.y_range.0) / self.height as f32;

                        let path = vec![
                            (x0 + dx * 0.5, y0),
                            (x0 + dx, y0 + dy * 0.5),
                        ];
                        paths.push(path);
                    }
                }
            }

            let t = level_idx as f32 / self.levels.len().max(1) as f32;
            let color = [t, 0.5, 1.0 - t, 1.0];
            self.contours.push(ContourLine { level, paths, color });
        }
    }

    // Render helpers
    pub fn level_color(&self, level_idx: usize) -> [f32; 4] {
        if level_idx < self.contours.len() {
            self.contours[level_idx].color
        } else {
            let t = level_idx as f32 / self.levels.len().max(1) as f32;
            [t, 0.5, 1.0 - t, 1.0]
        }
    }

    pub fn grid_value_at(&self, x: usize, y: usize) -> Option<f32> {
        if x < self.width && y < self.height {
            let idx = y * self.width + x;
            self.data.get(idx).copied()
        } else {
            None
        }
    }
}

impl Default for ContourPlotConfig {
    fn default() -> Self {
        Self {
            num_levels: 10,
            line_width: 1.5,
            show_labels: true,
            filled: true,
            colormap: "viridis".to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContourPlotPanel {
    id: ContourPlotId,
    title: String,
}

impl ContourPlotPanel {
    pub fn new(id: ContourPlotId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> ContourPlotId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "contour_plot"
    }

    pub fn kind_label(&self) -> &'static str {
        "Contour Plot"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 300.0)
    }
}
