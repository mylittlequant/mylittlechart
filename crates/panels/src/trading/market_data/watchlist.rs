use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// Watchlist panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WatchlistId(pub u64);

/// Watchlist panel state (heavy data)
#[derive(Clone, Debug)]
pub struct WatchlistState {
    /// List of tracked symbols
    pub symbols: Vec<String>,
    /// Latest ticker data for each symbol
    pub tickers: HashMap<String, Ticker>,
    /// Sort configuration (column, ascending)
    pub sort: (WatchlistColumn, bool),
    /// Selected row index
    pub selected: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
pub enum WatchlistColumn {
    Symbol,
    Last,
    ChangePercent,
    High24h,
    Low24h,
    Volume24h,
}

#[derive(Clone, Debug)]
pub struct Ticker {
    pub last_price: f64,
    pub price_change_percent_24h: f64,
    pub high_24h: f64,
    pub low_24h: f64,
    pub volume_24h: f64,
}

impl WatchlistState {
    pub fn new() -> Self {
        Self {
            symbols: Vec::new(),
            tickers: HashMap::new(),
            sort: (WatchlistColumn::Symbol, true),
            selected: None,
        }
    }

    /// Column headers for table rendering
    pub fn column_headers(&self) -> Vec<&'static str> {
        vec!["Symbol", "Last", "Change %", "High 24h", "Low 24h", "Volume 24h"]
    }

    /// Calculate proportional column widths
    pub fn column_widths(&self, total_width: f32) -> Vec<f32> {
        let widths = [0.15, 0.15, 0.15, 0.15, 0.15, 0.25]; // proportions
        widths.iter().map(|w| w * total_width).collect()
    }

    /// Get visible rows for rendering with scroll support
    pub fn visible_rows(&self, scroll_offset: usize, max_rows: usize) -> Vec<&str> {
        self.symbols
            .iter()
            .skip(scroll_offset)
            .take(max_rows)
            .map(|s| s.as_str())
            .collect()
    }

    /// Format cell value for rendering
    pub fn format_cell(&self, symbol: &str, column: WatchlistColumn) -> String {
        let ticker = match self.tickers.get(symbol) {
            Some(t) => t,
            None => return "—".to_string(),
        };

        match column {
            WatchlistColumn::Symbol => symbol.to_string(),
            WatchlistColumn::Last => format!("{:.2}", ticker.last_price),
            WatchlistColumn::ChangePercent => format!("{:+.2}%", ticker.price_change_percent_24h),
            WatchlistColumn::High24h => format!("{:.2}", ticker.high_24h),
            WatchlistColumn::Low24h => format!("{:.2}", ticker.low_24h),
            WatchlistColumn::Volume24h => format!("{:.2}", ticker.volume_24h),
        }
    }

    /// Get color for row based on price change
    pub fn row_color(&self, symbol: &str) -> [f32; 4] {
        if let Some(ticker) = self.tickers.get(symbol) {
            if ticker.price_change_percent_24h > 0.0 {
                [0.2, 0.8, 0.3, 1.0] // green
            } else if ticker.price_change_percent_24h < 0.0 {
                [0.9, 0.2, 0.2, 1.0] // red
            } else {
                [0.6, 0.6, 0.7, 1.0] // neutral
            }
        } else {
            [0.6, 0.6, 0.7, 1.0] // neutral
        }
    }
}

/// Watchlist panel configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WatchlistConfig {
    /// User-configured symbol list
    pub symbols: Vec<String>,
    /// Auto-refresh interval in seconds
    pub refresh_interval: u64,
    /// Show volume in quote asset
    pub show_quote_volume: bool,
    /// Color negative changes
    pub color_negative: bool,
}

/// Watchlist panel wrapper (lightweight, lives in PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WatchlistPanel {
    id: WatchlistId,
    title: String,
}

impl WatchlistPanel {
    pub fn new(id: WatchlistId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> WatchlistId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "watchlist" }
    pub fn kind_label(&self) -> &'static str { "Watchlist" }
    pub fn min_size(&self) -> (f32, f32) { (200.0, 150.0) }
}
