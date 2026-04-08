use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StatisticsId(pub u64);

#[derive(Clone, Debug)]
pub struct StatisticsState {
    /// Data source (e.g., returns, PnL, prices)
    pub data_source: StatDataSource,
    /// Time range for analysis
    pub time_range: TimeRange,
    /// Calculated statistics
    pub stats: Option<StatisticsData>,
}

#[derive(Clone, Debug)]
pub enum StatDataSource {
    Returns,
    PnL,
    Prices,
    Volume,
    Custom(Vec<f64>),
}

#[derive(Clone, Debug)]
pub enum TimeRange {
    Week,
    Month,
    Quarter,
    Year,
}

#[derive(Clone, Debug)]
pub struct StatisticsData {
    pub count: usize,
    pub mean: f64,
    pub median: f64,
    pub std_dev: f64,
    pub variance: f64,
    pub skewness: f64,
    pub kurtosis: f64,
    pub min: f64,
    pub max: f64,
    pub range: f64,
    pub sharpe_ratio: Option<f64>,
    pub sortino_ratio: Option<f64>,
    pub max_drawdown: Option<f64>,
    pub win_rate: Option<f64>,
}

impl StatisticsState {
    pub fn new() -> Self {
        Self {
            data_source: StatDataSource::Returns,
            time_range: TimeRange::Month,
            stats: None,
        }
    }

    /// Get list of statistics as key-value pairs for rendering
    pub fn stats_rows(&self) -> Vec<(&'static str, String)> {
        if let Some(ref s) = self.stats {
            let mut rows = vec![
                ("Count", format!("{}", s.count)),
                ("Mean", format!("{:.4}", s.mean)),
                ("Median", format!("{:.4}", s.median)),
                ("Std Dev", format!("{:.4}", s.std_dev)),
                ("Variance", format!("{:.4}", s.variance)),
                ("Min", format!("{:.4}", s.min)),
                ("Max", format!("{:.4}", s.max)),
                ("Range", format!("{:.4}", s.range)),
            ];

            // Add advanced stats if available
            if let Some(sharpe) = s.sharpe_ratio {
                rows.push(("Sharpe Ratio", format!("{:.3}", sharpe)));
            }
            if let Some(sortino) = s.sortino_ratio {
                rows.push(("Sortino Ratio", format!("{:.3}", sortino)));
            }
            if let Some(dd) = s.max_drawdown {
                rows.push(("Max Drawdown", format!("{:.2}%", dd * 100.0)));
            }
            if let Some(wr) = s.win_rate {
                rows.push(("Win Rate", format!("{:.1}%", wr * 100.0)));
            }

            rows
        } else {
            vec![("Status", "No data".to_string())]
        }
    }

    /// Get color for a specific statistic based on its value
    pub fn stat_color(&self, key: &str) -> [f32; 4] {
        if let Some(ref s) = self.stats {
            match key {
                "Sharpe Ratio" | "Sortino Ratio" => {
                    let value = match key {
                        "Sharpe Ratio" => s.sharpe_ratio.unwrap_or(0.0),
                        "Sortino Ratio" => s.sortino_ratio.unwrap_or(0.0),
                        _ => 0.0,
                    };
                    if value > 1.0 {
                        [0.2, 0.8, 0.3, 1.0] // green - good
                    } else if value > 0.0 {
                        [0.9, 0.7, 0.2, 1.0] // yellow - okay
                    } else {
                        [0.9, 0.2, 0.2, 1.0] // red - poor
                    }
                }
                "Max Drawdown" => {
                    let dd = s.max_drawdown.unwrap_or(0.0).abs();
                    if dd > 0.2 {
                        [0.9, 0.2, 0.2, 1.0] // red - large drawdown
                    } else if dd > 0.1 {
                        [0.9, 0.7, 0.2, 1.0] // yellow - moderate
                    } else {
                        [0.2, 0.8, 0.3, 1.0] // green - small
                    }
                }
                "Win Rate" => {
                    let wr = s.win_rate.unwrap_or(0.0);
                    if wr > 0.6 {
                        [0.2, 0.8, 0.3, 1.0] // green - high win rate
                    } else if wr > 0.4 {
                        [0.9, 0.7, 0.2, 1.0] // yellow - moderate
                    } else {
                        [0.9, 0.2, 0.2, 1.0] // red - low
                    }
                }
                _ => [0.6, 0.6, 0.7, 1.0], // neutral
            }
        } else {
            [0.6, 0.6, 0.7, 1.0]
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatisticsConfig {
    /// Show advanced statistics
    pub show_advanced: bool,
    /// Risk-free rate for Sharpe/Sortino
    pub risk_free_rate: f64,
    /// Calculation precision
    pub decimal_places: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatisticsPanel {
    id: StatisticsId,
    title: String,
}

impl StatisticsPanel {
    pub fn new(id: StatisticsId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> StatisticsId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "statistics"
    }

    pub fn kind_label(&self) -> &'static str {
        "Statistics"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (250.0, 200.0)
    }
}
