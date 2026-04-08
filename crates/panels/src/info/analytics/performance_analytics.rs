use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PerformanceAnalyticsId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct PerformanceAnalyticsState {
    pub equity_curve: Vec<(i64, f64)>,  // (timestamp, cumulative_equity)
    pub drawdown_curve: Vec<(i64, f64)>,  // (timestamp, drawdown_%)
    pub returns_distribution: Vec<(f64, u32)>,  // (return_%, frequency)
    pub metrics: PerformanceMetrics,
}

#[derive(Clone, Debug, Default)]
pub struct PerformanceMetrics {
    pub total_return_pct: f64,
    pub max_drawdown_pct: f64,
    pub sharpe_ratio: f64,
    pub sortino_ratio: f64,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub calmar_ratio: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PerformanceAnalyticsConfig {
    pub layout: LayoutMode,  // ThreeRow, TwoColumn
    pub equity_color: [f32; 3],  // OKLCH
    pub drawdown_color: [f32; 3],  // OKLCH (typically red)
    pub returns_color: [f32; 3],  // OKLCH
    pub show_benchmarks: bool,  // Compare vs S&P500, etc.
    pub benchmark_symbols: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum LayoutMode {
    ThreeRow,   // Equity | Drawdown | Returns (stacked vertically)
    TwoColumn,  // (Equity + Drawdown) | Returns
}

impl PerformanceAnalyticsState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns equity curve points in screen coordinates
    pub fn equity_points(&self, w: f32, h: f32) -> Vec<(f32, f32)> {
        if self.equity_curve.is_empty() {
            return Vec::new();
        }

        let (min_time, max_time) = self.equity_curve.iter()
            .fold((i64::MAX, i64::MIN), |(min, max), (t, _)| {
                (min.min(*t), max.max(*t))
            });

        let (min_equity, max_equity) = self.equity_curve.iter()
            .fold((f64::MAX, f64::MIN), |(min, max), (_, e)| {
                (min.min(*e), max.max(*e))
            });

        let time_range = (max_time - min_time) as f64;
        let equity_range = max_equity - min_equity;

        if time_range == 0.0 || equity_range == 0.0 {
            return Vec::new();
        }

        self.equity_curve.iter()
            .map(|(time, equity)| {
                let x = (((*time - min_time) as f64) / time_range) as f32 * w;
                let y = ((1.0 - (equity - min_equity) / equity_range) * h as f64) as f32;
                (x.clamp(0.0, w), y.clamp(0.0, h))
            })
            .collect()
    }

    /// Returns drawdown curve points in screen coordinates
    pub fn drawdown_points(&self, w: f32, h: f32) -> Vec<(f32, f32)> {
        if self.drawdown_curve.is_empty() {
            return Vec::new();
        }

        let (min_time, max_time) = self.drawdown_curve.iter()
            .fold((i64::MAX, i64::MIN), |(min, max), (t, _)| {
                (min.min(*t), max.max(*t))
            });

        let max_dd = self.drawdown_curve.iter()
            .map(|(_, dd)| dd.abs())
            .fold(0.0f64, f64::max);

        let time_range = (max_time - min_time) as f64;

        if time_range == 0.0 || max_dd == 0.0 {
            return Vec::new();
        }

        self.drawdown_curve.iter()
            .map(|(time, dd)| {
                let x = (((*time - min_time) as f64) / time_range) as f32 * w;
                let y = (dd.abs() / max_dd * h as f64) as f32;
                (x.clamp(0.0, w), y.clamp(0.0, h))
            })
            .collect()
    }

    /// Returns list of metrics as key-value pairs for rendering
    pub fn metrics_list(&self) -> Vec<(&'static str, String)> {
        vec![
            ("Total Return", format!("{:.2}%", self.metrics.total_return_pct)),
            ("Max Drawdown", format!("{:.2}%", self.metrics.max_drawdown_pct)),
            ("Sharpe Ratio", format!("{:.3}", self.metrics.sharpe_ratio)),
            ("Sortino Ratio", format!("{:.3}", self.metrics.sortino_ratio)),
            ("Win Rate", format!("{:.1}%", self.metrics.win_rate * 100.0)),
            ("Profit Factor", format!("{:.2}", self.metrics.profit_factor)),
            ("Calmar Ratio", format!("{:.3}", self.metrics.calmar_ratio)),
        ]
    }

    /// Format a metric value for display
    pub fn format_metric(&self, key: &str) -> String {
        match key {
            "Total Return" => format!("{:.2}%", self.metrics.total_return_pct),
            "Max Drawdown" => format!("{:.2}%", self.metrics.max_drawdown_pct),
            "Sharpe Ratio" => format!("{:.3}", self.metrics.sharpe_ratio),
            "Sortino Ratio" => format!("{:.3}", self.metrics.sortino_ratio),
            "Win Rate" => format!("{:.1}%", self.metrics.win_rate * 100.0),
            "Profit Factor" => format!("{:.2}", self.metrics.profit_factor),
            "Calmar Ratio" => format!("{:.3}", self.metrics.calmar_ratio),
            _ => "—".to_string(),
        }
    }
}

impl Default for PerformanceAnalyticsConfig {
    fn default() -> Self {
        Self {
            layout: LayoutMode::ThreeRow,
            equity_color: [0.65, 0.15, 145.0],   // Green
            drawdown_color: [0.60, 0.18, 25.0],  // Red
            returns_color: [0.6, 0.2, 240.0],    // Blue
            show_benchmarks: false,
            benchmark_symbols: vec![],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PerformanceAnalyticsPanel {
    id: PerformanceAnalyticsId,
    title: String,
}

impl PerformanceAnalyticsPanel {
    pub fn new(id: PerformanceAnalyticsId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> PerformanceAnalyticsId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "performance_analytics"
    }

    pub fn kind_label(&self) -> &'static str {
        "Performance"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 250.0)
    }
}
