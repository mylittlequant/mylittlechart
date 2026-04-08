use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OptionsExpiryId(pub u64);

#[derive(Clone, Debug)]
pub struct OptionsExpiryState {
    /// Expiration dates with summary
    pub expiries: Vec<OptionsExpiry>,
    /// Underlying filter
    pub underlying_filter: Option<String>,
    /// Date range
    pub date_range: DateRange,
}

#[derive(Clone, Debug)]
pub struct OptionsExpiry {
    pub date: i64,
    pub symbol: String,
    pub total_oi: u64,
    pub call_oi: u64,
    pub put_oi: u64,
    pub total_volume: u64,
    pub max_pain: Option<f64>,
    pub put_call_ratio: f64,
    pub spot_price: f64,
}

#[derive(Clone, Debug)]
pub enum DateRange {
    Today,
    ThisWeek,
    NextWeek,
    ThisMonth,
}

impl OptionsExpiryState {
    pub fn new() -> Self {
        Self {
            expiries: Vec::new(),
            underlying_filter: None,
            date_range: DateRange::ThisMonth,
        }
    }

    /// Get visible expiries for rendering
    pub fn visible_expiries(&self, scroll_offset: usize, max_rows: usize) -> &[OptionsExpiry] {
        let end = (scroll_offset + max_rows).min(self.expiries.len());
        &self.expiries[scroll_offset..end]
    }

    /// Format expiry for display
    pub fn format_expiry(&self, exp: &OptionsExpiry) -> (String, String, String, String, String, String) {
        let date = format_date(exp.date);
        let symbol = exp.symbol.clone();
        let total_oi = format_large_num(exp.total_oi);
        let pcr = format!("{:.2}", exp.put_call_ratio);
        let max_pain = exp.max_pain
            .map(|p| format!("${:.2}", p))
            .unwrap_or_else(|| "—".to_string());
        let spot = format!("${:.2}", exp.spot_price);
        (date, symbol, total_oi, pcr, max_pain, spot)
    }

    /// Get color based on put/call ratio
    pub fn pcr_color(&self, exp: &OptionsExpiry) -> [f32; 4] {
        if exp.put_call_ratio > 1.2 {
            [0.9, 0.2, 0.2, 1.0] // red - bearish
        } else if exp.put_call_ratio < 0.8 {
            [0.2, 0.8, 0.3, 1.0] // green - bullish
        } else {
            [0.9, 0.7, 0.2, 1.0] // yellow - neutral
        }
    }
}

fn format_date(ts: i64) -> String {
    let days = ts / 86400000;
    format!("Day {}", days % 365)
}

fn format_large_num(n: u64) -> String {
    if n > 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n > 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        format!("{}", n)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OptionsExpiryConfig {
    /// Show max pain calculation
    pub show_max_pain: bool,
    /// Show put/call ratio
    pub show_pcr: bool,
    /// Highlight weekly vs monthly
    pub highlight_period: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OptionsExpiryPanel {
    id: OptionsExpiryId,
    title: String,
}

impl OptionsExpiryPanel {
    pub fn new(id: OptionsExpiryId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> OptionsExpiryId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "options_expiry"
    }

    pub fn kind_label(&self) -> &'static str {
        "Options Expiry"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (250.0, 200.0)
    }
}
