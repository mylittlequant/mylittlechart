use serde::{Serialize, Deserialize};
use std::collections::{HashMap, VecDeque};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PairsTradingId(pub u64);

#[derive(Clone, Debug)]
pub struct PairsTradingState {
    /// Tracked pairs
    pub pairs: Vec<TradingPair>,
    /// Selected pair
    pub selected_pair: Option<usize>,
    /// Historical spread data
    pub spread_history: HashMap<String, VecDeque<(i64, f64)>>,
}

#[derive(Clone, Debug)]
pub struct TradingPair {
    pub id: String,
    pub symbol_a: String,
    pub symbol_b: String,
    pub current_spread: f64,
    pub z_score: f64,
    pub half_life: f64,
    pub correlation: f64,
    pub hedge_ratio: f64,
    pub signal: PairSignal,
    pub position_a: Option<f64>,
    pub position_b: Option<f64>,
}

#[derive(Clone, Debug)]
pub enum PairSignal {
    Buy,
    Sell,
    Neutral,
    Extreme,
}

impl PairsTradingState {
    pub fn new() -> Self {
        Self {
            pairs: Vec::new(),
            selected_pair: None,
            spread_history: HashMap::new(),
        }
    }

    /// Get visible pairs for rendering
    pub fn visible_pairs(&self, scroll_offset: usize, max_rows: usize) -> &[TradingPair] {
        let end = (scroll_offset + max_rows).min(self.pairs.len());
        &self.pairs[scroll_offset..end]
    }

    /// Format pair for display
    pub fn format_pair(&self, pair: &TradingPair) -> (String, String, String, String, String) {
        let symbols = format!("{}/{}", pair.symbol_a, pair.symbol_b);
        let z_score = format!("{:.2}", pair.z_score);
        let spread = format!("{:.4}", pair.current_spread);
        let correlation = format!("{:.3}", pair.correlation);
        let signal = match pair.signal {
            PairSignal::Buy => "BUY",
            PairSignal::Sell => "SELL",
            PairSignal::Neutral => "NEUTRAL",
            PairSignal::Extreme => "EXTREME",
        };
        (symbols, z_score, spread, correlation, signal.to_string())
    }

    /// Get color based on pair signal
    pub fn signal_color(&self, pair: &TradingPair) -> [f32; 4] {
        match pair.signal {
            PairSignal::Buy => [0.2, 0.8, 0.3, 1.0],      // green
            PairSignal::Sell => [0.9, 0.2, 0.2, 1.0],     // red
            PairSignal::Neutral => [0.6, 0.6, 0.7, 1.0],  // neutral
            PairSignal::Extreme => [0.9, 0.5, 0.2, 1.0],  // orange
        }
    }

    /// Get color intensity based on z-score magnitude
    pub fn zscore_color(&self, pair: &TradingPair) -> [f32; 4] {
        let abs_z = pair.z_score.abs();
        if abs_z > 3.0 {
            [0.9, 0.2, 0.2, 1.0] // red - extreme
        } else if abs_z > 2.0 {
            [0.9, 0.7, 0.2, 1.0] // yellow - significant
        } else if abs_z > 1.0 {
            [0.2, 0.8, 0.3, 1.0] // green - opportunity
        } else {
            [0.6, 0.6, 0.7, 1.0] // neutral
        }
    }

    /// Get spread history for selected pair
    pub fn selected_spread_history(&self) -> Option<&VecDeque<(i64, f64)>> {
        if let Some(idx) = self.selected_pair {
            if let Some(pair) = self.pairs.get(idx) {
                return self.spread_history.get(&pair.id);
            }
        }
        None
    }

    /// Format pair metrics for detail view
    pub fn format_pair_metrics(&self, pair: &TradingPair) -> Vec<(&'static str, String)> {
        vec![
            ("Z-Score", format!("{:.3}", pair.z_score)),
            ("Spread", format!("{:.4}", pair.current_spread)),
            ("Correlation", format!("{:.3}", pair.correlation)),
            ("Hedge Ratio", format!("{:.4}", pair.hedge_ratio)),
            ("Half Life", format!("{:.1} days", pair.half_life)),
            ("Position A", pair.position_a.map(|p| format!("{:.4}", p)).unwrap_or_else(|| "—".to_string())),
            ("Position B", pair.position_b.map(|p| format!("{:.4}", p)).unwrap_or_else(|| "—".to_string())),
        ]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PairsTradingConfig {
    /// Z-score entry threshold
    pub entry_threshold: f64,
    /// Z-score exit threshold
    pub exit_threshold: f64,
    /// Lookback period for spread calculation
    pub lookback_period: usize,
    /// Min correlation for pair validity
    pub min_correlation: f64,
    /// Show spread chart
    pub show_spread_chart: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PairsTradingPanel {
    id: PairsTradingId,
    title: String,
}

impl PairsTradingPanel {
    pub fn new(id: PairsTradingId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> PairsTradingId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "pairs_trading"
    }

    pub fn kind_label(&self) -> &'static str {
        "Pairs Trading"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 200.0)
    }
}
