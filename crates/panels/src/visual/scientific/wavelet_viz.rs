use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WaveletVizId(pub u64);

#[derive(Clone, Debug)]
pub struct WaveletVizState {
    /// Original signal
    pub signal: Vec<f32>,
    pub sample_rate: f32,

    /// Scalogram (time-frequency decomposition)
    pub scalogram: Vec<Vec<f32>>,

    /// Wavelet type
    pub wavelet: WaveletType,

    /// Scale range
    pub scale_range: (f32, f32),

    /// Colormap name
    pub colormap: String,
}

#[derive(Clone, Copy, Debug)]
pub enum WaveletType {
    Morlet,
    MexicanHat,
    Haar,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WaveletVizConfig {
    /// Number of scales
    pub num_scales: usize,

    /// Colormap
    pub colormap: String,

    /// Show original signal
    pub show_signal: bool,

    /// Log scale for frequency axis
    pub log_frequency: bool,
}

impl Default for WaveletVizState {
    fn default() -> Self {
        Self::new()
    }
}

impl WaveletVizState {
    pub fn new() -> Self {
        Self {
            signal: Vec::new(),
            sample_rate: 1000.0,
            scalogram: Vec::new(),
            wavelet: WaveletType::Morlet,
            scale_range: (1.0, 100.0),
            colormap: "viridis".to_string(),
        }
    }

    /// Get scalogram cells for rendering (x, y, cell_width, cell_height, color)
    pub fn scalogram_cells(&self, w: f32, h: f32) -> Vec<(f32, f32, f32, f32, [f32; 4])> {
        if self.scalogram.is_empty() {
            return Vec::new();
        }

        let num_scales = self.scalogram.len();
        let num_times = self.scalogram[0].len();

        if num_times == 0 {
            return Vec::new();
        }

        let cell_w = w / num_times as f32;
        let cell_h = h / num_scales as f32;

        // Find max magnitude for normalization
        let max_mag = self
            .scalogram
            .iter()
            .flat_map(|row| row.iter())
            .map(|&v| v.abs())
            .fold(0.0f32, f32::max);

        let mut cells = Vec::new();

        for (scale_idx, row) in self.scalogram.iter().enumerate() {
            for (time_idx, &magnitude) in row.iter().enumerate() {
                let x = time_idx as f32 * cell_w;
                let y = scale_idx as f32 * cell_h;
                let color = Self::magnitude_color(magnitude as f64, max_mag as f64);
                cells.push((x, y, cell_w, cell_h, color));
            }
        }

        cells
    }

    /// Map magnitude to color using viridis-like colormap
    pub fn magnitude_color(mag: f64, max: f64) -> [f32; 4] {
        let t = if max > 0.0 {
            (mag.abs() / max).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Simplified viridis: purple -> blue -> green -> yellow
        let r = if t < 0.5 {
            0.3 * t as f32 * 2.0
        } else {
            0.3 + (t as f32 - 0.5) * 2.0 * 0.7
        };

        let g = if t < 0.3 {
            0.2 * t as f32
        } else if t < 0.7 {
            0.2 + (t as f32 - 0.3) * 0.6
        } else {
            0.8 + (t as f32 - 0.7) * 0.2
        };

        let b = if t < 0.5 {
            0.8 - t as f32 * 0.4
        } else {
            0.6 - (t as f32 - 0.5) * 1.2
        };

        [r, g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), 1.0]
    }

    // Render helpers
    pub fn visible_scalogram_cells(&self, w: f32, h: f32) -> Vec<(f32, f32, f32, f32, [f32; 4])> {
        self.scalogram_cells(w, h)
    }

    pub fn frequency_color(&self, frequency: f32) -> [f32; 4] {
        let t = (frequency - self.scale_range.0) / (self.scale_range.1 - self.scale_range.0);
        let t = t.clamp(0.0, 1.0);
        [t, 0.5, 1.0 - t, 1.0]
    }

    pub fn time_to_x(&self, time_idx: usize, w: f32) -> f32 {
        if self.signal.is_empty() {
            return 0.0;
        }
        (time_idx as f32 / self.signal.len() as f32) * w
    }

    pub fn scale_to_y(&self, scale: f32, h: f32) -> f32 {
        let t = (scale - self.scale_range.0) / (self.scale_range.1 - self.scale_range.0);
        (1.0 - t.clamp(0.0, 1.0)) * h
    }
}

impl Default for WaveletVizConfig {
    fn default() -> Self {
        Self {
            num_scales: 64,
            colormap: "viridis".to_string(),
            show_signal: true,
            log_frequency: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WaveletVizPanel {
    id: WaveletVizId,
    title: String,
}

impl WaveletVizPanel {
    pub fn new(id: WaveletVizId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> WaveletVizId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "wavelet_viz"
    }

    pub fn kind_label(&self) -> &'static str {
        "Wavelet"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
