use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GpuTimeseriesId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct GpuTimeseriesState {
    pub series: Vec<TimeseriesData>,
    pub time_range: (i64, i64),  // Unix timestamps
    pub value_range: (f64, f64),
    pub lod_strategy: LodStrategy,  // Level-of-detail for performance
    pub viewport_cache: Option<CachedRender>,
}

#[derive(Clone, Debug)]
pub struct TimeseriesData {
    pub id: String,
    pub points: Vec<(i64, f32)>,  // (timestamp_ms, value) - optimized types
    pub color: [f32; 3],  // OKLCH
    pub line_width: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum LodStrategy {
    Downsample,  // Reduce points based on zoom level
    MinMax,      // Show min/max in each pixel column
    LTTB,        // Largest Triangle Three Buckets algorithm
}

#[derive(Clone, Debug)]
pub struct CachedRender {
    pub time_range: (i64, i64),
    pub texture_id: u64,  // Placeholder for Vello texture handle
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GpuTimeseriesConfig {
    pub point_density_threshold: usize,  // Max points per pixel before LOD kicks in
    pub lod_strategy: LodStrategy,
    pub enable_caching: bool,
    pub cache_invalidation_ms: u64,
}

impl Default for LodStrategy {
    fn default() -> Self {
        Self::MinMax
    }
}

impl GpuTimeseriesState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns visible points downsampled to pixel resolution
    pub fn visible_points(&self, w: f32, h: f32) -> Vec<(f32, f32)> {
        let mut points = Vec::new();

        if self.series.is_empty() {
            return points;
        }

        let (time_min, time_max) = self.time_range;
        let time_range = (time_max - time_min) as f64;
        if time_range == 0.0 {
            return points;
        }

        let (val_min, val_max) = self.value_range;
        let val_range = val_max - val_min;
        if val_range == 0.0 {
            return points;
        }

        for series in &self.series {
            let _pixels_per_point = w / series.points.len().max(1) as f32;

            let mut last_pixel_x = -1.0f32;

            for (timestamp, value) in &series.points {
                let t_norm = (*timestamp as f64 - time_min as f64) / time_range;
                let v_norm = (*value as f64 - val_min) / val_range;

                let x = (t_norm * w as f64) as f32;
                let y = ((1.0 - v_norm) * h as f64) as f32;

                if (x - last_pixel_x).abs() >= 1.0 {
                    points.push((x.clamp(0.0, w), y.clamp(0.0, h)));
                    last_pixel_x = x;
                }
            }
        }

        points
    }

    // Render helpers
    pub fn format_value(&self, value: f64) -> String {
        if value.abs() >= 1_000_000.0 {
            format!("{:.2}M", value / 1_000_000.0)
        } else if value.abs() >= 1_000.0 {
            format!("{:.1}K", value / 1_000.0)
        } else {
            format!("{:.2}", value)
        }
    }

    pub fn y_scale(&self, h: f32) -> f32 {
        let (val_min, val_max) = self.value_range;
        let range = val_max - val_min;
        if range > 0.0 {
            h / range as f32
        } else {
            1.0
        }
    }

    pub fn x_range(&self) -> (i64, i64) {
        self.time_range
    }
}

impl Default for GpuTimeseriesConfig {
    fn default() -> Self {
        Self {
            point_density_threshold: 10,
            lod_strategy: LodStrategy::MinMax,
            enable_caching: true,
            cache_invalidation_ms: 1000,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GpuTimeseriesPanel {
    id: GpuTimeseriesId,
    title: String,
}

impl GpuTimeseriesPanel {
    pub fn new(id: GpuTimeseriesId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> GpuTimeseriesId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "gpu_timeseries"
    }

    pub fn kind_label(&self) -> &'static str {
        "GPU Timeseries"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
