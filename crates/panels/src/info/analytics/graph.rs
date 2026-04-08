use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GraphId(pub u64);

/// Chart type
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChartType {
    Line,
    Bar,
    Area,
    Scatter,
    Candlestick,
    Mixed(Vec<ChartType>),
}

/// Axis position
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AxisPosition {
    Left,
    Right,
}

/// Axis scale
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AxisScale {
    Linear,
    Logarithmic,
}

/// Axis formatter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AxisFormatter {
    Number { decimals: u8 },
    Currency { symbol: String },
    DateTime { format: String },
    Percentage,
}

/// Legend position
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LegendPosition {
    Top,
    Bottom,
    Left,
    Right,
    Hidden,
}

/// Series configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesConfig {
    pub id: String,
    pub label: String,
    pub chart_type: ChartType,
    pub color: String,
    pub line_width: f32,
    pub show_points: bool,
    pub y_axis: AxisPosition,
}

/// Axis configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AxisConfig {
    pub label: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub scale: AxisScale,
    pub tick_count: usize,
    pub formatter: AxisFormatter,
}

/// Grid configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridConfig {
    pub show_x_grid: bool,
    pub show_y_grid: bool,
    pub grid_color: String,
}

/// Color scheme
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorScheme {
    pub background: String,
    pub text: String,
    pub axis: String,
    pub grid: String,
}

/// Configuration for graph panel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphConfig {
    pub chart_type: ChartType,
    pub series: Vec<SeriesConfig>,
    pub x_axis: AxisConfig,
    pub y_axis: AxisConfig,
    pub title: Option<String>,
    pub legend_position: LegendPosition,
    pub grid: GridConfig,
    pub colors: ColorScheme,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            chart_type: ChartType::Line,
            series: Vec::new(),
            x_axis: AxisConfig {
                label: None,
                min: None,
                max: None,
                scale: AxisScale::Linear,
                tick_count: 10,
                formatter: AxisFormatter::Number { decimals: 2 },
            },
            y_axis: AxisConfig {
                label: None,
                min: None,
                max: None,
                scale: AxisScale::Linear,
                tick_count: 8,
                formatter: AxisFormatter::Number { decimals: 2 },
            },
            title: None,
            legend_position: LegendPosition::Top,
            grid: GridConfig {
                show_x_grid: true,
                show_y_grid: true,
                grid_color: "#cccccc".into(),
            },
            colors: ColorScheme {
                background: "#ffffff".into(),
                text: "#000000".into(),
                axis: "#000000".into(),
                grid: "#cccccc".into(),
            },
        }
    }
}

/// Data point
#[derive(Debug, Clone)]
pub struct DataPoint {
    pub x: f64,
    pub y: f64,
    pub label: Option<String>,
}

/// Hover info
#[derive(Debug, Clone)]
pub struct HoverInfo {
    pub series_id: String,
    pub point: DataPoint,
    pub screen_pos: (f32, f32),
}

/// Graph state
#[derive(Clone, Debug, Default)]
pub struct GraphState {
    pub series_data: HashMap<String, Vec<DataPoint>>,
    pub x_range: (f64, f64),
    pub y_range: (f64, f64),
    pub hover_point: Option<HoverInfo>,
    pub selected_series: Vec<String>,
}

impl GraphState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get screen coordinates for a series (x, y)
    pub fn series_points(&self, series_idx: usize, w: f32, h: f32) -> Vec<(f32, f32)> {
        // Get series by index from series_data keys
        let series_ids: Vec<&String> = self.series_data.keys().collect();
        if series_idx >= series_ids.len() {
            return Vec::new();
        }

        let series_id = series_ids[series_idx];
        if let Some(points) = self.series_data.get(series_id) {
            points.iter()
                .map(|pt| self.data_to_screen(pt.x, pt.y, w, h))
                .collect()
        } else {
            Vec::new()
        }
    }

    fn data_to_screen(&self, x: f64, y: f64, w: f32, h: f32) -> (f32, f32) {
        let x_range_width = self.x_range.1 - self.x_range.0;
        let y_range_height = self.y_range.1 - self.y_range.0;

        if x_range_width == 0.0 || y_range_height == 0.0 {
            return (0.0, 0.0);
        }

        let x_normalized = (x - self.x_range.0) / x_range_width;
        let y_normalized = (y - self.y_range.0) / y_range_height;

        let screen_x = x_normalized as f32 * w;
        let screen_y = h - (y_normalized as f32 * h); // flip Y axis

        (screen_x, screen_y)
    }

    /// Get X axis labels (position, label)
    pub fn x_labels(&self, w: f32) -> Vec<(f32, String)> {
        let num_labels = 10;
        let x_range_width = self.x_range.1 - self.x_range.0;

        if x_range_width == 0.0 {
            return Vec::new();
        }

        (0..=num_labels)
            .map(|i| {
                let ratio = i as f64 / num_labels as f64;
                let value = self.x_range.0 + (x_range_width * ratio);
                let pos = (i as f32 / num_labels as f32) * w;
                (pos, format!("{:.2}", value))
            })
            .collect()
    }

    /// Get Y axis labels (position, label)
    pub fn y_labels(&self, h: f32) -> Vec<(f32, String)> {
        let num_labels = 8;
        let y_range_height = self.y_range.1 - self.y_range.0;

        if y_range_height == 0.0 {
            return Vec::new();
        }

        (0..=num_labels)
            .map(|i| {
                let ratio = i as f64 / num_labels as f64;
                let value = self.y_range.0 + (y_range_height * ratio);
                let pos = h - ((i as f32 / num_labels as f32) * h); // flip Y axis
                (pos, format!("{:.2}", value))
            })
            .collect()
    }

    /// Get legend entries (name, color)
    pub fn legend_entries(&self) -> Vec<(&str, [f32; 4])> {
        let colors = [
            [0.2, 0.6, 0.9, 1.0], // blue
            [0.9, 0.4, 0.2, 1.0], // orange
            [0.2, 0.8, 0.2, 1.0], // green
            [0.9, 0.2, 0.2, 1.0], // red
            [0.6, 0.2, 0.9, 1.0], // purple
            [0.9, 0.8, 0.2, 1.0], // yellow
        ];

        self.series_data.keys()
            .enumerate()
            .map(|(idx, name)| {
                let color = colors[idx % colors.len()];
                (name.as_str(), color)
            })
            .collect()
    }

    /// Get visible points for a series (all points in visible range)
    pub fn visible_points(&self, series_id: &str) -> Option<&[DataPoint]> {
        self.series_data.get(series_id).map(|v| v.as_slice())
    }

    /// Format a data point for display
    pub fn format_point(&self, point: &DataPoint) -> (String, String) {
        (format!("{:.2}", point.x), format!("{:.2}", point.y))
    }

    /// Get series color by index
    pub fn series_color(&self, series_idx: usize) -> [f32; 4] {
        let colors = [
            [0.2, 0.6, 0.9, 1.0], // blue
            [0.9, 0.4, 0.2, 1.0], // orange
            [0.2, 0.8, 0.2, 1.0], // green
            [0.9, 0.2, 0.2, 1.0], // red
            [0.6, 0.2, 0.9, 1.0], // purple
            [0.9, 0.8, 0.2, 1.0], // yellow
        ];
        colors[series_idx % colors.len()]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphPanel {
    id: GraphId,
    title: String,
}

impl GraphPanel {
    pub fn new(id: GraphId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> GraphId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "graph"
    }

    pub fn kind_label(&self) -> &'static str {
        "Graph"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (250.0, 200.0)
    }
}
