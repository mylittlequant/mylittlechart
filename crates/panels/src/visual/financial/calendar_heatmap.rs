use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CalendarHeatmapId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct CalendarHeatmapState {
    pub data: HashMap<String, f64>,  // "YYYY-MM-DD" -> metric value
    pub year: i32,
    pub metric_name: String,
    pub value_range: (f64, f64),
    pub selected_date: Option<String>,  // "YYYY-MM-DD"
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalendarHeatmapConfig {
    pub cell_size: f32,
    pub cell_spacing: f32,
    pub color_gradient: Vec<[f32; 3]>,  // OKLCH gradient (e.g., 5 levels)
    pub show_month_labels: bool,
    pub show_weekday_labels: bool,
    pub start_week_on: Weekday,  // Monday or Sunday
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Weekday {
    Monday,
    Sunday,
}

impl CalendarHeatmapState {
    pub fn new() -> Self {
        Self {
            year: 2026,
            ..Default::default()
        }
    }

    /// Returns visible cells with their data
    pub fn visible_cells(&self) -> Vec<(String, f64, usize, usize)> {
        self.data.iter()
            .filter_map(|(date_str, &value)| {
                let parts: Vec<&str> = date_str.split('-').collect();
                if parts.len() != 3 {
                    return None;
                }

                let year = parts[0].parse::<i32>().ok()?;
                let month = parts[1].parse::<u32>().ok()?;
                let day = parts[2].parse::<u32>().ok()?;

                if year != self.year {
                    return None;
                }

                let day_of_year = month * 30 + day;
                let week = (day_of_year / 7) as usize;
                let weekday = (day_of_year % 7) as usize;

                Some((date_str.clone(), value, week, weekday))
            })
            .collect()
    }

    /// Returns color for a metric value based on intensity
    pub fn value_color(&self, value: f64) -> [f32; 4] {
        let (min_val, max_val) = if self.value_range.0 != self.value_range.1 {
            self.value_range
        } else {
            self.data.values().fold((f64::MAX, f64::MIN), |(min, max), &val| {
                (min.min(val), max.max(val))
            })
        };

        let value_range = max_val - min_val;
        let norm_value = if value_range > 0.0 {
            ((value - min_val) / value_range).clamp(0.0, 1.0)
        } else {
            0.5
        };

        let level = (norm_value * 4.0) as usize;
        match level {
            0 => [0.3, 0.0, 0.0, 1.0],
            1 => [0.65, 0.10, 145.0, 1.0],
            2 => [0.65, 0.15, 145.0, 1.0],
            3 => [0.60, 0.20, 145.0, 1.0],
            _ => [0.55, 0.25, 145.0, 1.0],
        }
    }

    /// Returns week labels (e.g., "W1", "W2", ...)
    pub fn week_labels(&self) -> Vec<String> {
        (1..=53).map(|w| format!("W{}", w)).collect()
    }

    /// Returns month labels for the year
    pub fn month_labels(&self) -> Vec<String> {
        vec![
            "Jan".to_string(), "Feb".to_string(), "Mar".to_string(),
            "Apr".to_string(), "May".to_string(), "Jun".to_string(),
            "Jul".to_string(), "Aug".to_string(), "Sep".to_string(),
            "Oct".to_string(), "Nov".to_string(), "Dec".to_string(),
        ]
    }
}

impl Default for CalendarHeatmapConfig {
    fn default() -> Self {
        Self {
            cell_size: 12.0,
            cell_spacing: 2.0,
            color_gradient: vec![
                [0.3, 0.0, 0.0],        // Level 0: dark
                [0.65, 0.10, 145.0],    // Level 1
                [0.65, 0.15, 145.0],    // Level 2
                [0.60, 0.20, 145.0],    // Level 3
                [0.55, 0.25, 145.0],    // Level 4
            ],
            show_month_labels: true,
            show_weekday_labels: true,
            start_week_on: Weekday::Monday,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalendarHeatmapPanel {
    id: CalendarHeatmapId,
    title: String,
}

impl CalendarHeatmapPanel {
    pub fn new(id: CalendarHeatmapId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> CalendarHeatmapId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "calendar_heatmap"
    }

    pub fn kind_label(&self) -> &'static str {
        "Calendar Heatmap"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 200.0)
    }
}
