use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimelineId(pub u64);

/// Time range
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: i64,
    pub end: i64,
}

/// Event grouping
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EventGrouping {
    None,
    ByType,
    BySource,
    Custom(String),
}

/// Track configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackConfig {
    pub id: String,
    pub label: String,
    pub height: f32,
    pub color: String,
    pub event_types: Vec<String>,
}

/// Configuration for timeline panel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineConfig {
    pub time_range: TimeRange,
    pub zoom_level: f64,
    pub show_labels: bool,
    pub grouping: EventGrouping,
    pub tracks: Vec<TrackConfig>,
}

impl Default for TimelineConfig {
    fn default() -> Self {
        Self {
            time_range: TimeRange {
                start: 0,
                end: 0,
            },
            zoom_level: 100.0,
            show_labels: true,
            grouping: EventGrouping::ByType,
            tracks: Vec::new(),
        }
    }
}

/// Marker shape
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum MarkerShape {
    Circle,
    Square,
    Diamond,
    Triangle,
    Star,
}

/// Marker style
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkerStyle {
    pub shape: MarkerShape,
    pub color: String,
    pub size: f32,
    pub icon: Option<String>,
}

/// Timeline event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub id: String,
    pub timestamp: i64,
    pub event_type: String,
    pub label: String,
    pub description: String,
    pub metadata: HashMap<String, String>,
    pub marker_style: MarkerStyle,
}

/// Timeline state
#[derive(Clone, Debug, Default)]
pub struct TimelineState {
    pub events: Vec<TimelineEvent>,
    pub filtered_events: Vec<usize>,
    pub visible_range: TimeRange,
    pub scroll_offset: f32,
    pub hover_event: Option<usize>,
    pub selected_events: Vec<usize>,
}

impl Default for TimeRange {
    fn default() -> Self {
        Self { start: 0, end: 0 }
    }
}

impl TimelineState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get visible events with screen positions (x, y, label, color)
    pub fn visible_events_positioned(&self, w: f32, h: f32) -> Vec<(f32, f32, &str, [f32; 4])> {
        self.filtered_events.iter()
            .filter_map(|&idx| self.events.get(idx))
            .filter(|event| {
                event.timestamp >= self.visible_range.start &&
                event.timestamp <= self.visible_range.end
            })
            .map(|event| {
                let x = self.time_to_x(event.timestamp, w);
                let y = h / 2.0; // Center vertically for now
                let color = self.parse_color(&event.marker_style.color);
                (x, y, event.label.as_str(), color)
            })
            .collect()
    }

    /// Convert timestamp to X screen coordinate
    pub fn time_to_x(&self, timestamp: i64, w: f32) -> f32 {
        let range_duration = self.visible_range.end - self.visible_range.start;
        if range_duration == 0 {
            return 0.0;
        }

        let offset = timestamp - self.visible_range.start;
        let ratio = offset as f64 / range_duration as f64;
        (ratio as f32 * w).max(0.0).min(w)
    }

    /// Format timestamp as time label
    pub fn format_time_label(&self, timestamp: i64) -> String {
        // Convert to human-readable format
        let secs = timestamp / 1000;
        let mins = secs / 60;
        let hours = mins / 60;
        let days = hours / 24;

        if days > 0 {
            format!("{}d", days)
        } else if hours > 0 {
            format!("{}h", hours)
        } else if mins > 0 {
            format!("{}m", mins)
        } else {
            format!("{}s", secs)
        }
    }

    fn parse_color(&self, color_str: &str) -> [f32; 4] {
        // Simple hex color parser: #RRGGBB or named colors
        if color_str.starts_with('#') && color_str.len() == 7 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&color_str[1..3], 16),
                u8::from_str_radix(&color_str[3..5], 16),
                u8::from_str_radix(&color_str[5..7], 16),
            ) {
                return [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0];
            }
        }

        // Named colors fallback
        match color_str {
            "red" => [0.9, 0.2, 0.2, 1.0],
            "green" => [0.2, 0.8, 0.2, 1.0],
            "blue" => [0.2, 0.6, 0.9, 1.0],
            "yellow" => [0.9, 0.8, 0.2, 1.0],
            "orange" => [0.9, 0.5, 0.2, 1.0],
            "purple" => [0.6, 0.2, 0.9, 1.0],
            _ => [0.6, 0.6, 0.6, 1.0], // gray
        }
    }

    /// Get visible events (filtered by time range)
    pub fn visible_events(&self) -> Vec<&TimelineEvent> {
        self.filtered_events.iter()
            .filter_map(|&idx| self.events.get(idx))
            .filter(|event| {
                event.timestamp >= self.visible_range.start &&
                event.timestamp <= self.visible_range.end
            })
            .collect()
    }

    /// Format event for display
    pub fn format_event(&self, event: &TimelineEvent) -> (String, String, String) {
        let time = self.format_time_label(event.timestamp);
        let label = event.label.clone();
        let event_type = event.event_type.clone();
        (time, label, event_type)
    }

    /// Get event color from marker style
    pub fn event_color(&self, event: &TimelineEvent) -> [f32; 4] {
        self.parse_color(&event.marker_style.color)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimelinePanel {
    id: TimelineId,
    title: String,
}

impl TimelinePanel {
    pub fn new(id: TimelineId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> TimelineId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "timeline"
    }

    pub fn kind_label(&self) -> &'static str {
        "Timeline"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 100.0)
    }
}
