use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RiskMetricsId(pub u64);

#[derive(Clone, Debug)]
pub struct RiskMetricsState {
    /// Portfolio positions
    pub positions: Vec<Position>,
    /// Historical returns
    pub returns: Vec<f64>,
    /// Risk metrics
    pub metrics: Option<RiskMetrics>,
    /// Time range for calculation
    pub time_range: TimeRange,
}

#[derive(Clone, Debug)]
pub struct Position {
    pub symbol: String,
    pub quantity: f64,
    pub value: f64,
}

#[derive(Clone, Debug)]
pub struct RiskMetrics {
    pub var_95: f64,
    pub var_99: f64,
    pub cvar_95: f64,
    pub cvar_99: f64,
    pub beta: f64,
    pub alpha: f64,
    pub correlation: f64,
    pub max_drawdown: f64,
    pub sharpe_ratio: f64,
    pub sortino_ratio: f64,
    pub calmar_ratio: f64,
    pub volatility: f64,
}

#[derive(Clone, Debug)]
pub enum TimeRange {
    Week,
    Month,
    Quarter,
    Year,
}

impl RiskMetricsState {
    pub fn new() -> Self {
        Self {
            positions: Vec::new(),
            returns: Vec::new(),
            metrics: None,
            time_range: TimeRange::Month,
        }
    }

    /// Get list of metrics as key-value pairs for rendering
    pub fn metrics_rows(&self) -> Vec<(&'static str, String)> {
        if let Some(ref m) = self.metrics {
            vec![
                ("VaR (95%)", format!("{:.2}%", m.var_95 * 100.0)),
                ("VaR (99%)", format!("{:.2}%", m.var_99 * 100.0)),
                ("CVaR (95%)", format!("{:.2}%", m.cvar_95 * 100.0)),
                ("CVaR (99%)", format!("{:.2}%", m.cvar_99 * 100.0)),
                ("Beta", format!("{:.3}", m.beta)),
                ("Alpha", format!("{:.3}%", m.alpha * 100.0)),
                ("Correlation", format!("{:.3}", m.correlation)),
                ("Max Drawdown", format!("{:.2}%", m.max_drawdown * 100.0)),
                ("Sharpe Ratio", format!("{:.3}", m.sharpe_ratio)),
                ("Sortino Ratio", format!("{:.3}", m.sortino_ratio)),
                ("Calmar Ratio", format!("{:.3}", m.calmar_ratio)),
                ("Volatility", format!("{:.2}%", m.volatility * 100.0)),
            ]
        } else {
            vec![("Status", "Calculating...".to_string())]
        }
    }

    /// Get color for a specific metric based on its value
    pub fn metric_color(&self, key: &str) -> [f32; 4] {
        if let Some(ref m) = self.metrics {
            match key {
                "Sharpe Ratio" | "Sortino Ratio" | "Calmar Ratio" => {
                    let value = match key {
                        "Sharpe Ratio" => m.sharpe_ratio,
                        "Sortino Ratio" => m.sortino_ratio,
                        "Calmar Ratio" => m.calmar_ratio,
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
                    if m.max_drawdown.abs() > 0.2 {
                        [0.9, 0.2, 0.2, 1.0] // red - large drawdown
                    } else if m.max_drawdown.abs() > 0.1 {
                        [0.9, 0.7, 0.2, 1.0] // yellow - moderate
                    } else {
                        [0.2, 0.8, 0.3, 1.0] // green - small
                    }
                }
                "Alpha" => {
                    if m.alpha > 0.0 {
                        [0.2, 0.8, 0.3, 1.0] // green - positive alpha
                    } else {
                        [0.9, 0.2, 0.2, 1.0] // red - negative alpha
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
pub struct RiskMetricsConfig {
    /// Benchmark for beta/correlation
    pub benchmark: String,
    /// Confidence level for VaR
    pub var_confidence: f64,
    /// Risk-free rate
    pub risk_free_rate: f64,
    /// Calculation window (days)
    pub window: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RiskMetricsPanel {
    id: RiskMetricsId,
    title: String,
}

impl RiskMetricsPanel {
    pub fn new(id: RiskMetricsId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> RiskMetricsId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "risk_metrics"
    }

    pub fn kind_label(&self) -> &'static str {
        "Risk Metrics"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (250.0, 200.0)
    }
}
