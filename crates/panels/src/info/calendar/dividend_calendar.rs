use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DividendCalendarId(pub u64);

#[derive(Clone, Debug)]
pub struct DividendCalendarState {
    /// Dividend events
    pub dividends: Vec<DividendEvent>,
    /// Date range filter
    pub date_range: DateRange,
    /// Symbol filter
    pub symbol_filter: Option<String>,
    /// Yield filter (minimum yield %)
    pub min_yield: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct DividendEvent {
    pub id: String,
    pub ex_date: i64,
    pub symbol: String,
    pub company: String,
    pub amount: f64,
    pub yield_percent: f64,
    pub record_date: Option<i64>,
    pub payment_date: Option<i64>,
    pub frequency: DividendFrequency,
}

#[derive(Clone, Debug)]
pub enum DividendFrequency {
    Monthly,
    Quarterly,
    SemiAnnual,
    Annual,
    Special,
}

#[derive(Clone, Debug)]
pub enum DateRange {
    Today,
    ThisWeek,
    NextWeek,
    ThisMonth,
}

impl DividendCalendarState {
    pub fn new() -> Self {
        Self {
            dividends: Vec::new(),
            date_range: DateRange::ThisMonth,
            symbol_filter: None,
            min_yield: None,
        }
    }

    /// Get visible dividends for rendering
    pub fn visible_dividends(&self, scroll_offset: usize, max_rows: usize) -> &[DividendEvent] {
        let end = (scroll_offset + max_rows).min(self.dividends.len());
        &self.dividends[scroll_offset..end]
    }

    /// Format dividend event for display
    pub fn format_dividend(&self, div: &DividendEvent) -> (String, String, String, String, String) {
        let ex_date = format_date(div.ex_date);
        let symbol = div.symbol.clone();
        let company = div.company.clone();
        let amount = format!("${:.2}", div.amount);
        let yield_str = format!("{:.2}%", div.yield_percent);
        (ex_date, symbol, company, amount, yield_str)
    }

    /// Get color based on yield
    pub fn yield_color(&self, div: &DividendEvent) -> [f32; 4] {
        if div.yield_percent > 5.0 {
            [0.2, 0.8, 0.3, 1.0] // green - high yield
        } else if div.yield_percent > 3.0 {
            [0.9, 0.7, 0.2, 1.0] // yellow - medium yield
        } else {
            [0.6, 0.6, 0.7, 1.0] // neutral - low yield
        }
    }

    /// Format frequency for display
    pub fn format_frequency(&self, freq: &DividendFrequency) -> &'static str {
        match freq {
            DividendFrequency::Monthly => "Monthly",
            DividendFrequency::Quarterly => "Quarterly",
            DividendFrequency::SemiAnnual => "Semi-Annual",
            DividendFrequency::Annual => "Annual",
            DividendFrequency::Special => "Special",
        }
    }
}

fn format_date(ts: i64) -> String {
    let days = ts / 86400000;
    format!("Day {}", days % 365)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DividendCalendarConfig {
    /// Show record/payment dates
    pub show_additional_dates: bool,
    /// Highlight yield > threshold
    pub high_yield_threshold: f64,
    /// Currency for amount
    pub display_currency: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DividendCalendarPanel {
    id: DividendCalendarId,
    title: String,
}

impl DividendCalendarPanel {
    pub fn new(id: DividendCalendarId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> DividendCalendarId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "dividend_calendar"
    }

    pub fn kind_label(&self) -> &'static str {
        "Dividend Calendar"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 200.0)
    }
}
