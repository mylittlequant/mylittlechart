use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HorizonChartId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct HorizonChartState {
    pub series: Vec<TimeSeries>,
    pub time_range: (i64, i64),  // Unix timestamps
    pub value_range: (f64, f64),
    pub bands: usize,  // Number of bands (typically 2-4)
    pub selected_series: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct TimeSeries {
    pub id: String,
    pub label: String,
    pub data: Vec<(i64, f64)>,  // (timestamp, value)
    pub baseline: f64,  // Value considered "zero"
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HorizonChartConfig {
    pub row_height: f32,  // Height of each series row
    pub bands: usize,     // Number of bands (2-4 typical)
    pub positive_colors: Vec<[f32; 3]>,  // OKLCH colors for positive bands
    pub negative_colors: Vec<[f32; 3]>,  // OKLCH colors for negative bands
    pub mirror_negatives: bool,  // If true, flip negatives upward
    pub show_baseline: bool,
}

impl HorizonChartState {
    pub fn new() -> Self {
        Self {
            bands: 3,
            ..Default::default()
        }
    }

    /// Returns band data for a specific series with deviation from baseline
    pub fn band_data(&self, series_idx: usize) -> Vec<(i64, f64, usize, bool)> {
        if series_idx >= self.series.len() {
            return Vec::new();
        }

        let series = &self.series[series_idx];
        if series.data.is_empty() {
            return Vec::new();
        }

        let (min_val, max_val) = series.data.iter()
            .fold((f64::MAX, f64::MIN), |(min, max), (_, val)| {
                (min.min(*val), max.max(*val))
            });

        let value_range = max_val - min_val;
        if value_range == 0.0 {
            return Vec::new();
        }

        series.data.iter()
            .map(|(timestamp, value)| {
                let deviation = value - series.baseline;
                let norm_deviation = (deviation / value_range).clamp(-1.0, 1.0);
                let band_idx = ((norm_deviation.abs() * self.bands as f64) as usize).min(self.bands - 1);
                let is_positive = deviation >= 0.0;
                (*timestamp, deviation, band_idx, is_positive)
            })
            .collect()
    }

    /// Returns color for a specific band index and polarity
    pub fn band_color(&self, band_idx: usize, is_positive: bool) -> [f32; 4] {
        let band_idx = band_idx.min(self.bands - 1);
        let chroma = 0.10 + band_idx as f32 * 0.05;

        if is_positive {
            [0.65, chroma, 145.0, 1.0] // Green gradient
        } else {
            [0.65, chroma, 25.0, 1.0] // Red gradient
        }
    }

    /// Splits data into positive and negative components
    pub fn positive_negative_split(&self, series_idx: usize) -> (Vec<(i64, f64)>, Vec<(i64, f64)>) {
        if series_idx >= self.series.len() {
            return (Vec::new(), Vec::new());
        }

        let series = &self.series[series_idx];
        let mut positive = Vec::new();
        let mut negative = Vec::new();

        for (timestamp, value) in &series.data {
            let deviation = value - series.baseline;
            if deviation >= 0.0 {
                positive.push((*timestamp, deviation));
            } else {
                negative.push((*timestamp, deviation));
            }
        }

        (positive, negative)
    }
}

impl Default for HorizonChartConfig {
    fn default() -> Self {
        Self {
            row_height: 50.0,
            bands: 3,
            positive_colors: vec![
                [0.65, 0.10, 145.0],  // Light green
                [0.65, 0.15, 145.0],  // Medium green
                [0.60, 0.20, 145.0],  // Dark green
            ],
            negative_colors: vec![
                [0.65, 0.10, 25.0],   // Light red
                [0.65, 0.15, 25.0],   // Medium red
                [0.60, 0.20, 25.0],   // Dark red
            ],
            mirror_negatives: true,
            show_baseline: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HorizonChartPanel {
    id: HorizonChartId,
    title: String,
}

impl HorizonChartPanel {
    pub fn new(id: HorizonChartId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> HorizonChartId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "horizon_chart"
    }

    pub fn kind_label(&self) -> &'static str {
        "Horizon Chart"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 200.0)
    }
}
