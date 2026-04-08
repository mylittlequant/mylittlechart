use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SecFilingsId(pub u64);

#[derive(Clone, Debug)]
pub struct SecFilingsState {
    /// SEC filings
    pub filings: Vec<SecFiling>,
    /// Symbol filter
    pub symbol_filter: Option<String>,
    /// Filing type filter
    pub type_filter: Option<Vec<FilingType>>,
    /// Date range
    pub date_range: DateRange,
}

#[derive(Clone, Debug)]
pub struct SecFiling {
    pub id: String,
    pub date: i64,
    pub symbol: String,
    pub company: String,
    pub filing_type: FilingType,
    pub description: String,
    pub url: String,
    pub accession_number: String,
}

#[derive(Clone, Debug)]
pub enum FilingType {
    Form10K,
    Form10Q,
    Form8K,
    Form4,
    FormS1,
    Form13F,
    FormDEF14A,
    Other(String),
}

#[derive(Clone, Debug)]
pub enum DateRange {
    Today,
    Week,
    Month,
    Quarter,
}

impl SecFilingsState {
    pub fn new() -> Self {
        Self {
            filings: Vec::new(),
            symbol_filter: None,
            type_filter: None,
            date_range: DateRange::Month,
        }
    }

    /// Get visible filings for rendering
    pub fn visible_filings(&self, scroll_offset: usize, max_rows: usize) -> &[SecFiling] {
        let end = (scroll_offset + max_rows).min(self.filings.len());
        &self.filings[scroll_offset..end]
    }

    /// Format filing for display
    pub fn format_filing(&self, filing: &SecFiling) -> (String, String, String, String, String) {
        let date = format_date(filing.date);
        let symbol = filing.symbol.clone();
        let company = filing.company.clone();
        let filing_type = self.format_filing_type(&filing.filing_type);
        let desc = if filing.description.len() > 50 {
            format!("{}...", &filing.description[..50])
        } else {
            filing.description.clone()
        };
        (date, symbol, company, filing_type, desc)
    }

    fn format_filing_type(&self, filing_type: &FilingType) -> String {
        match filing_type {
            FilingType::Form10K => "10-K".to_string(),
            FilingType::Form10Q => "10-Q".to_string(),
            FilingType::Form8K => "8-K".to_string(),
            FilingType::Form4 => "4".to_string(),
            FilingType::FormS1 => "S-1".to_string(),
            FilingType::Form13F => "13F".to_string(),
            FilingType::FormDEF14A => "DEF 14A".to_string(),
            FilingType::Other(s) => s.clone(),
        }
    }

    /// Get color based on filing type importance
    pub fn filing_type_color(&self, filing_type: &FilingType) -> [f32; 4] {
        match filing_type {
            FilingType::Form10K | FilingType::Form10Q => [0.9, 0.2, 0.2, 1.0], // red - important
            FilingType::Form8K => [0.9, 0.7, 0.2, 1.0],                         // yellow - notable
            FilingType::Form4 => [0.3, 0.6, 0.9, 1.0],                          // blue - insider
            FilingType::FormS1 => [0.2, 0.8, 0.3, 1.0],                         // green - IPO
            _ => [0.6, 0.6, 0.7, 1.0],                                          // neutral
        }
    }
}

fn format_date(ts: i64) -> String {
    let days = ts / 86400000;
    format!("Day {}", days % 365)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SecFilingsConfig {
    /// Watched filing types
    pub watched_types: Vec<String>,
    /// Alert on watched symbols
    pub alert_watchlist: bool,
    /// Show description column
    pub show_description: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SecFilingsPanel {
    id: SecFilingsId,
    title: String,
}

impl SecFilingsPanel {
    pub fn new(id: SecFilingsId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> SecFilingsId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "sec_filings"
    }

    pub fn kind_label(&self) -> &'static str {
        "SEC Filings"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 250.0)
    }
}
