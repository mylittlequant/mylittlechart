use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EconomicCalendarId(pub u64);

#[derive(Clone, Debug)]
pub struct EconomicCalendarState {
    /// Economic events
    pub events: Vec<EconomicEvent>,
    /// Date range filter
    pub date_range: DateRange,
    /// Country filter
    pub country_filter: Option<Vec<String>>,
    /// Impact filter (High/Medium/Low)
    pub impact_filter: Option<Vec<EventImpact>>,
}

#[derive(Clone, Debug)]
pub struct EconomicEvent {
    pub id: String,
    pub timestamp: i64,
    pub country: String,
    pub event_name: String,
    pub impact: EventImpact,
    pub previous: Option<String>,
    pub forecast: Option<String>,
    pub actual: Option<String>,
    pub currency: Option<String>,
}

#[derive(Clone, Debug)]
pub enum EventImpact {
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug)]
pub enum DateRange {
    Today,
    ThisWeek,
    NextWeek,
    ThisMonth,
}

impl EconomicCalendarState {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            date_range: DateRange::ThisWeek,
            country_filter: None,
            impact_filter: None,
        }
    }

    /// Get visible events for rendering
    pub fn visible_events(&self, scroll_offset: usize, max_rows: usize) -> &[EconomicEvent] {
        let end = (scroll_offset + max_rows).min(self.events.len());
        &self.events[scroll_offset..end]
    }

    /// Format event for display
    pub fn format_event(&self, event: &EconomicEvent) -> (String, String, String, String, String, String) {
        let time = format_timestamp(event.timestamp);
        let country = event.country.clone();
        let name = event.event_name.clone();
        let previous = event.previous.as_ref().map(|s| s.as_str()).unwrap_or("—");
        let forecast = event.forecast.as_ref().map(|s| s.as_str()).unwrap_or("—");
        let actual = event.actual.as_ref().map(|s| s.as_str()).unwrap_or("—");
        (time, country, name, previous.to_string(), forecast.to_string(), actual.to_string())
    }

    /// Get color based on event impact
    pub fn impact_color(&self, event: &EconomicEvent) -> [f32; 4] {
        match event.impact {
            EventImpact::High => [0.9, 0.2, 0.2, 1.0],   // red
            EventImpact::Medium => [0.9, 0.7, 0.2, 1.0], // yellow
            EventImpact::Low => [0.6, 0.6, 0.7, 1.0],    // neutral
        }
    }

    /// Get color based on actual vs forecast
    pub fn result_color(&self, event: &EconomicEvent) -> Option<[f32; 4]> {
        if let (Some(actual), Some(forecast)) = (&event.actual, &event.forecast) {
            if let (Ok(act), Ok(fct)) = (actual.parse::<f64>(), forecast.parse::<f64>()) {
                if act > fct {
                    return Some([0.2, 0.8, 0.3, 1.0]); // green - beat
                } else if act < fct {
                    return Some([0.9, 0.2, 0.2, 1.0]); // red - miss
                }
            }
        }
        None
    }
}

fn format_timestamp(ts: i64) -> String {
    let secs = (ts / 1000) % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    format!("{:02}:{:02}", h, m)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EconomicCalendarConfig {
    /// Default countries to show
    pub default_countries: Vec<String>,
    /// Show only high impact events
    pub high_impact_only: bool,
    /// Alert before event (minutes)
    pub alert_before_minutes: Option<u32>,
    /// Timezone for display
    pub timezone: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EconomicCalendarPanel {
    id: EconomicCalendarId,
    title: String,
}

impl EconomicCalendarPanel {
    pub fn new(id: EconomicCalendarId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> EconomicCalendarId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "economic_calendar"
    }

    pub fn kind_label(&self) -> &'static str {
        "Economic Calendar"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 250.0)
    }
}
