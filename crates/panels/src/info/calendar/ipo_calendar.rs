use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IpoCalendarId(pub u64);

#[derive(Clone, Debug)]
pub struct IpoCalendarState {
    /// IPO events
    pub ipos: Vec<IpoEvent>,
    /// Date range filter
    pub date_range: DateRange,
    /// Status filter
    pub status_filter: Option<IpoStatus>,
}

#[derive(Clone, Debug)]
pub struct IpoEvent {
    pub id: String,
    pub date: i64,
    pub company: String,
    pub symbol: Option<String>,
    pub price_range_low: Option<f64>,
    pub price_range_high: Option<f64>,
    pub shares: Option<u64>,
    pub market_cap_estimate: Option<f64>,
    pub exchange: String,
    pub status: IpoStatus,
}

#[derive(Clone, Debug)]
pub enum IpoStatus {
    Scheduled,
    Priced,
    Trading,
    Withdrawn,
    Postponed,
}

#[derive(Clone, Debug)]
pub enum DateRange {
    Today,
    ThisWeek,
    NextWeek,
    ThisMonth,
}

impl IpoCalendarState {
    pub fn new() -> Self {
        Self {
            ipos: Vec::new(),
            date_range: DateRange::ThisMonth,
            status_filter: None,
        }
    }

    /// Get visible IPOs for rendering
    pub fn visible_ipos(&self, scroll_offset: usize, max_rows: usize) -> &[IpoEvent] {
        let end = (scroll_offset + max_rows).min(self.ipos.len());
        &self.ipos[scroll_offset..end]
    }

    /// Format IPO event for display
    pub fn format_ipo(&self, ipo: &IpoEvent) -> (String, String, String, String, String) {
        let date = format_date(ipo.date);
        let company = ipo.company.clone();
        let symbol = ipo.symbol.as_ref().map(|s| s.as_str()).unwrap_or("TBA");
        let price_range = if let (Some(low), Some(high)) = (ipo.price_range_low, ipo.price_range_high) {
            format!("${:.2}-${:.2}", low, high)
        } else {
            "—".to_string()
        };
        let market_cap = ipo.market_cap_estimate
            .map(|mc| format_market_cap(mc))
            .unwrap_or_else(|| "—".to_string());
        (date, company, symbol.to_string(), price_range, market_cap)
    }

    /// Get color based on IPO status
    pub fn status_color(&self, ipo: &IpoEvent) -> [f32; 4] {
        match ipo.status {
            IpoStatus::Scheduled => [0.3, 0.6, 0.9, 1.0],  // blue
            IpoStatus::Priced => [0.9, 0.7, 0.2, 1.0],     // yellow
            IpoStatus::Trading => [0.2, 0.8, 0.3, 1.0],    // green
            IpoStatus::Withdrawn => [0.5, 0.5, 0.5, 1.0],  // gray
            IpoStatus::Postponed => [0.9, 0.5, 0.2, 1.0],  // orange
        }
    }

    /// Format status for display
    pub fn format_status(&self, status: &IpoStatus) -> &'static str {
        match status {
            IpoStatus::Scheduled => "Scheduled",
            IpoStatus::Priced => "Priced",
            IpoStatus::Trading => "Trading",
            IpoStatus::Withdrawn => "Withdrawn",
            IpoStatus::Postponed => "Postponed",
        }
    }
}

fn format_date(ts: i64) -> String {
    let days = ts / 86400000;
    format!("Day {}", days % 365)
}

fn format_market_cap(mc: f64) -> String {
    if mc > 1_000_000_000.0 {
        format!("${:.1}B", mc / 1_000_000_000.0)
    } else if mc > 1_000_000.0 {
        format!("${:.1}M", mc / 1_000_000.0)
    } else {
        format!("${:.0}", mc)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IpoCalendarConfig {
    /// Show withdrawn IPOs
    pub show_withdrawn: bool,
    /// Minimum market cap filter
    pub min_market_cap: Option<f64>,
    /// Exchange filter
    pub exchange_filter: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IpoCalendarPanel {
    id: IpoCalendarId,
    title: String,
}

impl IpoCalendarPanel {
    pub fn new(id: IpoCalendarId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> IpoCalendarId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "ipo_calendar"
    }

    pub fn kind_label(&self) -> &'static str {
        "IPO Calendar"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 200.0)
    }
}
