use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EarningsCalendarId(pub u64);

#[derive(Clone, Debug)]
pub struct EarningsCalendarState {
    /// Earnings events
    pub earnings: Vec<EarningsEvent>,
    /// Date range filter
    pub date_range: DateRange,
    /// Symbol filter
    pub symbol_filter: Option<String>,
    /// Beat/miss filter
    pub surprise_filter: Option<SurpriseFilter>,
}

#[derive(Clone, Debug)]
pub struct EarningsEvent {
    pub id: String,
    pub date: i64,
    pub symbol: String,
    pub company: String,
    pub eps_estimate: Option<f64>,
    pub eps_actual: Option<f64>,
    pub revenue_estimate: Option<f64>,
    pub revenue_actual: Option<f64>,
    pub surprise_percent: Option<f64>,
    pub fiscal_quarter: String,
    pub call_time: Option<String>,
}

#[derive(Clone, Debug)]
pub enum SurpriseFilter {
    Beat,
    Miss,
    Inline,
}

#[derive(Clone, Debug)]
pub enum DateRange {
    Today,
    ThisWeek,
    NextWeek,
    ThisMonth,
}

impl EarningsCalendarState {
    pub fn new() -> Self {
        Self {
            earnings: Vec::new(),
            date_range: DateRange::ThisWeek,
            symbol_filter: None,
            surprise_filter: None,
        }
    }

    /// Get visible earnings events for rendering
    pub fn visible_earnings(&self, scroll_offset: usize, max_rows: usize) -> &[EarningsEvent] {
        let end = (scroll_offset + max_rows).min(self.earnings.len());
        &self.earnings[scroll_offset..end]
    }

    /// Format earning event for display
    pub fn format_earning(&self, earning: &EarningsEvent) -> (String, String, String, String, String, String) {
        let date = format_date(earning.date);
        let symbol = earning.symbol.clone();
        let company = earning.company.clone();
        let eps = if let (Some(est), Some(act)) = (earning.eps_estimate, earning.eps_actual) {
            format!("{:.2} / {:.2}", act, est)
        } else if let Some(est) = earning.eps_estimate {
            format!("— / {:.2}", est)
        } else {
            "—".to_string()
        };
        let surprise = earning.surprise_percent
            .map(|s| format!("{:+.1}%", s))
            .unwrap_or_else(|| "—".to_string());
        let time = earning.call_time.as_ref().map(|s| s.as_str()).unwrap_or("—");
        (date, symbol, company, eps, surprise, time.to_string())
    }

    /// Get color based on earnings surprise
    pub fn surprise_color(&self, earning: &EarningsEvent) -> [f32; 4] {
        if let Some(surprise) = earning.surprise_percent {
            if surprise > 5.0 {
                [0.2, 0.8, 0.3, 1.0] // green - beat
            } else if surprise < -5.0 {
                [0.9, 0.2, 0.2, 1.0] // red - miss
            } else {
                [0.9, 0.7, 0.2, 1.0] // yellow - inline
            }
        } else {
            [0.6, 0.6, 0.7, 1.0] // neutral
        }
    }
}

fn format_date(ts: i64) -> String {
    // Simple date format (would use chrono in real impl)
    let days = ts / 86400000;
    format!("Day {}", days % 365)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EarningsCalendarConfig {
    /// Show pre-market and after-hours
    pub show_call_time: bool,
    /// Alert on watched symbols
    pub alert_watchlist: bool,
    /// Highlight surprise > threshold
    pub surprise_threshold: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EarningsCalendarPanel {
    id: EarningsCalendarId,
    title: String,
}

impl EarningsCalendarPanel {
    pub fn new(id: EarningsCalendarId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> EarningsCalendarId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "earnings_calendar"
    }

    pub fn kind_label(&self) -> &'static str {
        "Earnings Calendar"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 250.0)
    }
}
