use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScreenerId(pub u64);

#[derive(Clone, Debug)]
pub struct ScreenerState {
    /// Active filters
    pub filters: Vec<ScreenerFilter>,
    /// Screener results
    pub results: Vec<ScreenerResult>,
    /// Sort configuration
    pub sort: (ScreenerColumn, bool),
    /// Saved filter presets
    pub presets: Vec<FilterPreset>,
    /// Active preset
    pub active_preset: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ScreenerFilter {
    pub field: ScreenerField,
    pub operator: FilterOperator,
    pub value: FilterValue,
}

#[derive(Clone, Debug)]
pub enum ScreenerField {
    Price,
    Volume,
    MarketCap,
    ChangePercent,
    PE,
    EPS,
    DividendYield,
    RSI,
    MACD,
    SMA50,
    SMA200,
    Custom(String),
}

#[derive(Clone, Debug)]
pub enum FilterOperator {
    GreaterThan,
    LessThan,
    Equals,
    Between,
}

#[derive(Clone, Debug)]
pub enum FilterValue {
    Number(f64),
    Range(f64, f64),
    String(String),
}

#[derive(Clone, Debug)]
pub struct ScreenerResult {
    pub symbol: String,
    pub price: f64,
    pub change_percent: f64,
    pub volume: f64,
    pub market_cap: Option<f64>,
    pub fields: HashMap<String, f64>,
}

#[derive(Clone, Debug)]
pub struct FilterPreset {
    pub name: String,
    pub filters: Vec<ScreenerFilter>,
}

#[derive(Clone, Debug, Copy)]
pub enum ScreenerColumn {
    Symbol,
    Price,
    ChangePercent,
    Volume,
    MarketCap,
}

impl ScreenerState {
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
            results: Vec::new(),
            sort: (ScreenerColumn::Symbol, true),
            presets: Vec::new(),
            active_preset: None,
        }
    }

    /// Get visible results for rendering
    pub fn visible_results(&self, scroll_offset: usize, max_rows: usize) -> &[ScreenerResult] {
        let end = (scroll_offset + max_rows).min(self.results.len());
        &self.results[scroll_offset..end]
    }

    /// Format result for display
    pub fn format_result(&self, result: &ScreenerResult, column: ScreenerColumn) -> String {
        match column {
            ScreenerColumn::Symbol => result.symbol.clone(),
            ScreenerColumn::Price => format!("{:.2}", result.price),
            ScreenerColumn::ChangePercent => format!("{:+.2}%", result.change_percent),
            ScreenerColumn::Volume => format_large_num(result.volume as u64),
            ScreenerColumn::MarketCap => result.market_cap
                .map(|mc| format_market_cap(mc))
                .unwrap_or_else(|| "—".to_string()),
        }
    }

    /// Get color based on change percentage
    pub fn change_color(&self, result: &ScreenerResult) -> [f32; 4] {
        if result.change_percent > 0.0 {
            [0.2, 0.8, 0.3, 1.0]
        } else if result.change_percent < 0.0 {
            [0.9, 0.2, 0.2, 1.0]
        } else {
            [0.6, 0.6, 0.7, 1.0]
        }
    }

    /// Format filter for display
    pub fn format_filter(&self, filter: &ScreenerFilter) -> String {
        let field = format!("{:?}", filter.field);
        let op = match filter.operator {
            FilterOperator::GreaterThan => ">",
            FilterOperator::LessThan => "<",
            FilterOperator::Equals => "=",
            FilterOperator::Between => "∈",
        };
        let value = match &filter.value {
            FilterValue::Number(n) => format!("{}", n),
            FilterValue::Range(a, b) => format!("[{}, {}]", a, b),
            FilterValue::String(s) => s.clone(),
        };
        format!("{} {} {}", field, op, value)
    }
}

fn format_large_num(n: u64) -> String {
    if n > 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1_000_000_000.0)
    } else if n > 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n > 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        format!("{}", n)
    }
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
pub struct ScreenerConfig {
    /// Default market (stocks/crypto/forex)
    pub default_market: String,
    /// Max results to display
    pub max_results: usize,
    /// Available fields for filtering
    pub available_fields: Vec<String>,
    /// Auto-run on filter change
    pub auto_run: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScreenerPanel {
    id: ScreenerId,
    title: String,
}

impl ScreenerPanel {
    pub fn new(id: ScreenerId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> ScreenerId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "screener"
    }

    pub fn kind_label(&self) -> &'static str {
        "Screener"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
