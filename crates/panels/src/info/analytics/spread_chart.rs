use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpreadChartId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct SpreadChartState {
    pub instrument_a: String,
    pub instrument_b: String,
    pub spread_data: Vec<(i64, f64)>,  // (timestamp, spread_value)
    pub spread_type: SpreadType,
    pub distribution: Vec<(f64, u32)>,  // (spread_value, frequency)
    pub stats: SpreadStatistics,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SpreadType {
    Absolute,  // A - B
    Ratio,     // A / B
    Percentage, // (A - B) / B * 100
}

#[derive(Clone, Debug, Default)]
pub struct SpreadStatistics {
    pub mean: f64,
    pub std_dev: f64,
    pub z_score_current: f64,
    pub correlation: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpreadChartConfig {
    pub chart_height_ratio: f32,  // 0.7 = 70% for line chart, 30% for histogram
    pub line_color: [f32; 3],  // OKLCH
    pub mean_line_color: [f32; 3],  // OKLCH
    pub std_dev_bands: bool,  // Show ±1, ±2 std dev bands
    pub histogram_bins: usize,  // Number of bins for distribution
    pub histogram_color: [f32; 3],  // OKLCH
}

impl Default for SpreadType {
    fn default() -> Self {
        Self::Absolute
    }
}

impl SpreadChartState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns spread line points in screen coordinates
    pub fn spread_points(&self, w: f32, h: f32) -> Vec<(f32, f32)> {
        if self.spread_data.is_empty() {
            return Vec::new();
        }

        let (min_time, max_time) = self.spread_data.iter()
            .fold((i64::MAX, i64::MIN), |(min, max), (t, _)| {
                (min.min(*t), max.max(*t))
            });

        let (min_spread, max_spread) = self.spread_data.iter()
            .fold((f64::MAX, f64::MIN), |(min, max), (_, s)| {
                (min.min(*s), max.max(*s))
            });

        let time_range = (max_time - min_time) as f64;
        let spread_range = max_spread - min_spread;

        if time_range == 0.0 || spread_range == 0.0 {
            return Vec::new();
        }

        self.spread_data.iter()
            .map(|(time, spread)| {
                let x = (((*time - min_time) as f64) / time_range) as f32 * w;
                let y = ((1.0 - (spread - min_spread) / spread_range) * h as f64) as f32;
                (x.clamp(0.0, w), y.clamp(0.0, h))
            })
            .collect()
    }

    /// Returns the Y coordinate for the zero line
    pub fn zero_line_y(&self, h: f32) -> f32 {
        if self.spread_data.is_empty() {
            return h / 2.0;
        }

        let (min_spread, max_spread) = self.spread_data.iter()
            .fold((f64::MAX, f64::MIN), |(min, max), (_, s)| {
                (min.min(*s), max.max(*s))
            });

        let spread_range = max_spread - min_spread;
        if spread_range == 0.0 {
            return h / 2.0;
        }

        let zero_norm = (0.0 - min_spread) / spread_range;
        ((1.0 - zero_norm) * h as f64) as f32
    }

    /// Format spread value for display
    pub fn format_spread(&self, spread_value: f64) -> String {
        match self.spread_type {
            SpreadType::Absolute => format!("{:.4}", spread_value),
            SpreadType::Ratio => format!("{:.3}", spread_value),
            SpreadType::Percentage => format!("{:+.2}%", spread_value),
        }
    }

    /// Get color for spread based on z-score
    pub fn spread_color(&self) -> [f32; 4] {
        let z_score = self.stats.z_score_current;
        if z_score.abs() > 2.0 {
            [0.9, 0.2, 0.2, 1.0] // red - extreme
        } else if z_score.abs() > 1.0 {
            [0.9, 0.7, 0.2, 1.0] // yellow - notable
        } else {
            [0.6, 0.6, 0.7, 1.0] // neutral
        }
    }
}

impl Default for SpreadChartConfig {
    fn default() -> Self {
        Self {
            chart_height_ratio: 0.7,
            line_color: [0.6, 0.2, 240.0],       // Blue
            mean_line_color: [0.7, 0.2, 60.0],   // Yellow
            std_dev_bands: true,
            histogram_bins: 50,
            histogram_color: [0.6, 0.15, 240.0], // Blue
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpreadChartPanel {
    id: SpreadChartId,
    title: String,
}

impl SpreadChartPanel {
    pub fn new(id: SpreadChartId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> SpreadChartId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "spread_chart"
    }

    pub fn kind_label(&self) -> &'static str {
        "Spread Chart"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 200.0)
    }
}
